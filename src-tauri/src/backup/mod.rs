//! 백업·복구 엔진 (M3) — plan.md §6, 설계 .plans/04-engine/backup-engine.md
//! 사용자 승인 2026-10-03: 실전 코드 작성 허용, 실기기 테스트 금지(FakeADBDevice 단위 테스트로 검증).
//! 진행 이벤트: 'backup:progress' / 'restore:progress' — 50ms 쓰로틀 + 항목 완료 시 즉시.

pub mod contacts;
pub mod model;
mod omissions;
pub mod paths;
pub mod puller;
pub mod quarantine;
mod recovery;
pub mod restore;
pub mod runner;
pub mod settings;
pub mod sms_role;
pub mod smsie;
mod smsie_export;
pub mod source_metadata;
#[cfg(feature = "dev-cli")]
pub(crate) mod transfer_probe;
pub mod verify;
pub mod walker;
pub mod winname;

#[cfg(test)]
mod access_diagnostics;
#[cfg(test)]
pub mod fake_device;

use crate::events::Events;
use model::BackupSummary;
use puller::CancelFlag;
use serde::Serialize;
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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

use crate::device_io::WriteOperation as Operation;

/// 백업 실행 단위 취소 — 프론트가 실행마다 만든 run_id로 대상을 지정한다.
/// 실행이 시작되기 전에 도착한 취소도 기억해 그 실행은 시작 즉시 멈추고,
/// 끝난(또는 다른) 실행의 취소가 다음 실행에 남지 않는다.
#[derive(Default)]
struct CancelRegistry {
    current: Option<(String, Arc<AtomicBool>)>,
    /// 아직 시작하지 않은 실행에 대한 취소(최근 것만 보관)
    early: VecDeque<String>,
}

const EARLY_CANCEL_KEEP: usize = 16;

impl CancelRegistry {
    fn begin(&mut self, run_id: &str) -> Arc<AtomicBool> {
        let cancelled = match self.early.iter().position(|id| id == run_id) {
            Some(i) => {
                self.early.remove(i);
                true
            }
            None => false,
        };
        let flag = Arc::new(AtomicBool::new(cancelled));
        self.current = Some((run_id.to_string(), Arc::clone(&flag)));
        flag
    }

    fn end(&mut self, run_id: &str) {
        if self.current.as_ref().is_some_and(|(id, _)| id == run_id) {
            self.current = None;
        }
    }

    /// run_id가 없으면 지금 실행 중인 백업만 취소한다
    fn cancel(&mut self, run_id: Option<&str>) {
        match (run_id, &self.current) {
            (None, Some((_, flag))) => flag.store(true, Ordering::Relaxed),
            (None, None) => {}
            (Some(id), Some((current, flag))) if current == id => {
                flag.store(true, Ordering::Relaxed)
            }
            (Some(id), _) => {
                if !self.early.iter().any(|e| e == id) {
                    if self.early.len() == EARLY_CANCEL_KEEP {
                        self.early.pop_front();
                    }
                    self.early.push_back(id.to_string());
                }
            }
        }
    }
}

static BACKUP_CANCELS: Mutex<CancelRegistry> = Mutex::new(CancelRegistry {
    current: None,
    early: VecDeque::new(),
});

fn cancels() -> std::sync::MutexGuard<'static, CancelRegistry> {
    // 잠금 중 패닉이 나도 취소 기록 자체는 유효하다
    BACKUP_CANCELS.lock().unwrap_or_else(|e| e.into_inner())
}

/// 실행이 끝나면(성공·실패·패닉) 현재 실행 등록을 해제한다
struct RunRegistration(String);
impl Drop for RunRegistration {
    fn drop(&mut self) {
        cancels().end(&self.0);
    }
}

