use super::{
    error::{Error, Result},
    wire,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
pub const MAX_FILE: usize = 16 * 1024 * 1024;
pub const MAX_TOTAL: usize = 128 * 1024 * 1024;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Target {
    Efs {
        path: String,
        mode: u32,
        entry_type: u32,
    },
    Nv {
        id: u16,
    },
}
#[derive(Debug, Clone)]
pub struct Entry {
    pub name: String,
    pub target: Target,
    pub data: Vec<u8>,
}
#[derive(Debug)]
pub struct Plan {
    pub entries: Vec<Entry>,
    pub sha256: String,
}
pub fn hash(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}
pub fn read_bounded(path: &Path, limit: usize, operation: &str) -> Result<Vec<u8>> {
    let file = fs::File::open(path).map_err(|e| Error::io(operation, e))?;
    let mut data = vec![];
    file.take(limit as u64 + 1)
        .read_to_end(&mut data)
        .map_err(|e| Error::io(operation, e))?;
    if data.len() > limit {
        return Err(Error::new("limit", operation, "File exceeds read bound"));
    }
    Ok(data)
}
pub fn is_reparse(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return true;
        }
    }
    meta.file_type().is_symlink()
}
pub fn safe_remote(p: &str) -> Result<()> {
    if !p.starts_with('/')
        || p == "/"
        || p.split('/')
            .skip(1)
            .any(|x| x.is_empty() || x == "." || x == ".." || x.contains('\\') || x.contains(':'))
    {
        return Err(Error::new("invalidPath", "manifest", "Unsafe remote path"));
    }
    wire::path(&mut vec![], p)
}
fn walk(root: &Path, dir: &Path, files: &mut Vec<PathBuf>, depth: usize) -> Result<()> {
    if depth > 64 {
        return Err(Error::new(
            "limit",
            "manifest",
            "Directory depth exceeds bound",
        ));
    }
    for entry in fs::read_dir(dir).map_err(|e| Error::io("manifest", e))? {
        let path = entry.map_err(|e| Error::io("manifest", e))?.path();
        let meta = fs::symlink_metadata(&path).map_err(|e| Error::io("manifest", e))?;
        if is_reparse(&meta)
            || !path
                .canonicalize()
                .map_err(|e| Error::io("manifest", e))?
                .starts_with(root)
        {
            return Err(Error::new(
                "invalidPath",
                "manifest",
                "Symlinks/reparse points are not allowed",
            ));
        }
        if meta.is_dir() {
            walk(root, &path, files, depth + 1)?;
        } else if meta.is_file() {
            files.push(path);
        } else {
            return Err(Error::new(
                "invalidPath",
                "manifest",
                "Non-regular preset entry",
            ));
        }
        if files.len() > 10000 {
            return Err(Error::new("limit", "manifest", "Too many preset files"));
        }
    }
    Ok(())
}
impl Plan {
    pub fn load(dir: &Path) -> Result<Self> {
        let root = dir.canonicalize().map_err(|e| Error::io("manifest", e))?;
        let mut paths = vec![];
        walk(&root, &root, &mut paths, 0)?;
        let mut named: Vec<_> = paths
            .into_iter()
            .map(|p| {
                let rel = p
                    .strip_prefix(&root)
                    .unwrap()
                    .to_str()
                    .ok_or_else(|| Error::new("invalidPath", "manifest", "Non-Unicode local name"))?
                    .replace('\\', "/");
                Ok((rel, p))
            })
            .collect::<Result<_>>()?;
        named.sort_by(|a, b| a.0.cmp(&b.0));
        let mut entries = vec![];
        let mut targets = BTreeSet::new();
        let mut sha = Sha256::new();
        let mut total = 0usize;
        for (name, local) in named {
            let size = fs::metadata(&local)
                .map_err(|e| Error::io("manifest", e))?
                .len();
            if size > MAX_FILE as u64 {
                return Err(Error::new("limit", "manifest", "File exceeds 16 MiB"));
            }
            let data = read_bounded(&local, MAX_FILE, "manifest")?;
            total += data.len();
            if total > MAX_TOTAL {
                return Err(Error::new("limit", "manifest", "Preset exceeds 128 MiB"));
            }
            // Existing efsPresets hashes use path + NUL + content + NUL.
            sha.update(name.as_bytes());
            sha.update([0]);
            sha.update(&data);
            sha.update([0]);
            let target = if let Some(id) = name.strip_prefix("NvItem__") {
                if id.len() != 8 || !id.bytes().all(|b| b.is_ascii_digit()) {
                    return Err(Error::new(
                        "invalidNvId",
                        "manifest",
                        format!("Invalid NV name: {name}"),
                    ));
                }
                let id: u16 = id
                    .parse()
                    .map_err(|_| Error::new("invalidNvId", "manifest", "NV ID exceeds u16"))?;
                if id == 0 || id == u16::MAX || data.len() > 128 {
                    return Err(Error::new(
                        "invalidNvLength",
                        "manifest",
                        format!("Invalid NV ID/length: {name}"),
                    ));
                }
                let known = wire::nv_logical_size(id);
                if !data.is_empty() && known > 0 && data.len() != known {
                    return Err(Error::new(
                        "invalidNvLength",
                        "manifest",
                        format!("NV {id} expected known length {known}, got {}", data.len()),
                    ));
                }
                Target::Nv { id }
            } else {
                if name.split('/').any(|n| n.starts_with("NvItem__")) {
                    return Err(Error::new(
                        "invalidNvId",
                        "manifest",
                        "NV files must be at preset root",
                    ));
                }
                let (remote, mode, entry_type) = if let Some((base, extra)) = name.rsplit_once("__")
                {
                    if let Some((mode, ty)) = extra.split_once('_') {
                        match (u32::from_str_radix(mode, 16), u32::from_str_radix(ty, 16)) {
                            (Ok(mode), Ok(ty)) if ty == 0 || ty == 15 => {
                                (format!("/{base}"), mode, ty)
                            }
                            _ => {
                                return Err(Error::new(
                                    "invalidMetadata",
                                    "manifest",
                                    format!("Invalid suffix: {name}"),
                                ))
                            }
                        }
                    } else {
                        (format!("/{name}"), wire::DEFAULT_MODE, 0)
                    }
                } else {
                    (format!("/{name}"), wire::DEFAULT_MODE, 0)
                };
                safe_remote(&remote)?;
                if entry_type == 15 {
                    wire::put(&remote, mode, &data)?;
                }
                Target::Efs {
                    path: remote,
                    mode,
                    entry_type,
                }
            };
            let key = match &target {
                Target::Nv { id } => format!("nv:{id}"),
                Target::Efs { path, .. } => format!("efs:{path}"),
            };
            if !targets.insert(key) {
                return Err(Error::new(
                    "duplicateTarget",
                    "manifest",
                    "Multiple files map to one device target",
                ));
            }
            entries.push(Entry { name, target, data });
        }
        if entries.is_empty() {
            return Err(Error::new("emptyPreset", "manifest", "No preset targets"));
        }
        Ok(Self {
            entries,
            sha256: hex::encode(sha.finalize()),
        })
    }
    pub fn active(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| !e.empty_nv())
    }
    pub fn load_approved(dir: &Path) -> Result<Self> {
        let plan = Self::load(dir)?;
        let name = dir.file_name().and_then(|name| name.to_str());
        if !APPROVED_PRESETS
            .iter()
            .any(|(folder, hash)| Some(*folder) == name && *hash == plan.sha256)
        {
            return Err(Error::new(
                "unapprovedPreset",
                "manifest",
                "Preset folder and hash do not match the pinned balance manifest",
            ));
        }
        Ok(plan)
    }
}
const APPROVED_PRESETS: &[(&str, &str)] = &[
    (
        "XPERIAsSKT1",
        "35e67f181a95f0126577304405575169321d6366db1d098dd320d8c59cb9d0a6",
    ),
    (
        "XPERIAsSKT2",
        "82ffa5d50d3e6140c038724163ebfb1b4dbf27f9d2806d08a4cc674ba85dc04d",
    ),
    (
        "XPERIAsonyKT1",
        "e8262cb4e56fbb0ed18fb17646f9b91b8d14fd4aefdeae6ffe38b34b778be791",
    ),
    (
        "XPERIAsonyKT2",
        "98cdf9244bf9daecbbb55e0219fbc08ed3b7a248017ae951851dfa790077b11e",
    ),
    (
        "XPERIAsonyLGU1",
        "ac3a078c07f6f38e8223fc96f4ec8598bdd3a2d45f14557c3ff232b84da8e2e9",
    ),
    (
        "XPERIAsonyLGU2",
        "eb24e89fda86eb59ced1640ec093c8fe9e77969dcbde199ae5d63f20fc0c22d7",
    ),
    (
        "LGU1＿for＿V＿1st",
        "c328be4fdede00810a6a68d4e8bf264f2b9d6ed1d80228499041c75dedc2f2a0",
    ),
    (
        "LGU1＿for＿V＿1st_Subscription01",
        "bda0578be86af85d6403e3f7da6d653c5f0c6bae0f4f53c1d306ade71af92a2a",
    ),
];
impl Entry {
    pub fn empty_nv(&self) -> bool {
        matches!(self.target, Target::Nv { .. }) && self.data.is_empty()
    }
}
pub fn validate_set(plans: &[Plan]) -> Result<()> {
    if plans.is_empty() {
        return Err(Error::new(
            "emptyPresetSet",
            "preflight",
            "Select at least one preset",
        ));
    }
    let mut shared = std::collections::BTreeMap::<String, &Entry>::new();
    for plan in plans {
        for entry in &plan.entries {
            let key = match &entry.target {
                Target::Nv { id } => format!("NV {id}"),
                Target::Efs { path, .. } => format!("EFS {path}"),
            };
            if let Some(previous) = shared.get(&key) {
                if previous.target != entry.target || previous.data != entry.data {
                    return Err(Error::new("presetConflict","preflight",format!("Selected presets disagree on shared {key} (content/mode/type). Global NV has no SIM namespace; no DIAG or mutation performed")));
                }
            } else {
                shared.insert(key, entry);
            }
        }
    }
    Ok(())
}
