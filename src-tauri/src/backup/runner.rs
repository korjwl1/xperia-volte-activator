//! 백업 실행 오케스트레이션 — 항목 id를 받아 수집기·풀러를 차례로 돌리고 manifest를 유지한다.
//! 이 파일의 함수는 전부 `&mut dyn ADBDeviceExt`를 받는다(단위 테스트 주입용).
//! 진행 이벤트는 콜백(Sink)으로만 올리고, emit은 mod.rs의 Tauri 명령 층에서 한다.

use crate::backup::contacts;
use crate::backup::model::{
    load_manifest, save_manifest_atomic, BackupSummary, ItemKind, ItemRecord, ItemStatus, Manifest,
};
use crate::backup::puller::{pull_item_files, CancelFlag, PullFile, PullProgress};
use crate::backup::quarantine::Quarantine;
use crate::backup::settings;
use crate::backup::walker::{self};
use crate::backup::winname::SeenPaths;
use adb_client::ADBDeviceExt;
use std::path::{Path, PathBuf};

/// 진행 알림 — mod.rs에서 이벤트 페이로드로 변환
#[derive(Clone)]
pub struct StepProgress {
    pub item_id: String,
    pub phase: &'static str,
    pub file: Option<String>,
    pub files_done: u64,
    pub files_total: u64,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

impl StepProgress {
    /// 파일·바이트 없이 단계 진행만 알리는 보고
    pub fn at(item_id: &str, phase: &'static str, done: u64, total: u64) -> Self {
        Self {
            item_id: item_id.to_string(),
            phase,
            file: None,
            files_done: done,
            files_total: total,
            bytes_done: 0,
            bytes_total: 0,
        }
    }

    pub fn start(item_id: &str, phase: &'static str, total: u64) -> Self {
        Self::at(item_id, phase, 0, total)
    }
}

pub type ProgressSink<'a> = Box<dyn FnMut(StepProgress) + Send + 'a>;

/// 항목 id → 기록 종류
fn item_kind(id: &str) -> ItemKind {
    match id {
        "settings-all" => ItemKind::Dump,
        "contacts" => ItemKind::Contacts,
        "sms" | "calllog" => ItemKind::SmsIe,
        _ => ItemKind::Files,
    }
}

/// 파일 항목 수집에 공통으로 쓰는 실행 상태
struct TreeCtx<'c, 's> {
    dev: &'c mut dyn ADBDeviceExt,
    root: &'c Path,
    seen: &'c mut SeenPaths,
    quarantine: &'c mut Quarantine,
    cancel: &'c CancelFlag,
    on_progress: &'c mut ProgressSink<'s>,
}

impl TreeCtx<'_, '_> {
    fn pull(
        &mut self,
        id: &str,
        files: &[PullFile],
        local_for: &dyn Fn(&PullFile) -> PathBuf,
    ) -> ItemRecord {
        let on_progress = &mut *self.on_progress;
        pull_item_files(
            self.dev,
            id,
            ItemKind::Files,
            files,
            self.root,
            local_for,
            self.seen,
            self.quarantine,
            self.cancel,
            |p| on_progress(copy_progress(id, &p)),
        )
    }

