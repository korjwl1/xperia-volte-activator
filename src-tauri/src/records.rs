//! 설정 → 기록 관리(2026-10-08 사용자 요청) — 앱 데이터 폴더에 쌓인 기록을 보여 주고 고른 것만 지운다.
//! 백업 폴더 자체(사용자가 고른 위치)는 지우지 않는다 — 백업은 완료 화면의 [백업 파일 삭제]로만 지운다.
//! 부트 기록 이력(flash-history)·이미지 대조 기록은 리락 안전 확인의 근거라 목록에 넣지 않는다.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct RecordItem {
    /// 삭제 요청에 그대로 돌려줄 식별값
    pub id: String,
    /// "patch" | "backup-location" | "journal" | "firmware" | "snapshot"
    pub kind: String,
    pub title: String,
    pub detail: String,
    pub bytes: u64,
    /// RFC 3339 또는 파일 수정 시각
    pub at: String,
}

fn data_dir() -> Result<PathBuf, String> {
    crate::app_paths::data_dir().ok_or_else(|| "앱 데이터 폴더 없음".into())
}

fn dir_bytes(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else { return 0 };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|it| it.flatten().map(|e| dir_bytes(&e.path())).sum())
        .unwrap_or(0)
}

fn modified(path: &Path) -> String {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .map(|t| chrono::DateTime::<chrono::Utc>::from(t).to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
        .unwrap_or_default()
}

fn children(path: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(path)
        .map(|it| it.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

fn list_in(dir: &Path) -> Vec<RecordItem> {
    let mut items = vec![];
    let mut referenced = std::collections::HashSet::new();
    for file in children(&dir.join("devices")) {
        let Some(key) = file.file_stem().and_then(|s| s.to_str()).map(str::to_string) else { continue };
        let Ok(Some(bytes)) = crate::storage::read_bounded(&file, 256 * 1024) else { continue };
        let Ok(record) = serde_json::from_slice::<crate::device_record::DeviceRecord>(&bytes) else { continue };
        let short = &key[..key.len().min(8)];
        if let Some(backup) = &record.last_backup_dir {
            items.push(RecordItem {
                id: format!("backup-location:{key}"),
                kind: "backup-location".into(),
                title: "마지막 백업 위치".into(),
                detail: format!("{backup} · 기기 {short}… (백업 파일은 지우지 않음)"),
                bytes: 0,
                at: modified(&file),
            });
        }
        for patch in &record.volte_patches {
            referenced.insert(PathBuf::from(&patch.snapshot));
            items.push(RecordItem {
                id: format!("patch:{key}:{}:{}", patch.slot, patch.at),
                kind: "patch".into(),
                title: format!("VoLTE 패치 · SIM{} {}{}", patch.slot, patch.carrier, if patch.rolled_back_at.is_some() { " · 되돌림" } else { "" }),
                detail: format!("패치 전 모뎀 설정 사본 · 기기 {short}… · {}", patch.snapshot),
                bytes: dir_bytes(Path::new(&patch.snapshot)),
                at: patch.at.clone(),
            });
        }
    }
    for path in children(&dir.join("efs-snapshots")) {
        if referenced.contains(&path) {
            continue;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        items.push(RecordItem {
            id: format!("snapshot:{name}"),
            kind: "snapshot".into(),
            title: "기록 없는 모뎀 설정 사본".into(),
            detail: name,
            bytes: dir_bytes(&path),
            at: modified(&path),
        });
    }
    for path in children(&dir.join("journal")) {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        if !name.ends_with(".json") {
            continue;
        }
        let state = if name.ends_with(".done.json") { "끝난 작업" } else if name.ends_with(".discarded.json") { "버린 작업" } else { "이어서 할 수 있는 작업" };
        items.push(RecordItem {
            id: format!("journal:{name}"),
            kind: "journal".into(),
            title: format!("진행 기록 · {state}"),
            detail: name,
            bytes: dir_bytes(&path),
            at: modified(&path),
        });
    }
    for path in children(&dir.join("firmware")) {
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        items.push(RecordItem {
            id: format!("firmware:{name}"),
            kind: "firmware".into(),
            title: "순정 부트 이미지 캐시".into(),
            detail: name,
            bytes: dir_bytes(&path),
            at: modified(&path),
        });
    }
    items
}

/// 이름 하나짜리 하위 경로만 허용(상위 폴더로 벗어나지 않게)
fn child(dir: &Path, sub: &str, name: &str) -> Result<PathBuf, String> {
    if name.is_empty() || name.contains(['/', '\\', ':']) || name == "." || name == ".." {
        return Err("잘못된 기록 이름".into());
    }
    Ok(dir.join(sub).join(name))
}

fn remove(path: &Path) -> Result<(), String> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.to_string()),
    };
    if meta.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) }.map_err(|e| format!("{}: {e}", path.display()))
}

