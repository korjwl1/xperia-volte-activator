use super::{
    device::Metadata,
    error::{Error, Result},
    manifest::{self, Plan, Target},
    session::{Session, Transport},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
};
const MAX_SNAPSHOT_MANIFEST: usize = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Warning {
    pub code: String,
    pub target: String,
    pub message: String,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub operation: String,
    pub file: String,
    pub n: usize,
    pub total: usize,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOut {
    pub errors: Vec<String>,
    pub files_seen: usize,
    pub planned: usize,
    pub skipped: usize,
    pub warnings: Vec<Warning>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub ok: bool,
    pub files: usize,
    pub matched: usize,
    pub planned: usize,
    pub skipped: usize,
    pub missing: Vec<String>,
    pub mismatches: Vec<String>,
    pub warnings: Vec<Warning>,
}
pub fn warnings(plan: &Plan) -> Vec<Warning> {
    let mut warnings:Vec<Warning> = plan.entries.iter().filter(|e|e.empty_nv()).map(|e|Warning{code:"emptyNvSkipped".into(),target:e.name.clone(),message:"Zero-byte NV entry is non-mutating; no expected value exists. Neither written nor verified".into()}).collect();
    for entry in plan.active() {
        if let Target::Nv { id } = entry.target {
            if super::wire::nv_logical_size(id) == 0 && entry.data.len() < 128 {
                warnings.push(Warning{code:"nvPrefixVerification".into(),target:entry.name.clone(),message:format!("Unknown logical NV size: only {} explicit bytes are verified; unspecified tail is not asserted",entry.data.len())});
            }
        }
    }
    warnings
}
pub fn upload<T: Transport>(
    s: &mut Session<T>,
    plan: &Plan,
    progress: &mut impl FnMut(Progress),
) -> Result<UploadOut> {
    let total = plan.active().count();
    for (i, e) in plan.active().enumerate() {
        s.check_cancel()?;
        let result = match &e.target {
            Target::Efs {
                path,
                mode,
                entry_type,
            } => s.write_file(path, *mode, *entry_type, &e.data),
            Target::Nv { id } => s.nv_write(*id, &e.data),
        };
        result.map_err(|err| Error {
            operation: format!("upload {}", e.name),
            ..err
        })?;
        progress(Progress {
            operation: "upload".into(),
            file: e.name.clone(),
            n: i + 1,
            total,
        });
    }
    Ok(UploadOut {
        errors: vec![],
        files_seen: total,
        planned: plan.entries.len(),
        skipped: plan.entries.len() - total,
        warnings: warnings(plan),
    })
}
pub fn verify<T: Transport>(
    s: &mut Session<T>,
    plan: &Plan,
    progress: &mut impl FnMut(Progress),
) -> Result<VerifyReport> {
    let total = plan.active().count();
    let mut report = VerifyReport {
        ok: true,
        files: total,
        matched: 0,
        planned: plan.entries.len(),
        skipped: plan.entries.len() - total,
        missing: vec![],
        mismatches: vec![],
        warnings: warnings(plan),
    };
    for (i, e) in plan.active().enumerate() {
        s.check_cancel()?;
        let read = match &e.target {
            Target::Efs { path, .. } => s.read_file(path),
            Target::Nv { id } => s
                .nv_read(*id)
                .and_then(|data| super::wire::normalize_nv(*id, &data, e.data.len())),
        };
        match read {
            Ok(data) if manifest::hash(&data) == manifest::hash(&e.data) => report.matched += 1,
            Ok(_) => report.mismatches.push(e.name.clone()),
            Err(err) if err.status == Some(2) && matches!(e.target, Target::Efs { .. }) => {
                report.missing.push(e.name.clone())
            }
            // An unreadable value fails closed; transport failure aborts (no further writes/reads).
            Err(err) => {
                return Err(Error {
                    operation: format!("verify {}", e.name),
                    ..err
                })
            }
        }
        progress(Progress {
            operation: "verify".into(),
            file: e.name.clone(),
            n: i + 1,
            total,
        });
    }
    report.ok = total > 0 && report.matched == total;
    Ok(report)
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotEntry {
    pub target: Target,
    pub metadata: Option<Metadata>,
    pub blob: Option<String>,
    pub sha256: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub version: u32,
    pub complete: bool,
    pub preset_sha256: String,
    pub entries: Vec<SnapshotEntry>,
    pub warnings: Vec<Warning>,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOut {
    pub path: String,
    pub files_seen: usize,
    pub warnings: Vec<Warning>,
}
/// Scoped before-image of every mutation target; root NV is read once per manifest ID.
/// Mode/type/times stay in metadata; blob names never encode phone paths.
pub fn snapshot<T: Transport>(
    s: &mut Session<T>,
    plan: &Plan,
    dest: &Path,
    progress: &mut impl FnMut(Progress),
) -> Result<SnapshotOut> {
    if dest.exists()
        && fs::read_dir(dest)
            .map_err(|e| Error::io("snapshot", e))?
            .next()
            .is_some()
    {
        return Err(Error::new(
            "invalidDestination",
            "snapshot",
            "Choose an empty directory",
        ));
    }
    fs::create_dir_all(dest).map_err(|e| Error::io("snapshot", e))?;
    let mut snapshot = Snapshot {
        version: 1,
        complete: false,
        preset_sha256: plan.sha256.clone(),
        entries: vec![],
        warnings: warnings(plan),
    };
    // Write incomplete marker first. Failure never leaves a restorable-looking snapshot.
    save_snapshot(dest, &snapshot)?;
    let mut directories = BTreeMap::new();
    let mut bytes_seen = 0usize;
    let total = plan.active().count();
    for (i, e) in plan.active().enumerate() {
        s.check_cancel()?;
        let (data, metadata, target) = match &e.target {
            Target::Nv { id } => (Some(s.nv_read(*id)?), None, e.target.clone()),
            Target::Efs { path, .. } => {
                let (parent, name) = path.rsplit_once('/').unwrap();
                let parent = if parent.is_empty() { "/" } else { parent };
                if !directories.contains_key(parent) {
                    let listing = match s.list(parent) {
                        Ok(l) => l,
                        Err(e) if e.status == Some(2) => vec![],
                        Err(e) => return Err(e),
                    };
                    directories.insert(parent.to_string(), listing);
                }
                let metadata = directories[parent]
                    .iter()
                    .find(|v| v.name == name)
                    .map(|v| v.metadata.clone());
                if let Some(meta) = &metadata {
                    if !matches!(meta.entry_type, 0 | 15) {
                        return Err(Error::new(
                            "unsupportedType",
                            "snapshot",
                            "Links/special entries cannot be safely restored",
                        ));
                    }
                    let data = s.read_file(path)?;
                    if data.len() != meta.size as usize {
                        return Err(Error::new(
                            "changedDuringRead",
                            "snapshot",
                            "File changed while taking before-image",
                        ));
                    }
                    (
                        Some(data),
                        metadata.clone(),
                        Target::Efs {
                            path: path.clone(),
                            mode: meta.mode,
                            entry_type: meta.entry_type,
                        },
                    )
                } else {
                    // Listing absence must agree with stat; no unreadable/missing target is silently treated as absent.
                    match s.stat(path) {
                        Err(e) if e.status == Some(2) => {}
                        Err(e) => return Err(e),
                        Ok(_) => {
                            return Err(Error::new(
                                "inconsistent",
                                "snapshot",
                                "stat/list disagree",
                            ))
                        }
                    }
                    (None, None, e.target.clone())
                }
            }
        };
        let (blob, sha) = if let Some(data) = data {
            bytes_seen += data.len();
            if bytes_seen > manifest::MAX_TOTAL {
                return Err(Error::new(
                    "limit",
                    "snapshot",
                    "Snapshot exceeds restore bound",
                ));
            }
            let blob = format!("{i:06}.bin");
            crate::storage::atomic_write(&dest.join(&blob), &data)
                .map_err(|e| Error::io("snapshot", e))?;
            (Some(blob), Some(manifest::hash(&data)))
        } else {
            (None, None)
        };
        snapshot.entries.push(SnapshotEntry {
            target,
            metadata,
            blob,
            sha256: sha,
        });
        progress(Progress {
            operation: "snapshot".into(),
            file: e.name.clone(),
            n: i + 1,
            total,
        });
    }
    s.check_cancel()?;
    snapshot.complete = true;
    save_snapshot(dest, &snapshot)?;
    Ok(SnapshotOut {
        path: dest.to_string_lossy().into(),
        files_seen: total,
        warnings: snapshot.warnings,
    })
}
fn save_snapshot(dest: &Path, s: &Snapshot) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(s).map_err(|e| Error::io("snapshot", e))?;
    if bytes.len() > MAX_SNAPSHOT_MANIFEST {
        return Err(Error::new(
            "limit",
            "snapshot",
            "Snapshot manifest exceeds restore bound",
        ));
    }
    crate::storage::atomic_write(&dest.join("snapshot.json"), &bytes)
        .map_err(|e| Error::io("snapshot", e))
}
pub struct RestorePlan {
    pub entries: Vec<(SnapshotEntry, Option<Vec<u8>>)>,
    pub warnings: Vec<Warning>,
}
pub fn load_snapshot(dir: &Path) -> Result<RestorePlan> {
    let root = dir.canonicalize().map_err(|e| Error::io("rollback", e))?;
    let path = root.join("snapshot.json");
    if fs::metadata(&path)
        .map_err(|e| Error::io("rollback", e))?
        .len()
        > MAX_SNAPSHOT_MANIFEST as u64
    {
        return Err(Error::new(
            "limit",
            "rollback",
            "Snapshot manifest too large",
        ));
    }
    let snap: Snapshot = serde_json::from_slice(&manifest::read_bounded(
        &path,
        MAX_SNAPSHOT_MANIFEST,
        "rollback",
    )?)
    .map_err(|e| Error::io("rollback", e))?;
    if snap.version != 1 || !snap.complete || snap.entries.is_empty() || snap.entries.len() > 10000
    {
        return Err(Error::new(
            "incompleteSnapshot",
            "rollback",
            "Incomplete or unsupported snapshot",
        ));
    }
    let mut entries = vec![];
    let mut seen = BTreeSet::new();
    let mut total = 0;
    for e in snap.entries {
        let key = match &e.target {
            Target::Nv { id } => {
                if *id == 0 || *id == 65535 {
                    return Err(Error::new("invalidNvId", "rollback", "Invalid NV ID"));
                }
                format!("nv:{id}")
            }
            Target::Efs {
                path,
                mode,
                entry_type,
            } => {
                manifest::safe_remote(path)?;
                if !matches!(entry_type, 0 | 15)
                    || e.metadata
                        .as_ref()
                        .is_some_and(|m| m.mode != *mode || m.entry_type != *entry_type)
                {
                    return Err(Error::new(
                        "invalidMetadata",
                        "rollback",
                        "Invalid restore type/mode",
                    ));
                }
                format!("efs:{path}")
            }
        };
        if !seen.insert(key) {
            return Err(Error::new(
                "duplicateTarget",
                "rollback",
                "Duplicate snapshot target",
            ));
        }
        let data = match (&e.blob, &e.sha256) {
            (Some(blob), Some(sha)) => {
                if blob.len() != 10
                    || !blob.ends_with(".bin")
                    || !blob[..6].bytes().all(|b| b.is_ascii_digit())
                {
                    return Err(Error::new("invalidPath", "rollback", "Invalid blob name"));
                }
                let p = root.join(blob);
                let meta = fs::symlink_metadata(&p).map_err(|e| Error::io("rollback", e))?;
                if !meta.is_file()
                    || manifest::is_reparse(&meta)
                    || meta.len() > manifest::MAX_FILE as u64
                    || !p
                        .canonicalize()
                        .map_err(|e| Error::io("rollback", e))?
                        .starts_with(&root)
                {
                    return Err(Error::new(
                        "invalidPath",
                        "rollback",
                        "Unsafe snapshot blob",
                    ));
                }
                let bytes = manifest::read_bounded(&p, manifest::MAX_FILE, "rollback")?;
                total += bytes.len();
                if total > manifest::MAX_TOTAL {
                    return Err(Error::new("limit", "rollback", "Snapshot exceeds bound"));
                }
                if manifest::hash(&bytes) != *sha {
                    return Err(Error::new(
                        "snapshotCorrupt",
                        "rollback",
                        "Blob hash mismatch",
                    ));
                }
                match &e.target {
                    Target::Nv { .. } if bytes.len() != 128 || e.metadata.is_some() => {
                        return Err(Error::new(
                            "invalidMetadata",
                            "rollback",
                            "NV snapshot must preserve full 128 bytes",
                        ))
                    }
                    Target::Efs {
                        path,
                        mode,
                        entry_type,
                    } => {
                        if e.metadata
                            .as_ref()
                            .is_none_or(|m| m.size as usize != bytes.len())
                        {
                            return Err(Error::new(
                                "invalidMetadata",
                                "rollback",
                                "Missing or inconsistent metadata",
                            ));
                        }
                        if *entry_type == 15 {
                            super::wire::put(path, *mode, &bytes)?;
                        }
                    }
                    _ => {}
                }
                Some(bytes)
            }
            (None, None) if matches!(e.target, Target::Efs { .. }) && e.metadata.is_none() => None,
            _ => {
                return Err(Error::new(
                    "invalidMetadata",
                    "rollback",
                    "Missing blob/hash",
                ))
            }
        };
        entries.push((e, data));
    }
    Ok(RestorePlan {
        entries,
        warnings: snap.warnings,
    })
}

pub fn rollback<T: Transport>(
    s: &mut Session<T>,
    plan: &RestorePlan,
    progress: &mut impl FnMut(Progress),
) -> Result<UploadOut> {
    for (i, (entry, data)) in plan.entries.iter().enumerate() {
        s.check_cancel()?;
        let name = match &entry.target {
            Target::Nv { id } => {
                let data = data.as_ref().unwrap();
                s.nv_write(*id, data)?;
                if s.nv_read(*id)? != *data {
                    return Err(Error::new(
                        "verifyFailed",
                        "rollback",
                        "NV restore mismatch",
                    ));
                }
                format!("NV {id}")
            }
            Target::Efs {
                path,
                mode,
                entry_type,
            } => {
                if let Some(data) = data {
                    s.write_file(path, *mode, *entry_type, data)?;
                    if s.read_file(path)? != *data {
                        return Err(Error::new(
                            "verifyFailed",
                            "rollback",
                            "EFS restore mismatch",
                        ));
                    }
                    let (parent, name) = path.rsplit_once('/').unwrap();
                    let list = s.list(if parent.is_empty() { "/" } else { parent })?;
                    if !list.iter().any(|e| {
                        e.name == name
                            && e.metadata.mode == *mode
                            && e.metadata.entry_type == *entry_type
                    }) {
                        return Err(Error::new(
                            "verifyFailed",
                            "rollback",
                            "Restored mode/type mismatch",
                        ));
                    }
                } else {
                    match s.unlink(path) {
                        Ok(()) => {}
                        Err(e) if e.status == Some(2) => {}
                        Err(e) => return Err(e),
                    }
                    match s.stat(path) {
                        Err(e) if e.status == Some(2) => {}
                        Err(e) => return Err(e),
                        Ok(_) => {
                            return Err(Error::new(
                                "verifyFailed",
                                "rollback",
                                "New target still exists",
                            ))
                        }
                    }
                }
                path.clone()
            }
        };
        progress(Progress {
            operation: "rollback".into(),
            file: name,
            n: i + 1,
            total: plan.entries.len(),
        });
    }
    Ok(UploadOut {
        errors: vec![],
        files_seen: plan.entries.len(),
        planned: plan.entries.len(),
        skipped: 0,
        warnings: plan.warnings.clone(),
    })
}

#[cfg(test)]
mod snapshot_storage_tests {
    use super::*;
    #[test]
    fn oversized_completion_cannot_replace_the_incomplete_marker() {
        let dir = tempfile::tempdir().unwrap();
        let mut snapshot = Snapshot {
            version: 1,
            complete: false,
            preset_sha256: "test".into(),
            entries: vec![],
            warnings: vec![],
        };
        save_snapshot(dir.path(), &snapshot).unwrap();
        snapshot.complete = true;
        snapshot.warnings.push(Warning {
            code: "test".into(),
            target: "test".into(),
            message: "x".repeat(MAX_SNAPSHOT_MANIFEST),
        });
        assert_eq!(
            save_snapshot(dir.path(), &snapshot).unwrap_err().code,
            "limit"
        );
        let recorded: Snapshot =
            serde_json::from_slice(&fs::read(dir.path().join("snapshot.json")).unwrap()).unwrap();
        assert!(!recorded.complete);
        assert!(load_snapshot(dir.path()).is_err());
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
