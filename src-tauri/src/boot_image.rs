//! 부트 이미지 입력의 공통 형식/크기 검사. 순정 인증·AVB 검증을 대신하지 않는다.
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

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

/// 테스트용 boot 이미지(헤더 v4, 커널 = 나머지, 램디스크 없음)
#[cfg(test)]
pub fn test_boot_kernel_image(len: usize, fill: u8) -> Vec<u8> {
    let mut bytes = test_image(len, fill);
    bytes[8..12].copy_from_slice(&((len - 4096) as u32).to_le_bytes()); // kernel_size
    bytes[12..16].fill(0); // ramdisk_size
    bytes
}

/// 이 도구가 다루는 부트 파티션(슬롯 접미사 없는 기저명) — 펌웨어 추출·패치·기록·리락 진단이 함께 쓴다.
/// 그 외 파티션(abl·xbl·vbmeta 등)은 잘못 기록하면 복구할 수 없으므로 직접 호출도 거부한다.
pub const BOOT_PARTITIONS: [&str; 2] = ["boot", "init_boot"];

pub fn validate_partition(partition: &str) -> Result<(), String> {
    if BOOT_PARTITIONS.contains(&partition) {
        Ok(())
    } else {
        Err("지원하는 파티션은 boot·init_boot뿐입니다(슬롯 접미사 없이)".into())
    }
}

/// 이미지 종류가 대상 파티션에 맞는지 — init_boot는 헤더 v4·커널 없음(램디스크만), boot는 커널 포함.
/// init_boot 이미지를 boot에(또는 그 반대로) 기록하면 기기가 부팅되지 않는다.
pub fn check_fits_partition(data: &[u8], partition: &str) -> Result<(), String> {
    validate_partition(partition)?;
    validate(data)?;
    let (version, kernel) = (u32_at(data, 40), u32_at(data, 8));
    match partition {
        "init_boot" if version != 4 || kernel != 0 => Err(
            "init_boot 이미지가 아닙니다(헤더 v4·커널 없음이어야 함) — 다른 파티션의 이미지는 기록하지 않습니다".into(),
        ),
        "boot" if kernel == 0 => Err(
            "boot 이미지가 아닙니다(커널 없음) — init_boot 이미지는 boot에 기록하지 않습니다".into(),
        ),
        _ => Ok(()),
    }
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

/// 펌웨어에서 추출한 이미지의 출처 기록 — 추출 때 update.xml 지문과 함께 이미지 옆에 저장한다(`<이미지>.json`).
/// 이미지를 쓰는 명령(기기 지문 대조·패치)은 호출부가 넘긴 지문 대신 이 기록을 기준으로 삼는다.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageOrigin {
    pub partition: String,
    pub fingerprint: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_sha256: Option<String>,
}

fn origin_path(image: &Path) -> PathBuf {
    let mut name = image.as_os_str().to_owned();
    name.push(".json");
    PathBuf::from(name)
}

/// 이미지와 출처 기록을 함께 저장(각각 원자 저장). 기록이 없는 이미지는 나중에 쓰지 않는다
pub fn save_with_origin(
    path: &Path,
    image: &[u8],
    partition: &str,
    fingerprint: &str,
) -> Result<(), String> {
    validate_partition(partition)?;
    if fingerprint.trim().is_empty() {
        return Err("펌웨어 지문이 없어 이미지 출처를 기록할 수 없습니다".into());
    }
    crate::storage::atomic_write(path, image).map_err(|e| format!("부트 이미지 저장 실패: {e}"))?;
    let origin = ImageOrigin {
        partition: partition.into(),
        fingerprint: fingerprint.trim().into(),
        sha256: sha256(image),
        source_sha256: None,
    };
    let json = serde_json::to_vec(&origin).map_err(|e| format!("이미지 출처 기록 실패: {e}"))?;
    crate::storage::atomic_write(&origin_path(path), &json)
        .map_err(|e| format!("이미지 출처 기록 저장 실패: {e}"))
}

