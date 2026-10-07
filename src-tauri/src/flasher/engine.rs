//! Transport-independent SIN transfer runner. Not reachable from a hardware command yet.
use super::{
    error, package,
    policy::{self, Disposition},
    protocol::S1,
    sin::{self, SinIndex},
    transport::FlashTransport,
    Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
};

#[derive(Clone)]
pub struct ImageTask {
    pub path: PathBuf,
    pub relative_path: String,
    pub file_sha256: String,
    pub index: SinIndex,
    pub target: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub sequence: u64,
    pub operation: String,
    pub state: String,
    pub target: Option<String>,
    pub member: Option<usize>,
}
pub trait Journal {
    /// Must durably store Intent before any I/O; an unpaired Intent is an unknown result.
    fn store(&mut self, record: &Record) -> Result<()>;
}

/// Uses the existing atomic-write helper. New run only; existing evidence is never overwritten.
pub struct FileJournal {
    path: PathBuf,
    records: Vec<Record>,
}
impl FileJournal {
    pub fn create(path: &Path) -> Result<Self> {
        if path.exists() || !path.is_absolute() {
            return Err(error("JOURNAL", "New absolute journal path is required"));
        }
        // Reserve the run path before persisting records; no check-then-overwrite of prior history.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .map_err(|_| error("JOURNAL", "Cannot reserve run journal"))?;
        Ok(Self {
            path: path.into(),
            records: Vec::new(),
        })
    }
}
impl Journal for FileJournal {
    fn store(&mut self, record: &Record) -> Result<()> {
        self.records.push(record.clone());
        let bytes = serde_json::to_vec(&serde_json::json!({"schema":1,"upstreamCommit":super::UPSTREAM_COMMIT,"records":self.records})).map_err(|_| error("JOURNAL", "Cannot encode journal"))?;
        if bytes.len() > 8 * 1024 * 1024 {
            return Err(error("JOURNAL", "Journal size limit exceeded"));
        }
        crate::storage::atomic_write(&self.path, &bytes)
            .map_err(|_| error("JOURNAL", "Cannot persist run evidence"))
    }
}

fn stamp(
    journal: &mut dyn Journal,
    sequence: &mut u64,
    operation: &str,
    state: &str,
    target: Option<&str>,
    member: Option<usize>,
) -> Result<()> {
    *sequence += 1;
    journal.store(&Record {
        sequence: *sequence,
        operation: operation.into(),
        state: state.into(),
        target: target.map(str::to_owned),
        member,
    })
}
fn cancelled(cancel: &AtomicBool) -> Result<()> {
    if cancel.load(Ordering::Acquire) {
        Err(error(
            "CANCELLED",
            "Stopped at an acknowledged transfer boundary",
        ))
    } else {
        Ok(())
    }
}

fn preflight(images: &[ImageTask], max_download: u32) -> Result<Vec<File>> {
    if images.is_empty() || images.len() > 8192 {
        return Err(error("PLAN", "Invalid image plan size"));
    }
    let mut files = Vec::new();
    let mut targets = std::collections::BTreeSet::new();
    for image in images {
        policy::partition_name(&image.target)?;
        let base = &image.index.partition;
        if image.target != *base
            && image.target != format!("{base}_a")
            && image.target != format!("{base}_b")
        {
            return Err(error("TARGET", "Plan target differs from SIN partition"));
        }
        if !targets.insert(image.target.clone()) {
            return Err(error("TARGET", "Duplicate plan target"));
        }
        if policy::decide(&image.relative_path, Some(base), &Default::default())?.disposition
            != Disposition::Include
        {
            return Err(error(
                "PRESERVED",
                "Plan contains a preserved or unsupported image",
            ));
        }
        if image
            .index
            .members
            .iter()
            .any(|m| m.bytes > max_download as u64)
        {
            return Err(error(
                "DOWNLOAD_LIMIT",
                "Plan chunk exceeds device download limit",
            ));
        }
        let mut file = package::open_checked(&image.path)?;
        let mut hash = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let n = file
                .read(&mut buffer)
                .map_err(|_| error("READ", "Cannot rehash planned SIN"))?;
            if n == 0 {
                break;
            }
            hash.update(&buffer[..n]);
        }
        if hex::encode(hash.finalize()) != image.file_sha256 {
            return Err(error("CHANGED", "SIN file hash changed after inspection"));
        }
        if sin::inspect(&mut file)? != image.index {
            return Err(error(
                "CHANGED",
                "SIN member index changed after inspection",
            ));
        }
        files.push(file);
    }
    Ok(files)
}

