//! 백업·복구 엔진 (M3) — plan.md §6, 설계 .plans/04-engine/backup-engine.md
//! 사용자 승인 2026-10-03: 실전 코드 작성 허용, 실기기 테스트 금지(FakeADBDevice 단위 테스트로 검증).
//! 진행 이벤트: 'backup:progress' / 'restore:progress' — 50ms 쓰로틀 + 항목 완료 시 즉시.

pub mod contacts;
pub mod model;
pub mod puller;
pub mod quarantine;
pub mod restore;
pub mod runner;
pub mod settings;
pub mod smsie;
pub mod walker;
pub mod winname;

#[cfg(test)]
pub mod fake_device;

use model::BackupSummary;
use puller::CancelFlag;
use serde::Serialize;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tauri::Emitter;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressPayload {
    pub item_id: String,
    pub phase: String,
    pub file: Option<String>,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// 전역 취소 플래그 — backup_run 시작 시 리셋, backup_cancel로 설정
static BACKUP_CANCEL: OnceLock<Arc<AtomicBool>> = OnceLock::new();

fn shared_cancel() -> Arc<AtomicBool> {
    Arc::clone(BACKUP_CANCEL.get_or_init(|| Arc::new(AtomicBool::new(false))))
}

/// 진행 이벤트 방출기 — 50ms 쓰로틀(마지막 파일·항목 전환은 즉시)
struct Emitter50ms {
    app: tauri::AppHandle,
    event: &'static str,
    last: Instant,
}

impl Emitter50ms {
    fn new(app: tauri::AppHandle, event: &'static str) -> Self {
        Self { app, event, last: Instant::now() - Duration::from_secs(1) }
    }
    fn emit(&mut self, p: runner::StepProgress) {
        let last_file = p.files_done == p.files_total && p.files_total > 0;
        let now = Instant::now();
        if !last_file && now - self.last < Duration::from_millis(50) {
            return;
        }
        self.last = now;
        let payload = ProgressPayload {
            item_id: p.item_id,
            phase: p.phase.to_string(),
            file: p.file,
            files_done: p.files_done,
            files_total: p.files_total,
            bytes_done: p.bytes_done,
            bytes_total: p.bytes_total,
        };
        let _ = self.app.emit(self.event, payload);
    }
}

/// 기존 백업 폴더 검사(완결 게이트용) — manifest 로드 + 파일 존재·크기 확인 + 항목당 최대 3파일 해시 대조.
/// 검사만 한다 — 사용자 백업 폴더의 manifest는 바꾸지 않는다
fn verify_backup_dir(dir: &std::path::Path) -> Result<BackupSummary, String> {
    let mut manifest = model::load_manifest(dir)?;
    let mut problems: Vec<String> = Vec::new();
    use sha2::{Digest, Sha256};
    for item in manifest.items.iter_mut() {
        if item.status == model::ItemStatus::Skipped {
            continue;
        }
        let mut hashed = 0u32;
        let mut item_bad = false;
        for e in item.entries.iter_mut() {
            if e.error.is_some() {
                continue;
            }
            if e.quarantined {
                continue; // 세그먼트 존재는 아래에서 한 번만 확인
            }
            let path = dir.join(&e.local);
            match std::fs::metadata(&path) {
                Ok(m) if m.len() == e.size => {}
                Ok(m) => {
                    e.error = Some(format!("크기 불일치: manifest {}B, 실제 {}B", e.size, m.len()));
                }
                Err(err) => {
                    e.error = Some(format!("파일 없음: {err}"));
                }
            }
            if e.error.is_none() && hashed < 3 {
                if let Ok(data) = std::fs::read(&path) {
                    let got = hex::encode(Sha256::digest(&data));
                    if Some(got.as_str()) != e.sha256.as_deref() {
                        e.error = Some("해시 불일치(내용 변조·손상)".into());
                    }
                }
                hashed += 1;
            }
            if let Some(err) = &e.error {
                item_bad = true;
                problems.push(format!("{}: {err}", e.remote));
            }
        }
        // 이 항목에서 문제가 있을 때만 (앞 항목의 문제가 뒤 항목을 미완결로 만들지 않게)
        if item_bad && item.status == model::ItemStatus::Done {
            item.status = model::ItemStatus::Partial;
        }
        if !item.artifacts.is_empty() {
            for a in &item.artifacts {
                if !dir.join(a).exists() {
                    item.status = model::ItemStatus::Partial;
                    problems.push(format!("산출물 없음: {a}"));
                }
            }
        }
    }
    // quarantine 세그먼트 존재 확인
    let qdir = dir.join("quarantine");
    if qdir.exists() {
        let ok = std::fs::read_dir(&qdir)
            .map(|rd| rd.filter_map(|e| e.ok()).any(|e| e.file_name().to_string_lossy().ends_with(".tar")))
            .unwrap_or(false);
        if !ok {
            problems.push("quarantine 세그먼트가 비었거나 손상되었습니다".into());
        }
    }
    let mut summary = BackupSummary::from(&manifest);
    if !problems.is_empty() {
        summary.complete = false;
        summary.errors.extend(problems);
    }
    summary.dir = dir.to_string_lossy().to_string();
    Ok(summary)
}

#[tauri::command]
pub async fn backup_run(
    app: tauri::AppHandle,
    serial: Option<String>,
    items: Vec<String>,
    dest: String,
    resume_dir: Option<String>,
) -> Result<BackupSummary, String> {
    let dest_path = PathBuf::from(&dest);
    if dest.trim().is_empty() || !dest_path.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다 — 먼저 지정해 주세요".into());
    }
    let cancel_arc = shared_cancel();
    cancel_arc.store(false, Ordering::Relaxed);
    let cancel = CancelFlag::from_shared(cancel_arc);
    let emitter = Emitter50ms::new(app.clone(), "backup:progress");
    let resume = resume_dir.filter(|d| !d.is_empty()).map(PathBuf::from);

    let work = move || {
        let mut emitter = emitter;
        let mut sink: runner::ProgressSink = Box::new(move |p| emitter.emit(p));
        crate::adb::with_first_device(&serial, |dev| {
            runner::run_backup_items(dev, &items, &dest_path, resume.as_deref(), &cancel, &mut sink)
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("백업 작업 스레드 오류: {e}")),
    }
}

/// 백업 시작 — 지정 폴더 아래 시작 시각 기준 폴더를 만들고 절대 경로를 반환(진행 기록에 먼저 저장)
#[tauri::command]
pub async fn backup_prepare(serial: Option<String>, dest: String) -> Result<String, String> {
    let dest_path = PathBuf::from(&dest);
    if dest.trim().is_empty() || !dest_path.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다 — 먼저 지정해 주세요".into());
    }
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| runner::prepare_backup_root(dev, &dest_path))
            .map(|p| p.to_string_lossy().to_string())
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("백업 준비 스레드 오류: {e}")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsCheck {
    pub backed_up: u64,
    pub on_device: u64,
}

