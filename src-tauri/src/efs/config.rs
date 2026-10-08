use super::{
    error::{Error, Result},
    manifest::Plan,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use tauri::Manager;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub port: String,
    pub preset_root: String,
    pub snapshot_root: String,
}
const MAX_CONFIGURATION: usize = 16 * 1024;
fn validate(cfg: &Configuration) -> Result<()> {
    validate_port(&cfg.port)?;
    if !Path::new(&cfg.preset_root).is_absolute() || !Path::new(&cfg.snapshot_root).is_absolute() {
        return Err(Error::new(
            "invalidPath",
            "configuration",
            "Preset and snapshot roots must be absolute",
        ));
    }
    Ok(())
}
pub fn validate_port(name: &str) -> Result<()> {
    if name == super::AUTO_PORT {
        return Ok(());
    }
    let digits = name.strip_prefix("COM").ok_or_else(|| {
        Error::new(
            "invalidPort",
            "configuration",
            "Explicit COM<number> required",
        )
    })?;
    if digits.is_empty()
        || digits.len() > 5
        || !digits.bytes().all(|b| b.is_ascii_digit())
        || digits.parse::<u32>().unwrap_or(0) == 0
    {
        return Err(Error::new(
            "invalidPort",
            "configuration",
            "Invalid COM port",
        ));
    }
    Ok(())
}
fn file(app: &tauri::AppHandle) -> Result<PathBuf> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|e| Error::io("configuration", e))?
        .join("efs-native.json"))
}
#[tauri::command]
pub async fn efs_config_get(app: tauri::AppHandle) -> Result<Option<Configuration>> {
    let path = file(&app)?;
    // 저장한 설정이 없으면(일반 사용) 앱이 정한다 — 포트 자동, 동봉 프리셋, 앱 데이터 폴더의 복원본(2026-10-08 사용자 결정)
    let defaults = defaults(&app)?;
    super::offline(move || Ok(Some(read_configuration(&path)?.unwrap_or(defaults)))).await
}
fn defaults(app: &tauri::AppHandle) -> Result<Configuration> {
    let presets = app
        .path()
        .resource_dir()
        .map_err(|e| Error::io("configuration", e))?
        .join("assets")
        .join("efs");
    let snapshots = app
        .path()
        .app_local_data_dir()
        .map_err(|e| Error::io("configuration", e))?
        .join("efs-snapshots");
    std::fs::create_dir_all(&snapshots).map_err(|e| Error::io("configuration", e))?;
    Ok(Configuration {
        port: super::AUTO_PORT.into(),
        preset_root: presets.to_string_lossy().into(),
        snapshot_root: snapshots.to_string_lossy().into(),
    })
}
fn read_configuration(path: &Path) -> Result<Option<Configuration>> {
    let Some(bytes) = crate::storage::read_bounded(path, MAX_CONFIGURATION)
        .map_err(|e| Error::io("configuration", e))?
    else {
        return Ok(None);
    };
    let cfg: Configuration =
        serde_json::from_slice(&bytes).map_err(|e| Error::io("configuration", e))?;
    validate(&cfg)?;
    Ok(Some(cfg))
}
fn save_configuration(path: &Path, configuration: &Configuration) -> Result<()> {
    validate(configuration)?;
    let bytes =
        serde_json::to_vec_pretty(configuration).map_err(|e| Error::io("configuration", e))?;
    if bytes.len() > MAX_CONFIGURATION {
        return Err(Error::new(
            "limit",
            "configuration",
            "Configuration exceeds read bound",
        ));
    }
    crate::storage::atomic_write(path, &bytes).map_err(|e| Error::io("configuration", e))
}
#[tauri::command]
pub async fn efs_config_set(app: tauri::AppHandle, configuration: Configuration) -> Result<()> {
    let owner = super::Operation::acquire()?;
    let path = file(&app)?;
    super::offline(move || {
        let _owner = owner;
        save_configuration(&path, &configuration)
    })
    .await
}
#[tauri::command]
pub async fn efs_resolve_preset(
    app: tauri::AppHandle,
    folder: String,
    configuration: Option<Configuration>,
) -> Result<String> {
    let cfg = match configuration {
        Some(cfg) => {
            validate(&cfg)?;
            Some(cfg)
        }
        None => efs_config_get(app).await?,
    }
    .ok_or_else(|| {
        Error::new(
            "notConfigured",
            "configuration",
            "Set COM, bundle root and snapshot root through api.efsConfigure first",
        )
    })?;
    super::offline(move || resolve_preset(&cfg, &folder)).await
}
fn resolve_preset(cfg: &Configuration, folder: &str) -> Result<String> {
    let rel = folder.strip_prefix("./util/SonyEFS/").ok_or_else(|| {
        Error::new(
            "invalidPath",
            "preset",
            "Only pinned balance preset folders are allowed",
        )
    })?;
    if rel
        .split('/')
        .any(|p| p.is_empty() || p == "." || p == ".." || p.contains('\\') || p.contains(':'))
    {
        return Err(Error::new("invalidPath", "preset", "Unsafe preset folder"));
    }
    let root = PathBuf::from(&cfg.preset_root)
        .canonicalize()
        .map_err(|e| Error::io("preset", e))?;
    let path = root
        .join("util/SonyEFS")
        .join(rel)
        .canonicalize()
        .map_err(|e| Error::io("preset", e))?;
    if !path.starts_with(root) {
        return Err(Error::new(
            "invalidPath",
            "preset",
            "Preset escaped bundle root",
        ));
    }
    Plan::load_approved(&path)?;
    Ok(path.to_string_lossy().into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_is_bounded_validated_and_replaced_atomically() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("efs-native.json");
        assert!(read_configuration(&path).unwrap().is_none());
        let mut cfg = Configuration {
            port: "COM9".into(),
            preset_root: dir.path().to_string_lossy().into(),
            snapshot_root: dir.path().to_string_lossy().into(),
        };
        save_configuration(&path, &cfg).unwrap();
        cfg.port = "COM10".into();
        save_configuration(&path, &cfg).unwrap();
        assert_eq!(read_configuration(&path).unwrap().unwrap().port, "COM10");
        cfg.preset_root = "relative".into();
        assert!(save_configuration(&path, &cfg).is_err());
        assert_eq!(read_configuration(&path).unwrap().unwrap().port, "COM10");
        std::fs::write(&path, vec![b' '; MAX_CONFIGURATION + 1]).unwrap();
        assert!(read_configuration(&path).is_err());
    }
}
