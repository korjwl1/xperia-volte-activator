//! Tauri 앱 진입점 — 모듈 등록과 명령 목록. 명령 계약은 .plans/02-contracts/tauri-commands.md
mod adb;
mod apk_verify;
mod app_paths;
mod backup;
mod boot_image;
mod device_io;
mod efs;
mod env;
mod events;
mod fastboot;
mod firmware;
mod guard;
mod host;
mod ims;
mod journal;
mod magisk;
mod storage;
mod tasks;
mod usbmode;

#[cfg(all(
    test,
    not(any(
        feature = "fastboot-write",
        feature = "root-write",
        feature = "efs-write"
    ))
))]
mod release_tests {
    #[test]
    fn default_build_rejects_device_mutations_before_transport_lookup() {
        let events = crate::events::Events::callback(|_, _| Ok(()));
        tauri::async_runtime::block_on(async {
            assert!(crate::fastboot::fastboot_flash_with_events(
                events.clone(),
                "init_boot".into(),
                "missing.img".into(),
                true,
                "phone".into(),
                "a".repeat(64)
            )
            .await
            .is_err());
            assert!(crate::fastboot::fastboot_unlock_with_events(
                events,
                "1234567890abcdef".into(),
                true,
                "phone".into()
            )
            .await
            .is_err());
            assert!(
                crate::magisk::root_reboot(Some("phone".into()), "os".into())
                    .await
                    .is_err()
            );
            assert!(crate::efs::efs_preflight("COM999".into()).await.is_err());
        });
    }
}

#[cfg(feature = "dev-cli")]
pub mod dev_cli;

use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // USB 직접 연결용 ADB 인증 키 보관 위치
            // 실패하면 진행 기록·키·펌웨어 캐시가 모두 "앱 데이터 폴더 없음"으로 실패하므로 원인을 남긴다
            match app.path().app_local_data_dir() {
                Ok(dir) => app_paths::init(dir),
                Err(e) => eprintln!("[rust] 앱 데이터 폴더 확인 실패: {e}"),
            }
            // 작업 중 Windows 종료 메시지 가로채기 (보호는 run_guard로 켤 때만 동작)
            guard::init(app.handle());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            env::engine_capabilities,
            adb::adb_status,
            adb::device_list,
            adb::storage_sizes,
            adb::app_flags,
            adb::settings_overview,
            adb::read_imei1,
            adb::open_settings_screen,
            usbmode::usb_modes,
            efs::efs_tool_check,
            efs::efs_validate_presets,
            efs::config::efs_config_get,
            efs::config::efs_config_set,
            efs::config::efs_resolve_preset,
            efs::efs_diag_open,
            efs::efs_preflight,
            efs::efs_upload,
            efs::efs_verify,
            efs::efs_snapshot,
            efs::efs_rollback,
            efs::efs_cancel,
            efs::volte::volte_props_set,
            host::disk_free,
            env::env_check,
            firmware::firmware_fetch,
            firmware::firmware_versions,
            firmware::firmware_dir_check,
            boot_image::boot_image_check,
            adb::root_check,
            journal::journal_save,
            journal::journal_load,
            journal::journal_archive,
            guard::run_guard,
            fastboot::fastboot_getvar,
            fastboot::fastboot_unlock,
            fastboot::fastboot_lock,
            fastboot::flash_history_archive,
            fastboot::fastboot_flash,
            fastboot::fastboot_reboot,
            fastboot::relock_gate_check,
            magisk::magisk_prepare,
            magisk::magisk_patch,
            magisk::magisk_install,
            magisk::root_reboot,
            backup::backup_prepare,
            backup::contacts_restore_check,
            backup::contacts_restore_finish,
            backup::backup_run,
            backup::backup_cancel,
            backup::backup_manifest_check,
            backup::smsie_probe,
            backup::backup_delete,
            backup::smsie_prepare,
            backup::smsie_collect,
            backup::restore_run,
            backup::smsie_restore_stage,
            backup::smsie_restore_finish
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
