//! 항목별 백업 실행 — 열거 결과(walker)를 받아 파일을 PC로 옮기고 manifest 항목을 만든다.
//! §6-1: 해시는 전송 중 계산, mtime은 stat 원본값을 로컬 파일에 적용(pull -a 상당).
//! §6-2: 전수 열거 대비 크기 불일치(백업 중 원본 변경)·pull 실패는 오류로 기록 → 완결 불가.

use crate::backup::model::{FileEntry, ItemKind, ItemRecord, ItemStatus};
use crate::backup::quarantine::{
    quarantine_tmp_path, quarantined_entry, HashingWriter, Quarantine,
};
use crate::backup::walker::WalkedEntry;
use crate::backup::winname::{self, SeenPaths};
use adb_client::ADBDeviceExt;
use std::path::{Path, PathBuf};

/// 풀 대상 파일 — tag는 항목별 부가 키(apk의 패키지명 등). local_for가 로컬 경로를 정한다.
#[derive(Debug, Clone)]
pub struct PullFile {
    pub entry: WalkedEntry,
    pub tag: String,
}

impl PullFile {
    pub fn plain(entry: WalkedEntry) -> Self {
        Self {
            entry,
            tag: String::new(),
        }
    }
}

/// 진행 보고 — mod.rs에서 이벤트로 변환(쓰로틀 포함)
pub struct PullProgress<'a> {
    pub file: &'a str,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// 백업 실행 상태 — 항목 사이·파일 사이에 점검하는 취소 플래그
