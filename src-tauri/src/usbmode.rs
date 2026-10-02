//! USB 연결 모드 감지 (읽기 전용) — 장치를 열지 않고 USB 디스크립터만 읽는다.
//! 부트로더(fastboot)·플래시 모드 진입을 자동으로 감지해 "파란/초록 LED 확인 후 Enter" 같은 수동 확인을 없앤다.
//! - fastboot: 인터페이스 class 0xFF / subclass 0x42 / protocol 0x03 (AOSP fastboot 표준)
//! - adb:      0xFF / 0x42 / 0x01
//! - flashmode(Sony S1): 제품 ID 0xADDE — ⚠ 실기기 미검증 (커뮤니티 기록 기준)

use crate::adb::guarded;
use serde::Serialize;
use std::time::Duration;

const SONY_VID: u16 = 0x0FCE;
const SONY_FLASHMODE_PID: u16 = 0xADDE;

#[derive(Serialize, Debug, PartialEq, Clone)]
#[serde(rename_all = "camelCase")]
pub struct UsbModeOut {
    /// "android"(adb) | "fastboot" | "flashmode" | "other"
    mode: String,
    vendor_id: u16,
    product_id: u16,
}

/// 인터페이스 (class, subclass, protocol) 목록과 PID로 모드 판정
fn classify(product_id: u16, ifaces: &[(u8, u8, u8)]) -> &'static str {
    if ifaces.contains(&(0xFF, 0x42, 0x03)) {
        "fastboot"
    } else if ifaces.contains(&(0xFF, 0x42, 0x01)) {
        "android"
    } else if product_id == SONY_FLASHMODE_PID {
        "flashmode"
    } else {
        "other"
    }
}

fn scan() -> Result<Vec<UsbModeOut>, String> {
    let devices = rusb::devices().map_err(|e| format!("USB 장치 목록 조회 실패: {e}"))?;
    let mut out = vec![];
    for dev in devices.iter() {
        let Ok(desc) = dev.device_descriptor() else { continue };
        if desc.vendor_id() != SONY_VID {
            continue;
        }
        let mut ifaces = vec![];
        for i in 0..desc.num_configurations() {
            if let Ok(cfg) = dev.config_descriptor(i) {
                for iface in cfg.interfaces() {
                    for d in iface.descriptors() {
                        ifaces.push((d.class_code(), d.sub_class_code(), d.protocol_code()));
                    }
                }
            }
        }
        out.push(UsbModeOut {
            mode: classify(desc.product_id(), &ifaces).into(),
            vendor_id: desc.vendor_id(),
            product_id: desc.product_id(),
        });
    }
    Ok(out)
}

/// PC에 Sony USB 장치가 하나라도 연결돼 있는지 (판별 실패 시 true — 안내를 숨기지 않도록)
pub(crate) fn sony_usb_present() -> bool {
    scan().map(|v| !v.is_empty()).unwrap_or(true)
}

/// 연결된 Sony 기기의 USB 모드 목록 (읽기 전용)
#[tauri::command]
pub async fn usb_modes() -> Result<Vec<UsbModeOut>, String> {
    guarded(Duration::from_secs(10), scan).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_modes() {
        assert_eq!(classify(0x0DDE, &[(0xFF, 0x42, 0x03)]), "fastboot");
        assert_eq!(classify(0x0AEB, &[(0x06, 0x01, 0x01), (0xFF, 0x42, 0x01)]), "android");
        assert_eq!(classify(0xADDE, &[(0xFF, 0xFF, 0xFF)]), "flashmode");
        assert_eq!(classify(0x1234, &[(0x08, 0x06, 0x50)]), "other");
    }

    /// 실기기 연결 상태 확인용 (cargo test -- --ignored live_usb)
    #[test]
    #[ignore]
    fn live_usb_scan() {
        eprintln!("{:?}", scan().unwrap());
    }
}