/// 이미지 옆 출처 기록을 읽어 이 바이트와 맞는지 확인한다. 호출부가 지문을 넘겼다면 기록과 같아야 한다
pub fn load_origin(
    path: &Path,
    image: &[u8],
    claimed_fingerprint: &str,
) -> Result<ImageOrigin, String> {
    const REFETCH: &str = "사전 준비에서 펌웨어를 다시 받아 주세요";
    let raw = crate::storage::read_bounded(&origin_path(path), 64 * 1024)?
        .ok_or_else(|| format!("부트 이미지의 펌웨어 출처 기록이 없습니다 — {REFETCH}"))?;
    let origin: ImageOrigin = serde_json::from_slice(&raw)
        .map_err(|_| format!("부트 이미지의 펌웨어 출처 기록이 손상됐습니다 — {REFETCH}"))?;
    validate_partition(&origin.partition)?;
    if origin.fingerprint.trim().is_empty() {
        return Err("이미지 출처의 펌웨어 지문이 없습니다".into());
    }
    if origin.source_sha256.as_ref().is_some_and(|h| {
        h.len() != 64
            || !h.bytes().all(|c| c.is_ascii_hexdigit())
            || h.eq_ignore_ascii_case(&origin.sha256)
    }) {
        return Err("패치 이미지의 순정 출처 해시가 올바르지 않습니다".into());
    }
    if !origin.sha256.eq_ignore_ascii_case(&sha256(image)) {
        return Err(format!(
            "부트 이미지가 펌웨어에서 추출한 뒤 바뀌었습니다 — {REFETCH}"
        ));
    }
    let claimed = claimed_fingerprint.trim();
    if !claimed.is_empty() && claimed != origin.fingerprint {
        return Err("지정한 지문이 이 부트 이미지의 펌웨어 지문과 다릅니다".into());
    }
    Ok(origin)
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct DeviceImageCheck {
    origin: ImageOrigin,
    device_key: String,
    checked_at: String,
}

fn check_path(dir: &Path, device_key: &str, hash: &str) -> Result<PathBuf, String> {
    if [device_key, hash]
        .iter()
        .any(|v| v.len() != 64 || !v.bytes().all(|c| c.is_ascii_hexdigit()))
    {
        return Err("이미지 검사 기록의 기기 키/해시가 올바르지 않습니다".into());
    }
    Ok(dir.join("image-checks").join(format!(
        "{}-{}.json",
        device_key.to_ascii_lowercase(),
        hash.to_ascii_lowercase()
    )))
}

pub(crate) fn save_device_check(
    dir: &Path,
    device_key: &str,
    origin: &ImageOrigin,
) -> Result<(), String> {
    observe_firmware(dir, device_key, &origin.fingerprint)?;
    let proof = DeviceImageCheck {
        origin: origin.clone(),
        device_key: device_key.into(),
        checked_at: chrono::Utc::now().to_rfc3339(),
    };
    crate::storage::atomic_write(
        &check_path(dir, device_key, &origin.sha256)?,
        &serde_json::to_vec(&proof).map_err(|e| e.to_string())?,
    )
}

static OBSERVED_FIRMWARE: std::sync::Mutex<
    Option<std::collections::HashMap<PathBuf, (String, bool)>>,
> = std::sync::Mutex::new(None);

pub(crate) fn observe_physical_firmware(
    dir: &Path,
    serial: &str,
    fingerprint: &str,
) -> Result<(), String> {
    if serial.trim().is_empty() {
        return Ok(());
    }
    observe_firmware(dir, &sha256(serial.trim().as_bytes()), fingerprint)
}

pub(crate) fn observe_firmware(
    dir: &Path,
    device_key: &str,
    fingerprint: &str,
) -> Result<(), String> {
    if fingerprint.trim().is_empty() {
        return Ok(());
    }
    let path = dir
        .join("image-checks")
        .join(format!("{}-firmware.json", device_key.to_ascii_lowercase()));
    // Validate the key before constructing a filename from it.
    check_path(dir, device_key, &"0".repeat(64))?;
    let value = serde_json::to_vec(fingerprint).map_err(|e| e.to_string())?;
    let result = (|| {
        if crate::storage::read_bounded(&path, 64 * 1024)?.as_deref() != Some(value.as_slice()) {
            crate::storage::atomic_write(&path, &value)?;
        }
        Ok(())
    })();
    OBSERVED_FIRMWARE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_default()
        .insert(path.clone(), (fingerprint.into(), result.is_ok()));
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result
}

/// A supplied hash alone is not evidence that this image matched this phone's firmware.
pub(crate) fn require_device_check(
    dir: &Path,
    device_key: &str,
    path: &Path,
    image: &[u8],
    partition: &str,
    stock_only: bool,
) -> Result<(), String> {
    let origin = load_origin(path, image, "")?;
    let observed_path = dir
        .join("image-checks")
        .join(format!("{}-firmware.json", device_key.to_ascii_lowercase()));
    if OBSERVED_FIRMWARE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
        .and_then(|m| m.get(&observed_path))
        .is_some_and(|(fp, saved)| !saved || fp != &origin.fingerprint)
    {
        return Err("현재 펌웨어 관찰과 이미지 출처가 다르거나 관찰 기록을 저장하지 못했습니다 — OS에서 다시 대조하세요".into());
    }
    check_fits_partition(image, partition)?;
    if origin.partition != partition || (stock_only && origin.source_sha256.is_some()) {
        return Err("현재 요청에 맞는 순정 이미지 출처가 아닙니다".into());
    }
    let raw = crate::storage::read_bounded(&check_path(dir, device_key, &origin.sha256)?, 64 * 1024)?
        .ok_or("이 기기와 이미지의 펌웨어 대조 기록이 없습니다 — OS에서 boot_image_check 또는 magisk_patch를 먼저 실행하세요")?;
    let proof: DeviceImageCheck =
        serde_json::from_slice(&raw).map_err(|e| format!("이미지 대조 기록 손상: {e}"))?;
    if proof.device_key != device_key || proof.origin != origin || proof.checked_at.is_empty() {
        return Err("이미지·기기·펌웨어 대조 기록이 요청과 다릅니다".into());
    }
    let current = crate::storage::read_bounded(
        &dir.join("image-checks")
            .join(format!("{}-firmware.json", device_key.to_ascii_lowercase())),
        64 * 1024,
    )?
    .ok_or("현재 펌웨어 확인 기록이 없습니다")?;
    let current: String = serde_json::from_slice(&current).map_err(|e| e.to_string())?;
    if current != origin.fingerprint {
        return Err(
            "이미지 확인 이후 기기의 펌웨어가 변경됐습니다 — 현재 펌웨어를 다시 대조하세요".into(),
        );
    }
    Ok(())
}

pub(crate) fn save_patched_origin(
    path: &Path,
    image: &[u8],
    stock: &ImageOrigin,
) -> Result<ImageOrigin, String> {
    check_fits_partition(image, &stock.partition)?;
    let origin = ImageOrigin {
        partition: stock.partition.clone(),
        fingerprint: stock.fingerprint.clone(),
        sha256: sha256(image),
        source_sha256: Some(stock.sha256.clone()),
    };
    crate::storage::atomic_write(
        &origin_path(path),
        &serde_json::to_vec(&origin).map_err(|e| e.to_string())?,
    )?;
    Ok(origin)
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
        let path = Path::new(&path);
        let bytes = read(path)?;
        // 기기와 대조하는 지문은 추출 때 기록한 것 — 넘겨받은 지문은 그 기록과 같을 때만 인정
        let origin = load_origin(path, &bytes, &fingerprint)?;
        check_fits_partition(&bytes, &origin.partition)?;
        let dir = crate::app_paths::data_dir().ok_or("앱 데이터 폴더 없음")?;
        crate::adb::with_first_device(&Some(serial), |dev| {
            verify_fingerprint(dev, &origin.fingerprint)?;
            let key = crate::device_io::identity_key(dev)?;
            save_device_check(&dir, &key, &origin)
        })?;
        Ok(sha256(&bytes))
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn firmware_observation_uses_physical_identity_and_storage_failure_invalidates_proof() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stock.img");
        let image = test_image(8192, 0x41);
        let key = sha256(b"TEST-SERIAL");
        save_with_origin(&path, &image, "init_boot", "Sony/current").unwrap();
        let origin = load_origin(&path, &image, "").unwrap();
        save_device_check(dir.path(), &key, &origin).unwrap();
        observe_physical_firmware(dir.path(), " TEST-SERIAL ", "Sony/updated").unwrap();
        assert!(require_device_check(dir.path(), &key, &path, &image, "init_boot", true).is_err());
        save_device_check(dir.path(), &key, &origin).unwrap();
        let stamp = dir
            .path()
            .join("image-checks")
            .join(format!("{key}-firmware.json"));
        std::fs::remove_file(&stamp).unwrap();
        std::fs::create_dir(&stamp).unwrap();
        assert!(observe_physical_firmware(dir.path(), "TEST-SERIAL", "Sony/current").is_err());
        assert!(require_device_check(dir.path(), &key, &path, &image, "init_boot", true).is_err());
    }

    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn caller_hash_cannot_replace_device_firmware_check_or_patch_provenance() {
        let dir = tempfile::tempdir().unwrap();
        let stock = dir.path().join("stock.img");
        let patched = dir.path().join("patched.img");
        let image = test_image(8192, 0x41);
        let changed = test_image(8192, 0x42);
        let key = "a".repeat(64);
        save_with_origin(&stock, &image, "init_boot", "Sony/current").unwrap();
        assert!(
            require_device_check(dir.path(), &key, &stock, &image, "init_boot", false).is_err()
        );
        let origin = load_origin(&stock, &image, "").unwrap();
        save_device_check(dir.path(), &key, &origin).unwrap();
        assert!(require_device_check(dir.path(), &key, &stock, &image, "init_boot", true).is_ok());
        assert!(require_device_check(
            dir.path(),
            &"b".repeat(64),
            &stock,
            &image,
            "init_boot",
            false
        )
        .is_err());
        assert!(
            require_device_check(dir.path(), &key, &stock, &changed, "init_boot", false).is_err()
        );
        assert!(require_device_check(dir.path(), &key, &stock, &image, "boot", false).is_err());
        std::fs::write(&patched, &changed).unwrap();
        let patched_origin = save_patched_origin(&patched, &changed, &origin).unwrap();
        save_device_check(dir.path(), &key, &patched_origin).unwrap();
        assert!(
            require_device_check(dir.path(), &key, &patched, &changed, "init_boot", false).is_ok()
        );
        assert!(
            require_device_check(dir.path(), &key, &patched, &changed, "init_boot", true).is_err()
        );
        observe_firmware(dir.path(), &key, "Sony/updated").unwrap();
        assert!(require_device_check(dir.path(), &key, &stock, &image, "init_boot", true).is_err());
    }

    #[test]
    fn extracted_image_carries_its_firmware_fingerprint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("init_boot.img");
        let img = test_image(8192, 0x41);
        save_with_origin(
            &path,
            &img,
            "init_boot",
            "Sony/XQ-DQ44/XQ-DQ44:15/67.2.A.3.178/1:user/release-keys",
        )
        .unwrap();
        let origin = load_origin(&path, &img, "").unwrap();
        assert_eq!(origin.partition, "init_boot");
        // 넘겨받은 지문은 기록과 같아야 한다
        assert!(load_origin(&path, &img, &origin.fingerprint).is_ok());
        assert!(load_origin(&path, &img, "Sony/other").is_err());
        // 기록 뒤 바뀐 이미지·기록 없는 이미지는 쓰지 않는다
        assert!(load_origin(&path, &test_image(8192, 0x42), "").is_err());
        let bare = dir.path().join("boot.img");
        std::fs::write(&bare, &img).unwrap();
        assert!(load_origin(&bare, &img, "").is_err());
        // 지문 없이는 기록하지 않는다
        assert!(save_with_origin(&bare, &img, "init_boot", " ").is_err());
    }

    #[test]
    fn image_kind_must_match_the_partition() {
        let init_boot = test_image(8192, 0x41);
        let boot = test_boot_kernel_image(8192, 0x42);
        assert!(check_fits_partition(&init_boot, "init_boot").is_ok());
        assert!(check_fits_partition(&boot, "boot").is_ok());
        // 서로 바꿔 기록하면 부팅되지 않는다
        assert!(check_fits_partition(&init_boot, "boot").is_err());
        assert!(check_fits_partition(&boot, "init_boot").is_err());
        // v2 boot(커널 있음)는 boot에만
        let mut v2 = vec![0u8; 8192];
        v2[..8].copy_from_slice(b"ANDROID!");
        v2[8..12].copy_from_slice(&3000u32.to_le_bytes());
        v2[36..40].copy_from_slice(&2048u32.to_le_bytes());
        v2[40..44].copy_from_slice(&2u32.to_le_bytes());
        assert!(check_fits_partition(&v2, "boot").is_ok());
        assert!(check_fits_partition(&v2, "init_boot").is_err());
        assert!(check_fits_partition(&init_boot, "vbmeta").is_err());
    }

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