/// 연락처 가져오기 확인 — 백업한 수와 지금 폰의 수 비교용
#[tauri::command]
pub async fn contacts_restore_check(serial: Option<String>, dir: String) -> Result<ContactsCheck, String> {
    let backup_dir = PathBuf::from(&dir);
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| contacts::restore_check(dev, &backup_dir))
            .map(|(backed_up, on_device)| ContactsCheck { backed_up, on_device })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("연락처 확인 스레드 오류: {e}")),
    }
}

#[tauri::command]
pub async fn backup_cancel() -> Result<(), String> {
    shared_cancel().store(true, Ordering::Relaxed);
    Ok(())
}

/// 기존 백업 폴더 완결 검사 — 백업을 건너뛰고 파괴 단계로 갈 때 게이트의 입력 (§3-3)
#[tauri::command]
pub async fn backup_manifest_check(dir: String) -> Result<Option<BackupSummary>, String> {
    let path = PathBuf::from(&dir);
    if !path.is_dir() {
        return Ok(None);
    }
    verify_backup_dir(&path).map(Some)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsIeOutcome {
    /// false = 앱에서 아직 내보내지 않음(수동 개입 유지)
    pub ready: bool,
    pub summary: Option<BackupSummary>,
}

/// SMS Import/Export 준비 — 설치·권한·임시 폴더 (download=false면 미설치 오류)
#[tauri::command]
pub async fn smsie_prepare(serial: Option<String>, download: bool) -> Result<Vec<String>, String> {
    let cache = crate::adb::app_data_dir()
        .map(|d| d.join("cache").join("smsie"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| smsie::prepare(dev, &cache, download))
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("준비 스레드 오류: {e}")),
    }
}

