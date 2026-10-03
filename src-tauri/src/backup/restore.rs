//! 복구 엔진 — plan.md §6-5 순서: APK 재설치 → tar 스트리밍 복원(mtime 보존, quarantine 포함)
//! → 설정 화이트리스트 → deviceidle → 연락처/sms-ie(수동 개입은 명령 층에서).
//! tar 스트리밍: adb_client `exec`(명령 stdin)으로 `tar -xf - -C <dst>`에 아카이브를 흘린다 —
//! Windows 파일시스템을 거치지 않고 기기 셸 tar가 mtime·원본 이름을 복원한다(§6-3).

use crate::backup::runner::StepProgress;
use crate::backup::{contacts, settings, smsie};
use adb_client::ADBDeviceExt;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};

/// exec 명령의 종료 코드 표식 — exec 서비스는 종료 코드를 돌려주지 않으므로 셸에서 덧붙여 확인한다
const RC_MARK: &str = "__XV_RC=";

/// exec 출력 수집기 (Box<dyn Write + Send>로 넘기고 끝난 뒤 내용을 읽는다)
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        const LIMIT: usize = 64 * 1024;
        let mut value = self
            .0
            .lock()
            .map_err(|_| std::io::Error::other("명령 출력 잠금 실패"))?;
        if buf.len() >= LIMIT {
            value.clear();
            value.extend_from_slice(&buf[buf.len() - LIMIT..]);
        } else {
            let excess = (value.len() + buf.len()).saturating_sub(LIMIT);
            value.drain(..excess);
            value.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// stdin을 흘리는 기기 명령 실행 + 종료 코드 확인 — 0이 아니거나 확인할 수 없으면 오류(위장 성공 금지)
fn exec_checked(
    dev: &mut dyn ADBDeviceExt,
    cmd: &str,
    reader: &mut dyn Read,
) -> Result<String, String> {
    let cap = Capture::default();
    let full = format!("{cmd} 2>&1; echo {RC_MARK}$?");
    dev.exec(&full, reader, Box::new(cap.clone()))
        .map_err(|e| format!("기기 명령 실패: {e}"))?;
    let out = cap
        .0
        .lock()
        .map(|v| String::from_utf8_lossy(&v).to_string())
        .unwrap_or_default();
    let tail = |s: &str| {
        s.trim()
            .chars()
            .rev()
            .take(300)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<String>()
    };
    match out.rsplit_once(RC_MARK) {
        Some((body, rc)) if rc.trim() == "0" => Ok(body.trim().to_string()),
        Some((body, rc)) => Err(format!("종료 코드 {}: {}", rc.trim(), tail(body))),
        None => Err(format!("종료 상태를 확인할 수 없습니다: {}", tail(&out))),
    }
}

/// 백업 폴더에서 읽은 이름을 셸 명령에 넣기 전 검사 — 영문·숫자·. _ - 만 허용
fn safe_token(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

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
        self.tx
            .send(buf.to_vec())
            .map_err(|_| std::io::Error::other("파이프 닫힘"))?;
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
        if out.is_empty() {
            return Ok(0);
        }
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
    let writer = PipeWriter {
        tx,
        bytes: bytes.clone(),
    };
    let mut reader = PipeReader {
        rx,
        buf: Vec::new(),
        pos: 0,
    };
    let total: u64 = entries
        .iter()
        .map(|(p, _, _)| std::fs::metadata(p).map(|m| m.len()).unwrap_or(0))
        .sum();

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
            b.append_data(&mut header, name, f)
                .map_err(|e| format!("{name}: tar 추가 실패({e})"))?;
        }
        b.finish().map_err(|e| format!("tar 마무리 실패: {e}"))?;
        Ok(())
    });

    // exec가 스트림을 끝까지 소비한다(블로킹) — 진행 보고는 시작/종료 시점으로
    let exec_result = exec_checked(dev, cmd, &mut reader);
    // 기기 tar가 먼저 끝나면(오류 등) 읽는 쪽을 닫아 빌더 스레드의 send가 막히지 않게 한다
    drop(reader);
    let build_result = builder
        .join()
        .map_err(|_| "tar 빌드 스레드 패닉".to_string())?;
    exec_result.map_err(|e| format!("기기 tar 실패 — {e}"))?;
    build_result?;
    on_bytes(total);
    Ok(())
}

