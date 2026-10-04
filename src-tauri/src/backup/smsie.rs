//! SMS Import/Export(tmo1/sms-ie, GPL-3.0) 연동 — 문자·통화 기록(세미수동).
//! 조사(2026-10-03): adb shell의 SMS/통화 프로바이더 직접 접근은 최신 Android에서 차단.
//! 이 앱은 설치(install)·권한(pm grant)·기본 SMS 역할(cmd role)·파일 전송을 PC에서 자동화하고,
//! 앱 안의 내보내기/가져오기 버튼 + 파일 선택(SAF)만 사용자가 누른다(수동 개입 2탭).
//! APK는 GitHub Releases에서 런타임 다운로드(D12 Magisk 패턴) — 번들하지 않는다.
//! 주의(앱 README): 기본 SMS 앱 전환 중 수신 문자 유실 방지를 위해 비행기 모드 안내가 필요하다.

use crate::backup::model::{FileEntry, ItemKind, ItemRecord, ItemStatus};
use crate::backup::quarantine::HashingWriter;
use adb_client::ADBDeviceExt;
use std::io::Read;
use std::path::{Path, PathBuf};

pub const SMSIE_PKG: &str = "com.github.tmo1.sms_ie";
/// 기기 측 임시 폴더 — 고정 경로만 rm -rf 한다(그 외 삭제 금지)
pub const DEVICE_TMP_DIR: &str = "/sdcard/xvolte-smsie";
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
    // 재시도/재시작 때 이미 전환된 역할에서 원래 앱 기록을 덮어쓰지 않는다.
    if record.is_none() {
        super::sms_role::save(state_dir, &key, current.clone())?;
    }
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
}
/// 내보내기 산출물 파일명 접두사(앱 규칙) — messages-*.zip / call-logs-*.json
const MESSAGES_PREFIX: &str = "messages";
const CALLLOG_PREFIX: &str = "call-logs";

fn run(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    crate::device_io::shell(dev, cmd)
}

pub fn installed(dev: &mut dyn ADBDeviceExt) -> Result<bool, String> {
    let out = run(dev, &format!("pm list packages {SMSIE_PKG}"))?;
    Ok(out
        .lines()
        .any(|line| line.trim().strip_prefix("package:") == Some(SMSIE_PKG)))
}

/// GitHub Releases에서 APK 다운로드(캐시) — standard flavor 우선, legacy 제외.
/// 신뢰 기반: HTTPS + github.com (인증서 핀닝은 안 함 — Magisk 다운로드와 동일 기준, 설계문서 참조)
pub fn download_apk(cache_dir: &Path) -> Result<PathBuf, String> {
    let resp = ureq::get(GH_RELEASES_API)
        .timeout(std::time::Duration::from_secs(30))
        .call()
        .map_err(|e| format!("릴리스 조회 실패: {e}"))?;
    let text = resp
        .into_string()
        .map_err(|e| format!("릴리스 정보 수신 실패: {e}"))?;
    let json: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("릴리스 정보 해석 실패: {e}"))?;
    let tag = json["tag_name"]
        .as_str()
        .ok_or("릴리스 버전이 없습니다")?
        .to_string();
    if tag.is_empty()
        || !tag
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return Err("릴리스 버전 형식이 올바르지 않습니다".into());
    }
    let mut assets: Vec<(String, String)> = Vec::new();
    if let Some(list) = json["assets"].as_array() {
        for a in list {
            if let (Some(name), Some(url)) =
                (a["name"].as_str(), a["browser_download_url"].as_str())
            {
                assets.push((name.to_string(), url.to_string()));
            }
        }
    }
    // standard flavor의 .apk — legacy(구 Android용)는 제외
    let pick = assets
        .iter()
        .find(|(name, _)| {
            name.ends_with(".apk") && name.contains("standard") && !name.contains("legacy")
        })
        .or_else(|| {
            assets
                .iter()
                .find(|(name, _)| name.ends_with(".apk") && !name.contains("legacy"))
        })
        .ok_or("APK 자산을 찾을 수 없습니다")?;
    let dest = cache_dir.join(format!("sms-ie-{tag}.apk"));
    if dest.exists() {
        let mut file =
            std::fs::File::open(&dest).map_err(|e| format!("APK 캐시 읽기 실패: {e}"))?;
        let size = file.metadata().map_err(|e| e.to_string())?.len();
        let mut magic = [0u8; 2];
        if (2..=128 * 1024 * 1024).contains(&size)
            && file.read_exact(&mut magic).is_ok()
            && magic == *b"PK"
        {
            return Ok(dest);
        }
    }
    let bytes = ureq::get(&pick.1)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("APK 다운로드 실패: {e}"))?
        .into_reader()
        .take(128 * 1024 * 1024 + 1);
    let mut download = bytes;
    let mut bytes = vec![];
    download
        .read_to_end(&mut bytes)
        .map_err(|e| format!("APK 수신 실패: {e}"))?;
    if bytes.len() > 128 * 1024 * 1024 || !bytes.starts_with(b"PK") {
        return Err("APK 크기/형식이 올바르지 않습니다".into());
    }
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    crate::storage::atomic_write(&dest, &bytes).map_err(|e| format!("APK 저장 실패: {e}"))?;
    Ok(dest)
}

