//! Retains the original VoLTE property/reboot stage without an EfsTools wrapper.
use super::{
    error::{Error, Result},
    gate, Operation,
};
use std::sync::atomic::Ordering;

const PROPERTIES: &[&str] = &[
    "su -c setprop persist.dbg.ims_avail_ovr 1",
    "su -c setprop persist.dbg.volte_avail_ovr 1",
    "su -c setprop persist.dbg.vt_avail_ovr 1",
    "su -c setprop persist.dbg.wfc_avail_ovr 1",
];
/// Shared by the command and fake-device tests; failure/cancellation stops before reboot.
fn apply_properties(
    dev: &mut dyn adb_client::ADBDeviceExt,
    cancel: &std::sync::atomic::AtomicBool,
) -> std::result::Result<Vec<String>, String> {
    let mut applied = Vec::new();
    for command in PROPERTIES {
        if cancel.load(Ordering::Acquire) {
            return Err("VoLTE settings cancelled; some properties may be changed".into());
        }
        crate::device_io::shell_write(dev, command)?;
        applied.push(command.to_string());
    }
    if cancel.load(Ordering::Acquire) {
        return Err("VoLTE settings cancelled before reboot".into());
    }
    dev.reboot(adb_client::RebootType::System)
        .map_err(|e| e.to_string())?;
    Ok(applied)
}

#[tauri::command]
pub async fn volte_props_set(serial: Option<String>) -> Result<Vec<String>> {
    gate()?;
    let owner = Operation::acquire()?;
    let serial = serial.filter(|s| !s.is_empty()).ok_or_else(|| {
        Error::new(
            "invalidDevice",
            "VoLTE settings",
            "Explicit ADB serial required",
        )
    })?;
    tauri::async_runtime::spawn_blocking(move || {
        crate::adb::with_first_device(&Some(serial), |dev| apply_properties(dev, &owner.cancel))
            .map_err(|message| {
                Error::new(
                    if owner.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "io"
                    },
                    "VoLTE settings",
                    message,
                )
            })
    })
    .await
    .map_err(|e| Error::io("VoLTE settings worker", e))?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use std::sync::atomic::AtomicBool;
    #[test]
    fn properties_use_checked_status_and_reboot_only_after_all_four_succeed() {
        let mut phone = FakeADBDevice::new();
        for command in PROPERTIES {
            phone.answer_shell(command, "");
        }
        let cancel = AtomicBool::new(false);
        assert_eq!(apply_properties(&mut phone, &cancel).unwrap().len(), 4);
        assert_eq!(phone.reboot_calls, 1);
        phone.reboot_calls = 0;
        phone.shell_calls.clear();
        phone.shell_exit_codes.insert(PROPERTIES[1].to_string(), 1);
        assert!(apply_properties(&mut phone, &cancel).is_err());
        assert_eq!(phone.shell_calls.len(), 2);
        assert_eq!(phone.reboot_calls, 0);
        phone.shell_calls.clear();
        cancel.store(true, Ordering::Release);
        assert!(apply_properties(&mut phone, &cancel).is_err());
        assert!(phone.shell_calls.is_empty());
        assert_eq!(phone.reboot_calls, 0);
    }
}
