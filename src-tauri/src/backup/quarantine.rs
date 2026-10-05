//! quarantine — Windows 비호환 파일의 tar 격리 (plan.md §6-3).
//! 비호환 이름은 Windows 파일시스템에 절대 쓰지 않는다: 안전한 임시 이름으로 받아
//! tar 세그먼트에 원본 유닉스 경로·mtime·크기로 옮긴 뒤 임시 파일을 즉시 삭제.
//! 복원은 기기 측 스트리밍(restore.rs) — Windows를 거치지 않고 원본 이름 복구.

use crate::backup::model::FileEntry;
use sha2::{Digest, Sha256};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// 세그먼트 하나의 크기 상한 — 큰 파일 여러 개를 한 아카이브에 무한정 쌓지 않는다
const SEGMENT_LIMIT: u64 = 512 * 1024 * 1024;

pub struct Quarantine {
    dir: PathBuf,
    builder: Option<tar::Builder<std::fs::File>>,
    segment: usize,
    /// 이번 실행에서 만든 세그먼트 수(이어서 백업 시 기존 번호와 구분)
    created: usize,
    written: u64,
}

impl Quarantine {
    pub fn new(backup_root: &Path) -> Result<Self, String> {
        let target = super::paths::write_target(backup_root, "quarantine/seg000.tar")?;
        let dir = target.parent().expect("quarantine 부모").to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| {
            format!(
                "quarantine 폴더 생성 실패: {}",
                crate::storage::io_error(&e)
            )
        })?;
        // 이어서 백업할 때 이전 실행의 세그먼트(완료된 항목의 격리 파일)를 덮어쓰지 않도록 다음 번호부터
        let mut segment = 0;
        for entry in std::fs::read_dir(&dir).map_err(|e| {
            format!(
                "quarantine 목록 읽기 실패: {}",
                crate::storage::io_error(&e)
            )
        })? {
            let name = entry
                .map_err(|e| e.to_string())?
                .file_name()
                .to_string_lossy()
                .to_string();
            // 이전 실행이 강제 종료되며 남긴 임시 파일 — 원본 이름이 없는 생성명이라 지워도 안전
            if name.starts_with(".qtmp-") {
                let _ = std::fs::remove_file(dir.join(&name));
                continue;
            }
            if let Some(number) = name
                .strip_prefix("seg")
                .and_then(|n| n.strip_suffix(".tar"))
                .and_then(|n| n.parse::<usize>().ok())
            {
                segment = segment.max(
                    number
                        .checked_add(1)
                        .ok_or("quarantine 세그먼트 번호가 너무 큽니다")?,
                );
            }
        }
        Ok(Self {
            dir,
            builder: None,
            segment,
            created: 0,
            written: 0,
        })
    }

    fn seg_path(&self) -> PathBuf {
        self.dir.join(format!("seg{:03}.tar", self.segment))
    }

    /// 새 세그먼트를 연다(필요할 때)
    fn ensure_open(&mut self) -> Result<&mut tar::Builder<std::fs::File>, String> {
        if self.builder.is_none() {
            let f = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(self.seg_path())
                .map_err(|e| {
                    format!(
                        "quarantine 세그먼트 생성 실패: {}",
                        crate::storage::io_error(&e)
                    )
                })?;
            self.builder = Some(tar::Builder::new(f));
            self.written = 0;
            self.created += 1;
        }
        Ok(self.builder.as_mut().expect("방금 열었음"))
    }

    /// 현재 세그먼트를 닫아 확정
    fn rotate(&mut self) -> Result<(), String> {
        if let Some(b) = self.builder.take() {
            b.into_inner()
                .map_err(|e| {
                    format!(
                        "quarantine 세그먼트 종료 실패: {}",
                        crate::storage::io_error(&e)
                    )
                })?
                .sync_all()
                .map_err(|e| {
                    format!(
                        "quarantine 디스크 저장 실패: {}",
                        crate::storage::io_error(&e)
                    )
                })?;
            self.segment += 1;
        }
        Ok(())
    }

    /// manifest 저장 전에 tar 끝 표시와 내용을 디스크에 확정한다.
    pub fn checkpoint(&mut self) -> Result<(), String> {
        self.rotate()
    }

    /// 임시 파일(안전한 이름)에 받아 둔 내용을 원본 이름의 tar 항목으로 추가.
    /// 쓰다가 실패하면 그 항목을 잘라내고 세그먼트를 닫는다 — 다음 항목이 어긋난 위치에 붙어
    /// 세그먼트 전체를 읽을 수 없게 되는 것을 막는다.
    pub fn add(&mut self, remote: &str, size: u64, mtime: u32, tmp: &Path) -> Result<(), String> {
        // 세그먼트 크기 관리 — 이 파일을 넣어 한도를 넘으면 먼저 닫고 다음 세그먼트
        if self.written + size > SEGMENT_LIMIT {
            self.rotate()?;
        }
        let data = std::fs::File::open(tmp).map_err(|e| e.to_string())?;
        let builder = self.ensure_open()?;
        let start = builder.get_mut().stream_position().map_err(|e| {
            format!(
                "quarantine 위치 확인 실패: {}",
                crate::storage::io_error(&e)
            )
        })?;
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_mode(0o644);
        header.set_mtime(mtime as u64);
        // 유닉스 절대경로 그대로 — 복원 시 tar -xf가 원 위치에 푼다(§6-3)
        let name = remote.trim_start_matches('/');
        if let Err(e) = append_unix_path(builder, &mut header, name, ExactReader::new(data, size)) {
            let rollback = self.truncate_and_close(start);
            return Err(match rollback {
                Ok(()) => format!(
                    "quarantine 항목 추가 실패({remote}): {}",
                    crate::storage::io_error(&e)
                ),
                Err(r) => format!(
                    "quarantine 항목 추가 실패({remote}): {} (정리 실패: {r})",
                    crate::storage::io_error(&e)
                ),
            });
        }
        self.written += size;
        Ok(())
    }

    /// 실패한 항목 시작 위치로 잘라내고 세그먼트를 닫는다(끝 표시는 잘라낸 위치에 기록)
    fn truncate_and_close(&mut self, start: u64) -> Result<(), String> {
        if let Some(builder) = self.builder.as_mut() {
            let file = builder.get_mut();
            file.set_len(start).map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(start))
                .map_err(|e| e.to_string())?;
        }
        self.rotate()
    }

    /// 열려 있던 세그먼트를 닫아 마무리 — 백업 종료 시 반드시 호출
    pub fn finish(mut self) -> Result<usize, String> {
        self.rotate()?;
        Ok(self.created)
    }
}

