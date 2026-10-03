//! 작업 진행 기록 (PC 앱 데이터 폴더, 사용자 승인 2026-10-03)
//! 실행 중 상태를 기기별 JSON으로 남겨, 연결이 끊기거나 앱이 꺼진 뒤 같은 폰을 다시 연결하면 이어서 진행할 수 있게 한다.
//! - 위치: <앱 데이터>/journal/<key>.json (key = 기기 식별 해시, 프론트에서 생성)
//! - 끝난 작업은 <key>.done.json, 새로 시작해 버린 작업은 <key>.discarded.json으로 보관 (디버깅용, 마지막 1개)
//! - 내용은 프론트가 만든 JSON 그대로 — 언락 코드·IMEI는 넣지 않는다

use crate::app_paths::data_dir as app_data_dir;
use std::path::PathBuf;

const MAX_JOURNAL_BYTES: usize = 8 * 1024 * 1024;

fn valid_key(key: &str) -> bool {
    (16..=64).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_hexdigit())
}

fn journal_dir() -> Result<PathBuf, String> {
    Ok(app_data_dir()
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?
        .join("journal"))
}

fn path_for(key: &str, suffix: &str) -> Result<PathBuf, String> {
    if !valid_key(key) {
        return Err("잘못된 기록 키입니다".into());
    }
    Ok(journal_dir()?.join(format!("{key}{suffix}.json")))
}

fn save_work(key: &str, data: &str) -> Result<(), String> {
    if data.len() > MAX_JOURNAL_BYTES {
        return Err("진행 기록이 너무 큽니다".into());
    }
    let path = path_for(key, "")?;
    std::fs::create_dir_all(path.parent().expect("journal 폴더"))
        .map_err(|e| format!("기록 폴더 생성 실패: {e}"))?;
    // 쓰는 도중 꺼져도 이전 기록이 깨지지 않도록 임시 파일에 쓴 뒤 교체
    serde_json::from_str::<serde_json::Value>(data)
        .map_err(|e| format!("진행 기록 JSON 오류: {e}"))?;
    crate::storage::atomic_write(&path, data.as_bytes())
}

fn load_work(key: &str) -> Result<Option<String>, String> {
    let path = path_for(key, "")?;
    load_file(&path)
}

fn load_file(path: &std::path::Path) -> Result<Option<String>, String> {
    crate::storage::read_bounded(path, MAX_JOURNAL_BYTES)?
        .map(|raw| String::from_utf8(raw).map_err(|e| format!("진행 기록 UTF-8 오류: {e}")))
        .transpose()
}

fn archive_work(key: &str, tag: &str) -> Result<(), String> {
    if tag != "done" && tag != "discarded" {
        return Err("잘못된 보관 구분입니다".into());
    }
    let from = path_for(key, "")?;
    if !from.exists() {
        return Ok(());
    }
    let to = path_for(key, &format!(".{tag}"))?;
    let _ = std::fs::remove_file(&to);
    std::fs::rename(&from, &to).map_err(|e| format!("진행 기록 보관 실패: {e}"))
}

/// 진행 기록 저장 (덮어쓰기)
#[tauri::command]
pub async fn journal_save(key: String, data: String) -> Result<(), String> {
    crate::tasks::blocking("진행 기록 저장", move || save_work(&key, &data)).await
}

/// 끝나지 않은 진행 기록 (없으면 null)
#[tauri::command]
pub async fn journal_load(key: String) -> Result<Option<String>, String> {
    crate::tasks::blocking("진행 기록 읽기", move || load_work(&key)).await
}

/// 진행 기록 보관 — 끝난 작업(done) / 새로 시작해 버린 작업(discarded)
#[tauri::command]
pub async fn journal_archive(key: String, tag: String) -> Result<(), String> {
    crate::tasks::blocking("진행 기록 보관", move || archive_work(&key, &tag)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn load_rejects_oversized_and_non_utf8_disk_records() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("journal.json");
        assert!(load_file(&file).unwrap().is_none());
        std::fs::write(&file, b"{}").unwrap();
        assert_eq!(load_file(&file).unwrap().as_deref(), Some("{}"));
        std::fs::write(&file, [255]).unwrap();
        assert!(load_file(&file).is_err());
        std::fs::write(&file, vec![b' '; MAX_JOURNAL_BYTES + 1]).unwrap();
        assert!(load_file(&file).is_err());
    }

    #[test]
    fn key_validation() {
        assert!(valid_key("0123456789abcdef0123456789abcdef"));
        assert!(!valid_key("short"));
        assert!(!valid_key("../../etc/passwd/aaaaaaaaaaaaaaa"));
        assert!(!valid_key("0123456789abcdeg0123456789abcdef"));
    }
}