fn delete_in(dir: &Path, ids: &[String]) -> Result<usize, String> {
    let snapshots = dir.join("efs-snapshots");
    let mut done = 0;
    for id in ids {
        let (kind, rest) = id.split_once(':').ok_or("잘못된 기록 식별값")?;
        match kind {
            "journal" => remove(&child(dir, "journal", rest)?)?,
            "firmware" => remove(&child(dir, "firmware", rest)?)?,
            "snapshot" => remove(&child(dir, "efs-snapshots", rest)?)?,
            "backup-location" => {
                crate::device_record::update(dir, rest, |r| r.last_backup_dir = None)?;
            }
            "patch" => {
                let mut parts = rest.splitn(3, ':');
                let (Some(key), Some(slot), Some(at)) = (parts.next(), parts.next(), parts.next()) else {
                    return Err("잘못된 패치 기록 식별값".into());
                };
                let mut removed = vec![];
                crate::device_record::update(dir, key, |r| {
                    r.volte_patches.retain(|p| {
                        let hit = p.slot.to_string() == slot && p.at == at;
                        if hit { removed.push(PathBuf::from(&p.snapshot)); }
                        !hit
                    })
                })?;
                // 사본은 앱 데이터 폴더 안에 있는 것만 지운다(사용자가 다른 곳에 둔 원본은 남긴다)
                for path in removed {
                    if path.parent() == Some(snapshots.as_path()) {
                        remove(&path)?;
                    }
                }
            }
            _ => return Err("지원하지 않는 기록 종류".into()),
        }
        done += 1;
    }
    Ok(done)
}

#[tauri::command]
pub async fn records_list() -> Result<Vec<RecordItem>, String> {
    let dir = data_dir()?;
    crate::tasks::blocking("기록 목록", move || Ok(list_in(&dir))).await
}

#[tauri::command]
pub async fn records_delete(ids: Vec<String>) -> Result<usize, String> {
    let dir = data_dir()?;
    crate::tasks::blocking("기록 삭제", move || delete_in(&dir, &ids)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lists_and_deletes_selected_records_without_escaping_the_data_folder() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        let key = "a".repeat(64);
        let snap = d.join("efs-snapshots").join("s1");
        std::fs::create_dir_all(&snap).unwrap();
        std::fs::write(snap.join("snapshot.json"), "{}").unwrap();
        std::fs::create_dir_all(d.join("efs-snapshots").join("orphan")).unwrap();
        std::fs::create_dir_all(d.join("journal")).unwrap();
        std::fs::write(d.join("journal").join("x.done.json"), "{}").unwrap();
        crate::device_record::update(d, &key, |r| {
            r.last_backup_dir = Some("D:/backup".into());
            r.volte_patches.push(crate::device_record::VoltePatch { at: "2026-10-07T00:00:00Z".into(), slot: 1, carrier: "SKT".into(), snapshot: snap.to_string_lossy().into(), rolled_back_at: None });
        }).unwrap();
        let items = list_in(d);
        let kinds: Vec<_> = items.iter().map(|i| i.kind.as_str()).collect();
        assert!(kinds.contains(&"patch") && kinds.contains(&"backup-location") && kinds.contains(&"journal") && kinds.contains(&"snapshot"));
        assert_eq!(items.iter().filter(|i| i.kind == "snapshot").count(), 1, "기록에 있는 사본은 따로 나오지 않는다");
        let patch = items.iter().find(|i| i.kind == "patch").unwrap().id.clone();
        assert_eq!(delete_in(d, &[patch, "journal:x.done.json".into(), format!("backup-location:{key}")]).unwrap(), 3);
        assert!(!snap.exists());
        assert!(!d.join("journal").join("x.done.json").exists());
        let rec = crate::device_record::read(d, &key).unwrap().unwrap();
        assert!(rec.volte_patches.is_empty() && rec.last_backup_dir.is_none());
        assert!(delete_in(d, &["journal:../x".into()]).is_err());
        assert!(delete_in(d, &["firmware:..".into()]).is_err());
    }
}
