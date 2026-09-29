// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod adb;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            adb::adb_status,
            adb::device_list,
            adb::storage_sizes
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
