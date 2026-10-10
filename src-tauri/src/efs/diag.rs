//! Model-specific DIAG activation. SIM presence/carrier is deliberately not queried.
use adb_client::ADBDeviceExt;
use std::sync::atomic::{AtomicBool, Ordering};

const USB_DIAG: &str = crate::device_io::su!(
    "setprop sys.usb.config diag,diag_mdm,diag_mdm2,qdss,qdss_mdm,serial_cdev,dpl,rmnet,adb"
);
const IV_ENG: &str = crate::device_io::su!("setprop persist.usb.eng 1");

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Acquire) {
        Err("DIAG activation cancelled; USB settings may already have changed".into())
    } else {
        Ok(())
    }
}

/// `needs_eng_port`는 프런트 기종 레지스트리(src/lib/data/devices.ts)의 `diagEngineering`가 정한다 —
/// 백엔드는 모델로 재판정하지 않는다(기기별 분기의 단일 공급원은 레지스트리). Mark IV(1·5 IV)의
/// Hanabi 예외: EFS 포트 열기 전에 persist.usb.eng를 올린다.
pub(super) fn open(dev: &mut dyn ADBDeviceExt, cancel: &AtomicBool, needs_eng_port: bool) -> Result<(), String> {
    check_cancel(cancel)?;
    // 모델은 판정이 아니라 연결 sanity 확인용으로만 읽는다(빈값이면 중단).
    if crate::device_io::shell(dev, "getprop ro.product.model")?.trim().is_empty() {
        return Err("Cannot determine Xperia model for DIAG activation".into());
    }
    check_cancel(cancel)?;
    if needs_eng_port {
        crate::device_io::shell_write(dev, IV_ENG)?;
        check_cancel(cancel)?;
        if crate::device_io::shell(dev, "getprop persist.usb.eng")?.trim() != "1" {
            return Err("Mark IV DIAG prerequisite persist.usb.eng was not applied".into());
        }
    }
    check_cancel(cancel)?;
    // A missing/truncated shell exit status is an error, even with empty stdout.
    crate::device_io::shell_write(dev, USB_DIAG)?;
    check_cancel(cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    fn phone(model: &str) -> FakeADBDevice {
        let mut phone = FakeADBDevice::new();
        phone.answer_shell("getprop ro.product.model", model);
        phone.answer_shell(IV_ENG, "");
        phone.answer_shell("getprop persist.usb.eng", "1");
        phone.answer_shell(USB_DIAG, "");
        phone
    }

    #[test]
    fn eng_port_flag_applies_prerequisite_before_usb_switch() {
        // needs_eng_port=true면 persist.usb.eng를 올리고 리드백 뒤 USB 전환(모델 재판정 없음)
        let mut dev = phone("XQ-CT72");
        open(&mut dev, &AtomicBool::new(false), true).unwrap();
        assert_eq!(dev.shell_calls.len(), 4);
        assert!(dev.shell_calls[1].starts_with(IV_ENG));
        assert!(dev.shell_calls[2].starts_with("getprop persist.usb.eng"));
        assert!(dev.shell_calls[3].starts_with(USB_DIAG));
        // false면 eng 단계 없이 바로 USB 전환
        let mut dev = phone("XQ-DQ44");
        open(&mut dev, &AtomicBool::new(false), false).unwrap();
        assert_eq!(dev.shell_calls.len(), 2);
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with(IV_ENG)));
    }

    #[test]
    fn failed_prerequisite_or_readback_prevents_usb_switch() {
        let mut dev = phone("XQ-CT72");
        dev.shell_exit_codes.insert(IV_ENG.into(), 1);
        assert!(open(&mut dev, &AtomicBool::new(false), true).is_err());
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with(USB_DIAG)));
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("getprop ro.product.model", "XQ-CQ72");
        dev.answer_shell(IV_ENG, "");
        dev.answer_shell("getprop persist.usb.eng", "0");
        assert!(open(&mut dev, &AtomicBool::new(false), true).is_err());
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with(USB_DIAG)));
    }

    #[test]
    fn cancellation_unknown_model_and_failed_usb_switch_are_not_success() {
        let mut dev = phone("XQ-DQ44");
        assert!(open(&mut dev, &AtomicBool::new(true), false).is_err());
        assert!(dev.shell_calls.is_empty());
        let mut dev = phone("");
        assert!(open(&mut dev, &AtomicBool::new(false), false).is_err());
        assert_eq!(dev.shell_calls.len(), 1);
        let mut dev = phone("XQ-DQ44");
        dev.shell_exit_codes.insert(USB_DIAG.into(), 1);
        assert!(open(&mut dev, &AtomicBool::new(false), false).is_err());
        let mut dev = phone("XQ-DQ44");
        dev.shell_truncated.insert(USB_DIAG.into());
        assert!(open(&mut dev, &AtomicBool::new(false), false).is_err());
    }
}
