//! Root observations, not proof that an arbitrary image may be written.
use adb_client::ADBDeviceExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Access {
    Granted,
    Unavailable,
    Denied,
    Unknown,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Engine {
    Magisk,
    KernelsuFamily,
    Conflicting,
    Unknown,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RootState {
    pub access: Access,
    pub engine: Engine,
    pub magisk_markers: Option<bool>,
    pub kernelsu_markers: Option<bool>,
}

pub(crate) const VERSION: &str = concat!(crate::device_io::su_path!(), " -v 2>/dev/null");
// Fixed paths only. Never run a discovered module, daemon or manager APK.
pub(crate) const MARKERS: &str = crate::device_io::su!("'id || exit; test -d /data/adb || exit 2; test -r /data/adb && test -x /data/adb || exit 2; if test -e /data/adb/magisk || test -e /data/adb/magisk.db; then echo MAGISK=1; else echo MAGISK=0; fi; if test -e /data/adb/ksud || test -e /data/adb/ksu; then echo KSU=1; else echo KSU=0; fi'");

pub(crate) fn root_uid(text: &str) -> bool {
    text.split_ascii_whitespace()
        .any(|token| token == "uid=0" || token.starts_with("uid=0("))
}
fn bounded_text(bytes: &[u8]) -> Result<&str, String> {
    if bytes.len() > 4096 {
        return Err("ROOT_INSPECT_OUTPUT|Root observation is too large".into());
    }
    std::str::from_utf8(bytes).map_err(|_| "ROOT_INSPECT_OUTPUT|Invalid root observation".into())
}
fn marker(text: &str, key: &str) -> Result<bool, String> {
    let values: Vec<_> = text
        .lines()
        .filter_map(|line| line.strip_prefix(key))
        .collect();
    match values.as_slice() {
        ["0"] => Ok(false),
        ["1"] => Ok(true),
        _ => Err("ROOT_INSPECT_OUTPUT|Incomplete or conflicting markers".into()),
    }
}
fn version_engine(version: &str) -> Engine {
    let v = version.trim();
    if v.lines().count() != 1 || v.len() > 128 {
        return Engine::Unknown;
    }
    match v
        .rsplit_once(':')
        .map(|(_, brand)| brand.to_ascii_lowercase())
        .as_deref()
    {
        Some("magisksu") => Engine::Magisk,
        Some("kernelsu" | "resukisu" | "sukisu" | "bakasu") => Engine::KernelsuFamily,
        _ => Engine::Unknown,
    }
}

pub(crate) fn inspect(dev: &mut dyn ADBDeviceExt) -> Result<RootState, String> {
    let id = crate::device_io::shell_run(dev, crate::device_io::su!("id 2>&1"))?;
    let id_text = bounded_text(&id.stdout)?;
    let access = match id.code {
        0 if root_uid(id_text) => Access::Granted,
        127 => Access::Unavailable,
        1 => Access::Denied,
        _ => Access::Unknown,
    };
    let mut state = RootState {
        access,
        engine: Engine::Unknown,
        magisk_markers: None,
        kernelsu_markers: None,
    };
    // A refusal or missing su does not mean /data/adb is empty or the phone is stock.
    if access != Access::Granted {
        return Ok(state);
    }
    let version = crate::device_io::shell_run(dev, VERSION)?;
    let active = if version.code == 0 {
        version_engine(bounded_text(&version.stdout)?)
    } else {
        Engine::Unknown
    };
    let markers = crate::device_io::shell_run(dev, MARKERS)?;
    let text = bounded_text(&markers.stdout)?;
    if markers.code != 0 || !root_uid(text) {
        return Ok(state);
    }
    let magisk = marker(text, "MAGISK=")?;
    let ksu = marker(text, "KSU=")?;
    state.magisk_markers = Some(magisk);
    state.kernelsu_markers = Some(ksu);
    state.engine = match (active, magisk, ksu) {
        (_, true, true) | (Engine::Magisk, _, true) | (Engine::KernelsuFamily, true, _) => {
            Engine::Conflicting
        }
        (Engine::Magisk, true, false) => Engine::Magisk,
        (Engine::KernelsuFamily, false, true) => Engine::KernelsuFamily,
        _ => Engine::Unknown,
    };
    Ok(state)
}

/// On-demand read-only query. May show a su permission prompt; never polled automatically.
#[tauri::command]
pub async fn root_inspect(serial: String) -> Result<RootState, String> {
    if serial.trim().is_empty() {
        return Err("작업 대상 기기 식별값이 필요합니다".into());
    }
    crate::tasks::guarded(std::time::Duration::from_secs(60), move || {
        crate::adb::with_first_device(&Some(serial), inspect)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    fn device(version: &str, markers: &str) -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            crate::device_io::su!("id 2>&1"),
            "uid=0(root) gid=0(root)\n",
        );
        d.answer_shell(VERSION, version);
        d.answer_shell(MARKERS, markers);
        d
    }
    #[test]
    fn detects_active_family_without_manager_package_or_root_writes() {
        for (version, markers, expected) in [
            ("30.7:MAGISKSU\n", "MAGISK=1\nKSU=0", Engine::Magisk),
            (
                "4.2.0:KernelSU\n",
                "MAGISK=0\nKSU=1",
                Engine::KernelsuFamily,
            ),
            (
                "4.2.0:ReSukiSU\n",
                "MAGISK=0\nKSU=1",
                Engine::KernelsuFamily,
            ),
            ("30.7:MAGISKSU", "MAGISK=1\nKSU=1", Engine::Conflicting),
            ("4.2.0:KernelSU", "MAGISK=1\nKSU=0", Engine::Conflicting),
            ("unknown", "MAGISK=1\nKSU=0", Engine::Unknown),
            ("30.7:MAGISKSU", "MAGISK=0\nKSU=0", Engine::Unknown),
        ] {
            let mut d = device(version, &format!("uid=0(root)\n{markers}\n"));
            assert_eq!(inspect(&mut d).unwrap().engine, expected);
            assert_eq!(d.shell_calls.len(), 3);
            assert!(d.installs.is_empty());
            assert_eq!(d.reboot_calls, 0);
        }
    }
    #[test]
    fn refusal_missing_su_and_nonroot_never_become_stock_or_empty_markers() {
        for (code, body, expected) in [
            (1, "permission denied", Access::Denied),
            (127, "missing", Access::Unavailable),
            (0, "uid=2000(shell)", Access::Unknown),
            (2, "error", Access::Unknown),
        ] {
            let mut d = device("30.7:MAGISKSU", "");
            let command = crate::device_io::su!("id 2>&1");
            d.answer_shell(command, body);
            d.shell_exit_codes.insert(command.into(), code);
            let state = inspect(&mut d).unwrap();
            assert_eq!(state.access, expected);
            assert_eq!(state.engine, Engine::Unknown);
            assert_eq!(state.magisk_markers, None);
            assert_eq!(d.shell_calls.len(), 1);
        }
        assert!(!root_uid("otheruid=0 uid=00 uid=01"));
    }
    #[test]
    fn missing_marker_permission_and_malformed_or_truncated_output_do_not_pass() {
        let mut d = device("30.7:MAGISKSU", "uid=0(root)\nMAGISK=1\nKSU=0");
        d.shell_exit_codes.insert(MARKERS.into(), 2);
        assert_eq!(inspect(&mut d).unwrap().engine, Engine::Unknown);
        d.shell_exit_codes.clear();
        d.answer_shell(MARKERS, "uid=0(root)\nMAGISK=1\nMAGISK=0\nKSU=0");
        assert!(inspect(&mut d).is_err());
        d.shell_truncated.insert(VERSION.into());
        assert!(inspect(&mut d).is_err());
        assert!(bounded_text(&vec![0; 4097]).is_err());
    }
}
