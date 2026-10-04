//! 기본 문자 앱 원복 기록 — 역할 변경 전에 기기별로 디스크에 남긴다.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoleRecord {
    version: u32,
    device_key: String,
    #[serde(deserialize_with = "deserialize_holder")]
    pub previous_holder: Option<String>,
}

fn deserialize_holder<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(d)
}

pub fn valid_package(package: &str) -> bool {
    package.contains('.')
        && package.split('.').all(|part| {
            !part.is_empty() && part.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

fn path_for(dir: &Path, key: &str) -> Result<PathBuf, String> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("문자 앱 원복 기록의 기기 키가 잘못됐습니다".into());
    }
    Ok(dir.join(format!("{key}.json")))
}

pub fn load(dir: &Path, key: &str) -> Result<Option<RoleRecord>, String> {
    let path = path_for(dir, key)?;
    let Some(raw) = crate::storage::read_bounded(&path, 16 * 1024)? else {
        return Ok(None);
    };
    let record: RoleRecord =
        serde_json::from_slice(&raw).map_err(|e| format!("문자 앱 원복 기록 해석 실패: {e}"))?;
    if record.version != 1
        || record.device_key != key
        || record
            .previous_holder
            .as_ref()
            .is_some_and(|p| !valid_package(p))
    {
        return Err("문자 앱 원복 기록이 현재 기기와 맞지 않거나 손상됐습니다".into());
    }
    Ok(Some(record))
}

pub fn save(dir: &Path, key: &str, previous_holder: Option<String>) -> Result<(), String> {
    if previous_holder.as_ref().is_some_and(|p| !valid_package(p)) {
        return Err("원래 문자 앱 패키지 형식이 잘못됐습니다".into());
    }
    let record = RoleRecord {
        version: 1,
        device_key: key.into(),
        previous_holder,
    };
    let data = serde_json::to_vec(&record).map_err(|e| e.to_string())?;
    crate::storage::atomic_write(&path_for(dir, key)?, &data)
}

pub fn remove(dir: &Path, key: &str) -> Result<(), String> {
    match std::fs::remove_file(path_for(dir, key)?) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("문자 앱 원복 기록 정리 실패: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn records_are_isolated_by_device_and_survive_store_recreation() {
        let dir = tempfile::tempdir().unwrap();
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        save(dir.path(), &a, Some("com.example.sms".into())).unwrap();
        save(dir.path(), &b, Some("com.other.sms".into())).unwrap();
        assert_eq!(
            load(dir.path(), &a)
                .unwrap()
                .unwrap()
                .previous_holder
                .as_deref(),
            Some("com.example.sms")
        );
        assert_eq!(
            load(dir.path(), &b)
                .unwrap()
                .unwrap()
                .previous_holder
                .as_deref(),
            Some("com.other.sms")
        );
        remove(dir.path(), &a).unwrap();
        assert!(load(dir.path(), &a).unwrap().is_none());
        assert!(load(dir.path(), &b).unwrap().is_some());
        assert!(save(dir.path(), "../escape", None).is_err());
        assert!(save(dir.path(), &a, Some("com.sms;rm".into())).is_err());
    }
    #[test]
    fn mismatched_or_invalid_disk_record_is_never_replayed() {
        let dir = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        save(dir.path(), &key, Some("com.example.sms".into())).unwrap();
        let file = dir.path().join(format!("{key}.json"));
        let raw = std::fs::read_to_string(&file)
            .unwrap()
            .replace(&key, &"b".repeat(64));
        std::fs::write(&file, raw).unwrap();
        assert!(load(dir.path(), &key).is_err());
        std::fs::write(&file, "{}").unwrap();
        assert!(load(dir.path(), &key).is_err());
        std::fs::write(&file, format!("{{\"version\":1,\"deviceKey\":\"{key}\"}}")).unwrap();
        assert!(load(dir.path(), &key).is_err());
    }
}
