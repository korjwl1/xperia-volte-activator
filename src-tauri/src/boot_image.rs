//! 부트 이미지 입력의 공통 형식/크기 검사. 순정 인증·AVB 검증을 대신하지 않는다.
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_BYTES: usize = 256 * 1024 * 1024;

fn u32_at(data: &[u8], offset: usize) -> u64 {
    let b = &data[offset..offset + 4];
    u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as u64
}

/// 헤더가 말하는 이미지 최소 길이(AOSP bootimg.h v0~v4 레이아웃). 해석할 수 없으면 None.
/// v0~v2: [헤더 page][kernel][ramdisk][second](v1+ recovery_dtbo, v2+ dtb) — 각 구역 page 정렬
/// v3·v4: page 4096 고정, [헤더][kernel][ramdisk](v4+ boot signature)
/// 호출 전에 길이 ≥ 4096이 보장돼야 한다(필드는 모두 1660바이트 안).
fn required_len(data: &[u8]) -> Option<u64> {
    let version = u32_at(data, 40);
    let (page, sections): (u64, Vec<u64>) = match version {
        0..=2 => {
            let page = u32_at(data, 36);
            if !(2048..=65536).contains(&page) || !page.is_power_of_two() {
                return None;
            }
            let mut s = vec![u32_at(data, 8), u32_at(data, 16), u32_at(data, 24)];
            if version >= 1 {
                s.push(u32_at(data, 1632)); // recovery_dtbo_size
            }
            if version >= 2 {
                s.push(u32_at(data, 1648)); // dtb_size
            }
            (page, s)
        }
        3 | 4 => {
            let mut s = vec![u32_at(data, 8), u32_at(data, 12)];
            if version == 4 {
                s.push(u32_at(data, 1580)); // signature_size
            }
            (4096, s)
        }
        _ => return None,
    };
    sections.into_iter().try_fold(page, |total, size| {
        let aligned = size.checked_add(page - 1)? / page * page;
        total.checked_add(aligned)
    })
}

pub fn validate(data: &[u8]) -> Result<(), String> {
    if !(4096..=MAX_BYTES).contains(&data.len()) || !data.starts_with(b"ANDROID!") {
        return Err("부트 이미지 형식/크기가 올바르지 않습니다 — SIN 원본이나 잘린 파일은 사용할 수 없습니다".into());
    }
    // 매직만으로는 잘린 이미지·여러 조각 중 첫 조각을 걸러내지 못한다 — 헤더가 말하는 길이와 대조
    let need = required_len(data).ok_or(
        "부트 이미지 헤더를 해석할 수 없습니다 — 지원하지 않는 형식이거나 손상된 파일입니다",
    )?;
    if (data.len() as u64) < need {
        return Err(format!(
            "부트 이미지가 잘렸습니다 — 헤더 기준 {need}바이트가 필요하지만 {}바이트입니다",
            data.len()
        ));
    }
    Ok(())
}

/// 테스트용 v4 부트 이미지(kernel 없음, ramdisk = 나머지, init_boot 형태) — 헤더 뒤는 fill로 채워 내용이 구별된다
#[cfg(test)]
pub fn test_image(len: usize, fill: u8) -> Vec<u8> {
    assert!(len >= 4096 && len.is_multiple_of(4096));
    let mut bytes = vec![fill; len];
    bytes[..8].copy_from_slice(b"ANDROID!");
    bytes[8..44].fill(0);
    bytes[12..16].copy_from_slice(&((len - 4096) as u32).to_le_bytes()); // ramdisk_size
    bytes[40..44].copy_from_slice(&4u32.to_le_bytes()); // header_version
    bytes[1580..1584].fill(0); // signature_size
    bytes
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
    fn header_declared_length_must_be_present() {
        assert!(validate(&test_image(8192, 0x41)).is_ok());
        // 헤더는 ramdisk 8192B를 말하는데 파일은 4096B뿐(잘림·첫 조각만)
        let mut cut = test_image(12288, 0x41);
        cut.truncate(8192);
        assert!(validate(&cut).unwrap_err().contains("잘렸습니다"));
        // v2: page 2048, kernel 3000B → 헤더 2048 + kernel 4096 = 6144B 필요
        let mut v2 = vec![0u8; 4096];
        v2[..8].copy_from_slice(b"ANDROID!");
        v2[8..12].copy_from_slice(&3000u32.to_le_bytes());
        v2[36..40].copy_from_slice(&2048u32.to_le_bytes());
        v2[40..44].copy_from_slice(&2u32.to_le_bytes());
        assert!(validate(&v2).is_err());
        v2.resize(6144, 0);
        assert!(validate(&v2).is_ok());
        // 알 수 없는 헤더 버전·잘못된 page 크기는 해석 불가
        let mut v9 = test_image(4096, 0);
        v9[40..44].copy_from_slice(&9u32.to_le_bytes());
        assert!(validate(&v9).is_err());
        let mut bad_page = v2.clone();
        bad_page[36..40].copy_from_slice(&3000u32.to_le_bytes());
        assert!(validate(&bad_page).is_err());
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
