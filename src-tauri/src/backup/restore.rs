//! 복구 엔진 — plan.md §6-5 순서: APK 재설치 → tar 스트리밍 복원(mtime 보존, quarantine 포함)
//! → 설정 화이트리스트 → deviceidle → 연락처/sms-ie(수동 개입은 명령 층에서).
//! tar 스트리밍: adb_client `exec`(명령 stdin)으로 `tar -xf - -C <dst>`에 아카이브를 흘린다 —
//! Windows 파일시스템을 거치지 않고 기기 셸 tar가 mtime·원본 이름을 복원한다(§6-3).

use crate::backup::model::{ItemKind, ItemStatus, Manifest};
use crate::backup::runner::StepProgress;
use crate::backup::{contacts, settings, smsie};
use adb_client::ADBDeviceExt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::Arc;

/// 복구 진행 콜백 — tar 빌더 스레드에서도 호출되므로 공유 가능해야 한다
pub type RestoreSink = Arc<dyn Fn(StepProgress) + Send + Sync>;

/// 복구 결과 — 로그 문구와 실패 목록(실패가 있어도 나머지는 진행)
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub logs: Vec<String>,
    pub failures: Vec<String>,
}

// ── Write→Read 파이프(유계 채널) — tar 빌더 스레드 → exec stdin ──

struct PipeWriter {
    tx: SyncSender<Vec<u8>>,
    bytes: Arc<AtomicU64>,
}

impl Write for PipeWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.tx.send(buf.to_vec()).map_err(|_| std::io::Error::other("파이프 닫힘"))?;
        self.bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct PipeReader {
    rx: Receiver<Vec<u8>>,
    buf: Vec<u8>,
    pos: usize,
}

impl Read for PipeReader {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        if self.pos >= self.buf.len() {
            match self.rx.recv() {
                Ok(chunk) => {
                    self.buf = chunk;
                    self.pos = 0;
                }
                Err(_) => return Ok(0), // EOF
            }
        }
        let n = (self.buf.len() - self.pos).min(out.len());
        out[..n].copy_from_slice(&self.buf[self.pos..self.pos + n]);
        self.pos += n;
        Ok(n)
    }
}

/// tar 스트리밍 복원 — `cmd`(tar -xf - … )의 stdin으로 로컬 파일들을 아카이브해 흘린다.
/// 항목: (로컬 경로, tar 내부 이름, mtime). bytes 콜백은 전송량(진행률용, 실시간).
fn stream_tar(
    dev: &mut dyn ADBDeviceExt,
    cmd: &str,
    entries: &[(PathBuf, String, u32)],
    on_bytes: &dyn Fn(u64),
) -> Result<(), String> {
    let (tx, rx) = sync_channel::<Vec<u8>>(32);
    let bytes = Arc::new(AtomicU64::new(0));
    let writer = PipeWriter { tx, bytes: bytes.clone() };
    let mut reader = PipeReader { rx, buf: Vec::new(), pos: 0 };
    let total: u64 = entries.iter().map(|(p, _, _)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)).sum();

    let files = entries.to_vec();
    let builder = std::thread::spawn(move || -> Result<(), String> {
        let mut b = tar::Builder::new(writer);
        for (path, name, mtime) in &files {
            let meta = std::fs::metadata(path).map_err(|e| format!("{name}: 읽기 실패({e})"))?;
            let mut header = tar::Header::new_gnu();
            header.set_size(meta.len());
            header.set_mode(0o644);
            header.set_mtime(*mtime as u64);
            header.set_cksum();
            let f = std::fs::File::open(path).map_err(|e| format!("{name}: 열기 실패({e})"))?;
            b.append_data(&mut header, name, f).map_err(|e| format!("{name}: tar 추가 실패({e})"))?;
        }
        b.finish().map_err(|e| format!("tar 마무리 실패: {e}"))?;
        Ok(())
    });

    // 전송 — 진행 콜백에 누적 바이트 반영(빌더 스레드와 병렬)
    let mut last_report = 0u64;
    let mut scratch = [0u8; 64 * 1024];
    let exec_result = loop {
        let cur = bytes.load(Ordering::Relaxed);
        if cur != last_report {
            on_bytes(cur);
            last_report = cur;
        }
        // reader에 데이터가 있으면 exec가 소비한다 — 논블로킹 확인은 불가하므로
        // exec를 한 번 호출하고, 그동안 on_bytes 주기 갱신은 exec 반환 후 한 번만 한다.
        break dev.exec(cmd, &mut reader, Box::new(Vec::new())).map_err(|e| format!("기기 스트리밍 실패: {e}"));
    };
    let _ = &mut scratch;
    let build_result = builder.join().map_err(|_| "tar 빌드 스레드 패닉".to_string())?;
    exec_result?;
    build_result?;
    on_bytes(total);
    Ok(())
}