/// 설치 + 권한 + 임시 폴더 준비 (백업·복원 공통)
pub fn prepare(
    dev: &mut dyn ADBDeviceExt,
    cache_dir: &Path,
    download: bool,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    if !installed(dev)? {
        if !download {
            return Err("SMS Import/Export 앱이 설치되어 있지 않습니다".into());
        }
        let apk = download_apk(cache_dir)?;
        let (sha, _) =
            super::verify::hash_reader(std::fs::File::open(&apk).map_err(|e| e.to_string())?)?;
        dev.install(&apk, None)
            .map_err(|e| format!("앱 설치 실패: {e}"))?;
        // 받은 APK의 해시를 기록 — 버전 고정·검증 정책을 정하기 전까지 최소한 무엇을 설치했는지 남긴다
        log.push(format!(
            "설치: {} (sha256 {})",
            apk.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default(),
            sha.get(..16).unwrap_or(&sha)
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
    if lower.starts_with(MESSAGES_PREFIX) {
        Some("sms")
    } else if lower.starts_with(CALLLOG_PREFIX) {
        Some("calllog")
    } else {
        None
    }
}

/// 수집 — 임시 폴더의 파일을 전부 받아(smsie/) 항목 기록 병합. 비었으면 not_ready.
pub fn collect(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<CollectState, String> {
    let tmp_dir = DEVICE_TMP_DIR.to_string();
    let entries = dev
        .list(&tmp_dir)
        .map_err(|e| format!("임시 폴더 조회 실패: {e}"))?;
    let mut found: Vec<(String, u64, u32)> = Vec::new();
    for e in entries {
        if let adb_client::ADBListItemType::File(f) = e {
            if f.name.ends_with(".apk") {
                continue;
            }
            found.push((f.name, f.size as u64, f.time));
        }
    }
    if found.is_empty() {
        return Ok(CollectState::NotReady);
    }
    let target = super::paths::write_target(backup_root, "smsie/export.tmp")?;
    let dir = target.parent().expect("smsie 부모");
    std::fs::create_dir_all(dir).map_err(|e| format!("smsie 폴더 생성 실패: {e}"))?;
    let mut records: Vec<(String, ItemRecord)> = vec![
        ("sms".into(), empty_item("sms")),
        ("calllog".into(), empty_item("calllog")),
    ];
    for (name, size, mtime) in found {
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let local_rel = format!("smsie/{name}");
        let dest = crate::backup::paths::write_target(backup_root, &local_rel)?;
        let file = std::fs::File::create(&dest).map_err(|e| format!("{name} 생성 실패: {e}"))?;
        let mut writer = HashingWriter::new(file);
        let mut entry = FileEntry {
            remote: remote.clone(),
            local: local_rel.clone(),
            size,
            mtime,
            sha256: None,
            quarantined: false,
            error: None,
        };
        match dev.pull(&remote, &mut writer) {
            Ok(()) => {
                let (file, sha256, written) = writer.finish();
                if let Err(e) = file.sync_all() {
                    entry.error = Some(format!("디스크 저장 실패: {e}"));
                }
                drop(file);
                if written != size {
                    let _ = std::fs::remove_file(&dest);
                    entry.error = Some(format!("크기 불일치: 예상 {size}B, 수신 {written}B"));
                } else if entry.error.is_none() {
                    entry.sha256 = Some(sha256);
                }
            }
            Err(e) => {
                drop(writer);
                let _ = std::fs::remove_file(&dest);
                entry.error = Some(format!("전송 실패: {e}"));
            }
        }
        match item_for(&name) {
            Some(id) => {
                let rec = records
                    .iter_mut()
                    .find(|(rid, _)| rid == id)
                    .expect("2종 준비됨");
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
    // 임시 폴더 정리 — 고정 경로만
    if records
        .iter()
        .all(|(_, item)| item.status == ItemStatus::Done)
    {
        let _ = run(dev, &format!("rm -rf {DEVICE_TMP_DIR}"));
    }
    Ok(CollectState::Records(records))
}

pub enum CollectState {
    /// 앱에서 아직 내보내지 않음 — 프론트는 수동 개입 유지
    NotReady,
    /// 항목 기록 — manifest에 병합한다
    Records(Vec<(String, ItemRecord)>),
}

fn empty_item(id: &str) -> ItemRecord {
    ItemRecord {
        id: id.into(),
        kind: ItemKind::SmsIe,
        status: ItemStatus::Pending,
        files: 0,
        bytes: 0,
        entries: vec![],
        artifacts: vec![],
        errors: vec![],
    }
}

/// 복원 준비 — 파일 전송 + 기본 SMS 앱 역할 부여(비행기 모드 안내는 프론트).
/// 반환: 사용자 안내 문구
pub fn restore_stage(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
    items: &[String],
    state_dir: &Path,
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
    let problems = super::verify::verify_manifest(backup_root, &mut manifest);
    if !problems.is_empty() || !manifest.complete() {
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
            names.push(name.to_string());
        }
    }
    if names.is_empty() {
        return Err("백업된 문자·통화 기록 파일이 없습니다".into());
    }
    run(dev, &format!("mkdir -p {DEVICE_TMP_DIR}"))?;
    for name in &names {
        let path = crate::backup::paths::existing_file(backup_root, &format!("smsie/{name}"))?;
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let mut reader = std::fs::File::open(path).map_err(|e| format!("{name} 읽기 실패: {e}"))?;
        dev.push(&mut reader, &remote)
            .map_err(|e| format!("{name} 전송 실패: {e}"))?;
    }
    stage_role(dev, state_dir)?;
    let list = names.join(", ");
    Ok(format!(
        "폰을 비행기 모드로 전환했는지 확인하세요(전환 중 수신 문자 유실 방지).\nSMS Import/Export 앱에서 Import → {DEVICE_TMP_DIR}의 파일({list})을 선택해 가져오세요. 끝나면 [완료]를 눌러주세요.",
    ))
}

/// 복원 마무리 — 기본 SMS 앱 역할 원복 + 임시 정리. 원복 실패는 안내 문구로 반환
pub fn restore_finish(dev: &mut dyn ADBDeviceExt, state_dir: &Path) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    let key = crate::device_io::identity_key(dev)?;
    let prev = super::sms_role::load(state_dir, &key)?
        .ok_or("이 기기의 문자 앱 원복 기록이 없습니다 — 설정에서 기본 문자 앱을 확인해 주세요")?
        .previous_holder;
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
    super::sms_role::remove(state_dir, &key)?;
    let _ = run(dev, &format!("rm -rf {DEVICE_TMP_DIR}"));
    log.push(
        "완전한 반영을 위해 문자 앱 설정에서 저장공간/캐시 삭제가 필요할 수 있습니다(앱 캐싱)"
            .into(),
    );
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

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
        d
    }

    #[test]
    fn collect_maps_files_to_items() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-2026-10-03.zip"),
            b"MZzip",
            1700000000,
            0o644,
        );
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/call-logs-2026-10-03.json"),
            b"[]",
            1700000001,
            0o644,
        );
        let tmp = tempfile::tempdir().unwrap();
        match collect(&mut d, tmp.path()).unwrap() {
            CollectState::Records(recs) => {
                let sms = recs.iter().find(|(id, _)| id == "sms").unwrap();
                let calls = recs.iter().find(|(id, _)| id == "calllog").unwrap();
                assert_eq!(sms.1.status, ItemStatus::Done);
                assert_eq!(calls.1.status, ItemStatus::Done);
                assert_eq!(sms.1.files, 1);
                assert!(tmp.path().join("smsie/messages-2026-10-03.zip").exists());
                assert!(d
                    .shell_calls
                    .iter()
                    .any(|c| c.contains("rm -rf /sdcard/xvolte-smsie")));
            }
            _ => panic!("records 여야 함"),
        }
    }

    #[test]
    fn restore_role_goes_back_to_previous_holder() {
        // 실제 상태가 바뀌는 가짜 역할 서비스로 원복까지 검사한다.
        let mut d = FakeADBDevice::new();
        d.answer_shell("getprop ro.serialno", "FAKE-A");
        d.sms_role_holder = Some(Some("com.google.android.apps.messaging".into()));
        d.answer_shell("mkdir", "");
        d.answer_shell("rm -rf", "");
        let tmp = tempfile::tempdir().unwrap();
        let state = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("smsie")).unwrap();
        std::fs::write(tmp.path().join("smsie/messages-1.zip"), b"z").unwrap();
        let mut manifest = super::super::model::Manifest::new("XQ", "masked", "v", "15");
        let mut item = empty_item("sms");
        item.status = ItemStatus::Done;
        use sha2::Digest;
        item.entries.push(FileEntry {
            remote: format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            local: "smsie/messages-1.zip".into(),
            size: 1,
            mtime: 0,
            sha256: Some(hex::encode(sha2::Sha256::digest(b"z"))),
            quarantined: false,
            error: None,
        });
        manifest.record(item);
        super::super::model::save_manifest_atomic(&manifest, tmp.path()).unwrap();
        std::fs::write(tmp.path().join("smsie/unrecorded.zip"), b"extra").unwrap();
        assert!(restore_stage(
            &mut d,
            tmp.path(),
            &["sms".into(), "calllog".into()],
            state.path()
        )
        .is_err());
        assert!(d.pushed.is_empty());
        restore_stage(&mut d, tmp.path(), &["sms".into()], state.path()).unwrap();
        assert_eq!(d.pushed.len(), 1);
        assert!(!d
            .pushed
            .contains_key(&format!("{DEVICE_TMP_DIR}/unrecorded.zip")));
        let mut disconnected = FakeADBDevice::new();
        assert!(restore_finish(&mut disconnected, state.path()).is_err());
        d.fail_shell.insert("cmd role add-role-holder".into());
        assert!(restore_finish(&mut d, state.path()).is_err());
        d.fail_shell.clear();
        restore_finish(&mut d, state.path()).unwrap();
        assert!(d.shell_calls.iter().any(|c| c
            == "cmd role add-role-holder android.app.role.SMS com.google.android.apps.messaging"));
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
            collect(&mut d, tmp.path()).unwrap(),
            CollectState::NotReady
        ));
    }

    #[test]
    fn unknown_export_cannot_become_complete() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(&format!("{DEVICE_TMP_DIR}/messages-1.zip"), b"z", 0, 0o644);
        d.add_file(&format!("{DEVICE_TMP_DIR}/unknown.json"), b"[]", 0, 0o644);
        let tmp = tempfile::tempdir().unwrap();
        let CollectState::Records(records) = collect(&mut d, tmp.path()).unwrap() else {
            panic!()
        };
        let (_, sms) = records.iter().find(|(id, _)| id == "sms").unwrap();
        assert_eq!(sms.status, ItemStatus::Partial);
        assert!(!sms.errors.is_empty());
    }

    #[test]
    fn missing_one_type_is_partial() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(
            &format!("{DEVICE_TMP_DIR}/messages-1.zip"),
            b"z",
            1700000000,
            0o644,
        );
        // call-logs 없음 → calllog 항목 partial + 오류 안내
        let tmp = tempfile::tempdir().unwrap();
        match collect(&mut d, tmp.path()).unwrap() {
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
