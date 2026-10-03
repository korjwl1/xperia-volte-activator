//! 백업·복구 엔진의 데이터 모델 — manifest.json 구조와 프론트 반환 요약.
//! 원천 정책: tasks/plan.md §6, 설계: .plans/04-engine/backup-engine.md

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// manifest.json 버전 — 구조가 바뀌면 올리고 복구 쪽에서 마이그레이션한다
pub const MANIFEST_VERSION: u32 = 1;

/// 백업 항목 종류
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemKind {
    /// 파일 풀(sdcard 폴더·APK·Android/data) — FileEntry 전수 기록
    Files,
    /// 셸 덤프 산출물(settings 3종·deviceidle·packages)
    Dump,
    /// 연락처 vCard
    Contacts,
    /// SMS Import/Export 산출물(문자·통화 기록)
    SmsIe,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ItemStatus {
    /// 진행 전
    Pending,
    /// 완료 — 오류 0
    Done,
    /// 부분 완료 — 오류 또는 누락 존재(완결 게이트 통과 불가)
    Partial,
    /// 선택에서 빠짐
    Skipped,
}

/// 파일 항목 1개 — 완결 판정의 원자 단위(§6-2 전수 열거)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// 기기 측 절대경로 (예: /sdcard/DCIM/Camera/xxx.jpg)
    pub remote: String,
    /// 백업 루트 상대 로컬 경로 (Windows 구분자)
    pub local: String,
    pub size: u64,
    /// 원본 수정시각(unix 초) — 복원 시 mtime 보존에 사용
    pub mtime: u32,
    /// 전송 중 계산한 해시 — 완결 항목은 항상 존재
    pub sha256: Option<String>,
    /// quarantine segNNN.tar 안에 있음(Windows 비호환 — 로컬 파일 없음)
    pub quarantined: bool,
    /// 이 파일의 백업 실패 사유(있으면 완결 불가)
    pub error: Option<String>,
}

/// 항목 1개의 백업 기록
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRecord {
    /// 항목 id — mock/apps.ts의 id와 동일(dcim, fs-rest, apk, …)
    pub id: String,
    pub kind: ItemKind,
    pub status: ItemStatus,
    /// FileEntry 개수(kind=Files)
    pub files: u32,
    /// 항목 합계 바이트
    pub bytes: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entries: Vec<FileEntry>,
    /// 산출물 상대경로(kind != Files) — settings/*.txt, contacts.vcf, smsie/*
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifacts: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

/// manifest.json — 백업 폴더의 단일 진실 공급원
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub version: u32,
    /// ISO 8601
    pub created: String,
    pub updated: String,
    pub model: String,
    /// 전체 시리얼은 기록하지 않는다(마스킹만 — §12.5)
    pub serial_masked: String,
    pub firmware: String,
    pub android: String,
    pub items: Vec<ItemRecord>,
}

impl Manifest {
    pub fn new(model: &str, serial_masked: &str, firmware: &str, android: &str) -> Self {
        let now = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        Self {
            version: MANIFEST_VERSION,
            created: now.clone(),
            updated: now,
            model: model.to_string(),
            serial_masked: serial_masked.to_string(),
            firmware: firmware.to_string(),
            android: android.to_string(),
            items: Vec::new(),
        }
    }

    /// 전체 완결 여부 — "전수 열거 완료 + 오류 0"(§6-2). 파괴 단계 게이트의 입력.
    /// skipped 항목은 판정에서 제외(선택하지 않은 것은 완결에 포함하지 않는다).
    pub fn complete(&self) -> bool {
        self.items
            .iter()
            .filter(|i| i.status != ItemStatus::Skipped)
            .all(|i| i.status == ItemStatus::Done)
    }

    /// (선택 항목 중) 오류가 있는 항목id → 대표 사유 첫 줄
    pub fn error_summary(&self) -> Vec<String> {
        self.items
            .iter()
            .filter(|i| i.status == ItemStatus::Partial && !i.errors.is_empty())
            .map(|i| format!("{}: {}", i.id, i.errors[0]))
            .collect()
    }

    pub fn total_files(&self) -> u64 {
        self.items.iter().map(|i| i.files as u64).sum()
    }

    pub fn total_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.bytes).sum()
    }

    pub fn record(&mut self, rec: ItemRecord) {
        if let Some(existing) = self.items.iter_mut().find(|i| i.id == rec.id) {
            *existing = rec;
        } else {
            self.items.push(rec);
        }
        self.touch();
    }

    pub fn touch(&mut self) {
        self.updated = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
    }
}

