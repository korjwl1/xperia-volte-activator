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
use adb_client::ADBDeviceExt;
use std::collections::HashMap;
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

pub type ProgressSink<'a> = Box<dyn FnMut(StepProgress) + Send + 'a>;

fn mask_serial(serial: &str) -> String {
    let n = serial.chars().count();
    if n > 6 {
        format!("{}****", serial.chars().take(6).collect::<String>())
    } else {
        serial.to_string()
    }
}

/// getprop 덤프에서 manifest 메타데이터 수집
pub fn device_meta(dev: &mut dyn ADBDeviceExt) -> Result<(String, String, String, String), String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    dev.shell_command(&"getprop", Some(&mut out), Some(&mut err))
        .map_err(|e| format!("기기 정보 조회 실패: {e}"))?;
    let text = String::from_utf8_lossy(&out);
    let props = crate::adb::parse_getprop(&text);
    let model = props.get("ro.product.model").cloned().unwrap_or_default();
    let firmware = props.get("ro.build.id").cloned().unwrap_or_default();
    let android = props.get("ro.build.version.release").cloned().unwrap_or_default();
    let serial = props.get("ro.serialno").cloned().unwrap_or_default();
    Ok((model, firmware, android, mask_serial(&serial)))
}

/// `pm list packages -3 -f`에서 (패키지, 설치 폴더) 목록 — APK 항목의 소스
fn package_install_dirs(dev: &mut dyn ADBDeviceExt) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    dev.shell_command(&"pm list packages -3 -f", Some(&mut out), Some(&mut err))
        .map_err(|e| format!("앱 목록 조회 실패: {e}"))?;
    let text = String::from_utf8_lossy(&out);
    let mut pairs = Vec::new();
    for line in text.lines() {
        // package:/data/app/~~x/com.pkg-y/base.apk=com.pkg
        let Some(rest) = line.trim().strip_prefix("package:") else { continue };
        let Some((path, pkg)) = rest.rsplit_once('=') else { continue };
        let Some((dir, _)) = path.rsplit_once('/') else { continue };
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
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '_' })
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
    let (model, firmware, android, serial_masked) = device_meta(dev)?;
    let root = dest.join(backup_dir_name(&model));
    std::fs::create_dir_all(&root).map_err(|e| format!("백업 폴더 생성 실패: {e}"))?;
    save_manifest_atomic(&Manifest::new(&model, &serial_masked, &firmware, &android), &root)?;
    Ok(root)
}

/// 이어서 백업 — 끝나지 않은 항목이 남긴 이전 파일을 지운다(그 항목은 처음부터 다시 받아 덮어씀).
/// 백업 폴더 밖 경로는 건드리지 않는다
fn clear_unfinished_item(root: &Path, prev: &ItemRecord) {
    for e in &prev.entries {
        if e.local.is_empty() || e.local.split('/').any(|c| c == "..") {
            continue;
        }
        let _ = std::fs::remove_file(root.join(&e.local));
    }
}