/// tar 항목을 유닉스 경로 바이트 그대로 추가한다.
/// `tar::Builder::append_data`는 Windows에서 이름 안의 `\`를 `/`로 바꿔 다른 경로를 만들므로
/// 헤더 이름 필드를 직접 채운다(100바이트 초과는 GNU 긴 이름 항목 — tar 크레이트와 같은 형식).
pub(super) fn append_unix_path<W: Write>(
    builder: &mut tar::Builder<W>,
    header: &mut tar::Header,
    name: &str,
    data: impl Read,
) -> std::io::Result<()> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.contains(&0) {
        return Err(std::io::Error::other("tar 항목 이름이 올바르지 않습니다"));
    }
    if bytes.len() > 100 {
        let mut long = tar::Header::new_gnu();
        let link = b"././@LongLink";
        long.as_old_mut().name[..link.len()].copy_from_slice(link);
        long.set_mode(0o644);
        long.set_mtime(0);
        long.set_entry_type(tar::EntryType::GNULongName);
        long.set_size(bytes.len() as u64 + 1);
        long.set_cksum();
        let mut payload = bytes.to_vec();
        payload.push(0);
        builder.append(&long, payload.as_slice())?;
    }
    let field = &mut header.as_old_mut().name;
    *field = [0; 100];
    let n = bytes.len().min(100);
    field[..n].copy_from_slice(&bytes[..n]);
    header.set_cksum();
    builder.append(header, data)
}

/// 헤더에 적은 크기만큼 정확히 읽는다 — 파일이 그보다 짧으면 오류(tar는 길이를 확인하지 않아
/// 짧은 본문이 아카이브를 어긋나게 만든다). 길면 나머지는 읽지 않는다.
pub(super) struct ExactReader<R: Read> {
    inner: std::io::Take<R>,
}

impl<R: Read> ExactReader<R> {
    pub fn new(inner: R, size: u64) -> Self {
        Self {
            inner: inner.take(size),
        }
    }
}

impl<R: Read> Read for ExactReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(buf)?;
        if n == 0 && !buf.is_empty() && self.inner.limit() > 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "파일이 기록된 크기보다 짧습니다",
            ));
        }
        Ok(n)
    }
}

/// pull 결과를 담는 스트림 래퍼 — 쓰면서 sha256·바이트 수를 센다(§6-1 비용≈0)
pub struct HashingWriter<W: Write> {
    inner: W,
    hasher: Sha256,
    pub bytes: u64,
}

