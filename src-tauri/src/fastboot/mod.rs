//! fastboot 엔진 — 실기기 테스트 금지, FakeTransport로 검증.
//! 쓰기/재부팅은 기본 꺼진 Cargo feature fastboot-write + 프론트 게이트 뒤에 있다.

pub mod protocol;
pub mod relock;
pub mod transport;

use protocol::FastbootDevice;
use serde::Serialize;
use std::io::Write;
use tauri::Emitter;

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

/// fastboot serialno → 이력 deviceKey(fastboot_flash 기록 방식과 동일)
fn device_key(serial: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(serial.as_bytes()))
}

/// 리락 게이트(§3-3) — 이력 파일을 읽어 판정. 순수 로직은 relock::verify.
fn verify_from_dir(
    dir: &std::path::Path,
    partition: &str,
    stock_sha: &str,
    device_key: Option<&str>,
) -> Result<relock::GateResult, String> {
    let raw = crate::storage::read_bounded(&dir.join("flash-history.jsonl"), 16 * 1024 * 1024)?
        .ok_or("플래시 이력 파일이 없습니다 — 슬롯 상태를 확인할 수 없습니다")?;
    let raw = String::from_utf8(raw).map_err(|e| format!("플래시 이력 UTF-8 오류: {e}"))?;
    let history = relock::parse_history(&raw)?;
    Ok(relock::for_relock(relock::verify(
        &history, partition, stock_sha, device_key,
    )))
}

