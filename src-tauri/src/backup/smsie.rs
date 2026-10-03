//! SMS Import/Export(tmo1/sms-ie, GPL-3.0) 연동 — 문자·통화 기록(세미수동).
//! 조사(2026-10-03): adb shell의 SMS/통화 프로바이더 직접 접근은 최신 Android에서 차단.
//! 이 앱은 설치(install)·권한(pm grant)·기본 SMS 역할(cmd role)·파일 전송을 PC에서 자동화하고,
//! 앱 안의 내보내기/가져오기 버튼 + 파일 선택(SAF)만 사용자가 누른다(수동 개입 2탭).
//! APK는 GitHub Releases에서 런타임 다운로드(D12 Magisk 패턴) — 번들하지 않는다.
//! 주의(앱 README): 기본 SMS 앱 전환 중 수신 문자 유실 방지를 위해 비행기 모드 안내가 필요하다.

use crate::backup::model::{FileEntry, ItemKind, ItemRecord, ItemStatus};
use crate::backup::quarantine::HashingWriter;
use adb_client::ADBDeviceExt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

pub const SMSIE_PKG: &str = "com.github.tmo1.sms_ie";
/// 기기 측 임시 폴더 — 고정 경로만 rm -rf 한다(그 외 삭제 금지)
pub const DEVICE_TMP_DIR: &str = "/sdcard/xvolte-smsie";
const GH_RELEASES_API: &str = "https://api.github.com/repos/tmo1/sms-ie/releases/latest";
/// 내보내기 산출물 파일명 접두사(앱 규칙) — messages-*.zip / call-logs-*.json
const MESSAGES_PREFIX: &str = "messages";
const CALLLOG_PREFIX: &str = "call-logs";

fn run(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    dev.shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&err).trim()))?;
    if !err.is_empty() {
        return Err(String::from_utf8_lossy(&err).trim().to_string());
    }
    String::from_utf8(out).map_err(|e| format!("출력 해석 실패: {e}"))
}

pub fn installed(dev: &mut dyn ADBDeviceExt) -> Result<bool, String> {
    let out = run(dev, &format!("pm list packages {SMSIE_PKG}"))?;
    Ok(out.contains(SMSIE_PKG))
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
    let tag = json["tag_name"].as_str().unwrap_or("unknown").to_string();
    let mut assets: Vec<(String, String)> = Vec::new();
    if let Some(list) = json["assets"].as_array() {
        for a in list {
            if let (Some(name), Some(url)) = (a["name"].as_str(), a["browser_download_url"].as_str()) {
                assets.push((name.to_string(), url.to_string()));
            }
        }
    }
    // standard flavor의 .apk — legacy(구 Android용)는 제외
    let pick = assets
        .iter()
        .find(|(name, _)| name.ends_with(".apk") && name.contains("standard") && !name.contains("legacy"))
        .or_else(|| assets.iter().find(|(name, _)| name.ends_with(".apk") && !name.contains("legacy")))
        .ok_or("APK 자산을 찾을 수 없습니다")?;
    let dest = cache_dir.join(format!("sms-ie-{tag}.apk"));
    if dest.exists() {
        return Ok(dest); // 캐시 재사용
    }
    let bytes = ureq::get(&pick.1)
        .timeout(std::time::Duration::from_secs(300))
        .call()
        .map_err(|e| format!("APK 다운로드 실패: {e}"))?
        .into_reader()
        .bytes()
        .collect::<Result<Vec<u8>, _>>()
        .map_err(|e| format!("APK 수신 실패: {e}"))?;
    std::fs::create_dir_all(cache_dir).map_err(|e| format!("캐시 폴더 생성 실패: {e}"))?;
    std::fs::write(&dest, &bytes).map_err(|e| format!("APK 저장 실패: {e}"))?;
    Ok(dest)
}