pub struct CancelFlag(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl CancelFlag {
    pub fn new() -> Self {
        Self(std::sync::Arc::new(std::sync::atomic::AtomicBool::new(
            false,
        )))
    }
    /// 전역(static) 취소 플래그와 연결 — backup_cancel 명령이 같은 플래그를 건드린다
    pub fn from_shared(arc: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self(arc)
    }
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

impl Default for CancelFlag {
    fn default() -> Self {
        Self::new()
    }
}

/// 파일 항목 백업 — `local_for`가 풀 대상 → 백업 루트 상대 로컬 경로를 정한다.
/// 대소문자 충돌 판정용 `seen`은 백업 전체에서 공유한다(서로 다른 항목끼리도 겹치지 않게).
#[allow(clippy::too_many_arguments)]
pub fn pull_item_files(
    dev: &mut dyn ADBDeviceExt,
    item_id: &str,
    kind: ItemKind,
    files: &[PullFile],
    backup_root: &Path,
    local_for: &dyn Fn(&PullFile) -> PathBuf,
    seen: &mut SeenPaths,
    quarantine: &mut Quarantine,
    cancel: &CancelFlag,
    mut on_progress: impl FnMut(PullProgress),
) -> ItemRecord {
    let mut rec = ItemRecord::new(item_id, kind);
    rec.files = files.len() as u32;
    rec.entries.reserve(files.len());
    let total_bytes: u64 = files.iter().map(|f| f.entry.size).sum();
    let mut bytes_done = 0u64;
    let mut nonce = 0u64;
    for (i, pf) in files.iter().enumerate() {
        if cancel.cancelled() {
            rec.errors.push("사용자가 작업을 취소했습니다".into());
            rec.status = ItemStatus::Partial;
            return rec;
        }
        let f = &pf.entry;
        let rel = local_for(pf);
        // `\`를 `/`로 바꾸지 않는다 — 기기 파일명 안의 `\`는 금지 문자로 판정돼 격리된다
        let rel_str = rel.to_string_lossy();
        let issue = winname::check_relative_path(&rel_str, &f.remote, seen);
        let entry = match issue {
            None => pull_to_disk(dev, &f.remote, &rel, backup_root, f.size, f.mtime),
            Some(_issue) => {
                // Windows 비호환 — 임시 이름으로 받아 tar로 격리(§6-3)
                nonce += 1;
                let tmp = quarantine_tmp_path(backup_root, nonce);
                match pull_to_tmp(dev, &f.remote, &tmp, f.size) {
                    Ok((sha256, actual)) => {
                        let qres = quarantine.add(&f.remote, actual, f.mtime, &tmp);
                        let _ = std::fs::remove_file(&tmp);
                        match qres {
                            Ok(()) => quarantined_entry(&f.remote, actual, f.mtime, &sha256),
                            Err(e) => error_entry(&f.remote, actual, f.mtime, e),
                        }
                    }
                    Err(e) => {
                        let _ = std::fs::remove_file(&tmp);
                        error_entry(&f.remote, f.size, f.mtime, e)
                    }
                }
            }
        };
        if entry.error.is_none() {
            // 목록 크기(32비트)가 아니라 검증된 실제 크기로 센다(4GiB 이상 파일)
            bytes_done += entry.size;
            rec.bytes += entry.size;
        } else {
            rec.errors.push(format!(
                "{}: {}",
                f.remote,
                entry.error.clone().unwrap_or_default()
            ));
        }
        on_progress(PullProgress {
            file: &f.remote,
            files_done: (i + 1) as u64,
            files_total: files.len() as u64,
            bytes_done,
            bytes_total: total_bytes,
        });
        let no_space = entry
            .error
            .as_deref()
            .is_some_and(|error| error.contains("NO_SPACE|"));
        rec.entries.push(entry);
        if no_space {
            cancel.set();
            break;
        }
    }
    rec.finalize();
    rec
}

/// 파일 1개를 디스크로 — 부모 폴더 생성, 해시 래퍼로 pull, 크기 검증, mtime 적용
pub(crate) fn pull_to_disk(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    rel: &Path,
    backup_root: &Path,
    expected_size: u64,
    mtime: u32,
) -> FileEntry {
    pull_to_disk_checked(
        dev,
        remote,
        rel,
        backup_root,
        expected_size,
        mtime,
        &|_, _| Ok(()),
    )
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn pull_to_disk_checked(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    rel: &Path,
    backup_root: &Path,
    expected_size: u64,
    mtime: u32,
    validate: &dyn Fn(&mut dyn ADBDeviceExt, &Path) -> Result<(), String>,
) -> FileEntry {
    let dest = match crate::backup::paths::write_target(backup_root, &rel.to_string_lossy()) {
        Ok(path) => path,
        Err(e) => return error_entry(remote, expected_size, mtime, e),
    };
    let mut base = FileEntry {
        remote: remote.to_string(),
        local: rel.to_string_lossy().replace('\\', "/"),
        size: expected_size,
        mtime,
        sha256: None,
        quarantined: false,
        error: None,
    };
    if let Some(parent) = dest.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            base.error = Some(format!("폴더 생성 실패: {}", crate::storage::io_error(&e)));
            return base;
        }
    }
    let result = crate::storage::atomic_file(&dest, |file, temporary| {
        let mut writer = HashingWriter::new(file);
        dev.pull(&remote, &mut writer)
            .map_err(|e| format!("전송 실패: {}", crate::storage::adb_io_error(&e)))?;
        let (_, hash, written) = writer.finish();
        let actual = verify_size(dev, remote, expected_size, written)?;
        validate(dev, temporary)?;
        let ft = filetime::FileTime::from_unix_time(mtime as i64, 0);
        filetime::set_file_mtime(temporary, ft)
            .map_err(|e| format!("파일 수정시각 보존 실패: {}", crate::storage::io_error(&e)))?;
        Ok((hash, actual))
    });
    match result {
        Ok((hash, size)) => {
            base.sha256 = Some(hash);
            base.size = size;
        }
        Err(error) => base.error = Some(error),
    }
    base
}

/// 임시 파일로 받아 (해시, 실제 크기) 반환(비호환 경로용)
fn pull_to_tmp(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    tmp: &Path,
    expected_size: u64,
) -> Result<(String, u64), String> {
    let file = std::fs::File::create(tmp)
        .map_err(|e| format!("임시 파일 생성 실패: {}", crate::storage::io_error(&e)))?;
    let mut writer = HashingWriter::new(file);
    dev.pull(&remote, &mut writer)
        .map_err(|e| format!("전송 실패: {}", crate::storage::adb_io_error(&e)))?;
    let (file, sha256, written) = writer.finish();
    file.sync_all().map_err(|e| {
        format!(
            "임시 파일 디스크 저장 실패: {}",
            crate::storage::io_error(&e)
        )
    })?;
    drop(file);
    let actual = verify_size(dev, remote, expected_size, written)?;
    Ok((sha256, actual))
}

/// 크기 검증 — SYNC list의 size는 u32라 4GiB 이상 파일은 wrap될 수 있다.
/// 불일치 시에만 셸 stat(64비트)으로 재확인한다(§6-2 소스 변경 감지와 구분).
fn verify_size(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    expected: u64,
    written: u64,
) -> Result<u64, String> {
    if expected == written {
        return Ok(written);
    }
    if let Ok(Some(ext)) = dev.stat_extended(&remote) {
        if ext.size == written {
            return Ok(written); // list 크기 wrap — 실제로는 정상 수신
        }
        return Err(format!(
            "크기 불일치(원본 변경 감지): stat {}B, 수신 {written}B",
            ext.size
        ));
    }
    Err(format!(
        "크기 불일치(원본 변경 감지): 예상 {expected}B, 수신 {written}B"
    ))
}

fn error_entry(remote: &str, size: u64, mtime: u32, err: String) -> FileEntry {
    FileEntry {
        remote: remote.to_string(),
        local: String::new(),
        size,
        mtime,
        sha256: None,
        quarantined: false,
        error: Some(err),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn adversarial_failed_repull_does_not_truncate_a_previous_success() {
        let root = tempfile::tempdir().unwrap();
        let mut d = FakeADBDevice::new();
        d.add_file("/sdcard/a", b"original", 0, 0o644);
        let first = pull_to_disk(&mut d, "/sdcard/a", Path::new("a"), root.path(), 8, 0);
        assert!(first.error.is_none());
        d.add_file("/sdcard/a", b"changed", 0, 0o644);
        d.fail_pull("/sdcard/a");
        assert!(
            pull_to_disk(&mut d, "/sdcard/a", Path::new("a"), root.path(), 7, 0)
                .error
                .is_some()
        );
        assert_eq!(std::fs::read(root.path().join("a")).unwrap(), b"original");
        d.fail_pull.clear();
        assert!(pull_to_disk_checked(
            &mut d,
            "/sdcard/a",
            Path::new("a"),
            root.path(),
            7,
            0,
            &|_, _| Err("validation failed".into())
        )
        .error
        .is_some());
        assert_eq!(std::fs::read(root.path().join("a")).unwrap(), b"original");
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 1);
    }

    fn setup() -> (
        FakeADBDevice,
        tempfile::TempDir,
        Quarantine,
        SeenPaths,
        CancelFlag,
    ) {
        let mut d = FakeADBDevice::new();
        d.add_dir("/sdcard/DCIM");
        d.add_file("/sdcard/DCIM/ok.jpg", b"jpeg-ok", 1700000000, 0o644);
        d.add_file("/sdcard/DCIM/bad?.jpg", b"q", 1700000001, 0o644);
        d.add_file("/sdcard/DCIM/lost.jpg", b"will-fail", 1700000002, 0o644);
        d.fail_pull("/sdcard/DCIM/lost.jpg");
        let tmp = tempfile::tempdir().unwrap();
        let q = Quarantine::new(tmp.path()).unwrap();
        (d, tmp, q, SeenPaths::new(), CancelFlag::new())
    }

    fn walk_files(d: &mut FakeADBDevice) -> Vec<PullFile> {
        crate::backup::walker::walk(d, "/sdcard/DCIM", &|_| false)
            .files
            .into_iter()
            .map(PullFile::plain)
            .collect()
    }

    #[test]
    fn pulls_hash_mtime_and_quarantines_bad_names() {
        let (mut d, tmp, mut q, mut seen, cancel) = setup();
        let files = walk_files(&mut d);
        assert_eq!(files.len(), 3);
        let rec = pull_item_files(
            &mut d,
            "dcim",
            ItemKind::Files,
            &files,
            tmp.path(),
            &|pf| {
                PathBuf::from(format!(
                    "sdcard/{}",
                    pf.entry.remote.trim_start_matches("/sdcard/")
                ))
            },
            &mut seen,
            &mut q,
            &cancel,
            |_| {},
        );
        // pull 실패 1건 → 부분 상태
        assert_eq!(rec.status, ItemStatus::Partial);
        assert_eq!(rec.errors.len(), 1);
        assert!(rec.errors[0].contains("lost.jpg"));
        // 정상 파일: 로컬 존재 + sha256 + mtime 적용
        let ok = rec
            .entries
            .iter()
            .find(|e| e.remote.ends_with("ok.jpg"))
            .unwrap();
        assert!(ok.error.is_none());
        assert_eq!(ok.sha256.as_deref().map(|s| s.len()), Some(64));
        let local = tmp.path().join(&ok.local);
        assert_eq!(std::fs::read(&local).unwrap(), b"jpeg-ok");
        let meta = std::fs::metadata(&local).unwrap();
        let got_mtime = filetime::FileTime::from_last_modification_time(&meta).unix_seconds();
        assert_eq!(got_mtime as u32, 1700000000);
        // 비호환 파일: quarantine 세그먼트에 원본 경로로 들어 있음
        let bad = rec
            .entries
            .iter()
            .find(|e| e.remote.ends_with("bad?.jpg"))
            .unwrap();
        assert!(bad.quarantined);
        assert!(bad.error.is_none());
        assert_eq!(q.finish().unwrap(), 1);
    }

    #[test]
    fn cancel_stops_between_files() {
        let (mut d, tmp, mut q, mut seen, cancel) = setup();
        let files = walk_files(&mut d);
        cancel.set();
        let rec = pull_item_files(
            &mut d,
            "dcim",
            ItemKind::Files,
            &files,
            tmp.path(),
            &|pf| PathBuf::from(pf.entry.remote.trim_start_matches('/')),
            &mut seen,
            &mut q,
            &cancel,
            |_| {},
        );
        assert_eq!(rec.status, ItemStatus::Partial);
        assert!(rec.entries.is_empty()); // 첫 파일 전에 취소
    }

    #[test]
    fn size_wrap_recheck_via_stat_extended() {
        // SYNC list의 u32 크기가 실제와 다를 때(4GiB wrap 등) 셸 stat(64비트)으로 재확인해 오탐 방지
        let mut d = FakeADBDevice::new();
        d.add_dir("/sdcard/DCIM");
        d.add_file("/sdcard/DCIM/wrap.bin", &[0u8; 100], 1700000000, 0o644);
        d.list_size_override
            .insert("/sdcard/DCIM/wrap.bin".into(), 50); // list는 50B라고 보고
        let tmp = tempfile::tempdir().unwrap();
        let mut q = Quarantine::new(tmp.path()).unwrap();
        let files: Vec<PullFile> = crate::backup::walker::walk(&mut d, "/sdcard/DCIM", &|_| false)
            .files
            .into_iter()
            .map(PullFile::plain)
            .collect();
        let rec = pull_item_files(
            &mut d,
            "dcim",
            ItemKind::Files,
            &files,
            tmp.path(),
            &|_| PathBuf::from("sdcard/DCIM/wrap.bin"),
            &mut SeenPaths::new(),
            &mut q,
            &CancelFlag::new(),
            |_| {},
        );
        assert_eq!(rec.status, ItemStatus::Done, "errors: {:?}", rec.errors);
        assert_eq!(rec.entries[0].size, 100); // 실제 크기로 정정
        q.finish().unwrap();
    }
}
