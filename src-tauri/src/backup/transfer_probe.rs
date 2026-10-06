//! Developer-only, bounded comparison using the real backup writer and verification.
//! Reads the phone; stores samples separately without modifying a backup manifest.
use super::{puller, quarantine::HashingWriter, walker};
use adb_client::ADBDeviceExt;
use serde_json::{json, Value};
use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

fn select_samples(
    mut files: Vec<walker::WalkedEntry>,
    max_files: usize,
    budget: u64,
) -> Vec<walker::WalkedEntry> {
    files.retain(|file| file.size <= budget);
    files.sort_by(|a, b| a.size.cmp(&b.size).then(a.remote.cmp(&b.remote)));
    let mut files = VecDeque::from(files);
    let mut selected = Vec::new();
    let mut bytes = 0u64;
    // Include small and large files so session overhead and throughput both appear.
    let mut largest = true;
    while !files.is_empty() && selected.len() < max_files {
        let file = if largest {
            files.pop_back()
        } else {
            files.pop_front()
        }
        .unwrap();
        largest = !largest;
        if bytes + file.size <= budget {
            bytes += file.size;
            selected.push(file);
        }
    }
    selected
}

fn round(
    dev: &mut dyn ADBDeviceExt,
    files: &[walker::WalkedEntry],
    output: &Path,
    batch: bool,
) -> Result<(Value, Vec<(u64, String)>), String> {
    std::fs::create_dir(output).map_err(|e| e.to_string())?;
    let started = Instant::now();
    let reused = if batch {
        dev.begin_sync_batch().map_err(|e| e.to_string())?
    } else {
        false
    };
    let result = (|| {
        let mut records = Vec::new();
        for (index, file) in files.iter().enumerate() {
            let relative = PathBuf::from(format!("sample-{index}.bin"));
            let entry = puller::pull_to_disk_limited(
                dev,
                &file.remote,
                &relative,
                output,
                file.size,
                file.mtime,
            );
            if let Some(error) = entry.error {
                return Err(error);
            }
            records.push((entry.size, entry.sha256.ok_or("sample hash missing")?));
        }
        Ok(records)
    })();
    let close = if batch {
        dev.end_sync_batch().map_err(|e| e.to_string())
    } else {
        Ok(())
    };
    let seconds = started.elapsed().as_secs_f64();
    let records = result?;
    close?;
    // Independently reread PC samples; this work is outside the transfer timer.
    for (index, expected) in records.iter().enumerate() {
        let mut input = std::fs::File::open(output.join(format!("sample-{index}.bin")))
            .map_err(|e| e.to_string())?;
        let mut hash = HashingWriter::new(std::io::sink());
        std::io::copy(&mut input, &mut hash).map_err(|e| e.to_string())?;
        let (_, digest, size) = hash.finish();
        if &(size, digest) != expected {
            return Err(format!("PC sample {index} failed size/hash verification"));
        }
    }
    let bytes: u64 = records.iter().map(|record| record.0).sum();
    Ok((
        json!({"mode": if batch {"batch"} else {"per-file"}, "sessionReuse": reused, "files": records.len(), "bytes": bytes, "seconds": seconds, "mibPerSecond": bytes as f64 / 1048576.0 / seconds.max(0.000001), "pcHashesVerified": true}),
        records,
    ))
}