fn load_and_verify(
    partition: &str,
    stock_sha: &str,
    device_key: Option<&str>,
) -> Result<relock::GateResult, String> {
    let dir = crate::app_paths::data_dir().ok_or("앱 데이터 폴더를 찾지 못했습니다")?;
    verify_from_dir(&dir, partition, stock_sha, device_key)
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
    operation: crate::device_io::WriteOperation,
    app: tauri::AppHandle,
    secret: Option<String>,
    work: impl FnOnce(FastbootDevice<transport::RusbTransport>) -> Result<T, String>,
) -> Result<T, String> {
    // 기기 변경 실행권(전역 단일)을 작업이 끝날 때까지 쥔다 — 별도 fastboot 잠금은 필요 없다
    let _operation = operation;
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
    let operation = crate::device_io::WriteOperation::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        with_device(operation, app, None, |mut d| d.getvar_all())
    })
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
    expected_serial: String,
) -> Result<UnlockResult, String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    let code = normalize_unlock_code(&code)?;
    let secret = code.clone();
    let operation = crate::device_io::WriteOperation::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        with_device(operation, app, Some(secret), move |mut d| {
            ensure_target(&mut d, &expected_serial)?;
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

/// 순정 출처·AVB·전체 부트 체인 검증 전까지 실제 리락은 차단한다.
#[tauri::command]
pub async fn fastboot_lock(
    app: tauri::AppHandle,
    confirm: bool,
    partition: String,
    stock_path: String,
    expected_serial: String,
) -> Result<UnlockResult, String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    validate_base_partition(&partition)?;
    let _ = (app, stock_path, expected_serial);
    Err(relock::RELOCK_BLOCKED.into())
}

/// 리락 이력 진단(읽기 전용). 진단 통과도 실제 리락을 허용하지 않는다.
#[tauri::command]
pub async fn relock_gate_check(
    partition: String,
    stock_path: String,
    device_key: Option<String>,
) -> Result<relock::GateResult, String> {
    validate_base_partition(&partition)?;
    crate::tasks::blocking("리락 게이트 점검", move || {
        let stock_sha = relock::stock_sha256(std::path::Path::new(&stock_path))
            .map_err(|e| format!("순정 이미지 확인 실패: {e}"))?;
        load_and_verify(
            &partition,
            &stock_sha,
            device_key.as_deref().filter(|k| !k.is_empty()),
        )
    })
    .await
}

/// 이 도구가 기록하는 파티션은 부트 이미지 두 가지뿐이다(루팅·언루팅). 슬롯 접미사 없이 받는다.
/// 그 외 파티션(abl·xbl·vbmeta 등)은 잘못 기록하면 복구할 수 없으므로 직접 호출도 거부한다.
fn validate_base_partition(partition: &str) -> Result<(), String> {
    if matches!(partition, "boot" | "init_boot") {
        Ok(())
    } else {
        Err("지원하는 파티션은 boot·init_boot뿐입니다(슬롯 접미사 없이)".into())
    }
}

#[tauri::command]
pub async fn fastboot_flash(
    app: tauri::AppHandle,
    partition: String,
    path: String,
    confirm: bool,
    expected_serial: String,
    expected_sha256: String,
) -> Result<(), String> {
    ensure_write_enabled()?;
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    validate_base_partition(&partition)?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        let image = crate::boot_image::read(std::path::Path::new(&path))?;
        // 디스크 파일을 다시 읽지 않고 실제 전송할 버퍼를 해싱한다.
        let sha256 = crate::boot_image::sha256(&image);
        if !expected_sha256.eq_ignore_ascii_case(&sha256) {
            return Err("이미지가 사전 검사 후 변경됐습니다 — 기록하지 않습니다".into());
        }
        with_device(operation, app, None, move |mut d| {
            let serial = ensure_target(&mut d, &expected_serial)?;
            d.ensure_bootloader()?;
            let device_key = device_key(&serial);
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
            let mut record = |part: &str, status: &str| {
                record_flash_history(
                    &mut history,
                    &device_key,
                    part,
                    &path,
                    image.len() as u64,
                    &sha256,
                    status,
                )
            };
            for slot in ["_a", "_b"] {
                let part = format!("{partition}{slot}");
                // 미완료 시도도 남긴다. 하나의 슬롯만 완료된 경우 두 슬롯 성공으로 오인하지 않는다.
                record(&part, "started")?;
                match d.flash(&part, &image) {
                    Ok(()) => record(&part, "done")?,
                    Err(e) => {
                        // 이력 저장이 실패해도 기기 오류 원인은 잃지 않는다
                        return Err(match record(&part, "failed") {
                            Ok(()) => e,
                            Err(h) => format!("{e} (이력 저장도 실패: {h})"),
                        });
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
pub async fn fastboot_reboot(
    app: tauri::AppHandle,
    target: String,
    expected_serial: String,
) -> Result<(), String> {
    ensure_write_enabled()?;
    // USB를 열기 전에 대상 검사
    if !matches!(target.as_str(), "os" | "bootloader") {
        return Err(format!("알 수 없는 재부팅 대상: {target}"));
    }
    let operation = crate::device_io::WriteOperation::acquire()?;
    tauri::async_runtime::spawn_blocking(move || {
        with_device(operation, app, None, move |mut d| {
            ensure_target(&mut d, &expected_serial)?;
            d.reboot(&target)
        })
    })
    .await
    .map_err(|e| format!("재부팅 스레드 오류: {e}"))?
}

/// 작업을 시작한 기기인지 확인하고, 확인된 serialno(앞뒤 공백 제거)를 돌려준다.
fn ensure_target<T: transport::FastbootTransport>(
    d: &mut FastbootDevice<T>,
    expected: &str,
) -> Result<String, String> {
    if expected.trim().is_empty() {
        return Err("작업 대상 기기 식별값이 필요합니다".into());
    }
    let actual = d
        .getvar("serialno")?
        .ok_or("fastboot 기기 식별값을 확인할 수 없습니다")?;
    if actual.trim() != expected.trim() {
        return Err("fastboot 기기가 작업을 시작한 기기와 다릅니다".into());
    }
    Ok(actual.trim().to_string())
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
    fn wrong_or_missing_serial_prevents_mutating_commands() {
        use protocol::fake::{FakeTransport, Frame};
        for (expected, actual, allowed) in [("A", "B", false), ("", "A", false), ("A", "A", true)] {
            let transport = FakeTransport::new(vec![Frame::Ok(actual), Frame::Ok("")]);
            let mut device = FastbootDevice::new(transport, Box::new(|_| {}));
            let result = ensure_target(&mut device, expected).and_then(|_| device.reboot("os"));
            assert_eq!(result.is_ok(), allowed);
            let transport = device.into_transport();
            assert_eq!(
                transport
                    .sent_cmds
                    .iter()
                    .any(|command| command == "reboot"),
                allowed
            );
            if expected.is_empty() {
                assert!(transport.sent_cmds.is_empty());
            }
        }
    }

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
    fn relock_requires_more_than_a_successful_flash_history() {
        #[cfg(not(feature = "fastboot-write"))]
        assert!(ensure_write_enabled().is_err());
        let dir = tempfile::tempdir().unwrap();
        let stock = "a".repeat(64);
        let key = "c".repeat(64);
        assert!(verify_from_dir(dir.path(), "init_boot", &stock, Some(&key)).is_err());
        let history = ["a", "b"].map(|slot| serde_json::json!({"deviceKey": key, "partition": format!("init_boot_{slot}"), "sha256": stock, "status": "done"}).to_string()).join("\n");
        std::fs::write(dir.path().join("flash-history.jsonl"), history).unwrap();
        let gate = verify_from_dir(dir.path(), "init_boot", &stock, Some(&key)).unwrap();
        assert!(gate.checked.iter().all(|slot| slot.ok));
        assert!(!gate.ok);
        assert!(gate.reasons.iter().any(|reason| reason.contains("AVB")));
        std::fs::write(dir.path().join("flash-history.jsonl"), "{broken").unwrap();
        assert!(verify_from_dir(dir.path(), "init_boot", &stock, Some(&key)).is_err());
    }

    #[test]
    fn only_boot_partitions_without_slot_suffix_are_accepted() {
        assert!(validate_base_partition("init_boot").is_ok());
        assert!(validate_base_partition("boot").is_ok());
        assert!(validate_base_partition("boot_a").is_err());
        assert!(validate_base_partition("boot;reboot").is_err());
        // 부트 이미지가 아닌 파티션은 기록 대상이 아니다
        for other in ["abl", "xbl", "vbmeta", "modem", ""] {
            assert!(validate_base_partition(other).is_err());
        }
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
