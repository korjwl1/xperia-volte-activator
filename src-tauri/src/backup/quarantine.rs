//! quarantine — Windows 비호환 파일의 tar 격리 (plan.md §6-3).
//! 비호환 이름은 Windows 파일시스템에 절대 쓰지 않는다: 안전한 임시 이름으로 받아
//! tar 세그먼트에 원본 유닉스 경로·mtime·크기로 옮긴 뒤 임시 파일을 즉시 삭제.
//! 복원은 기기 측 스트리밍(restore.rs) — Windows를 거치지 않고 원본 이름 복구.

use crate::backup::model::FileEntry;
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// 세그먼트 하나의 크기 상한 — 큰 파일 여러 개를 한 아카이브에 무한정 쌓지 않는다
const SEGMENT_LIMIT: u64 = 512 * 1024 * 1024;

pub struct Quarantine {
    dir: PathBuf,
    builder: Option<tar::Builder<std::fs::File>>,
    segment: usize,
    written: u64,
}

impl Quarantine {
    pub fn new(backup_root: &Path) -> Result<Self, String> {
        let target = super::paths::write_target(backup_root, "quarantine/seg000.tar")?;
        let dir = target.parent().expect("quarantine 부모").to_path_buf();
        std::fs::create_dir_all(&dir).map_err(|e| format!("quarantine 폴더 생성 실패: {e}"))?;
        // 이어서 백업할 때 이전 실행의 세그먼트(완료된 항목의 격리 파일)를 덮어쓰지 않도록 다음 번호부터
        let mut segment = 0;
        for entry in
            std::fs::read_dir(&dir).map_err(|e| format!("quarantine 목록 읽기 실패: {e}"))?
        {
            let name = entry
                .map_err(|e| e.to_string())?
                .file_name()
                .to_string_lossy()
                .to_string();
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
                .map_err(|e| format!("quarantine 세그먼트 생성 실패: {e}"))?;
            self.builder = Some(tar::Builder::new(f));
            self.written = 0;
        }
        Ok(self.builder.as_mut().expect("방금 열었음"))
    }

    /// 현재 세그먼트를 닫아 확정
    fn rotate(&mut self) -> Result<(), String> {
        if let Some(b) = self.builder.take() {
            b.into_inner()
                .map_err(|e| format!("quarantine 세그먼트 종료 실패: {e}"))?
                .sync_all()
                .map_err(|e| format!("quarantine 디스크 저장 실패: {e}"))?;
            self.segment += 1;
        }
        Ok(())
    }

    /// 임시 파일(안전한 이름)에 받아 둔 내용을 원본 이름의 tar 항목으로 추가
    pub fn add(
        &mut self,
        remote: &str,
        size: u64,
        mtime: u32,
        tmp: &Path,
        _sha256: &str,
    ) -> Result<(), String> {
        // 세그먼트 크기 관리 — 이 파일을 넣어 한도를 넘으면 먼저 닫고 다음 세그먼트
        if self.written + size > SEGMENT_LIMIT {
            self.rotate()?;
        }
        let builder = self.ensure_open()?;
        let mut header = tar::Header::new_gnu();
        header.set_size(size);
        header.set_mode(0o644);
        header.set_mtime(mtime as u64);
        header.set_cksum();
        // 유닉스 절대경로 그대로 — 복원 시 tar -xf가 원 위치에 푼다(§6-3)
        let name = remote.trim_start_matches('/');
        builder
            .append_data(
                &mut header,
                name,
                std::fs::File::open(tmp).map_err(|e| e.to_string())?,
            )
            .map_err(|e| format!("quarantine 항목 추가 실패({remote}): {e}"))?;
        self.written += size;
        Ok(())
    }

    /// 열려 있던 세그먼트를 닫아 마무리 — 백업 종료 시 반드시 호출
    pub fn finish(mut self) -> Result<usize, String> {
        self.rotate()?;
        Ok(self.segment)
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
        q.add("/sdcard/x/con:.jpg", 10, 1700000000, &tmp, "abc")
            .unwrap();
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
}
