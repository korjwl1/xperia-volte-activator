//! Manual root tools. No device calls are made at program startup.
pub mod modules;
pub mod packages;
pub mod switch;
use std::path::PathBuf;

pub fn write_gate(serial: &str, confirm: bool) -> Result<(), String> {
    if !cfg!(feature = "root-tools-write") {
        return Err("ROOT_WRITE_DISABLED|root-tools-write 기능이 꺼져 있습니다".into());
    }
    if !confirm || serial.trim().is_empty() {
        return Err("작업 대상과 명시적 확인이 필요합니다".into());
    }
    Ok(())
}
#[tauri::command]
pub fn root_tools_capabilities() -> serde_json::Value {
    serde_json::json!({"writeEnabled":cfg!(feature="root-tools-write"),"switchEnabled":cfg!(all(feature="root-tools-write",feature="fastboot-write"))})
}
pub fn data_dir() -> Result<PathBuf, String> {
    crate::app_paths::data_dir().ok_or_else(|| "앱 데이터 폴더 없음".into())
}
pub fn key_path(dir: &std::path::Path, key: &str, group: &str) -> Result<PathBuf, String> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("기기 키 형식 오류".into());
    }
    Ok(dir
        .join(group)
        .join(format!("{}.json", key.to_ascii_lowercase())))
}
pub fn save<T: serde::Serialize>(path: &std::path::Path, record: &T) -> Result<(), String> {
    crate::storage::atomic_write(
        path,
        &serde_json::to_vec(record).map_err(|e| e.to_string())?,
    )
}

#[cfg(all(test, not(feature = "root-tools-write")))]
mod gate_tests {
    #[test]
    fn default_build_rejects_every_new_mutation_before_paths_or_device_lookup() {
        tauri::async_runtime::block_on(async {
            let serial = "offline-phone".to_string();
            let hash = "a".repeat(64);
            let results = [
                super::modules::root_module_install(serial.clone(), hash.clone(), true, true)
                    .await
                    .map(|_| ()),
                super::modules::root_module_action(
                    serial.clone(),
                    "test".into(),
                    "disable".into(),
                    true,
                )
                .await,
                super::switch::root_switch_prepare(
                    serial.clone(),
                    "missing.img".into(),
                    "resukisu".into(),
                    true,
                )
                .await
                .map(|_| ()),
                super::switch::root_external_patch_import(
                    serial.clone(),
                    "missing.img".into(),
                    "missing-patch.img".into(),
                    true,
                )
                .await
                .map(|_| ()),
                super::switch::resukisu_install(serial, hash, true).await,
            ];
            for result in results {
                assert!(result.unwrap_err().starts_with("ROOT_WRITE_DISABLED|"));
            }
        });
    }
}