/// 설치 + 권한 + 임시 폴더 준비 (백업·복원 공통)
pub fn prepare(dev: &mut dyn ADBDeviceExt, cache_dir: &Path, download: bool) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    if !installed(dev)? {
        if !download {
            return Err("SMS Import/Export 앱이 설치되어 있지 않습니다".into());
        }
        let apk = download_apk(cache_dir)?;
        dev.install(&apk, None).map_err(|e| format!("앱 설치 실패: {e}"))?;
        log.push(format!("설치: {}", apk.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default()));
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
pub fn collect(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
) -> Result<CollectState, String> {
    let tmp_dir = DEVICE_TMP_DIR.to_string();
    let entries = dev.list(&tmp_dir).map_err(|e| format!("임시 폴더 조회 실패: {e}"))?;
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
    let dir = backup_root.join("smsie");
    std::fs::create_dir_all(&dir).map_err(|e| format!("smsie 폴더 생성 실패: {e}"))?;
    let mut records: Vec<(String, ItemRecord)> = vec![
        ("sms".into(), empty_item("sms")),
        ("calllog".into(), empty_item("calllog")),
    ];
    for (name, size, mtime) in found {
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let local_rel = format!("smsie/{name}");
        let dest = backup_root.join(&local_rel);
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
                drop(file);
                if written != size {
                    let _ = std::fs::remove_file(&dest);
                    entry.error = Some(format!("크기 불일치: 예상 {size}B, 수신 {written}B"));
                } else {
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
                let rec = records.iter_mut().find(|(rid, _)| rid == id).expect("2종 준비됨");
                rec.1.entries.push(entry);
            }
            None => {
                // 알 수 없는 산출물 — sms 항목에 오류로 기록(가져오기 대상 파악 불가)
                let rec = records.iter_mut().find(|(rid, _)| rid == "sms").expect("준비됨");
                rec.1.errors.push(format!("알 수 없는 파일: {name}"));
            }
        }
    }
    for (_, rec) in records.iter_mut() {
        rec.files = rec.entries.len() as u32;
        rec.bytes = rec.entries.iter().filter(|e| e.error.is_none()).map(|e| e.size).sum();
        rec.artifacts = rec.entries.iter().filter(|e| e.error.is_none()).map(|e| e.local.clone()).collect();
        let failed = rec.entries.iter().any(|e| e.error.is_some()) || rec.entries.is_empty();
        rec.status = if failed { ItemStatus::Partial } else { ItemStatus::Done };
        if rec.entries.is_empty() {
            rec.errors.push("이 항목의 산출 파일을 찾지 못했습니다 — 앱에서 해당 데이터를 내보냈는지 확인해 주세요".into());
        }
    }
    // 임시 폴더 정리 — 고정 경로만
    let _ = run(dev, &format!("rm -rf {DEVICE_TMP_DIR}"));
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
pub fn restore_stage(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<String, String> {
    let dir = backup_root.join("smsie");
    let mut names: Vec<String> = Vec::new();
    for e in std::fs::read_dir(&dir).map_err(|e| format!("smsie 폴더 읽기 실패: {e}"))? {
        let name = e.map_err(|e| e.to_string())?.file_name().to_string_lossy().to_string();
        if !name.ends_with(".apk") {
            names.push(name);
        }
    }
    if names.is_empty() {
        return Err("백업된 문자·통화 기록 파일이 없습니다".into());
    }
    run(dev, &format!("mkdir -p {DEVICE_TMP_DIR}"))?;
    for name in &names {
        let bytes = std::fs::read(dir.join(name)).map_err(|e| format!("{name} 읽기 실패: {e}"))?;
        let remote = format!("{DEVICE_TMP_DIR}/{name}");
        let mut reader = &bytes[..];
        dev.push(&mut reader, &remote).map_err(|e| format!("{name} 전송 실패: {e}"))?;
    }
    // 기본 SMS 앱 역할 — 복원 권한의 핵심. 해제는 restore_finish에서
    run(dev, &format!("cmd role add-role-holder android.app.role.SMS {SMSIE_PKG}"))
        .map_err(|e| format!("기본 문자 앱 전환 실패: {e}"))?;
    let list = names.join(", ");
    Ok(format!(
        "폰을 비행기 모드로 전환했는지 확인하세요(전환 중 수신 문자 유실 방지).\nSMS Import/Export 앱에서 Import → {DEVICE_TMP_DIR}의 파일({list})을 선택해 가져오세요. 끝나면 [완료]를 눌러주세요.",
    ))
}

/// 복원 마무리 — 기본 SMS 앱 역할 원복 + 임시 정리. 원복 실패는 안내 문구로 반환
pub fn restore_finish(dev: &mut dyn ADBDeviceExt) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    match run(dev, &format!("cmd role remove-role-holder android.app.role.SMS {SMSIE_PKG}")) {
        Ok(_) => log.push("기본 문자 앱을 원래 앱으로 되돌렸습니다 — 문자 앱을 한 번 열어 확인하세요".into()),
        Err(e) => log.push(format!("기본 문자 앱 원복 실패 — 설정 > 앱 > 기본 앱에서 직접 바꿔주세요: {e}")),
    }
    let _ = run(dev, &format!("rm -rf {DEVICE_TMP_DIR}"));
    log.push("완전한 반영을 위해 문자 앱 설정에서 저장공간/캐시 삭제가 필요할 수 있습니다(앱 캐싱)".into());
    Ok(log)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev_ready() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell(&format!("pm list packages {SMSIE_PKG}"), SMSIE_PKG);
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
        d.add_file(&format!("{DEVICE_TMP_DIR}/messages-2026-10-03.zip"), b"MZzip", 1700000000, 0o644);
        d.add_file(&format!("{DEVICE_TMP_DIR}/call-logs-2026-10-03.json"), b"[]", 1700000001, 0o644);
        let tmp = tempfile::tempdir().unwrap();
        match collect(&mut d, tmp.path()).unwrap() {
            CollectState::Records(recs) => {
                let sms = recs.iter().find(|(id, _)| id == "sms").unwrap();
                let calls = recs.iter().find(|(id, _)| id == "calllog").unwrap();
                assert_eq!(sms.1.status, ItemStatus::Done);
                assert_eq!(calls.1.status, ItemStatus::Done);
                assert_eq!(sms.1.files, 1);
                assert!(tmp.path().join("smsie/messages-2026-10-03.zip").exists());
                assert!(d.shell_calls.iter().any(|c| c.contains("rm -rf /sdcard/xvolte-smsie")));
            }
            _ => panic!("records 여야 함"),
        }
    }

    #[test]
    fn not_ready_when_no_exports() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        let tmp = tempfile::tempdir().unwrap();
        assert!(matches!(collect(&mut d, tmp.path()).unwrap(), CollectState::NotReady));
    }

    #[test]
    fn missing_one_type_is_partial() {
        let mut d = dev_ready();
        d.add_dir(DEVICE_TMP_DIR);
        d.add_file(&format!("{DEVICE_TMP_DIR}/messages-1.zip"), b"z", 1700000000, 0o644);
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