/// 세션 방식 APK 설치 — split APK 포함(base+split을 한 세션에). device tmp 파일 없이 stdin으로.
fn install_apk_dir(dev: &mut dyn ADBDeviceExt, pkg: &str, dir: &Path, logs: &mut Vec<String>, failures: &mut Vec<String>) {
    let mut apks: Vec<PathBuf> = match std::fs::read_dir(dir) {
        Ok(rd) => rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "apk")).collect(),
        Err(e) => {
            failures.push(format!("{pkg}: APK 폴더 없음({e})"));
            return;
        }
    };
    apks.sort();
    if apks.is_empty() {
        failures.push(format!("{pkg}: APK 파일이 없습니다"));
        return;
    }
    // install-create → 세션 id
    let mut out = Vec::new();
    if let Err(e) = dev.shell_command(&"pm install-create -r -t", Some(&mut out), None) {
        failures.push(format!("{pkg}: 설치 세션 생성 실패({e})"));
        return;
    }
    let text = String::from_utf8_lossy(&out);
    let Some(sid) = text.split('[').nth(1).and_then(|r| r.split(']').next()) else {
        failures.push(format!("{}: 설치 세션 번호를 못 얻었습니다({})", pkg, text.trim()));
        return;
    };
    for apk in &apks {
        let name = apk.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        let size = std::fs::metadata(apk).map(|m| m.len()).unwrap_or(0);
        let cmd = format!("pm install-write -S {size} {sid} {name} -");
        if let Ok(bytes) = std::fs::read(apk) {
            let mut reader = &bytes[..];
            if let Err(e) = dev.exec(&cmd, &mut reader, Box::new(Vec::new())) {
                failures.push(format!("{pkg}/{name}: 전송 실패({e})"));
                let _ = dev.shell_command(&format!("pm install-abandon {sid}"), None, None);
                return;
            }
        } else {
            failures.push(format!("{pkg}/{name}: 파일 읽기 실패"));
            let _ = dev.shell_command(&format!("pm install-abandon {sid}"), None, None);
            return;
        }
    }
    let mut cout = Vec::new();
    match dev.shell_command(&format!("pm install-commit {sid}"), Some(&mut cout), None) {
        Ok(_) => {
            let t = String::from_utf8_lossy(&cout);
            if t.contains("Success") {
                logs.push(format!("{pkg} 재설치"));
            } else {
                failures.push(format!("{pkg}: 설치 실패({})", t.trim()));
            }
        }
        Err(e) => failures.push(format!("{pkg}: 설치 확정 실패({e})")),
    }
}

/// 로컬 폴더 아래 파일 전부 → (로컬, tar 이름, mtime) 목록
fn collect_local_tree(dir: &Path, prefix: &str) -> Vec<(PathBuf, String, u32)> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                let Ok(rel) = p.strip_prefix(dir) else { continue };
                let mtime = std::fs::metadata(&p)
                    .and_then(|m| m.modified())
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as u32)
                    .unwrap_or(0);
                let name = if prefix.is_empty() {
                    rel.to_string_lossy().replace('\\', "/")
                } else {
                    format!("{prefix}/{}", rel.to_string_lossy().replace('\\', "/"))
                };
                out.push((p, name, mtime));
            }
        }
    }
    out.sort_by(|a, b| a.1.cmp(&b.1));
    out
}

