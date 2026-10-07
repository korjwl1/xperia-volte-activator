//! SMS Import/Export(tmo1/sms-ie, GPL-3.0) 연동 — 문자·통화 기록(세미수동).
//! 조사(2026-10-03): adb shell의 SMS/통화 프로바이더 직접 접근은 최신 Android에서 차단.
//! 이 앱은 설치(install)·권한(pm grant)·기본 SMS 역할(cmd role)·파일 전송을 PC에서 자동화하고,
//! 앱 안의 내보내기/가져오기 버튼 + 파일 선택(SAF)만 사용자가 누른다(수동 개입 2탭).
//! APK는 GitHub Releases에서 런타임 다운로드(D12 Magisk 패턴) — 번들하지 않는다.
//! 주의(앱 README): 기본 SMS 앱 전환 중 수신 문자 유실 방지를 위해 비행기 모드 안내가 필요하다.

use crate::apk_verify::{self, ReleaseAsset};
use crate::backup::model::{ItemKind, ItemRecord, ItemStatus};
use adb_client::ADBDeviceExt;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SMSIE_PKG: &str = "com.github.tmo1.sms_ie";
/// 기기 측 내보내기 폴더 — 검증한 선택 산출물만 rm -f로 정리한다.
pub const DEVICE_TMP_DIR: &str = "/sdcard/volte_sms_backup";
const GH_RELEASES_API: &str = "https://api.github.com/repos/tmo1/sms-ie/releases/latest";
const SMS_ROLE: &str = "android.app.role.SMS";
/// 한 기기에서 기본 SMS 역할은 하나여야 한다. 오류/여러 응답을 빈 역할로 추측하지 않는다.
fn current_holder(dev: &mut dyn ADBDeviceExt) -> Result<Option<String>, String> {
    let raw = run(dev, &format!("cmd role get-role-holders {SMS_ROLE}"))?;
    let holders: Vec<_> = raw
        .split([';', '\n', ' ', '\r', '\t'])
        .filter(|p| !p.is_empty())
        .collect();
    match holders.as_slice() {
        [] => Ok(None),
        [p] if super::sms_role::valid_package(p) => Ok(Some((*p).to_string())),
        _ => Err("현재 기본 문자 앱 응답을 확인할 수 없습니다".into()),
    }
}

fn stage_role(dev: &mut dyn ADBDeviceExt, state_dir: &Path) -> Result<(), String> {
    let key = crate::device_io::identity_key(dev)?;
    let record = super::sms_role::load(state_dir, &key)?;
    let current = current_holder(dev)?;
    if record
        .as_ref()
        .is_some_and(|r| current != r.previous_holder)
        && current.as_deref() != Some(SMSIE_PKG)
    {
        return Err(
            "기본 문자 앱이 복원 중 다른 앱으로 바뀌었습니다 — 설정에서 확인해 주세요".into(),
        );
    }
    // 재시도/재시작 때 이미 전환된 역할에서 원래 앱·비행기 모드 기록을 덮어쓰지 않는다.
    let previous_airplane = match &record {
        Some(r) => r.previous_airplane,
        None => {
            let airplane = airplane_mode(dev)?;
            super::sms_role::save(state_dir, &key, current.clone(), Some(airplane))?;
            Some(airplane)
        }
    };
    // 기본 문자 앱이 바뀐 동안 수신 문자가 유실되지 않게 비행기 모드를 켠다(사용자 조작 대신, 2026-10-08 실기기 확인)
    set_airplane_mode(dev, true)?;
    let switched = (|| {
        if current.as_deref() != Some(SMSIE_PKG) {
            run(
                dev,
                &format!("cmd role add-role-holder {SMS_ROLE} {SMSIE_PKG}"),
            )
            .map_err(|e| format!("기본 문자 앱 전환 실패: {e}"))?;
        }
        if current_holder(dev)?.as_deref() != Some(SMSIE_PKG) {
            return Err("기본 문자 앱 전환 명령 후 실제 역할을 확인하지 못했습니다".into());
        }
        Ok(())
    })();
    if switched.is_err() && current_holder(dev).ok().flatten() == current && previous_airplane == Some(false) {
        // 역할이 그대로면 비행기 모드를 둘 이유가 없다 — 원래대로 되돌린다(실패해도 원래 오류를 보인다)
        let _ = set_airplane_mode(dev, false);
    }
    switched
}

/// 비행기 모드 켜짐 여부 — `cmd connectivity airplane-mode`(Android 11+)
fn airplane_mode(dev: &mut dyn ADBDeviceExt) -> Result<bool, String> {
    match run(dev, "cmd connectivity airplane-mode")?.trim() {
        "enabled" => Ok(true),
        "disabled" => Ok(false),
        other => Err(format!("비행기 모드 상태를 읽지 못했습니다({other})")),
    }
}

fn set_airplane_mode(dev: &mut dyn ADBDeviceExt, on: bool) -> Result<(), String> {
    let verb = if on { "enable" } else { "disable" };
    run(dev, &format!("cmd connectivity airplane-mode {verb}"))
        .map_err(|e| format!("비행기 모드 {} 실패: {e}", if on { "켜기" } else { "끄기" }))?;
    if airplane_mode(dev)? != on {
        return Err(format!("비행기 모드를 {} 못했습니다", if on { "켜지" } else { "끄지" }));
    }
    Ok(())
}
/// 내보내기 산출물 파일명 접두사(sms-ie 규칙, 원본 소스 MainActivity.kt·ImportExportWorker.kt 확인 2026-10-05)
/// — messages<날짜>.zip / calls<날짜>.json (예전 가정 call-logs-*.json은 sms-ie가 만들지 않는 이름이었다)
const MESSAGES_PREFIX: &str = "messages";
const CALLLOG_PREFIX: &str = "calls";

