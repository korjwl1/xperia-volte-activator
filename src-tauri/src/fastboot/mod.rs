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
    hex::encode(Sha256::digest(serial.trim().as_bytes()))
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
    Ok(relock::verify(&history, partition, stock_sha, device_key))
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
    // 읽기 전용이지만 fastboot USB 인터페이스를 독점하므로 기기 작업 실행권을 함께 쓴다
    let operation = crate::device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("fastboot 조회", move || {
        with_device(operation, app, None, |mut d| d.getvar_all())
    })
    .await
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
    crate::tasks::blocking("부트로더 언락", move || {
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
}

/// 추출 출처·양 슬롯 복원 이력·기기/모드를 확인한 뒤 리락한다.
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
    let operation = crate::device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("부트로더 리락", move || {
        let stock_sha = verified_stock_sha(&partition, &stock_path)?;
        let dir = crate::app_paths::data_dir().ok_or("앱 데이터 폴더를 찾지 못했습니다")?;
        // 로컬 선행 조건 실패는 USB를 열기 전에 거부한다. 실제 기기 기준으로도 재검사한다.
        require_relock_gate(verify_from_dir(
            &dir,
            &partition,
            &stock_sha,
            Some(&device_key(&expected_serial)),
        )?)?;
        with_device(operation, app, None, move |mut d| {
            lock_verified_device(&mut d, &expected_serial, &dir, &partition, &stock_sha)
        })
    })
    .await
}

/// 리락 로컬 선행 조건 검사(읽기 전용). 실제 기기/모드는 fastboot_lock이 재검사한다.
#[tauri::command]
pub async fn relock_gate_check(
    partition: String,
    stock_path: String,
    device_key: Option<String>,
) -> Result<relock::GateResult, String> {
    validate_base_partition(&partition)?;
    crate::tasks::blocking("리락 게이트 점검", move || {
        let stock_sha = verified_stock_sha(&partition, &stock_path)
            .map_err(|e| format!("순정 이미지 확인 실패: {e}"))?;
        load_and_verify(
            &partition,
            &stock_sha,
            device_key.as_deref().filter(|k| !k.is_empty()),
        )
    })
    .await
}

fn verified_stock_sha(partition: &str, stock_path: &str) -> Result<String, String> {
    let path = std::path::Path::new(stock_path);
    let image = crate::boot_image::read(path)?;
    crate::boot_image::check_fits_partition(&image, partition)?;
    let origin = crate::boot_image::load_origin(path, &image, "")?;
    if origin.partition != partition || origin.fingerprint.trim().is_empty() {
        return Err("순정 이미지의 추출 파티션·펌웨어 출처가 일치하지 않습니다".into());
    }
    Ok(crate::boot_image::sha256(&image))
}

fn require_relock_gate(gate: relock::GateResult) -> Result<(), String> {
    if gate.ok {
        Ok(())
    } else {
        Err(format!(
            "리락 선행 조건을 확인하지 못했습니다 — {}",
            gate.reasons.join(" / ")
        ))
    }
}

fn lock_verified_device<T: transport::FastbootTransport>(
    d: &mut FastbootDevice<T>,
    expected_serial: &str,
    dir: &std::path::Path,
    partition: &str,
    stock_sha: &str,
) -> Result<UnlockResult, String> {
    let serial = ensure_target(d, expected_serial)?;
    d.ensure_bootloader()?;
    if d.getvar(&format!("has-slot:{partition}"))?.as_deref() != Some("yes") {
        return Err("양쪽 슬롯이 있는 파티션인지 확인할 수 없습니다".into());
    }
    require_relock_gate(verify_from_dir(
        dir,
        partition,
        stock_sha,
        Some(&device_key(&serial)),
    )?)?;
    if d.unlocked()? {
        d.oem_lock()?;
    }
    let unlocked = d.unlocked()?;
    if unlocked {
        return Err("리락 후 unlocked=no가 확인되지 않습니다".into());
    }
    Ok(UnlockResult { unlocked })
}