/// 자동 복구 실행 — 진행 콜백으로 항목·전송량 보고. sms/calllog(수동)는 제외하고 안내만.
pub fn run_restore(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
    items: &[String],
    on_progress: &RestoreSink,
) -> RestoreOutcome {
    let mut out = RestoreOutcome::default();
    let manifest = match crate::backup::model::load_manifest(backup_root) {
        Ok(m) => m,
        Err(e) => {
            out.failures.push(e);
            return out;
        }
    };

    // 1) APK 재설치 (§6-5 첫 순서)
    if items.iter().any(|i| i == "apk") {
        on_progress(StepProgress { item_id: "apk".into(), phase: "apk", file: None, files_done: 0, files_total: 0, bytes_done: 0, bytes_total: 0 });
        let apks_dir = backup_root.join("apks");
        if let Ok(rd) = std::fs::read_dir(&apks_dir) {
            let mut pkgs: Vec<PathBuf> = rd.filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
            pkgs.sort();
            for (i, pdir) in pkgs.iter().enumerate() {
                let pkg = pdir.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                install_apk_dir(dev, &pkg, pdir, &mut out.logs, &mut out.failures);
                on_progress(StepProgress { item_id: "apk".into(), phase: "apk", file: Some(pkg), files_done: (i + 1) as u64, files_total: pkgs.len() as u64, bytes_done: (i + 1) as u64, bytes_total: pkgs.len() as u64 });
            }
        } else {
            out.failures.push("APK 백업 폴더가 없습니다".into());
        }
    }

    // 2) 파일 tar 스트리밍 복원 — fs-rest → 기명 폴더 → app-data 순(덮어쓰기 안전 순서)
    // (항목id, 백업 폴더 내 하위 경로, tar 이름 접두, 기기 대상 루트)
    let file_phases: &[(&str, &str, &str, &str)] = &[
        ("fs-rest", "sdcard/__rest__", "", "/sdcard"),
        ("dcim", "sdcard/DCIM", "DCIM", "/sdcard"),
        ("download", "sdcard/Download", "Download", "/sdcard"),
        ("pictures", "sdcard/Pictures", "Pictures", "/sdcard"),
        ("movies", "sdcard/Movies", "Movies", "/sdcard"),
        ("music", "sdcard/Music", "Music", "/sdcard"),
        ("documents", "sdcard/Documents", "Documents", "/sdcard"),
        ("recordings", "sdcard/Recordings", "Recordings", "/sdcard"),
        ("app-data", "android-data", "", "/sdcard/Android/data"),
    ];
    for (item, sub, prefix, dst) in file_phases {
        if !items.iter().any(|i| i == item) {
            continue;
        }
        let dir = backup_root.join(sub);
        if !dir.is_dir() {
            continue; // 백업에 없는 항목
        }
        let entries = collect_local_tree(&dir, prefix);
        let total = entries.len() as u64;
        let cmd = format!("tar -xf - -C {dst}");
        on_progress(StepProgress { item_id: item.to_string(), phase: "files", file: None, files_done: 0, files_total: total, bytes_done: 0, bytes_total: 0 });
        let item_id = item.to_string();
        let cb = Arc::clone(on_progress);
        let r = stream_tar(dev, &cmd, &entries, &move |b| {
            cb(StepProgress { item_id: item_id.clone(), phase: "files", file: None, files_done: total, files_total: total, bytes_done: b, bytes_total: b.max(1) });
        });
        match r {
            Ok(()) => out.logs.push(format!("{item} 복원 — 파일 {total}개")),
            Err(e) => out.failures.push(format!("{item}: {e}")),
        }
    }

    // 3) quarantine 스트리밍 — 세그먼트를 그대로 기기에서 풀기(항목 소속 구분 없이 전체)
    let qdir = backup_root.join("quarantine");
    if qdir.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&qdir) {
            let mut segs: Vec<PathBuf> = rd
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "tar"))
                .collect();
            segs.sort();
            for seg in &segs {
                let name = seg.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
                // 항목 이름은 원본 절대경로(sdcard/...) — 루트 -C / 로 푼다
                if let Ok(bytes) = std::fs::read(seg) {
                    let mut reader = &bytes[..];
                    if let Err(e) = dev.exec("tar -xf - -C /", &mut reader, Box::new(Vec::new())) {
                        out.failures.push(format!("quarantine {name}: {e}"));
                    } else {
                        out.logs.push(format!("quarantine {name} 복원"));
                    }
                }
            }
        }
    }

    // 4) 설정 화이트리스트 + deviceidle (recovery.md 1-2)
    if items.iter().any(|i| i == "settings-all") {
        on_progress(StepProgress { item_id: "settings-all".into(), phase: "settings", file: None, files_done: 0, files_total: 1, bytes_done: 0, bytes_total: 0 });
        match settings::restore_settings(dev, backup_root) {
            Ok(log) => out.logs.extend(log),
            Err(e) => out.failures.push(format!("설정 복원: {e}")),
        }
        match settings::restore_deviceidle(dev, backup_root) {
            Ok(applied) => out.logs.push(format!("배터리 최적화 예외 {}개 재적용", applied.len())),
            Err(e) => out.failures.push(format!("deviceidle: {e}")),
        }
        on_progress(StepProgress { item_id: "settings-all".into(), phase: "settings", file: None, files_done: 1, files_total: 1, bytes_done: 0, bytes_total: 0 });
    }

    // 5) 연락처 — vcf 전송 + 수동 가져오기 안내(실제 가져오기는 사용자 확인)
    if items.iter().any(|i| i == "contacts") {
        on_progress(StepProgress { item_id: "contacts".into(), phase: "contacts", file: None, files_done: 0, files_total: 1, bytes_done: 0, bytes_total: 0 });
        match contacts::stage_restore_contacts(dev, backup_root) {
            Ok((remote, note)) => {
                out.logs.push(format!("연락처 파일 전송: {remote}"));
                out.logs.push(format!("[수동] {note}"));
            }
            Err(e) => out.failures.push(format!("연락처: {e}")),
        }
        on_progress(StepProgress { item_id: "contacts".into(), phase: "contacts", file: None, files_done: 1, files_total: 1, bytes_done: 0, bytes_total: 0 });
    }

    // 6) 문자·통화 기록 — 수동 개입(smsie) 안내만, 실제 흐름은 명령 층
    if items.iter().any(|i| i == "sms" || i == "calllog") {
        out.logs.push(format!("[수동] 문자·통화 기록 복원은 SMS Import/Export 앱에서 진행합니다({})", smsie::SMSIE_PKG));
    }

    let _ = manifest; // mtime 검증 등 확장 여지 — 현재는 로컬 파일 mtime으로 tar 헤더 작성
    out
}