impl<W: Write> HashingWriter<W> {
    pub fn new(inner: W) -> Self {
        Self {
            inner,
            hasher: Sha256::new(),
            bytes: 0,
        }
    }
    pub fn finish(self) -> (W, String, u64) {
        let digest = self.hasher.finalize();
        (self.inner, hex::encode(digest), self.bytes)
    }
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        Digest::update(&mut self.hasher, &buf[..n]);
        self.bytes += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

/// 비호환 파일용 임시 파일 경로 — 원본 이름이 Windows에 절대 나타나지 않게 생성명 사용
pub fn quarantine_tmp_path(backup_root: &Path, nonce: u64) -> PathBuf {
    backup_root
        .join("quarantine")
        .join(format!(".qtmp-{nonce}"))
}

/// FileEntry를 quarantine 기록으로(성공 시)
pub fn quarantined_entry(remote: &str, size: u64, mtime: u32, sha256: &str) -> FileEntry {
    FileEntry {
        remote: remote.to_string(),
        local: String::new(),
        size,
        mtime,
        sha256: Some(sha256.to_string()),
        quarantined: true,
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn hashing_writer_counts_and_hashes() {
        let mut w = HashingWriter::new(Vec::new());
        w.write_all(b"hello ").unwrap();
        w.write_all(b"world").unwrap();
        let (buf, hash, n) = w.finish();
        assert_eq!(buf, b"hello world");
        assert_eq!(n, 11);
        // sha256("hello world") 표준 검증 벡터
        assert_eq!(
            hash,
            "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9"
        );
    }

    #[test]
    fn quarantine_segments_and_keeps_unix_names() {
        let dir = tempfile::tempdir().unwrap();
        let mut q = Quarantine::new(dir.path()).unwrap();
        // 임시 파일 준비
        let tmp = dir.path().join(".qtmp-1");
        std::fs::write(&tmp, b"data-bytes").unwrap();
        q.add("/sdcard/x/con:.jpg", 10, 1700000000, &tmp).unwrap();
        std::fs::remove_file(&tmp).unwrap();
        let segments = q.finish().unwrap();
        assert_eq!(segments, 1);
        // tar를 다시 읽어 원본 이름·내용·mtime 확인
        let f = std::fs::File::open(dir.path().join("quarantine/seg000.tar")).unwrap();
        let mut ar = tar::Archive::new(f);
        let mut entries = ar.entries().unwrap();
        let e = entries.next().unwrap().unwrap();
        assert_eq!(e.path().unwrap().to_str().unwrap(), "sdcard/x/con:.jpg");
        assert_eq!(e.header().size().unwrap(), 10);
        assert_eq!(e.header().mtime().unwrap(), 1700000000);
        let mut content = String::new();
        {
            let f2 = std::fs::File::open(dir.path().join("quarantine/seg000.tar")).unwrap();
            let mut ar2 = tar::Archive::new(f2);
            let mut entry = ar2.entries().unwrap().next().unwrap().unwrap();
            entry.read_to_string(&mut content).unwrap();
        }
        assert_eq!(content, "data-bytes");
    }

    #[test]
    fn failed_append_is_cut_off_and_later_entries_stay_readable() {
        let dir = tempfile::tempdir().unwrap();
        let mut q = Quarantine::new(dir.path()).unwrap();
        let good = dir.path().join(".qtmp-a");
        std::fs::write(&good, b"good").unwrap();
        let short = dir.path().join(".qtmp-b");
        std::fs::write(&short, b"x").unwrap();
        q.add("/sdcard/a?.jpg", 4, 0, &good).unwrap();
        // 기록 크기(10)보다 짧은 파일 — 실패하고, 쓰다 만 항목은 잘려야 한다
        assert!(q.add("/sdcard/b?.jpg", 10, 0, &short).is_err());
        q.add("/sdcard/c\\d.jpg", 4, 0, &good).unwrap();
        assert_eq!(q.finish().unwrap(), 2);
        let mut names = vec![];
        for seg in ["seg000.tar", "seg001.tar"] {
            let f = std::fs::File::open(dir.path().join("quarantine").join(seg)).unwrap();
            let mut ar = tar::Archive::new(f);
            for e in ar.entries().unwrap() {
                let e = e.unwrap();
                names.push(String::from_utf8_lossy(&e.path_bytes()).to_string());
            }
        }
        assert_eq!(names, vec!["sdcard/a?.jpg", "sdcard/c\\d.jpg"]);
    }

    #[test]
    fn leftover_temporary_files_are_removed_on_start() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("quarantine")).unwrap();
        let stale = dir.path().join("quarantine/.qtmp-7");
        std::fs::write(&stale, b"private").unwrap();
        Quarantine::new(dir.path()).unwrap();
        assert!(!stale.exists());
    }
}
