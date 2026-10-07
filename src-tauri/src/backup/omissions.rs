//! Unreadable app data is excluded as a whole package, while APKs remain.
//! Publish exclusions before deletion so interruption cannot restore half an app.
use super::model::{save_manifest_atomic, ItemStatus, Manifest, OmittedApp};
use super::{paths, quarantine, verify};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub(super) fn package(remote: &str) -> Option<&str> {
    let rest = remote.strip_prefix("/sdcard/Android/data/")?;
    let name = rest.split('/').next()?;
    (name.contains('.')
        && name.split('.').all(|part| {
            !part.is_empty() && part.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        }))
    .then_some(name)
}

fn source_denial(error: &str) -> bool {
    error.contains("Permission denied")
        && (error.contains("SYNC RECV failed:") || error.contains("폴더를 읽을 수 없습니다(권한)"))
}

fn error_package(error: &str) -> Option<&str> {
    let (remote, _) = error.split_once(": ")?;
    source_denial(error).then(|| package(remote)).flatten()
}

fn reparse(metadata: &std::fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
}

/// Check the resolved absolute tree before any recursive removal, including children.
fn inspect_tree(root: &Path, path: &Path) -> Result<(), String> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    if reparse(&metadata)
        || !path
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(root)
    {
        return Err("앱 백업 정리 경로가 링크이거나 지정한 백업 밖입니다".into());
    }
    if metadata.is_dir() {
        for entry in std::fs::read_dir(path).map_err(|e| e.to_string())? {
            inspect_tree(root, &entry.map_err(|e| e.to_string())?.path())?;
        }
    } else if !metadata.is_file() {
        return Err("앱 백업에 일반 파일이 아닌 항목이 있습니다".into());
    }
    Ok(())
}

fn remove_tree(root: &Path, path: &Path) -> Result<(), String> {
    inspect_tree(root, path)?;
    if !path.exists() {
        return Ok(());
    }
    if !path.is_dir() {
        return Err("앱 백업 폴더가 일반 폴더가 아닙니다".into());
    }
    // Only this prevalidated, absolute app folder is removed, never the backup root.
    std::fs::remove_dir_all(path)
        .map_err(|error| format!("앱 백업 삭제 실패: {}", crate::storage::io_error(&error)))
}

fn rewrite_segment(segment: &Path, omitted: &BTreeSet<String>) -> Result<(), String> {
    let mut archive = tar::Archive::new(std::fs::File::open(segment).map_err(|e| e.to_string())?);
    let mut matched = false;
    for entry in archive.entries_with_seek().map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let remote = verify::tar_remote(&entry);
        paths::archive_relative(&remote)?;
        if !entry.header().entry_type().is_file() {
            return Err("격리 tar에 일반 파일이 아닌 항목이 있습니다".into());
        }
        matched |= package(&remote).is_some_and(|name| omitted.contains(name));
    }
    drop(archive);
    if !matched {
        return Ok(());
    }
    crate::storage::atomic_file(segment, |file, _| {
        let mut archive =
            tar::Archive::new(std::fs::File::open(segment).map_err(|e| e.to_string())?);
        let mut builder = tar::Builder::new(std::io::BufWriter::with_capacity(256 * 1024, file));
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let remote = verify::tar_remote(&entry);
            let name = paths::archive_relative(&remote)?;
            if package(&remote).is_some_and(|name| omitted.contains(name)) {
                continue;
            }
            let mut header = entry.header().clone();
            if !header.entry_type().is_file() {
                return Err("격리 tar에 일반 파일이 아닌 항목이 있습니다".into());
            }
            let size = header.size().map_err(|e| e.to_string())?;
            quarantine::append_unix_path(
                &mut builder,
                &mut header,
                name,
                quarantine::ExactReader::new(&mut entry, size),
            )
            .map_err(|e| e.to_string())?;
        }
        use std::io::Write;
        builder
            .into_inner()
            .map_err(|e| e.to_string())?
            .flush()
            .map_err(|e| e.to_string())
    })
}