fn run(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    crate::device_io::shell(dev, cmd)
}

pub fn installed(dev: &mut dyn ADBDeviceExt) -> Result<bool, String> {
    let out = run(dev, &format!("pm list packages {SMSIE_PKG}"))?;
    Ok(out
        .lines()
        .any(|line| line.trim().strip_prefix("package:") == Some(SMSIE_PKG)))
}

/// 정상 APK는 수 MiB — 상한은 디스크·메모리 보호용
const MAX_APK: u64 = 128 * 1024 * 1024;

fn safe_name(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
}

/// 릴리스 JSON → (태그, 자산). standard flavor의 .apk 우선, legacy(구 Android용) 제외.
/// 내려받기 주소는 tmo1/sms-ie 릴리스 경로만, 다이제스트·크기가 없으면 실패.
fn pick_asset(json: &serde_json::Value) -> Result<(String, ReleaseAsset), String> {
    let tag = json["tag_name"]
        .as_str()
        .filter(|t| safe_name(t))
        .ok_or("릴리스 버전 형식이 올바르지 않습니다")?
        .to_string();
    let assets = json["assets"]
        .as_array()
        .ok_or("릴리스 자산 목록이 없습니다")?;
    let official = |a: &&serde_json::Value| {
        let (Some(name), Some(url)) = (a["name"].as_str(), a["browser_download_url"].as_str())
        else {
            return false;
        };
        safe_name(name)
            && name.ends_with(".apk")
            && !name.contains("legacy")
            && url == format!("https://github.com/tmo1/sms-ie/releases/download/{tag}/{name}")
    };
    let pick = assets
        .iter()
        .filter(official)
        .find(|a| a["name"].as_str().is_some_and(|n| n.contains("standard")))
        .or_else(|| assets.iter().find(official))
        .ok_or("APK 자산을 찾을 수 없습니다")?;
    let name = pick["name"].as_str().unwrap_or_default();
    let url = pick["browser_download_url"].as_str().unwrap_or_default();
    Ok((tag, apk_verify::asset_from_json(pick, name, url, MAX_APK)?))
}

/// GitHub Releases에서 APK 확보(캐시) → (경로, SHA-256).
/// 받은 바이트는 GitHub 다이제스트·서명 인증서 핀(tmo1 릴리스 키)을 통과해야 캐시에 저장된다.
/// 릴리스 조회가 안 되면(오프라인·요청 한도) 다이제스트가 기록된 검증 캐시만 쓴다.
/// 앱 서명 자체는 설치 때 Android가 검증한다.
pub fn download_apk(cache_dir: &Path) -> Result<(PathBuf, String), String> {
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    let pins = apk_verify::SMSIE_CERT_SHA256;
    let (tag, asset) = match fetch_release() {
        Ok(release) => release,
        Err(online) => {
            let (path, _, sha) = apk_verify::newest_verified_cache(
                cache_dir,
                |n| n.starts_with("sms-ie-") && n.ends_with(".apk"),
                pins,
                MAX_APK as usize,
            )
            .ok_or_else(|| format!("{online} — 검증된 APK 캐시도 없습니다"))?;
            eprintln!("[rust] sms-ie 릴리스 조회 실패({online}) — 검증된 캐시 사용");
            return Ok((path, sha));
        }
    };
    let dest = cache_dir.join(format!("sms-ie-{tag}.apk"));
    if let Some((_, sha)) =
        apk_verify::load_cached(&dest, Some(&asset.sha256), pins, MAX_APK as usize)
    {
        return Ok((dest, sha));
    }
    let mut bytes = Vec::with_capacity(asset.size as usize);
    ureq::get(&asset.url)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("APK 다운로드 실패: {e}"))?
        .into_reader()
        .take(MAX_APK + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("APK 수신 실패: {e}"))?;
    // 검증을 모두 통과한 뒤에만 캐시에 남긴다
    apk_verify::verify_download(&bytes, &asset)?;
    apk_verify::check_pins(&bytes, pins)?;
    apk_verify::store_verified(&dest, &bytes, &asset.sha256)?;
    Ok((dest, asset.sha256))
}

fn fetch_release() -> Result<(String, ReleaseAsset), String> {
    let mut text = vec![];
    ureq::get(GH_RELEASES_API)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("릴리스 조회 실패: {e}"))?
        .into_reader()
        .take(1024 * 1024 + 1)
        .read_to_end(&mut text)
        .map_err(|e| format!("릴리스 정보 수신 실패: {e}"))?;
    if text.len() > 1024 * 1024 {
        return Err("릴리스 응답이 너무 큽니다".into());
    }
    let json: serde_json::Value =
        serde_json::from_slice(&text).map_err(|e| format!("릴리스 정보 해석 실패: {e}"))?;
    pick_asset(&json)
}

/// 설치 + 권한 + 임시 폴더 준비 (백업·복원 공통)
/// 설치·권한 준비. `apk`는 미리 받아 검증한 (경로, sha256) — 다운로드는 기기 연결을 잡기 전에 한다.
pub fn prepare(
    dev: &mut dyn ADBDeviceExt,
    apk: Option<&(PathBuf, String)>,
) -> Result<Vec<String>, String> {
    prepare_with_pins(dev, apk, apk_verify::SMSIE_CERT_SHA256)
}

