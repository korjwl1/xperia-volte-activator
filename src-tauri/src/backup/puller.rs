//! 항목별 백업 실행 — 열거 결과(walker)를 받아 파일을 PC로 옮기고 manifest 항목을 만든다.
//! §6-1: 해시는 전송 중 계산, mtime은 stat 원본값을 로컬 파일에 적용(pull -a 상당).
//! §6-2: 전수 열거 대비 크기 불일치(백업 중 원본 변경)·pull 실패는 오류로 기록 → 완결 불가.

#[cfg(test)]
use crate::backup::model::ItemStatus;
use crate::backup::model::{FileEntry, ItemKind, ItemRecord};
use crate::backup::quarantine::{
    quarantine_tmp_path, quarantined_entry, HashingWriter, Quarantine,
};
use crate::backup::walker::WalkedEntry;
use crate::backup::winname::{self, SeenPaths};
use adb_client::ADBDeviceExt;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

const WRITE_BUFFER_SIZE: usize = 256 * 1024;

struct LimitedWriter<W> {
    inner: W,
    remaining: Option<u64>,
}
impl<W: Write> Write for LimitedWriter<W> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self
            .remaining
            .is_some_and(|remaining| bytes.len() as u64 > remaining)
        {
            return Err(std::io::Error::other(
                "sample file exceeded its verified byte limit",
            ));
        }
        let count = self.inner.write(bytes)?;
        if let Some(remaining) = self.remaining.as_mut() {
            *remaining -= count as u64;
        }
        Ok(count)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

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
pub struct CancelFlag(
    std::sync::Arc<std::sync::atomic::AtomicBool>,
    std::sync::Mutex<Option<String>>,
);

