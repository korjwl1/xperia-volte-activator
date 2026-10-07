// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 관리자 권한으로 다시 띄워진 fastboot 드라이버 설치 요청이면 그것만 처리하고 끝낸다
    let args: Vec<_> = std::env::args_os().collect();
    if let Some(code) = xperia_volte_activator_lib::usb_driver::elevated_entry(&args) {
        std::process::exit(code);
    }
    xperia_volte_activator_lib::run()
}