/// Diagnostic copies are not restore payloads. Exclusions must remove their data too.
fn clean_recovery_history(root: &Path, omitted: &BTreeSet<String>) -> Result<(), String> {
    let directory = root.join("recovery");
    inspect_tree(root, &directory)?;
    if !directory.exists() {
        return Ok(());
    }
    for file in std::fs::read_dir(&directory).map_err(|e| e.to_string())? {
        let file = file.map_err(|e| e.to_string())?;
        let name = file.file_name().to_string_lossy().into_owned();
        let generated = name
            .strip_prefix("seg")
            .and_then(|n| n.strip_suffix(".damaged"))
            .and_then(|n| n.split_once(".tar-"))
            .is_some_and(|(seg, seq)| {
                !seg.is_empty()
                    && !seq.is_empty()
                    && seg.bytes().chain(seq.bytes()).all(|c| c.is_ascii_digit())
            });
        if !generated {
            continue;
        }
        let path = paths::existing_file(root, &format!("recovery/{name}"))?;
        let matched = (|| -> Result<bool, String> {
            let mut archive =
                tar::Archive::new(std::fs::File::open(&path).map_err(|e| e.to_string())?);
            for entry in archive.entries_with_seek().map_err(|e| e.to_string())? {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        let message = super::recovery::archive_error(error);
                        if message.starts_with("CORRUPT_ARCHIVE|") {
                            return Ok(true);
                        }
                        return Err(message);
                    }
                };
                let remote = verify::tar_remote(&entry);
                if package(&remote).is_some_and(|p| omitted.contains(p)) {
                    return Ok(true);
                }
            }
            Ok(false)
        })()?;
        // A damaged diagnostic tail cannot prove absence of the excluded package.
        // The durable manifest and validated active segments remain the restore sources.
        if matched {
            std::fs::remove_file(&path).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub(super) fn clean(root: &Path, manifest: &mut Manifest) -> Result<(), String> {
    let mut affected = BTreeSet::new();
    if let Some(item) = manifest
        .items
        .iter()
        .find(|item| item.id == "app-data" && manifest.is_selected(item))
    {
        for error in &item.errors {
            if let Some(name) = error_package(error) {
                affected.insert(name.to_owned());
            }
        }
        for entry in &item.entries {
            if entry.error.as_deref().is_some_and(source_denial) {
                if let Some(name) = package(&entry.remote) {
                    affected.insert(name.to_owned());
                }
            }
        }
    }
    for app in &manifest.omitted_apps {
        if package(&format!("/sdcard/Android/data/{}/", app.package)) != Some(app.package.as_str())
        {
            return Err("잘못된 제외 앱 패키지 이름입니다".into());
        }
        if app.cleanup_pending {
            affected.insert(app.package.clone());
        }
    }
    if affected.is_empty() {
        return Ok(());
    }
    if reparse(&std::fs::symlink_metadata(root).map_err(|e| e.to_string())?) {
        return Err("링크인 백업 폴더는 정리할 수 없습니다".into());
    }
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    if !root
        .file_name()
        .is_some_and(|name| super::runner::is_backup_dir_name(&name.to_string_lossy()))
    {
        return Err("이 앱이 만든 백업 폴더만 정리할 수 있습니다".into());
    }
    for relative in ["android-data", "quarantine"] {
        let parent = root.join(relative);
        if parent.exists()
            && reparse(&std::fs::symlink_metadata(&parent).map_err(|e| e.to_string())?)
        {
            return Err("링크인 앱 백업 저장 위치는 정리할 수 없습니다".into());
        }
    }
    inspect_tree(&root, &root.join("quarantine"))?;
    let mut directories: Vec<PathBuf> = Vec::new();
    for name in &affected {
        let directory = paths::write_target(&root, &format!("android-data/{name}"))?;
        if directory.exists()
            && directory
                .canonicalize()
                .map_err(|e| e.to_string())?
                .file_name()
                .and_then(|n| n.to_str())
                != Some(name.as_str())
        {
            return Err("제외 앱과 PC 폴더의 대소문자가 달라 삭제하지 않습니다".into());
        }
        inspect_tree(&root, &directory)?;
        directories.push(directory);
    }
    if let Some(item) = manifest.items.iter().find(|item| item.id == "app-data") {
        for entry in &item.entries {
            if package(&entry.remote).is_some_and(|name| affected.contains(name))
                && !entry.quarantined
                && entry.error.is_none()
            {
                let local = paths::relative_path(&entry.local)?;
                let expected =
                    PathBuf::from(format!("android-data/{}", package(&entry.remote).unwrap()));
                if !local.starts_with(expected) {
                    return Err("앱 데이터 기록이 다른 백업 경로를 가리킵니다".into());
                }
            }
        }
    }
    if let Some(item) = manifest.items.iter_mut().find(|item| item.id == "app-data") {
        for name in &affected {
            if !manifest.omitted_apps.iter().any(|app| &app.package == name) {
                let entries: Vec<_> = item
                    .entries
                    .iter()
                    .filter(|entry| {
                        package(&entry.remote) == Some(name.as_str()) && entry.error.is_none()
                    })
                    .collect();
                manifest.omitted_apps.push(OmittedApp {
                    package: name.clone(),
                    reasons: item
                        .errors
                        .iter()
                        .filter(|error| error_package(error) == Some(name.as_str()))
                        .cloned()
                        .chain(item.entries.iter().filter_map(|entry| {
                            (package(&entry.remote) == Some(name.as_str()))
                                .then(|| entry.error.as_ref())
                                .flatten()
                                .filter(|error| source_denial(error))
                                .map(|error| format!("{}: {error}", entry.remote))
                        }))
                        .collect::<BTreeSet<_>>()
                        .into_iter()
                        .collect(),
                    removed_files: entries.len() as u64,
                    removed_bytes: entries.iter().map(|entry| entry.size).sum(),
                    cleanup_pending: true,
                });
            }
        }
        item.entries
            .retain(|entry| !package(&entry.remote).is_some_and(|name| affected.contains(name)));
        item.errors
            .retain(|error| !error_package(error).is_some_and(|name| affected.contains(name)));
        item.files = item.entries.len() as u32;
        item.bytes = item
            .entries
            .iter()
            .filter(|entry| entry.error.is_none())
            .map(|entry| entry.size)
            .sum();
        if item.status != ItemStatus::Pending {
            item.finalize();
        }
    }
    for app in &mut manifest.omitted_apps {
        if affected.contains(&app.package) {
            app.cleanup_pending = true;
        }
    }
    manifest.touch();
    save_manifest_atomic(manifest, &root)?; // incomplete until all PC deletion succeeds
    for directory in &directories {
        remove_tree(&root, directory)?;
    }
    let omitted: BTreeSet<_> = manifest
        .omitted_apps
        .iter()
        .map(|app| app.package.clone())
        .collect();
    super::recovery::repair_segments(&root, manifest)?;
    for segment in verify::segments(&root)? {
        rewrite_segment(&segment, &omitted)?;
    }
    clean_recovery_history(&root, &omitted)?;
    super::source_metadata::prune_omitted(&root, manifest)?;
    for app in &mut manifest.omitted_apps {
        if affected.contains(&app.package) {
            app.cleanup_pending = false;
        }
    }
    manifest.touch();
    save_manifest_atomic(manifest, &root)
}

#[cfg(test)]
mod tests {
    use super::super::model::{load_manifest, FileEntry, ItemKind, ItemRecord};
    use super::*;
    use sha2::{Digest, Sha256};

    fn fixture() -> (tempfile::TempDir, Manifest) {
        let root = tempfile::Builder::new()
            .prefix("backup-omissions-")
            .tempdir()
            .unwrap();
        let mut manifest = Manifest::new("test-model", "TEST****", "test-fw", "15");
        let mut item = ItemRecord::new("app-data", ItemKind::Files);
        for name in ["org.example.blocked", "org.example.keep"] {
            let local = format!("android-data/{name}/files/data");
            let path = root.path().join(&local);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, b"data").unwrap();
            item.entries.push(FileEntry {
                remote: format!("/sdcard/Android/data/{name}/files/data"),
                local,
                size: 4,
                mtime: 1,
                sha256: Some(format!("{:x}", Sha256::digest(b"data"))),
                quarantined: false,
                error: None,
            });
        }
        item.files = 2;
        item.bytes = 8;
        item.errors.push("/sdcard/Android/data/org.example.blocked/private: 폴더를 읽을 수 없습니다(권한) — Permission denied".into());
        item.finalize();
        manifest.record(item);
        std::fs::create_dir_all(root.path().join("apk/org.example.blocked")).unwrap();
        std::fs::write(root.path().join("apk/org.example.blocked/base.apk"), b"APK").unwrap();
        (root, manifest)
    }

    #[test]
    fn no_exclusions_do_not_require_a_deletion_safe_root_name() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        clean(root.path(), &mut manifest).unwrap();
        assert!(!root.path().join("manifest.json").exists());
    }
    #[test]
    fn removes_whole_app_keeps_other_app_and_apk_and_records_omission() {
        let (root, mut manifest) = fixture();
        // A copy not present in the receipt must also be removed.
        std::fs::write(
            root.path()
                .join("android-data/org.example.blocked/unlisted"),
            b"old",
        )
        .unwrap();
        clean(root.path(), &mut manifest).unwrap();
        assert!(!root
            .path()
            .join("android-data/org.example.blocked")
            .exists());
        assert_eq!(
            std::fs::read(root.path().join("android-data/org.example.keep/files/data")).unwrap(),
            b"data"
        );
        assert_eq!(
            std::fs::read(root.path().join("apk/org.example.blocked/base.apk")).unwrap(),
            b"APK"
        );
        assert_eq!(manifest.items[0].entries.len(), 1);
        assert_eq!(manifest.items[0].bytes, 4);
        assert!(manifest.complete());
        let app = &manifest.omitted_apps[0];
        assert_eq!(app.package, "org.example.blocked");
        assert_eq!((app.removed_files, app.removed_bytes), (1, 4));
        assert_eq!(app.reasons.len(), 1);
        assert!(!app.cleanup_pending);
        assert!(verify::verify_manifest(root.path(), &mut manifest).is_empty());
        let saved = std::fs::read(root.path().join("manifest.json")).unwrap();
        clean(root.path(), &mut manifest).unwrap();
        assert_eq!(
            saved,
            std::fs::read(root.path().join("manifest.json")).unwrap()
        );
    }

    #[test]
    fn pc_denials_transport_and_integrity_errors_are_not_hidden() {
        let (root, mut manifest) = fixture();
        let integrity = "/sdcard/Android/data/org.example.blocked/files/data: 크기/해시 불일치";
        manifest.items[0].errors.extend([
            integrity.into(),
            "PC 파일 쓰기 실패: Permission denied".into(),
            "SYNC_BATCH_BROKEN".into(),
        ]);
        clean(root.path(), &mut manifest).unwrap();
        assert!(!manifest.complete());
        assert!(manifest.items[0].errors.contains(&integrity.to_owned()));
        assert_eq!(manifest.items[0].errors.len(), 3);
        assert_eq!(manifest.items[0].status, ItemStatus::Partial);
        assert_eq!(manifest.omitted_apps.len(), 1);
    }

    #[test]
    fn deselected_item_is_not_deleted() {
        let (root, mut manifest) = fixture();
        manifest.excluded_items.push("app-data".into());
        clean(root.path(), &mut manifest).unwrap();
        assert!(root
            .path()
            .join("android-data/org.example.blocked/files/data")
            .exists());
        assert!(manifest.omitted_apps.is_empty());
    }

    #[test]
    fn foreign_local_path_is_rejected_before_deletion_or_manifest_change() {
        let (root, mut manifest) = fixture();
        manifest.items[0].entries[0].local = "apk/org.example.blocked/base.apk".into();
        assert!(clean(root.path(), &mut manifest).is_err());
        assert!(root
            .path()
            .join("android-data/org.example.blocked/files/data")
            .exists());
        assert!(manifest.omitted_apps.is_empty());
        for path in [
            "/sdcard/Android/data/../keep",
            "/sdcard/Android/data/org.example.blocked-other/data",
            "/sdcard/Android/obb/org.example.blocked/data",
        ] {
            assert!(package(path).is_none());
        }
    }

    #[cfg(windows)]
    #[test]
    fn junction_to_another_folder_is_rejected_before_recursive_deletion() {
        let (root, mut manifest) = fixture();
        let outside = tempfile::tempdir().unwrap();
        std::fs::write(outside.path().join("keep"), b"outside").unwrap();
        let link = root
            .path()
            .join("android-data/org.example.blocked/junction");
        let status = std::process::Command::new("cmd.exe")
            .args(["/d", "/c", "mklink", "/J"])
            .arg(link.to_string_lossy().replace('/', "\\"))
            .arg(outside.path())
            .output()
            .unwrap();
        assert!(
            status.status.success(),
            "{}",
            String::from_utf8_lossy(&status.stderr)
        );
        assert!(clean(root.path(), &mut manifest).is_err());
        assert!(manifest.omitted_apps.is_empty());
        assert_eq!(
            std::fs::read(outside.path().join("keep")).unwrap(),
            b"outside"
        );
        assert!(root
            .path()
            .join("android-data/org.example.blocked/files/data")
            .exists());
        // Remove the junction itself with RemoveDirectory semantics, never its target.
        std::fs::remove_dir(link).unwrap();
    }

    fn archive(path: &Path, entries: &[(&str, &[u8])]) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let mut builder = tar::Builder::new(std::fs::File::create(path).unwrap());
        for (name, bytes) in entries {
            let mut header = tar::Header::new_gnu();
            header.set_size(bytes.len() as u64);
            header.set_mode(0o600);
            header.set_mtime(123);
            quarantine::append_unix_path(&mut builder, &mut header, name, *bytes).unwrap();
        }
        builder.finish().unwrap();
    }

    #[test]
    fn quarantine_removes_all_stale_versions_preserves_raw_names_payload_and_mtime() {
        use std::io::Read;
        let (root, mut manifest) = fixture();
        let segment = root.path().join("quarantine/seg000.tar");
        let omitted = "sdcard/Android/data/org.example.blocked/file?";
        let kept = format!(
            "sdcard/Android/data/org.example.keep/{}\\raw?",
            "long".repeat(40)
        );
        archive(
            &segment,
            &[(omitted, b"old"), (omitted, b"new"), (&kept, b"kept")],
        );
        let untouched = root.path().join("quarantine/seg001.tar");
        archive(&untouched, &[("sdcard/DCIM/image?", b"image")]);
        let before = std::fs::read(&untouched).unwrap();
        clean(root.path(), &mut manifest).unwrap();
        assert_eq!(before, std::fs::read(&untouched).unwrap());
        let mut archive = tar::Archive::new(std::fs::File::open(&segment).unwrap());
        let entries: Vec<_> = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let mut entry = entry.unwrap();
                let path = verify::tar_remote(&entry);
                let time = entry.header().mtime().unwrap();
                let mut payload = Vec::new();
                entry.read_to_end(&mut payload).unwrap();
                (path, time, payload)
            })
            .collect();
        assert_eq!(entries, vec![(format!("/{kept}"), 123, b"kept".to_vec())]);
    }

    #[test]
    fn corrupt_unreferenced_segment_no_longer_permanently_blocks_exclusion() {
        let (root, mut manifest) = fixture();
        let segment = root.path().join("quarantine/seg000.tar");
        std::fs::create_dir_all(segment.parent().unwrap()).unwrap();
        std::fs::write(&segment, b"invalid tar").unwrap();
        clean(root.path(), &mut manifest).unwrap();
        let mut saved = load_manifest(root.path()).unwrap();
        assert!(saved.complete());
        assert!(!saved.omitted_apps[0].cleanup_pending);
        assert!(saved.items[0]
            .entries
            .iter()
            .all(|entry| package(&entry.remote) != Some("org.example.blocked")));
        assert!(verify::verify_manifest(root.path(), &mut saved).is_empty());
        archive(
            &segment,
            &[("sdcard/Android/data/org.example.blocked/old?", b"old")],
        );
        clean(root.path(), &mut saved).unwrap();
        assert!(saved.complete());
        assert!(!saved.omitted_apps[0].cleanup_pending);
        assert!(verify::verify_manifest(root.path(), &mut saved).is_empty());
        assert_eq!(saved.omitted_apps[0].removed_files, 1);
    }
    #[test]
    fn exclusion_removes_damaged_diagnostics_that_can_contain_omitted_app_data() {
        let (root, mut manifest) = fixture();
        std::fs::create_dir(root.path().join("recovery")).unwrap();
        for (name, package_name) in [
            ("seg000.tar-1.damaged", "org.example.blocked"),
            ("seg001.tar-2.damaged", "org.example.keep"),
        ] {
            let mut archive = tar::Builder::new(Vec::new());
            let mut header = tar::Header::new_gnu();
            header.set_size(4);
            header.set_mode(0o600);
            quarantine::append_unix_path(
                &mut archive,
                &mut header,
                &format!("sdcard/Android/data/{package_name}/files/data"),
                b"data".as_slice(),
            )
            .unwrap();
            std::fs::write(
                root.path().join("recovery").join(name),
                archive.into_inner().unwrap(),
            )
            .unwrap();
        }
        std::fs::write(
            root.path().join("recovery/seg002.tar-3.damaged"),
            b"unidentifiable damaged diagnostic",
        )
        .unwrap();
        clean(root.path(), &mut manifest).unwrap();
        assert!(!root.path().join("recovery/seg000.tar-1.damaged").exists());
        assert!(!root.path().join("recovery/seg002.tar-3.damaged").exists());
        assert!(root.path().join("recovery/seg001.tar-2.damaged").exists());
        assert!(manifest.complete());
    }
}
