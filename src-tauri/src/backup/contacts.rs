//! 연락처 백업 — adb 셸에서 Contacts 프로바이더의 연락처 데이터를 읽어 vCard 3.0으로 만든다(자동·완결 게이트 포함).
//! 포함: 이름·전화·이메일·회사/직함·주소·메모·별명·웹사이트·생일 / 미포함: 사진·그룹(라벨)·메신저 계정
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
    let cmd = "content query --uri content://com.android.contacts/contacts --projection _id:";
    let text = crate::device_io::shell(dev, cmd)?;
    let mut ids = Vec::new();
    for line in text.lines() {
        // 형식: Row: 0 _id=57, display_name=홍길동 — "_id=" 이후 값을 콤마까지
        let Some(rest) = line.split_once("_id=") else {
            continue;
        };
        let id = rest.1.split(',').next().unwrap_or("").trim();
        if !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()) {
            ids.push(id.to_string());
        }
    }
    Ok(ids)
}

/// 연락처 데이터 조회 — raw_contact_entities(연락처별 전체 데이터 행).
/// 실측(2026-10-03 XQ-DQ44): 셸에서 contacts/<id>/as_vcard는 "No files supported by provider"로 읽을 수 없고,
/// /data URI는 일부 행만 돌려준다. raw_contact_entities는 295명 전원·1677행을 돌려줬다
const ENTITY_KEYS: &[&str] = &[
    "contact_id",
    "deleted",
    "mimetype",
    "data1",
    "data2",
    "data3",
    "data4",
    "data5",
    "data6",
];

/// content query 출력 → 행별 (키 → 값). 값 안의 개행(메모 등)은 다음 "Row: " 전까지 이어 붙인다.
/// 값 경계는 투영 순서대로 ", <다음 키>=" 위치로 찾는다(값에 쉼표가 있어도 깨지지 않게)
fn parse_rows(text: &str, keys: &[&str]) -> Vec<std::collections::HashMap<String, String>> {
    let mut joined: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with("Row: ") {
            joined.push(line.to_string());
        } else if let Some(last) = joined.last_mut() {
            last.push('\n');
            last.push_str(line);
        }
    }
    let mut rows = Vec::new();
    for row in joined {
        let Some(body) = row.strip_prefix("Row: ").map(|r| {
            r.trim_start_matches(|c: char| c.is_ascii_digit())
                .trim_start()
        }) else {
            continue;
        };
        let mut map = std::collections::HashMap::new();
        let mut rest = body;
        for (i, key) in keys.iter().enumerate() {
            let Some(after) = rest.strip_prefix(&format!("{key}=")) else {
                break;
            };
            let (value, next) = match keys.get(i + 1) {
                Some(nk) => match after.find(&format!(", {nk}=")) {
                    Some(pos) => (&after[..pos], &after[pos + 2..]),
                    None => (after, ""),
                },
                None => (after, ""),
            };
            if value != "NULL" {
                map.insert(key.to_string(), value.to_string());
            }
            rest = next;
        }
        rows.push(map);
    }
    rows
}

/// vCard 3.0 값 이스케이프
fn esc(v: &str) -> String {
    v.replace('\\', "\\\\")
        .replace(',', "\\,")
        .replace(';', "\\;")
        .replace('\n', "\\n")
}

