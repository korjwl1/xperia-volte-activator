// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod adb;
mod host;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // USB 직접 연결용 ADB 인증 키 보관 위치
            if let Ok(dir) = app.path().app_local_data_dir() {
                adb::set_key_dir(dir);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            adb::adb_status,
            adb::device_list,
            adb::storage_sizes,
            adb::app_flags,
            adb::settings_overview,
            host::disk_free
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
