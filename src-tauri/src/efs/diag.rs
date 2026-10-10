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

pub(super) fn open(dev: &mut dyn ADBDeviceExt, cancel: &AtomicBool) -> Result<(), String> {
    check_cancel(cancel)?;
    let model = crate::device_io::shell(dev, "getprop ro.product.model")?;
    let model = model.trim();
    if model.is_empty() {
        return Err("Cannot determine Xperia model for DIAG activation".into());
    }
    check_cancel(cancel)?;
    // Hanabi's Mark IV exception: persist.usb.eng before opening the EFS port.
    // 1 IV = XQ-CT*, 5 IV = XQ-CQ*. 10 IV is a separate series (XQ-CC*).
    // ⚠ 이 접두사 목록은 프런트 src/lib/data/devices.ts의 `diagEngineering` 플래그와 같은 집합이어야 한다 — 기기 추가 시 두 곳을 함께 수정한다.
    if model.starts_with("XQ-CT") || model.starts_with("XQ-CQ") {
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
    fn model_exception_is_applied_and_read_back_before_usb_switch() {
        for model in ["XQ-CT72", "XQ-CQ44"] {
            let mut dev = phone(model);
            open(&mut dev, &AtomicBool::new(false)).unwrap();
            assert_eq!(dev.shell_calls.len(), 4);
            assert!(dev.shell_calls[1].starts_with(IV_ENG));
            assert!(dev.shell_calls[2].starts_with("getprop persist.usb.eng"));
            assert!(dev.shell_calls[3].starts_with(USB_DIAG));
        }
        for model in ["XQ-DQ44", "XQ-CC72", "XQ-BE72"] {
            let mut dev = phone(model);
            open(&mut dev, &AtomicBool::new(false)).unwrap();
            assert_eq!(dev.shell_calls.len(), 2);
            assert!(!dev.shell_calls.iter().any(|c| c.starts_with(IV_ENG)));
        }
    }

    #[test]
    fn failed_prerequisite_or_readback_prevents_usb_switch() {
        let mut dev = phone("XQ-CT72");
        dev.shell_exit_codes.insert(IV_ENG.into(), 1);
        assert!(open(&mut dev, &AtomicBool::new(false)).is_err());
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with(USB_DIAG)));
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("getprop ro.product.model", "XQ-CQ72");
        dev.answer_shell(IV_ENG, "");
        dev.answer_shell("getprop persist.usb.eng", "0");
        assert!(open(&mut dev, &AtomicBool::new(false)).is_err());
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with(USB_DIAG)));
    }

    #[test]
    fn cancellation_unknown_model_and_failed_usb_switch_are_not_success() {
        let mut dev = phone("XQ-DQ44");
        assert!(open(&mut dev, &AtomicBool::new(true)).is_err());
        assert!(dev.shell_calls.is_empty());
        let mut dev = phone("");
        assert!(open(&mut dev, &AtomicBool::new(false)).is_err());
        assert_eq!(dev.shell_calls.len(), 1);
        let mut dev = phone("XQ-DQ44");
        dev.shell_exit_codes.insert(USB_DIAG.into(), 1);
        assert!(open(&mut dev, &AtomicBool::new(false)).is_err());
        let mut dev = phone("XQ-DQ44");
        dev.shell_truncated.insert(USB_DIAG.into());
        assert!(open(&mut dev, &AtomicBool::new(false)).is_err());
    }
}