/// 한 연락처의 데이터 행들 → vCard 3.0. 사진·그룹(라벨)·메신저 등은 포함하지 않는다(문서 명시)
fn build_vcard(rows: &[&std::collections::HashMap<String, String>]) -> String {
    let g = |r: &std::collections::HashMap<String, String>, k: &str| {
        r.get(k)
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let mut out = String::from("BEGIN:VCARD\r\nVERSION:3.0\r\n");
    let mut fn_name: Option<String> = None;
    let mut body = String::new();
    for r in rows {
        let mime = r.get("mimetype").map(String::as_str).unwrap_or("");
        match mime {
            "vnd.android.cursor.item/name" => {
                fn_name = g(r, "data1").or(fn_name);
                let n = [g(r, "data3"), g(r, "data2"), g(r, "data5"), g(r, "data4"), g(r, "data6")]
                    .iter()
                    .map(|v| v.as_deref().map(esc).unwrap_or_default())
                    .collect::<Vec<_>>()
                    .join(";");
                body.push_str(&format!("N:{n}\r\n"));
            }
            "vnd.android.cursor.item/phone_v2" => {
                if let Some(num) = g(r, "data1") {
                    let t = match g(r, "data2").as_deref() {
                        Some("1") => "HOME",
                        Some("2") => "CELL",
                        Some("3") => "WORK",
                        Some("4") => "WORK,FAX",
                        Some("5") => "HOME,FAX",
                        Some("6") => "PAGER",
                        Some("7") => "OTHER",
                        _ => "VOICE",
                    };
                    body.push_str(&format!("TEL;TYPE={t}:{}\r\n", esc(&num)));
                }
            }
            "vnd.android.cursor.item/email_v2" => {
                if let Some(v) = g(r, "data1") {
                    let t = match g(r, "data2").as_deref() {
                        Some("1") => "INTERNET,HOME",
                        Some("2") => "INTERNET,WORK",
                        _ => "INTERNET",
                    };
                    body.push_str(&format!("EMAIL;TYPE={t}:{}\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/organization" => {
                if let Some(v) = g(r, "data1") {
                    body.push_str(&format!("ORG:{}\r\n", esc(&v)));
                }
                if let Some(v) = g(r, "data4") {
                    body.push_str(&format!("TITLE:{}\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/postal-address_v2" => {
                if let Some(v) = g(r, "data1") {
                    let t = match g(r, "data2").as_deref() {
                        Some("1") => "HOME",
                        Some("2") => "WORK",
                        _ => "OTHER",
                    };
                    body.push_str(&format!("ADR;TYPE={t}:;;{};;;;\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/note" => {
                if let Some(v) = g(r, "data1") {
                    body.push_str(&format!("NOTE:{}\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/nickname" => {
                if let Some(v) = g(r, "data1") {
                    body.push_str(&format!("NICKNAME:{}\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/website" => {
                if let Some(v) = g(r, "data1") {
                    body.push_str(&format!("URL:{}\r\n", esc(&v)));
                }
            }
            "vnd.android.cursor.item/contact_event"
                // 생일(type 3)만
                if g(r, "data2").as_deref() == Some("3") => {
                    if let Some(v) = g(r, "data1") {
                        body.push_str(&format!("BDAY:{}\r\n", esc(&v)));
                    }
                }
            _ => {}
        }
    }
    // 이름이 없는 연락처는 첫 전화번호로 표시명(FN은 vCard 3.0 필수)
    let fallback = rows
        .iter()
        .find(|r| r.get("mimetype").map(String::as_str) == Some("vnd.android.cursor.item/phone_v2"))
        .and_then(|r| g(r, "data1"))
        .unwrap_or_else(|| "(이름 없음)".into());
    out.push_str(&format!("FN:{}\r\n", esc(&fn_name.unwrap_or(fallback))));
    if !body.contains("\nN:") && !body.starts_with("N:") {
        out.push_str("N:;;;;\r\n");
    }
    out.push_str(&body);
    out.push_str("END:VCARD\r\n");
    out
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
    let fail = |mut rec: ItemRecord, e: String| {
        rec.errors.push(e);
        rec.status = ItemStatus::Partial;
        rec
    };
    let ids = match contact_ids(dev) {
        Ok(ids) => ids,
        Err(e) => return fail(rec, e),
    };
    let cmd = format!(
        "content query --uri content://com.android.contacts/raw_contact_entities --projection {}",
        ENTITY_KEYS.join(":")
    );
    let text = match crate::device_io::shell(dev, &cmd) {
        Ok(text) => text,
        Err(e) => return fail(rec, format!("연락처 데이터 조회 실패: {e}")),
    };
    let rows = parse_rows(&text, ENTITY_KEYS);
    // 연락처별로 묶기 — 삭제 표시된 원시 연락처는 제외
    let mut by_contact: std::collections::BTreeMap<
        u64,
        Vec<&std::collections::HashMap<String, String>>,
    > = Default::default();
    for r in &rows {
        if r.get("deleted").map(String::as_str) == Some("1") {
            continue;
        }
        if let Some(id) = r.get("contact_id").and_then(|v| v.parse::<u64>().ok()) {
            by_contact.entry(id).or_default().push(r);
        }
    }
    let path = match super::paths::write_target(backup_root, "contacts/contacts.vcf") {
        Ok(path) => path,
        Err(e) => return fail(rec, e),
    };
    let dir = path.parent().expect("contacts 부모");
    if let Err(e) = std::fs::create_dir_all(dir) {
        return fail(rec, format!("폴더 생성 실패: {e}"));
    }
    let mut vcf = String::new();
    for id in &ids {
        match id.parse::<u64>().ok().and_then(|n| by_contact.get(&n)) {
            Some(rs) => vcf.push_str(&build_vcard(rs)),
            None => rec
                .errors
                .push(format!("연락처 {id}: 데이터 행을 찾지 못했습니다")),
        }
    }
    let mut writer = match std::fs::File::create(&path) {
        Ok(f) => HashingWriter::new(f),
        Err(e) => return fail(rec, format!("contacts.vcf 생성 실패: {e}")),
    };
    if let Err(e) = writer.write_all(vcf.as_bytes()) {
        return fail(rec, format!("contacts.vcf 기록 실패: {e}"));
    }
    let (file, sha256, bytes) = writer.finish();
    if let Err(e) = file.sync_all() {
        return fail(rec, format!("contacts.vcf 디스크 저장 실패: {e}"));
    }
    drop(file);
    rec.files = 1;
    rec.bytes = bytes;
    rec.artifacts.push("contacts/contacts.vcf".into());
    rec.entries.push(crate::backup::model::FileEntry {
        remote: format!("content://com.android.contacts (연락처 {}명)", ids.len()),
        local: "contacts/contacts.vcf".into(),
        size: bytes,
        mtime: 0,
        sha256: Some(sha256),
        quarantined: false,
        error: None,
    });
    rec.status = if rec.errors.is_empty() {
        ItemStatus::Done
    } else {
        ItemStatus::Partial
    };
    rec
}

/// 복원 확인 — (백업한 연락처 수, 지금 폰의 연락처 수). 폰 쪽은 목록 조회만(읽기 전용)
pub fn restore_check(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<(u64, u64), String> {
    let vcf = std::fs::read_to_string(backup_root.join("contacts").join("contacts.vcf"))
        .map_err(|e| format!("contacts.vcf 읽기 실패: {e}"))?;
    let backed_up = vcf.matches("BEGIN:VCARD").count() as u64;
    let on_device = contact_ids(dev)?.len() as u64;
    Ok((backed_up, on_device))
}

/// 복원 — vcf를 기기에 올리고 연락처 앱 가져오기 안내(수동 1탭).
/// 반환: (올린 경로, 안내 문구) — 실제 가져오기 확인은 사용자 몫(진행 로그에 기록)
pub fn stage_restore_contacts(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
) -> Result<(String, String), String> {
    let src = backup_root.join("contacts").join("contacts.vcf");
    let bytes = std::fs::read(&src).map_err(|e| format!("contacts.vcf 읽기 실패: {e}"))?;
    if bytes.is_empty() {
        return Err("백업된 연락처가 없습니다".into());
    }
    let remote = "/sdcard/contacts-restore.vcf";
    let mut reader = &bytes[..];
    dev.push(&mut reader, &remote)
        .map_err(|e| format!("연락처 파일 전송 실패: {e}"))?;
    let note = "연락처 앱 → 설정(⋮) → 가져오기 → .vcf 파일 → contacts-restore.vcf 선택 (수동 1회)"
        .to_string();
    Ok((remote.to_string(), note))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    const IDS: &str = "Row: 0 _id=57\nRow: 1 _id=63\n";
    const ENTITIES: &str = "Row: 0 contact_id=57, deleted=0, mimetype=vnd.android.cursor.item/name, data1=홍길동, data2=길동, data3=홍, data4=NULL, data5=NULL, data6=NULL\n\
Row: 1 contact_id=57, deleted=0, mimetype=vnd.android.cursor.item/phone_v2, data1=010-0000-0000, data2=2, data3=NULL, data4=NULL, data5=NULL, data6=NULL\n\
Row: 2 contact_id=57, deleted=0, mimetype=vnd.android.cursor.item/note, data1=첫 줄, 쉼표 포함\n둘째 줄, data2=NULL, data3=NULL, data4=NULL, data5=NULL, data6=NULL\n\
Row: 3 contact_id=63, deleted=0, mimetype=vnd.android.cursor.item/phone_v2, data1=02-000-0000, data2=3, data3=NULL, data4=NULL, data5=NULL, data6=NULL\n";

    fn dev() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts --projection _id:",
            IDS,
        );
        d.answer_shell(
            "content query --uri content://com.android.contacts/raw_contact_entities",
            ENTITIES,
        );
        d
    }

    #[test]
    fn builds_vcards_from_entities() {
        let mut d = dev();
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_contacts(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Done, "{:?}", rec.errors);
        let raw = std::fs::read_to_string(tmp.path().join("contacts/contacts.vcf")).unwrap();
        assert_eq!(raw.matches("BEGIN:VCARD").count(), 2);
        assert!(raw.contains("FN:홍길동"));
        assert!(raw.contains("N:홍;길동;;;"));
        assert!(raw.contains("TEL;TYPE=CELL:010-0000-0000"));
        // 쉼표·개행이 들어간 메모도 한 값으로 — 이스케이프
        assert!(raw.contains("NOTE:첫 줄\\, 쉼표 포함\\n둘째 줄"));
        // 이름 없는 연락처는 전화번호로 표시명
        assert!(raw.contains("FN:02-000-0000"));
        assert!(rec.entries[0].sha256.is_some());
    }

    #[test]
    fn missing_contact_data_is_partial() {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            "content query --uri content://com.android.contacts/contacts --projection _id:",
            "Row: 0 _id=1\nRow: 1 _id=99\n",
        );
        d.answer_shell(
            "content query --uri content://com.android.contacts/raw_contact_entities",
            "Row: 0 contact_id=1, deleted=0, mimetype=vnd.android.cursor.item/name, data1=A, data2=NULL, data3=NULL, data4=NULL, data5=NULL, data6=NULL\n",
        );
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_contacts(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Partial);
        assert!(rec.errors[0].contains("99"));
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
