//! 백업 실행 오케스트레이션 — 항목 id를 받아 수집기·풀러를 차례로 돌리고 manifest를 유지한다.
//! 이 파일의 함수는 전부 `&mut dyn ADBDeviceExt`를 받는다(단위 테스트 주입용).
//! 진행 이벤트는 콜백(Sink)으로만 올리고, emit은 mod.rs의 Tauri 명령 층에서 한다.

use crate::backup::contacts;
use crate::backup::model::{
    load_manifest, save_manifest_atomic, BackupSummary, ItemKind, ItemRecord, ItemStatus, Manifest,
};
use crate::backup::puller::{pull_item_files_resuming, CancelFlag, PullFile, PullProgress};
use crate::backup::quarantine::Quarantine;
use crate::backup::settings;
use crate::backup::walker::{self};
use crate::backup::winname::SeenPaths;
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
    verified: &'c HashMap<String, super::model::FileEntry>,
    source_metadata: Vec<super::source_metadata::Snapshot>,
    previous_metadata: Vec<super::source_metadata::Snapshot>,
}

impl TreeCtx<'_, '_> {
    fn remember_metadata(
        &mut self,
        mut snapshot: super::source_metadata::Snapshot,
    ) -> Result<(), String> {
        let mut manifest = load_manifest(self.root)?;
        if let Some(old) = manifest
            .source_metadata
            .iter()
            .find(|r| r.item_id == snapshot.item_id)
        {
            let previous = match super::source_metadata::load(self.root, old) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    snapshot.limitations.push(format!(
                        "Previous attribute record unavailable; new observation: {error}"
                    ));
                    for attr in &mut snapshot.entries {
                        attr.capture_context = "resume-observation".into();
                    }
                    self.source_metadata.push(snapshot.clone());
                    let receipt = super::source_metadata::publish(self.root, &snapshot)?;
                    manifest
                        .source_metadata
                        .retain(|r| r.item_id != receipt.item_id);
                    manifest.source_metadata.push(receipt);
                    return save_manifest_atomic(&manifest, self.root);
                }
            };
            self.previous_metadata.push(previous.clone());
            let old_attrs: HashMap<_, _> = previous
                .entries
                .into_iter()
                .map(|a| (a.remote.clone(), a))
                .collect();
            for attr in &mut snapshot.entries {
                if let Some(old) = old_attrs.get(&attr.remote) {
                    let retained = self.verified.get(&attr.remote);
                    if (old.mode & 0o170000 == 0o040000 && attr.mode & 0o170000 == 0o040000)
                        || (old.size == attr.size
                            && old.modification_time == attr.modification_time
                            && old.mode & 0o170000 == attr.mode & 0o170000)
                        || retained.is_some_and(|file| {
                            (old.backup_sha256.is_none() || old.backup_sha256 == file.sha256)
                                && old.size == file.size
                                && (old.modification_time as u32) == file.mtime
                                && file.size == attr.size
                                && i64::from(file.mtime) == attr.modification_time
                        })
                    {
                        *attr = old.clone();
                    }
                }
            }
            let present: std::collections::HashSet<_> =
                snapshot.entries.iter().map(|a| a.remote.clone()).collect();
            for (remote, attr) in old_attrs {
                if (attr.mode & 0o170000 == 0o040000
                    || self.verified.get(&remote).is_some_and(|file| {
                        attr.backup_sha256.is_some() && attr.backup_sha256 == file.sha256
                    }))
                    && !present.contains(&remote)
                {
                    snapshot.entries.push(attr);
                }
            }
        }
        let receipt = super::source_metadata::publish(self.root, &snapshot)?;
        manifest
            .source_metadata
            .retain(|r| r.item_id != receipt.item_id);
        manifest.source_metadata.push(receipt);
        manifest.touch();
        save_manifest_atomic(&manifest, self.root)?;
        self.source_metadata.push(snapshot);
        Ok(())
    }

    fn pull(
        &mut self,
        id: &str,
        files: &[PullFile],
        local_for: &dyn Fn(&PullFile) -> PathBuf,
    ) -> ItemRecord {
        let on_progress = &mut *self.on_progress;
        let record = pull_item_files_resuming(
            self.dev,
            id,
            ItemKind::Files,
            files,
            self.root,
            local_for,
            self.seen,
            self.quarantine,
            self.cancel,
            self.verified,
            |p| on_progress(copy_progress(id, &p)),
        );
        record
    }

    /// 기기 폴더 하나를 전수 열거해 받는 항목 — `local`은 기기 경로 → 백업 루트 상대 경로
    fn tree_item(
        &mut self,
        id: &str,
        remote_root: &str,
        skip: &dyn Fn(&str) -> bool,
        local: &dyn Fn(&str) -> String,
    ) -> ItemRecord {
        (self.on_progress)(StepProgress::start(id, "scan", 0));
        let walker::WalkResult {
            files,
            directories,
            skipped,
            errors,
        } = walker::walk_with_progress(
            self.dev,
            remote_root,
            skip,
            self.cancel,
            &mut |directories, files| {
                // Totals are unknown until enumeration ends; never count discovered files as copied.
                let mut progress = StepProgress::start(id, "scan", files);
                progress.file = Some(format!("폴더 {directories}개 · 파일 {files}개 확인"));
                (self.on_progress)(progress);
            },
        );
        let paths = files
            .iter()
            .map(|f| f.remote.clone())
            .chain(directories)
            .collect();
        let snapshot = super::source_metadata::capture(
            self.dev,
            id,
            paths,
            "before-copy",
            self.cancel,
            &mut |done, total| (self.on_progress)(StepProgress::at(id, "metadata", done, total)),
        );
        if let Err(error) = self.remember_metadata(snapshot) {
            return ItemRecord::new(id, ItemKind::Files).fail(error);
        }
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
    let mut directories = Vec::new();
    for (pkg, dir) in dirs {
        if ctx.cancel.cancelled() {
            walk_errors.push("열거가 취소됐습니다".into());
            break;
        }
        let w = walker::walk_cancellable(ctx.dev, &dir, &|_| false, ctx.cancel);
        if !w
            .files
            .iter()
            .any(|file| file.remote.rsplit('/').next() == Some("base.apk"))
        {
            walk_errors.push(format!("{pkg}: 설치 APK의 base.apk를 확인하지 못했습니다"));
        }
        walk_errors.extend(w.errors);
        directories.extend(w.directories);
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
    let paths = files
        .iter()
        .map(|f| f.entry.remote.clone())
        .chain(directories)
        .collect();
    let snapshot = super::source_metadata::capture(
        ctx.dev,
        id,
        paths,
        "before-copy",
        ctx.cancel,
        &mut |done, total| (ctx.on_progress)(StepProgress::at(id, "metadata", done, total)),
    );
    if let Err(error) = ctx.remember_metadata(snapshot) {
        return ItemRecord::new(id, ItemKind::Files).fail(error);
    }
    let mut rec = ctx.pull(id, &files, &|pf| {
        let name = pf.entry.remote.rsplit('/').next().unwrap_or("apk");
        let parent = pf
            .entry
            .remote
            .rsplit_once('/')
            .map(|(p, _)| p)
            .unwrap_or("");
        let prefix = crate::boot_image::sha256(parent.as_bytes());
        PathBuf::from(format!("apks/{}/{}-{name}", pf.tag, &prefix[..16]))
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

/// 공장 초기화 판정용 키 — SHA-256(android_id). 읽을 수 없으면 None(판정 불가)
pub fn install_key(dev: &mut dyn ADBDeviceExt) -> Option<String> {
    use sha2::{Digest, Sha256};
    let id = crate::device_io::shell(dev, "settings get secure android_id").ok()?;
    let id = id.trim();
    (!id.is_empty() && id != "null").then(|| hex::encode(Sha256::digest(id.as_bytes())))
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

/// 백업 폴더명 — xva-<model>-backup (모델은 파일명 안전 문자만). 시각을 넣지 않는다:
/// 같은 저장 위치에서는 같은 폰의 백업을 한 폴더로 이어서 갱신한다
pub fn backup_dir_name(model: &str) -> String {
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
    format!("xva-{safe}-backup")
}

/// 이 앱이 만든 백업 폴더 이름인지 — 현재 이름(xva-<모델>-backup)과 이전 이름(backup-<시각>-<모델>) 모두
pub fn is_backup_dir_name(name: &str) -> bool {
    name.starts_with("backup-") || (name.starts_with("xva-") && name.ends_with("-backup"))
}

/// 백업 시작 — 지정 폴더 아래의 xva-<모델>-backup을 쓴다. 반환 경로는 절대 경로.
/// - 없으면 새로 만들고 빈 manifest를 저장한다.
/// - 이미 있으면 같은 폰의 백업인지 확인한 뒤 그대로 돌려준다 — 이어서 받으면(resume) 바뀐 파일만 갱신된다.
/// - 다른 폰의 백업, 기록 없는 폴더, 링크·파일이면 덮어쓰지 않고 거부한다.
pub fn prepare_backup_root(
    dev: &mut dyn ADBDeviceExt,
    dest: &Path,
) -> Result<(PathBuf, bool), String> {
    let dest = std::path::absolute(dest).map_err(|e| format!("백업 위치 확인 실패: {e}"))?;
    if !dest.is_dir() {
        return Err("백업 저장 위치 폴더가 없습니다".into());
    }
    let meta = device_meta(dev)?;
    let name = backup_dir_name(&meta.model);
    let root = dest.join(&name);
    match std::fs::symlink_metadata(&root) {
        Ok(found) => {
            if found.file_type().is_symlink() || !found.is_dir() {
                return Err(format!("{name}이(가) 폴더가 아니거나 링크라서 백업 폴더로 쓸 수 없습니다"));
            }
            let empty = std::fs::read_dir(&root)
                .map_err(|e| format!("기존 백업 폴더 확인 실패: {e}"))?
                .next()
                .is_none();
            if !empty {
                let manifest = load_manifest(&root)
                    .map_err(|e| format!("{name} 폴더에 백업 기록이 없거나 손상돼 이어서 쓸 수 없습니다 — {e}"))?;
                if meta.device_key.is_none() || manifest.device_key != meta.device_key {
                    return Err(format!("{name} 폴더는 다른 기기의 백업입니다 — 다른 저장 위치를 지정해 주세요"));
                }
                return Ok((root, true));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(&root).map_err(|e| format!("백업 폴더 생성 실패: {e}"))?;
        }
        Err(e) => return Err(format!("백업 폴더 확인 실패: {e}")),
    }
    let mut manifest = Manifest::new(
        &meta.model,
        &meta.serial_masked,
        &meta.firmware,
        &meta.android,
    );
    manifest.device_key = meta.device_key;
    manifest.install_key = install_key(dev);
    save_manifest_atomic(&manifest, &root)?;
    Ok((root, false))
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
    // 저장 위치에 같은 폰의 기존 백업 폴더가 있으면 재개와 똑같이 이어서 갱신한다(바뀐 파일만 받음)
    let (root, resuming) = match resume_dir {
        Some(dir) => {
            // 재개 — manifest가 있는 폴더만 허용(그 외 덮어쓰기 방지)
            load_manifest(dir).map_err(|e| format!("이어서 진행할 수 없습니다 — {e}"))?;
            (std::path::absolute(dir).map_err(|e| format!("백업 폴더 확인 실패: {e}"))?, true)
        }
        None => prepare_backup_root(dev, dest)?,
    };
    let mut manifest = load_manifest(&root)?;
    if resuming {
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
    // 저장 위치에서 찾은 기존 백업을 갱신하는 경우(끊긴 실행의 재개가 아님): 완료 항목도 다시 훑는다.
    // 파일 항목은 안 바뀐 파일을 다시 받지 않고, 폰에서 사라진 파일의 사본은 유지된다.
    // 연락처·설정·문자·통화는 다시 수집하면 덮어쓰므로, 백업 이후 공장 초기화된 폰(android_id가 바뀜)이거나
    // 판정할 수 없으면 기존 사본을 그대로 둔다.
    let refreshing = resume_dir.is_none() && resuming;
    let current_install = if refreshing { install_key(dev) } else { None };
    let reset_since_backup = matches!(
        (&manifest.install_key, &current_install),
        (Some(saved), Some(current)) if saved != current
    );
    let refresh_snapshots = refreshing && current_install.is_some() && !reset_since_backup;
    if refreshing && !reset_since_backup && current_install.is_some() {
        manifest.install_key = current_install.clone();
    }
    // 첫 항목 전에 취소돼도 선택한 모든 항목이 Pending으로 남아야 한다.
    manifest.excluded_items = manifest
        .items
        .iter()
        .filter(|item| !items.contains(&item.id))
        .map(|item| item.id.clone())
        .collect();
    for item in &mut manifest.items {
        if items.contains(&item.id) && item.status == ItemStatus::Skipped {
            // Re-enumerate reselected items; a previously skipped Pending item is not Done.
            item.status = ItemStatus::Pending;
        }
    }
    super::recovery::repair_segments(&root, &mut manifest)?;
    if super::source_metadata::rejudge_receipts(&root, &mut manifest) {
        manifest.touch();
        save_manifest_atomic(&manifest, &root)?;
    }
    let mut verified = if resuming {
        crate::backup::verify::verify_selected_retaining_progress(
            &root,
            &mut manifest,
            items,
            cancel,
            on_progress,
        )
    } else {
        HashMap::new()
    };
    if cancel.cancelled() && resuming {
        return Err(format!("{} — 완료 파일은 보존됩니다", cancel.reason()));
    }
    super::recovery::clean_temporaries(&root)?;
    for id in items {
        if !manifest.items.iter().any(|item| item.id == *id) {
            manifest.record(ItemRecord::new(id, item_kind(id)));
        }
    }
    save_manifest_atomic(&manifest, &root)?;
    let mut quarantine = Quarantine::new(&root)?;
    let mut seen = SeenPaths::new();
    // Completed items and deleted phone files still own their PC paths.
    for item in &manifest.items {
        let Some(entries) = verified.get(&item.id) else {
            continue;
        };
        for entry in item
            .entries
            .iter()
            .filter(|entry| !entry.quarantined && entries.contains_key(&entry.remote))
        {
            super::winname::check_relative_path(&entry.local, &entry.remote, &mut seen);
        }
    }

    if resuming && items.iter().any(|id| id == "apk") {
        let dirs = package_install_dirs(dev)?;
        if let Some(apk) = manifest.items.iter_mut().find(|i| i.id == "apk") {
            let changed = dirs.iter().any(|(pkg, dir)| {
                let old: Vec<_> = apk
                    .entries
                    .iter()
                    .filter(|e| e.local.split('/').nth(1) == Some(pkg.as_str()))
                    .collect();
                old.is_empty()
                    || old.iter().any(|e| {
                        e.remote
                            .rsplit_once('/')
                            .is_none_or(|(parent, _)| parent != dir)
                    })
            });
            if changed {
                apk.status = ItemStatus::Partial;
            }
        }
    }
    for id in items {
        if cancel.cancelled() {
            break;
        }
        // 끊긴 실행의 재개: 이미 완료된 항목은 다시 받지 않는다(§9-2 재개 프로브 — 항목 단위).
        // 기존 백업 갱신: 파일 항목은 다시 훑어 바뀐 것만 받고, 덮어쓰는 항목은 위의 초기화 판정을 따른다.
        if let Some(prev) = manifest.items.iter().find(|i| i.id == *id) {
            if prev.status == ItemStatus::Done
                && (!refreshing || (prev.kind != ItemKind::Files && !refresh_snapshots))
            {
                continue;
            }
        }
        let retained =
            if manifest.items.iter().any(|item| {
                item.id == *id && matches!(item.kind, ItemKind::Files | ItemKind::SmsIe)
            }) {
                verified.remove(id).unwrap_or_default()
            } else {
                HashMap::new()
            };
        let previous_entries = manifest
            .items
            .iter()
            .find(|i| i.id == *id && matches!(i.kind, ItemKind::Files | ItemKind::SmsIe))
            .map(|i| i.entries.clone())
            .unwrap_or_default();
        let mut ctx = TreeCtx {
            dev: &mut *dev,
            root: &root,
            seen: &mut seen,
            quarantine: &mut quarantine,
            cancel,
            on_progress: &mut *on_progress,
            verified: &retained,
            source_metadata: Vec::new(),
            previous_metadata: Vec::new(),
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
            "app-data" => ctx.tree_item(
                id,
                walker::ANDROID_DATA_ROOT,
                &|remote| {
                    super::omissions::package(remote).is_some_and(|name| {
                        manifest.omitted_apps.iter().any(|app| app.package == name)
                    })
                },
                &|remote| {
                    let rest = remote
                        .strip_prefix("/sdcard/Android/data/")
                        .unwrap_or(remote);
                    format!("android-data/{rest}")
                },
            ),
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
        let snapshots = std::mem::take(&mut ctx.source_metadata);
        let previous_metadata = std::mem::take(&mut ctx.previous_metadata);
        drop(ctx);
        let entry_indices: HashMap<_, _> = rec
            .entries
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.remote.clone(), index))
            .collect();
        let successful_apk_packages: std::collections::HashSet<_> = if id == "apk"
            && rec.errors.is_empty()
            && rec.entries.iter().all(|e| e.error.is_none())
        {
            rec.entries
                .iter()
                .filter_map(|e| e.local.split('/').nth(1).map(String::from))
                .collect()
        } else {
            std::collections::HashSet::new()
        };
        for entry in retained.into_values() {
            if id == "apk"
                && entry
                    .local
                    .split('/')
                    .nth(1)
                    .is_some_and(|pkg| successful_apk_packages.contains(pkg))
                && !entry_indices.contains_key(&entry.remote)
            {
                continue;
            }
            if let Some(&index) = entry_indices.get(&entry.remote) {
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
        let present: std::collections::HashSet<_> =
            rec.entries.iter().map(|e| e.remote.clone()).collect();
        for mut old in previous_entries
            .into_iter()
            .filter(|entry| entry.sha256.is_some())
        {
            if present.contains(&old.remote)
                || (id == "apk"
                    && old
                        .local
                        .split('/')
                        .nth(1)
                        .is_some_and(|p| successful_apk_packages.contains(p)))
            {
                continue;
            }
            let error = format!(
                "검증하지 못한 이전 백업 파일을 원본에서도 다시 수집하지 못했습니다: {}",
                old.remote
            );
            old.error = Some(error.clone());
            rec.errors.push(error);
            rec.entries.push(old);
        }
        rec.files = rec.entries.len() as u32;
        for mut snapshot in snapshots {
            let current: HashMap<_, _> = rec.entries.iter().map(|e| (&e.remote, e)).collect();
            let mut attributes: HashMap<_, _> = snapshot
                .entries
                .into_iter()
                .map(|a| (a.remote.clone(), a))
                .collect();
            for previous in &previous_metadata {
                for attr in &previous.entries {
                    if current.get(&attr.remote).is_some_and(|e| {
                        e.error.is_none()
                            && e.sha256.is_some()
                            && attr.backup_sha256 == e.sha256
                            && attr.size == e.size
                            && (attr.modification_time as u32) == e.mtime
                    }) {
                        attributes.insert(attr.remote.clone(), attr.clone());
                        snapshot.issues.retain(|i| i.remote != attr.remote);
                    }
                }
            }
            snapshot.entries = attributes.into_values().collect();
            snapshot.entries.sort_by(|a, b| a.remote.cmp(&b.remote));
            super::source_metadata::refresh_copied(
                &mut *dev,
                &root,
                &mut snapshot,
                &mut rec.entries,
                cancel,
            );
            super::source_metadata::associate(&mut snapshot, &rec.entries);
            let receipt = super::source_metadata::publish(&root, &snapshot)?;
            manifest
                .source_metadata
                .retain(|r| r.item_id != receipt.item_id);
            manifest.source_metadata.push(receipt);
        }
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
        if rec.errors.iter().any(|e| e.contains("NO_SPACE|")) {
            cancel.set();
        }
        // Commit closed/synced quarantine segments before publishing their receipts.
        quarantine.checkpoint()?;
        manifest.record(rec);
        save_manifest_atomic(&manifest, &root)?;
        on_progress(progress);
    }
    let segments = quarantine.finish()?;
    if segments > 0 {
        manifest.touch();
        save_manifest_atomic(&manifest, &root)?;
    }
    if !cancel.cancelled() {
        super::omissions::clean(&root, &mut manifest)?;
    }
    let mut summary = BackupSummary::from(&manifest);
    summary.dir = root.to_string_lossy().to_string();
    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backup_finishes_with_whole_app_exclusion_and_resume_does_not_recollect_it() {
        let mut device = dev_full();
        let app = "/sdcard/Android/data/org.example.blocked";
        device.add_dir(&format!("{app}/files"));
        device.add_dir(&format!("{app}/private"));
        device.add_file(&format!("{app}/files/data"), b"ordinary", 100, 0o644);
        device.add_file(&format!("{app}/files/raw?"), b"quarantined", 100, 0o644);
        device.deny_list.insert(format!("{app}/private"));
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut device,
            &["app-data".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(first.complete, "{:?}", first.errors);
        assert_eq!(first.omitted_apps.len(), 1);
        assert_eq!(first.omitted_apps[0].removed_files, 2);
        let root = Path::new(&first.dir);
        assert!(!root.join("android-data/org.example.blocked").exists());
        let mut manifest = load_manifest(root).unwrap();
        assert!(super::super::verify::verify_manifest(root, &mut manifest).is_empty());
        // A later partial item must scan again, while the whole omitted app stays excluded.
        manifest.items[0].status = ItemStatus::Partial;
        manifest.items[0]
            .errors
            .push("injected interruption".into());
        save_manifest_atomic(&manifest, root).unwrap();
        device.list_calls.clear();
        device.pull_calls.clear();
        let resumed = run_backup_items(
            &mut device,
            &["app-data".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(resumed.complete, "{:?}", resumed.errors);
        assert_eq!(resumed.omitted_apps.len(), 1);
        assert!(device.list_calls.iter().all(|path| !path.starts_with(app)));
        assert!(device.pull_calls.iter().all(|path| !path.starts_with(app)));
        assert!(!root.join("android-data/org.example.blocked").exists());
        assert!(
            super::super::verify::verify_manifest(root, &mut load_manifest(root).unwrap())
                .is_empty()
        );
    }

    #[test]
    fn deselecting_completed_files_preserves_restore_and_reselection_after_wipe() {
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
        let root = Path::new(&first.dir);
        let second = run_backup_items(
            &mut d,
            &["app-data".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(second.complete);
        let m = load_manifest(root).unwrap();
        assert_eq!(
            m.items.iter().find(|i| i.id == "dcim").unwrap().status,
            ItemStatus::Done
        );
        assert_eq!(m.excluded_items, vec!["dcim"]);
        d.remove_file("/sdcard/DCIM/a.jpg");
        let pulls = d.pull_calls.len();
        let third = run_backup_items(
            &mut d,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(third.complete);
        assert_eq!(d.pull_calls.len(), pulls);
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/a.jpg")).unwrap(),
            b"photo"
        );
        let restore_sink: super::super::restore::RestoreSink = std::sync::Arc::new(|_| {});
        let mut restored = super::super::fake_device::FakeADBDevice::new();
        restored.answer_shell("getprop ro.serialno", "AB1234CDEFGH");
        let out = super::super::restore::run_restore(
            &mut restored,
            root,
            &["dcim".into()],
            &restore_sink,
        );
        assert!(out.failures.is_empty(), "{:?}", out.failures);
    }
    #[test]
    fn full_disk_stops_before_the_next_file_and_item() {
        let mut d = dev_full();
        d.add_file("/sdcard/DCIM/b.jpg", b"b", 0, 0o644);
        d.storage_full = true;
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut d,
            &["dcim".into(), "apk".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!result.complete);
        assert_eq!(d.pull_calls.len(), 1);
        assert!(result.errors.iter().any(|e| e.contains("NO_SPACE|")));
        let m = load_manifest(Path::new(&result.dir)).unwrap();
        assert_eq!(
            m.items.iter().find(|i| i.id == "apk").unwrap().status,
            ItemStatus::Pending
        );
    }

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
    fn same_phone_backup_folder_is_found_and_only_changed_files_are_pulled() {
        let dest = tempfile::tempdir().unwrap();
        let mut d = dev_full();
        let items = vec!["dcim".to_string(), "app-data".to_string()];
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        assert!(first.complete);
        // 같은 저장 위치를 다시 고르면 같은 폴더(xva-<모델>-backup)를 찾아 바뀐 파일만 받는다
        let (found, existing) = prepare_backup_root(&mut d, dest.path()).unwrap();
        assert!(existing && found == PathBuf::from(&first.dir));
        d.add_file("/sdcard/DCIM/b.jpg", b"new photo", 1700000100, 0o644);
        d.pull_calls.clear();
        let second = run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(second.dir, first.dir);
        assert!(second.complete, "{:?}", second.errors);
        assert_eq!(d.pull_calls, vec!["/sdcard/DCIM/b.jpg".to_string()]);
        assert!(std::fs::read_dir(dest.path()).unwrap().count() == 1, "no second backup folder");
    }

    #[test]
    fn updating_after_a_factory_reset_never_overwrites_settings_or_contacts_snapshots() {
        let dest = tempfile::tempdir().unwrap();
        let items = vec!["settings-all".to_string(), "dcim".to_string()];
        let mut sink: ProgressSink = Box::new(|_| {});
        let mut d = dev_full();
        d.answer_shell("settings get secure android_id", "aaaa1111
");
        run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        let settings_runs = |d: &FakeADBDevice| d.shell_calls.iter().filter(|c| c.starts_with("settings list system")).count();
        // 같은 설치(초기화 없음): 설정도 최신으로 다시 수집한다
        let before = settings_runs(&d);
        run_backup_items(&mut d, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        assert!(settings_runs(&d) > before, "settings refreshed on the same install");
        // 공장 초기화(android_id 변경): 기존 설정 사본을 덮어쓰지 않는다. 파일 항목은 계속 갱신된다
        let mut wiped = dev_full();
        wiped.answer_shell("settings get secure android_id", "bbbb2222
");
        wiped.answer_shell("settings list system", "screen_brightness=0
");
        let saved = std::fs::read(dest.path().join("xva-XQ-DQ44-backup/settings/settings_system.txt")).unwrap();
        wiped.add_file("/sdcard/DCIM/after-reset.jpg", b"x", 1700000200, 0o644);
        let summary = run_backup_items(&mut wiped, &items, dest.path(), None, &CancelFlag::new(), &mut sink).unwrap();
        assert_eq!(settings_runs(&wiped), 0, "no settings recollection after a reset");
        assert_eq!(std::fs::read(dest.path().join("xva-XQ-DQ44-backup/settings/settings_system.txt")).unwrap(), saved);
        assert!(wiped.pull_calls.contains(&"/sdcard/DCIM/after-reset.jpg".to_string()));
        assert!(summary.complete, "{:?}", summary.errors);
        // 초기화 판정 키는 원래 값을 유지해 다음 갱신도 같은 판정을 한다
        let manifest = load_manifest(Path::new(&summary.dir)).unwrap();
        assert_ne!(manifest.install_key, super::install_key(&mut wiped));
    }

    #[test]
    fn a_folder_with_the_backup_name_but_no_record_or_another_phone_is_never_reused() {
        let dest = tempfile::tempdir().unwrap();
        let mut d = dev_full();
        let root = dest.path().join("xva-XQ-DQ44-backup");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("note.txt"), b"user file").unwrap();
        assert!(prepare_backup_root(&mut d, dest.path()).unwrap_err().contains("기록"));
        assert_eq!(std::fs::read(root.join("note.txt")).unwrap(), b"user file");
        std::fs::remove_file(root.join("note.txt")).unwrap();
        let (_, existing) = prepare_backup_root(&mut d, dest.path()).unwrap();
        assert!(!existing, "an empty folder is adopted as a new backup");
        let mut other = dev_full();
        other.answer_shell("getprop", "[ro.product.model]: [XQ-DQ44]
[ro.serialno]: [OTHERPHONE99]
");
        assert!(prepare_backup_root(&mut other, dest.path()).unwrap_err().contains("다른 기기"));
    }

    #[test]
    fn updated_apk_replaces_the_whole_verified_cohort_without_path_collision() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let first = run_backup_items(
            &mut dev,
            &["apk".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&first.dir);
        dev.answer_shell(
            "pm list packages -3 -f",
            "package:/data/app/new/com.example.app/base.apk=com.example.app\n",
        );
        dev.add_dir("/data/app/new/com.example.app");
        dev.add_file("/data/app/new/com.example.app/base.apk", b"NEW", 101, 0o644);
        dev.add_file(
            "/data/app/new/com.example.app/split_config.apk",
            b"SPLIT",
            101,
            0o644,
        );
        assert_eq!(
            package_install_dirs(&mut dev).unwrap(),
            vec![(
                "com.example.app".into(),
                "/data/app/new/com.example.app".into()
            )]
        );
        let result = run_backup_items(
            &mut dev,
            &["apk".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(result.complete, "{:?}", result.errors);
        let manifest = load_manifest(root).unwrap();
        let entries = &manifest.items[0].entries;
        assert_eq!(
            entries.len(),
            2,
            "entries={entries:?}, pulls={:?}, shells={:?}",
            dev.pull_calls,
            dev.shell_calls
        );
        assert!(entries
            .iter()
            .all(|e| e.remote.starts_with("/data/app/new/") && !e.quarantined));
        assert_eq!(
            std::fs::read(
                root.join(
                    &entries
                        .iter()
                        .find(|e| e.remote.ends_with("/base.apk"))
                        .unwrap()
                        .local
                )
            )
            .unwrap(),
            b"NEW"
        );
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
        assert!(summary.dir.ends_with("xva-XQ-DQ44-backup"), "{}", summary.dir);
        let root = PathBuf::from(&summary.dir);
        assert!(root.join("settings/settings_secure.txt").exists());
        assert!(root.join("android-data/com.kakao.talk/db").exists());
        assert!(load_manifest(&root)
            .unwrap()
            .items
            .iter()
            .find(|i| i.id == "apk")
            .unwrap()
            .entries
            .iter()
            .all(|e| root.join(&e.local).exists()));
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
        let calls_before = d.pull_calls.len();
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
        let dcim_pulls = d.pull_calls[calls_before..]
            .iter()
            .filter(|c| c.contains("DCIM"))
            .count();
        assert_eq!(dcim_pulls, 0, "끝난 항목은 다시 받지 않는다");
        assert!(Path::new(&second.dir)
            .join("android-data/com.kakao.talk/db")
            .exists());
    }

    #[test]
    fn resume_pulls_only_missing_changed_and_corrupted_files() {
        let mut d = dev_full();
        for name in [
            "unchanged",
            "changed",
            "corrupted",
            "failed",
            "unknown-time",
            "bad?.jpg",
        ] {
            d.add_file(
                &format!("/sdcard/DCIM/{name}"),
                b"before",
                1700000001,
                0o644,
            );
        }
        d.add_file("/sdcard/DCIM/unknown-time", b"before", 0, 0o644);
        d.fail_pull("/sdcard/DCIM/failed");
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
        assert!(!first.complete);
        let root = Path::new(&first.dir);
        // Same size, newer source timestamp must be copied again.
        d.add_file("/sdcard/DCIM/changed", b"after!", 1700000002, 0o644);
        std::fs::write(root.join("sdcard/DCIM/corrupted"), b"broken").unwrap();
        d.fail_pull.clear();
        d.pull_calls.clear();
        let resumed = run_backup_items(
            &mut d,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(resumed.complete, "{:?}", resumed.errors);
        assert_eq!(
            d.pull_calls.len(),
            4,
            "unchanged normal/tar files must not be transferred"
        );
        for name in ["changed", "corrupted", "failed", "unknown-time"] {
            assert!(d.pull_calls.contains(&format!("/sdcard/DCIM/{name}")));
        }
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/changed")).unwrap(),
            b"after!"
        );
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/corrupted")).unwrap(),
            b"before"
        );
        assert!(super::super::verify::backup_summary(root).unwrap().complete);
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
        d.add_file("/sdcard/DCIM/a.jpg", b"changed source", 1700000002, 0o644);
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
            ItemStatus::Pending
        );
        assert_eq!(manifest.excluded_items, vec!["sms"]);
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
    fn resume_cancel_keeps_durable_records_and_a_later_check_detects_corruption() {
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
        .unwrap_err();
        assert!(resumed.contains("취소"));
        let resumed = BackupSummary::from(&load_manifest(root).unwrap());
        assert!(resumed.complete);
        assert!(resumed.errors.is_empty());
        assert!(
            !super::super::verify::verify_manifest(root, &mut load_manifest(root).unwrap())
                .is_empty()
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

    #[cfg(windows)]
    #[test]
    fn resume_sharing_violation_preserves_manifest_and_does_not_recollect() {
        use std::os::windows::fs::OpenOptionsExt;
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let items = ["dcim".into()];
        let first = run_backup_items(
            &mut dev,
            &items,
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&first.dir);
        let original = std::fs::read(root.join("manifest.json")).unwrap();
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(root.join("sdcard/DCIM/a.jpg"))
            .unwrap();
        dev.pull_calls.clear();
        let error = run_backup_items(
            &mut dev,
            &items,
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap_err();
        assert!(error.contains("PC_READ_IO|"), "{error}");
        assert!(!error.contains("사용자가"), "{error}");
        assert_eq!(std::fs::read(root.join("manifest.json")).unwrap(), original);
        assert!(dev.pull_calls.is_empty());
        drop(lock);
        assert!(
            run_backup_items(
                &mut dev,
                &items,
                dest.path(),
                Some(root),
                &CancelFlag::new(),
                &mut sink
            )
            .unwrap()
            .complete
        );
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
    #[test]
    fn failed_changed_source_retry_preserves_payload_original_attributes_and_repairs_bad_sidecar() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&result.dir);
        let mut manifest = load_manifest(root).unwrap();
        let before =
            super::super::source_metadata::load(root, &manifest.source_metadata[0]).unwrap();
        let attr = before
            .entries
            .iter()
            .find(|a| a.remote == "/sdcard/DCIM/a.jpg")
            .unwrap()
            .clone();
        manifest.items[0].status = ItemStatus::Partial;
        manifest.items[0].errors.push("interrupted".into());
        save_manifest_atomic(&manifest, root).unwrap();
        dev.add_file("/sdcard/DCIM/a.jpg", b"different", 101, 0o600);
        dev.fail_pull("/sdcard/DCIM/a.jpg");
        let retry = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!retry.complete);
        let manifest = load_manifest(root).unwrap();
        let after =
            super::super::source_metadata::load(root, &manifest.source_metadata[0]).unwrap();
        let preserved = after
            .entries
            .iter()
            .find(|a| a.remote == attr.remote)
            .unwrap();
        assert_eq!(preserved.backup_sha256, attr.backup_sha256);
        assert_eq!(preserved.observed_at, attr.observed_at);
        assert_eq!(preserved.size, attr.size);
        assert_eq!(
            std::fs::read(root.join("sdcard/DCIM/a.jpg")).unwrap(),
            b"photo"
        );
        std::fs::write(root.join(&manifest.source_metadata[0].path), b"bad sidecar").unwrap();
        dev.fail_pull.clear();
        let repaired = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(repaired.complete, "{:?}", repaired.errors);
    }

    #[test]
    fn disappeared_unverified_payload_is_recorded_as_a_loss_instead_of_done() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&result.dir);
        std::fs::remove_file(root.join("sdcard/DCIM/a.jpg")).unwrap();
        dev.remove_file("/sdcard/DCIM/a.jpg");
        let retry = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!retry.complete);
        let manifest = load_manifest(root).unwrap();
        assert!(manifest.items[0]
            .entries
            .iter()
            .any(|e| e.remote == "/sdcard/DCIM/a.jpg" && e.error.is_some()));
    }
    #[test]
    fn a_never_received_failure_that_disappears_is_not_a_lost_backup_receipt() {
        let mut dev = dev_full();
        dev.add_file("/sdcard/DCIM/failed.jpg", b"failed", 42, 0o644);
        dev.fail_pull("/sdcard/DCIM/failed.jpg");
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(!result.complete);
        let root = Path::new(&result.dir);
        dev.remove_file("/sdcard/DCIM/failed.jpg");
        dev.fail_pull.clear();
        let retry = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            Some(root),
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        assert!(retry.complete, "{:?}", retry.errors);
    }
    #[test]
    fn unchanged_content_with_new_mtime_converges_and_keeps_current_attributes() {
        let mut dev = dev_full();
        let dest = tempfile::tempdir().unwrap();
        let mut sink: ProgressSink = Box::new(|_| {});
        let result = run_backup_items(
            &mut dev,
            &["dcim".into()],
            dest.path(),
            None,
            &CancelFlag::new(),
            &mut sink,
        )
        .unwrap();
        let root = Path::new(&result.dir);
        let mut manifest = load_manifest(root).unwrap();
        manifest.items[0].status = ItemStatus::Partial;
        manifest.items[0].errors.push("interrupted".into());
        save_manifest_atomic(&manifest, root).unwrap();
        dev.add_file("/sdcard/DCIM/a.jpg", b"photo", 99, 0o644);
        for _ in 0..2 {
            let retry = run_backup_items(
                &mut dev,
                &["dcim".into()],
                dest.path(),
                Some(root),
                &CancelFlag::new(),
                &mut sink,
            )
            .unwrap();
            assert!(retry.complete, "{:?}", retry.errors);
        }
        let manifest = load_manifest(root).unwrap();
        let snapshot =
            super::super::source_metadata::load(root, &manifest.source_metadata[0]).unwrap();
        assert_eq!(
            snapshot
                .entries
                .iter()
                .find(|a| a.remote == "/sdcard/DCIM/a.jpg")
                .unwrap()
                .modification_time,
            99
        );
    }
}
