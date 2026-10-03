//! fastboot 엔진 — 실기기 테스트 금지, FakeTransport로 검증.
//! 쓰기/재부팅은 기본 꺼진 Cargo feature fastboot-write + 프론트 게이트 뒤에 있다.

pub mod protocol;
pub mod transport;

use protocol::FastbootDevice;
use serde::Serialize;
use std::io::{Read, Write};
use std::sync::Mutex;
use tauri::Emitter;

static DEVICE_OPERATION: Mutex<()> = Mutex::new(());

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockResult {
    pub unlocked: bool,
}

fn ensure_write_enabled() -> Result<(), String> {
    if cfg!(feature = "fastboot-write") {
        Ok(())
    } else {
        Err("fastboot 쓰기/재부팅은 이 빌드에서 비활성화되어 있습니다(실기기 검증 대기)".into())
    }
}

fn ensure_relock_verified() -> Result<(), String> {
    // §3-3: 언루팅 done/skipped 또는 su 부재로는 AVB 안전을 증명할 수 없다.
    // 세션·기기별 순정 해시와 부트 체인×슬롯 이력 검증 구현 전에는 허용하지 않는다.
    Err("리락 차단: 검증된 순정 이미지와 부트 체인·슬롯별 플래시 이력 게이트가 아직 구현되지 않았습니다 (§3-3)".into())
}

/// 로그와 반환 오류를 같은 규칙으로 마스킹한다.
fn redact(line: &str, secret: Option<&str>) -> String {
    let mut line = line.to_string();
    if let Some(secret) = secret.filter(|s| !s.is_empty()) {
        // 기기의 대소문자 변형도 처리한다. ASCII 일치 위치는 UTF-8 경계다.
        let lower = line.to_ascii_lowercase();
        let needle = secret.to_ascii_lowercase();
        for (start, _) in lower
            .match_indices(&needle)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            line.replace_range(start..start + secret.len(), "[마스킹]");
        }
    }
    let lower = line.to_ascii_lowercase();
    if ["imei", "meid", "serialno", "serial-number"]
        .iter()
        .any(|key| lower.contains(key))
    {
        // getvar:all의 기기 식별값이 journal 로그로 흘러가지 않게 한다.
        if let Some((name, _)) = line.split_once(':') {
            return format!("{name}: [마스킹]");
        }
        return "[기기 식별정보 마스킹]".into();
    }
    line
}

fn with_device<T>(
    app: tauri::AppHandle,
    secret: Option<String>,
    work: impl FnOnce(FastbootDevice<transport::RusbTransport>) -> Result<T, String>,
) -> Result<T, String> {
    let _guard = DEVICE_OPERATION
        .try_lock()
        .map_err(|_| "다른 fastboot 작업이 진행 중입니다")?;
    let t = transport::RusbTransport::open()?;
    let log_secret = secret.clone();
    let dev = FastbootDevice::new(
        t,
        Box::new(move |line| {
            let _ = app.emit("fastboot:log", redact(&line, log_secret.as_deref()));
        }),
    );
    work(dev).map_err(|e| redact(&e, secret.as_deref()))
}

/// getvar:all — 읽기 전용 프로브
#[tauri::command]
pub async fn fastboot_getvar(
    app: tauri::AppHandle,
) -> Result<std::collections::HashMap<String, String>, String> {
    tauri::async_runtime::spawn_blocking(move || with_device(app, None, |mut d| d.getvar_all()))
        .await
        .map_err(|e| format!("프로브 스레드 오류: {e}"))?
}

fn normalize_unlock_code(raw: &str) -> Result<String, String> {
    let raw = raw.trim();
    let code = raw
        .strip_prefix("0x")
        .or_else(|| raw.strip_prefix("0X"))
        .unwrap_or(raw);
    protocol::validate_unlock_code(code)?;
    Ok(code.to_string())
}