/// Executes only an already-selected image/slot list. Device/profile selection is a separate gate.
/// Since that gate is not validated, no Tauri/CLI path currently constructs a hardware runner.
pub fn run<T: FlashTransport>(
    s1: &mut S1<T>,
    images: &[ImageTask],
    journal: &mut dyn Journal,
    cancel: &AtomicBool,
) -> Result<()> {
    cancelled(cancel)?;
    let mut files = preflight(images, s1.max_download)?;
    let mut sequence = 0;
    let outcome = (|| {
        cancelled(cancel)?;
        stamp(
            journal,
            &mut sequence,
            "session-enter",
            "intent",
            None,
            None,
        )?;
        s1.session_flag(true)?;
        stamp(journal, &mut sequence, "session-enter", "ack", None, None)?;
        for (image, file) in images.iter().zip(files.iter_mut()) {
            cancelled(cancel)?;
            file.seek(SeekFrom::Start(0))
                .map_err(|_| error("READ", "Cannot seek SIN"))?;
            let (_, stream) = sin::decoded(file)?;
            let mut archive = tar::Archive::new(stream);
            let mut seen = 0usize;
            for entry in archive
                .entries()
                .map_err(|_| error("SIN", "Cannot read SIN"))?
                .raw(true)
            {
                cancelled(cancel)?;
                let mut entry = entry.map_err(|_| error("SIN", "Cannot read SIN member"))?;
                let expected = image
                    .index
                    .members
                    .get(seen)
                    .ok_or_else(|| error("CHANGED", "Unexpected SIN member"))?;
                if entry.path_bytes().as_ref() != expected.name.as_bytes()
                    || entry.size() != expected.bytes
                {
                    return Err(error("CHANGED", "SIN member changed during execution"));
                }
                let operation = if seen == 0 { "signature" } else { "download" };
                stamp(
                    journal,
                    &mut sequence,
                    operation,
                    "intent",
                    Some(&image.target),
                    Some(seen),
                )?;
                s1.payload(expected.bytes, &mut entry, &expected.sha256, seen == 0)?;
                stamp(
                    journal,
                    &mut sequence,
                    operation,
                    "ack",
                    Some(&image.target),
                    Some(seen),
                )?;
                if seen == 1 {
                    stamp(
                        journal,
                        &mut sequence,
                        "erase",
                        "intent",
                        Some(&image.target),
                        None,
                    )?;
                    s1.erase(&image.target)?;
                    stamp(
                        journal,
                        &mut sequence,
                        "erase",
                        "ack",
                        Some(&image.target),
                        None,
                    )?;
                }
                if seen > 0 {
                    // A successfully downloaded chunk must be committed before accepting cancellation.
                    stamp(
                        journal,
                        &mut sequence,
                        "flash",
                        "intent",
                        Some(&image.target),
                        Some(seen),
                    )?;
                    s1.flash(&image.target)?;
                    stamp(
                        journal,
                        &mut sequence,
                        "flash",
                        "ack",
                        Some(&image.target),
                        Some(seen),
                    )?;
                }
                seen += 1;
            }
            if seen != image.index.members.len() {
                return Err(error("CHANGED", "SIN ended before planned chunks"));
            }
        }
        // No partially written package is marked complete or automatically rebooted.
        cancelled(cancel)?;
        stamp(journal, &mut sequence, "session-exit", "intent", None, None)?;
        s1.session_flag(false)?;
        stamp(journal, &mut sequence, "session-exit", "ack", None, None)?;
        stamp(journal, &mut sequence, "sync", "intent", None, None)?;
        s1.okay("Sync")?;
        stamp(journal, &mut sequence, "sync", "ack", None, None)?;
        stamp(
            journal,
            &mut sequence,
            "stock-images",
            "written-not-boot-verified",
            None,
            None,
        )
    })();
    if let Err(reason) = &outcome {
        // Retain any unmatched Intent even if recording the final status also fails.
        let state = if reason.starts_with("FLASH_CANCELLED|") {
            "cancelled-partial"
        } else {
            "failed-revalidation-required"
        };
        let _ = stamp(journal, &mut sequence, "run", state, None, None);
    }
    outcome
}
