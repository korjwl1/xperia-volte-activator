//! 부트 이미지 입력의 공통 형식/크기 검사. 순정 인증·AVB 검증을 대신하지 않는다.
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_BYTES: usize = 256 * 1024 * 1024;

pub fn validate(data: &[u8]) -> Result<(), String> {
    if !(4096..=MAX_BYTES).contains(&data.len()) || !data.starts_with(b"ANDROID!") {
        return Err("부트 이미지 형식/크기가 올바르지 않습니다 — SIN 원본이나 잘린 파일은 사용할 수 없습니다".into());
    }
    Ok(())
}
pub fn read(path: &Path) -> Result<Vec<u8>, String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("부트 이미지 조회 실패: {e}"))?;
    if !meta.is_file() || !(4096..=MAX_BYTES as u64).contains(&meta.len()) {
        return Err("부트 이미지 파일 크기가 올바르지 않습니다".into());
    }
    let bytes =
        crate::storage::read_bounded(path, MAX_BYTES)?.ok_or("부트 이미지 파일이 없습니다")?;
    validate(&bytes)?;
    Ok(bytes)
}

pub fn sha256(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn verify_fingerprint(
    dev: &mut dyn adb_client::ADBDeviceExt,
    expected: &str,
) -> Result<(), String> {
    if expected.is_empty()
        || crate::device_io::shell(dev, "getprop ro.build.fingerprint")?.trim() != expected
    {
        return Err("부트 이미지의 펌웨어 지문이 현재 기기와 다릅니다 — 현재 버전에 맞는 펌웨어를 준비해 주세요".into());
    }
    Ok(())
}

#[tauri::command]
pub async fn boot_image_check(
    serial: String,
    path: String,
    fingerprint: String,
) -> Result<String, String> {
    if serial.trim().is_empty() {
        return Err("작업 대상 기기 식별값이 필요합니다".into());
    }
    crate::tasks::guarded(std::time::Duration::from_secs(60), move || {
        let bytes = read(Path::new(&path))?;
        crate::adb::with_first_device(&Some(serial), |dev| verify_fingerprint(dev, &fingerprint))?;
        Ok(sha256(&bytes))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn malformed_and_oversized_images_fail_before_device_io() {
        assert!(validate(b"ANDROID!").is_err());
        assert!(validate(&vec![0; 4096]).is_err());
        let file = tempfile::NamedTempFile::new().unwrap();
        file.as_file().set_len(MAX_BYTES as u64 + 1).unwrap();
        assert!(read(file.path()).is_err());
    }

    #[test]
    fn fingerprint_must_match_the_current_firmware() {
        let mut device = FakeADBDevice::new();
        device.answer_shell("getprop ro.build.fingerprint", "Sony/current\n");
        assert!(verify_fingerprint(&mut device, "Sony/current").is_ok());
        assert!(verify_fingerprint(&mut device, "Sony/new").is_err());
        assert!(verify_fingerprint(&mut device, "").is_err());
    }
}
