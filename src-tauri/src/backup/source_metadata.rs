//! Source attributes are portable records, never inferred from the PC filesystem.
//! Batched stat has no filename in stdout: argument order is used only after a
//! successful command AND an exact record count. Failed batches are bisected.
use super::model::{FileEntry, Manifest};
use super::puller::CancelFlag;
use adb_client::ADBDeviceExt;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const FORMAT: &str = "%f\t%u\t%g\t%s\t%X\t%Y\t%Z\t%W\t%i\t%h\t%b\t%U\t%G\t%C\t%x\t%y\t%z\t%w";
const MAX_PATHS: usize = 128;
const MAX_COMMAND: usize = 24 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attributes {
    pub remote: String,
    #[serde(default)]
    pub observed_at: String,
    #[serde(default)]
    pub capture_context: String,
    /// Full st_mode, including type and special permission bits.
    pub mode: u32,
    pub uid: u32,
    pub gid: u32,
    pub size: u64,
    pub access_time: i64,
    pub modification_time: i64,
    /// POSIX ctime: inode status change, NOT birth/creation time.
    pub status_change_time: i64,
    pub birth_time: Option<i64>,
    pub inode: u64,
    pub hard_links: u64,
    pub blocks_512: u64,
    pub owner_name: String,
    pub group_name: String,
    pub security_context: Option<String>,
    /// Preserve the source's precision and timezone even when epoch output is seconds.
    pub access_time_text: String,
    pub modification_time_text: String,
    pub status_change_time_text: String,
    pub birth_time_text: Option<String>,
    /// Only an association by size + mtime, not proof of a filesystem snapshot.
    pub payload_matches: Option<bool>,
    pub backup_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Issue {
    pub remote: String,
    pub error: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: u32,
    pub item_id: String,
    pub captured_at: String,
    /// before-copy or after-copy-enrichment; never pretend enrichment was original.
    pub capture_context: String,
    pub limitations: Vec<String>,
    pub entries: Vec<Attributes>,
    pub issues: Vec<Issue>,
    #[serde(skip)]
    format_failures: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Receipt {
    pub item_id: String,
    pub path: String,
    pub sha256: String,
    pub entries: u64,
    pub directories: u64,
    pub unavailable_birth_times: u64,
    pub payload_mismatches: u64,
    pub complete: bool,
    pub errors: Vec<String>,
    #[serde(default)]
    pub capture_context: String,
}

impl Receipt {
    pub fn required(&self) -> bool {
        self.capture_context != "after-copy-enrichment"
    }
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn parse(remote: &str, line: &str) -> Result<Attributes, String> {
    let fields: Vec<_> = line.split('\t').collect();
    if fields.len() != 18 {
        return Err("원본 속성 응답 필드 수가 다릅니다".into());
    }
    let number = |i: usize| {
        fields[i]
            .parse::<u64>()
            .map_err(|_| "원본 속성 숫자 해석 실패".to_string())
    };
    let time = |i: usize| {
        fields[i]
            .parse::<i64>()
            .or_else(|_| fields[i].parse::<u64>().map(|n| n as i64))
            .map_err(|_| "원본 시각 해석 실패".to_string())
    };
    let birth = fields[7]
        .parse::<i64>()
        .ok()
        .filter(|_| !matches!(fields[17], "?" | "-" | ""));
    Ok(Attributes {
        remote: remote.into(),
        observed_at: chrono::Utc::now().to_rfc3339(),
        capture_context: String::new(),
        mode: u32::from_str_radix(fields[0], 16).map_err(|_| "원본 모드 해석 실패")?,
        uid: u32::try_from(number(1)?).map_err(|_| "원본 UID 범위 오류")?,
        gid: u32::try_from(number(2)?).map_err(|_| "원본 GID 범위 오류")?,
        size: number(3)?,
        access_time: time(4)?,
        modification_time: time(5)?,
        status_change_time: time(6)?,
        birth_time: birth,
        inode: number(8)?,
        hard_links: number(9)?,
        blocks_512: number(10)?,
        owner_name: fields[11].into(),
        group_name: fields[12].into(),
        security_context: (!matches!(fields[13], "?" | "-" | "")).then(|| fields[13].into()),
        access_time_text: fields[14].into(),
        modification_time_text: fields[15].into(),
        status_change_time_text: fields[16].into(),
        birth_time_text: birth
            .and_then(|_| (!matches!(fields[17], "?" | "-" | "")).then(|| fields[17].into())),
        payload_matches: None,
        backup_sha256: None,
    })
}

fn batch(dev: &mut dyn ADBDeviceExt, paths: &[String], cancel: &CancelFlag, out: &mut Snapshot) {
    if paths.is_empty() || cancel.cancelled() {
        out.issues.extend(paths.iter().map(|remote| Issue {
            remote: remote.clone(),
            error: "원본 속성 수집 취소".into(),
        }));
        return;
    }
    let command = format!(
        "stat -L -c {} -- {}",
        quote(FORMAT),
        paths.iter().map(|p| quote(p)).collect::<Vec<_>>().join(" ")
    );
    let result = crate::device_io::shell_run(dev, &command)
        .map_err(|error| format!("METADATA_TRANSPORT|{error}"))
        .and_then(|r| {
            if r.code != 0 {
                return Err(if r.stderr.is_empty() {
                    format!(
                        "stat 종료 코드 {}: {}",
                        r.code,
                        String::from_utf8_lossy(&r.stdout).trim()
                    )
                } else {
                    String::from_utf8_lossy(&r.stderr).into_owned()
                });
            }
            let text = String::from_utf8(r.stdout).map_err(|_| "원본 속성 응답 UTF-8 오류")?;
            let lines: Vec<_> = text.lines().collect();
            if lines.len() != paths.len() {
                return Err("원본 속성 응답이 누락되었습니다".into());
            }
            paths
                .iter()
                .zip(lines)
                .map(|(p, l)| parse(p, l))
                .collect::<Result<Vec<_>, _>>()
        });
    match result {
        Ok(mut entries) => {
            for entry in &mut entries {
                entry.capture_context = out.capture_context.clone();
            }
            out.entries.extend(entries);
        }
        Err(error)
            if paths.len() > 1
                && !error.contains("SYNC_BATCH_BROKEN|")
                && !error.starts_with("METADATA_TRANSPORT|") =>
        {
            let mid = paths.len() / 2;
            batch(dev, &paths[..mid], cancel, out);
            batch(dev, &paths[mid..], cancel, out);
        }
        Err(mut error) => {
            if paths.len() == 1 && !error.starts_with("METADATA_TRANSPORT|") {
                if error.trim_end().ends_with(": No such file or directory") {
                    error = format!("SOURCE_GONE|{error}");
                } else if error.starts_with("원본 속성")
                    || dev.stat(&paths[0]).is_ok_and(|stat| stat.file_perm != 0)
                {
                    out.format_failures += 1;
                    if out.format_failures >= 3 {
                        cancel.stop_with_error(format!(
                            "원본 속성 조회 실패가 반복되어 중단했습니다: {error}"
                        ));
                    }
                }
            }
            if error.contains("SYNC_BATCH_BROKEN|") || error.starts_with("METADATA_TRANSPORT|") {
                cancel.stop_with_error(format!("원본 속성 조회 중 기기 연결 오류: {error}"));
            }
            for remote in paths {
                out.issues.push(Issue {
                    remote: remote.clone(),
                    error: error.clone(),
                });
            }
        }
    }
}

pub fn capture(
    dev: &mut dyn ADBDeviceExt,
    id: &str,
    paths: Vec<String>,
    context: &str,
    cancel: &CancelFlag,
    progress: &mut dyn FnMut(u64, u64),
) -> Snapshot {
    let mut out = Snapshot { version: 1, item_id: id.into(), captured_at: chrono::Utc::now().to_rfc3339(), capture_context: context.into(),
        limitations: vec![
            "Attributes describe the shell-visible source filesystem, including synthesized emulated-storage attributes.".into(),
            "POSIX ctime is status-change time, not creation time. Unsupported birth time is null; it is never inferred from ctime or PC creation time.".into(),
            "ACLs, arbitrary extended attributes, and symlink targets are not captured by this collector. SELinux context is recorded when exposed.".into(),
            "Recording attributes does not grant permission to restore them. Old numeric UIDs/security labels must not be blindly replayed after installation.".into(),
            "This is not an atomic filesystem snapshot. Post-copy enrichment cannot recover original access times or attributes changed since backup.".into(),
        ], entries: vec![], issues: vec![], format_failures:0 };
    let paths: Vec<_> = paths
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let total = paths.len() as u64;
    let mut chunk = Vec::new();
    let mut bytes = FORMAT.len() + 32;
    let mut done = 0;
    for path in paths {
        if cancel.cancelled() {
            out.issues.push(Issue {
                remote: path,
                error: "원본 속성 수집 취소".into(),
            });
            continue;
        }
        if super::paths::archive_relative(&path).is_err() {
            out.issues.push(Issue {
                remote: path,
                error: "원본 속성 경로 오류".into(),
            });
            continue;
        }
        let cost = quote(&path).len() + 1;
        if !chunk.is_empty() && (chunk.len() >= MAX_PATHS || bytes + cost > MAX_COMMAND) {
            batch(dev, &chunk, cancel, &mut out);
            done += chunk.len() as u64;
            progress(done, total);
            chunk.clear();
            bytes = FORMAT.len() + 32;
        }
        bytes += cost;
        chunk.push(path);
    }
    batch(dev, &chunk, cancel, &mut out);
    progress(total, total);
    if cancel.cancelled() && out.issues.is_empty() {
        out.issues.push(Issue {
            remote: String::new(),
            error: "원본 속성 수집 취소".into(),
        });
    }
    out.entries.sort_by(|a, b| a.remote.cmp(&b.remote));
    out
}

pub fn associate(snapshot: &mut Snapshot, entries: &[FileEntry]) {
    let files: BTreeMap<_, _> = entries.iter().map(|e| (e.remote.as_str(), e)).collect();
    for attr in &mut snapshot.entries {
        attr.payload_matches = None;
        attr.backup_sha256 = None;
        if let Some(file) = files.get(attr.remote.as_str()) {
            let matches = attr.mode & 0o170000 == 0o100000
                && attr.size == file.size
                && (attr.modification_time as u32) == file.mtime
                && file.error.is_none();
            attr.payload_matches = Some(matches);
            attr.backup_sha256 = matches.then(|| file.sha256.clone()).flatten();
        }
    }
}

/// Receipts written before "a stale pre-copy attribute is not a failure" keep complete=false even
/// though their payload is verified. Re-judge them from their own (hash-checked) sidecar so a
/// resumed backup does not stay incomplete for that reason alone. Unreadable sidecars are left as is.
pub fn rejudge_receipts(root: &Path, manifest: &mut Manifest) -> bool {
    let mut changed = false;
    for receipt in manifest.source_metadata.iter_mut().filter(|r| !r.complete) {
        let Ok(snapshot) = load(root, receipt) else {
            continue;
        };
        if let Ok(updated) = publish(root, &snapshot) {
            changed |= updated.complete != receipt.complete || updated.errors != receipt.errors;
            *receipt = updated;
        }
    }
    changed
}

/// The backup is each file as it was at its copy moment. When an app rewrote a file after the
/// pre-copy stat, describe the copied version instead: re-stat only those files right after the
/// copy and, when the size now matches the received bytes, record that observation and its mtime
/// (also on the PC copy). Never re-pulls and never fails the backup.
pub fn refresh_copied(
    dev: &mut dyn ADBDeviceExt,
    root: &Path,
    snapshot: &mut Snapshot,
    entries: &mut [FileEntry],
    cancel: &CancelFlag,
) {
    let stale: BTreeSet<String> = {
        let files: BTreeMap<_, _> = entries.iter().map(|e| (e.remote.as_str(), e)).collect();
        snapshot
            .entries
            .iter()
            .filter(|a| a.mode & 0o170000 == 0o100000)
            .filter(|a| {
                files.get(a.remote.as_str()).is_some_and(|f| {
                    f.error.is_none()
                        && f.sha256.is_some()
                        && (a.size != f.size || (a.modification_time as u32) != f.mtime)
                })
            })
            .map(|a| a.remote.clone())
            .collect()
    };
    if stale.is_empty() || cancel.cancelled() {
        return;
    }
    let current = capture(
        dev,
        &snapshot.item_id,
        stale.into_iter().collect(),
        "copy-time",
        cancel,
        &mut |_, _| {},
    );
    let observed: BTreeMap<_, _> = current.entries.into_iter().map(|a| (a.remote.clone(), a)).collect();
    for entry in entries.iter_mut() {
        let Some(attr) = observed.get(&entry.remote) else {
            continue;
        };
        // Changed again after the copy: the observation does not describe the copy; keep the old one.
        if attr.mode & 0o170000 != 0o100000 || attr.size != entry.size {
            continue;
        }
        let mtime = attr.modification_time as u32;
        // A quarantined copy keeps the mtime in its tar header; only plain PC files are re-timed.
        if mtime != entry.mtime && !entry.quarantined {
            let applied = super::paths::existing_file(root, &entry.local).and_then(|path| {
                filetime::set_file_mtime(path, filetime::FileTime::from_unix_time(mtime as i64, 0))
                    .map_err(|e| e.to_string())
            });
            if applied.is_err() {
                continue; // PC copy keeps its recorded time; the old observation stays.
            }
            entry.mtime = mtime;
        }
        if let Some(slot) = snapshot.entries.iter_mut().find(|a| a.remote == entry.remote) {
            *slot = attr.clone();
        }
    }
}

pub fn publish(root: &Path, snapshot: &Snapshot) -> Result<Receipt, String> {
    super::runner::validate_items(&[snapshot.item_id.clone()])?;
    let bytes = serde_json::to_vec_pretty(snapshot).map_err(|e| e.to_string())?;
    let digest = crate::boot_image::sha256(&bytes);
    let relative = format!(
        "source-metadata/{}-{}.json",
        snapshot.item_id,
        &digest[..16]
    );
    let path = super::paths::write_target(root, &relative)?;
    crate::storage::atomic_write(&path, &bytes)?;
    Ok(Receipt {
        capture_context:snapshot.capture_context.clone(),
        item_id: snapshot.item_id.clone(),
        path: relative,
        sha256: digest,
        entries: snapshot.entries.len() as u64,
        directories: snapshot
            .entries
            .iter()
            .filter(|e| e.mode & 0o170000 == 0o040000)
            .count() as u64,
        unavailable_birth_times: snapshot
            .entries
            .iter()
            .filter(|e| e.birth_time.is_none())
            .count() as u64,
        payload_mismatches: snapshot
            .entries
            .iter()
            .filter(|e| e.payload_matches == Some(false))
            .count() as u64,
        // A running app may rewrite a file between the pre-copy stat and the copy. The copy itself
        // is size/hash verified, so a stale attribute is a recorded observation, never a failure.
        complete: snapshot.issues.iter().all(|i|snapshot.capture_context=="before-copy" && i.error.starts_with("SOURCE_GONE|")),
        errors: snapshot
            .issues
            .iter()
            .filter(|i|snapshot.capture_context!="before-copy" || !i.error.starts_with("SOURCE_GONE|"))
            .map(|i| format!("{}: 원본 속성 조회 실패: {}", i.remote, i.error))
            .collect(),
    })
}

pub fn load(root: &Path, receipt: &Receipt) -> Result<Snapshot, String> {
    let bytes = std::fs::read(super::paths::existing_file(root, &receipt.path)?)
        .map_err(super::paths::read_error)?;
    if crate::boot_image::sha256(&bytes) != receipt.sha256 {
        return Err("원본 속성 기록 해시 불일치".into());
    }
    let snapshot: Snapshot = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
    if snapshot.version != 1 || snapshot.item_id != receipt.item_id {
        return Err("원본 속성 기록 항목 불일치".into());
    }
    Ok(snapshot)
}

pub fn prune_omitted(root: &Path, manifest: &mut Manifest) -> Result<(), String> {
    let omitted: BTreeSet<_> = manifest
        .omitted_apps
        .iter()
        .map(|app| app.package.as_str())
        .collect();
    if omitted.is_empty() {
        return Ok(());
    }
    for receipt in &mut manifest.source_metadata {
        if receipt.item_id != "app-data" {
            continue;
        }
        let mut snapshot = match load(root, receipt) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                receipt.complete = false;
                receipt.errors = vec![format!("원본 속성 기록을 다시 수집해야 합니다: {error}")];
                continue;
            }
        };
        snapshot
            .entries
            .retain(|e| !super::omissions::package(&e.remote).is_some_and(|p| omitted.contains(p)));
        snapshot
            .issues
            .retain(|e| !super::omissions::package(&e.remote).is_some_and(|p| omitted.contains(p)));
        *receipt = publish(root, &snapshot)?;
    }
    Ok(())
}

/// Reads only source attributes. No file pull, install, chmod, or device deletion.
#[cfg(any(feature = "dev-cli", test))]
pub fn enrich(
    dev: &mut dyn ADBDeviceExt,
    root: &Path,
    cancel: &CancelFlag,
    progress: &mut super::runner::ProgressSink,
) -> Result<Vec<Receipt>, String> {
    let mut manifest = super::model::load_manifest(root)?;
    let key = crate::device_io::identity_key(dev)?;
    if manifest.device_key.as_deref() != Some(key.as_str()) {
        return Err("원본 속성을 수집할 기기가 백업 원본과 다릅니다".into());
    }
    let items: Vec<_> = manifest
        .items
        .iter()
        .filter(|i| manifest.is_selected(i) && i.kind == super::model::ItemKind::Files)
        .cloned()
        .collect();
    for item in &items {
        if cancel.cancelled() {
            return Err("원본 속성 수집 취소".into());
        }
        // Never overwrite an original before-copy snapshot with a later observation.
        if manifest
            .source_metadata
            .iter()
            .any(|r| r.item_id == item.id && (r.required() || r.complete))
        {
            continue;
        }
        let mut paths: Vec<_> = item.entries.iter().map(|e| e.remote.clone()).collect();
        let roots: Vec<_> = if item.id == "apk" {
            item.entries
                .iter()
                .filter_map(|e| e.remote.rsplit_once('/').map(|(p, _)| p.to_string()))
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect()
        } else {
            vec![match item.id.as_str() {
                "app-data" => super::walker::ANDROID_DATA_ROOT.into(),
                "fs-rest" => super::walker::FS_REST_ROOT.into(),
                id => format!(
                    "/sdcard/{}",
                    super::walker::NAMED_FILE_ITEMS
                        .iter()
                        .find(|(name, _)| *name == id)
                        .ok_or("알 수 없는 파일 항목")?
                        .1
                ),
            }]
        };
        let mut scan_issues = Vec::new();
        for remote_root in roots {
            let walk = super::walker::walk_cancellable(
                dev,
                &remote_root,
                &|path| {
                    (item.id == "fs-rest" && super::walker::fs_rest_skip(path))
                        || (item.id == "app-data"
                            && super::omissions::package(path).is_some_and(|p| {
                                manifest.omitted_apps.iter().any(|a| a.package == p)
                            }))
                },
                cancel,
            );
            // Include current empty folders as enrichment, not as original backup evidence.
            paths.extend(walk.directories);
            scan_issues.extend(walk.errors.into_iter().map(|error| Issue {
                remote: remote_root.clone(),
                error,
            }));
        }
        let mut snapshot = capture(
            dev,
            &item.id,
            paths,
            "after-copy-enrichment",
            cancel,
            &mut |done, total| {
                progress(super::runner::StepProgress::at(
                    &item.id, "metadata", done, total,
                ))
            },
        );
        snapshot.issues.extend(scan_issues);
        associate(&mut snapshot, &item.entries);
        let receipt = publish(root, &snapshot)?;
        manifest
            .source_metadata
            .retain(|r| r.item_id != receipt.item_id);
        manifest.source_metadata.push(receipt);
        manifest.touch();
        super::model::save_manifest_atomic(&manifest, root)?;
    }
    Ok(manifest.source_metadata)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::{
        fake_device::FakeADBDevice,
        model::{ItemKind, ItemRecord, ItemStatus},
    };
    #[test]
    fn ctime_is_never_used_as_birth_time_and_precision_is_retained() {
        let line="81a0\t10123\t1078\t5000000000\t100\t200\t300\t?\t42\t2\t100\tu0_a123\text_data_rw\tu:object_r:fuse:s0\tA.123456789\tM.987654321\tC.456\t?";
        let attr = parse("/sdcard/x", line).unwrap();
        assert_eq!(attr.mode, 0o100640);
        assert_eq!(attr.uid, 10123);
        assert_eq!(attr.size, 5_000_000_000);
        assert_eq!(attr.status_change_time, 300);
        assert_eq!(attr.birth_time, None);
        assert_eq!(attr.modification_time_text, "M.987654321");
        let birth = parse(
            "/sdcard/x",
            &line
                .replace("\t?\t42", "\t50\t42")
                .replace("\t?", "\tB.789"),
        )
        .unwrap();
        assert_eq!(birth.birth_time, Some(50));
        assert_eq!(birth.birth_time_text.as_deref(), Some("B.789"));
    }
    #[test]
    fn one_missing_path_does_not_shift_attributes_and_weird_names_are_literal() {
        let mut dev = FakeADBDevice::new();
        dev.add_file("/sdcard/a missing", b"a", 11, 0o600);
        dev.add_file("/sdcard/c '\n$(x);\\", b"cc", 22, 0o640);
        let snapshot = capture(
            &mut dev,
            "dcim",
            vec![
                "/sdcard/a missing".into(),
                "/sdcard/b gone".into(),
                "/sdcard/c '\n$(x);\\".into(),
            ],
            "before-copy",
            &CancelFlag::new(),
            &mut |_, _| {},
        );
        assert_eq!(snapshot.entries.len(), 2);
        assert_eq!(snapshot.issues.len(), 1);
        assert_eq!(snapshot.entries[1].remote, "/sdcard/c '\n$(x);\\");
        assert_eq!(snapshot.entries[1].modification_time, 22);
        assert!(dev.pull_calls.is_empty());
    }
    #[test]
    fn hundreds_of_paths_are_batched_and_cancel_never_contacts_device() {
        let mut dev = FakeADBDevice::new();
        let paths: Vec<_> = (0..300).map(|n| format!("/sdcard/f{n}")).collect();
        for p in &paths {
            dev.add_file(p, b"", 100, 0o600);
        }
        let snapshot = capture(
            &mut dev,
            "dcim",
            paths.clone(),
            "before-copy",
            &CancelFlag::new(),
            &mut |_, _| {},
        );
        assert!(snapshot.issues.is_empty());
        assert_eq!(dev.shell_calls.len(), 3);
        dev.shell_calls.clear();
        let cancel = CancelFlag::new();
        cancel.set();
        assert!(!capture(
            &mut dev,
            "dcim",
            paths,
            "before-copy",
            &cancel,
            &mut |_, _| {}
        )
        .issues
        .is_empty());
        assert!(dev.shell_calls.is_empty());
    }
    #[test]
    fn original_snapshot_contains_empty_folders_and_tamper_is_detected() {
        let dir = tempfile::tempdir().unwrap();
        let mut dev = FakeADBDevice::new();
        dev.answer_shell(
            "getprop",
            "[ro.product.model]: [test]\n[ro.serialno]: [test-phone]\n",
        );
        dev.add_dir("/sdcard/Download/empty");
        dev.add_file("/sdcard/Download/f", b"ok", 200, 0o600);
        let mut sink: crate::backup::runner::ProgressSink = Box::new(|_| {});
        let result = crate::backup::runner::run_backup_items(
            &mut dev,
            &["download".into()],
            dir.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&result.dir);
        let manifest = crate::backup::model::load_manifest(root).unwrap();
        let receipt = &manifest.source_metadata[0];
        assert_eq!(receipt.directories, 2);
        assert!(receipt.complete);
        let snapshot = load(root, receipt).unwrap();
        assert_eq!(snapshot.capture_context, "before-copy");
        assert!(snapshot
            .entries
            .iter()
            .any(|e| e.remote.ends_with("/empty") && e.mode & 0o170000 == 0o040000));
        assert!(snapshot
            .entries
            .iter()
            .any(|e| e.backup_sha256.is_some() && e.payload_matches == Some(true)));
        std::fs::write(root.join(&receipt.path), b"{}").unwrap();
        assert!(load(root, receipt).is_err());
    }

    #[test]
    fn repeated_attribute_format_failure_keeps_its_real_stop_reason() {
        let mut dev = FakeADBDevice::new();
        let cancel = CancelFlag::new();
        let mut out = capture(
            &mut dev,
            "download",
            vec![],
            "before-copy",
            &cancel,
            &mut |_, _| {},
        );
        for n in 0..3 {
            let path = format!("/sdcard/Download/{n}");
            dev.answer_shell(
                &format!("stat -L -c {} -- {}", quote(FORMAT), quote(&path)),
                "malformed\n",
            );
            batch(&mut dev, &[path], &cancel, &mut out);
        }
        assert!(cancel.cancelled());
        assert!(cancel.reason().contains("원본 속성 조회 실패가 반복"));
        assert!(!cancel.reason().contains("사용자"));
    }
    #[test]
    fn enrichment_does_not_pull_or_replace_original_attributes_and_rejects_other_device() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        manifest.device_key = Some(crate::boot_image::sha256(b"phone"));
        let mut item = ItemRecord::new("download", ItemKind::Files);
        item.status = ItemStatus::Done;
        manifest.record(item);
        crate::backup::model::save_manifest_atomic(&manifest, root.path()).unwrap();
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("getprop ro.serialno", "phone");
        dev.add_dir("/sdcard/Download/empty");
        let mut sink: crate::backup::runner::ProgressSink = Box::new(|_| {});
        let receipts = enrich(&mut dev, root.path(), &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(
            load(root.path(), &receipts[0]).unwrap().capture_context,
            "after-copy-enrichment"
        );
        let bytes = std::fs::read(root.path().join(&receipts[0].path)).unwrap();
        dev.now += 1;
        enrich(&mut dev, root.path(), &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(
            std::fs::read(root.path().join(&receipts[0].path)).unwrap(),
            bytes
        );
        assert!(dev.pull_calls.is_empty());
        let mut other = FakeADBDevice::new();
        other.answer_shell("getprop ro.serialno", "other");
        assert!(enrich(&mut other, root.path(), &CancelFlag::new(), &mut sink).is_err());
        assert!(other.list_calls.is_empty());
    }
    #[test]
    fn incomplete_enrichment_never_blocks_payload_and_can_be_retried() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        manifest.device_key = Some(crate::boot_image::sha256(b"phone"));
        let mut item = ItemRecord::new("download", ItemKind::Files);
        item.status = ItemStatus::Done;
        item.entries.push(FileEntry {
            remote: "/sdcard/Download/a".into(),
            local: "a".into(),
            size: 3,
            mtime: 42,
            sha256: Some(crate::boot_image::sha256(b"abc")),
            quarantined: false,
            error: None,
        });
        item.bytes = 3;
        item.files = 1;
        manifest.record(item);
        std::fs::write(root.path().join("a"), b"abc").unwrap();
        crate::backup::model::save_manifest_atomic(&manifest, root.path()).unwrap();
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("getprop ro.serialno", "phone");
        dev.add_dir("/sdcard/Download");
        let mut sink: crate::backup::runner::ProgressSink = Box::new(|_| {});
        let receipts = enrich(&mut dev, root.path(), &CancelFlag::new(), &mut sink).unwrap();
        assert!(!receipts[0].complete);
        let mut saved = crate::backup::model::load_manifest(root.path()).unwrap();
        assert!(saved.complete());
        assert!(crate::backup::verify::verify_manifest(root.path(), &mut saved).is_empty());
        dev.add_file("/sdcard/Download/a", b"abc", 42, 0o644);
        let receipts = enrich(&mut dev, root.path(), &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(receipts.len(), 1);
        assert!(receipts[0].complete);
        assert!(dev.pull_calls.is_empty());
    }

    #[test]
    fn file_rewritten_by_its_app_before_copy_is_described_by_the_copied_version() {
        // 실기기 사례: 복사 직전 stat은 1731바이트, 앱이 다시 써서 실제 사본은 479바이트(새 mtime)
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("copy"), vec![1; 479]).unwrap();
        let mut dev = FakeADBDevice::new();
        dev.add_file("/sdcard/Android/data/app/x", &[1; 479], 2_000, 0o660);
        let stale = parse(
            "/sdcard/Android/data/app/x",
            "81b0\t10433\t1078\t1731\t1000\t1000\t1000\t?\t42\t1\t8\tu0_a433\text_data_rw\tu:object_r:fuse:s0\tA\tM\tC\t?",
        )
        .unwrap();
        let mut snapshot = Snapshot {
            version: 1,
            item_id: "app-data".into(),
            captured_at: String::new(),
            capture_context: "before-copy".into(),
            limitations: vec![],
            entries: vec![stale],
            issues: vec![],
            format_failures: 0,
        };
        // 사본 기록의 mtime은 앞선 목록 조회 값(옛 시각)이다
        let mut entries = vec![FileEntry {
            remote: "/sdcard/Android/data/app/x".into(),
            local: "copy".into(),
            size: 479,
            mtime: 1_000,
            sha256: Some("ab".repeat(32)),
            quarantined: false,
            error: None,
        }];
        refresh_copied(&mut dev, root.path(), &mut snapshot, &mut entries, &CancelFlag::new());
        associate(&mut snapshot, &entries);
        assert_eq!(snapshot.entries[0].size, 479);
        assert_eq!(snapshot.entries[0].capture_context, "copy-time");
        assert_eq!(entries[0].mtime, 2_000);
        let meta = std::fs::metadata(root.path().join("copy")).unwrap();
        assert_eq!(filetime::FileTime::from_last_modification_time(&meta).unix_seconds(), 2_000);
        assert_eq!(snapshot.entries[0].payload_matches, Some(true));
        assert!(dev.pull_calls.is_empty(), "the copy is never pulled again");
        // 앱이 복사 뒤에 또 바꿔 크기가 다르면 기록만 다를 뿐 백업은 실패하지 않는다
        let receipt = publish(root.path(), &snapshot).unwrap();
        assert!(receipt.complete);
        entries[0].size = 5;
        associate(&mut snapshot, &entries);
        let receipt = publish(root.path(), &snapshot).unwrap();
        assert!(receipt.complete && receipt.errors.is_empty() && receipt.payload_mismatches == 1);
        // 예전 규칙으로 저장된 기록(불일치=미완결)은 재개 시 새 규칙으로 다시 판정된다
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        let mut old = receipt.clone();
        old.complete = false;
        old.errors = vec!["/sdcard/Android/data/app/x: 원본 속성과 수집 사본의 크기·수정 시각이 달라 일치를 확인하지 못했습니다".into()];
        manifest.source_metadata.push(old);
        assert!(rejudge_receipts(root.path(), &mut manifest));
        assert!(manifest.source_metadata[0].complete && manifest.source_metadata[0].errors.is_empty());
    }
}
