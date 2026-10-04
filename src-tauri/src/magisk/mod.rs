//! 루팅 엔진 (M4) — Magisk 자동 패치. 설계 .plans/04-engine/root.md
//! 사용자 승인 2026-10-04: 실전 코드 작성 허용, 이 구현의 실기기 테스트 금지(FakeADBDevice 단위 테스트).
//! 게이트: 기기에 쓰는 명령(patch·install·reboot)은 Cargo feature `root-write` + 프론트 REAL_STEPS.root 이중.
//! 기록(fastboot)은 기존 fastboot_flash(fastboot-write 게이트)을 재사용한다.

pub mod patch;

use crate::adb;
use crate::app_paths;
use patch::PatchOutcome;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::PathBuf;
use tauri::Emitter;

const GH_LATEST: &str = "https://api.github.com/repos/topjohnwu/Magisk/releases/latest";

fn ensure_root_write() -> Result<(), String> {
    if cfg!(feature = "root-write") {
        Ok(())
    } else {
        Err("루팅 실행 명령은 이 빌드에서 비활성화되어 있습니다(실기기 검증 대기 — root-write)".into())
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MagiskPrepareOut {
    pub version: String,
    pub apk_path: String,
    pub sha256: String,
}

fn gh_get_json(url: &str) -> Result<serde_json::Value, String> {
    let resp = ureq::get(url)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("릴리스 조회 실패: {e}"))?;
    let text = resp.into_string().map_err(|e| format!("응답 수신 실패: {e}"))?;
    serde_json::from_str(&text).map_err(|e| format!("릴리스 정보 해석 실패: {e}"))
}

/// GitHub 릴리스 JSON에서 `Magisk-v*.apk` 자산 선택 (디버그·기타 자산 제외)
fn pick_asset(json: &serde_json::Value) -> Result<(String, String, String), String> {
    let tag = json["tag_name"]
        .as_str()
        .ok_or("릴리스 태그를 찾을 수 없습니다")?
        .to_string();
    let assets = json["assets"]
        .as_array()
        .ok_or("릴리스 자산 목록이 없습니다")?;
    for a in assets {
        let Some(name) = a["name"].as_str() else { continue };
        let Some(url) = a["browser_download_url"].as_str() else { continue };
        if name.starts_with("Magisk-") && name.ends_with(".apk") {
            return Ok((tag, name.to_string(), url.to_string()));
        }
    }
    Err("Magisk APK 자산을 찾을 수 없습니다 (릴리스 형식 변경 가능)".into())
}

/// Magisk 최신 APK 확보 — 캐시 재사용(D12 패턴). 기기 무관·준비 단계(게이트 밖)
#[tauri::command]
pub async fn magisk_prepare() -> Result<MagiskPrepareOut, String> {
    let json = gh_get_json(GH_LATEST)?;
    let (tag, name, url) = pick_asset(&json)?;
    let dir = app_paths::data_dir()
        .map(|d| d.join("magisk"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    let path = dir.join(&name);
    if !path.exists() {
        let mut reader = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(300))
            .call()
            .map_err(|e| format!("APK 다운로드 실패: {e}"))?
            .into_reader();
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|e| format!("APK 수신 실패: {e}"))?;
        std::fs::write(&path, &bytes).map_err(|e| format!("APK 저장 실패: {e}"))?;
    }
    let data = std::fs::read(&path).map_err(|e| format!("APK 읽기 실패: {e}"))?;
    Ok(MagiskPrepareOut {
        version: tag,
        apk_path: path.to_string_lossy().to_string(),
        sha256: hex::encode(Sha256::digest(&data)),
    })
}

/// 부트 패치 — 스테이징 → boot_patch.sh → 검증 → pull → 정리 (전 과정 adb)
#[tauri::command]
pub async fn magisk_patch(
    app: tauri::AppHandle,
    serial: Option<String>,
    apk_path: String,
    image_path: String,
) -> Result<PatchOutcome, String> {
    ensure_root_write()?;
    let apk = PathBuf::from(&apk_path);
    let image = PathBuf::from(&image_path);
    if !apk.is_file() {
        return Err("Magisk APK 경로가 올바르지 않습니다".into());
    }
    if !image.is_file() {
        return Err("순정 부트 이미지 경로가 올바르지 않습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요".into());
    }
    // 결과는 항상 앱 데이터 폴더(쓰기 보장) — <원본이름>-patched.img
    let stem = image.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let out = app_paths::data_dir()
        .map(|d| d.join("magisk").join(format!("{stem}-patched.img")))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let work = move || {
        adb::with_first_device(&serial, move |dev| {
            let mut outcome_logs: Vec<String> = vec![];
            let r = patch::run_patch(dev, &apk, &image, &out, &mut |line| {
                outcome_logs.push(line.clone());
                let _ = app.emit("magisk:log", line);
            });
            r.map(|mut o| {
                o.log = outcome_logs;
                o
            })
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("패치 스레드 오류: {e}")),
    }
}

/// Magisk 앱 설치 (adb install)
#[tauri::command]
pub async fn magisk_install(serial: Option<String>, apk_path: String) -> Result<(), String> {
    ensure_root_write()?;
    let apk = PathBuf::from(&apk_path);
    if !apk.is_file() {
        return Err("Magisk APK 경로가 올바르지 않습니다".into());
    }
    let work = move || {
        adb::with_first_device(&serial, move |dev| {
            dev.install(&apk, None).map_err(|e| format!("Magisk 앱 설치 실패: {e}"))
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("설치 스레드 오류: {e}")),
    }
}

/// adb 재부팅 — fastboot_reboot의 adb 짝 (fastboot 진입/복귀)
#[tauri::command]
pub async fn root_reboot(serial: Option<String>, target: String) -> Result<(), String> {
    ensure_root_write()?;
    let reboot = match target.as_str() {
        "os" => adb_client::RebootType::System,
        "bootloader" => adb_client::RebootType::Bootloader,
        other => return Err(format!("알 수 없는 재부팅 대상: {other}")),
    };
    let work = move || {
        adb::with_first_device(&serial, move |dev| {
            dev.reboot(reboot).map_err(|e| format!("재부팅 요청 실패: {e}"))
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("재부팅 스레드 오류: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_picking_filters_magisk_apk() {
        let json = serde_json::json!({
            "tag_name": "v30.7",
            "assets": [
                {"name": "Magisk-v30.7.apk", "browser_download_url": "https://x/Magisk-v30.7.apk"},
                {"name": "magisk-debug-xxxx.zip", "browser_download_url": "https://x/d.zip"},
                {"name": "Source code (zip)", "browser_download_url": "https://x/src.zip"},
                {"name": "another-lib.apk", "browser_download_url": "https://x/lib.apk"}
            ]
        });
        let (tag, name, url) = pick_asset(&json).unwrap();
        assert_eq!(tag, "v30.7");
        assert_eq!(name, "Magisk-v30.7.apk");
        assert!(url.ends_with("Magisk-v30.7.apk"));
    }

    #[test]
    fn asset_picking_requires_official_name() {
        let json = serde_json::json!({
            "tag_name": "v99",
            "assets": [{"name": "source.zip", "browser_download_url": "https://x/s.zip"}]
        });
        assert!(pick_asset(&json).is_err());
    }
}
