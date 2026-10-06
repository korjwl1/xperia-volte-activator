//! 백업 무결성 검사 — 모든 기록 파일을 고정 크기 버퍼로 스트리밍 해싱한다.
use super::model::{BackupSummary, FileEntry, ItemRecord, ItemStatus, Manifest};
use super::paths;
pub use crate::storage::hash_reader;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

pub fn segments(root: &Path) -> Result<Vec<PathBuf>, String> {
    let dir = root.join("quarantine");
    if !dir.exists() {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for entry in std::fs::read_dir(&dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.path().extension().is_some_and(|e| e == "tar") {
            let relative = format!("quarantine/{}", entry.file_name().to_string_lossy());
            out.push(paths::existing_file(root, &relative)?);
        }
    }
    out.sort();
    Ok(out)
}

/// tar 항목의 원본 유닉스 경로("/sdcard/...") — 경로 바이트를 그대로 쓴다.
/// Windows `Path`로 바꾸면 이름 안의 `\`가 폴더 구분자로 해석돼 다른 파일이 된다.
pub(super) fn tar_remote<R: Read>(entry: &tar::Entry<'_, R>) -> String {
    format!("/{}", String::from_utf8_lossy(&entry.path_bytes()))
}

/// 격리 세그먼트별 해시. 손상된 세그먼트 하나가 다른 세그먼트의 검증까지 막지 않도록
/// 세그먼트 오류는 따로 모은다. 재시도로 같은 원본 이름이 여러 세그먼트에 있을 수 있다.
pub(super) struct QuarantineHashes {
    pub versions: HashMap<String, Vec<(String, u64)>>,
    pub segment_errors: Vec<String>,
}

pub(super) fn quarantine_hashes(
    root: &Path,
    wanted: &HashSet<String>,
) -> Result<QuarantineHashes, String> {
    quarantine_hashes_control(
        root,
        wanted,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

fn quarantine_hashes_control(
    root: &Path,
    wanted: &HashSet<String>,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Result<QuarantineHashes, String> {
    let mut out = QuarantineHashes {
        versions: HashMap::new(),
        segment_errors: vec![],
    };
    for segment in segments(root)? {
        if cancel.cancelled() {
            return Err("PC 격리 파일 검사 취소".into());
        }
        let name = segment
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let result = std::fs::File::open(&segment)
            .map_err(paths::read_error)
            .and_then(|file| archive_hashes_control(file, wanted, cancel, progress));
        match result {
            Ok(hashes) => {
                for (remote, versions) in hashes {
                    out.versions.entry(remote).or_default().extend(versions);
                }
            }
            Err(error) => {
                if error.contains("PC_READ_IO|") || error.starts_with("PC_IO|") {
                    cancel.stop_with_error(format!(
                        "PC 격리 파일 읽기 실패 — 완료 기록을 보존합니다: {error}"
                    ));
                }
                out.segment_errors.push(format!("{name}: {error}"));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
fn archive_hashes(
    reader: impl Read + Seek,
    wanted: &HashSet<String>,
) -> Result<HashMap<String, Vec<(String, u64)>>, String> {
    archive_hashes_control(
        reader,
        wanted,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

fn archive_hashes_control(
    reader: impl Read + Seek,
    wanted: &HashSet<String>,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Result<HashMap<String, Vec<(String, u64)>>, String> {
    use sha2::{Digest, Sha256};
    let mut hashes: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries_with_seek()
        .map_err(super::recovery::archive_error)?
    {
        let entry = entry.map_err(super::recovery::archive_error)?;
        let remote = tar_remote(&entry);
        paths::archive_relative(&remote)?;
        if !entry.header().entry_type().is_file() {
            return Err("격리 tar에 일반 파일이 아닌 항목이 있습니다".into());
        }
        // 모든 헤더의 경로/유형을 검사하되, 선택하지 않은 본문은 seek로 건너뛴다.
        if wanted.contains(&remote) {
            let mut entry = entry;
            let total = entry.size();
            let mut hash = Sha256::new();
            let mut size = 0;
            let mut buffer = vec![0; 256 * 1024];
            loop {
                if cancel.cancelled() {
                    return Err("PC 격리 파일 검사 취소".into());
                }
                let count = entry
                    .read(&mut buffer)
                    .map_err(super::recovery::archive_error)?;
                if count == 0 {
                    break;
                }
                size += count as u64;
                hash.update(&buffer[..count]);
                progress(super::runner::StepProgress {
                    item_id: "quarantine".into(),
                    phase: "verify",
                    file: Some(remote.clone()),
                    files_done: hashes.len() as u64,
                    files_total: wanted.len() as u64,
                    bytes_done: size,
                    bytes_total: total,
                });
            }
            hashes
                .entry(remote)
                .or_default()
                .push((hex::encode(hash.finalize()), size));
        }
    }
    Ok(hashes)
}

#[cfg(test)]
fn check_entry(root: &Path, entry: &FileEntry) -> Result<(), String> {
    check_entry_control(root, entry, &super::puller::CancelFlag::new(), &|_| {})
}

fn check_entry_control(
    root: &Path,
    entry: &FileEntry,
    cancel: &super::puller::CancelFlag,
    bytes: &dyn Fn(u64),
) -> Result<(), String> {
    if cancel.cancelled() {
        return Err("PC 파일 검사 취소".into());
    }
    if let Some(error) = &entry.error {
        return Err(format!("{}: {error}", entry.remote));
    }
    if entry
        .sha256
        .as_ref()
        .is_none_or(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err(format!("해시 기록 없음/잘못된 해시: {}", entry.remote));
    }
    if entry.quarantined {
        return Ok(()); // tar contents are checked once by verify_items.
    }
    let result = paths::existing_file(root, &entry.local)
        .and_then(|path| std::fs::File::open(path).map_err(paths::read_error))
        .and_then(|mut file| {
            use sha2::{Digest, Sha256};
            let mut hash = Sha256::new();
            let mut size = 0;
            let mut buffer = vec![0; 1024 * 1024];
            loop {
                if cancel.cancelled() {
                    return Err("PC 파일 검사 취소".into());
                }
                let count = file.read(&mut buffer).map_err(paths::read_error)?;
                if count == 0 {
                    break;
                }
                hash.update(&buffer[..count]);
                size += count as u64;
                bytes(count as u64);
            }
            Ok((hex::encode(hash.finalize()), size))
        });
    match result {
        Ok((hash, size))
            if size == entry.size && Some(hash.as_str()) == entry.sha256.as_deref() =>
        {
            Ok(())
        }
        Ok(_) => Err(format!("크기/해시 불일치: {}", entry.remote)),
        Err(error) => {
            if error.contains("PC_READ_IO|") {
                cancel.stop_with_error(format!(
                    "PC 백업 파일 읽기 실패 — 완료 기록을 보존합니다: {}: {error}",
                    entry.remote
                ));
            }
            Err(format!("{}: {error}", entry.remote))
        }
    }
}

/// Bounded PC workers, cancellable inside large files. The caller thread alone emits progress.
#[cfg(test)]
fn check_entries(root: &Path, entries: &[FileEntry]) -> Vec<Result<(), String>> {
    check_entries_control(
        root,
        entries,
        &super::puller::CancelFlag::new(),
        &mut |_, _, _| {},
    )
}
fn check_entries_control(
    root: &Path,
    entries: &[FileEntry],
    cancel: &super::puller::CancelFlag,
    progress: &mut dyn FnMut(u64, u64, Option<String>),
) -> Vec<Result<(), String>> {
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get().min(4))
        .min(entries.len());
    let next = AtomicUsize::new(0);
    let total_bytes = AtomicU64::new(0);
    let done = AtomicU64::new(0);
    let (tx, rx) = std::sync::mpsc::sync_channel::<Option<String>>(32);
    let mut results = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                let tx = tx.clone();
                let next = &next;
                let total_bytes = &total_bytes;
                let done = &done;
                scope.spawn(move || {
                    let mut results = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(entry) = entries.get(index) else {
                            break;
                        };
                        let result = check_entry_control(root, entry, cancel, &|bytes| {
                            total_bytes.fetch_add(bytes, Ordering::Relaxed);
                            let _ = tx.send(None);
                        });
                        done.fetch_add(1, Ordering::Relaxed);
                        let _ = tx.send(Some(entry.remote.clone()));
                        results.push((index, result));
                    }
                    results
                })
            })
            .collect();
        drop(tx);
        for file in rx {
            progress(
                done.load(Ordering::Relaxed),
                total_bytes.load(Ordering::Relaxed),
                file,
            );
        }
        handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("backup hash worker panicked"))
            .collect::<Vec<_>>()
    });
    results.sort_unstable_by_key(|(index, _)| *index);
    results.into_iter().map(|(_, result)| result).collect()
}

#[cfg(test)]
fn checked_item(root: &Path, item: &ItemRecord) -> (Vec<String>, Vec<bool>) {
    checked_item_control(
        root,
        item,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

fn checked_item_control(
    root: &Path,
    item: &ItemRecord,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> (Vec<String>, Vec<bool>) {
    let mut errors = item.errors.clone();
    let mut names = HashMap::new();
    let mut valid = vec![false; item.entries.len()];
    let total = item.entries.iter().map(|e| e.size).sum();
    progress(super::runner::StepProgress::start(
        &item.id,
        "verify",
        item.entries.len() as u64,
    ));
    let results = check_entries_control(root, &item.entries, cancel, &mut |files, bytes, file| {
        progress(super::runner::StepProgress {
            item_id: item.id.clone(),
            phase: "verify",
            file,
            files_done: files,
            files_total: item.entries.len() as u64,
            bytes_done: bytes,
            bytes_total: total,
        });
    });
    for (index, result) in results.into_iter().enumerate() {
        let entry = &item.entries[index];
        match result {
            Ok(()) => valid[index] = true,
            Err(error) => errors.push(error),
        }
        if let Some(previous) = names.insert(&entry.remote, index) {
            errors.push(format!("중복 파일 기록: {}", entry.remote));
            valid[previous] = false;
            valid[index] = false;
        }
    }
    for artifact in &item.artifacts {
        if let Err(error) = paths::existing_file(root, artifact) {
            errors.push(error);
        }
    }
    (errors, valid)
}

#[cfg(test)]
pub fn item_problems(root: &Path, item: &ItemRecord) -> Vec<String> {
    checked_item(root, item).0
}

pub(super) type VerifiedEntries = HashMap<String, HashMap<String, FileEntry>>;

/// `include`가 고른 항목만 검사 → (항목 위치, 문제 목록). 문제가 있는 항목은 Partial로 낮춘다.
fn verify_items(
    root: &Path,
    manifest: &mut Manifest,
    include: &dyn Fn(&ItemRecord) -> bool,
    mut retained: Option<&mut VerifiedEntries>,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Vec<(usize, Vec<String>)> {
    let wanted: HashSet<String> = manifest
        .items
        .iter()
        .filter(|item| item.status != ItemStatus::Skipped && include(item))
        .flat_map(|item| item.entries.iter())
        .filter(|entry| entry.quarantined && entry.error.is_none())
        .map(|entry| entry.remote.clone())
        .collect();
    let quarantine = if !wanted.is_empty() {
        quarantine_hashes_control(root, &wanted, cancel, progress)
    } else {
        Ok(QuarantineHashes {
            versions: HashMap::new(),
            segment_errors: vec![],
        })
    };
    let mut out = vec![];
    for (index, item) in manifest.items.iter_mut().enumerate() {
        if item.status == ItemStatus::Skipped || !include(item) {
            continue;
        }
        let (mut errors, mut valid) = checked_item_control(root, item, cancel, progress);
        for receipt in manifest
            .source_metadata
            .iter()
            .filter(|r| r.item_id == item.id && r.required())
        {
            if let Err(error) = super::source_metadata::load(root, receipt) {
                if error.contains("PC_READ_IO|") {
                    cancel.stop_with_error(error.clone());
                }
                errors.push(error);
            }
            if !receipt.complete {
                errors.extend(receipt.errors.clone());
            }
        }
        for (entry_index, entry) in item
            .entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.quarantined && e.error.is_none())
        {
            let q = match &quarantine {
                Ok(q) => q,
                Err(error) => {
                    valid[entry_index] = false;
                    errors.push(error.clone());
                    continue;
                }
            };
            let found = q.versions.get(&entry.remote).is_some_and(|versions| {
                versions.iter().any(|(hash, size)| {
                    Some(hash.as_str()) == entry.sha256.as_deref() && *size == entry.size
                })
            });
            if found {
                continue;
            }
            valid[entry_index] = false;
            // 손상 세그먼트가 있으면 그 사유를 함께 보인다
            errors.push(match q.segment_errors.first() {
                Some(seg) => format!(
                    "격리 파일 누락/해시 불일치: {} (손상 세그먼트 {seg})",
                    entry.remote
                ),
                None => format!("격리 파일 누락/해시 불일치: {}", entry.remote),
            });
        }
        if let Some(retained) = retained.as_deref_mut() {
            retained.insert(
                item.id.clone(),
                item.entries
                    .iter()
                    .zip(valid)
                    .filter(|(_, valid)| *valid)
                    .map(|(entry, _)| (entry.remote.clone(), entry.clone()))
                    .collect(),
            );
        }
        if !errors.is_empty() {
            item.status = ItemStatus::Partial;
            out.push((index, errors));
        }
    }
    out
}

pub fn verify_manifest(root: &Path, manifest: &mut Manifest) -> Vec<String> {
    verify_manifest_progress(
        root,
        manifest,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

pub(super) fn verify_manifest_progress(
    root: &Path,
    manifest: &mut Manifest,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Vec<String> {
    let mut problems = vec![];
    for app in &manifest.omitted_apps {
        if app.cleanup_pending {
            problems.push(format!(
                "app-data: {}: 제외한 앱 데이터 정리 미완료",
                app.package
            ));
        }
    }
    for entry in manifest.items.iter().flat_map(|item| &item.entries) {
        if super::omissions::package(&entry.remote)
            .is_some_and(|name| manifest.omitted_apps.iter().any(|app| app.package == name))
        {
            problems.push(format!(
                "제외한 앱 데이터가 백업 기록에 남아 있습니다: {}",
                entry.remote
            ));
        }
    }
    let mut ids = HashSet::new();
    for item in &manifest.items {
        if !ids.insert(item.id.as_str()) {
            problems.push(format!("중복 항목: {}", item.id));
        }
    }
    for (index, errors) in verify_items(root, manifest, &|_| true, None, cancel, progress) {
        let id = &manifest.items[index].id;
        problems.extend(errors.into_iter().map(|e| format!("{id}: {e}")));
    }
    problems
}

/// 이어서 백업용 — 선택 항목만 다시 검사하고, 문제는 그 항목의 오류로 남긴다
/// (상태만 낮추고 사유를 버리면 요약에 이유 없이 미완결로 보인다).
pub fn verify_selected(root: &Path, manifest: &mut Manifest, selected: &[String]) {
    verify_selected_retaining(root, manifest, selected);
}

/// Return receipts from this same disk pass, without rehashing partial items.
pub(super) fn verify_selected_retaining(
    root: &Path,
    manifest: &mut Manifest,
    selected: &[String],
) -> VerifiedEntries {
    verify_selected_retaining_progress(
        root,
        manifest,
        selected,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

pub(super) fn verify_selected_retaining_progress(
    root: &Path,
    manifest: &mut Manifest,
    selected: &[String],
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> VerifiedEntries {
    let mut retained = HashMap::new();
    for (index, errors) in verify_items(
        root,
        manifest,
        &|item| selected.contains(&item.id),
        Some(&mut retained),
        cancel,
        progress,
    ) {
        let item = &mut manifest.items[index];
        for error in errors {
            if !item.errors.contains(&error) {
                item.errors.push(error);
            }
        }
    }
    retained
}

pub(super) fn verify_device(
    dev: &mut dyn adb_client::ADBDeviceExt,
    manifest: &Manifest,
) -> Result<(), String> {
    let saved = manifest
        .device_key
        .as_deref()
        .ok_or("백업의 원본 기기 키가 없어 같은 폰인지 확인할 수 없습니다")?;
    if crate::device_io::identity_key(dev)? != saved {
        return Err("백업 원본 기기와 복원 대상 기기가 다릅니다".into());
    }
    Ok(())
}

#[cfg(test)]
pub fn backup_summary(root: &Path) -> Result<BackupSummary, String> {
    backup_summary_progress(
        root,
        &super::puller::CancelFlag::new(),
        &mut (Box::new(|_| {}) as super::runner::ProgressSink),
    )
}

pub(super) fn backup_summary_progress(
    root: &Path,
    cancel: &super::puller::CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Result<BackupSummary, String> {
    let mut manifest = super::model::load_manifest(root)?;
    let excluded = manifest.excluded_items.clone();
    manifest.items.retain(|item| !excluded.contains(&item.id));
    let problems = verify_manifest_progress(root, &mut manifest, cancel, progress);
    if cancel.cancelled() {
        return Err("PC 백업 검사 취소 — 완료 파일은 보존됩니다".into());
    }
    let mut summary = BackupSummary::from(&manifest);
    if !problems.is_empty() {
        summary.complete = false;
        summary.errors.extend(problems);
    }
    let mut seen = HashSet::new();
    summary.errors.retain(|error| seen.insert(error.clone()));
    summary.dir = root.to_string_lossy().to_string();
    Ok(summary)
}

#[cfg(test)]
use sha2::{Digest, Sha256};

#[cfg(test)]
mod tests {
    use super::super::model::{FileEntry, ItemKind};
    use super::*;
    #[test]
    fn hashing_reports_bytes_and_can_cancel_inside_one_large_file() {
        let root = tempfile::tempdir().unwrap();
        let bytes = vec![42; 24 * 1024 * 1024];
        std::fs::write(root.path().join("large.jpg"), &bytes).unwrap();
        let entries = vec![entry("large.jpg", &bytes)];
        let cancel = super::super::puller::CancelFlag::new();
        let mut largest = 0;
        let results = check_entries_control(root.path(), &entries, &cancel, &mut |_, bytes, _| {
            largest = largest.max(bytes);
            cancel.set();
        });
        assert!(largest > 0 && largest < entries[0].size);
        assert!(results[0].as_ref().unwrap_err().contains("취소"));
    }
    #[test]
    fn unselected_tar_payload_is_skipped_without_reading() {
        use std::io::{Cursor, SeekFrom};
        struct GuardedReader(Cursor<Vec<u8>>);
        impl Read for GuardedReader {
            fn read(&mut self, buffer: &mut [u8]) -> std::io::Result<usize> {
                if (512..512 + 1024 * 1024).contains(&self.0.position()) {
                    return Err(std::io::Error::other("unselected payload was read"));
                }
                self.0.read(buffer)
            }
        }
        impl Seek for GuardedReader {
            fn seek(&mut self, offset: SeekFrom) -> std::io::Result<u64> {
                self.0.seek(offset)
            }
        }
        let mut builder = tar::Builder::new(Vec::new());
        for (name, bytes) in [
            ("sdcard/DCIM/unselected.jpg", vec![0; 1024 * 1024]),
            ("sdcard/DCIM/selected.jpg", vec![42]),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, bytes.as_slice())
                .unwrap();
        }
        let bytes = builder.into_inner().unwrap();
        let key = "/sdcard/DCIM/selected.jpg".to_string();
        let wanted = HashSet::from([key.clone()]);
        let hashes = archive_hashes(GuardedReader(Cursor::new(bytes)), &wanted).unwrap();
        assert_eq!(hashes.len(), 1);
        assert_eq!(hashes[&key], vec![hash_reader([42].as_slice()).unwrap()]);
    }
    fn entry(name: &str, bytes: &[u8]) -> FileEntry {
        FileEntry {
            remote: format!("/sdcard/DCIM/{name}"),
            local: name.into(),
            size: bytes.len() as u64,
            mtime: 0,
            sha256: Some(hex::encode(Sha256::digest(bytes))),
            quarantined: false,
            error: None,
        }
    }
    fn manifest(entries: Vec<FileEntry>) -> Manifest {
        let mut manifest = Manifest::new("XQ", "masked", "version", "15");
        manifest.items.push(ItemRecord {
            id: "dcim".into(),
            kind: ItemKind::Files,
            status: ItemStatus::Done,
            files: entries.len() as u32,
            bytes: 0,
            entries,
            artifacts: vec![],
            errors: vec![],
        });
        manifest
    }
    #[test]
    fn source_denials_are_reported_once_each_without_marking_complete() {
        let root = tempfile::tempdir().unwrap();
        let mut entries = vec![entry("a.jpg", b""), entry("b.jpg", b"")];
        for entry in &mut entries {
            entry.error = Some("Permission denied".into());
        }
        let mut manifest = manifest(entries);
        manifest.items[0].status = ItemStatus::Partial;
        manifest.items[0].errors = vec![
            "/sdcard/DCIM/a.jpg: Permission denied".into(),
            "/sdcard/DCIM/b.jpg: Permission denied".into(),
            "/sdcard/DCIM/private: Permission denied".into(),
        ];
        super::super::model::save_manifest_atomic(&manifest, root.path()).unwrap();
        let summary = backup_summary(root.path()).unwrap();
        assert!(!summary.complete);
        assert_eq!(
            summary.errors,
            vec![
                "dcim: /sdcard/DCIM/a.jpg: Permission denied",
                "dcim: /sdcard/DCIM/b.jpg: Permission denied",
                "dcim: /sdcard/DCIM/private: Permission denied",
            ]
        );
    }

    #[test]
    fn parallel_checks_retain_only_valid_unique_receipts() {
        let root = tempfile::tempdir().unwrap();
        let mut entries = vec![];
        for i in 0..24 {
            let name = format!("{i}.jpg");
            std::fs::write(root.path().join(&name), b"good").unwrap();
            entries.push(entry(&name, b"good"));
        }
        entries.push(entries[3].clone());
        std::fs::write(root.path().join("5.jpg"), b"evil").unwrap();
        std::fs::remove_file(root.path().join("7.jpg")).unwrap();
        let serial: Vec<_> = entries
            .iter()
            .map(|entry| check_entry(root.path(), entry))
            .collect();
        assert_eq!(check_entries(root.path(), &entries), serial);
        let mut manifest = manifest(entries);
        let retained = verify_selected_retaining(root.path(), &mut manifest, &["dcim".into()]);
        assert_eq!(retained["dcim"].len(), 21);
        for name in ["3.jpg", "5.jpg", "7.jpg"] {
            assert!(!retained["dcim"].contains_key(&format!("/sdcard/DCIM/{name}")));
        }
        assert_eq!(manifest.items[0].status, ItemStatus::Partial);
    }

    #[test]
    fn detects_corruption_beyond_the_first_three_files() {
        let root = tempfile::tempdir().unwrap();
        let mut entries = vec![];
        for i in 0..4 {
            let name = format!("{i}.jpg");
            std::fs::write(root.path().join(&name), b"good").unwrap();
            entries.push(entry(&name, b"good"));
        }
        std::fs::write(root.path().join("3.jpg"), b"evil").unwrap();
        let mut manifest = manifest(entries);
        assert!(!verify_manifest(root.path(), &mut manifest).is_empty());
        assert!(!manifest.complete());
    }
    #[test]
    fn missing_quarantine_directory_is_not_complete() {
        let root = tempfile::tempdir().unwrap();
        let mut file = entry("bad?.jpg", b"x");
        file.quarantined = true;
        file.local.clear();
        let mut manifest = manifest(vec![file]);
        assert!(!verify_manifest(root.path(), &mut manifest).is_empty());
        assert!(!manifest.complete());
    }
    #[test]
    fn rejects_absolute_and_parent_local_paths() {
        let root = tempfile::tempdir().unwrap();
        for name in ["../file", "..\\file", "C:\\file"] {
            let mut manifest = manifest(vec![entry(name, b"x")]);
            assert!(!verify_manifest(root.path(), &mut manifest).is_empty());
        }
    }
}