#[tauri::command]
pub async fn fastboot_unlock(
    app: tauri::AppHandle,
    code: String,
    confirm: bool,
) -> Result<UnlockResult, String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    let code = normalize_unlock_code(&code)?;
    let secret = code.clone();
    tauri::async_runtime::spawn_blocking(move || {
        with_device(app, Some(secret), move |mut d| {
            d.ensure_bootloader()?;
            if !d.unlocked()? {
                d.oem_unlock(&code)?;
            }
            let unlocked = d.unlocked()?;
            if !unlocked {
                return Err("언락 후 unlocked=yes가 확인되지 않습니다".into());
            }
            Ok(UnlockResult { unlocked })
        })
    })
    .await
    .map_err(|e| format!("언락 스레드 오류: {e}"))?
}

#[tauri::command]
pub async fn fastboot_lock(app: tauri::AppHandle, confirm: bool) -> Result<UnlockResult, String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    ensure_relock_verified()?;
    tauri::async_runtime::spawn_blocking(move || {
        with_device(app, None, |mut d| {
            d.ensure_bootloader()?;
            if d.unlocked()? {
                d.oem_lock()?;
            }
            let unlocked = d.unlocked()?;
            if unlocked {
                return Err("리락 후 unlocked=no가 확인되지 않습니다".into());
            }
            Ok(UnlockResult { unlocked })
        })
    })
    .await
    .map_err(|e| format!("리락 스레드 오류: {e}"))?
}

/// 양쪽 슬롯을 대상으로 하는 기본 파티션명만 허용한다.
fn validate_base_partition(partition: &str) -> Result<(), String> {
    if partition.is_empty()
        || partition.len() > 48
        || partition.ends_with("_a")
        || partition.ends_with("_b")
        || !partition
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
    {
        return Err("슬롯 접미사 없는 기본 파티션명을 입력해 주세요".into());
    }
    Ok(())
}

/// 파일은 blocking 스레드에서 읽고, 파일 크기가 변해도 최대 크기까지만 할당한다.
fn read_image(path: &str) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("이미지 파일 열기 실패: {e}"))?;
    let size = file.metadata().map_err(|e| e.to_string())?.len();
    if size == 0 || size > protocol::MAX_DOWNLOAD {
        return Err("이미지 크기가 허용 범위를 벗어났습니다".into());
    }
    let mut image = Vec::new();
    file.take(protocol::MAX_DOWNLOAD + 1)
        .read_to_end(&mut image)
        .map_err(|e| e.to_string())?;
    if image.is_empty() || image.len() as u64 > protocol::MAX_DOWNLOAD {
        return Err("이미지 크기가 허용 범위를 벗어났습니다".into());
    }
    Ok(image)
}

#[tauri::command]
pub async fn fastboot_flash(
    app: tauri::AppHandle,
    partition: String,
    path: String,
    confirm: bool,
) -> Result<(), String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    validate_base_partition(&partition)?;
    tauri::async_runtime::spawn_blocking(move || {
        let image = read_image(&path)?;
        use sha2::{Digest, Sha256};
        // 디스크 파일을 다시 읽지 않고 실제 전송할 버퍼를 해싱한다.
        let sha256 = hex::encode(Sha256::digest(&image));
        with_device(app, None, move |mut d| {
            d.ensure_bootloader()?;
            let serial = d
                .getvar("serialno")?
                .filter(|s| !s.trim().is_empty())
                .ok_or("플래시 이력에 연결할 기기 식별값을 확인할 수 없습니다")?;
            let device_key = hex::encode(Sha256::digest(serial.as_bytes()));
            let slot = d
                .getvar("current-slot")?
                .ok_or("현재 슬롯을 확인할 수 없습니다")?;
            if slot != "a" && slot != "b" {
                return Err("현재 슬롯이 올바르지 않습니다".into());
            }
            if d.getvar(&format!("has-slot:{partition}"))?.as_deref() != Some("yes") {
                return Err("양쪽 슬롯이 있는 파티션인지 확인할 수 없습니다".into());
            }
            let dir = crate::app_paths::data_dir().ok_or("앱 데이터 폴더를 찾지 못했습니다")?;
            std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
            let mut history = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("flash-history.jsonl"))
                .map_err(|e| format!("플래시 이력 열기 실패: {e}"))?;
            for slot in ["_a", "_b"] {
                let part = format!("{partition}{slot}");
                // 미완료 시도도 남긴다. 하나의 슬롯만 완료된 경우 두 슬롯 성공으로 오인하지 않는다.
                record_flash_history(
                    &mut history,
                    &device_key,
                    &part,
                    &path,
                    image.len() as u64,
                    &sha256,
                    "started",
                )?;
                match d.flash(&part, &image) {
                    Ok(()) => record_flash_history(
                        &mut history,
                        &device_key,
                        &part,
                        &path,
                        image.len() as u64,
                        &sha256,
                        "done",
                    )?,
                    Err(e) => {
                        record_flash_history(
                            &mut history,
                            &device_key,
                            &part,
                            &path,
                            image.len() as u64,
                            &sha256,
                            "failed",
                        )?;
                        return Err(e);
                    }
                }
            }
            Ok(())
        })
    })
    .await
    .map_err(|e| format!("기록 스레드 오류: {e}"))?
}

