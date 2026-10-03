//! 연락처 백업 — adb 셸에서 Contacts 프로바이더를 vCard로 직접 수집(자동·완결 게이트 포함).
//! 근거(2026-10-03 조사): content query로 contacts 조회는 최신 Android(Android 14 실측 사례)에서
//! 동작. SMS·통화 기록과 달리 shell이 읽을 수 있다. 복원(쓰기)은 권한이 불확실해
//! vcf를 기기에 올리고 연락처 앱 가져오기로 안내한다(수동 1탭) — 검증 대기 항목.

use crate::backup::model::{ItemKind, ItemRecord, ItemStatus};
use crate::backup::quarantine::HashingWriter;
use adb_client::ADBDeviceExt;
use std::io::Write;
use std::path::Path;

/// 연락처 id 목록 조회 — `content query`의 "Row: 0 _id=57, ..." 형식에서 _id 추출
fn contact_ids(dev: &mut dyn ADBDeviceExt) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    let cmd = "content query --uri content://com.android.contacts/contacts --projection _id:";
    dev.shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| format!("연락처 목록 조회 실패: {e}"))?;
    let text = String::from_utf8_lossy(&out);
    if !err.is_empty() && text.trim().is_empty() {
        return Err(format!("연락처 목록 조회 실패: {}", String::from_utf8_lossy(&err).trim()));
    }
    let mut ids = Vec::new();
    for line in text.lines() {
        // 형식: Row: 0 _id=57, display_name=홍길동 — "_id=" 이후 값을 콤마까지
        let Some(rest) = line.split_once("_id=") else { continue };
        let id = rest.1.split(',').next().unwrap_or("").trim();
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// vCard 1건 조회 — content query 출력 행에서 페이로드를 관대하게 추출.
/// 실측 전 확정 포맷이 없어: "Row: 0 <key>=<payload>" 와 "Row: 0 <payload>" 두 형태 모두 처리하고,
/// BEGIN:VCARD로 시작하지 않으면(이스케이프·포맷 차이) 오류로 기록 — 위장 성공 금지.
fn vcard_payload(row: &str) -> Option<String> {
    let rest = row.strip_prefix("Row: ")?;
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit()).trim_start();
    let payload = match rest.split_once('=') {
        // "key=value" — 키에 공백·콜론이 없고 값이 vCard로 시작할 때만 값으로 본다
        Some((k, v)) if !k.contains(' ') && v.trim_start().starts_with("BEGIN:VCARD") => v.trim_start().to_string(),
        _ if rest.trim_start().starts_with("BEGIN:VCARD") => rest.trim_start().to_string(),
        _ => return None,
    };
    // content query는 개행을 리터럴 "\n"으로 놓을 수 있다 — 실제 개행으로 복원
    Some(payload.replace("\\n", "\n"))
}

