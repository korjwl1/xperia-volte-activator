//! 폰별 장기 기록(2026-10-08 사용자 결정) — 앱 데이터 폴더 `devices/<기기 키>.json`.
//! 진행 기록(journal)은 작업 하나 단위라 끝나면 의미가 약해진다. 이 기록은 폰 하나 기준으로
//! 마지막 백업 폴더와 VoLTE 패치 이력(패치 전 모뎀 설정 사본 위치)을 오래 들고 있어 복원·VoLTE 되돌리기가 쓴다.
//! 기기 키는 원본 시리얼의 SHA-256(device_io::identity_key)이며 원본 시리얼은 남기지 않는다.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

const MAX_RECORD: usize = 256 * 1024;
const MAX_PATCHES: usize = 50;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VoltePatch {
    /// RFC 3339
    pub at: String,
    pub slot: u8,
    pub carrier: String,
    /// 패치 전 모뎀 설정 사본 폴더(snapshot.json 포함)
    pub snapshot: String,
    /// 이 사본으로 되돌린 시각 — 되돌린 패치는 다시 되돌릴 대상이 아니다
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rolled_back_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceRecord {
    #[serde(default)]
    pub last_backup_dir: Option<String>,
    #[serde(default)]
    pub volte_patches: Vec<VoltePatch>,
}

fn valid_key(key: &str) -> Result<(), String> {
    if key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err("기기 키가 올바르지 않습니다".into())
    }
}

fn path_in(dir: &std::path::Path, key: &str) -> Result<PathBuf, String> {
    valid_key(key)?;
    Ok(dir.join("devices").join(format!("{}.json", key.to_ascii_lowercase())))
}

#[cfg(test)]
pub(crate) fn read(dir: &std::path::Path, key: &str) -> Result<Option<DeviceRecord>, String> {
    read_in(dir, key)
}
pub(crate) fn update(dir: &std::path::Path, key: &str, change: impl FnOnce(&mut DeviceRecord)) -> Result<DeviceRecord, String> {
    update_in(dir, key, change)
}
fn read_in(dir: &std::path::Path, key: &str) -> Result<Option<DeviceRecord>, String> {
    let Some(bytes) = crate::storage::read_bounded(&path_in(dir, key)?, MAX_RECORD)? else {
        return Ok(None);
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| format!("기기 기록 해석 실패: {e}"))
}

fn update_in(
    dir: &std::path::Path,
    key: &str,
    change: impl FnOnce(&mut DeviceRecord),
) -> Result<DeviceRecord, String> {
    let mut record = read_in(dir, key)?.unwrap_or_default();
    change(&mut record);
    // 오래된 패치 이력은 앞에서부터 버린다(사본 폴더는 지우지 않는다)
    if record.volte_patches.len() > MAX_PATCHES {
        let extra = record.volte_patches.len() - MAX_PATCHES;
        record.volte_patches.drain(..extra);
    }
    let bytes = serde_json::to_vec_pretty(&record).map_err(|e| e.to_string())?;
    crate::storage::atomic_write(&path_in(dir, key)?, &bytes)?;
    Ok(record)
}

fn data_dir() -> Result<PathBuf, String> {
    crate::app_paths::data_dir().ok_or_else(|| "앱 데이터 폴더 없음".into())
}

#[tauri::command]
pub async fn device_record_get(key: String) -> Result<Option<DeviceRecord>, String> {
    let dir = data_dir()?;
    crate::tasks::blocking("기기 기록 읽기", move || read_in(&dir, &key)).await
}

/// 완료된 백업 폴더를 기억한다 — 복원 화면이 먼저 골라 둔다
#[tauri::command]
pub async fn device_record_set_backup(key: String, dir: String) -> Result<DeviceRecord, String> {
    let data = data_dir()?;
    crate::tasks::blocking("기기 기록 저장", move || {
        update_in(&data, &key, |r| r.last_backup_dir = Some(dir))
    })
    .await
}

/// VoLTE 패치 직전에 만든 모뎀 설정 사본을 기록한다 — VoLTE 되돌리기가 쓴다
#[tauri::command]
pub async fn device_record_add_patch(key: String, patch: VoltePatch) -> Result<DeviceRecord, String> {
    let data = data_dir()?;
    crate::tasks::blocking("기기 기록 저장", move || {
        update_in(&data, &key, |r| r.volte_patches.push(patch))
    })
    .await
}

/// VoLTE 되돌리기가 끝난 사본을 "되돌림"으로 표시한다
#[tauri::command]
pub async fn device_record_mark_rolled_back(key: String, snapshots: Vec<String>, at: String) -> Result<DeviceRecord, String> {
    let data = data_dir()?;
    crate::tasks::blocking("기기 기록 저장", move || {
        update_in(&data, &key, |r| {
            for p in r.volte_patches.iter_mut().filter(|p| snapshots.contains(&p.snapshot)) {
                p.rolled_back_at = Some(at.clone());
            }
        })
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn record_keeps_backup_and_bounded_patch_history_per_device() {
        let dir = tempfile::tempdir().unwrap();
        let key = "a".repeat(64);
        assert_eq!(read_in(dir.path(), &key).unwrap(), None);
        update_in(dir.path(), &key, |r| r.last_backup_dir = Some("D:/b".into())).unwrap();
        for i in 0..(MAX_PATCHES + 3) {
            update_in(dir.path(), &key, |r| {
                r.volte_patches.push(VoltePatch { at: format!("{i}"), slot: 1, carrier: "SKT".into(), snapshot: "s".into(), rolled_back_at: None })
            })
            .unwrap();
        }
        let r = read_in(dir.path(), &key).unwrap().unwrap();
        assert_eq!(r.last_backup_dir.as_deref(), Some("D:/b"));
        assert_eq!(r.volte_patches.len(), MAX_PATCHES);
        assert_eq!(r.volte_patches[0].at, "3");
        assert!(read_in(dir.path(), "../x").is_err());
        assert!(read_in(dir.path(), &"b".repeat(64)).unwrap().is_none());
    }
}