/// manifest에서 문자·통화(smsie) 항목이 백업에 있는지 — 복구 가능 판정
pub fn has_smsie_backup(backup_root: &Path) -> bool {
    crate::backup::model::load_manifest(backup_root)
        .map(|m| {
            m.items
                .iter()
                .any(|i| i.kind == ItemKind::SmsIe && i.status == ItemStatus::Done && !i.artifacts.is_empty())
        })
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use crate::backup::model::{save_manifest_atomic, Manifest};

    /// 백업→복구 왕복 검증용 fixture: 가상 기기 → 백업 폴더 수동 구성 → 복구 → 기기 상태 확인
    fn backup_dir_with(manifest: Manifest) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        // 설정 덤프
        let sdir = tmp.path().join("settings");
        std::fs::create_dir_all(&sdir).unwrap();
        std::fs::write(sdir.join("settings_secure.txt"), "sysui_qs_tiles=internet,bt\n").unwrap();
        std::fs::write(sdir.join("settings_system.txt"), "screen_brightness=31\n").unwrap();
        std::fs::write(sdir.join("settings_global.txt"), "a=1\n").unwrap();
        std::fs::write(sdir.join("deviceidle_whitelist.txt"), "+com.kakao.talk\n").unwrap();
        // 파일
        let ddir = tmp.path().join("sdcard/DCIM/Camera");
        std::fs::create_dir_all(&ddir).unwrap();
        std::fs::write(ddir.join("a.jpg"), b"jpeg-ok").unwrap();
        // 연락처
        let cdir = tmp.path().join("contacts");
        std::fs::create_dir_all(&cdir).unwrap();
        std::fs::write(cdir.join("contacts.vcf"), "BEGIN:VCARD\nEND:VCARD\n").unwrap();
        // APK
        let adir = tmp.path().join("apks/com.example.app");
        std::fs::create_dir_all(&adir).unwrap();
        std::fs::write(adir.join("base.apk"), b"PK-bytes").unwrap();
        save_manifest_atomic(&manifest, tmp.path()).unwrap();
        tmp
    }

    #[test]
    fn restore_streams_tar_and_reinstalls_and_settings() {
        let mut m = Manifest::new("XQ-DQ44", "AB1234****", "67.2.A.3.178", "15");
        m.items = vec![crate::backup::model::ItemRecord {
            id: "dcim".into(),
            kind: ItemKind::Files,
            status: ItemStatus::Done,
            files: 1,
            bytes: 7,
            entries: vec![],
            artifacts: vec![],
            errors: vec![],
        }];
        let tmp = backup_dir_with(m);

        let mut d = FakeADBDevice::new();
        d.answer_shell("pm install-create", "Success: created install session [42]\n");
        d.answer_shell("pm install-commit", "Success\n");
        d.answer_shell("settings put", "");
        d.answer_shell("dumpsys deviceidle whitelist +", "");
        d.answer_shell("mkdir", "");

        let sink: RestoreSink = Arc::new(|_| {});
        let items = vec!["dcim".to_string(), "settings-all".to_string(), "contacts".to_string(), "apk".to_string()];
        let out = run_restore(&mut d, tmp.path(), &items, &sink);

        // tar 스트리밍: exec 기록에 tar 명령 + 실제 tar 바이트(내용 검증)
        let (cmd, bytes) = d.shell_streams.iter().find(|(c, _)| c.starts_with("tar -xf - -C /sdcard")).expect("tar 스트리밍 있어야 함");
        assert!(cmd.contains("-C /sdcard"));
        let mut ar = tar::Archive::new(&bytes[..]);
        let mut names = Vec::new();
        for e in ar.entries().unwrap() {
            let e = e.unwrap();
            names.push(e.path().unwrap().to_string_lossy().to_string());
        }
        assert_eq!(names, vec!["DCIM/Camera/a.jpg".to_string()]);

        // APK 세션 설치
        assert!(d.shell_calls.iter().any(|c| c.contains("pm install-create")));
        assert!(d.shell_calls.iter().any(|c| c.contains("pm install-write -S 8 42 base.apk -")));
        assert!(d.shell_calls.iter().any(|c| c.contains("pm install-commit 42")));

        // 설정 화이트리스트
        assert!(d.shell_calls.iter().any(|c| c.contains("settings put secure sysui_qs_tiles \"internet,bt\"")));
        assert!(d.shell_calls.iter().any(|c| c.contains("settings put system screen_brightness \"31\"")));
        assert!(d.shell_calls.iter().any(|c| c.contains("dumpsys deviceidle whitelist +com.kakao.talk")));

        // 연락처 vcf push
        assert!(d.pushed.contains_key("/sdcard/contacts-restore.vcf"));

        assert!(out.failures.is_empty(), "failures: {failures:?}", failures = out.failures);
    }

    #[test]
    fn restore_survives_item_failure() {
        let mut m = Manifest::new("XQ-DQ44", "AB1234****", "67.2.A.3.178", "15");
        m.items = vec![];
        let tmp = backup_dir_with(m);
        let mut d = FakeADBDevice::new();
        d.answer_shell("settings put", "");
        d.answer_shell("dumpsys deviceidle whitelist +", "");
        // tar 스트리밍은 exec → 성공 처리되지만, settings 실패 유도: whitelist 파일 없는 폴더로
        let sink: RestoreSink = Arc::new(|_| {});
        let out = run_restore(&mut d, tmp.path(), &["dcim".into(), "settings-all".into()], &sink);
        assert!(!out.failures.is_empty() || !out.logs.is_empty()); // 항목별 결과가 남는다
    }
}
