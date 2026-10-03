//! 백업 무결성 검사 — 모든 기록 파일을 고정 크기 버퍼로 스트리밍 해싱한다.
use super::model::{BackupSummary, ItemRecord, ItemStatus, Manifest};
use super::paths;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek};
use std::path::{Path, PathBuf};

pub fn hash_reader(mut reader: impl Read) -> Result<(String, u64), String> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("해시 읽기 실패: {e}"))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        count += n as u64;
    }
    Ok((hex::encode(hash.finalize()), count))
}

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

// 재시도로 같은 원본 이름이 여러 세그먼트에 있을 수 있다. 일치하는 내용만 검증에 사용한다.
pub(super) fn quarantine_hashes(
    root: &Path,
    wanted: &HashSet<String>,
) -> Result<HashMap<String, Vec<(String, u64)>>, String> {
    let mut hashes: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    for segment in segments(root)? {
        let file = std::fs::File::open(segment).map_err(|e| e.to_string())?;
        for (remote, versions) in archive_hashes(file, wanted)? {
            hashes.entry(remote).or_default().extend(versions);
        }
    }
    Ok(hashes)
}

fn archive_hashes(
    reader: impl Read + Seek,
    wanted: &HashSet<String>,
) -> Result<HashMap<String, Vec<(String, u64)>>, String> {
    let mut hashes: HashMap<String, Vec<(String, u64)>> = HashMap::new();
    let mut archive = tar::Archive::new(reader);
    for entry in archive
        .entries_with_seek()
        .map_err(|e| format!("격리 tar 해석 실패: {e}"))?
    {
        let entry = entry.map_err(|e| format!("격리 tar 항목 오류: {e}"))?;
        let remote = format!(
            "/{}",
            entry
                .path()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/")
        );
        paths::sdcard_relative(&remote)?;
        if !entry.header().entry_type().is_file() {
            return Err("격리 tar에 일반 파일이 아닌 항목이 있습니다".into());
        }
        // 모든 헤더의 경로/유형을 검사하되, 선택하지 않은 본문은 seek로 건너뛴다.
        if wanted.contains(&remote) {
            hashes.entry(remote).or_default().push(hash_reader(entry)?);
        }
    }
    Ok(hashes)
}

pub fn item_problems(root: &Path, item: &ItemRecord) -> Vec<String> {
    let mut errors = item.errors.clone();
    let mut names = HashSet::new();
    for entry in &item.entries {
        if !names.insert(&entry.remote) {
            errors.push(format!("중복 파일 기록: {}", entry.remote));
        }
        if let Some(error) = &entry.error {
            errors.push(format!("{}: {error}", entry.remote));
            continue;
        }
        if entry
            .sha256
            .as_ref()
            .is_none_or(|h| h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            errors.push(format!("해시 기록 없음/잘못된 해시: {}", entry.remote));
            continue;
        }
        if entry.quarantined {
            continue;
        }
        let result = paths::existing_file(root, &entry.local)
            .and_then(|path| std::fs::File::open(path).map_err(|e| e.to_string()))
            .and_then(hash_reader);
        match result {
            Ok((hash, size))
                if size == entry.size && Some(hash.as_str()) == entry.sha256.as_deref() => {}
            Ok(_) => errors.push(format!("크기/해시 불일치: {}", entry.remote)),
            Err(error) => errors.push(format!("{}: {error}", entry.remote)),
        }
    }
    for artifact in &item.artifacts {
        if let Err(error) = paths::existing_file(root, artifact) {
            errors.push(error);
        }
    }
    errors
}

pub fn verify_manifest(root: &Path, manifest: &mut Manifest) -> Vec<String> {
    let wanted: HashSet<String> = manifest
        .items
        .iter()
        .filter(|item| item.status != ItemStatus::Skipped)
        .flat_map(|item| item.entries.iter())
        .filter(|entry| entry.quarantined && entry.error.is_none())
        .map(|entry| entry.remote.clone())
        .collect();
    let quarantine = if !wanted.is_empty() {
        quarantine_hashes(root, &wanted)
    } else {
        Ok(HashMap::new())
    };
    let mut problems = vec![];
    let mut ids = HashSet::new();
    for item in &mut manifest.items {
        if !ids.insert(item.id.clone()) {
            problems.push(format!("중복 항목: {}", item.id));
        }
        if item.status == ItemStatus::Skipped {
            continue;
        }
        let mut errors = item_problems(root, item);
        for entry in item
            .entries
            .iter()
            .filter(|e| e.quarantined && e.error.is_none())
        {
            match &quarantine {
                Ok(contents)
                    if contents.get(&entry.remote).is_some_and(|versions| {
                        versions.iter().any(|(hash, size)| {
                            Some(hash.as_str()) == entry.sha256.as_deref() && *size == entry.size
                        })
                    }) => {}
                Ok(_) => errors.push(format!("격리 파일 누락/해시 불일치: {}", entry.remote)),
                Err(error) => errors.push(error.clone()),
            }
        }
        if !errors.is_empty() {
            item.status = ItemStatus::Partial;
            problems.extend(errors.into_iter().map(|e| format!("{}: {e}", item.id)));
        }
    }
    problems
}

pub fn backup_summary(root: &Path) -> Result<BackupSummary, String> {
    let mut manifest = super::model::load_manifest(root)?;
    let problems = verify_manifest(root, &mut manifest);
    let mut summary = BackupSummary::from(&manifest);
    if !problems.is_empty() {
        summary.complete = false;
        summary.errors.extend(problems);
    }
    summary.dir = root.to_string_lossy().to_string();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::super::model::{FileEntry, ItemKind};
    use super::*;
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