/// 열거 결과를 풀 결과에 병합 — 열거 오류·스킵 기록도 항목 완결 판정에 들어간다(§6-2 전수 열거)
fn merge_walk(rec: &mut ItemRecord, w: &walker::WalkResult) {
    if !w.errors.is_empty() {
        rec.errors.extend(w.errors.iter().cloned());
        rec.status = ItemStatus::Partial;
    }
    for s in &w.skipped {
        rec.errors.push(format!("{}: 백업 제외({})", s.remote, s.reason));
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
    let root = match resume_dir {
        Some(dir) => {
            // 재개 — manifest가 있는 폴더만 허용(그 외 덮어쓰기 방지)
            load_manifest(dir).map_err(|e| format!("이어서 진행할 수 없습니다 — {e}"))?;
            std::path::absolute(dir).map_err(|e| format!("백업 폴더 확인 실패: {e}"))?
        }
        None => prepare_backup_root(dev, dest)?,
    };
    let mut manifest = match load_manifest(&root) {
        Ok(m) => m,
        Err(_) => {
            let (model, firmware, android, serial_masked) = device_meta(dev)?;
            Manifest::new(&model, &serial_masked, &firmware, &android)
        }
    };
    let mut quarantine = Quarantine::new(&root)?;
    let mut seen: HashMap<String, PathBuf> = HashMap::new();

    for id in items {
        if cancel.cancelled() {
            break;
        }
        // 이미 완료된 항목은 다시 받지 않는다(§9-2 재개 프로브 — 항목 단위)
        if let Some(prev) = manifest.items.iter().find(|i| i.id == *id) {
            if prev.status == ItemStatus::Done {
                continue;
            }
            // 중간에 멈췄던 항목 — 이전 파일을 지우고 처음부터 다시 받는다
            clear_unfinished_item(&root, prev);
        }
        let rec: ItemRecord = match id.as_str() {
            "settings-all" => {
                on_progress(StepProgress { item_id: id.clone(), phase: "settings", file: None, files_done: 0, files_total: 5, bytes_done: 0, bytes_total: 0 });
                settings::collect_settings(dev, &root)
            }
            "contacts" => {
                on_progress(StepProgress { item_id: id.clone(), phase: "contacts", file: None, files_done: 0, files_total: 0, bytes_done: 0, bytes_total: 0 });
                contacts::collect_contacts(dev, &root)
            }
            "calllog" | "sms" => ItemRecord {
                // SMS Import/Export 세미수동 — smsie_prepare/collect가 채운다
                id: id.clone(),
                kind: ItemKind::SmsIe,
                status: ItemStatus::Pending,
                files: 0,
                bytes: 0,
                entries: vec![],
                artifacts: vec![],
                errors: vec![],
            },
            "apk" => {
                let dirs = package_install_dirs(dev)?;
                let mut files: Vec<PullFile> = Vec::new();
                let mut walk_errors: Vec<String> = Vec::new();
                for (pkg, dir) in &dirs {
                    let w = walker::walk(dev, dir, &|_| false);
                    walk_errors.extend(w.errors.iter().cloned());
                    for f in w.files {
                        if f.remote.ends_with(".apk") {
                            files.push(PullFile { entry: f, tag: pkg.clone() });
                        }
                    }
                }
                let mut rec = pull_item_files(
                    dev,
                    id,
                    ItemKind::Files,
                    &files,
                    &root,
                    &|pf| {
                        let name = pf.entry.remote.rsplit('/').next().unwrap_or("apk");
                        PathBuf::from(format!("apks/{}/{}", pf.tag, name))
                    },
                    &mut seen,
                    &mut quarantine,
                    cancel,
                    |p| on_progress(copy_progress(id, &p)),
                );
                rec.errors.extend(walk_errors);
                if !rec.errors.is_empty() {
                    rec.status = ItemStatus::Partial;
                }
                rec
            }
            "app-data" => {
                let w = walker::walk(dev, walker::ANDROID_DATA_ROOT, &|_| false);
                let files: Vec<PullFile> = w.files.iter().cloned().map(|e| PullFile::plain(e)).collect();
                let mut rec = pull_item_files(
                    dev,
                    id,
                    ItemKind::Files,
                    &files,
                    &root,
                    &|pf| {
                        let rest = pf.entry.remote.strip_prefix("/sdcard/Android/data/").unwrap_or(&pf.entry.remote);
                        PathBuf::from(format!("android-data/{rest}"))
                    },
                    &mut seen,
                    &mut quarantine,
                    cancel,
                    |p| on_progress(copy_progress(id, &p)),
                );
                merge_walk(&mut rec, &w);
                rec
            }
            "fs-rest" => {
                let w = walker::walk(dev, walker::FS_REST_ROOT, &walker::fs_rest_skip);
                let files: Vec<PullFile> = w.files.iter().cloned().map(|e| PullFile::plain(e)).collect();
                let mut rec = pull_item_files(
                    dev,
                    id,
                    ItemKind::Files,
                    &files,
                    &root,
                    &|pf| {
                        let rest = pf.entry.remote.strip_prefix("/sdcard/").unwrap_or(&pf.entry.remote);
                        PathBuf::from(format!("sdcard/__rest__/{rest}"))
                    },
                    &mut seen,
                    &mut quarantine,
                    cancel,
                    |p| on_progress(copy_progress(id, &p)),
                );
                merge_walk(&mut rec, &w);
                rec
            }
            named => {
                let Some((_, dir)) = walker::NAMED_FILE_ITEMS.iter().find(|(nid, _)| *nid == named) else {
                    continue; // 계약에 없는 항목 — 무시
                };
                let root_path = format!("/sdcard/{dir}");
                let w = walker::walk(dev, &root_path, &|_| false);
                let files: Vec<PullFile> = w.files.iter().cloned().map(|e| PullFile::plain(e)).collect();
                let mut rec = pull_item_files(
                    dev,
                    named,
                    ItemKind::Files,
                    &files,
                    &root,
                    &|pf| {
                        let rest = pf.entry.remote.strip_prefix("/sdcard/").unwrap_or(&pf.entry.remote);
                        PathBuf::from(format!("sdcard/{rest}"))
                    },
                    &mut seen,
                    &mut quarantine,
                    cancel,
                    |p| on_progress(copy_progress(named, &p)),
                );
                merge_walk(&mut rec, &w);
                rec
            }
        };
        manifest.record(rec);
        save_manifest_atomic(&manifest, &root)?;
    }
    let segments = quarantine.finish().map_err(|e| e)?;
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
        d.add_file("/sdcard/Android/data/com.kakao.talk/db", b"db", 1700000001, 0o600);
        d.add_dir("/data/app/~~x/com.example.app-y");
        d.add_file("/data/app/~~x/com.example.app-y/base.apk", b"PK", 1700000002, 0o644);
        d.answer_shell("getprop", "[ro.product.model]: [XQ-DQ44]\n[ro.build.id]: [67.2.A.3.178]\n[ro.build.version.release]: [15]\n[ro.serialno]: [AB1234CDEFGH]\n");
        d.answer_shell("pm list packages -3 -f", "package:/data/app/~~x/com.example.app-y/base.apk=com.example.app\n");
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
        let summary = run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
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
        let summary = run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        assert!(!summary.complete); // Pending 항목 → 완결 게이트 닫힘
    }

    #[test]
    fn resume_reuses_folder_and_skips_done_items() {
        let mut d = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(&mut d, &["dcim".into()], dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        // 같은 폴더로 재개 — 이미 끝난 dcim은 건너뛰고 app-data만 진행
        let calls_before = d.shell_calls.len();
        let second =
            run_backup_items(&mut d, &["dcim".into(), "app-data".into()], dest.path(), Some(Path::new(&first.dir)), &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(second.dir, first.dir);
        assert!(second.complete);
        let dcim_pulls = d.shell_calls[calls_before..].iter().filter(|c| c.contains("DCIM")).count();
        assert_eq!(dcim_pulls, 0, "끝난 항목은 다시 받지 않는다");
        assert!(Path::new(&second.dir).join("android-data/com.kakao.talk/db").exists());
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
            if last.elapsed().as_secs() >= 5 || (p.files_total > 0 && p.files_done == p.files_total) {
                last = std::time::Instant::now();
                eprintln!(
                    "[진행] {} {} {}/{} 파일, {}/{} MB",
                    p.item_id, p.phase, p.files_done, p.files_total, p.bytes_done / 1_048_576, p.bytes_total / 1_048_576
                );
            }
        });
        let summary = crate::adb::with_first_device(&None, |dev| {
            // XVOLTE_LIVE_BACKUP_RESUME=<기존 백업 폴더> — 이어서 백업(끝난 항목은 건너뜀) 확인용
            let resume = std::env::var("XVOLTE_LIVE_BACKUP_RESUME").ok().filter(|v| !v.is_empty());
            run_backup_items(dev, &items, Path::new(&dest), resume.as_deref().map(Path::new), &CancelFlag::new(), &mut sink)
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
        let r = run_backup_items(&mut d, &["dcim".into()], dest.path(), Some(&empty), &CancelFlag::new(), &mut sink);
        assert!(r.is_err());
    }
}