/// 진행 이벤트 방출기 — 50ms 쓰로틀(마지막 파일·항목 전환은 즉시)
struct Emitter50ms {
    app: Events,
    event: &'static str,
    last: Instant,
    /// 마지막으로 보낸 (항목, 단계) — 바뀌면 쓰로틀 없이 바로 보낸다(다음 항목 시작이 묻히지 않게)
    last_key: Option<(String, &'static str)>,
}

impl Emitter50ms {
    fn new(app: Events, event: &'static str) -> Self {
        Self {
            app,
            event,
            last: Instant::now() - Duration::from_secs(1),
            last_key: None,
        }
    }
    fn emit(&mut self, p: runner::StepProgress) {
        let last_file = p.files_done == p.files_total && p.files_total > 0;
        let now = Instant::now();
        let terminal = matches!(p.phase, "done" | "partial" | "pending");
        let changed = self
            .last_key
            .as_ref()
            .is_none_or(|(id, phase)| *id != p.item_id || *phase != p.phase);
        if !changed && !last_file && !terminal && now - self.last < Duration::from_millis(50) {
            return;
        }
        self.last = now;
        if changed {
            self.last_key = Some((p.item_id.clone(), p.phase));
        }
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

#[tauri::command]
pub async fn backup_run(
    app: tauri::AppHandle,
    serial: Option<String>,
    items: Vec<String>,
    dest: String,
    resume_dir: Option<String>,
    run_id: String,
) -> Result<BackupSummary, String> {
    backup_run_with_events(
        Events::desktop(app),
        serial,
        items,
        dest,
        resume_dir,
        run_id,
    )
    .await
}

pub(crate) async fn backup_run_with_events(
    app: Events,
    serial: Option<String>,
    items: Vec<String>,
    dest: String,
    resume_dir: Option<String>,
    run_id: String,
) -> Result<BackupSummary, String> {
    let dest_path = PathBuf::from(&dest);
    if dest.trim().is_empty() || !dest_path.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다 — 먼저 지정해 주세요".into());
    }
    if run_id.trim().is_empty() {
        return Err("백업 실행 식별값이 필요합니다".into());
    }
    let operation = Operation::acquire()?;
    // 실행권을 얻은 뒤 등록 — 시작 전에 온 이 실행의 취소는 그대로 반영된다
    let cancel = CancelFlag::from_shared(cancels().begin(&run_id));
    let registration = RunRegistration(run_id);
    let emitter = Emitter50ms::new(app.clone(), "backup:progress");
    let resume = resume_dir.filter(|d| !d.is_empty()).map(PathBuf::from);

    let work = move || {
        let _operation = operation;
        let _registration = registration;
        let mut emitter = emitter;
        let mut sink: runner::ProgressSink = Box::new(move |p| emitter.emit(p));
        crate::adb::with_first_device(&serial, |dev| {
            runner::run_backup_items(
                dev,
                &items,
                &dest_path,
                resume.as_deref(),
                &cancel,
                &mut sink,
            )
        })
    };
    crate::tasks::blocking("백업 작업", work).await
}

/// 백업 시작 — 지정 폴더 아래 시작 시각 기준 폴더를 만들고 절대 경로를 반환(진행 기록에 먼저 저장)
#[tauri::command]
pub async fn backup_prepare(serial: Option<String>, dest: String) -> Result<PreparedBackup, String> {
    let dest_path = PathBuf::from(&dest);
    if dest.trim().is_empty() || !dest_path.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다 — 먼저 지정해 주세요".into());
    }
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        crate::adb::with_first_device(&serial, |dev| runner::prepare_backup_root(dev, &dest_path))
            .map(|(dir, existing)| PreparedBackup {
                dir: dir.to_string_lossy().to_string(),
                existing,
            })
    };
    crate::tasks::blocking("백업 준비", work).await
}

