// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
mod adb;
mod backup;
mod firmware;
mod host;
mod guard;
mod journal;
mod usbmode;

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
            // 작업 중 Windows 종료 메시지 가로채기 (보호는 run_guard로 켤 때만 동작)
            guard::init(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            adb::adb_status,
            adb::device_list,
            adb::storage_sizes,
            adb::app_flags,
            adb::settings_overview,
            adb::read_imei1,
            adb::open_settings_screen,
            usbmode::usb_modes,
            host::disk_free,
            firmware::firmware_fetch,
            firmware::firmware_versions,
            firmware::firmware_dir_check,
            adb::root_check,
            journal::journal_save,
            journal::journal_load,
            journal::journal_archive,
            guard::run_guard,
            backup::backup_prepare,
            backup::contacts_restore_check,
            backup::backup_run,
            backup::backup_cancel,
            backup::backup_manifest_check,
            backup::smsie_prepare,
            backup::smsie_collect,
            backup::restore_run,
            backup::smsie_restore_stage,
            backup::smsie_restore_finish
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
