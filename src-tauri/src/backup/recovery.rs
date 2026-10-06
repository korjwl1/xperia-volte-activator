//! Recovery of application-owned PC artifacts. Never touches phone originals.
use super::{model::Manifest, paths, verify};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Write};
use std::path::Path;

pub(super) fn archive_error(error: std::io::Error) -> String {
    let text = error.to_string();
    if error.raw_os_error().is_none()
        && (matches!(
            error.kind(),
            std::io::ErrorKind::InvalidData | std::io::ErrorKind::UnexpectedEof
        ) || text.contains("failed to read entire block")
            || text.contains("unexpected EOF during skip")
            || text.contains("members found describing a future member")
            || text.contains("size overflow")
            || text.contains("archive header checksum mismatch")
            || text.contains("numeric field")
            || text.contains("failed to parse")
            || text.contains("invalid octal"))
    {
        format!("CORRUPT_ARCHIVE|{text}")
    } else {
        format!("PC_IO|{text}")
    }
}

/// Only exact generated temporary names, within a manifest-validated backup.
/// Command callers hold the operation lock, so no writer runs concurrently.
pub(super) fn clean_temporaries(root: &Path) -> Result<(), String> {
    let recorded = manifest_paths(root)?;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(metadata) = std::fs::symlink_metadata(entry.path()) else {
                continue;
            };
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    continue;
                }
            }
            if metadata.file_type().is_symlink() {
                continue;
            }
            if metadata.is_dir() {
                pending.push(entry.path());
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let generated = name
                .strip_prefix(".xvolte-")
                .and_then(|n| n.strip_suffix(".tmp"))
                .is_some_and(|n| {
                    n.split_once('-').is_some_and(|(pid, seq)| {
                        !pid.is_empty()
                            && !seq.is_empty()
                            && pid.bytes().all(|c| c.is_ascii_digit())
                            && seq.bytes().all(|c| c.is_ascii_digit())
                    })
                });
            if generated && metadata.is_file() {
                let relative = entry
                    .path()
                    .strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                // An explicit manifest receipt always wins over a generated-looking name.
                if recorded.contains(&relative) {
                    continue;
                }
                std::fs::remove_file(paths::existing_file(root, &relative)?)
                    .map_err(|e| e.to_string())?;
            }
        }
    }
    Ok(())
}
fn manifest_paths(root: &Path) -> Result<HashSet<String>, String> {
    Ok(super::model::load_manifest(root)?
        .items
        .into_iter()
        .flat_map(|i| i.entries.into_iter().map(|e| e.local).chain(i.artifacts))
        .collect())
}

