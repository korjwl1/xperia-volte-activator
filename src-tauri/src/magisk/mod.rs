//! 루팅 엔진 (M4) — Magisk 자동 패치. 설계 .plans/04-engine/root.md
//! 사용자 승인 2026-10-04: 실전 코드 작성 허용, 이 구현의 실기기 테스트 금지(FakeADBDevice 단위 테스트).
//! 게이트: 기기에 쓰는 명령(patch·install·reboot)은 Cargo feature `root-write` + 프론트 REAL_STEPS.root 이중.
//! 기록(fastboot)은 기존 fastboot_flash(fastboot-write 게이트)을 재사용한다.

pub mod patch;

use crate::adb;
use crate::apk_verify::{self, ReleaseAsset};
use crate::app_paths;
use crate::events::Events;
use patch::PatchOutcome;
use serde::{Deserialize, Serialize};
use std::io::Read;
use std::path::PathBuf;

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

/// 정상 Magisk APK는 ~12MiB — 이보다 크면 비정상으로 본다(디스크·메모리 채우기 방지)
const MAX_APK: u64 = 256 * 1024 * 1024;

/// GitHub 릴리스 JSON에서 `Magisk-v*.apk` 자산 선택 (디버그·기타 자산 제외).
/// 자산의 SHA-256 다이제스트·크기가 없으면 검증할 수 없으므로 실패한다.
fn pick_asset(json: &serde_json::Value) -> Result<(String, ReleaseAsset), String> {
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
        if is_magisk_apk_name(name)
            && url == format!("https://github.com/topjohnwu/Magisk/releases/download/{tag}/{name}")
        {
            let asset = apk_verify::asset_from_json(a, name, url, MAX_APK)?;
            return Ok((tag, asset));
        }
    }
    Err("Magisk APK 자산을 찾을 수 없습니다 (릴리스 형식 변경 가능)".into())
}