/// 프론트 반환 요약 — 계약 `.plans/02-contracts/tauri-commands.md`
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BackupSummary {
    pub complete: bool,
    pub files: u64,
    pub bytes: u64,
    pub dir: String,
    pub errors: Vec<String>,
    /// 항목별 상태(id → done/partial)
    pub items: Vec<ItemBrief>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemBrief {
    pub id: String,
    pub status: String,
    pub files: u32,
    pub bytes: u64,
}

impl From<&Manifest> for BackupSummary {
    fn from(m: &Manifest) -> Self {
        Self {
            complete: m.complete(),
            files: m.total_files(),
            bytes: m.total_bytes(),
            dir: String::new(), // 호출부에서 채운다
            errors: m.error_summary(),
            items: m
                .items
                .iter()
                .filter(|i| i.status != ItemStatus::Skipped)
                .map(|i| ItemBrief {
                    id: i.id.clone(),
                    status: status_str(i.status),
                    files: i.files,
                    bytes: i.bytes,
                })
                .collect(),
        }
    }
}

pub fn status_str(s: ItemStatus) -> String {
    match s {
        ItemStatus::Pending => "pending".into(),
        ItemStatus::Done => "done".into(),
        ItemStatus::Partial => "partial".into(),
        ItemStatus::Skipped => "skipped".into(),
    }
}

/// manifest를 임시 파일에 쓴 뒤 원자 교체(journal.rs 패턴) — 중단 시 반쪽 파일이 남지 않게
pub fn save_manifest_atomic(manifest: &Manifest, dir: &std::path::Path) -> Result<(), String> {
    use std::io::Write;
    let path = dir.join("manifest.json");
    let tmp = dir.join("manifest.json.tmp");
    let json = serde_json::to_string_pretty(manifest).map_err(|e| format!("manifest 직렬화 실패: {e}"))?;
    let mut f = std::fs::File::create(&tmp).map_err(|e| format!("manifest 임시 파일 생성 실패: {e}"))?;
    f.write_all(json.as_bytes()).map_err(|e| format!("manifest 쓰기 실패: {e}"))?;
    f.sync_all().ok();
    drop(f);
    std::fs::rename(&tmp, &path).map_err(|e| format!("manifest 교체 실패: {e}"))
}

pub fn load_manifest(dir: &std::path::Path) -> Result<Manifest, String> {
    let path = dir.join("manifest.json");
    let raw = std::fs::read_to_string(&path).map_err(|e| format!("manifest 읽기 실패: {e}"))?;
    let m: Manifest = serde_json::from_str(&raw).map_err(|e| format!("manifest 해석 실패: {e}"))?;
    if m.version != MANIFEST_VERSION {
        return Err(format!("지원하지 않는 manifest 버전: {}", m.version));
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: ItemKind, status: ItemStatus, errors: Vec<String>) -> ItemRecord {
        ItemRecord {
            id: id.into(),
            kind,
            status,
            files: 1,
            bytes: 10,
            entries: vec![],
            artifacts: vec!["out.txt".into()],
            errors,
        }
    }

    #[test]
    fn complete_requires_all_done_no_errors() {
        let mut m = Manifest::new("XQ-DQ44", "AB1234****", "67.2.A.3.178", "15");
        assert!(m.complete()); // 항목 없음 = 완결(전부 선택 안 함)
        m.record(item("dcim", ItemKind::Files, ItemStatus::Done, vec![]));
        assert!(m.complete());
        m.record(item("apk", ItemKind::Files, ItemStatus::Partial, vec!["3개 APK 실패".into()]));
        assert!(!m.complete());
        // 부분 상태의 오류는 요약에 보인다
        assert_eq!(m.error_summary().len(), 1);
        m.record(item("sms", ItemKind::SmsIe, ItemStatus::Skipped, vec![]));
        assert!(!m.complete()); // 다른 항목이 partial이면 여전히 불완결
    }

    #[test]
    fn manifest_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let mut m = Manifest::new("XQ-DQ44", "AB1234****", "67.2.A.3.178", "15");
        m.record(item("contacts", ItemKind::Contacts, ItemStatus::Done, vec![]));
        save_manifest_atomic(&m, dir.path()).unwrap();
        let loaded = load_manifest(dir.path()).unwrap();
        assert_eq!(loaded.items.len(), 1);
        assert_eq!(loaded.items[0].id, "contacts");
        assert_eq!(loaded.items[0].kind, ItemKind::Contacts);
        assert!(loaded.complete());
    }
}