pub async fn run(
    serial: String,
    remote_root: String,
    dest: String,
    max_files: usize,
    max_total_bytes: u64,
) -> Result<Value, String> {
    if !remote_root.starts_with("/sdcard/")
        || remote_root.contains('\0')
        || remote_root
            .split('/')
            .any(|part| part == ".." || part == ".")
    {
        return Err("Probe source must be a directory under /sdcard without traversal".into());
    }
    if !(1..=64).contains(&max_files) || !(1..=512 * 1024 * 1024).contains(&max_total_bytes) {
        return Err("Probe limits: 1–64 files, at most 512 MiB per round".into());
    }
    let destination = PathBuf::from(dest);
    if !destination.is_dir() {
        return Err("Probe destination directory does not exist".into());
    }
    let operation = super::Operation::acquire()?;
    crate::tasks::blocking("백업 전송 비교", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            let walked = walker::walk_cancellable(dev, &remote_root, &|_| false, &puller::CancelFlag::new());
            if !walked.errors.is_empty() { return Err(walked.errors.join("; ")); }
            let candidates = select_samples(walked.files, max_files, max_total_bytes);
            let mut files = Vec::new();
            let mut bytes = 0u64;
            for mut candidate in candidates {
                // LIST is u32; check actual video sizes before enforcing sample budgets.
                let actual = dev.stat_extended(&candidate.remote).map_err(|e| e.to_string())?.ok_or("Probe sample disappeared")?;
                candidate.size = actual.size;
                if bytes + candidate.size <= max_total_bytes { bytes += candidate.size; files.push(candidate); }
            }
            if files.is_empty() { return Err("No files fit the probe limits".into()); }
            let nonce = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|e| e.to_string())?.as_nanos();
            let output = destination.join(format!("transfer-probe-{nonce}"));
            std::fs::create_dir(&output).map_err(|e| e.to_string())?;
            let mut rounds = Vec::new();
            let mut reference = None;
            // ABBA reduces the influence of always testing one mode with a warm cache.
            for (index, batch) in [false, true, true, false].into_iter().enumerate() {
                let (report, signatures) = round(dev, &files, &output.join(format!("round-{index}")), batch)?;
                if reference.as_ref().is_some_and(|expected| expected != &signatures) {
                    return Err("Phone sample contents changed between comparison rounds".into());
                }
                reference = Some(signatures);
                rounds.push(report);
            }
            Ok(json!({"dir": output, "rounds": rounds, "sameFileHashesAcrossRounds": true, "note": "Compares per-file versus reused sessions with identical buffering and durability; not a USB bandwidth measurement."}))
        })
    }).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_sample_that_grows_past_its_limit_is_rejected_and_temporary_files_are_removed() {
        let mut device = super::super::fake_device::FakeADBDevice::new();
        device.add_file("/sdcard/growing.bin", b"larger-than-before", 123, 0o644);
        let files = vec![walker::WalkedEntry {
            remote: "/sdcard/growing.bin".into(),
            size: 1,
            mtime: 123,
        }];
        let root = tempfile::tempdir().unwrap();
        let output = root.path().join("bounded");
        assert!(round(&mut device, &files, &output, true).is_err());
        assert_eq!(std::fs::read_dir(output).unwrap().count(), 0);
        assert!(!device.sync_batch_active);
    }
    #[test]
    fn samples_respect_count_and_byte_limits_and_include_both_sizes() {
        let files = [0, 1, 10, 30, 90, 1000]
            .into_iter()
            .enumerate()
            .map(|(index, size)| walker::WalkedEntry {
                remote: format!("/sdcard/test/{index}"),
                size,
                mtime: 0,
            })
            .collect();
        let selected = select_samples(files, 3, 100);
        assert_eq!(selected.len(), 3);
        assert_eq!(selected.iter().map(|file| file.size).sum::<u64>(), 91);
        assert_eq!(
            selected.iter().map(|file| file.size).collect::<Vec<_>>(),
            [90, 0, 1]
        );
    }

    #[test]
    fn comparison_uses_real_writer_verifies_disk_and_finishes_failed_batches() {
        let mut device = super::super::fake_device::FakeADBDevice::new();
        device.add_file("/sdcard/photo.bin", b"photo-bytes", 123, 0o644);
        let files = vec![walker::WalkedEntry {
            remote: "/sdcard/photo.bin".into(),
            size: 11,
            mtime: 123,
        }];
        let root = tempfile::tempdir().unwrap();
        let (first, signatures) =
            round(&mut device, &files, &root.path().join("per-file"), false).unwrap();
        let (second, repeated) =
            round(&mut device, &files, &root.path().join("batch"), true).unwrap();
        assert_eq!(signatures, repeated);
        assert_eq!(first["pcHashesVerified"], true);
        assert_eq!(second["sessionReuse"], true);
        device.fail_pull("/sdcard/photo.bin");
        assert!(round(&mut device, &files, &root.path().join("failed"), true).is_err());
        assert!(!device.sync_batch_active);
        assert_eq!(device.sync_batch_calls, ["begin", "end", "begin", "end"]);
    }
}