    /// 기기 폴더 하나를 전수 열거해 받는 항목 — `local`은 기기 경로 → 백업 루트 상대 경로
    fn tree_item(
        &mut self,
        id: &str,
        remote_root: &str,
        skip: &dyn Fn(&str) -> bool,
        local: &dyn Fn(&str) -> String,
    ) -> ItemRecord {
        let walker::WalkResult {
            files,
            skipped,
            errors,
        } = walker::walk(self.dev, remote_root, skip);
        let files: Vec<PullFile> = files.into_iter().map(PullFile::plain).collect();
        let mut rec = self.pull(id, &files, &|pf| PathBuf::from(local(&pf.entry.remote)));
        merge_walk(&mut rec, errors, &skipped);
        rec
    }
}

/// APK 항목 — 패키지 설치 폴더마다 .apk만. 목록 조회 실패는 이 항목의 오류(다른 항목은 계속)
fn collect_apks(ctx: &mut TreeCtx, id: &str) -> ItemRecord {
    let dirs = match package_install_dirs(ctx.dev) {
        Ok(dirs) => dirs,
        Err(e) => {
            return ItemRecord::new(id, ItemKind::Files).fail(format!("앱 목록 조회 실패: {e}"))
        }
    };
    let mut files: Vec<PullFile> = Vec::new();
    let mut walk_errors: Vec<String> = Vec::new();
    for (pkg, dir) in dirs {
        let w = walker::walk(ctx.dev, &dir, &|_| false);
        walk_errors.extend(w.errors);
        files.extend(
            w.files
                .into_iter()
                .filter(|f| f.remote.ends_with(".apk"))
                .map(|entry| PullFile {
                    entry,
                    tag: pkg.clone(),
                }),
        );
    }
    let mut rec = ctx.pull(id, &files, &|pf| {
        let name = pf.entry.remote.rsplit('/').next().unwrap_or("apk");
        PathBuf::from(format!("apks/{}/{}", pf.tag, name))
    });
    rec.errors.extend(walk_errors);
    rec.finalize();
    rec
}

pub fn validate_items(items: &[String]) -> Result<(), String> {
    if items.is_empty() {
        return Err("백업/복구 항목을 선택해 주세요".into());
    }
    let mut seen = std::collections::HashSet::new();
    for id in items {
        let known = matches!(
            id.as_str(),
            "settings-all" | "contacts" | "sms" | "calllog" | "apk" | "app-data" | "fs-rest"
        ) || walker::NAMED_FILE_ITEMS.iter().any(|(name, _)| name == id);
        if !known || !seen.insert(id) {
            return Err(format!("잘못되거나 중복된 백업/복구 항목: {id}"));
        }
    }
    Ok(())
}

fn mask_serial(serial: &str) -> String {
    let n = serial.chars().count();
    if n > 6 {
        format!("{}****", serial.chars().take(6).collect::<String>())
    } else {
        serial.to_string()
    }
}

/// manifest 메타데이터 — 원본 시리얼은 마스킹값과 해시로만 남긴다
pub struct DeviceMeta {
    pub model: String,
    pub firmware: String,
    pub android: String,
    pub serial_masked: String,
    /// SHA-256(ro.serialno) — device_io::identity_key와 같은 값
    pub device_key: Option<String>,
}

/// getprop 덤프에서 manifest 메타데이터 수집
pub fn device_meta(dev: &mut dyn ADBDeviceExt) -> Result<DeviceMeta, String> {
    use sha2::{Digest, Sha256};
    let text = crate::device_io::shell(dev, "getprop")?;
    let props = crate::adb::parse_getprop(&text);
    let prop = |key: &str| props.get(key).cloned().unwrap_or_default();
    let serial = prop("ro.serialno");
    let serial = serial.trim();
    Ok(DeviceMeta {
        model: prop("ro.product.model"),
        firmware: prop("ro.build.id"),
        android: prop("ro.build.version.release"),
        serial_masked: mask_serial(serial),
        device_key: (!serial.is_empty()).then(|| hex::encode(Sha256::digest(serial.as_bytes()))),
    })
}

/// `pm list packages -3 -f`에서 (패키지, 설치 폴더) 목록 — APK 항목의 소스
fn package_install_dirs(dev: &mut dyn ADBDeviceExt) -> Result<Vec<(String, String)>, String> {
    let text = crate::device_io::shell(dev, "pm list packages -3 -f")?;
    let mut pairs = Vec::new();
    for line in text.lines() {
        // package:/data/app/~~x/com.pkg-y/base.apk=com.pkg
        let Some(rest) = line.trim().strip_prefix("package:") else {
            continue;
        };
        let Some((path, pkg)) = rest.rsplit_once('=') else {
            continue;
        };
        let Some((dir, _)) = path.rsplit_once('/') else {
            continue;
        };
        if pkg.contains('.') && dir.starts_with('/') {
            pairs.push((pkg.to_string(), dir.to_string()));
        }
    }
    Ok(pairs)
}

/// 백업 폴더명 — backup-<ts>-<model> (모델은 파일명 안전 문자만)
pub fn backup_dir_name(model: &str) -> String {
    let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let safe: String = model
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("backup-{ts}-{safe}")
}

/// 백업 시작 — 지정 폴더 아래에 시작 시각 기준 폴더(backup-<시각>-<모델>)를 만들고 빈 manifest를 저장한다.
/// 반환 경로는 절대 경로 — 진행 기록에 그대로 저장해 끊겨도 같은 폴더로 이어서 받는다
pub fn prepare_backup_root(dev: &mut dyn ADBDeviceExt, dest: &Path) -> Result<PathBuf, String> {
    let dest = std::path::absolute(dest).map_err(|e| format!("백업 위치 확인 실패: {e}"))?;
    if !dest.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다".into());
    }
    let meta = device_meta(dev)?;
    let name = backup_dir_name(&meta.model);
    let root = dest.join(&name);
    // 같은 초에 두 번 시작해도 기존 백업 manifest를 덮어쓰지 않는다.
    let mut root = root;
    for suffix in 0..1000 {
        let candidate = if suffix == 0 {
            root.clone()
        } else {
            dest.join(format!("{name}-{suffix}"))
        };
        match std::fs::create_dir(&candidate) {
            Ok(()) => {
                root = candidate;
                break;
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && suffix < 999 => continue,
            Err(e) => return Err(format!("백업 폴더 생성 실패: {e}")),
        }
    }
    let mut manifest = Manifest::new(
        &meta.model,
        &meta.serial_masked,
        &meta.firmware,
        &meta.android,
    );
    manifest.device_key = meta.device_key;
    save_manifest_atomic(&manifest, &root)?;
    Ok(root)
}

/// 열거 결과를 풀 결과에 병합 — 열거 오류·스킵 기록도 항목 완결 판정에 들어간다(§6-2 전수 열거)
fn merge_walk(rec: &mut ItemRecord, errors: Vec<String>, skipped: &[walker::Skip]) {
    rec.errors.extend(errors);
    for s in skipped {
        rec.errors
            .push(format!("{}: 백업 제외({})", s.remote, s.reason));
    }
    if !rec.errors.is_empty() {
        rec.status = ItemStatus::Partial;
    }
}

fn copy_progress(item_id: &str, p: &PullProgress) -> StepProgress {
    StepProgress {
        item_id: item_id.to_string(),
        phase: "copy",
        file: Some(p.file.to_string()),
        files_done: p.files_done,
        files_total: p.files_total,
        bytes_done: p.bytes_done,
        bytes_total: p.bytes_total,
    }
}

/// 자동 항목 백업 실행 — sms/calllog는 smsie 별도 흐름이 채운다(Pending으로 남김 → 완결 게이트 닫힘).
/// `resume_dir`가 있으면 그 폴더의 manifest를 이어 쓴다(실패한 백업의 재시도). 항목 완료마다 원자 저장.
pub fn run_backup_items(
    dev: &mut dyn ADBDeviceExt,
    items: &[String],
    dest: &Path,
    resume_dir: Option<&Path>,
    cancel: &CancelFlag,
    on_progress: &mut ProgressSink,
) -> Result<BackupSummary, String> {
    validate_items(items)?;
    let root = match resume_dir {
        Some(dir) => {
            // 재개 — manifest가 있는 폴더만 허용(그 외 덮어쓰기 방지)
            load_manifest(dir).map_err(|e| format!("이어서 진행할 수 없습니다 — {e}"))?;
            std::path::absolute(dir).map_err(|e| format!("백업 폴더 확인 실패: {e}"))?
        }
        None => prepare_backup_root(dev, dest)?,
    };
    let mut manifest = load_manifest(&root)?;
    if resume_dir.is_some() {
        let meta = device_meta(dev)?;
        // 마스킹 시리얼은 앞 6자리만 같아도 일치하므로, 기록된 기기 해시가 있으면 그것으로 확인한다
        let same_device = match (&manifest.device_key, &meta.device_key) {
            (Some(saved), Some(current)) => saved == current,
            (Some(_), None) => false,
            (None, _) => {
                !meta.serial_masked.is_empty() && meta.serial_masked == manifest.serial_masked
            }
        };
        if meta.model.is_empty() || meta.model != manifest.model || !same_device {
            return Err(
                "백업 원본 기기와 현재 기기가 일치하지 않아 이어서 진행할 수 없습니다".into(),
            );
        }
        // 예전 manifest는 이번 확인부터 해시로 이어 간다
        if manifest.device_key.is_none() {
            manifest.device_key = meta.device_key;
        }
    }
    // 첫 항목 전에 취소돼도 선택한 모든 항목이 Pending으로 남아야 한다.
    for item in &mut manifest.items {
        if !items.contains(&item.id) {
            item.status = ItemStatus::Skipped;
        } else if item.status == ItemStatus::Skipped {
            // Re-enumerate reselected items; a previously skipped Pending item is not Done.
            item.status = ItemStatus::Pending;
        }
    }
    if resume_dir.is_some() {
        crate::backup::verify::verify_selected(&root, &mut manifest, items);
    }
    for id in items {
        if !manifest.items.iter().any(|item| item.id == *id) {
            manifest.record(ItemRecord::new(id, item_kind(id)));
        }
    }
    save_manifest_atomic(&manifest, &root)?;
    let mut quarantine = Quarantine::new(&root)?;
    let mut seen = SeenPaths::new();

    for id in items {
        if cancel.cancelled() {
            break;
        }
        // 이미 완료된 항목은 다시 받지 않는다(§9-2 재개 프로브 — 항목 단위)
        if let Some(prev) = manifest.items.iter().find(|i| i.id == *id) {
            if prev.status == ItemStatus::Done {
                continue;
            }
        }
        let retained = manifest
            .items
            .iter()
            .find(|i| i.id == *id)
            .filter(|i| i.kind == ItemKind::Files || i.kind == ItemKind::SmsIe)
            .map(|i| super::verify::retained_entries(&root, i))
            .unwrap_or_default();
        let mut ctx = TreeCtx {
            dev: &mut *dev,
            root: &root,
            seen: &mut seen,
            quarantine: &mut quarantine,
            cancel,
            on_progress: &mut *on_progress,
        };
        let mut rec: ItemRecord = match id.as_str() {
            "settings-all" => {
                (ctx.on_progress)(StepProgress::start(id, "settings", 5));
                settings::collect_settings(ctx.dev, &root)
            }
            "contacts" => {
                (ctx.on_progress)(StepProgress::start(id, "contacts", 0));
                contacts::collect_contacts(ctx.dev, &root)
            }
            // SMS Import/Export 세미수동 — smsie_prepare/collect가 채운다
            "calllog" | "sms" => ItemRecord::new(id, ItemKind::SmsIe),
            "apk" => collect_apks(&mut ctx, id),
            "app-data" => ctx.tree_item(id, walker::ANDROID_DATA_ROOT, &|_| false, &|remote| {
                let rest = remote
                    .strip_prefix("/sdcard/Android/data/")
                    .unwrap_or(remote);
                format!("android-data/{rest}")
            }),
            "fs-rest" => {
                ctx.tree_item(id, walker::FS_REST_ROOT, &walker::fs_rest_skip, &|remote| {
                    let rest = remote.strip_prefix("/sdcard/").unwrap_or(remote);
                    format!("sdcard/__rest__/{rest}")
                })
            }
            named => {
                let Some((_, dir)) = walker::NAMED_FILE_ITEMS
                    .iter()
                    .find(|(nid, _)| *nid == named)
                else {
                    continue; // 계약에 없는 항목 — 무시
                };
                let root_path = format!("/sdcard/{dir}");
                ctx.tree_item(named, &root_path, &|_| false, &|remote| {
                    let rest = remote.strip_prefix("/sdcard/").unwrap_or(remote);
                    format!("sdcard/{rest}")
                })
            }
        };
        for entry in retained {
            if let Some(index) = rec.entries.iter().position(|e| e.remote == entry.remote) {
                if rec.entries[index].error.is_some() {
                    // Keep the good snapshot's hash/provenance while recording the failed retry in errors.
                    rec.bytes += entry.size;
                    rec.entries[index] = entry;
                }
            } else {
                rec.bytes += entry.size;
                rec.entries.push(entry);
            }
        }
        rec.files = rec.entries.len() as u32;
        // A pending SMS export still needs the export collector's content validation.
        if rec.kind == ItemKind::Files {
            rec.finalize();
        }
        let progress = StepProgress {
            item_id: id.clone(),
            phase: match rec.status {
                ItemStatus::Done => "done",
                ItemStatus::Partial => "partial",
                _ => "pending",
            },
            file: None,
            files_done: rec.files as u64,
            files_total: rec.files as u64,
            bytes_done: rec.bytes,
            bytes_total: rec.bytes,
        };
        manifest.record(rec);
        save_manifest_atomic(&manifest, &root)?;
        on_progress(progress);
    }
    let segments = quarantine.finish()?;
    if segments > 0 {
        manifest.touch();
        save_manifest_atomic(&manifest, &root)?;
    }
    let mut summary = BackupSummary::from(&manifest);
    summary.dir = root.to_string_lossy().to_string();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    fn dev_full() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.add_dir("/sdcard");
        d.add_dir("/sdcard/DCIM");
        d.add_file("/sdcard/DCIM/a.jpg", b"photo", 1700000000, 0o644);
        d.add_dir("/sdcard/Android/data/com.kakao.talk");
        d.add_file(
            "/sdcard/Android/data/com.kakao.talk/db",
            b"db",
            1700000001,
            0o600,
        );
        d.add_dir("/data/app/~~x/com.example.app-y");
        d.add_file(
            "/data/app/~~x/com.example.app-y/base.apk",
            b"PK",
            1700000002,
            0o644,
        );
        d.answer_shell("getprop", "[ro.product.model]: [XQ-DQ44]\n[ro.build.id]: [67.2.A.3.178]\n[ro.build.version.release]: [15]\n[ro.serialno]: [AB1234CDEFGH]\n");
        d.answer_shell(
            "pm list packages -3 -f",
            "package:/data/app/~~x/com.example.app-y/base.apk=com.example.app\n",
        );
        d.answer_shell("settings list system", "screen_brightness=31\n");
        d.answer_shell("settings list global", "a=1\n");
        d.answer_shell("settings list secure", "sysui_qs_tiles=internet\n");
        d.answer_shell("dumpsys deviceidle whitelist", "+com.example.app\n");
        d
    }

