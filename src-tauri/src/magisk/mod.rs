//! 루팅 엔진 (M4) — Magisk 자동 패치. 설계 .plans/04-engine/root.md
//! 사용자 승인 2026-10-04: 실전 코드 작성 허용, 이 구현의 실기기 테스트 금지(FakeADBDevice 단위 테스트).
//! 게이트: 기기에 쓰는 명령(patch·install·reboot)은 Cargo feature `root-write` + 프론트 REAL_STEPS.root 이중.
//! 기록(fastboot)은 기존 fastboot_flash(fastboot-write 게이트)을 재사용한다.

pub mod patch;

use crate::adb;
use crate::app_paths;
use patch::PatchOutcome;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;
use tauri::Emitter;

const GH_LATEST: &str = "https://api.github.com/repos/topjohnwu/Magisk/releases/latest";

fn ensure_root_write() -> Result<(), String> {
    if cfg!(feature = "root-write") {
        Ok(())
    } else {
        Err(
            "루팅 실행 명령은 이 빌드에서 비활성화되어 있습니다(실기기 검증 대기 — root-write)"
                .into(),
        )
    }
}

fn require_serial(serial: &Option<String>) -> Result<(), String> {
    if serial
        .as_deref()
        .is_none_or(|value| value.trim().is_empty())
    {
        return Err("작업 대상 기기 식별값이 필요합니다".into());
    }
    Ok(())
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
    let mut bytes = vec![];
    resp.into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("응답 수신 실패: {e}"))?;
    if bytes.len() > 1024 * 1024 {
        return Err("릴리스 응답이 너무 큽니다".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("릴리스 정보 해석 실패: {e}"))
}

/// GitHub 릴리스 JSON에서 `Magisk-v*.apk` 자산 선택 (디버그·기타 자산 제외)
fn pick_asset(json: &serde_json::Value) -> Result<(String, String, String), String> {
    let tag = json["tag_name"]
        .as_str()
        .ok_or("릴리스 태그를 찾을 수 없습니다")?
        .to_string();
    if tag.is_empty()
        || tag.len() > 128
        || !tag
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("Magisk 릴리스 태그 형식이 올바르지 않습니다".into());
    }
    let assets = json["assets"]
        .as_array()
        .ok_or("릴리스 자산 목록이 없습니다")?;
    for a in assets {
        let Some(name) = a["name"].as_str() else {
            continue;
        };
        let Some(url) = a["browser_download_url"].as_str() else {
            continue;
        };
        if name.starts_with("Magisk-v")
            && name.ends_with(".apk")
            && name.len() <= 128
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
            && url == format!("https://github.com/topjohnwu/Magisk/releases/download/{tag}/{name}")
        {
            return Ok((tag, name.to_string(), url.to_string()));
        }
    }
    Err("Magisk APK 자산을 찾을 수 없습니다 (릴리스 형식 변경 가능)".into())
}

/// Magisk 최신 APK 확보 — 캐시 재사용(D12 패턴). 기기 무관·준비 단계(게이트 밖).
/// 다운로드는 임시 파일에 받은 뒤 원자 교체 — 중단 시 반쪽 APK가 캐시로 오인되지 않게.
#[tauri::command]
pub async fn magisk_prepare() -> Result<MagiskPrepareOut, String> {
    crate::tasks::blocking("Magisk 다운로드", prepare_work).await
}