/// 백업 폴더 준비 결과 — existing이면 같은 폰의 기존 백업을 이어서 갱신한다
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PreparedBackup {
    pub dir: String,
    pub existing: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContactsCheck {
    pub backed_up: u64,
    pub on_device: u64,
}

/// 연락처 가져오기 확인 — 백업한 수와 지금 폰의 수 비교용
#[tauri::command]
pub async fn contacts_restore_check(
    serial: Option<String>,
    dir: String,
) -> Result<ContactsCheck, String> {
    let backup_dir = PathBuf::from(&dir);
    let work = move || {
        crate::adb::with_first_device(&serial, |dev| contacts::restore_check(dev, &backup_dir)).map(
            |(backed_up, on_device)| ContactsCheck {
                backed_up,
                on_device,
            },
        )
    };
    crate::tasks::blocking("연락처 확인", work).await
}

/// Import has been acknowledged and counted before deleting the fixed phone-side temporary.
#[tauri::command]
pub async fn contacts_restore_finish(serial: Option<String>, dir: String) -> Result<(), String> {
    let operation = Operation::acquire()?;
    crate::tasks::blocking("연락처 임시 파일 정리", move || {
        let _operation = operation;
        let dir = PathBuf::from(dir);
        crate::adb::with_first_device(&serial, |dev| {
            verify::verify_device(dev, &model::load_manifest(&dir)?)?;
            let (saved, current) = contacts::restore_check(dev, &dir)?;
            if current < saved {
                return Err("연락처 가져오기가 완료되지 않았습니다".into());
            }
            contacts::finish_restore_contacts(dev)
        })
    })
    .await
}

/// run_id를 주면 그 실행만(시작 전이면 시작 즉시) 취소, 없으면 지금 실행 중인 백업을 취소
#[tauri::command]
pub async fn backup_cancel(run_id: Option<String>) -> Result<(), String> {
    cancels().cancel(run_id.as_deref().filter(|id| !id.is_empty()));
    Ok(())
}

/// 이 앱이 만든 백업 폴더인지 확인한 뒤 통째로 지운다(되돌릴 수 없음).
/// 조건: 실제 폴더(심볼릭 링크·정션 아님) · 이름이 `backup-` 로 시작 · 유효한 manifest.json.
/// 사용자가 고른 상위 폴더나 다른 폴더를 잘못 넘겨도 지우지 않는다.
fn delete_backup_dir(dir: &Path) -> Result<(), String> {
    // 상대 경로는 프로세스 작업 폴더 기준으로 풀리므로 받지 않는다
    if !dir.is_absolute() {
        return Err("백업 폴더 경로가 올바르지 않습니다".into());
    }
    let meta = std::fs::symlink_metadata(dir).map_err(|e| format!("백업 폴더 확인 실패: {e}"))?;
    if meta.file_type().is_symlink() || !meta.is_dir() {
        return Err("백업 폴더가 아닙니다".into());
    }
    let is_backup_name = dir
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(runner::is_backup_dir_name);
    if !is_backup_name {
        return Err("이 앱이 만든 백업 폴더(xva-<모델>-backup)만 삭제할 수 있습니다".into());
    }
    model::load_manifest(dir)
        .map_err(|e| format!("백업 폴더로 확인되지 않아 삭제하지 않습니다 — {e}"))?;
    // manifest를 맨 마지막에 지운다 — 중간에 잠긴 파일로 실패해도 다시 시도할 때 백업 폴더로 확인된다
    let entries = std::fs::read_dir(dir).map_err(|e| format!("백업 폴더 읽기 실패: {e}"))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("백업 폴더 읽기 실패: {e}"))?;
        if entry.file_name() == "manifest.json" {
            continue;
        }
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|e| format!("백업 폴더 읽기 실패: {e}"))?;
        // remove_dir_all은 링크·정션을 따라가지 않고 링크 자체만 지운다
        let removed = if kind.is_dir() || kind.is_symlink() && path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        removed.map_err(|e| {
            format!(
                "백업 폴더 삭제 실패({}): {e}",
                entry.file_name().to_string_lossy()
            )
        })?;
    }
    std::fs::remove_file(dir.join("manifest.json"))
        .and_then(|_| std::fs::remove_dir(dir))
        .map_err(|e| format!("백업 폴더 삭제 실패: {e}"))
}

/// 완료 화면 [백업 파일 삭제] — 사용자 확인 후에만 호출(사용자 승인 2026-10-04)
#[tauri::command]
pub async fn backup_delete(dir: String) -> Result<(), String> {
    // 다른 기기 변경 작업(백업·복원·fastboot·Magisk)이 진행 중이면 지우지 않는다(전역 실행권)
    let operation = Operation::acquire()?;
    crate::tasks::blocking("백업 삭제", move || {
        let _operation = operation;
        delete_backup_dir(Path::new(&dir))
    })
    .await
}