    #[test]
    fn full_run_creates_complete_manifest() {
        let mut d = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let progress_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counter = progress_calls.clone();
        let mut sink: ProgressSink<'_> = Box::new(move |_| {
            counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        });
        let items = vec![
            "settings-all".to_string(),
            "apk".to_string(),
            "app-data".to_string(),
            "dcim".to_string(),
        ];
        let summary = run_backup_items(
            &mut d,
            &items,
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(summary.complete, "errors: {:?}", summary.errors);
        assert!(progress_calls.load(std::sync::atomic::Ordering::Relaxed) > 0);
        assert!(summary.dir.contains("backup-"));
        assert!(summary.dir.contains("XQ-DQ44"));
        let root = PathBuf::from(&summary.dir);
        assert!(root.join("settings/settings_secure.txt").exists());
        assert!(root.join("android-data/com.kakao.talk/db").exists());
        assert!(root.join("apks/com.example.app/base.apk").exists());
        assert!(root.join("sdcard/DCIM/a.jpg").exists());
        assert!(root.join("manifest.json").exists());
        // manifest의 시리얼은 마스킹만
        let raw = std::fs::read_to_string(root.join("manifest.json")).unwrap();
        assert!(raw.contains("AB1234****"));
        assert!(!raw.contains("AB1234CDEFGH"));
    }

    #[test]
    fn sms_calllog_stay_pending_blocks_completeness() {
        let mut d = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let items = vec!["sms".to_string(), "calllog".to_string()];
        let summary = run_backup_items(
            &mut d,
            &items,
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!summary.complete); // Pending 항목 → 완결 게이트 닫힘
    }

    #[test]
    fn resume_reuses_folder_and_skips_done_items() {
        let mut d = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut d,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        // 같은 폴더로 재개 — 이미 끝난 dcim은 건너뛰고 app-data만 진행
        let calls_before = d.shell_calls.len();
        let second = run_backup_items(
            &mut d,
            &["dcim".into(), "app-data".into()],
            dest.path(),
            Some(Path::new(&first.dir)),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert_eq!(second.dir, first.dir);
        assert!(second.complete);
        let dcim_pulls = d.shell_calls[calls_before..]
            .iter()
            .filter(|c| c.contains("DCIM"))
            .count();
        assert_eq!(dcim_pulls, 0, "끝난 항목은 다시 받지 않는다");
        assert!(Path::new(&second.dir)
            .join("android-data/com.kakao.talk/db")
            .exists());
    }

    #[test]
    fn adversarial_resume_retains_copied_files_deleted_from_phone_and_updates_selection() {
        let mut d = dev_full();
        d.add_file("/sdcard/DCIM/b.jpg", b"second", 1700000001, 0o644);
        d.fail_pull("/sdcard/DCIM/b.jpg");
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut d,
            &["dcim".into(), "sms".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&first.dir);
        assert!(!first.complete);
        // A second interrupted attempt must not lose the first copy's manifest entry.
        d.fail_pull("/sdcard/DCIM/a.jpg");
        let interrupted = run_backup_items(
            &mut d,
            &["dcim".into(), "sms".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!interrupted.complete);
        let saved = load_manifest(root).unwrap();
        let photo = saved
            .items
            .iter()
            .find(|i| i.id == "dcim")
            .unwrap()
            .entries
            .iter()
            .find(|e| e.remote.ends_with("a.jpg"))
            .unwrap();
        assert!(photo.error.is_none() && photo.sha256.is_some());
        // New connection sees only the previously failed file; successful source is gone.
        let mut reconnected = dev_full();
        reconnected.remove_file("/sdcard/DCIM/a.jpg");
        reconnected.add_file("/sdcard/DCIM/b.jpg", b"second", 1700000001, 0o644);
        let second = run_backup_items(
            &mut reconnected,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(second.complete, "{:?}", second.errors);
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/a.jpg")).unwrap(),
            b"photo"
        );
        let manifest = load_manifest(root).unwrap();
        let dcim = manifest.items.iter().find(|i| i.id == "dcim").unwrap();
        assert_eq!(dcim.entries.len(), 2);
        assert_eq!(
            manifest
                .items
                .iter()
                .find(|i| i.id == "sms")
                .unwrap()
                .status,
            ItemStatus::Skipped
        );
        // Reselecting a previously Pending SMS item cannot mark it Done without an export.
        let third = run_backup_items(
            &mut reconnected,
            &["dcim".into(), "sms".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!third.complete);
    }

    /// 실기기 백업 확인용 — 폰에서 읽어 PC로 복사만 한다(기기 변경 없음).
    /// 실행: XVOLTE_LIVE_BACKUP_DEST=<폴더> XVOLTE_LIVE_BACKUP_ITEMS=settings-all,apk,... cargo test --lib -- --ignored live_backup --nocapture
    #[test]
    #[ignore]
    fn live_backup() {
        let dest = std::env::var("XVOLTE_LIVE_BACKUP_DEST").expect("XVOLTE_LIVE_BACKUP_DEST 필요");
        let items: Vec<String> = std::env::var("XVOLTE_LIVE_BACKUP_ITEMS")
            .expect("XVOLTE_LIVE_BACKUP_ITEMS 필요")
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty() && s != "sms" && s != "calllog") // 앱 설치가 필요한 항목은 제외
            .collect();
        let started = std::time::Instant::now();
        let mut last = std::time::Instant::now();
        let mut sink: ProgressSink = Box::new(move |p| {
            if last.elapsed().as_secs() >= 5 || (p.files_total > 0 && p.files_done == p.files_total)
            {
                last = std::time::Instant::now();
                eprintln!(
                    "[진행] {} {} {}/{} 파일, {}/{} MB",
                    p.item_id,
                    p.phase,
                    p.files_done,
                    p.files_total,
                    p.bytes_done / 1_048_576,
                    p.bytes_total / 1_048_576
                );
            }
        });
        let summary = crate::adb::with_first_device(&None, |dev| {
            // XVOLTE_LIVE_BACKUP_RESUME=<기존 백업 폴더> — 이어서 백업(끝난 항목은 건너뜀) 확인용
            let resume = std::env::var("XVOLTE_LIVE_BACKUP_RESUME")
                .ok()
                .filter(|v| !v.is_empty());
            run_backup_items(
                dev,
                &items,
                Path::new(&dest),
                resume.as_deref().map(Path::new),
                &CancelFlag::new(),
                &mut sink,
            )
        })
        .expect("백업 실행 실패");
        eprintln!("[결과] 폴더 {}", summary.dir);
        eprintln!(
            "[결과] 완결 {} · 파일 {} · {} MB · {}초",
            summary.complete,
            summary.files,
            summary.bytes / 1_048_576,
            started.elapsed().as_secs()
        );
        for e in summary.errors.iter().take(40) {
            eprintln!("[오류] {e}");
        }
    }

    #[test]
    fn resume_requires_manifest() {
        let mut d = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let empty = dest.path().join("not-a-backup");
        std::fs::create_dir_all(&empty).unwrap();
        let r = run_backup_items(
            &mut d,
            &["dcim".into()],
            dest.path(),
            Some(&empty),
            &CancelFlag::new(),
            &mut sink,
        );
        assert!(r.is_err());
    }

    #[test]
    fn cancellation_before_first_item_leaves_every_selection_pending() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let cancel = CancelFlag::new();
        cancel.set();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut dev,
            &["dcim".into(), "apk".into()],
            dest.path(),
            None,
            &cancel,
            &mut sink,
        )
        .unwrap();
        assert!(!result.complete);
        let manifest = load_manifest(Path::new(&result.dir)).unwrap();
        assert_eq!(manifest.items.len(), 2);
        assert!(manifest
            .items
            .iter()
            .all(|item| item.status == ItemStatus::Pending));
    }

    #[test]
    fn resume_recollects_a_corrupted_completed_file() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&first.dir);
        std::fs::write(root.join("sdcard/DCIM/a.jpg"), b"evil").unwrap();
        let resumed = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(resumed.complete);
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/a.jpg")).unwrap(),
            b"photo"
        );
        let original = load_manifest(root).unwrap();
        assert!(original.device_key.is_some(), "기기 해시가 기록돼야 한다");
        // 마스킹 시리얼이 같아도(앞 6자리) 기기 해시가 다르면 다른 기기다
        let mut manifest = original.clone();
        manifest.device_key = Some("0".repeat(64));
        save_manifest_atomic(&manifest, root).unwrap();
        let resume = |dev: &mut FakeADBDevice, sink: &mut ProgressSink| {
            run_backup_items(
                dev,
                &["dcim".into()],
                dest.path(),
                Some(root),
                &CancelFlag::new(),
                sink,
            )
        };
        assert!(resume(&mut dev, &mut sink).is_err());
        // 해시가 없는 예전 manifest는 마스킹 시리얼로 확인한다
        let mut manifest = original.clone();
        manifest.device_key = None;
        manifest.serial_masked = "OTHER****".into();
        save_manifest_atomic(&manifest, root).unwrap();
        assert!(resume(&mut dev, &mut sink).is_err());
        // 일치하면 이어 가며 해시를 채워 둔다
        let mut manifest = original;
        manifest.device_key = None;
        save_manifest_atomic(&manifest, root).unwrap();
        assert!(resume(&mut dev, &mut sink).is_ok());
        assert!(load_manifest(root).unwrap().device_key.is_some());
    }

    #[test]
    fn resume_records_why_a_completed_item_became_incomplete() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&first.dir);
        std::fs::write(root.join("sdcard/DCIM/a.jpg"), b"evil").unwrap();
        // 첫 항목 전에 취소 — 손상된 항목은 다시 받지 못하고 사유가 남아야 한다
        let cancel = CancelFlag::new();
        cancel.set();
        let resumed = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &cancel,
            &mut sink,
        )
        .unwrap();
        assert!(!resumed.complete);
        assert!(
            resumed
                .errors
                .iter()
                .any(|e| e.contains("크기/해시 불일치")),
            "{:?}",
            resumed.errors
        );
    }

    #[test]
    fn package_list_failure_only_fails_the_apk_item() {
        let mut dev = dev_full();
        dev.fail_shell.insert("pm list packages -3 -f".into());
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let summary = run_backup_items(
            &mut dev,
            &["apk".into(), "dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!summary.complete);
        let brief = |id: &str| summary.items.iter().find(|i| i.id == id).unwrap();
        assert_eq!(brief("apk").status, "partial");
        assert_eq!(brief("dcim").status, "done");
        assert!(summary
            .errors
            .iter()
            .any(|e| e.starts_with("apk: 앱 목록 조회 실패")));
    }

    #[test]
    fn invalid_selections_fail_before_device_io() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        for items in [
            vec![],
            vec!["unknown".into()],
            vec!["dcim".into(), "dcim".into()],
        ] {
            assert!(run_backup_items(
                &mut dev,
                &items,
                dest.path(),
                None,
                &CancelFlag::new(),
                &mut sink
            )
            .is_err());
        }
        assert!(dev.shell_calls.is_empty());
    }
}
