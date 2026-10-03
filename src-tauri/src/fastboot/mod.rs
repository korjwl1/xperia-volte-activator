//! fastboot 엔진 (M2) — plan.md §2·§9-3·§10-2, 설계 .plans/04-engine/fastboot.md
//! 사용자 승인 2026-10-03: 실전 코드 작성 허용, 실기기 테스트 금지(FakeTransport 단위 테스트).
//! 파괴 명령(unlock/lock/flash)은 confirm 인자 필수 — 프론트 REAL_STEPS.fastboot 기본 꺼짐.

pub mod protocol;
pub mod transport;

use protocol::FastbootDevice;
use serde::Serialize;
use std::path::PathBuf;
use tauri::Emitter;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnlockResult {
    pub unlocked: bool,
}

/// 로그 방출기 — 'fastboot:log' 이벤트 (민감값 마스킹은 호출부 책임)
struct LogEmitter {
    app: tauri::AppHandle,
    secret: Option<String>,
}

impl LogEmitter {
    fn emit(&self, line: String) {
        let line = match &self.secret {
            Some(s) if s.len() >= 8 => {
                let head: String = s.chars().take(4).collect();
                let tail: String = s.chars().skip(s.len() - 4).collect();
                line.replace(s.as_str(), &format!("{head}…{tail}"))
            }
            _ => line,
        };
        let _ = self.app.emit("fastboot:log", line);
    }
    fn boxed(self) -> Box<dyn FnMut(String) + Send> {
        Box::new(move |l| self.emit(l))
    }
}

fn with_device<T>(
    app: tauri::AppHandle,
    secret: Option<String>,
    work: impl FnOnce(FastbootDevice<transport::RusbTransport>) -> Result<T, String> + Send + 'static,
) -> Result<T, String>
where
    T: Send + 'static,
{
    let t = transport::RusbTransport::open()?;
    let emitter = LogEmitter { app, secret };
    let dev = FastbootDevice::new(t, emitter.boxed());
    work(dev)
}

/// getvar:all — 읽기 전용 프로브 (§10-2 게이트의 입력: unlocked·슬롯·헬스)
#[tauri::command]
pub async fn fastboot_getvar(app: tauri::AppHandle) -> Result<std::collections::HashMap<String, String>, String> {
    let work = move || with_device(app, None, |mut d| d.getvar_all());
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("프로브 스레드 오류: {e}"))?
}

/// 부트로더 언락 — `oem unlock 0x{code}` 후 getvar로 이중 확인
#[tauri::command]
pub async fn fastboot_unlock(
    app: tauri::AppHandle,
    code: String,
    confirm: bool,
) -> Result<UnlockResult, String> {
    if !confirm {
        return Err("확인(위험 배지·확인 게이트) 없이는 실행하지 않습니다".into());
    }
    // 0x 접두 제거(프론트 normalizedUnlockCode와 같은 규칙 — 이중 방어)
    let code = code.trim().trim_start_matches("0x").trim().to_string();
    let secret = code.clone();
    let work = move || {
        with_device(app, Some(secret), move |mut d| {
            d.oem_unlock(&code)?;
            // 결과 확인 — OKAY여도 unlocked=yes가 나와야 성공
            let unlocked = d
                .getvar("unlocked")
                .ok()
                .flatten()
                .map(|v| v.eq_ignore_ascii_case("yes"))
                .unwrap_or(false);
            Ok(UnlockResult { unlocked })
        })
    };
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("언락 스레드 오류: {e}"))?
}

/// 부트로더 리락 — `oem lock` 후 확인
#[tauri::command]
pub async fn fastboot_lock(app: tauri::AppHandle, confirm: bool) -> Result<UnlockResult, String> {
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    let work = move || {
        with_device(app, None, |mut d| {
            d.oem_lock()?;
            let unlocked = d
                .getvar("unlocked")
                .ok()
                .flatten()
                .map(|v| v.eq_ignore_ascii_case("yes"))
                .unwrap_or(false);
            Ok(UnlockResult { unlocked })
        })
    };
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("리락 스레드 오류: {e}"))?
}

/// 파티션 기록 — download → flash <partition>_a/_b (원본 CLI fastbootFlash 계승: 양쪽 슬롯)
/// 플래시 이력을 앱 데이터 폴더 flash-history.json에 기록(§3-3 리락 게이트의 입력 — 판정은 M4 후 완성)
#[tauri::command]
pub async fn fastboot_flash(
    app: tauri::AppHandle,
    partition: String,
    path: String,
    confirm: bool,
) -> Result<(), String> {
    if !confirm {
        return Err("확인 없이는 실행하지 않습니다".into());
    }
    let image = std::fs::read(PathBuf::from(&path))
        .map_err(|e| format!("이미지 파일 읽기 실패({path}): {e}"))?;
    let part_base = partition.clone();
    let img_path = path.clone();
    let work = move || {
        with_device(app, None, move |mut d| {
            for slot in ["_a", "_b"] {
                let part = format!("{part_base}{slot}");
                d.flash(&part, &image)?;
                record_flash_history(&part, &img_path, image.len() as u64);
            }
            Ok(())
        })
    };
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("기록 스레드 오류: {e}"))?
}

#[tauri::command]
pub async fn fastboot_reboot(app: tauri::AppHandle, target: String) -> Result<(), String> {
    let work = move || with_device(app, None, move |mut d| d.reboot(&target));
    tauri::async_runtime::spawn_blocking(work).await.map_err(|e| format!("재부팅 스레드 오류: {e}"))?
}

/// 플래시 이력 — 파티션×이미지 경로·크기·시각 (리락 게이트 §3-3 ②의 입력)
fn record_flash_history(partition: &str, path: &str, bytes: u64) {
    use std::io::Write;
    let Some(dir) = crate::adb::app_data_dir() else { return };
    let _ = std::fs::create_dir_all(&dir);
    let file = dir.join("flash-history.jsonl");
    let mut entry = serde_json::json!({
        "partition": partition,
        "image": path,
        "bytes": bytes,
        "at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    });
    // 이미지 해시 — 리락 게이트의 "검증된 순정 이미지" 판정 재료
    use sha2::{Digest, Sha256};
    if let Ok(data) = std::fs::read(path) {
        entry["sha256"] = hex::encode(Sha256::digest(&data)).into();
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(file) {
        let _ = writeln!(f, "{entry}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlock_code_prefix_stripped_in_command_layer() {
        // command 레이어에서 "0x" 제거 후 프로토콜에 전달 — 형식 검증은 프로토콜이 담당
        let raw = "0x1234567890abcdef".to_string();
        let stripped = raw.trim().trim_start_matches("0x").trim().to_string();
        assert_eq!(stripped, "1234567890abcdef");
        assert!(protocol::validate_unlock_code(&stripped).is_ok());
    }
}
