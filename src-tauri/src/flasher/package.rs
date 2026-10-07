use super::{
    error,
    policy::{self, Decision, Disposition},
    sin::{self, SinIndex},
    Result, UPSTREAM_COMMIT,
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs::{self, File},
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

const MAX_FILES: usize = 8192;
const MAX_PACKAGE: u64 = 128 * 1024 * 1024 * 1024;
const MAX_XML: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageFile {
    pub relative_path: String,
    pub bytes: u64,
    pub sha256: String,
    pub sin: Option<SinIndex>,
    pub decision: Decision,
}
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackageReport {
    pub upstream_commit: String,
    pub target_fingerprint: String,
    pub manifest_sha256: String,
    pub total_bytes: u64,
    pub candidate_bytes: u64,
    pub files: Vec<PackageFile>,
    pub blockers: Vec<String>,
    pub write_ready: bool,
}

fn plain(path: &Path) -> Result<fs::Metadata> {
    let meta =
        fs::symlink_metadata(path).map_err(|_| error("FILE", "Cannot inspect package path"))?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(error("LINK", "Reparse points are forbidden in packages"));
        }
    }
    if meta.file_type().is_symlink() {
        return Err(error("LINK", "Symbolic links are forbidden in packages"));
    }
    Ok(meta)
}

pub fn open_checked(path: &Path) -> Result<File> {
    if !plain(path)?.is_file() {
        return Err(error("FILE", "Package input must be a regular file"));
    }
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        // Refuse concurrent write/delete handles and inspect the opened object, not only its path.
        options.share_mode(1).custom_flags(0x00200000); // FILE_FLAG_OPEN_REPARSE_POINT
        let file = options
            .open(path)
            .map_err(|_| error("FILE", "Cannot lock package file for inspection"))?;
        if file
            .metadata()
            .map_err(|_| error("FILE", "Cannot inspect opened file"))?
            .file_attributes()
            & 0x400
            != 0
        {
            return Err(error("LINK", "Opened input is a reparse point"));
        }
        Ok(file)
    }
    #[cfg(not(windows))]
    options
        .open(path)
        .map_err(|_| error("FILE", "Cannot open package file"))
}

fn collect(
    root: &Path,
    relative: &str,
    depth: usize,
    output: &mut Vec<(String, PathBuf)>,
    names: &mut BTreeSet<String>,
) -> Result<()> {
    if depth > 4 {
        return Err(error("DEPTH", "Package directory nesting limit exceeded"));
    }
    let dir = if relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(relative)
    };
    for entry in
        fs::read_dir(dir).map_err(|_| error("DIRECTORY", "Cannot list package directory"))?
    {
        let entry = entry.map_err(|_| error("DIRECTORY", "Cannot read package entry"))?;
        let name = entry
            .file_name()
            .into_string()
            .map_err(|_| error("PATH", "Invalid package filename"))?;
        let path = if relative.is_empty() {
            name
        } else {
            format!("{relative}/{name}")
        };
        let normalized = policy::safe_relative(&path)?;
        if names.len() >= MAX_FILES || !names.insert(normalized) {
            return Err(error(
                "DUPLICATE",
                "Too many entries or case-colliding package paths",
            ));
        }
        let meta = plain(&entry.path())?;
        if meta.is_dir() {
            collect(root, &path, depth + 1, output, names)?;
        } else if meta.is_file() {
            output.push((path, entry.path()));
        } else {
            return Err(error("FILE", "Unsupported filesystem entry in package"));
        }
    }
    Ok(())
}

pub(crate) fn update_metadata(xml: &str) -> Result<(String, BTreeSet<String>)> {
    let doc =
        roxmltree::Document::parse(xml).map_err(|_| error("XML", "Cannot parse update.xml"))?;
    let fingerprints: Vec<_> = doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("fingerprint"))
        .collect();
    if fingerprints.len() != 1 {
        return Err(error(
            "FINGERPRINT",
            "update.xml must contain one fingerprint",
        ));
    }
    let fingerprint = fingerprints[0].text().unwrap_or("").trim().to_owned();
    if fingerprint.is_empty() || fingerprint.len() > 512 {
        return Err(error("FINGERPRINT", "Invalid target fingerprint"));
    }
    let mut noerase = BTreeSet::new();
    for node in doc
        .descendants()
        .filter(|n| n.is_element() && n.tag_name().name().eq_ignore_ascii_case("noerase"))
    {
        if node.attributes().len() != 0 {
            return Err(error("NOERASE", "Unsupported NOERASE attributes"));
        }
        let mut count = 0;
        for leaf in node
            .descendants()
            .filter(|n| n.is_element() && !n.children().any(|c| c.is_element()))
        {
            if leaf.attributes().len() != 0 {
                return Err(error("NOERASE", "Unsupported NOERASE file declaration"));
            }
            let filename = leaf.text().unwrap_or("").trim();
            let name = policy::safe_relative(filename)?;
            if !name.ends_with(".sin") && !name.ends_with(".ta") {
                return Err(error(
                    "NOERASE",
                    "NOERASE target must be a firmware input file",
                ));
            }
            noerase.insert(name);
            count += 1;
        }
        if count == 0 {
            return Err(error("NOERASE", "Empty NOERASE declaration"));
        }
    }
    Ok((fingerprint, noerase))
}