/// Open the app only; export/import buttons and the save picker remain user actions.
pub fn open_export_app(dev: &mut dyn ADBDeviceExt) -> Result<(), String> {
    let output = run(dev, &format!("am start -n {SMSIE_PKG}/.MainActivity"))?;
    if output.lines().any(|line| {
        let line = line.trim();
        line.starts_with("Error:") || line.starts_with("Exception")
    }) || !output.contains("Starting: Intent")
    {
        return Err(format!(
            "문자 백업 앱 실행을 확인할 수 없습니다: {}",
            output.trim()
        ));
    }
    Ok(())
}

fn prepare_with_pins(
    dev: &mut dyn ADBDeviceExt,
    apk: Option<&(PathBuf, String)>,
    pins: &[&str],
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    if !installed(dev)? {
        let Some((apk, sha)) = apk else {
            return Err("SMS Import/Export 앱이 설치되어 있지 않습니다".into());
        };
        apk_verify::load_cached(apk, Some(sha), pins, MAX_APK as usize)
            .ok_or("sms-ie APK가 준비 후 변경됐거나 검증된 캐시가 아닙니다")?;
        dev.install(&apk, None)
            .map_err(|e| format!("앱 설치 실패: {e}"))?;
        // 설치한 APK(GitHub 다이제스트·서명 핀 검증 완료)의 해시를 기록한다
        log.push(format!(
            "설치: {} (sha256 {})",
            apk.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            sha.get(..16).unwrap_or(sha)
        ));
    } else {
        log.push("이미 설치됨".into());
    }
    // 내보내기·가져오기에 필요한 권한 전부 미리 부여 — 실패는 로그로만(기기별 차이)
    for perm in [
        "android.permission.READ_SMS",
        "android.permission.READ_CALL_LOG",
        "android.permission.READ_CONTACTS",
        "android.permission.WRITE_CALL_LOG",
        "android.permission.WRITE_CONTACTS",
    ] {
        match run(dev, &format!("pm grant {SMSIE_PKG} {perm}")) {
            Ok(_) => log.push(format!("권한: {perm}")),
            Err(e) => log.push(format!("권한 실패(계속): {perm} — {e}")),
        }
    }
    run(dev, &format!("mkdir -p {DEVICE_TMP_DIR}"))?;
    log.push(format!("임시 폴더: {DEVICE_TMP_DIR}"));
    Ok(log)
}

/// 임시 폴더의 산출물을 item에 매핑 — 파일명 접두사로 구분
fn item_for(name: &str) -> Option<&'static str> {
    let lower = name.to_lowercase();
    if lower.starts_with(MESSAGES_PREFIX) && lower.ends_with(".zip") {
        Some("sms")
    } else if lower.starts_with(CALLLOG_PREFIX) && lower.ends_with(".json") {
        Some("calllog")
    } else {
        None
    }
}

pub fn probe(dev: &mut dyn ADBDeviceExt, selected: &[&str]) -> Result<bool, String> {
    let entries = dev
        .list(&DEVICE_TMP_DIR)
        .map_err(|e| format!("내보내기 파일 목록 조회 실패: {e}"))?;
    let found: std::collections::HashSet<_> = entries
        .into_iter()
        .filter_map(|e| match e {
            adb_client::ADBListItemType::File(f) if f.size > 0 => item_for(&f.name),
            _ => None,
        })
        .collect();
    Ok(!selected.is_empty() && selected.iter().all(|id| found.contains(id)))
}

/// 문자·통화 수집 — 연락처 JSON은 제외하고 기존 VCF 백업을 사용한다.
/// 삭제는 선택 항목 검증과 manifest 저장이 모두 끝난 뒤 명령 층에서 실행한다.
pub fn collect(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
    selected: &[&str],
) -> Result<CollectState, String> {
    let tmp_dir = DEVICE_TMP_DIR.to_string();
    let entries = dev
        .list(&tmp_dir)
        .map_err(|e| format!("임시 폴더 조회 실패: {e}"))?;
    let mut found: Vec<(String, u64, u32)> = Vec::new();
    for e in entries {
        if let adb_client::ADBListItemType::File(f) = e {
            if item_for(&f.name).is_none() || !selected.contains(&item_for(&f.name).unwrap()) {
                continue;
            }
            found.push((f.name, f.size, f.time));
        }
    }
    if found.is_empty() {
        return Ok(CollectState::NotReady);
    }
    let mut records: Vec<(String, ItemRecord)> = vec![
        ("sms".into(), ItemRecord::new("sms", ItemKind::SmsIe)),
        (
            "calllog".into(),
            ItemRecord::new("calllog", ItemKind::SmsIe),
        ),
    ];
    for (name, size, mtime) in found {
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let local_rel = format!("smsie/{name}");
        // 파일 백업과 같은 경로 — 해시·mtime 보존, 4GiB 이상 크기(stat 재확인)·실패 시 반쪽 파일 정리.
        // 받을 수 없는 이름 등 개별 실패는 그 파일의 오류로 남기고 나머지는 계속 받는다.
        let entry = super::puller::pull_to_disk_checked(
            dev,
            &remote,
            Path::new(&local_rel),
            backup_root,
            size,
            mtime,
            &|dev, path| {
                let id = item_for(&name).ok_or("알 수 없는 내보내기 파일")?;
                super::smsie_export::validate(path, id)?;
                let stat = dev
                    .stat_extended(&remote)
                    .map_err(|e| e.to_string())?
                    .ok_or("내보내기 파일 상태를 확인할 수 없습니다")?;
                let local_size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
                if stat.size != local_size || stat.mtime != mtime {
                    return Err("내보내기가 진행 중입니다 — 완료 후 다시 확인하세요".into());
                }
                Ok(())
            },
        );
        match item_for(&name) {
            Some(id) => {
                let rec = records
                    .iter_mut()
                    .find(|(rid, _)| rid == id)
                    .expect("산출물 종류 준비됨");
                rec.1.entries.push(entry);
            }
            None => {
                // 알 수 없는 산출물 — sms 항목에 오류로 기록(가져오기 대상 파악 불가)
                let rec = records
                    .iter_mut()
                    .find(|(rid, _)| rid == "sms")
                    .expect("준비됨");
                rec.1.errors.push(format!("알 수 없는 파일: {name}"));
            }
        }
    }
    for (_, rec) in records.iter_mut() {
        rec.files = rec.entries.len() as u32;
        rec.bytes = rec
            .entries
            .iter()
            .filter(|e| e.error.is_none())
            .map(|e| e.size)
            .sum();
        rec.artifacts = rec
            .entries
            .iter()
            .filter(|e| e.error.is_none())
            .map(|e| e.local.clone())
            .collect();
        let failed = !rec.errors.is_empty()
            || rec.entries.iter().any(|e| e.error.is_some())
            || rec.entries.is_empty();
        rec.status = if failed {
            ItemStatus::Partial
        } else {
            ItemStatus::Done
        };
        if rec.entries.is_empty() {
            rec.errors.push("이 항목의 산출 파일을 찾지 못했습니다 — 앱에서 해당 데이터를 내보냈는지 확인해 주세요".into());
        }
    }
    let _ = selected;
    Ok(CollectState::Records(records))
}