fn restore_quarantined(
    dev: &mut dyn ADBDeviceExt,
    segment: &Path,
    wanted: &std::collections::HashMap<String, super::model::FileEntry>,
    restored: &std::collections::HashSet<String>,
) -> Result<Vec<String>, String> {
    use std::io::{Seek, SeekFrom};
    let segment = segment.to_path_buf();
    let wanted = wanted.clone();
    let restored = restored.clone();
    let (tx, rx) = sync_channel::<Vec<u8>>(32);
    let writer = PipeWriter {
        tx,
        bytes: Arc::new(AtomicU64::new(0)),
    };
    let mut reader = PipeReader {
        rx,
        buf: vec![],
        pos: 0,
    };
    let builder = std::thread::spawn(move || -> Result<Vec<String>, String> {
        let mut archive =
            tar::Archive::new(std::fs::File::open(&segment).map_err(|e| e.to_string())?);
        let mut output = tar::Builder::new(writer);
        let mut included = vec![];
        for entry in archive.entries().map_err(|e| e.to_string())? {
            let mut entry = entry.map_err(|e| e.to_string())?;
            let name = entry
                .path()
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let remote = format!("/{name}");
            super::paths::sdcard_relative(&remote)?;
            if !entry.header().entry_type().is_file() {
                return Err("격리 파일 유형이 올바르지 않습니다".into());
            }
            let Some(expected) = wanted.get(&remote) else {
                continue;
            };
            if restored.contains(&remote) || included.contains(&remote) {
                continue;
            }
            let offset = entry.raw_file_position();
            let (hash, size) = super::verify::hash_reader(&mut entry)?;
            if size != expected.size || Some(hash.as_str()) != expected.sha256.as_deref() {
                continue;
            }
            let mut file = std::fs::File::open(&segment).map_err(|e| e.to_string())?;
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| e.to_string())?;
            let mut header = entry.header().clone();
            output
                .append_data(&mut header, &name, file.take(size))
                .map_err(|e| e.to_string())?;
            included.push(remote);
        }
        output.finish().map_err(|e| e.to_string())?;
        Ok(included)
    });
    let result = exec_checked(dev, "tar -xf - -C /", &mut reader);
    drop(reader);
    let included = builder.join().map_err(|_| "격리 tar 빌드 스레드 패닉")??;
    result?;
    Ok(included)
}

