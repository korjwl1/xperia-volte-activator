//! Sony Flash mode engine. Hardware writes are not exposed until profiles are validated.
//! Reference: Newflasher 59f12e437d29f0385eb27dcebba5158aa3f97b45 (MIT; NOTICE.md).
// These core modules deliberately have no hardware entry point until identity/profile checks exist.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod engine;
pub(crate) mod package;
pub(crate) mod policy;
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod protocol;
pub(crate) mod sin;
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) mod transport;

pub const UPSTREAM_COMMIT: &str = "59f12e437d29f0385eb27dcebba5158aa3f97b45";
pub type Result<T> = std::result::Result<T, String>;

pub(crate) fn error(code: &str, message: &str) -> String {
    format!("FLASH_{code}|{message}")
}

/// PC-only inspection. No ADB, USB, driver installation, extraction or firmware writes.
#[tauri::command]
pub async fn firmware_package_inspect(
    dir: String,
    target_fingerprint: String,
) -> Result<package::PackageReport> {
    crate::tasks::blocking("firmware package inspection", move || {
        package::inspect(std::path::Path::new(&dir), &target_fingerprint)
    })
    .await
}

#[cfg(test)]
mod tests;