fn prepare_work() -> Result<MagiskPrepareOut, String> {
    static PREPARING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _preparing = PREPARING
        .try_lock()
        .map_err(|_| "Magisk 다운로드가 이미 진행 중입니다")?;
    /// 정상 Magisk APK는 ~12MiB — 이보다 크면 비정상으로 본다(디스크 채우기 방지)
    const MAX_APK: u64 = 256 * 1024 * 1024;
    let json = gh_get_json(GH_LATEST)?;
    let (tag, name, url) = pick_asset(&json)?;
    let dir = app_paths::data_dir()
        .map(|d| d.join("magisk"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    let path = dir.join(&name);
    if validate_apk(&path).is_err() {
        let mut reader = ureq::get(&url)
            .timeout(std::time::Duration::from_secs(300))
            .call()
            .map_err(|e| format!("APK 다운로드 실패: {e}"))?
            .into_reader()
            .take(MAX_APK + 1);
        let mut bytes = Vec::new();
        reader
            .read_to_end(&mut bytes)
            .map_err(|e| format!("APK 수신 실패: {e}"))?;
        if bytes.len() as u64 > MAX_APK {
            return Err("다운로드한 APK가 비정상적으로 큽니다(256 MiB 초과)".into());
        }
        crate::storage::atomic_write(&path, &bytes)?;
    }
    validate_apk(&path)?;
    let (sha256, _) =
        crate::storage::hash_reader(std::fs::File::open(&path).map_err(|e| e.to_string())?)?;
    Ok(MagiskPrepareOut {
        version: tag,
        apk_path: path.to_string_lossy().to_string(),
        sha256,
    })
}

fn verify_apk(path: &std::path::Path, expected: &str) -> Result<(), String> {
    validate_apk(path)?;
    let file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let (actual, _) = crate::storage::hash_reader(file.take(256 * 1024 * 1024 + 1))?;
    if actual != expected {
        return Err("Magisk APK가 사전 준비 후 변경됐습니다 — 해시가 일치하지 않습니다".into());
    }
    Ok(())
}

fn validate_apk(path: &std::path::Path) -> Result<(), String> {
    let mut zip = crate::firmware::LocalZip::open(path)?;
    let names: Vec<_> = patch::APK_ENTRIES.iter().map(|(name, _)| *name).collect();
    crate::firmware::zip_extract_named(&mut zip, &names)?;
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MagiskPatchRequest {
    serial: String,
    apk_path: String,
    image_path: String,
    partition: String,
    image_sha256: String,
    fingerprint: String,
    apk_sha256: String,
}

/// 부트 패치 — 스테이징 → boot_patch.sh → 검증 → pull → 정리 (전 과정 adb).
/// partition은 결과 파일명에만 쓴다(기기 측 이미지명은 고정) — boot|init_boot 등 계약 파티션명.
#[tauri::command]
pub async fn magisk_patch(
    app: tauri::AppHandle,
    request: MagiskPatchRequest,
) -> Result<PatchOutcome, String> {
    let MagiskPatchRequest {
        serial,
        apk_path,
        image_path,
        partition,
        image_sha256,
        fingerprint,
        apk_sha256,
    } = request;
    let serial = Some(serial);
    ensure_root_write()?;
    require_serial(&serial)?;
    let apk = PathBuf::from(&apk_path);
    let image = PathBuf::from(&image_path);
    if !apk.is_file() {
        return Err("Magisk APK 경로가 올바르지 않습니다".into());
    }
    if !image.is_file() {
        return Err(
            "순정 부트 이미지 경로가 올바르지 않습니다 — 사전 준비에서 펌웨어를 먼저 받아 주세요"
                .into(),
        );
    }
    if !matches!(partition.as_str(), "boot" | "init_boot") {
        return Err("부트 파티션 이름이 올바르지 않습니다".into());
    }
    // 결과는 항상 앱 데이터 폴더(쓰기 보장) — 파티션명 기반 고정 경로(사용자 파일명 미사용)
    let out = app_paths::data_dir()
        .map(|d| d.join("magisk").join(format!("{partition}-patched.img")))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    let work = move || {
        let _operation = operation;
        adb::with_first_device(&serial, move |dev| {
            verify_apk(&apk, &apk_sha256)?;
            crate::boot_image::verify_fingerprint(dev, &fingerprint)?;
            let mut outcome_logs: Vec<String> = vec![];
            let r = patch::run_patch(dev, &apk, &image, &out, Some(&image_sha256), &mut |line| {
                outcome_logs.push(line.clone());
                let _ = app.emit("magisk:log", line);
            });
            r.map(|mut o| {
                o.log = outcome_logs;
                o
            })
        })
    };
    crate::tasks::blocking("Magisk 패치", work).await
}

/// Magisk 앱 설치 (adb install)
#[tauri::command]
pub async fn magisk_install(
    serial: Option<String>,
    apk_path: String,
    apk_sha256: String,
) -> Result<(), String> {
    ensure_root_write()?;
    require_serial(&serial)?;
    let apk = PathBuf::from(&apk_path);
    if !apk.is_file() {
        return Err("Magisk APK 경로가 올바르지 않습니다".into());
    }
    let operation = crate::device_io::WriteOperation::acquire()?;
    let work = move || {
        let _operation = operation;
        verify_apk(&apk, &apk_sha256)?;
        adb::with_first_device(&serial, move |dev| {
            dev.install(&apk, None)
                .map_err(|e| format!("Magisk 앱 설치 실패: {e}"))
        })
    };
    crate::tasks::blocking("Magisk 설치", work).await
}

/// adb 재부팅 — fastboot_reboot의 adb 짝 (fastboot 진입/복귀)
#[tauri::command]
pub async fn root_reboot(serial: Option<String>, target: String) -> Result<(), String> {
    if !cfg!(feature = "root-write") && !cfg!(feature = "fastboot-write") {
        return Err("기기 재부팅은 이 빌드에서 비활성화되어 있습니다".into());
    }
    require_serial(&serial)?;
    let reboot = match target.as_str() {
        "os" => adb_client::RebootType::System,
        "bootloader" => adb_client::RebootType::Bootloader,
        other => return Err(format!("알 수 없는 재부팅 대상: {other}")),
    };
    let operation = crate::device_io::WriteOperation::acquire()?;
    let work = move || {
        let _operation = operation;
        adb::with_first_device(&serial, move |dev| {
            dev.reboot(reboot)
                .map_err(|e| format!("재부팅 요청 실패: {e}"))
        })
    };
    crate::tasks::blocking("기기 재부팅", work).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_picking_filters_magisk_apk() {
        let json = serde_json::json!({
            "tag_name": "v30.7",
            "assets": [
                {"name": "Magisk-v30.7.apk", "browser_download_url": "https://github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk"},
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
    fn cache_integrity_and_explicit_identity_are_required() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("Magisk.apk");
        let bytes = crate::firmware::tests::build_zip(
            &patch::APK_ENTRIES
                .iter()
                .map(|(name, _)| (*name, name.as_bytes()))
                .collect::<Vec<_>>(),
        );
        std::fs::write(&path, &bytes).unwrap();
        assert!(verify_apk(&path, &crate::boot_image::sha256(&bytes)).is_ok());
        assert!(verify_apk(&path, &"f".repeat(64)).is_err());
        std::fs::write(&path, b"broken").unwrap();
        assert!(validate_apk(&path).is_err());
        assert!(require_serial(&None).is_err());
        assert!(require_serial(&Some(" ".into())).is_err());
    }

    #[test]
    fn asset_picking_rejects_paths_and_untrusted_download_origins() {
        for (tag, name, url) in [
            (
                "../v30.7",
                "Magisk-v30.7.apk",
                "https://github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk",
            ),
            (
                "v30.7",
                "Magisk-v30.7/../../file.apk",
                "https://github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk",
            ),
            ("v30.7", "Magisk-v30.7.apk", "https://example.com/other.apk"),
        ] {
            assert!(pick_asset(&serde_json::json!({"tag_name": tag, "assets": [{"name": name, "browser_download_url": url}]})).is_err());
        }
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