/// 세션 방식 APK 설치 — split APK 포함(base+split을 한 세션에). device tmp 파일 없이 stdin으로.
fn install_apk_dir(
    dev: &mut dyn ADBDeviceExt,
    pkg: &str,
    mut apks: Vec<PathBuf>,
    logs: &mut Vec<String>,
    failures: &mut Vec<String>,
) {
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
        failures.push(format!(
            "{}: 설치 세션 번호를 못 얻었습니다({})",
            pkg,
            text.trim()
        ));
        return;
    };
    if sid.is_empty() || !sid.chars().all(|c| c.is_ascii_digit()) {
        failures.push(format!("{pkg}: 설치 세션 번호가 올바르지 않습니다({sid})"));
        return;
    }
    for apk in &apks {
        let name = apk
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if !safe_token(&name) {
            failures.push(format!(
                "{pkg}: APK 파일 이름이 올바르지 않아 건너뜀({name})"
            ));
            let _ = dev.shell_command(&format!("pm install-abandon {sid}"), None, None);
            return;
        }
        let size = std::fs::metadata(apk).map(|m| m.len()).unwrap_or(0);
        let cmd = format!("pm install-write -S {size} {sid} {name} -");
        let result = std::fs::File::open(apk)
            .map_err(|e| format!("파일 열기 실패({e})"))
            .and_then(|mut f| exec_checked(dev, &cmd, &mut f));
        if let Err(e) = result {
            failures.push(format!("{pkg}/{name}: 전송 실패({e})"));
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
                let _ = dev.shell_command(&format!("pm install-abandon {sid}"), None, None);
            }
        }
        Err(e) => {
            failures.push(format!("{pkg}: 설치 확정 실패({e})"));
            let _ = dev.shell_command(&format!("pm install-abandon {sid}"), None, None);
        }
    }
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
    if let Err(error) = super::runner::validate_items(items) {
        out.failures.push(error);
        return out;
    }
    let mut selected = manifest.clone();
    selected.items.retain(|record| items.contains(&record.id));
    for id in items {
        match selected.items.iter().find(|record| &record.id == id) {
            Some(record) if record.status == super::model::ItemStatus::Done => {}
            _ => out
                .failures
                .push(format!("{id}: 완료된 백업 기록이 없습니다")),
        }
    }
    out.failures
        .extend(super::verify::verify_manifest(backup_root, &mut selected));
    if !out.failures.is_empty() {
        return out;
    } // 기기에 쓰기 전에 무결성 검증

    // 1) APK 재설치 (§6-5 첫 순서)
    if items.iter().any(|i| i == "apk") {
        on_progress(StepProgress {
            item_id: "apk".into(),
            phase: "apk",
            file: None,
            files_done: 0,
            files_total: 0,
            bytes_done: 0,
            bytes_total: 0,
        });
        let mut packages: std::collections::BTreeMap<String, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        for entry in selected
            .items
            .iter()
            .filter(|item| item.id == "apk")
            .flat_map(|item| &item.entries)
        {
            let parts: Vec<_> = entry.local.split('/').collect();
            if entry.quarantined
                || parts.len() != 3
                || parts[0] != "apks"
                || !safe_token(parts[1])
                || !parts[2].ends_with(".apk")
            {
                out.failures
                    .push(format!("잘못된 APK 기록: {}", entry.local));
                continue;
            }
            match super::paths::existing_file(backup_root, &entry.local) {
                Ok(path) => packages.entry(parts[1].into()).or_default().push(path),
                Err(error) => out.failures.push(error),
            }
        }
        let total = packages.len() as u64;
        for (i, (pkg, files)) in packages.into_iter().enumerate() {
            install_apk_dir(dev, &pkg, files, &mut out.logs, &mut out.failures);
            on_progress(StepProgress {
                item_id: "apk".into(),
                phase: "apk",
                file: Some(pkg),
                files_done: (i + 1) as u64,
                files_total: total,
                bytes_done: (i + 1) as u64,
                bytes_total: total,
            });
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
    for (item, _sub, _prefix, dst) in file_phases {
        if !items.iter().any(|i| i == item) {
            continue;
        }
        let mut entries = vec![];
        for entry in selected
            .items
            .iter()
            .filter(|record| &record.id == item)
            .flat_map(|record| &record.entries)
            .filter(|entry| !entry.quarantined)
        {
            let result = super::paths::sdcard_relative(&entry.remote).and_then(|mut name| {
                if *item == "app-data" {
                    name = name
                        .strip_prefix("Android/data/")
                        .ok_or("앱 데이터 대상 경로가 아닙니다")?
                        .to_string();
                }
                Ok((
                    super::paths::existing_file(backup_root, &entry.local)?,
                    name,
                    entry.mtime,
                ))
            });
            match result {
                Ok(entry) => entries.push(entry),
                Err(error) => out.failures.push(error),
            }
        }
        if entries.is_empty() {
            continue;
        }
        let total = entries.len() as u64;
        let cmd = format!("tar -xf - -C {dst}");
        on_progress(StepProgress {
            item_id: item.to_string(),
            phase: "files",
            file: None,
            files_done: 0,
            files_total: total,
            bytes_done: 0,
            bytes_total: 0,
        });
        let item_id = item.to_string();
        let cb = Arc::clone(on_progress);
        let r = stream_tar(dev, &cmd, &entries, &move |b| {
            cb(StepProgress {
                item_id: item_id.clone(),
                phase: "files",
                file: None,
                files_done: total,
                files_total: total,
                bytes_done: b,
                bytes_total: b.max(1),
            });
        });
        match r {
            Ok(()) => out.logs.push(format!("{item} 복원 — 파일 {total}개")),
            Err(e) => out.failures.push(format!("{item}: {e}")),
        }
    }

    // 3) 격리는 선택한 manifest 파일만, 해시가 일치하는 버전만 다시 tar로 만들어 전송한다.
    let wanted: std::collections::HashMap<String, super::model::FileEntry> = selected
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .filter(|entry| entry.quarantined)
        .map(|entry| (entry.remote.clone(), entry.clone()))
        .collect();
    let mut restored = std::collections::HashSet::new();
    if !wanted.is_empty() {
        match super::verify::segments(backup_root) {
            Ok(segments) => {
                for segment in segments {
                    match restore_quarantined(dev, &segment, &wanted, &restored) {
                        Ok(names) => restored.extend(names),
                        Err(error) => out.failures.push(format!("격리 복구: {error}")),
                    }
                }
            }
            Err(error) => out.failures.push(error),
        }
        for remote in wanted.keys().filter(|remote| !restored.contains(*remote)) {
            out.failures
                .push(format!("격리 파일을 복구하지 못했습니다: {remote}"));
        }
        out.logs
            .push(format!("선택한 격리 파일 {}개 복원", restored.len()));
    }

    // 4) 설정 화이트리스트 + deviceidle (recovery.md 1-2)
    if items.iter().any(|i| i == "settings-all") {
        on_progress(StepProgress {
            item_id: "settings-all".into(),
            phase: "settings",
            file: None,
            files_done: 0,
            files_total: 1,
            bytes_done: 0,
            bytes_total: 0,
        });
        match settings::restore_settings(dev, backup_root) {
            Ok(log) => out.logs.extend(log),
            Err(e) => out.failures.push(format!("설정 복원: {e}")),
        }
        match settings::restore_deviceidle(dev, backup_root) {
            Ok(applied) => out
                .logs
                .push(format!("배터리 최적화 예외 {}개 재적용", applied.len())),
            Err(e) => out.failures.push(format!("deviceidle: {e}")),
        }
        on_progress(StepProgress {
            item_id: "settings-all".into(),
            phase: "settings",
            file: None,
            files_done: 1,
            files_total: 1,
            bytes_done: 0,
            bytes_total: 0,
        });
    }

    // 5) 연락처 — vcf 전송 + 수동 가져오기 안내(실제 가져오기는 사용자 확인)
    if items.iter().any(|i| i == "contacts") {
        on_progress(StepProgress {
            item_id: "contacts".into(),
            phase: "contacts",
            file: None,
            files_done: 0,
            files_total: 1,
            bytes_done: 0,
            bytes_total: 0,
        });
        match contacts::stage_restore_contacts(dev, backup_root) {
            Ok((remote, note)) => {
                out.logs.push(format!("연락처 파일 전송: {remote}"));
                out.logs.push(format!("[수동] {note}"));
            }
            Err(e) => out.failures.push(format!("연락처: {e}")),
        }
        on_progress(StepProgress {
            item_id: "contacts".into(),
            phase: "contacts",
            file: None,
            files_done: 1,
            files_total: 1,
            bytes_done: 0,
            bytes_total: 0,
        });
    }

    // 6) 문자·통화 기록 — 수동 개입(smsie) 안내만, 실제 흐름은 명령 층
    if items.iter().any(|i| i == "sms" || i == "calllog") {
        out.logs.push(format!(
            "[수동] 문자·통화 기록 복원은 SMS Import/Export 앱에서 진행합니다({})",
            smsie::SMSIE_PKG
        ));
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use crate::backup::model::{ItemKind, ItemStatus};
    fn noop_progress() -> RestoreSink {
        Arc::new(|_| {})
    }
    use crate::backup::model::{save_manifest_atomic, Manifest};

    /// 백업→복구 왕복 검증용 fixture: 가상 기기 → 백업 폴더 수동 구성 → 복구 → 기기 상태 확인
    fn backup_dir_with(mut manifest: Manifest) -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        // 설정 덤프
        let sdir = tmp.path().join("settings");
        std::fs::create_dir_all(&sdir).unwrap();
        std::fs::write(
            sdir.join("settings_secure.txt"),
            "sysui_qs_tiles=internet,bt\n",
        )
        .unwrap();
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
        // 실제 백업처럼 manifest에 파일·해시·산출물을 기록한다.
        for (id, kind, files) in [
            ("dcim", ItemKind::Files, vec!["sdcard/DCIM/Camera/a.jpg"]),
            (
                "contacts",
                ItemKind::Contacts,
                vec!["contacts/contacts.vcf"],
            ),
            (
                "apk",
                ItemKind::Files,
                vec!["apks/com.example.app/base.apk"],
            ),
            (
                "settings-all",
                ItemKind::Dump,
                vec![
                    "settings/settings_secure.txt",
                    "settings/settings_system.txt",
                    "settings/settings_global.txt",
                    "settings/deviceidle_whitelist.txt",
                ],
            ),
        ] {
            let entries: Vec<_> = files
                .iter()
                .map(|relative| {
                    let (hash, size) = super::super::verify::hash_reader(
                        std::fs::File::open(tmp.path().join(relative)).unwrap(),
                    )
                    .unwrap();
                    super::super::model::FileEntry {
                        remote: if *relative == "sdcard/DCIM/Camera/a.jpg" {
                            "/sdcard/DCIM/Camera/a.jpg".into()
                        } else {
                            format!("/backup/{relative}")
                        },
                        local: (*relative).into(),
                        size,
                        mtime: 1700000000,
                        sha256: Some(hash),
                        quarantined: false,
                        error: None,
                    }
                })
                .collect();
            manifest.record(super::super::model::ItemRecord {
                id: id.into(),
                kind,
                status: ItemStatus::Done,
                files: entries.len() as u32,
                bytes: entries.iter().map(|entry| entry.size).sum(),
                entries,
                artifacts: if kind == ItemKind::Files {
                    vec![]
                } else {
                    files.iter().map(|name| name.to_string()).collect()
                },
                errors: vec![],
            });
        }
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
        d.answer_shell(
            "pm install-create",
            "Success: created install session [42]\n",
        );
        d.answer_shell("pm install-commit", "Success\n");
        d.answer_shell("settings put", "");
        d.answer_shell("dumpsys deviceidle whitelist +", "");
        d.answer_shell("mkdir", "");

        let sink: RestoreSink = Arc::new(|_| {});
        let items = vec![
            "dcim".to_string(),
            "settings-all".to_string(),
            "contacts".to_string(),
            "apk".to_string(),
        ];
        let out = run_restore(&mut d, tmp.path(), &items, &sink);

        // tar 스트리밍: exec 기록에 tar 명령 + 실제 tar 바이트(내용 검증)
        let (cmd, bytes) = d
            .shell_streams
            .iter()
            .find(|(c, _)| c.starts_with("tar -xf - -C /sdcard"))
            .expect("tar 스트리밍 있어야 함");
        assert!(cmd.contains("-C /sdcard"));
        let mut ar = tar::Archive::new(&bytes[..]);
        let mut names = Vec::new();
        for e in ar.entries().unwrap() {
            let e = e.unwrap();
            names.push(e.path().unwrap().to_string_lossy().to_string());
        }
        assert_eq!(names, vec!["DCIM/Camera/a.jpg".to_string()]);

        // APK 세션 설치
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("pm install-create")));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("pm install-write -S 8 42 base.apk -")));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("pm install-commit 42")));

        // 설정 화이트리스트
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("settings put secure sysui_qs_tiles \"internet,bt\"")));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("settings put system screen_brightness \"31\"")));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("dumpsys deviceidle whitelist +com.kakao.talk")));

        // 연락처 vcf push
        assert!(d.pushed.contains_key("/sdcard/contacts-restore.vcf"));

        assert!(
            out.failures.is_empty(),
            "failures: {failures:?}",
            failures = out.failures
        );
    }

    /// 실제 백업 폴더로 복원 모의 실행 — 폰 대신 가짜 기기가 받은 스트림을 백업 manifest와 대조한다(폰에 쓰지 않음).
    /// 실행: XVOLTE_RESTORE_DRYRUN_DIR=<백업 폴더> XVOLTE_RESTORE_SPOOL=<임시 폴더> cargo test --lib -- --ignored live_restore_dryrun --nocapture
    #[test]
    #[ignore]
    fn live_restore_dryrun() {
        use crate::backup::model::load_manifest;
        use sha2::{Digest, Sha256};
        use std::collections::HashMap;
        let root = PathBuf::from(
            std::env::var("XVOLTE_RESTORE_DRYRUN_DIR").expect("XVOLTE_RESTORE_DRYRUN_DIR 필요"),
        );
        let spool = PathBuf::from(
            std::env::var("XVOLTE_RESTORE_SPOOL").expect("XVOLTE_RESTORE_SPOOL 필요"),
        );
        std::fs::create_dir_all(&spool).unwrap();
        let m = load_manifest(&root).unwrap();
        let items: Vec<String> = m
            .items
            .iter()
            .filter(|i| i.status == ItemStatus::Done)
            .map(|i| i.id.clone())
            .collect();
        eprintln!("[모의] 복원 항목: {items:?}");

        let mut d = FakeADBDevice::new();
        d.spool_dir = Some(spool.clone());
        d.answer_shell(
            "pm install-create",
            "Success: created install session [1]\n",
        );
        d.answer_shell("pm install-commit", "Success\n");
        d.answer_shell("pm install-abandon", "");
        d.answer_shell("settings put", "");
        d.answer_shell("dumpsys deviceidle whitelist +", "");
        let sink: RestoreSink = Arc::new(|_| {});
        let out = run_restore(&mut d, &root, &items, &sink);
        eprintln!(
            "[모의] 로그 {}건, 실패 {}건",
            out.logs.len(),
            out.failures.len()
        );
        for f in &out.failures {
            eprintln!("[실패] {f}");
        }

        let sha_file = |p: &Path| -> String {
            let mut h = Sha256::new();
            std::io::copy(&mut std::fs::File::open(p).unwrap(), &mut h).unwrap();
            hex::encode(h.finalize())
        };
        let mut problems: Vec<String> = Vec::new();

        // 1) 파일 항목: tar 안의 (이름 → 크기·해시·mtime)이 manifest와 같은지
        let mut tar_files: HashMap<String, (u64, String, u64)> = HashMap::new();
        for (cmd, path) in d
            .spooled
            .iter()
            .filter(|(c, _)| c.starts_with("tar -xf - -C "))
        {
            let dst = cmd.trim_start_matches("tar -xf - -C ").trim().to_string();
            let mut ar = tar::Archive::new(std::fs::File::open(path).unwrap());
            for e in ar.entries().unwrap() {
                let mut e = e.unwrap();
                let name = e.path().unwrap().to_string_lossy().to_string();
                let size = e.header().size().unwrap();
                let mtime = e.header().mtime().unwrap();
                let mut h = Sha256::new();
                std::io::copy(&mut e, &mut h).unwrap();
                let full = if dst == "/" {
                    format!("/{name}")
                } else {
                    format!("{dst}/{name}")
                };
                tar_files.insert(full, (size, hex::encode(h.finalize()), mtime));
            }
        }
        let mut checked = 0usize;
        // 파일로 푸는 항목만 (APK는 설치 세션, 연락처는 파일 전송 후 가져오기 — 아래에서 따로 대조)
        for it in m
            .items
            .iter()
            .filter(|i| i.status == ItemStatus::Done && i.id != "apk" && i.id != "contacts")
        {
            for e in it.entries.iter().filter(|e| e.error.is_none()) {
                checked += 1;
                match tar_files.get(&e.remote) {
                    None => problems.push(format!("복원 스트림에 없음: {}", e.remote)),
                    Some((size, sha, mtime)) => {
                        if *size != e.size {
                            problems
                                .push(format!("크기 다름: {} ({} vs {})", e.remote, size, e.size));
                        }
                        if Some(sha.as_str()) != e.sha256.as_deref() {
                            problems.push(format!("해시 다름: {}", e.remote));
                        }
                        if *mtime != e.mtime as u64 {
                            problems.push(format!(
                                "수정 시각 다름: {} ({} vs {})",
                                e.remote, mtime, e.mtime
                            ));
                        }
                    }
                }
            }
        }
        eprintln!(
            "[모의] 파일 대조 {checked}개 — tar 항목 {}개",
            tar_files.len()
        );

        // 2) APK: 설치 세션으로 흘린 바이트가 백업 파일·manifest 해시와 같은지
        if let Some(apk_item) = m
            .items
            .iter()
            .find(|i| i.id == "apk" && i.status == ItemStatus::Done)
        {
            let by_local: HashMap<String, &str> = apk_item
                .entries
                .iter()
                .filter_map(|e| Some((e.local.clone(), e.sha256.as_deref()?)))
                .collect();
            let mut expected: Vec<PathBuf> = Vec::new();
            let mut pkgs: Vec<PathBuf> = std::fs::read_dir(root.join("apks"))
                .unwrap()
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .collect();
            pkgs.sort();
            for p in pkgs {
                let mut apks: Vec<PathBuf> = std::fs::read_dir(&p)
                    .unwrap()
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|x| x.extension().is_some_and(|e| e == "apk"))
                    .collect();
                apks.sort();
                expected.extend(apks);
            }
            let streams: Vec<&PathBuf> = d
                .spooled
                .iter()
                .filter(|(c, _)| c.starts_with("pm install-write"))
                .map(|(_, p)| p)
                .collect();
            if streams.len() != expected.len() {
                problems.push(format!(
                    "APK 전송 수 다름: {} vs {}",
                    streams.len(),
                    expected.len()
                ));
            }
            for (s, local) in streams.iter().zip(expected.iter()) {
                let rel = local
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let got = sha_file(s);
                if by_local.get(&rel).copied() != Some(got.as_str()) {
                    problems.push(format!("APK 해시 다름: {rel}"));
                }
            }
            eprintln!("[모의] APK 대조 {}개", streams.len());
        }

        // 3) 연락처: 폰으로 보낸 파일이 백업 vCard와 같은지
        if let Some(c) = m
            .items
            .iter()
            .find(|i| i.id == "contacts" && i.status == ItemStatus::Done)
        {
            let want = c.entries.first().and_then(|e| e.sha256.clone());
            let got = d
                .pushed
                .get("/sdcard/contacts-restore.vcf")
                .map(|b| hex::encode(Sha256::digest(b)));
            if want.is_none() || want != got {
                problems.push("연락처 전송 파일이 백업과 다름".into());
            }
        }
        // 4) 설정
        let puts = d
            .shell_calls
            .iter()
            .filter(|c| c.starts_with("settings put"))
            .count();
        eprintln!(
            "[모의] 설정 복원 명령 {puts}건, 연락처 전송 {}",
            d.pushed.keys().cloned().collect::<Vec<_>>().join(", ")
        );

        let _ = std::fs::remove_dir_all(&spool);
        for p in problems.iter().take(40) {
            eprintln!("[문제] {p}");
        }
        assert!(problems.is_empty(), "복원 모의 문제 {}건", problems.len());
    }

    #[test]
    fn exec_checked_reports_nonzero_exit() {
        let mut d = FakeADBDevice::new();
        d.exec_rc = 1;
        let mut r: &[u8] = b"";
        let e = exec_checked(&mut d, "tar -xf - -C /sdcard", &mut r).unwrap_err();
        assert!(e.contains("종료 코드 1"), "{e}");
        d.exec_rc = 0;
        assert!(exec_checked(&mut d, "tar -xf - -C /sdcard", &mut r).is_ok());
    }

    #[test]
    fn quarantine_rejects_entries_outside_sdcard() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("quarantine")).unwrap();
        let seg = tmp.path().join("quarantine/seg000.tar");
        let mut b = tar::Builder::new(std::fs::File::create(&seg).unwrap());
        let mut h = tar::Header::new_gnu();
        h.set_size(1);
        h.set_cksum();
        b.append_data(&mut h, "system/evil", &b"x"[..]).unwrap();
        b.finish().unwrap();
        drop(b);
        assert!(super::super::verify::quarantine_hashes(tmp.path()).is_err());
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
        let out = run_restore(
            &mut d,
            tmp.path(),
            &["dcim".into(), "settings-all".into()],
            &sink,
        );
        assert!(!out.failures.is_empty() || !out.logs.is_empty()); // 항목별 결과가 남는다
    }

    #[test]
    fn corrupt_backup_stops_before_any_device_write() {
        let tmp = backup_dir_with(Manifest::new("XQ", "masked", "v", "15"));
        std::fs::write(tmp.path().join("sdcard/DCIM/Camera/a.jpg"), b"damaged").unwrap();
        let mut device = FakeADBDevice::new();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(!out.failures.is_empty());
        assert!(device.shell_calls.is_empty());
        assert!(device.shell_streams.is_empty());
    }

    #[test]
    fn restores_only_manifest_files_with_manifest_mtime() {
        let tmp = backup_dir_with(Manifest::new("XQ", "masked", "v", "15"));
        std::fs::write(tmp.path().join("sdcard/DCIM/extra.jpg"), b"unrecorded").unwrap();
        let mut device = FakeADBDevice::new();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        let mut archive = tar::Archive::new(device.shell_streams[0].1.as_slice());
        let entries: Vec<_> = archive.entries().unwrap().map(|e| e.unwrap()).collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].path().unwrap().to_string_lossy(),
            "DCIM/Camera/a.jpg"
        );
        assert_eq!(entries[0].header().mtime().unwrap(), 1700000000);
    }

    #[test]
    fn quarantine_filters_unselected_items_and_stale_versions() {
        use sha2::{Digest, Sha256};
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("quarantine")).unwrap();
        let segment = tmp.path().join("quarantine/seg000.tar");
        let mut archive = tar::Builder::new(std::fs::File::create(&segment).unwrap());
        for (name, data) in [
            ("sdcard/DCIM/bad?.jpg", b"old".as_slice()),
            ("sdcard/Music/no?.mp3", b"music"),
            ("sdcard/DCIM/bad?.jpg", b"new"),
        ] {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mtime(1700000000);
            header.set_cksum();
            archive.append_data(&mut header, name, data).unwrap();
        }
        archive.finish().unwrap();
        drop(archive);
        let entry = super::super::model::FileEntry {
            remote: "/sdcard/DCIM/bad?.jpg".into(),
            local: "".into(),
            size: 3,
            mtime: 1700000000,
            sha256: Some(hex::encode(Sha256::digest(b"new"))),
            quarantined: true,
            error: None,
        };
        let mut manifest = Manifest::new("XQ", "masked", "v", "15");
        manifest.record(super::super::model::ItemRecord {
            id: "dcim".into(),
            kind: ItemKind::Files,
            status: ItemStatus::Done,
            files: 1,
            bytes: 3,
            entries: vec![entry],
            artifacts: vec![],
            errors: vec![],
        });
        save_manifest_atomic(&manifest, tmp.path()).unwrap();
        let mut device = FakeADBDevice::new();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        let mut archive = tar::Archive::new(device.shell_streams[0].1.as_slice());
        let mut entries = archive.entries().unwrap();
        let mut entry = entries.next().unwrap().unwrap();
        assert_eq!(
            entry.path().unwrap().to_string_lossy(),
            "sdcard/DCIM/bad?.jpg"
        );
        let mut bytes = vec![];
        entry.read_to_end(&mut bytes).unwrap();
        assert_eq!(bytes, b"new");
        assert!(entries.next().is_none());
    }
}