/// 기존 백업 폴더 완결 검사 — 백업을 건너뛰고 파괴 단계로 갈 때 게이트의 입력 (§3-3)
#[tauri::command]
pub async fn backup_manifest_check(
    app: tauri::AppHandle,
    dir: String,
    run_id: Option<String>,
) -> Result<Option<BackupSummary>, String> {
    backup_manifest_check_with_events(Events::desktop(app), dir, run_id).await
}

pub(crate) async fn backup_manifest_check_with_events(
    events: Events,
    dir: String,
    run_id: Option<String>,
) -> Result<Option<BackupSummary>, String> {
    let path = PathBuf::from(&dir);
    if !path.is_dir() {
        return Ok(None);
    }
    let operation = Operation::acquire()?;
    let run_id = run_id.unwrap_or_else(|| {
        format!(
            "verify-{}",
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        )
    });
    let cancel = CancelFlag::from_shared(cancels().begin(&run_id));
    let registration = RunRegistration(run_id);
    crate::tasks::blocking("백업 검사", move || {
        let _operation = operation;
        let _registration = registration;
        let mut emitter = Emitter50ms::new(events, "backup:progress");
        let mut sink: runner::ProgressSink = Box::new(move |p| emitter.emit(p));
        verify::backup_summary_progress(&path, &cancel, &mut sink).map(Some)
    })
    .await
}

/// PC-only repair of an already-finished backup; does not contact the phone.
#[cfg(feature = "dev-cli")]
pub(crate) async fn backup_clean_unreadable_apps(dir: String) -> Result<BackupSummary, String> {
    let _operation = Operation::acquire()?;
    let root = PathBuf::from(dir);
    let mut manifest = model::load_manifest(&root)?;
    omissions::clean(&root, &mut manifest)?;
    let mut summary = BackupSummary::from(&manifest);
    summary.dir = root.to_string_lossy().into_owned();
    Ok(summary)
}

#[cfg(feature = "dev-cli")]
pub(crate) async fn backup_metadata_enrich(
    events: Events,
    serial: String,
    dir: String,
) -> Result<Vec<source_metadata::Receipt>, String> {
    let _operation = Operation::acquire()?;
    crate::tasks::blocking("원본 속성 수집", move || {
        let mut sink: runner::ProgressSink = Box::new(move |p| {
            let _ = events.emit(
                "backup:progress",
                ProgressPayload {
                    item_id: p.item_id,
                    phase: p.phase.into(),
                    file: p.file,
                    files_done: p.files_done,
                    files_total: p.files_total,
                    bytes_done: p.bytes_done,
                    bytes_total: p.bytes_total,
                },
            );
        });
        crate::adb::with_first_device(&Some(serial), |dev| {
            source_metadata::enrich(dev, Path::new(&dir), &CancelFlag::new(), &mut sink)
        })
    })
    .await
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmsIeOutcome {
    /// false = 앱에서 아직 내보내지 않음(수동 개입 유지)
    pub ready: bool,
    pub summary: Option<BackupSummary>,
    pub cleanup_warning: Option<String>,
}

/// Detection never transfers or validates bodies, and never changes the manifest.
#[tauri::command]
pub async fn smsie_probe(serial: Option<String>, backup_dir: String) -> Result<bool, String> {
    let _operation = Operation::acquire()?;
    crate::tasks::blocking("문자 파일 감지", move || {
        crate::adb::with_first_device(&serial, |dev| {
            let manifest = model::load_manifest(Path::new(&backup_dir))?;
            let key = crate::device_io::identity_key(dev)?;
            if manifest.device_key.as_deref() != Some(&key) {
                return Err("문자 백업의 원본 기기가 다릅니다".into());
            }
            let selected: Vec<_> = manifest
                .items
                .iter()
                .filter(|i| i.kind == model::ItemKind::SmsIe && manifest.is_selected(i))
                .map(|i| i.id.as_str())
                .collect();
            smsie::probe(dev, &selected)
        })
    })
    .await
}

/// SMS Import/Export 준비 — 설치·권한·임시 폴더 (download=false면 미설치 오류)
#[tauri::command]
pub async fn smsie_prepare(serial: Option<String>, download: bool) -> Result<Vec<String>, String> {
    let cache = crate::app_paths::data_dir()
        .map(|d| d.join("cache").join("smsie"))
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        // 다운로드(수십 초)는 USB 연결을 잡지 않은 채로 한다 — 그동안 기기 조회가 막히지 않게
        let installed = crate::adb::with_first_device(&serial, |dev| smsie::installed(dev))?;
        let apk = if !installed && download {
            Some(smsie::download_apk(&cache)?)
        } else {
            None
        };
        crate::adb::with_first_device(&serial, |dev| {
            let mut logs = smsie::prepare(dev, apk.as_ref())?;
            smsie::open_export_app(dev)?;
            logs.push(
                "폰에서 SMS Import/Export 앱 열림 — 내장 저장소/volte_sms_backup에 저장해 주세요"
                    .into(),
            );
            Ok(logs)
        })
    };
    crate::tasks::blocking("준비", work).await
}

