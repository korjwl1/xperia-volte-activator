//! 읽기 전용 기기 경로 확인(일회성 진단) — /sdcard 실체·스토리지 목록 조회.
//! 실행: cargo run --example probe_sdcard
//! 정책: 읽기 전용 셸 질의만 (AGENTS 허용 범위 — 기기·PC 변경 없음)

use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use adb_client::usb::find_all_connected_adb_devices;
use adb_client::{ADBDeviceExt, RustADBError};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::time::Duration;

fn shell(dev: &mut dyn ADBDeviceExt, cmd: &str) -> String {
    let mut out = Vec::new();
    match dev.shell_command(&cmd, Some(&mut out), None) {
        Ok(_) => String::from_utf8_lossy(&out).trim().to_string(),
        Err(e) => format!("[오류] {e}"),
    }
}

fn main() {
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5037);
    let server_up =
        TcpStream::connect_timeout(&SocketAddr::V4(addr), Duration::from_millis(300)).is_ok();

    let mut handled = false;
    if server_up {
        // 기존 adb 서버 재사용 — 바이너리 실행 차단용 가짜 경로
        let mut server = ADBServer::new_from_path(addr, Some(r"\\?\xvolte\no-adb-binary".into()));
        if let Ok(devices) = server.devices() {
            let ready: Vec<_> = devices
                .into_iter()
                .filter(|d| d.state.to_string().eq_ignore_ascii_case("device"))
                .collect();
            if let Some(first) = ready.first() {
                let mut dev = ADBServerDevice::new(first.identifier.clone(), Some(addr));
                run(&mut dev, &format!("(adb 서버 모드: {})", first.identifier));
                handled = true;
            }
        }
    }
    if handled {
        return;
    }
    // USB 직접 연결 폴백 — Sony VID만
    if server_up {
        // 서버가 USB 인터페이스를 쥐고 있어 직접 연결은 실패한다
        println!(
            "adb 서버에 준비된 기기가 없습니다 — 폰에서 USB 디버깅을 허용했는지 확인해 주세요"
        );
        return;
    }
    match find_all_connected_adb_devices() {
        Ok(list) if !list.is_empty() => {
            let Some(info) = list.into_iter().find(|d| d.vendor_id == 0x0FCE) else {
                println!("연결된 Sony 기기가 없습니다");
                return;
            };
            let Some(key) = std::env::var_os("USERPROFILE")
                .map(|h| std::path::PathBuf::from(h).join(".android").join("adbkey"))
            else {
                println!("USERPROFILE을 확인할 수 없습니다");
                return;
            };
            match adb_client::usb::ADBUSBDevice::new_with_custom_private_key(
                info.vendor_id,
                info.product_id,
                key,
            ) {
                Ok(mut dev) => run(&mut dev, "(USB 직접 연결)"),
                Err(RustADBError::ADBRequestFailed(e)) if e.contains("device unauthorized") => {
                    println!("기기가 승인 대기 중입니다 — 폰 화면에서 USB 디버깅을 허용해 주세요");
                }
                Err(e) => println!("USB 연결 실패: {e}"),
            }
        }
        Ok(_) => println!("연결된 기기가 없습니다 — 폰을 USB로 연결해 주세요"),
        Err(e) => println!("USB 검색 실패: {e}"),
    }
}

fn run(dev: &mut dyn ADBDeviceExt, via: &str) {
    println!("== 연결: {via} ==");
    for (label, cmd) in [
        ("모델", "getprop ro.product.model"),
        ("Android", "getprop ro.build.version.release"),
        ("/sdcard 실체", "readlink -f /sdcard"),
        ("심볼릭 링크", "ls -la /sdcard"),
        ("/storage 목록", "ls /storage/"),
        ("내장 용량", "df -h /storage/emulated/0 | tail -1"),
        (
            "외장 SD 여부",
            "ls /storage/ | grep -E '^[0-9A-F]{4}-[0-9A-F]{4}$' || echo '(외장 SD 없음)'",
        ),
        ("sdcard 루트", "ls /sdcard/ | head -12"),
        ("Android/data 접근", "ls /sdcard/Android/data | head -4"),
    ] {
        println!("[{label}]\n{}", shell(dev, cmd));
    }
}