/// If header traversal fails, salvage only payloads matching durable manifest hashes.
/// Unrecoverable receipts become Partial and are re-pulled on resume. This makes an
/// interrupted segment recoverable without discarding good receipts from other segments.
pub(super) fn repair_segments(root: &Path, manifest: &mut Manifest) -> Result<(), String> {
    let wanted: HashMap<_, _> = manifest
        .items
        .iter()
        .flat_map(|i| &i.entries)
        .filter(|e| e.quarantined && e.error.is_none())
        .map(|e| (e.remote.clone(), e.clone()))
        .collect();
    let mut repaired = false;
    for segment in verify::segments(root)? {
        let inspect = || -> Result<(), String> {
            let mut archive =
                tar::Archive::new(std::fs::File::open(&segment).map_err(|e| e.to_string())?);
            let length = std::fs::metadata(&segment)
                .map_err(|e| e.to_string())?
                .len();
            for entry in archive.entries_with_seek().map_err(archive_error)? {
                let entry = entry.map_err(archive_error)?;
                if entry
                    .raw_file_position()
                    .checked_add(entry.size())
                    .is_none_or(|end| end > length)
                {
                    return Err("CORRUPT_ARCHIVE|Truncated tar payload".into());
                }
                paths::archive_relative(&verify::tar_remote(&entry))
                    .map_err(|e| format!("CORRUPT_ARCHIVE|{e}"))?;
                if !entry.header().entry_type().is_file() {
                    return Err("CORRUPT_ARCHIVE|Invalid archive type".into());
                }
            }
            Ok(())
        };
        match inspect() {
            Ok(()) => continue,
            Err(error) if error.starts_with("CORRUPT_ARCHIVE|") => {}
            Err(error) => return Err(error),
        }
        // Preserve damaged input unless app-data exclusion explicitly forbids retaining it.
        if manifest.omitted_apps.is_empty() {
            let name = segment.file_name().unwrap().to_string_lossy();
            let history = paths::write_target(
                root,
                &format!(
                    "recovery/{name}-{}.damaged",
                    chrono::Utc::now()
                        .timestamp_nanos_opt()
                        .ok_or("recovery timestamp")?
                ),
            )?;
            crate::storage::atomic_file(&history, |out, _| {
                let mut input = std::fs::File::open(&segment).map_err(|e| e.to_string())?;
                std::io::copy(&mut input, out).map_err(|e| crate::storage::io_error(&e))?;
                Ok(())
            })?;
        }
        crate::storage::atomic_file(&segment, |out, _| {
            let mut builder = tar::Builder::new(out);
            let mut archive =
                tar::Archive::new(std::fs::File::open(&segment).map_err(|e| e.to_string())?);
            let entries = archive.entries().map_err(|e| e.to_string())?;
            for entry in entries {
                let mut entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        let error = archive_error(error);
                        if error.starts_with("CORRUPT_ARCHIVE|") {
                            break;
                        }
                        return Err(error);
                    }
                };
                let remote = verify::tar_remote(&entry);
                let Ok(name) = paths::archive_relative(&remote) else {
                    break;
                };
                if !entry.header().entry_type().is_file() {
                    break;
                }
                let Some(receipt) = wanted.get(&remote) else {
                    continue;
                };
                if manifest
                    .omitted_apps
                    .iter()
                    .any(|a| super::omissions::package(&remote) == Some(a.package.as_str()))
                {
                    continue;
                }
                // A generated scratch file is removed on every normal exit.
                let scratch = paths::write_target(
                    root,
                    &format!(
                        "quarantine/.xvolte-{}-{}.tmp",
                        std::process::id(),
                        chrono::Utc::now()
                            .timestamp_nanos_opt()
                            .ok_or("복구 임시 파일 식별 시각 오류")?
                    ),
                )?;
                // Reserve our own name exclusively; never replace a caller's existing file.
                std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&scratch)
                    .map_err(|e| crate::storage::io_error(&e))?;
                let result = crate::storage::atomic_file(&scratch, |file, _| {
                    let mut hash = super::quarantine::HashingWriter::new(file);
                    let mut buffer = vec![0; 256 * 1024];
                    loop {
                        let count = entry.read(&mut buffer).map_err(archive_error)?;
                        if count == 0 {
                            break;
                        }
                        hash.write_all(&buffer[..count])
                            .map_err(|e| crate::storage::io_error(&e))?;
                    }
                    let (_, sha, size) = hash.finish();
                    if size != receipt.size || Some(sha.as_str()) != receipt.sha256.as_deref() {
                        return Ok(false);
                    }
                    Ok(true)
                });
                match result {
                    Ok(true) => {
                        let mut header = entry.header().clone();
                        let mut source =
                            std::fs::File::open(&scratch).map_err(|e| e.to_string())?;
                        let copied = super::quarantine::append_unix_path(
                            &mut builder,
                            &mut header,
                            name,
                            &mut source,
                        )
                        .map_err(|e| e.to_string());
                        let _ = std::fs::remove_file(&scratch);
                        copied?;
                    }
                    Ok(false) => {
                        let _ = std::fs::remove_file(&scratch);
                    }
                    Err(error) => {
                        let _ = std::fs::remove_file(&scratch);
                        if error.starts_with("CORRUPT_ARCHIVE|") {
                            break;
                        }
                        return Err(error);
                    }
                }
            }
            builder.finish().map_err(|e| e.to_string())
        })?;
        repaired = true;
    }
    if repaired {
        let keys: HashSet<_> = wanted.keys().cloned().collect();
        let valid = verify::quarantine_hashes(root, &keys)?;
        if !valid.segment_errors.is_empty() {
            return Err(valid.segment_errors.join("; "));
        }
        for item in &mut manifest.items {
            for entry in item
                .entries
                .iter_mut()
                .filter(|e| e.quarantined && e.error.is_none())
            {
                if !valid.versions.get(&entry.remote).is_some_and(|v| {
                    v.iter().any(|(h, n)| {
                        Some(h.as_str()) == entry.sha256.as_deref() && *n == entry.size
                    })
                }) {
                    entry.error=Some("손상된 tar에서 복구하지 못했습니다 — 원본 기기를 연결해 백업을 이어서 실행하세요".into());
                    item.status = super::model::ItemStatus::Partial;
                }
            }
        }
        manifest.touch();
        super::model::save_manifest_atomic(manifest, root)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn transient_io_is_never_a_structural_archive_error() {
        for kind in [
            std::io::ErrorKind::PermissionDenied,
            std::io::ErrorKind::TimedOut,
            std::io::ErrorKind::Other,
        ] {
            assert!(
                archive_error(std::io::Error::new(kind, "temporary disk failure"))
                    .starts_with("PC_IO|")
            );
        }
    }
    #[test]
    fn corrupt_segment_keeps_verified_payload_and_marks_lost_receipts_for_retry() {
        use super::super::model::{FileEntry, ItemKind, ItemRecord, ItemStatus};
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("quarantine")).unwrap();
        let segment = root.path().join("quarantine/seg000.tar");
        let mut builder = tar::Builder::new(Vec::new());
        let mut item = ItemRecord::new("dcim", ItemKind::Files);
        for name in ["first?", "second?"] {
            let remote = format!("/sdcard/DCIM/{name}");
            let mut header = tar::Header::new_gnu();
            header.set_size(3);
            header.set_mode(0o640);
            header.set_mtime(100);
            super::super::quarantine::append_unix_path(
                &mut builder,
                &mut header,
                remote.trim_start_matches('/'),
                b"abc".as_slice(),
            )
            .unwrap();
            item.entries.push(FileEntry {
                remote,
                local: String::new(),
                size: 3,
                mtime: 100,
                sha256: Some(crate::boot_image::sha256(b"abc")),
                quarantined: true,
                error: None,
            });
        }
        let mut bytes = builder.into_inner().unwrap();
        bytes[1024..1536].fill(0xff);
        std::fs::write(&segment, bytes).unwrap();
        item.status = ItemStatus::Done;
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        manifest.record(item);
        super::super::model::save_manifest_atomic(&manifest, root.path()).unwrap();
        repair_segments(root.path(), &mut manifest).unwrap();
        assert!(manifest.items[0].entries[0].error.is_none());
        assert!(manifest.items[0].entries[1].error.is_some());
        assert!(!manifest.complete());
        let wanted = HashSet::from(["/sdcard/DCIM/first?".into()]);
        let hashes = verify::quarantine_hashes(root.path(), &wanted).unwrap();
        assert!(hashes.segment_errors.is_empty());
        assert_eq!(hashes.versions["/sdcard/DCIM/first?"].len(), 1);
        repair_segments(root.path(), &mut manifest).unwrap(); // no permanent tar parse failure
    }
    #[test]
    fn only_unrecorded_exact_generated_temporary_names_are_removed() {
        let root = tempfile::tempdir().unwrap();
        let mut manifest = Manifest::new("test", "masked", "v", "15");
        let mut item =
            super::super::model::ItemRecord::new("dcim", super::super::model::ItemKind::Files);
        item.artifacts.push(".xvolte-7-8.tmp".into());
        manifest.record(item);
        super::super::model::save_manifest_atomic(&manifest, root.path()).unwrap();
        for name in [
            ".xvolte-1-2.tmp",
            ".xvolte-7-8.tmp",
            ".xvolte-not-generated.tmp",
            "user.tmp",
        ] {
            std::fs::write(root.path().join(name), b"x").unwrap();
        }
        clean_temporaries(root.path()).unwrap();
        assert!(!root.path().join(".xvolte-1-2.tmp").exists());
        for name in [".xvolte-7-8.tmp", ".xvolte-not-generated.tmp", "user.tmp"] {
            assert!(root.path().join(name).exists());
        }
    }
    #[test]
    fn truncated_orphan_and_longname_metadata_do_not_permanently_block_resume() {
        for longname in [false, true] {
            let root = tempfile::tempdir().unwrap();
            std::fs::create_dir(root.path().join("quarantine")).unwrap();
            let mut header = tar::Header::new_gnu();
            header.set_size(4096);
            header.set_mode(0o644);
            let bytes = if longname {
                let mut b = tar::Builder::new(Vec::new());
                super::super::quarantine::append_unix_path(
                    &mut b,
                    &mut header,
                    &format!("sdcard/{}", "a".repeat(160)),
                    &[0; 4096][..],
                )
                .unwrap();
                let mut bytes = b.into_inner().unwrap();
                bytes.truncate(1024);
                bytes
            } else {
                header.set_path("sdcard/orphan").unwrap();
                header.set_cksum();
                let mut bytes = header.as_bytes().to_vec();
                bytes.extend_from_slice(&[0; 100]);
                bytes
            };
            std::fs::write(root.path().join("quarantine/seg000.tar"), bytes).unwrap();
            let mut manifest = Manifest::new("test", "masked", "v", "15");
            super::super::model::save_manifest_atomic(&manifest, root.path()).unwrap();
            repair_segments(root.path(), &mut manifest).unwrap();
            repair_segments(root.path(), &mut manifest).unwrap();
            assert!(verify::quarantine_hashes(root.path(), &HashSet::new())
                .unwrap()
                .segment_errors
                .is_empty());
        }
    }
}
