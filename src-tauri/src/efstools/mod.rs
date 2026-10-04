//! EFS 래퍼 엔진 (M5 1단계) — EfsTools 서브프로세스 호출(newflasher M6 패턴).
//! 사용자 승인 2026-10-04: 실전 코드 작성 허용, 실기기 테스트 금지(파서·검증 단위 테스트).
//! 원본 절차(efs.py) 계승: su -c setprop diag 전환 → 슬롯별 2회 업로드 → 전수 리드백 검증.
//! 게이트: Cargo feature `efs-write` + 프론트 REAL_STEPS.efs — efs_tool_check만 읽기 전용(게이트 밖).
//! 조사 근거: ../tasks/research-efstools-integration.md, 설계: .plans/04-engine/efstools-wrapper.md

pub mod runner;
pub mod verify;

use crate::adb;
use crate::app_paths;
use runner::{run_args, resolve_tool_dir, EFS_CANCEL};
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use tauri::Emitter;

fn ensure_efs_write() -> Result<(), String> {
    if cfg!(feature = "efs-write") {
        Ok(())
    } else {
        Err("EFS 실행 명령은 이 빌드에서 비활성화되어 있습니다(실기기 검증 대기 — efs-write)".into())
    }
}

/// 원본 efs.py 계승 — DIAG 포트 개방 setprop (루트 필요)
const DIAG_SETPROP: &str = "su -c setprop sys.usb.config diag,diag_mdm,diag_mdm2,qdss,qdss_mdm,serial_cdev,dpl,rmnet,adb";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCheck {
    pub version: String,
    pub path: String,
}

/// 도구 확인 — `version` 실행(기기 무접촉). 게이트 밖(env_check 성격)
#[tauri::command]
pub async fn efs_tool_check() -> Result<ToolCheck, String> {
    let dir = resolve_tool_dir()?;
    let mut version = String::new();
    let out = run_args(&dir, &["version"], &mut |l| version.push_str(l))?;
    if !out.errors.is_empty() {
        return Err(format!("EfsTools 확인 실패: {}", out.errors.join(" / ")));
    }
    Ok(ToolCheck {
        version: version.trim().to_string(),
        path: dir.join("EfsTools.exe").to_string_lossy().to_string(),
    })
}