/// SMS Import/Export 수집 — 임시 폴더의 산출물을 manifest에 병합
#[tauri::command]
pub async fn smsie_collect(serial: Option<String>, backup_dir: String) -> Result<SmsIeOutcome, String> {
    let dir = PathBuf::from(&backup_dir);
    if !dir.is_dir() {
        return Err("백업 폴더를 찾을 수 없습니다".into());
    }
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| -> Result<SmsIeOutcome, String> {
            match smsie::collect(dev, &dir)? {
                smsie::CollectState::NotReady => Ok(SmsIeOutcome { ready: false, summary: None }),
                smsie::CollectState::Records(records) => {
                    let mut manifest = model::load_manifest(&dir)?;
                    for (_, rec) in records {
                        manifest.record(rec);
                    }
                    model::save_manifest_atomic(&manifest, &dir)?;
                    let mut summary = BackupSummary::from(&manifest);
                    summary.dir = dir.to_string_lossy().to_string();
                    Ok(SmsIeOutcome { ready: true, summary: Some(summary) })
                }
            }
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("수집 스레드 오류: {e}")),
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcomeOut {
    pub logs: Vec<String>,
    pub failures: Vec<String>,
    /// 문자·통화 기록(smsie) 수동 복원이 남아 있는지 — 프론트 수동 개입 표시용
    pub smsie_pending: bool,
}

/// 자동 복구 실행 — APK 재설치 → tar 스트리밍 → 설정 → 연락처 전송 (smsie는 수동 단계로)
#[tauri::command]
pub async fn restore_run(
    app: tauri::AppHandle,
    serial: Option<String>,
    dir: String,
    items: Vec<String>,
) -> Result<RestoreOutcomeOut, String> {
    let backup_dir = PathBuf::from(&dir);
    if !backup_dir.is_dir() {
        return Err("백업 폴더를 찾을 수 없습니다".into());
    }
    let emitter = std::sync::Mutex::new(Emitter50ms::new(app.clone(), "restore:progress"));
    let sink: restore::RestoreSink = std::sync::Arc::new(move |p| {
        if let Ok(mut e) = emitter.lock() {
            e.emit(p);
        }
    });
    let smsie_selected = items.iter().any(|i| i == "sms" || i == "calllog");
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| {
            let out = restore::run_restore(dev, &backup_dir, &items, &sink);
            Ok(RestoreOutcomeOut {
                logs: out.logs,
                failures: out.failures,
                smsie_pending: smsie_selected,
            })
        })
    };
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("복구 스레드 오류: {e}")),
    }
}

/// 문자·통화 기록 복원 준비 — 파일 전송 + 기본 문자 앱 역할(비행기 모드 안내 문구 반환)
#[tauri::command]
pub async fn smsie_restore_stage(serial: Option<String>, dir: String) -> Result<String, String> {
    let backup_dir = PathBuf::from(&dir);
    let work = move || crate::adb::with_first_device(&serial, |dev| smsie::restore_stage(dev, &backup_dir));
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("준비 스레드 오류: {e}")),
    }
}

/// 문자·통화 기록 복원 마무리 — 기본 문자 앱 역할 원복 + 임시 정리(안내 로그 반환)
#[tauri::command]
pub async fn smsie_restore_finish(serial: Option<String>) -> Result<Vec<String>, String> {
    let work = move || crate::adb::with_first_device(&serial, smsie::restore_finish);
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(r) => r,
        Err(e) => Err(format!("마무리 스레드 오류: {e}")),
    }
}

/// 로그·manifest에 남기는 문자열 정리 — 백업 항목 경로는 사용자 파일명뿐이라 추가 스크럽 불필요
pub(crate) fn scrub(s: &str) -> String {
    s.to_string()
}