pub(super) fn selected_done(records: &[(String, ItemRecord)], selected: &[&str]) -> bool {
    !selected.is_empty()
        && selected.iter().all(|id| {
            records.iter().any(|(rid, item)| {
                rid == id
                    && item.status == ItemStatus::Done
                    && item.errors.is_empty()
                    && !item.entries.is_empty()
                    && item.entries.iter().all(|e| e.error.is_none())
            })
        })
}

pub(super) fn cleanup_exports(
    dev: &mut dyn ADBDeviceExt,
    records: &[ItemRecord],
) -> Result<(), String> {
    for record in records {
        for entry in &record.entries {
            let name = entry
                .remote
                .strip_prefix(&format!("{DEVICE_TMP_DIR}/"))
                .ok_or("내보내기 정리 경로 오류")?;
            if name.contains('/')
                || item_for(name) != Some(record.id.as_str())
                || entry.error.is_some()
                || entry.sha256.is_none()
            {
                return Err("검증되지 않은 내보내기 파일은 삭제할 수 없습니다".into());
            }
            let stat = dev
                .stat_extended(&entry.remote)
                .map_err(|e| e.to_string())?;
            if let Some(stat) = stat {
                if stat.size != entry.size || stat.mtime != entry.mtime {
                    return Err("백업 후 변경된 내보내기 파일을 폰에 보존했습니다".into());
                }
                let quoted = format!("'{}'", entry.remote.replace('\'', "'\"'\"'"));
                run(dev, &format!("rm -f -- {quoted}"))?;
            }
        }
    }
    Ok(())
}

pub enum CollectState {
    /// 앱에서 아직 내보내지 않음 — 프론트는 수동 개입 유지
    NotReady,
    /// 항목 기록 — manifest에 병합한다
    Records(Vec<(String, ItemRecord)>),
}

