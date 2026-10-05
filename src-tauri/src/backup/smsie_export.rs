//! Complete SMS Import/Export payload validation, including ZIP entry CRCs.
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
const MAX_JSON: u64 = 128 * 1024 * 1024;
const MAX_LINE: u64 = 8 * 1024 * 1024;
const MAX_EXPANDED: u64 = 32 * 1024 * 1024 * 1024;

pub(super) fn validate(path: &Path, item: &str) -> Result<(), String> {
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    if item == "calllog" {
        if file.metadata().map_err(|e| e.to_string())?.len() > MAX_JSON {
            return Err("통화 기록 JSON이 허용 크기(128 MiB)를 초과했습니다".into());
        }
        let value: serde_json::Value = serde_json::from_reader(file).map_err(|e| e.to_string())?;
        if !value
            .as_array()
            .is_some_and(|a| a.iter().all(|v| v.is_object()))
        {
            return Err("통화 기록은 JSON 객체 배열이어야 합니다".into());
        }
        return Ok(());
    }
    if item != "sms" {
        return Err("알 수 없는 문자 백업 종류입니다".into());
    }
    let mut archive = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let mut expanded = 0u64;
    let mut messages = 0;
    for i in 0..archive.len() {
        let entry = archive.by_index(i).map_err(|e| e.to_string())?;
        expanded = expanded.checked_add(entry.size()).ok_or("ZIP 크기 초과")?;
        if expanded > MAX_EXPANDED {
            return Err("문자 ZIP의 해제 크기가 허용 크기(32 GiB)를 초과했습니다".into());
        }
        let size = entry.size();
        let name = entry.name().to_string();
        let is_dir = entry.is_dir();
        if entry.enclosed_name().is_none() || entry.is_symlink() {
            return Err("문자 ZIP에 잘못된 경로가 있습니다".into());
        }
        let mut reader = BufReader::new(entry.take(size.saturating_add(1)));
        let actual = if name == "messages.ndjson" {
            messages += 1;
            let mut actual = 0;
            loop {
                let mut line = Vec::new();
                let n = reader
                    .by_ref()
                    .take(MAX_LINE + 1)
                    .read_until(b'\n', &mut line)
                    .map_err(|e| e.to_string())?;
                if n == 0 {
                    break;
                }
                if n as u64 > MAX_LINE {
                    return Err("문자 JSON 행이 너무 큽니다".into());
                }
                let value: serde_json::Value =
                    serde_json::from_slice(&line).map_err(|e| e.to_string())?;
                if !value.is_object() {
                    return Err("문자 JSON 행은 객체여야 합니다".into());
                }
                actual += n as u64;
            }
            actual
        } else if name.starts_with("data/") || is_dir {
            std::io::copy(&mut reader, &mut std::io::sink()).map_err(|e| e.to_string())?
        } else {
            return Err(format!("알 수 없는 문자 ZIP 항목: {name}"));
        };
        if actual != size {
            return Err(format!("문자 ZIP 항목 크기 불일치: {name}"));
        }
    }
    if messages != 1 {
        return Err("문자 ZIP에 messages.ndjson이 하나 있어야 합니다".into());
    }
    Ok(())
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use std::io::{Cursor, Write};
    pub fn export_zip(messages: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("messages.ndjson", zip::write::SimpleFileOptions::default())
            .unwrap();
        zip.write_all(messages).unwrap();
        zip.finish().unwrap().into_inner()
    }
    #[test]
    fn empty_exports_pass_and_interrupted_or_wrong_payloads_fail() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("export");
        for bytes in [export_zip(b""), export_zip(b"{\"body\":\"hello\"}\n")] {
            std::fs::write(&path, &bytes).unwrap();
            assert!(validate(&path, "sms").is_ok());
            for len in [0, 10, bytes.len() - 22, bytes.len() - 1] {
                std::fs::write(&path, &bytes[..len]).unwrap();
                assert!(validate(&path, "sms").is_err());
            }
        }
        for bytes in [
            b"".as_slice(),
            b"[",
            b"[{}",
            b"{}",
            b"[null]",
            b"[]trailing",
        ] {
            std::fs::write(&path, bytes).unwrap();
            assert!(validate(&path, "calllog").is_err());
        }
        for bytes in [b"[]".as_slice(), b"[{\"number\":\"123\"}]"] {
            std::fs::write(&path, bytes).unwrap();
            assert!(validate(&path, "calllog").is_ok());
        }
        std::fs::write(&path, export_zip(b"{\"body\":")).unwrap();
        assert!(validate(&path, "sms").is_err());
    }

    #[test]
    fn corrupted_crc_and_truncated_attachment_are_not_complete_exports() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("messages.zip");
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        zip.start_file("messages.ndjson", options).unwrap();
        zip.write_all(b"{}\n").unwrap();
        zip.start_file("data/attachment", options).unwrap();
        zip.write_all(b"unique attachment").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        std::fs::write(&path, &bytes).unwrap();
        assert!(validate(&path, "sms").is_ok());
        let mut corrupted = bytes.clone();
        let at = corrupted
            .windows(17)
            .position(|s| s == b"unique attachment")
            .unwrap();
        corrupted[at] ^= 1;
        std::fs::write(&path, corrupted).unwrap();
        assert!(validate(&path, "sms").is_err());
        std::fs::write(&path, &bytes[..at + 5]).unwrap();
        assert!(validate(&path, "sms").is_err());
    }
}
