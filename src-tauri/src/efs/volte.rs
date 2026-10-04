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
        crate::adb::with_first_device(&Some(serial), |dev| {
            let mut applied = Vec::new();
            for command in PROPERTIES {
                if owner.cancel.load(Ordering::Acquire) {
                    return Err("VoLTE settings cancelled; some properties may be changed".into());
                }
                crate::device_io::shell(dev, command)?;
                applied.push(command.to_string());
            }
            if owner.cancel.load(Ordering::Acquire) {
                return Err("VoLTE settings cancelled before reboot".into());
            }
            dev.reboot(adb_client::RebootType::System)
                .map_err(|e| e.to_string())?;
            Ok(applied)
        })
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