/// SMS Import/Export 수집 — 임시 폴더의 산출물을 manifest에 병합
#[tauri::command]
pub async fn smsie_collect(
    serial: Option<String>,
    backup_dir: String,
    confirm_complete: Option<bool>,
) -> Result<SmsIeOutcome, String> {
    let dir = PathBuf::from(&backup_dir);
    if !dir.is_dir() {
        return Err("백업 폴더를 찾을 수 없습니다".into());
    }
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        crate::adb::with_first_device(&serial, |dev| {
            collect_smsie_backup(dev, &dir, confirm_complete == Some(true))
        })
    };
    crate::tasks::blocking("수집", work).await
}

fn collect_smsie_backup(
    dev: &mut dyn adb_client::ADBDeviceExt,
    dir: &Path,
    confirm_complete: bool,
) -> Result<SmsIeOutcome, String> {
    // 백업에 고른 문자·통화 기록 항목만 병합한다(정리 판단도 이 기준)
    let mut manifest = model::load_manifest(dir)?;
    let key = crate::device_io::identity_key(dev)?;
    if manifest.device_key.as_deref() != Some(&key) {
        return Err("문자 백업의 원본 기기와 현재 기기가 일치하지 않습니다".into());
    }
    let selected: Vec<String> = manifest
        .items
        .iter()
        .filter(|item| item.kind == model::ItemKind::SmsIe && manifest.is_selected(item))
        .map(|item| item.id.clone())
        .collect();
    let selected_ids: Vec<&str> = selected.iter().map(String::as_str).collect();
    if !selected_ids.is_empty()
        && selected_ids.iter().all(|id| {
            manifest
                .items
                .iter()
                .any(|i| i.id == *id && i.status == model::ItemStatus::Done && i.errors.is_empty())
        })
    {
        verify::verify_selected(dir, &mut manifest, &selected);
        if selected.iter().any(|id| {
            manifest
                .items
                .iter()
                .any(|i| &i.id == id && i.status != model::ItemStatus::Done)
        }) {
            return Err("저장된 문자 백업 파일이 변경됐습니다 — 백업을 다시 실행하세요".into());
        }
        let mut summary = BackupSummary::from(&manifest);
        summary.dir = dir.to_string_lossy().into_owned();
        return Ok(SmsIeOutcome {
            ready: true,
            summary: Some(summary),
            cleanup_warning: None,
        });
    }
    match smsie::collect(dev, dir, &selected_ids)? {
        smsie::CollectState::NotReady => Ok(SmsIeOutcome {
            ready: false,
            summary: None,
            cleanup_warning: None,
        }),
        smsie::CollectState::Records(records) => {
            let ready = smsie::selected_done(&records, &selected_ids);
            for (_, mut rec) in records {
                if selected.contains(&rec.id) {
                    if rec.status == model::ItemStatus::Done && (!ready || !confirm_complete) {
                        rec.status = model::ItemStatus::Partial;
                        rec.errors.push("폰 앱의 내보내기 성공 확인 대기".into());
                    }
                    manifest.record(rec);
                }
            }
            model::save_manifest_atomic(&manifest, dir)?;
            // The phone copy survives until the receipt is durable.
            let cleanup_warning = if ready && confirm_complete {
                smsie::cleanup_exports(
                    dev,
                    &manifest
                        .items
                        .iter()
                        .filter(|i| selected.contains(&i.id))
                        .cloned()
                        .collect::<Vec<_>>(),
                )
                .err()
            } else {
                None
            };
            let mut summary = BackupSummary::from(&manifest);
            summary.dir = dir.to_string_lossy().to_string();
            Ok(SmsIeOutcome {
                ready,
                summary: Some(summary),
                cleanup_warning,
            })
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcomeOut {
    pub logs: Vec<String>,
    pub failures: Vec<String>,
    /// 문자·통화 기록(smsie) 수동 복원이 남아 있는지 — 프론트 수동 개입 표시용
    pub smsie_pending: bool,
    /// 연락처 가져오기가 폰에 남았는지 — 이미 계정 동기화로 있으면 false(가져오기 단계 생략)
    pub contacts_pending: bool,
}

/// 자동 복구 실행 — APK 재설치 → tar 스트리밍 → 설정 → 연락처 전송 (smsie는 수동 단계로)
#[tauri::command]
pub async fn restore_run(
    app: tauri::AppHandle,
    serial: Option<String>,
    dir: String,
    items: Vec<String>,
) -> Result<RestoreOutcomeOut, String> {
    restore_run_with_events(Events::desktop(app), serial, dir, items).await
}

pub(crate) async fn restore_run_with_events(
    app: Events,
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
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        crate::adb::with_first_device(&serial, |dev| {
            let out = restore::run_restore(dev, &backup_dir, &items, &sink);
            Ok(RestoreOutcomeOut {
                logs: out.logs,
                failures: out.failures,
                // 무결성 검증에서 멈췄다면 수동 문자 복원을 이어 안내하지 않는다
                smsie_pending: smsie_selected && out.verified,
                contacts_pending: out.contacts_pending,
            })
        })
    };
    crate::tasks::blocking("복구", work).await
}

/// 문자·통화 기록 복원 준비 — 파일 전송 + 기본 문자 앱 역할(비행기 모드 안내 문구 반환)
#[tauri::command]
pub async fn smsie_restore_stage(
    serial: Option<String>,
    dir: String,
    items: Vec<String>,
) -> Result<String, String> {
    let backup_dir = PathBuf::from(&dir);
    let state_dir = crate::app_paths::data_dir()
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?
        .join("sms-role");
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        let installed = crate::adb::with_first_device(&serial, |dev| smsie::installed(dev))?;
        let apk = if installed {
            None
        } else {
            let cache = state_dir
                .parent()
                .ok_or("앱 데이터 폴더 없음")?
                .join("cache/smsie");
            Some(smsie::download_apk(&cache)?)
        };
        crate::adb::with_first_device(&serial, |dev| {
            verify::verify_device(dev, &model::load_manifest(&backup_dir)?)?;
            smsie::restore_stage(dev, &backup_dir, &items, &state_dir, apk.as_ref())
        })
    };
    crate::tasks::blocking("준비", work).await
}

/// 문자·통화 기록 복원 마무리 — 기본 문자 앱 역할 원복 + 임시 정리(안내 로그 반환)
#[tauri::command]
pub async fn smsie_restore_finish(serial: Option<String>) -> Result<Vec<String>, String> {
    let state_dir = crate::app_paths::data_dir()
        .ok_or("앱 데이터 폴더를 확인할 수 없습니다")?
        .join("sms-role");
    let operation = Operation::acquire()?;
    let work = move || {
        let _operation = operation;
        crate::adb::with_first_device(&serial, |dev| smsie::restore_finish(dev, &state_dir))
    };
    crate::tasks::blocking("마무리", work).await
}

/// 로그·manifest에 남기는 문자열 정리 — 백업 항목 경로는 사용자 파일명뿐이라 추가 스크럽 불필요
pub(crate) fn scrub(s: &str) -> String {
    s.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contacts_json_is_ignored_without_changing_vcard_or_blocking_sms() {
        let root = tempfile::tempdir().unwrap();
        let mut dev = fake_device::FakeADBDevice::new();
        dev.answer_shell("getprop ro.serialno", "phone-A");
        dev.answer_shell("rm -f --", "");
        dev.add_dir(smsie::DEVICE_TMP_DIR);
        dev.add_file(
            &format!("{}/messages.zip", smsie::DEVICE_TMP_DIR),
            &smsie_export::tests::export_zip(b"{}\n"),
            0,
            0o644,
        );
        let contact_path = format!("{}/contacts-2027-01-02 (1).json", smsie::DEVICE_TMP_DIR);
        dev.add_file(&contact_path, b"[{", 0, 0o644);
        std::fs::create_dir(root.path().join("contacts")).unwrap();
        let vcard = b"BEGIN:VCARD\nEND:VCARD\n";
        std::fs::write(root.path().join("contacts/contacts.vcf"), vcard).unwrap();
        let mut manifest = model::Manifest::new("XQ", "masked", "v", "15");
        manifest.device_key = Some(crate::boot_image::sha256(b"phone-A"));
        let mut contacts = model::ItemRecord::new("contacts", model::ItemKind::Contacts);
        contacts.status = model::ItemStatus::Done;
        contacts.artifacts.push("contacts/contacts.vcf".into());
        contacts.entries.push(model::FileEntry {
            remote: "content://contacts".into(),
            local: "contacts/contacts.vcf".into(),
            size: vcard.len() as u64,
            mtime: 0,
            sha256: Some(crate::boot_image::sha256(vcard)),
            quarantined: false,
            error: None,
        });
        manifest.record(contacts);
        manifest.record(model::ItemRecord::new("sms", model::ItemKind::SmsIe));
        model::save_manifest_atomic(&manifest, root.path()).unwrap();
        assert!(
            collect_smsie_backup(&mut dev, root.path(), true)
                .unwrap()
                .ready
        );
        let saved = model::load_manifest(root.path()).unwrap();
        assert!(saved.complete());
        let contacts = saved
            .items
            .iter()
            .find(|item| item.id == "contacts")
            .unwrap();
        assert_eq!(contacts.entries.len(), 1);
        assert!(!dev.pull_calls.contains(&contact_path));
        assert!(!root
            .path()
            .join("smsie/contacts-2027-01-02 (1).json")
            .exists());
        assert!(verify::item_problems(root.path(), contacts).is_empty());
        assert_eq!(
            std::fs::read(root.path().join("contacts/contacts.vcf")).unwrap(),
            vcard
        );
    }

    #[test]
    fn adversarial_polling_cannot_complete_backup_or_delete_phone_copy_without_confirmation() {
        let root = tempfile::tempdir().unwrap();
        let mut dev = fake_device::FakeADBDevice::new();
        dev.answer_shell("getprop ro.serialno", "phone-A");
        dev.answer_shell("rm -rf", "");
        dev.add_dir(smsie::DEVICE_TMP_DIR);
        let zip = smsie_export::tests::export_zip(b"{}\n");
        dev.add_file(
            &format!("{}/messages.zip", smsie::DEVICE_TMP_DIR),
            &zip,
            0,
            0o644,
        );
        let mut manifest = model::Manifest::new("XQ", "masked", "v", "15");
        manifest.device_key = Some(crate::boot_image::sha256(b"phone-A"));
        manifest.record(model::ItemRecord::new("sms", model::ItemKind::SmsIe));
        manifest.record(model::ItemRecord::new("calllog", model::ItemKind::SmsIe));
        model::save_manifest_atomic(&manifest, root.path()).unwrap();
        assert!(
            !collect_smsie_backup(&mut dev, root.path(), false)
                .unwrap()
                .ready
        );
        assert!(!model::load_manifest(root.path()).unwrap().complete());
        dev.add_file(
            &format!("{}/calls.json", smsie::DEVICE_TMP_DIR),
            b"[]",
            0,
            0o644,
        );
        let outcome = collect_smsie_backup(&mut dev, root.path(), false).unwrap();
        assert!(outcome.ready);
        assert!(!outcome.summary.unwrap().complete);
        assert!(!dev.shell_calls.iter().any(|c| c.starts_with("rm -f --")));
        assert!(!model::load_manifest(root.path()).unwrap().complete());
        // Cleanup failure is a warning after the durable backup, not data loss or failed backup.
        dev.fail_shell.insert("rm -f --".into());
        let outcome = collect_smsie_backup(&mut dev, root.path(), true).unwrap();
        assert!(outcome.ready && outcome.summary.unwrap().complete);
        assert!(outcome.cleanup_warning.is_some());
        assert!(model::load_manifest(root.path()).unwrap().complete());
    }

    #[test]
    fn cancel_before_start_applies_only_to_that_run() {
        let mut reg = CancelRegistry::default();
        // 준비~시작 사이에 온 취소는 그 실행이 시작되자마자 반영된다
        reg.cancel(Some("run-1"));
        let first = reg.begin("run-1");
        assert!(first.load(Ordering::Relaxed));
        reg.end("run-1");
        // 이전 실행의 취소가 다음 실행에 남지 않는다
        let second = reg.begin("run-2");
        assert!(!second.load(Ordering::Relaxed));
        // 다른 실행 대상 취소는 지금 실행을 멈추지 않는다
        reg.cancel(Some("run-old"));
        assert!(!second.load(Ordering::Relaxed));
        // 식별값 없는 취소는 지금 실행 중인 것만
        reg.cancel(None);
        assert!(second.load(Ordering::Relaxed));
        reg.end("run-2");
        reg.cancel(None);
        assert!(reg.current.is_none());
    }

    #[test]
    fn only_app_backup_folders_with_a_manifest_are_deleted() {
        let parent = tempfile::tempdir().unwrap();
        // 사용자가 고른 상위 폴더 — 이름이 backup- 아님
        assert!(delete_backup_dir(parent.path()).is_err());
        assert!(parent.path().exists());
        // 이름은 맞지만 manifest 없음
        let bare = parent.path().join("backup-20261004-000000-XQ-DQ44");
        std::fs::create_dir(&bare).unwrap();
        std::fs::write(bare.join("photo.jpg"), b"x").unwrap();
        assert!(delete_backup_dir(&bare).is_err());
        assert!(bare.join("photo.jpg").exists());
        // 이 앱이 만든 백업 폴더
        let real = parent.path().join("backup-20261004-000001-XQ-DQ44");
        std::fs::create_dir(&real).unwrap();
        model::save_manifest_atomic(
            &model::Manifest::new("XQ-DQ44", "AB1234****", "v1", "15"),
            &real,
        )
        .unwrap();
        std::fs::write(real.join("photo.jpg"), b"x").unwrap();
        std::fs::create_dir_all(real.join("sdcard/DCIM/Camera")).unwrap();
        std::fs::write(real.join("sdcard/DCIM/Camera/a.jpg"), b"y").unwrap();
        // 상대 경로·일반 파일은 거부
        assert!(delete_backup_dir(Path::new("backup-20261004-000001-XQ-DQ44")).is_err());
        assert!(delete_backup_dir(&real.join("photo.jpg")).is_err());
        assert!(real.join("photo.jpg").exists());
        delete_backup_dir(&real).unwrap();
        assert!(!real.exists());
        assert!(parent.path().exists());
    }

    #[test]
    fn early_cancels_are_bounded() {
        let mut reg = CancelRegistry::default();
        for i in 0..100 {
            reg.cancel(Some(&format!("never-started-{i}")));
        }
        assert_eq!(reg.early.len(), EARLY_CANCEL_KEEP);
    }
}