pub fn collect_contacts(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> ItemRecord {
    let mut rec = ItemRecord {
        id: "contacts".into(),
        kind: ItemKind::Contacts,
        status: ItemStatus::Pending,
        files: 0,
        bytes: 0,
        entries: vec![],
        artifacts: vec![],
        errors: vec![],
    };
    let ids = match contact_ids(dev) {
        Ok(ids) => ids,
        Err(e) => {
            rec.errors.push(e);
            rec.status = ItemStatus::Partial;
            return rec;
        }
    };
    let dir = backup_root.join("contacts");
    if let Err(e) = std::fs::create_dir_all(&dir) {
        rec.errors.push(format!("폴더 생성 실패: {e}"));
        rec.status = ItemStatus::Partial;
        return rec;
    }
    let vcf_path = dir.join("contacts.vcf");
    let file = match std::fs::File::create(&vcf_path) {
        Ok(f) => f,
        Err(e) => {
            rec.errors.push(format!("contacts.vcf 생성 실패: {e}"));
            rec.status = ItemStatus::Partial;
            return rec;
        }
    };
    let mut writer = HashingWriter::new(file);
    for id in &ids {
        let cmd = format!("content query --uri content://com.android.contacts/contacts/{id}/as_vcard");
        let mut out = Vec::new();
        let mut err = Vec::new();
        if dev.shell_command(&cmd, Some(&mut out), Some(&mut err)).is_err() {
            rec.errors.push(format!("연락처 {id} 조회 실패"));
            continue;
        }
        let text = String::from_utf8_lossy(&out);
        match text.lines().next().and_then(vcard_payload) {
            Some(card) => {
                let _ = writer.write_all(card.as_bytes());
                let _ = writer.write_all(b"\r\n");
            }
            None => {
                let detail = if err.is_empty() {
                    "vCard 형식 아님(포맷 변경 가능성)".to_string()
                } else {
                    String::from_utf8_lossy(&err).trim().to_string()
                };
                rec.errors.push(format!("연락처 {id}: {detail}"));
            }
        }
    }
    let (file, _sha256, bytes) = writer.finish();
    drop(file);
    rec.files = 1;
    rec.bytes = bytes;
    rec.artifacts.push("contacts/contacts.vcf".into());
    if rec.errors.is_empty() && !ids.is_empty() {
        rec.status = ItemStatus::Done;
    } else if ids.is_empty() {
        // 연락처 0명 — 빈 백업도 정상 완결
        rec.status = ItemStatus::Done;
    } else {
        rec.status = ItemStatus::Partial;
    }
    rec
}

/// 복원 — vcf를 기기에 올리고 연락처 앱 가져오기 안내(수동 1탭).
/// 반환: (올린 경로, 안내 문구) — 실제 가져오기 확인은 사용자 몫(진행 로그에 기록)
pub fn stage_restore_contacts(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<(String, String), String> {
    let src = backup_root.join("contacts").join("contacts.vcf");
    let bytes = std::fs::read(&src).map_err(|e| format!("contacts.vcf 읽기 실패: {e}"))?;
    if bytes.is_empty() {
        return Err("백업된 연락처가 없습니다".into());
    }
    let remote = "/sdcard/contacts-restore.vcf";
    let mut reader = &bytes[..];
    dev.push(&mut reader, &remote).map_err(|e| format!("연락처 파일 전송 실패: {e}"))?;
    let note = "연락처 앱 → 설정(⋮) → 가져오기 → .vcf 파일 → contacts-restore.vcf 선택 (수동 1회)".to_string();
    Ok((remote.to_string(), note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    const IDS: &str = "Row: 0 _id=57, display_name=홍길동\nRow: 1 _id=63, display_name=\n";

    #[test]
    fn collects_vcards() {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts --projection _id:",
            IDS,
        );
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts/57/as_vcard",
            "Row: 0 vcard=BEGIN:VCARD\\nVERSION:4.0\\nFN:홍길동\\nEND:VCARD\\n",
        );
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts/63/as_vcard",
            "Row: 0 BEGIN:VCARD\\nVERSION:4.0\\nEND:VCARD\\n",
        );
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_contacts(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Done, "{:?}", rec.errors);
        let raw = std::fs::read_to_string(tmp.path().join("contacts/contacts.vcf")).unwrap();
        assert!(raw.contains("BEGIN:VCARD"));
        assert!(raw.contains("FN:홍길동")); // \\n → 실제 개행 복원 확인
        assert_eq!(raw.matches("BEGIN:VCARD").count(), 2);
    }

    #[test]
    fn unreadable_card_is_partial_not_fake_success() {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts --projection _id:",
            "Row: 0 _id=1, display_name=A\nRow: 1 _id=2, display_name=B\n",
        );
        d.answer_shell("content query --uri content://com.android.contacts/contacts/1/as_vcard", "Row: 0 vcard=garbage-not-vcard");
        d.answer_shell("content query --uri content://com.android.contacts/contacts/2/as_vcard", "Row: 0 vcard=BEGIN:VCARD\nEND:VCARD\n");
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_contacts(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Partial);
        assert_eq!(rec.errors.len(), 1);
    }

    #[test]
    fn provider_denied_is_partial() {
        let mut d = FakeADBDevice::new(); // 목록 조회 자체가 실패
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_contacts(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Partial);
        assert!(!rec.errors.is_empty());
    }
}
