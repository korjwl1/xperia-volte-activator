use super::{
    error::{Error, Result},
    manifest::Plan,
};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tauri::Manager;
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Configuration {
    pub port: String,
    pub preset_root: String,
    pub snapshot_root: String,
}
pub fn validate_port(name: &str) -> Result<()> {
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
    if !path.exists() {
        return Ok(None);
    }
    let bytes = fs::read(path).map_err(|e| Error::io("configuration", e))?;
    let cfg: Configuration =
        serde_json::from_slice(&bytes).map_err(|e| Error::io("configuration", e))?;
    validate_port(&cfg.port)?;
    Ok(Some(cfg))
}
#[tauri::command]
pub async fn efs_config_set(app: tauri::AppHandle, configuration: Configuration) -> Result<()> {
    let _owner = super::Operation::acquire()?;
    validate_port(&configuration.port)?;
    if !Path::new(&configuration.preset_root).is_absolute()
        || !Path::new(&configuration.snapshot_root).is_absolute()
    {
        return Err(Error::new(
            "invalidPath",
            "configuration",
            "Preset and snapshot roots must be absolute",
        ));
    }
    let path = file(&app)?;
    fs::create_dir_all(path.parent().unwrap()).map_err(|e| Error::io("configuration", e))?;
    fs::write(
        path,
        serde_json::to_vec_pretty(&configuration).map_err(|e| Error::io("configuration", e))?,
    )
    .map_err(|e| Error::io("configuration", e))
}
#[tauri::command]
pub async fn efs_resolve_preset(app: tauri::AppHandle, folder: String) -> Result<String> {
    let cfg = efs_config_get(app).await?.ok_or_else(|| {
        Error::new(
            "notConfigured",
            "configuration",
            "Set COM, bundle root and snapshot root through api.efsConfigure first",
        )
    })?;
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
    let root = PathBuf::from(cfg.preset_root)
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