/// 복원 준비 — 파일 전송 + 기본 SMS 앱 역할 부여(비행기 모드 안내는 프론트).
/// 반환: 사용자 안내 문구
pub fn restore_stage(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
    items: &[String],
    state_dir: &Path,
    apk: Option<&(PathBuf, String)>,
) -> Result<String, String> {
    super::runner::validate_items(items)?;
    let mut manifest = super::model::load_manifest(backup_root)?;
    manifest
        .items
        .retain(|item| items.contains(&item.id) && item.kind == ItemKind::SmsIe);
    for id in items
        .iter()
        .filter(|id| matches!(id.as_str(), "sms" | "calllog"))
    {
        if !manifest.items.iter().any(|item| &item.id == id) {
            return Err(format!("{id}: 문자·통화 기록 백업이 없습니다"));
        }
    }
    let mut problems = super::verify::verify_manifest(backup_root, &mut manifest);
    // 요청한 항목 자체로 판정한다(일반 복원과 같은 기준). manifest.complete()는 마지막 백업 실행의 선택 기준이라,
    // 갱신 때 문자를 고르지 않으면(excludedItems) 앞서 정상 백업된 문자 기록까지 못 쓰게 만들었다(2026-10-08 실기기).
    for item in &manifest.items {
        if item.status != super::model::ItemStatus::Done
            || !item.errors.is_empty()
            || item.entries.iter().any(|e| e.error.is_some())
        {
            problems.push(format!("{}: 완료되지 않은 백업입니다", item.id));
        }
    }
    if !problems.is_empty() {
        return Err(format!(
            "문자·통화 기록 백업 검증 실패: {}",
            problems.join(" / ")
        ));
    }
    let mut names = Vec::new();
    for item in &manifest.items {
        for entry in &item.entries {
            let name = entry
                .local
                .strip_prefix("smsie/")
                .ok_or("문자 백업 경로가 잘못됐습니다")?;
            if name.contains(['/', '\\']) || item_for(name) != Some(item.id.as_str()) {
                return Err("문자 백업 파일 형식이 잘못됐습니다".into());
            }
            let path = super::paths::existing_file(backup_root, &entry.local)?;
            super::smsie_export::validate(&path, &item.id)?;
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Err("백업된 문자·통화 기록 파일이 없습니다".into());
    }
    prepare(dev, apk)?;
    for name in &names {
        let path = crate::backup::paths::existing_file(backup_root, &format!("smsie/{name}"))?;
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let mut reader = std::fs::File::open(path).map_err(|e| format!("{name} 읽기 실패: {e}"))?;
        dev.push(&mut reader, &remote)
            .map_err(|e| format!("{name} 전송 실패: {e}"))?;
    }
    stage_role(dev, state_dir)?;
    open_export_app(dev)?;
    let list = names.join(", ");
    let instructions = manifest
        .items
        .iter()
        .map(|item| {
            if item.id == "sms" {
                "Import Messages → messages*.zip"
            } else {
                "Import Call Log → calls*.json"
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    Ok(format!(
        "비행기 모드를 켰습니다(기본 문자 앱이 바뀐 동안 수신 문자 유실 방지 — 마무리에서 원래대로 돌립니다).\n내장 저장소 → volte_sms_backup에 검증한 파일({list})을 올려 두었습니다.\n{instructions}\n선택한 항목의 성공 안내를 확인한 뒤 [확인하고 진행]을 눌러주세요.",
    ))
}

/// 복원 마무리 — 기본 SMS 앱 역할 원복 + 임시 정리. 원복 실패는 안내 문구로 반환
pub fn restore_finish(dev: &mut dyn ADBDeviceExt, state_dir: &Path) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let key = crate::device_io::identity_key(dev)?;
    let record = super::sms_role::load(state_dir, &key)?
        .ok_or("이 기기의 문자 앱 원복 기록이 없습니다 — 설정에서 기본 문자 앱을 확인해 주세요")?;
    let prev = record.previous_holder.clone();
    let current = current_holder(dev)?;
    if current != prev && current.as_deref() != Some(SMSIE_PKG) {
        return Err(
            "기본 문자 앱이 복원 중 다른 앱으로 바뀌었습니다 — 설정에서 확인해 주세요".into(),
        );
    }
    let result = if current == prev {
        Ok(String::new())
    } else {
        match &prev {
            // 원래 앱을 다시 기본으로 지정하면 sms-ie는 자동으로 역할을 잃는다
            Some(p) => run(dev, &format!("cmd role add-role-holder {SMS_ROLE} {p}")),
            None => run(
                dev,
                &format!("cmd role remove-role-holder {SMS_ROLE} {SMSIE_PKG}"),
            ),
        }
    };
    match result {
        Ok(_) => log.push(match &prev {
            Some(p) => format!("기본 문자 앱을 원래 앱({p})으로 되돌렸습니다 — 문자 앱을 한 번 열어 확인하세요"),
            None => "기본 문자 앱 역할을 해제했습니다 — 문자 앱을 열어 기본 앱으로 지정됐는지 확인하세요".into(),
        }),
        Err(e) => return Err(format!("기본 문자 앱 원복 실패 — 설정 > 앱 > 기본 앱에서 직접 바꿔주세요: {e}")),
    }
    if current_holder(dev)? != prev {
        return Err("기본 문자 앱 원복 명령 후 실제 역할을 확인하지 못했습니다".into());
    }
    // 문자 앱을 되돌린 뒤에만 비행기 모드를 원래대로 — 그 전에 끄면 문자가 sms-ie로 들어올 수 있다
    if record.previous_airplane == Some(false) {
        set_airplane_mode(dev, false)
            .map_err(|e| format!("{e} — 폰에서 비행기 모드를 직접 꺼 주세요"))?;
        log.push("비행기 모드를 원래대로 껐습니다 — 통신이 다시 연결됩니다".into());
    }
    super::sms_role::remove(state_dir, &key)?;
    log.push(
        "가져오기 파일은 volte_sms_backup에 보존했습니다. 필요 없으면 직접 삭제할 수 있습니다"
            .into(),
    );
    log.push(
        "완전한 반영을 위해 문자 앱 설정에서 저장공간/캐시 삭제가 필요할 수 있습니다(앱 캐싱)"
            .into(),
    );
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_names_follow_sms_ie() {
        // sms-ie 원본: "messages$dateInString.zip", "calls$dateInString.json"
        assert_eq!(item_for("messages-2026-10-05.zip"), Some("sms"));
        assert_eq!(item_for("messages-2027-01-02 (1).zip"), Some("sms"));
        assert_eq!(item_for("calls-2026-10-05.json"), Some("calllog"));
        assert_eq!(item_for("calls-2025-12-31 (2).json"), Some("calllog"));
        assert_eq!(item_for("Calls 2026-10-05.json"), Some("calllog"));
        assert_eq!(item_for("contacts-2027-01-02 (1).json"), None);
    }

    #[test]
    fn release_asset_requires_digest_official_origin_and_prefers_standard() {
        let base = "https://github.com/tmo1/sms-ie/releases/download/v2.11.1";
        let asset = |name: &str, url: String, digest: bool| {
            let mut a = serde_json::json!({"name": name, "browser_download_url": url, "size": 100});
            if digest {
                a["digest"] = format!("sha256:{}", "cd".repeat(32)).into();
            }
            a
        };
        let legacy = "com.github.tmo1.sms_ie-v2.11.1-legacy-release.apk";
        let standard = "com.github.tmo1.sms_ie-v2.11.1-standard-release.apk";
        let json = serde_json::json!({"tag_name": "v2.11.1", "assets": [
            asset(legacy, format!("{base}/{legacy}"), true),
            asset(standard, format!("{base}/{standard}"), true),
        ]});
        let (tag, picked) = pick_asset(&json).unwrap();
        assert_eq!(tag, "v2.11.1");
        assert_eq!(picked.name, standard);
        assert_eq!(picked.sha256, "cd".repeat(32));
        // 다이제스트 없음·다른 출처·legacy만 있음은 거부
        let no_digest = serde_json::json!({"tag_name": "v2.11.1", "assets": [asset(standard, format!("{base}/{standard}"), false)]});
        assert!(pick_asset(&no_digest).is_err());
        let foreign = serde_json::json!({"tag_name": "v2.11.1", "assets": [asset(standard, "https://example.com/a.apk".into(), true)]});
        assert!(pick_asset(&foreign).is_err());
        let only_legacy = serde_json::json!({"tag_name": "v2.11.1", "assets": [asset(legacy, format!("{base}/{legacy}"), true)]});
        assert!(pick_asset(&only_legacy).is_err());
    }
    use crate::backup::model::FileEntry;

    fn dev_ready() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            &format!("pm list packages {SMSIE_PKG}"),
            &format!("package:{SMSIE_PKG}\n"),
        );
        d.answer_shell("pm grant", ""); // 권한 부여 성공 응답
        d.answer_shell("mkdir", "");
        d.answer_shell("rm -rf", "");
        d.answer_shell("cmd role", "");
        d.answer_shell(
            "am start",
            "Starting: Intent { cmp=com.github.tmo1.sms_ie/.MainActivity }",
        );
        d
    }

    #[test]
    fn collect_maps_files_to_items() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-2026-10-03.zip"),
            &super::super::smsie_export::tests::export_zip(b"{}\n"),
            1700000000,
            0o644,
        );
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/calls-2026-10-03.json"),
            b"[]",
            1700000001,
            0o644,
        );
        let tmp = tempfile::tempdir().unwrap();
        match collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap() {
            CollectState::Records(recs) => {
                let sms = recs.iter().find(|(id, _)| id == "sms").unwrap();
                let calls = recs.iter().find(|(id, _)| id == "calllog").unwrap();
                assert_eq!(sms.1.status, ItemStatus::Done);
                assert_eq!(calls.1.status, ItemStatus::Done);
                assert_eq!(sms.1.files, 1);
                assert!(tmp.path().join("smsie/messages-2026-10-03.zip").exists());
                assert!(!d.shell_calls.iter().any(|c| c.starts_with("rm -f --")));
            }
            _ => panic!("records 여야 함"),
        }
    }

    #[test]
    fn contacts_only_exports_are_not_sms_readiness_and_are_never_pulled() {
        let mut device = dev_ready();
        device.add_dir(DEVICE_TMP_DIR);
        for name in ["contacts-2027-01-02.json", "Contacts-2028-03-04 (1).JSON"] {
            device.add_file(
                &format!("{DEVICE_TMP_DIR}/{name}"),
                b"invalid JSON",
                1,
                0o644,
            );
        }
        let root = tempfile::tempdir().unwrap();
        assert!(matches!(
            collect(&mut device, root.path(), &["sms", "calllog"]).unwrap(),
            CollectState::NotReady
        ));
        assert!(device.pull_calls.is_empty());
    }

    #[test]
    fn restore_role_goes_back_to_previous_holder() {
        // 실제 상태가 바뀌는 가짜 역할 서비스로 원복까지 검사한다.
        let mut d = dev_ready();
        d.answer_shell("getprop ro.serialno", "FAKE-A");
        d.sms_role_holder = Some(Some("com.google.android.apps.messaging".into()));
        d.answer_shell("mkdir", "");
        d.answer_shell("rm -rf", "");
        let tmp = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("smsie")).unwrap();
        std::fs::write(
            tmp.path().join("smsie/messages-1.zip"),
            super::super::smsie_export::tests::export_zip(b"{}\n"),
        )
        .unwrap();
        let mut manifest = super::super::model::Manifest::new("XQ", "masked", "v", "15");
        let mut item = ItemRecord::new("sms", ItemKind::SmsIe);
        item.status = ItemStatus::Done;
        use sha2::Digest;
        item.entries.push(FileEntry {
            remote: format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            local: "smsie/messages-1.zip".into(),
            size: super::super::smsie_export::tests::export_zip(b"{}\n").len() as u64,
            mtime: 0,
            sha256: Some(hex::encode(sha2::Sha256::digest(
                super::super::smsie_export::tests::export_zip(b"{}\n"),
            ))),
            quarantined: false,
            error: None,
        });
        manifest.record(item);
        // 실기기: 마지막 백업 갱신에서 문자를 고르지 않아 제외 목록에 있어도, 앞서 정상 백업된 기록은 복원할 수 있다
        manifest.excluded_items = vec!["sms".into(), "calllog".into()];
        super::super::model::save_manifest_atomic(&manifest, tmp.path()).unwrap();
        std::fs::write(tmp.path().join("smsie/unrecorded.zip"), b"extra").unwrap();
        assert!(restore_stage(
            &mut d,
            tmp.path(),
            &["sms".into(), "calllog".into()],
            state.path(),
            None
        )
        .is_err());
        assert!(d.pushed.is_empty());
        restore_stage(&mut d, tmp.path(), &["sms".into()], state.path(), None).unwrap();
        // 기본 문자 앱이 sms-ie인 동안 수신 문자가 그리로 가지 않게 비행기 모드를 켠다
        assert!(d.airplane);
        assert!(d.shell_calls.iter().any(|c| c.starts_with("pm grant")));
        assert_eq!(d.pushed.len(), 1);
        assert!(!d
            .pushed
            .contains_key(&format!("{DEVICE_TMP_DIR}/unrecorded.zip")));
        let mut disconnected = FakeADBDevice::new();
        assert!(restore_finish(&mut disconnected, state.path()).is_err());
        d.fail_shell.insert("cmd role add-role-holder".into());
        assert!(restore_finish(&mut d, state.path()).is_err());
        // 문자 앱을 되돌리지 못했으면 비행기 모드도 그대로 둔다
        assert!(d.airplane);
        d.fail_shell.clear();
        restore_finish(&mut d, state.path()).unwrap();
        assert!(d.shell_calls.iter().any(|c| c
            == "cmd role add-role-holder android.app.role.SMS com.google.android.apps.messaging"));
        // 원래 꺼져 있었으므로 마무리에서 끈다
        assert!(!d.airplane);
    }

    fn role_device(serial: &str, holder: Option<&str>) -> FakeADBDevice {
        let mut d = dev_ready();
        d.answer_shell("getprop ro.serialno", serial);
        d.sms_role_holder = Some(holder.map(str::to_string));
        d
    }

    #[test]
    fn installation_check_requires_the_exact_package() {
        for (output, expected) in [
            (format!("package:{SMSIE_PKG}.other\n"), false),
            (format!("package:{SMSIE_PKG}\n"), true),
            ("Error: permission denied".into(), false),
        ] {
            let mut d = FakeADBDevice::new();
            d.answer_shell("pm list packages", &output);
            assert_eq!(installed(&mut d).unwrap(), expected);
        }
    }

    #[test]
    fn missing_identity_prevents_role_mutation() {
        let state = tempfile::tempdir().unwrap();
        let mut d = role_device("", Some("com.original.sms"));
        assert!(stage_role(&mut d, state.path()).is_err());
        assert!(restore_finish(&mut d, state.path()).is_err());
        assert!(!d.shell_calls.iter().any(|c| c.contains("role-holder")));
        assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
    }

    #[test]
    fn role_journal_survives_reconnect_and_isolates_devices() {
        let state = tempfile::tempdir().unwrap();
        let mut a = role_device("FAKE-A", Some("com.original.sms"));
        let mut b = role_device("FAKE-B", None);
        stage_role(&mut a, state.path()).unwrap();
        stage_role(&mut b, state.path()).unwrap();
        // 새로운 연결에서 재시도해도 원래 앱을 유지한다.
        let mut reconnected = role_device("FAKE-A", Some(SMSIE_PKG));
        stage_role(&mut reconnected, state.path()).unwrap();
        restore_finish(&mut reconnected, state.path()).unwrap();
        assert_eq!(
            reconnected.sms_role_holder,
            Some(Some("com.original.sms".into()))
        );
        restore_finish(&mut b, state.path()).unwrap();
        assert_eq!(b.sms_role_holder, Some(None));
        assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 0);
    }

    #[test]
    fn role_change_requires_persisted_record_and_confirmed_role() {
        let state = tempfile::tempdir().unwrap();
        let invalid_dir = state.path().join("file");
        std::fs::write(&invalid_dir, b"x").unwrap();
        let mut d = role_device("FAKE-A", Some("com.original.sms"));
        assert!(stage_role(&mut d, &invalid_dir).is_err());
        assert!(!d.shell_calls.iter().any(|c| c.contains("add-role-holder")));
        d.ignore_role_changes = true;
        assert!(stage_role(&mut d, state.path()).is_err());
        let key = crate::device_io::identity_key(&mut d).unwrap();
        assert!(super::super::sms_role::load(state.path(), &key)
            .unwrap()
            .is_some());
        d.ignore_role_changes = false;
        stage_role(&mut d, state.path()).unwrap();
        d.ignore_role_changes = true;
        assert!(restore_finish(&mut d, state.path()).is_err());
        assert!(super::super::sms_role::load(state.path(), &key)
            .unwrap()
            .is_some());
        d.ignore_role_changes = false;
        restore_finish(&mut d, state.path()).unwrap();
    }

    #[test]
    fn role_missing_record_or_changed_user_choice_is_not_overridden() {
        let state = tempfile::tempdir().unwrap();
        let mut d = role_device("FAKE-A", Some(SMSIE_PKG));
        assert!(restore_finish(&mut d, state.path()).is_err());
        // 원래 sms-ie를 쓰는 사용자의 기본 앱은 해제하지 않는다.
        stage_role(&mut d, state.path()).unwrap();
        restore_finish(&mut d, state.path()).unwrap();
        assert_eq!(d.sms_role_holder, Some(Some(SMSIE_PKG.into())));
        d.sms_role_holder = Some(Some("com.original.sms".into()));
        stage_role(&mut d, state.path()).unwrap();
        d.sms_role_holder = Some(Some("com.changed.sms".into()));
        assert!(stage_role(&mut d, state.path()).is_err());
        assert!(restore_finish(&mut d, state.path()).is_err());
        assert_eq!(d.sms_role_holder, Some(Some("com.changed.sms".into())));
        d.sms_role_holder = Some(Some("unknown output".into()));
        assert!(stage_role(&mut d, state.path()).is_err());
    }

    #[test]
    fn not_ready_when_no_exports() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(
            collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap(),
            CollectState::NotReady
        ));
    }

    #[test]
    fn export_app_launch_rejects_errors_and_preparation_creates_the_requested_folder() {
        let mut d = dev_ready();
        prepare(&mut d, None).unwrap();
        assert!(d
            .shell_calls
            .iter()
            .any(|cmd| cmd == "mkdir -p /sdcard/volte_sms_backup"));
        open_export_app(&mut d).unwrap();
        assert!(d
            .shell_calls
            .iter()
            .any(|cmd| cmd == "am start -n com.github.tmo1.sms_ie/.MainActivity"));
        for output in [
            "",
            "Starting: Intent\nError: Activity not started",
            "Exception: permission denied",
        ] {
            let mut broken = FakeADBDevice::new();
            broken.answer_shell("am start", output);
            assert!(open_export_app(&mut broken).is_err());
        }
    }

    #[test]
    fn adversarial_uninstalled_restore_requires_install_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sms-ie.apk");
        let apk = apk_verify::tests::signed_apk(b"fixture certificate", true);
        let sha = apk_verify::sha256_hex(&apk);
        apk_verify::store_verified(&path, &apk, &sha).unwrap();
        let pin = apk_verify::sha256_hex(b"fixture certificate");
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("pm list packages", "");
        dev.answer_shell("pm grant", "");
        dev.answer_shell("mkdir", "");
        let input = (path.clone(), sha);
        assert!(prepare_with_pins(&mut dev, None, &[&pin]).is_err());
        prepare_with_pins(&mut dev, Some(&input), &[&pin]).unwrap();
        assert_eq!(dev.installs.len(), 1);
        assert_eq!(
            dev.shell_calls
                .iter()
                .filter(|c| c.starts_with("pm grant"))
                .count(),
            5
        );
        std::fs::write(path, b"changed").unwrap();
        assert!(prepare_with_pins(&mut dev, Some(&input), &[&pin]).is_err());
        assert_eq!(dev.installs.len(), 1);
    }

    #[test]
    fn adversarial_zero_or_incomplete_export_never_authorizes_cleanup() {
        for payload in [b"".as_slice(), b"PK\x03\x04half-written", b"not a ZIP"] {
            let mut d = dev_ready();
            d.add_dir(DEVICE_TMP_DIR);
            d.add_file(&format!("{DEVICE_TMP_DIR}/messages.zip"), payload, 0, 0o644);
            d.add_file(&format!("{DEVICE_TMP_DIR}/calls.json"), b"[]", 0, 0o644);
            let tmp = tempfile::tempdir().unwrap();
            let CollectState::Records(records) =
                collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap()
            else {
                panic!()
            };
            assert!(!selected_done(&records, &["sms", "calllog"]));
            assert!(!d.shell_calls.iter().any(|c| c.starts_with("rm -rf")));
            assert!(!tmp.path().join("smsie/messages.zip").exists());
        }
        assert!(!selected_done(&[], &["sms"]));
    }

    #[test]
    fn probe_only_lists_and_never_pulls_exports() {
        let mut dev = dev_ready();
        dev.add_dir(DEVICE_TMP_DIR);
        dev.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-2099-10-07 (1).zip"),
            b"large",
            1,
            0o644,
        );
        assert!(probe(&mut dev, &["sms"]).unwrap());
        assert!(!probe(&mut dev, &["sms", "calllog"]).unwrap());
        dev.add_file(
            &format!("{DEVICE_TMP_DIR}/calls-2099-10-07.json"),
            b"[]",
            1,
            0o644,
        );
        assert!(probe(&mut dev, &["sms", "calllog"]).unwrap());
        assert!(dev.pull_calls.is_empty());
        assert!(dev.shell_calls.is_empty());
    }
    #[test]
    fn unrelated_user_files_are_never_collected_or_deleted() {
        let mut d = dev_ready();
        d.answer_shell("rm -f --", "");
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            &super::super::smsie_export::tests::export_zip(b"{}\n"),
            0,
            0o644,
        );
        d.add_file(&format!("{DEVICE_TMP_DIR}/unknown.json"), b"[]", 0, 0o644);
        let tmp = tempfile::tempdir().unwrap();
        let CollectState::Records(records) =
            collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap()
        else {
            panic!()
        };
        let (_, sms) = records.iter().find(|(id, _)| id == "sms").unwrap();
        assert_eq!(sms.status, ItemStatus::Done);
        assert!(sms.errors.is_empty());
        assert!(!d.pull_calls.iter().any(|p| p.ends_with("unknown.json")));
        cleanup_exports(&mut d, &[sms.clone()]).unwrap();
        assert!(!d
            .shell_calls
            .iter()
            .any(|c| c.contains("unknown.json") || c.starts_with("rm -rf")));
    }

    #[test]
    fn device_copy_is_removed_when_every_selected_item_is_done() {
        let mut d = dev_ready();
        d.answer_shell("rm -f --", "");
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            &super::super::smsie_export::tests::export_zip(b"{}\n"),
            1700000000,
            0o644,
        );
        let tmp = tempfile::tempdir().unwrap();
        let removed = |d: &FakeADBDevice| d.shell_calls.iter().any(|c| c.starts_with("rm -f --"));
        // 통화 기록도 골랐다면 아직 끝나지 않았다 — 지우지 않는다
        collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap();
        assert!(!removed(&d));
        // 문자만 골랐다면 끝났다 — 개인 데이터 사본을 지운다
        let CollectState::Records(records) = collect(&mut d, tmp.path(), &["sms"]).unwrap() else {
            panic!("records")
        };
        assert!(!removed(&d));
        cleanup_exports(
            &mut d,
            &records
                .into_iter()
                .filter(|(id, _)| id == "sms")
                .map(|(_, rec)| rec)
                .collect::<Vec<_>>(),
        )
        .unwrap();
        assert!(removed(&d));
    }

    #[test]
    fn missing_one_type_is_partial() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            &super::super::smsie_export::tests::export_zip(b"{}\n"),
            1700000000,
            0o644,
        );
        // calls 없음 → calllog 항목 partial + 오류 안내
        let tmp = tempfile::tempdir().unwrap();
        match collect(&mut d, tmp.path(), &["sms", "calllog"]).unwrap() {
            CollectState::Records(recs) => {
                let calls = recs.iter().find(|(id, _)| id == "calllog").unwrap();
                assert_eq!(calls.1.status, ItemStatus::Partial);
                assert!(!calls.1.errors.is_empty());
            }
            _ => panic!(),
        }
    }

    use crate::backup::fake_device::FakeADBDevice;
}