fn is_magisk_apk_name(name: &str) -> bool {
    name.starts_with("Magisk-v")
        && name.ends_with(".apk")
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// Magisk 최신 APK 확보 — 캐시 재사용(D12 패턴). 기기 무관·준비 단계(게이트 밖).
/// 받은 바이트를 GitHub 다이제스트·서명 인증서 핀으로 검증한 뒤에만 캐시에 원자 저장한다.
#[tauri::command]
pub async fn magisk_prepare() -> Result<MagiskPrepareOut, String> {
    crate::tasks::blocking("Magisk 다운로드", prepare_work).await
}

fn prepare_work() -> Result<MagiskPrepareOut, String> {
    static PREPARING: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _preparing = PREPARING
        .try_lock()
        .map_err(|_| "Magisk 다운로드가 이미 진행 중입니다")?;
    let dir = app_paths::data_dir()
        .map(|d| d.join("magisk"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    let release = gh_get_json(GH_LATEST).and_then(|json| pick_asset(&json));
    let online = release.and_then(|(tag, asset)| {
        let path = dir.join(&asset.name);
        let (bytes, sha256) = match apk_verify::load_cached(
            &path,
            Some(&asset.sha256),
            apk_verify::MAGISK_CERT_SHA256,
            MAX_APK as usize,
        ) {
            Some(hit) => hit,
            None => {
                let bytes = download(&asset)?;
                // 검증을 모두 통과한 뒤에만 캐시에 남긴다
                apk_verify::verify_download(&bytes, &asset)?;
                apk_verify::check_pins(&bytes, apk_verify::MAGISK_CERT_SHA256)?;
                validate_apk(&bytes)?;
                apk_verify::store_verified(&path, &bytes, &asset.sha256)?;
                (bytes, asset.sha256)
            }
        };
        Ok((tag, path, bytes, sha256))
    });
    prepare_or_cache(&dir, online, apk_verify::MAGISK_CERT_SHA256)
}

type PreparedApk = (String, PathBuf, Vec<u8>, String);
fn prepare_or_cache(
    dir: &std::path::Path,
    online: Result<PreparedApk, String>,
    pins: &[&str],
) -> Result<MagiskPrepareOut, String> {
    let (version, path, bytes, sha256) = match online {
        Ok(prepared) => prepared,
        // 오프라인·요청 한도 초과 — 다이제스트가 기록된 검증 캐시만 쓴다
        Err(online) => {
            let (path, bytes, sha256) =
                apk_verify::newest_verified_cache(dir, is_magisk_apk_name, pins, MAX_APK as usize)
                    .ok_or_else(|| format!("{online} — 검증된 Magisk 캐시도 없습니다"))?;
            let version = path
                .file_name()
                .and_then(|n| n.to_str())
                .and_then(|n| n.strip_prefix("Magisk-")?.strip_suffix(".apk"))
                .unwrap_or_default()
                .to_string();
            eprintln!("[rust] Magisk 다운로드 준비 실패({online}) — 검증된 캐시 {version} 사용");
            (version, path, bytes, sha256)
        }
    };
    validate_apk(&bytes)?;
    Ok(MagiskPrepareOut {
        version,
        apk_path: path.to_string_lossy().to_string(),
        sha256,
    })
}

fn download(asset: &ReleaseAsset) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::with_capacity(asset.size as usize);
    ureq::get(&asset.url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("APK 다운로드 실패: {e}"))?
        .into_reader()
        .take(MAX_APK + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("APK 수신 실패: {e}"))?;
    if bytes.len() as u64 > MAX_APK {
        return Err("다운로드한 APK가 비정상적으로 큽니다(256 MiB 초과)".into());
    }
    Ok(bytes)
}

/// 준비 단계(magisk_prepare)가 GitHub 다이제스트·서명 핀을 확인해 앱 데이터 magisk/ 캐시에 저장한 APK만 쓴다.
/// 호출부가 넘긴 경로·해시만으로는 신뢰하지 않는다(서명 인증서 블록은 복사할 수 있다) — 캐시 폴더 안의
/// Magisk-v*.apk여야 하고, 저장 때 기록한 다이제스트(sidecar)·넘긴 해시·파일 내용·서명 핀이 모두 맞아야 한다.
/// 확인한 바이트를 그대로 돌려줘 패치가 같은 바이트를 쓴다(확인 후 바꿔치기 틈 제거).
fn load_verified_apk(path: &std::path::Path, expected: &str) -> Result<Vec<u8>, String> {
    let dir = app_paths::data_dir()
        .map(|d| d.join("magisk"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    load_verified_apk_in(&dir, path, expected, apk_verify::MAGISK_CERT_SHA256)
}

fn load_verified_apk_in(
    cache_dir: &std::path::Path,
    path: &std::path::Path,
    expected: &str,
    pins: &[&str],
) -> Result<Vec<u8>, String> {
    const PREPARE_AGAIN: &str = "루팅 준비를 다시 진행해 주세요";
    let dir = cache_dir
        .canonicalize()
        .map_err(|_| format!("Magisk 캐시 폴더가 없습니다 — {PREPARE_AGAIN}"))?;
    let file = path
        .canonicalize()
        .map_err(|_| format!("Magisk APK 파일이 없습니다 — {PREPARE_AGAIN}"))?;
    let named = file
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(is_magisk_apk_name);
    if file.parent() != Some(dir.as_path()) || !named {
        return Err(format!(
            "준비 단계에서 받은 Magisk APK가 아닙니다 — {PREPARE_AGAIN}"
        ));
    }
    let (bytes, _) = apk_verify::load_cached(&file, Some(expected), pins, MAX_APK as usize)
        .ok_or_else(|| {
            format!("Magisk APK가 준비 단계에서 확인한 파일과 다릅니다(다이제스트·해시·서명 불일치) — {PREPARE_AGAIN}")
        })?;
    validate_apk(&bytes)?;
    Ok(bytes)
}

fn validate_apk(bytes: &[u8]) -> Result<(), String> {
    patch::extract_payloads(bytes).map(|_| ())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
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
/// partition은 순정 이미지 종류·출처 기록 대조에 쓴다(기기 측 이미지명은 고정). 결과는 앱 데이터 magisk/patched-<sha256>.img.
#[tauri::command]
pub async fn magisk_patch(
    app: tauri::AppHandle,
    request: MagiskPatchRequest,
) -> Result<PatchOutcome, String> {
    magisk_patch_with_events(Events::desktop(app), request).await
}

pub(crate) async fn magisk_patch_with_events(
    app: Events,
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
    crate::boot_image::validate_partition(&partition)?;
    // 결과는 항상 앱 데이터 magisk 폴더(쓰기 보장) — 파일명은 결과 해시(사용자 파일명 미사용)
    let out_dir = app_paths::data_dir()
        .map(|d| d.join("magisk"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    let work = move || {
        let _operation = operation;
        // 기기 연결을 잡기 전에 검증 — 검증한 바이트를 그대로 패치에 쓴다
        let apk_bytes = load_verified_apk(&apk, &apk_sha256)?;
        // 순정 이미지: 대상 파티션 종류와 맞는지, 추출 때 기록한 펌웨어 지문(넘겨받은 지문은 기록과 같아야 함)
        let stock = crate::boot_image::read(&image)?;
        crate::boot_image::check_fits_partition(&stock, &partition)?;
        let origin = crate::boot_image::load_origin(&image, &stock, &fingerprint)?;
        if origin.source_sha256.is_some() || !origin.sha256.eq_ignore_ascii_case(&image_sha256) {
            return Err("패치할 순정 이미지의 출처·해시가 요청과 다릅니다".into());
        }
        if origin.partition != partition {
            return Err("순정 이미지의 파티션이 요청과 다릅니다".into());
        }
        adb::with_first_device(&serial, move |dev| {
            crate::boot_image::verify_fingerprint(dev, &origin.fingerprint)?;
            let key = crate::device_io::identity_key(dev)?;
            crate::root_tools::switch::require_install_stage(
                dev,
                out_dir.parent().ok_or("앱 데이터 폴더 없음")?,
                "magisk",
                &origin.sha256,
            )?;
            let mut outcome_logs: Vec<String> = vec![];
            let r = patch::run_patch(
                dev,
                &apk_bytes,
                &image,
                &out_dir,
                Some(&origin.sha256),
                &mut |line| {
                    outcome_logs.push(line.clone());
                    let _ = app.emit("magisk:log", line);
                },
            );
            r.and_then(|mut o| {
                let path = std::path::Path::new(&o.path);
                let patched = crate::boot_image::read(path)?;
                if crate::boot_image::sha256(&patched) != o.patched_sha256 {
                    return Err("패치 결과가 저장 후 변경됐습니다".into());
                }
                let patched_origin =
                    crate::boot_image::save_patched_origin(path, &patched, &origin)?;
                crate::boot_image::save_device_check(
                    out_dir.parent().ok_or("앱 데이터 폴더 없음")?,
                    &key,
                    &patched_origin,
                )?;
                o.log = outcome_logs;
                Ok(o)
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
        // adb install은 경로로 다시 읽는다 — 직전에 해시·서명 핀을 다시 확인해 틈을 최소화한다.
        // 앱 서명 자체는 설치 때 Android가 검증한다.
        load_verified_apk(&apk, &apk_sha256)?;
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
    if !cfg!(feature = "root-write")
        && !cfg!(feature = "fastboot-write")
        && !(target == "os" && cfg!(feature = "efs-write"))
    {
        return Err("기기 재부팅은 이 빌드에서 비활성화되어 있습니다".into());
    }
    require_serial(&serial)?;
    let reboot = match target.as_str() {
        "os" => adb_client::RebootType::System,
        "bootloader" => adb_client::RebootType::Bootloader,
        // fastbootd — 부트 이미지 기록용(원본 도구 rebootFromAdb("fastboot") 계승)
        "fastboot" => adb_client::RebootType::Fastboot,
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

    fn asset(name: &str, url: &str) -> serde_json::Value {
        serde_json::json!({
            "name": name,
            "browser_download_url": url,
            "digest": format!("sha256:{}", "ab".repeat(32)),
            "size": 1024
        })
    }

    #[test]
    fn asset_picking_filters_magisk_apk() {
        let json = serde_json::json!({
            "tag_name": "v30.7",
            "assets": [
                asset("Magisk-v30.7.apk", "https://github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk"),
                asset("magisk-debug-xxxx.zip", "https://x/d.zip"),
                asset("Source code (zip)", "https://x/src.zip"),
                asset("another-lib.apk", "https://x/lib.apk")
            ]
        });
        let (tag, asset) = pick_asset(&json).unwrap();
        assert_eq!(tag, "v30.7");
        assert_eq!(asset.name, "Magisk-v30.7.apk");
        assert!(asset.url.ends_with("Magisk-v30.7.apk"));
        assert_eq!(asset.sha256, "ab".repeat(32));
        assert_eq!(asset.size, 1024);
    }

    #[test]
    fn asset_without_digest_cannot_be_used() {
        let json = serde_json::json!({
            "tag_name": "v30.7",
            "assets": [{"name": "Magisk-v30.7.apk", "browser_download_url": "https://github.com/topjohnwu/Magisk/releases/download/v30.7/Magisk-v30.7.apk"}]
        });
        assert!(pick_asset(&json).unwrap_err().contains("다이제스트"));
    }

    #[test]
    fn only_the_prepared_cache_apk_is_trusted() {
        let cache = tempfile::tempdir().unwrap();
        let path = cache.path().join("Magisk-v30.7.apk");
        let zip = crate::firmware::tests::build_zip(
            &patch::APK_ENTRIES
                .iter()
                .map(|(name, _)| (*name, name.as_bytes()))
                .collect::<Vec<_>>(),
        );
        let cert = b"topjohnwu-test-cert";
        let pin = apk_verify::sha256_hex(cert);
        let bytes = apk_verify::tests::sign_zip(&zip, cert);
        let sha = crate::boot_image::sha256(&bytes);
        std::fs::write(&path, &bytes).unwrap();
        // 준비 단계의 다이제스트 기록(sidecar)이 없으면 해시가 맞아도 거부
        assert!(load_verified_apk_in(cache.path(), &path, &sha, &[&pin]).is_err());
        apk_verify::store_verified(&path, &bytes, &sha).unwrap();
        assert_eq!(
            load_verified_apk_in(cache.path(), &path, &sha, &[&pin]).unwrap(),
            bytes
        );
        let fallback =
            prepare_or_cache(cache.path(), Err("APK 다운로드 실패".into()), &[&pin]).unwrap();
        assert_eq!(fallback.apk_path, path.to_string_lossy());
        assert_eq!(fallback.sha256, sha);
        assert!(prepare_or_cache(
            cache.path(),
            Err("APK 다운로드 실패".into()),
            &[&"0".repeat(64)]
        )
        .is_err());
        // 해시 불일치·다른 서명자
        assert!(load_verified_apk_in(cache.path(), &path, &"f".repeat(64), &[&pin]).is_err());
        assert!(load_verified_apk_in(cache.path(), &path, &sha, &[&"0".repeat(64)]).is_err());
        // 캐시 폴더 밖의 같은 파일(호출부가 임의 경로를 넘긴 경우)
        let elsewhere = tempfile::tempdir().unwrap();
        let outside = elsewhere.path().join("Magisk-v30.7.apk");
        std::fs::write(&outside, &bytes).unwrap();
        apk_verify::store_verified(&outside, &bytes, &sha).unwrap();
        assert!(load_verified_apk_in(cache.path(), &outside, &sha, &[&pin]).is_err());
        // 기록 뒤 파일이 바뀐 경우
        std::fs::write(&path, &zip).unwrap();
        assert!(load_verified_apk_in(cache.path(), &path, &sha, &[&pin]).is_err());
        assert!(validate_apk(b"broken").is_err());
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
            assert!(pick_asset(
                &serde_json::json!({"tag_name": tag, "assets": [asset(name, url)]})
            )
            .is_err());
        }
    }

    #[test]
    fn asset_picking_requires_official_name() {
        let json = serde_json::json!({
            "tag_name": "v99",
            "assets": [asset("source.zip", "https://x/s.zip")]
        });
        assert!(pick_asset(&json).is_err());
    }
}