/// 이 도구가 기록하는 파티션은 부트 이미지 두 가지뿐이다(공통 규칙: boot_image::validate_partition)
fn validate_base_partition(partition: &str) -> Result<(), String> {
    crate::boot_image::validate_partition(partition)
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
    if expected_sha256.trim().is_empty() {
        return Err("기록할 이미지의 확인값(sha256)이 없습니다 — 기록하지 않습니다".into());
    }
    let operation = crate::device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("부트 이미지 기록", move || {
        let image = crate::boot_image::read(std::path::Path::new(&path))?;
        // init_boot 이미지를 boot에(또는 반대로) 기록하면 부팅되지 않는다 — USB를 열기 전에 거부
        crate::boot_image::check_fits_partition(&image, &partition)?;
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
    crate::tasks::blocking("fastboot 재부팅", move || {
        with_device(operation, app, None, move |mut d| {
            ensure_target(&mut d, &expected_serial)?;
            d.reboot(&target)
        })
    })
    .await
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
    // 한 줄을 한 번에 쓴다 — 여러 번 나눠 쓰다 중단되면 찢어진 줄이 이력 전체를 손상으로 만든다
    file.write_all(format!("{entry}\n").as_bytes())
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
    fn relock_history_passes_only_when_both_slots_finished() {
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
        assert!(gate.ok);
        std::fs::write(dir.path().join("flash-history.jsonl"), "{broken").unwrap();
        assert!(verify_from_dir(dir.path(), "init_boot", &stock, Some(&key)).is_err());
    }

    fn write_stock_history(dir: &std::path::Path, serial: &str, sha: &str) {
        let mut file = std::fs::File::create(dir.join("flash-history.jsonl")).unwrap();
        for slot in ["a", "b"] {
            record_flash_history(
                &mut file,
                &device_key(serial),
                &format!("init_boot_{slot}"),
                "stock.img",
                8192,
                sha,
                "done",
            )
            .unwrap();
        }
    }

    #[test]
    fn relock_stock_requires_extraction_origin_partition_and_unchanged_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("stock.img");
        let image = crate::boot_image::test_image(8192, 0x41);
        std::fs::write(&path, &image).unwrap();
        let path_str = path.to_str().unwrap();
        assert!(verified_stock_sha("init_boot", path_str).is_err());
        crate::boot_image::save_with_origin(&path, &image, "boot", "Sony/current").unwrap();
        assert!(verified_stock_sha("init_boot", path_str).is_err());
        crate::boot_image::save_with_origin(&path, &image, "init_boot", "Sony/current").unwrap();
        assert_eq!(
            verified_stock_sha("init_boot", path_str).unwrap(),
            crate::boot_image::sha256(&image)
        );
        assert!(verified_stock_sha("boot", path_str).is_err());
        std::fs::write(&path, crate::boot_image::test_image(8192, 0x42)).unwrap();
        assert!(verified_stock_sha("init_boot", path_str).is_err());
    }

    #[test]
    fn relock_sends_lock_only_after_matching_device_mode_and_stock_history() {
        use protocol::fake::{FakeTransport, Frame};
        let dir = tempfile::tempdir().unwrap();
        let sha = "a".repeat(64);
        write_stock_history(dir.path(), "A", &sha);
        let cases = [
            (
                vec![
                    Frame::Ok("A"),
                    Frame::Ok("no"),
                    Frame::Ok("yes"),
                    Frame::Ok("yes"),
                    Frame::Ok(""),
                    Frame::Ok("no"),
                ],
                true,
                true,
            ),
            (vec![Frame::Ok("B")], false, false),
            (vec![Frame::Ok("A"), Frame::Ok("yes")], false, false),
            (
                vec![Frame::Ok("A"), Frame::Ok("no"), Frame::Ok("no")],
                false,
                false,
            ),
            (
                vec![
                    Frame::Ok("A"),
                    Frame::Ok("no"),
                    Frame::Ok("yes"),
                    Frame::Fail("unknown"),
                ],
                false,
                false,
            ),
            (
                vec![
                    Frame::Ok("A"),
                    Frame::Ok("no"),
                    Frame::Ok("yes"),
                    Frame::Ok("no"),
                    Frame::Ok("no"),
                ],
                true,
                false,
            ),
        ];
        for (frames, success, locked) in cases {
            let mut d = FastbootDevice::new(FakeTransport::new(frames), Box::new(|_| {}));
            let result = lock_verified_device(&mut d, "A", dir.path(), "init_boot", &sha);
            assert_eq!(result.is_ok(), success);
            if let Ok(result) = result {
                assert!(!result.unlocked);
            }
            assert_eq!(
                d.into_transport().sent_cmds.iter().any(|s| s == "oem lock"),
                locked
            );
        }
    }

    #[test]
    fn relock_stale_or_partial_history_never_sends_lock() {
        use protocol::fake::{FakeTransport, Frame};
        let dir = tempfile::tempdir().unwrap();
        let sha = "a".repeat(64);
        for mode in ["missing", "other-phone", "patched", "started", "corrupt"] {
            write_stock_history(dir.path(), "A", &sha);
            let path = dir.path().join("flash-history.jsonl");
            match mode {
                "missing" => std::fs::remove_file(&path).unwrap(),
                "other-phone" => write_stock_history(dir.path(), "B", &sha),
                "patched" => write_stock_history(dir.path(), "A", &"b".repeat(64)),
                "started" => {
                    let mut file = std::fs::OpenOptions::new()
                        .append(true)
                        .open(&path)
                        .unwrap();
                    record_flash_history(
                        &mut file,
                        &device_key("A"),
                        "init_boot_b",
                        "stock.img",
                        8192,
                        &sha,
                        "started",
                    )
                    .unwrap();
                }
                _ => std::fs::write(&path, "{broken").unwrap(),
            }
            let mut d = FastbootDevice::new(
                FakeTransport::new(vec![Frame::Ok("A"), Frame::Ok("no"), Frame::Ok("yes")]),
                Box::new(|_| {}),
            );
            assert!(lock_verified_device(&mut d, "A", dir.path(), "init_boot", &sha).is_err());
            assert!(!d.into_transport().sent_cmds.iter().any(|s| s == "oem lock"));
        }
    }

    #[test]
    fn relock_does_not_report_success_from_okay_without_explicit_locked_state() {
        use protocol::fake::{FakeTransport, Frame};
        let dir = tempfile::tempdir().unwrap();
        let sha = "a".repeat(64);
        write_stock_history(dir.path(), "A", &sha);
        for last in [Frame::Ok("yes"), Frame::Ok(""), Frame::Fail("unsupported")] {
            let mut d = FastbootDevice::new(
                FakeTransport::new(vec![
                    Frame::Ok("A"),
                    Frame::Ok("no"),
                    Frame::Ok("yes"),
                    Frame::Ok("yes"),
                    Frame::Ok(""),
                    last,
                ]),
                Box::new(|_| {}),
            );
            assert!(lock_verified_device(&mut d, "A", dir.path(), "init_boot", &sha).is_err());
            assert!(d.into_transport().sent_cmds.iter().any(|s| s == "oem lock"));
        }
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