impl CancelFlag {
    pub fn new() -> Self {
        Self(
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            std::sync::Mutex::new(None),
        )
    }
    /// 전역(static) 취소 플래그와 연결 — backup_cancel 명령이 같은 플래그를 건드린다
    pub fn from_shared(arc: std::sync::Arc<std::sync::atomic::AtomicBool>) -> Self {
        Self(arc, std::sync::Mutex::new(None))
    }
    pub fn set(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// 처음 멈춘 원인만 남긴다 — 사용자 취소(사유 없는 set)가 먼저였다면 뒤 오류로 덮지 않는다
    pub fn stop_with_error(&self, error: String) {
        let mut reason = self.1.lock().unwrap_or_else(|e| e.into_inner());
        if !self.0.swap(true, std::sync::atomic::Ordering::Relaxed) {
            *reason = Some(error);
        }
    }
    pub fn reason(&self) -> String {
        self.1
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| "사용자가 작업을 취소했습니다".into())
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
#[cfg(test)]
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
    pull_item_files_resuming(
        dev,
        item_id,
        kind,
        files,
        backup_root,
        local_for,
        seen,
        quarantine,
        cancel,
        &std::collections::HashMap::new(),
        &mut on_progress,
    )
}

/// `verified` contains only receipts hashed successfully during this resume.
#[allow(clippy::too_many_arguments)]
pub(super) fn pull_item_files_resuming(
    dev: &mut dyn ADBDeviceExt,
    item_id: &str,
    kind: ItemKind,
    files: &[PullFile],
    backup_root: &Path,
    local_for: &dyn Fn(&PullFile) -> PathBuf,
    seen: &mut SeenPaths,
    quarantine: &mut Quarantine,
    cancel: &CancelFlag,
    verified: &std::collections::HashMap<String, FileEntry>,
    mut on_progress: impl FnMut(PullProgress),
) -> ItemRecord {
    let mut rec = ItemRecord::new(item_id, kind);
    rec.files = files.len() as u32;
    rec.entries.reserve(files.len());
    let total_bytes: u64 = files.iter().map(|f| f.entry.size).sum();
    let mut bytes_done = 0u64;
    let mut nonce = 0u64;
    if let Err(error) = dev.begin_sync_batch() {
        rec.errors.push(format!(
            "파일 전송 준비 실패: {}",
            crate::backup::scrub(&error.to_string())
        ));
        rec.finalize();
        return rec;
    }
    for (i, pf) in files.iter().enumerate() {
        if cancel.cancelled() {
            rec.errors.push(cancel.reason());
            break;
        }
        let f = &pf.entry;
        let rel = local_for(pf);
        // `\`를 `/`로 바꾸지 않는다 — 기기 파일명 안의 `\`는 금지 문자로 판정돼 격리된다
        let rel_str = rel.to_string_lossy();
        let issue = winname::check_relative_path(&rel_str, &f.remote, seen);
        // Standard size/mtime quick-check, after validating the PC snapshot's hash.
        // Unknown timestamps and wrapped LIST sizes fall back to a normal pull.
        let reusable = verified.get(&f.remote).filter(|old| {
            if old.error.is_some() || old.mtime == 0 || old.size != f.size || old.mtime != f.mtime {
                return false;
            }
            if old.quarantined {
                return issue.is_some();
            }
            issue.is_none()
                && Path::new(&old.local) == rel
                && super::paths::existing_file(backup_root, &old.local)
                    .ok()
                    .and_then(|path| std::fs::metadata(path).ok())
                    .is_some_and(|metadata| {
                        metadata.len() == old.size
                            && metadata
                                .modified()
                                .ok()
                                .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                                .is_some_and(|time| time.as_secs() == u64::from(old.mtime))
                    })
        });
        let entry = if let Some(old) = reusable {
            old.clone()
        } else {
            match issue {
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
        let no_space = entry.error.as_deref().is_some_and(|error| {
            error.contains("NO_SPACE|") || error.contains("SYNC_BATCH_BROKEN|")
        });
        rec.entries.push(entry);
        if no_space {
            cancel.set();
            break;
        }
    }
    if let Err(error) = dev.end_sync_batch() {
        rec.errors.push(format!(
            "파일 전송 종료 실패: {}",
            crate::backup::scrub(&error.to_string())
        ));
        cancel.set();
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
    pull_to_disk_internal(
        dev,
        remote,
        rel,
        backup_root,
        expected_size,
        mtime,
        validate,
        None,
    )
}

#[cfg(feature = "dev-cli")]
pub(crate) fn pull_to_disk_limited(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    rel: &Path,
    backup_root: &Path,
    expected_size: u64,
    mtime: u32,
) -> FileEntry {
    pull_to_disk_internal(
        dev,
        remote,
        rel,
        backup_root,
        expected_size,
        mtime,
        &|_, _| Ok(()),
        Some(expected_size),
    )
}

#[allow(clippy::too_many_arguments)]
fn pull_to_disk_internal(
    dev: &mut dyn ADBDeviceExt,
    remote: &str,
    rel: &Path,
    backup_root: &Path,
    expected_size: u64,
    mtime: u32,
    validate: &dyn Fn(&mut dyn ADBDeviceExt, &Path) -> Result<(), String>,
    byte_limit: Option<u64>,
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
        let mut writer = HashingWriter::new(LimitedWriter {
            inner: BufWriter::with_capacity(WRITE_BUFFER_SIZE, file),
            remaining: byte_limit,
        });
        dev.pull(&remote, &mut writer)
            .map_err(|e| format!("전송 실패: {}", crate::storage::adb_io_error(&e)))?;
        let (mut buffered, hash, written) = writer.finish();
        buffered
            .flush()
            .map_err(|e| format!("파일 쓰기 실패: {}", crate::storage::io_error(&e)))?;
        drop(buffered);
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
    let mut writer = HashingWriter::new(BufWriter::with_capacity(WRITE_BUFFER_SIZE, file));
    dev.pull(&remote, &mut writer)
        .map_err(|e| format!("전송 실패: {}", crate::storage::adb_io_error(&e)))?;
    let (buffered, sha256, written) = writer.finish();
    let file = buffered
        .into_inner()
        .map_err(|e| format!("파일 쓰기 실패: {}", crate::storage::io_error(e.error())))?;
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
    fn first_stop_cause_wins_over_later_errors() {
        // 사용자 취소(공유 플래그만 켜짐)가 먼저면 뒤이은 오류가 사유를 덮지 않는다
        let shared = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel = CancelFlag::from_shared(shared.clone());
        shared.store(true, std::sync::atomic::Ordering::Relaxed);
        cancel.stop_with_error("PC_READ_IO|late".into());
        assert_eq!(cancel.reason(), "사용자가 작업을 취소했습니다");
        // 오류가 먼저면 그 사유를 유지하고 다음 오류로 바꾸지 않는다
        let cancel = CancelFlag::new();
        cancel.stop_with_error("first".into());
        cancel.stop_with_error("second".into());
        assert!(cancel.cancelled());
        assert_eq!(cancel.reason(), "first");
    }

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
    fn cancelled_batch_and_failed_close_do_not_report_success_or_leak_batch_state() {
        for cancel_before_copy in [true, false] {
            let (mut device, root, mut quarantine, mut seen, cancel) = setup();
            device.fail_pull.clear();
            let files = walk_files(&mut device);
            device.sync_batch_calls.clear();
            if cancel_before_copy {
                cancel.set();
            } else {
                device.fail_sync_batch_end = true;
            }
            let record = pull_item_files(
                &mut device,
                "dcim",
                ItemKind::Files,
                &files,
                root.path(),
                &|file| PathBuf::from(file.entry.remote.rsplit('/').next().unwrap()),
                &mut seen,
                &mut quarantine,
                &cancel,
                |_| {},
            );
            assert_eq!(record.status, ItemStatus::Partial);
            assert!(!device.sync_batch_active);
            assert_eq!(device.sync_batch_calls, ["begin", "end"]);
            assert!(!record.errors.is_empty());
            if cancel_before_copy {
                assert!(device.pull_calls.is_empty());
            } else {
                assert_eq!(record.entries.len(), files.len());
            }
        }
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