#[tauri::command]
pub async fn fastboot_reboot(app: tauri::AppHandle, target: String) -> Result<(), String> {
    ensure_write_enabled()?;
    tauri::async_runtime::spawn_blocking(move || {
        with_device(app, None, move |mut d| d.reboot(&target))
    })
    .await
    .map_err(|e| format!("재부팅 스레드 오류: {e}"))?
}

fn record_flash_history(
    file: &mut std::fs::File,
    device_key: &str,
    partition: &str,
    path: &str,
    bytes: u64,
    sha256: &str,
    status: &str,
) -> Result<(), String> {
    let entry = serde_json::json!({
        "deviceKey": device_key, "partition": partition, "image": path,
        "bytes": bytes, "sha256": sha256, "status": status,
        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    });
    writeln!(file, "{entry}")
        .and_then(|_| file.sync_data())
        .map_err(|e| format!("플래시 이력 저장 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_normalization_and_validation() {
        assert_eq!(
            normalize_unlock_code(" 0X1234567890ABCDEF ").unwrap(),
            "1234567890ABCDEF"
        );
        assert!(normalize_unlock_code("0x0x1234567890abcdef").is_err());
        assert!(normalize_unlock_code("0x 1234567890abcdef").is_err());
    }

    #[test]
    fn masks_code_in_events_and_errors_case_insensitively() {
        let secret = "1234567890abcdef";
        for line in [
            "oem unlock 0x1234567890abcdef",
            "FAIL code=1234567890ABCDEF",
        ] {
            let masked = redact(line, Some(secret));
            assert!(!masked.to_ascii_lowercase().contains(secret));
            assert!(masked.contains("[마스킹]"));
        }
        assert!(!redact("(bootloader) imei: 123456789012345", None).contains("123456789012345"));
        assert!(!redact("serialno: AB12345678", None).contains("AB12345678"));
    }

    #[test]
    fn relock_stays_blocked_without_stock_evidence() {
        assert!(ensure_relock_verified().is_err());
        #[cfg(not(feature = "fastboot-write"))]
        assert!(ensure_write_enabled().is_err());
    }

    #[test]
    fn rejects_slot_suffixes_and_empty_images() {
        assert!(validate_base_partition("init_boot").is_ok());
        assert!(validate_base_partition("boot_a").is_err());
        assert!(validate_base_partition("boot;reboot").is_err());
        let image = tempfile::NamedTempFile::new().unwrap();
        assert!(read_image(image.path().to_str().unwrap()).is_err());
        std::fs::write(image.path(), b"image").unwrap();
        assert_eq!(
            read_image(image.path().to_str().unwrap()).unwrap(),
            b"image"
        );
    }

    #[test]
    fn flash_history_records_exact_buffer_hash_and_device() {
        use sha2::{Digest, Sha256};
        let mut file = tempfile::tempfile().unwrap();
        let digest = hex::encode(Sha256::digest(b"actual image"));
        record_flash_history(
            &mut file,
            "hashed-device",
            "boot_a",
            "changed.img",
            12,
            &digest,
            "done",
        )
        .unwrap();
        use std::io::Seek;
        file.rewind().unwrap();
        let entry: serde_json::Value = serde_json::from_reader(file).unwrap();
        assert_eq!(entry["sha256"], digest);
        assert_eq!(entry["deviceKey"], "hashed-device");
        assert_eq!(entry["status"], "done");
    }
}