/// DIAG 전환 — 원본 efsPortOpen 계승. 폰이 USB 재열거되는 동안 adb가 잠시 끊길 수 있다
#[tauri::command]
pub async fn efs_diag_open(serial: Option<String>) -> Result<(), String> {
    ensure_efs_write()?;
    crate::tasks::blocking("DIAG 전환", move || {
        adb::with_first_device(&serial, |dev| {
            crate::device_io::shell(dev, DIAG_SETPROP)
                .map(|_| ())
                .map_err(|e| format!("DIAG 포트 전환 실패(루트 승인 확인): {e}"))
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreflightOut {
    pub log: Vec<String>,
    pub errors: Vec<String>,
}

/// 연결 안정성 검사 — targetInfo + efsInfo (DIAG COM 개방·기기·EFS 정보)
#[tauri::command]
pub async fn efs_preflight(app: tauri::AppHandle) -> Result<PreflightOut, String> {
    ensure_efs_write()?;
    let dir = resolve_tool_dir()?;
    EFS_CANCEL.store(false, Ordering::Relaxed);
    crate::tasks::blocking("EFS 사전 점검", move || {
        let mut all: Vec<String> = vec![];
        let mut errors: Vec<String> = vec![];
        for cmd in ["targetInfo", "efsInfo"] {
            let out = run_args(&dir, &[cmd], &mut |line| {
                let _ = app.emit("efs:log", serde_json::json!({ "cmd": cmd, "line": line }));
            })?;
            all.extend(out.lines.iter().cloned());
            errors.extend(out.errors);
        }
        Ok(PreflightOut { log: all, errors })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOut {
    pub errors: Vec<String>,
    pub files_seen: u32,
}

/// 업로드 1회 — uploadDirectory -i <presetDir> -o / -v.
/// 슬롯별 2회는 호출부(wizard)가 담당하고, 성공 판정은 efs_verify만이 내린다.
#[tauri::command]
pub async fn efs_upload(app: tauri::AppHandle, preset_dir: String) -> Result<UploadOut, String> {
    ensure_efs_write()?;
    let dir = resolve_tool_dir()?;
    let preset = PathBuf::from(&preset_dir);
    if !preset.is_dir() {
        return Err("프리셋 폴더를 찾을 수 없습니다".into());
    }
    EFS_CANCEL.store(false, Ordering::Relaxed);
    crate::tasks::blocking("EFS 업로드", move || {
        let input = preset.to_string_lossy().to_string();
        let out = run_args(
            &dir,
            &["uploadDirectory", "-i", input.as_str(), "-o", "/", "-v"],
            &mut |line| {
                let _ = app.emit("efs:log", serde_json::json!({ "cmd": "uploadDirectory", "line": line }));
            },
        )?;
        if out.killed {
            return Err("사용자가 EFS 업로드를 취소했습니다".into());
        }
        // 파일 라인(올림/확인) 개수 — 참고용(판정 아님)
        let files_seen = out.lines.iter().filter(|l| l.contains("file")).count() as u32;
        Ok(UploadOut { errors: out.errors, files_seen })
    })
    .await
}

/// 전수 리드백 검증 — 프리셋 최상위 폴더별 downloadDirectory 후 SHA-256 전수 비교.
/// 업로드 성공의 유일한 최종 근거(계약: 두 번 썼다는 것만으로 성공 판정하지 않음).
#[tauri::command]
pub async fn efs_verify(
    app: tauri::AppHandle,
    preset_dir: String,
) -> Result<verify::VerifyReport, String> {
    ensure_efs_write()?;
    let dir = resolve_tool_dir()?;
    let preset = PathBuf::from(&preset_dir);
    if !preset.is_dir() {
        return Err("프리셋 폴더를 찾을 수 없습니다".into());
    }
    EFS_CANCEL.store(false, Ordering::Relaxed);
    crate::tasks::blocking("EFS 검증", move || {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let tmp = app_paths::data_dir()
            .map(|d| d.join("efs-verify").join(format!("{nonce}")))
            .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
        std::fs::create_dir_all(&tmp).map_err(|e| format!("검증 폴더 생성 실패: {e}"))?;

        let result = (|| -> Result<verify::VerifyReport, String> {
            let tops = verify::top_level_dirs(&preset);
            if tops.is_empty() {
                return Err("프리셋에 하위 폴더가 없습니다 — 구조를 확인해 주세요".into());
            }
            let mut download_errors: Vec<String> = vec![];
            for top in &tops {
                // EFS 쪽 경로는 /<top>, 받는 쪽은 tmp/<top> — 프리셋 구조와 동일하게
                let efs_path = format!("/{top}");
                let local = tmp.join(top).to_string_lossy().to_string();
                let out = run_args(
                    &dir,
                    &["downloadDirectory", "-i", efs_path.as_str(), "-o", local.as_str(), "-v"],
                    &mut |line| {
                        let _ = app.emit("efs:log", serde_json::json!({ "cmd": "downloadDirectory", "line": line }));
                    },
                )?;
                if out.killed {
                    return Err("사용자가 EFS 검증을 취소했습니다".into());
                }
                download_errors.extend(out.errors);
            }
            let mut report = verify::compare(&preset, &tmp);
            if !download_errors.is_empty() {
                report.ok = false;
                report.mismatches.extend(download_errors);
            }
            Ok(report)
        })();
        let _ = std::fs::remove_dir_all(&tmp); // 임시 정리(고정 앱 데이터 하위)
        result
    })
    .await
}

/// before-image 스냅샷 — downloadDirectory -i / -o <dest> (롤백용 원본 전체, 오류 허용형)
#[tauri::command]
pub async fn efs_snapshot(
    app: tauri::AppHandle,
    dest: String,
) -> Result<UploadOut, String> {
    ensure_efs_write()?;
    let dir = resolve_tool_dir()?;
    let dest_path = PathBuf::from(&dest);
    if dest_path.exists() && dest_path.read_dir().map(|mut r| r.next().is_some()).unwrap_or(false) {
        return Err("스냅샷 대상 폴더가 비어 있지 않습니다 — 빈 폴더를 지정해 주세요".into());
    }
    EFS_CANCEL.store(false, Ordering::Relaxed);
    crate::tasks::blocking("EFS 스냅샷", move || {
        std::fs::create_dir_all(&dest_path).map_err(|e| format!("스냅샷 폴더 생성 실패: {e}"))?;
        let local = dest_path.to_string_lossy().to_string();
        let out = run_args(
            &dir,
            &["downloadDirectory", "-i", "/", "-o", local.as_str(), "-v"],
            &mut |line| {
                let _ = app.emit("efs:log", serde_json::json!({ "cmd": "snapshot", "line": line }));
            },
        )?;
        if out.killed {
            return Err("사용자가 EFS 스냅샷을 취소했습니다".into());
        }
        Ok(UploadOut { errors: out.errors, files_seen: 0 })
    })
    .await
}

/// 진행 중 EfsTools 프로세스 취소
#[tauri::command]
pub async fn efs_cancel() -> Result<(), String> {
    ensure_efs_write()?;
    EFS_CANCEL.store(true, Ordering::Relaxed);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_setprop_matches_original_cli() {
        // 원본 efs.py efsPortOpen과 동일한 문자열인지 — 계승 검증
        assert!(DIAG_SETPROP.starts_with("su -c setprop sys.usb.config diag,"));
        assert!(DIAG_SETPROP.contains("rmnet,adb"));
        assert!(!DIAG_SETPROP.contains(';')); // 셸 보간·체인 없음
    }
}