pub fn inspect(root: &Path, expected_fingerprint: &str) -> Result<PackageReport> {
    if !root.is_absolute() || !plain(root)?.is_dir() {
        return Err(error(
            "DIRECTORY",
            "An absolute regular package directory is required",
        ));
    }
    let mut inputs = Vec::new();
    collect(root, "", 0, &mut inputs, &mut BTreeSet::new())?;
    inputs.sort_by(|a, b| a.0.cmp(&b.0));
    let update = inputs
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("update.xml"))
        .ok_or_else(|| error("XML", "Missing update.xml"))?;
    let mut xml_file = open_checked(&update.1)?;
    let mut xml = String::new();
    (&mut xml_file)
        .take(MAX_XML + 1)
        .read_to_string(&mut xml)
        .map_err(|_| error("XML", "Cannot read update.xml"))?;
    if xml.len() as u64 > MAX_XML {
        return Err(error("XML_SIZE", "update.xml exceeds size limit"));
    }
    let (fingerprint, noerase) = update_metadata(&xml)?;
    if expected_fingerprint.trim().is_empty() || fingerprint != expected_fingerprint {
        return Err(error(
            "FINGERPRINT",
            "Package fingerprint does not match requested target",
        ));
    }
    let mut blockers = vec![
        "device-identity-region-profile-not-validated".to_owned(),
        "hardware-flash-api-not-enabled".to_owned(),
    ];
    if noerase.is_empty() {
        blockers.push("noerase-preservation-list-missing".into());
    }
    let mut files = Vec::new();
    let mut total_bytes = 0u64;
    let mut candidate_bytes = 0u64;
    for (relative_path, path) in inputs {
        let mut file = open_checked(&path)?;
        let bytes = file
            .metadata()
            .map_err(|_| error("FILE", "Cannot inspect package file length"))?
            .len();
        total_bytes = total_bytes
            .checked_add(bytes)
            .filter(|&n| n <= MAX_PACKAGE)
            .ok_or_else(|| error("PACKAGE_SIZE", "Package exceeds size limit"))?;
        let mut hasher = Sha256::new();
        let mut seen = 0u64;
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|_| error("READ", "Cannot hash package file"))?;
            if n == 0 {
                break;
            }
            seen += n as u64;
            if seen > bytes {
                return Err(error("CHANGED", "Package file changed during inspection"));
            }
            hasher.update(&buffer[..n]);
        }
        if seen != bytes {
            return Err(error("CHANGED", "Package file length changed"));
        }
        let sha256 = hex::encode(hasher.finalize());
        file.seek(SeekFrom::Start(0))
            .map_err(|_| error("READ", "Cannot seek package file"))?;
        let normalized = policy::safe_relative(&relative_path)?;
        let sin = if normalized.ends_with(".sin") {
            Some(sin::inspect(&mut file)?)
        } else {
            None
        };
        let decision = if normalized.ends_with(".xml") {
            if bytes > MAX_XML {
                return Err(error("XML_SIZE", "Package XML exceeds size limit"));
            }
            let mut contents = String::new();
            file.read_to_string(&mut contents)
                .map_err(|_| error("XML", "Cannot read package XML"))?;
            roxmltree::Document::parse(&contents)
                .map_err(|_| error("XML", "Invalid package XML"))?;
            if normalized.starts_with("partition/")
                || normalized.ends_with("partition_delivery.xml")
            {
                blockers.push("partition-layout-not-validated".into());
            }
            if normalized.ends_with("boot_delivery.xml") {
                blockers.push("boot-delivery-selection-not-validated".into());
            }
            Decision {
                disposition: Disposition::Preserve,
                reason: "control-metadata-not-flashed".into(),
            }
        } else {
            policy::decide(
                &relative_path,
                sin.as_ref().map(|s| s.partition.as_str()),
                &noerase,
            )?
        };
        if decision.disposition == Disposition::Include {
            candidate_bytes += bytes;
        }
        if decision.disposition == Disposition::Block {
            blockers.push(decision.reason.clone());
        }
        files.push(PackageFile {
            relative_path,
            bytes,
            sha256,
            sin,
            decision,
        });
    }
    for excluded in &noerase {
        if !files
            .iter()
            .any(|f| f.relative_path.eq_ignore_ascii_case(excluded))
        {
            blockers.push("noerase-reference-unresolved".into());
        }
    }
    if !files
        .iter()
        .any(|f| f.sin.is_some() && f.decision.disposition == Disposition::Include)
    {
        blockers.push("no-candidate-images".into());
    }
    blockers.sort();
    blockers.dedup();
    let manifest_sha256 = hex::encode(Sha256::digest(
        serde_json::to_vec(&(UPSTREAM_COMMIT, &fingerprint, &files))
            .map_err(|_| error("MANIFEST", "Cannot serialize manifest"))?,
    ));
    Ok(PackageReport {
        upstream_commit: UPSTREAM_COMMIT.into(),
        target_fingerprint: fingerprint,
        manifest_sha256,
        total_bytes,
        candidate_bytes,
        files,
        blockers,
        write_ready: false,
    })
}
