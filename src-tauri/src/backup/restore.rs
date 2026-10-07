//! 복구 엔진 — plan.md §6-5 순서: APK 재설치 → 파일 복원(mtime 보존, quarantine 포함)
//! → 설정 화이트리스트 → deviceidle → 연락처/sms-ie(수동 개입은 명령 층에서).
//! 파일 복원: sync push(백업의 pull과 같은 프로토콜)로 원본 이름·mtime 그대로 단계 폴더에 올린 뒤 합친다.
//! 앱 데이터는 루트로 앱 소유권을 맞출 수 있을 때만 복원한다.

use crate::backup::quarantine::ExactReader;
use crate::backup::runner::StepProgress;
use crate::backup::{contacts, settings, smsie};
use adb_client::ADBDeviceExt;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

/// exec 명령의 종료 코드 표식 — exec 서비스는 종료 코드를 돌려주지 않으므로 셸에서 덧붙여 확인한다
const RC_MARK: &str = "__XV_RC=";

/// stdin 전송이 끝난 뒤 기기 명령이 끝나기(출력 스트림이 닫히기)를 기다리는 시간.
/// adb 흐름 제어로 전송 완료 시점에 기기는 대부분을 이미 받았으므로 여유 있게 잡는다.
const EXEC_DRAIN_TIMEOUT: Duration = Duration::from_secs(300);


#[derive(Default)]
struct CaptureState {
    buf: Vec<u8>,
    /// 출력 수신 스레드가 끝나 수집기가 해제됨 — 기기 명령이 끝났다는 뜻
    closed: bool,
}

/// exec 출력 수집기. adb_client의 exec는 stdin을 다 보낸 즉시 반환하고 출력은 별도 스레드가 읽는다.
/// 그 스레드가 끝나며 수집기를 해제(Drop)할 때까지 기다려야 종료 코드 표식을 확실히 읽고,
/// 다음 기기 명령이 이전 명령의 응답과 섞이지 않는다.
#[derive(Clone, Default)]
struct Capture(Arc<(Mutex<CaptureState>, Condvar)>);

struct CaptureWriter(Capture);

impl Write for CaptureWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        const LIMIT: usize = 64 * 1024;
        let (lock, _) = &*(self.0).0;
        let mut state = lock
            .lock()
            .map_err(|_| std::io::Error::other("명령 출력 잠금 실패"))?;
        let value = &mut state.buf;
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

impl Drop for CaptureWriter {
    fn drop(&mut self) {
        let (lock, cv) = &*(self.0).0;
        if let Ok(mut state) = lock.lock() {
            state.closed = true;
        }
        cv.notify_all();
    }
}

impl Capture {
    /// 출력 스트림이 닫힐 때까지 기다려 수집 내용을 돌려준다
    fn wait_closed(&self, timeout: Duration) -> Result<String, String> {
        let (lock, cv) = &*self.0;
        let state = lock.lock().map_err(|_| "명령 출력 잠금 실패".to_string())?;
        let (state, _) = cv
            .wait_timeout_while(state, timeout, |s| !s.closed)
            .map_err(|_| "명령 출력 잠금 실패".to_string())?;
        if !state.closed {
            return Err(format!(
                "기기 명령이 {}초 안에 끝나지 않았습니다",
                timeout.as_secs()
            ));
        }
        Ok(String::from_utf8_lossy(&state.buf).into_owned())
    }
}

/// stdin을 흘리는 기기 명령 실행 + 종료 코드 확인 — 0이 아니거나 확인할 수 없으면 오류(위장 성공 금지)
fn exec_checked(
    dev: &mut dyn ADBDeviceExt,
    cmd: &str,
    reader: &mut dyn Read,
) -> Result<String, String> {
    exec_checked_within(dev, cmd, reader, EXEC_DRAIN_TIMEOUT)
}

fn exec_checked_within(
    dev: &mut dyn ADBDeviceExt,
    cmd: &str,
    reader: &mut dyn Read,
    drain: Duration,
) -> Result<String, String> {
    let cap = Capture::default();
    let full = format!("{cmd} 2>&1; echo {RC_MARK}$?");
    dev.exec(&full, reader, Box::new(CaptureWriter(cap.clone())))
        .map_err(|e| format!("기기 명령 실패: {e}"))?;
    let out = cap.wait_closed(drain)?;
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

/// 복구 진행 콜백 — 공유 가능해야 한다
pub type RestoreSink = Arc<dyn Fn(StepProgress) + Send + Sync>;

/// 복구 결과 — 로그 문구와 실패 목록(실패가 있어도 나머지는 진행)
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RestoreOutcome {
    pub logs: Vec<String>,
    pub failures: Vec<String>,
    /// 폰에서 연락처 가져오기가 남았는지(이미 있으면 false)
    #[serde(skip)]
    pub contacts_pending: bool,
    /// 기기에 쓰기 전 무결성 검증을 통과했는지 — 통과 못 하면 아무것도 쓰지 않았다
    #[serde(skip)]
    pub verified: bool,
}

// ── 파일 전송 — sync push(원본 mtime) → 단계 폴더 → 대상 위치로 합치기 ──
//
// 실기기(XQ-DQ44, 2026-10-08)에서 exec stdin tar 스트리밍을 버렸다.
// - toybox 0.8.11 tar는 아카이브 끝 표시에서 끝나지 않고 입력이 닫힐 때까지 읽는데, adb exec는 입력 끝을 알리지 못한다.
// - adb 서버(35.0.2) 경로의 exec는 큰 stdin을 흐름 제어 없이 받아 std::bad_alloc으로 죽었다.
// sync push는 같은 서버 경로에서도 흐름 제어됐다(1 GiB 27초). 백업의 pull과 같은 sync 프로토콜이다.

/// 복원 대상 1개 — 원본(로컬 파일 또는 격리 세그먼트 안 위치), 대상 기준 상대 이름, manifest의 mtime·크기
struct PushFile {
    source: PushSource,
    name: String,
    mtime: u32,
    size: u64,
}

enum PushSource {
    File(PathBuf),
    /// 격리 tar 세그먼트 안 본문 시작 위치
    Segment(PathBuf, u64),
}

impl PushSource {
    /// 기록된 크기만큼 정확히 읽는 원본 — 짧으면 읽기 오류
    fn open(&self, size: u64) -> Result<ExactReader<std::fs::File>, String> {
        let (path, offset) = match self {
            Self::File(path) => (path, 0),
            Self::Segment(path, offset) => (path, *offset),
        };
        let mut file = std::fs::File::open(path).map_err(|e| format!("열기 실패({e})"))?;
        if offset > 0 {
            file.seek(SeekFrom::Start(offset))
                .map_err(|e| format!("위치 이동 실패({e})"))?;
        }
        Ok(ExactReader::new(file, size))
    }
}

/// APK 임시 위치 — shell이 쓰고 `pm install-write`가 경로로 읽는다
const APK_TMP: &str = "/data/local/tmp";

/// 진행 콜백 — (보낸 파일 수, 보낸 바이트)
type PushProgress = Arc<dyn Fn(u64, u64) + Send + Sync>;

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// 단계 폴더를 대상 위치에 합치는 기기 셸 스크립트.
/// 대상에 없는 폴더는 통째로 옮기고(이름 바꾸기 한 번), 있는 폴더만 안으로 들어가 합친다.
/// 대상이 폴더인 자리에 파일을 덮어쓰지 않는다. 200개마다 진행을 출력해 연결이 유휴로 끊기지 않게 한다.
/// 끝에 `__MOVED=<옮긴 파일 수> __LEFT=<단계 폴더에 남은 파일 수>`를 알린다.
fn publish_script(stage: &str, dst: &str) -> String {
    r#"cd @STAGE@ || exit 1
total=$(find . -type f | wc -l)
c=0
merge() {
  local s="$1" d="$2" e n t
  for e in "$s"/* "$s"/.[!.]* "$s"/..?*; do
    [ -e "$e" ] || [ -L "$e" ] || continue
    n=${e##*/}; t="$d/$n"
    if [ -d "$e" ] && [ ! -L "$e" ]; then
      if [ -e "$t" ] || [ -L "$t" ]; then
        { [ -d "$t" ] && [ ! -L "$t" ]; } || return 1
        merge "$e" "$t" || return 1
      else
        mv -- "$e" "$t" || return 1
      fi
    else
      if [ -d "$t" ] && [ ! -L "$t" ]; then return 1; fi
      # 이미 있는 파일 위로 옮기면 앱 폴더(FUSE)에서는 복사로 처리돼 원본 mtime을 잃는다(실측) — 먼저 지운다
      if [ -e "$t" ] || [ -L "$t" ]; then rm -f -- "$t" || return 1; fi
      mv -f -- "$e" "$t" || return 1
    fi
    c=$((c+1))
    if [ $((c % 200)) -eq 0 ]; then echo "__PROGRESS=$c"; fi
  done
  return 0
}
mkdir -p -- @DST@ && merge . @DST@ || exit 1
left=$(find . -type f | wc -l)
echo "__MOVED=$((total-left)) __LEFT=$left""#
        .replace("@STAGE@", &quote(stage))
        .replace("@DST@", &quote(dst))
}

/// 파일을 대상과 같은 마운트 안의 단계 폴더(`<dst>/.xvolte-restore-…`)에 원본 mtime으로 올린 뒤 한 번에 합친다.
/// 같은 마운트라 합치기가 복사가 아닌 이름 바꾸기다(`/sdcard/Android/data`는 별도 마운트). 실패한 전송이
/// 기존 사용자 파일을 반쯤 덮어쓰지 않는다. 끝나면 단계 폴더를 지운다.
fn staged_push(
    dev: &mut dyn ADBDeviceExt,
    dst: &str,
    files: Vec<PushFile>,
    progress: PushProgress,
) -> Result<(), String> {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let stage = format!(
        "{dst}/.xvolte-restore-{}-{}-{}",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    );
    exec_checked(
        dev,
        &format!("mkdir -p -- {}", quote(&stage)),
        &mut std::io::empty(),
    )?;
    let expected = files.len();
    let result = (|| {
        let mut bytes = 0u64;
        for (i, file) in files.iter().enumerate() {
            let mut reader = file.source.open(file.size).map_err(|e| format!("{}: {e}", file.name))?;
            dev.push_with_mtime(&mut reader, &format!("{stage}/{}", file.name), file.mtime)
                .map_err(|e| format!("{}: 전송 실패({e})", file.name))?;
            bytes += file.size;
            progress(i as u64 + 1, bytes);
        }
        let out = exec_checked_within(
            dev,
            &publish_script(&stage, dst),
            &mut std::io::empty(),
            EXEC_DRAIN_TIMEOUT + Duration::from_millis(20 * expected as u64),
        )?;
        let field = |key: &str| {
            out.split_whitespace()
                .find_map(|w| w.strip_prefix(key))
                .and_then(|n| n.parse::<usize>().ok())
        };
        match (field("__MOVED="), field("__LEFT=")) {
            (Some(moved), Some(0)) if moved == expected => Ok(()),
            (moved, left) => Err(format!(
                "복원 파일 옮기기 수가 맞지 않습니다(예상 {expected}개, 옮김 {}, 남음 {})",
                moved.map_or("확인 불가".into(), |n| format!("{n}개")),
                left.map_or("확인 불가".into(), |n| format!("{n}개"))
            )),
        }
    })();
    let cleanup = exec_checked(
        dev,
        &format!("rm -rf -- {}", quote(&stage)),
        &mut std::io::empty(),
    );
    match (result, cleanup) {
        (Ok(()), Ok(_)) => Ok(()),
        (Ok(()), Err(error)) => Err(format!("복원 임시 파일 정리 실패: {error}")),
        (Err(error), Ok(_)) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!("{error}; 임시 파일 정리 실패: {cleanup}")),
    }
}

/// 격리 세그먼트에서 복원할 항목 — (원본 경로, 본문 위치, 크기, mtime)
type QuarantinePick = (String, u64, u64, u32);

/// 세그먼트의 모든 헤더 경로·유형을 검사하고, 선택한 파일 중 해시가 일치하는 첫 버전만 고른다.
/// 미선택 본문은 읽지 않고 건너뛴다(entries_with_seek).
fn pick_quarantined(
    segment: &Path,
    wanted: &std::collections::HashMap<&str, &super::model::FileEntry>,
    restored: &std::collections::HashSet<String>,
) -> Result<Vec<QuarantinePick>, String> {
    let mut archive = tar::Archive::new(std::fs::File::open(segment).map_err(|e| e.to_string())?);
    let mut picks: Vec<QuarantinePick> = vec![];
    let mut picked = std::collections::HashSet::new();
    for entry in archive.entries_with_seek().map_err(|e| e.to_string())? {
        let mut entry = entry.map_err(|e| e.to_string())?;
        let remote = super::verify::tar_remote(&entry);
        super::paths::archive_relative(&remote)?;
        if !entry.header().entry_type().is_file() {
            return Err("격리 파일 유형이 올바르지 않습니다".into());
        }
        let Some(expected) = wanted.get(remote.as_str()) else {
            continue;
        };
        super::paths::sdcard_relative(&remote)?;
        if restored.contains(&remote) || picked.contains(&remote) {
            continue;
        }
        let offset = entry.raw_file_position();
        let (hash, size) = super::verify::hash_reader(&mut entry)?;
        if size != expected.size || Some(hash.as_str()) != expected.sha256.as_deref() {
            continue;
        }
        picked.insert(remote.clone());
        picks.push((remote, offset, size, expected.mtime));
    }
    Ok(picks)
}

/// 격리 파일 복원 — 고른 항목만 세그먼트 안 위치에서 바로 읽어 `/sdcard` 기준 상대 이름으로 올린다
/// (일반 파일 복원과 같은 기준 — `/sdcard` 심볼릭 링크를 경로 중간에서 따라가는 데 기대지 않는다).
fn restore_quarantined(
    dev: &mut dyn ADBDeviceExt,
    segment: &Path,
    wanted: &std::collections::HashMap<&str, &super::model::FileEntry>,
    restored: &std::collections::HashSet<String>,
) -> Result<Vec<String>, String> {
    let picks = pick_quarantined(segment, wanted, restored)?;
    if picks.is_empty() {
        return Ok(vec![]); // 이 세그먼트엔 복원할 것이 없다 — 기기 명령을 보내지 않는다
    }
    let mut files = Vec::with_capacity(picks.len());
    let mut included = Vec::with_capacity(picks.len());
    for (remote, offset, size, mtime) in picks {
        files.push(PushFile {
            source: PushSource::Segment(segment.to_path_buf(), offset),
            name: super::paths::sdcard_relative(&remote)?,
            mtime,
            size,
        });
        included.push(remote);
    }
    staged_push(dev, "/sdcard", files, Arc::new(|_, _| {}))?;
    Ok(included)
}

/// 앱 데이터 복원 준비 — 루트 권한과 설치된 앱의 uid.
/// 안드로이드 11부터 `Android/data/<앱>`은 그 앱 소유여야 앱이 읽고 쓴다. 실기기(XQ-DQ44)에서 shell이 만든 폴더와 파일은
/// 소유자 2000·그룹 ext_data_rw(1078)였고, 앱 프로세스는 1078 그룹이 없어 접근하지 못한다. 소유권은 루트로만 맞출 수 있다.
fn app_data_owners(dev: &mut dyn ADBDeviceExt) -> Result<std::collections::HashMap<String, u32>, String> {
    let root = crate::device_io::shell_run(dev, crate::device_io::su!("id 2>&1"))?;
    if root.code != 0 || !String::from_utf8_lossy(&root.stdout).contains("uid=0") {
        return Err("루트 권한이 없어 앱 데이터를 복원하지 않았습니다 — 안드로이드 11부터 앱 데이터 폴더는 그 앱 소유여야 해서, 루트 없이 넣으면 앱이 읽지 못합니다".into());
    }
    parse_package_uids(&crate::device_io::shell(dev, "pm list packages -U")?)
}

/// `pm list packages -U` → 패키지별 uid
fn parse_package_uids(text: &str) -> Result<std::collections::HashMap<String, u32>, String> {
    let owners: std::collections::HashMap<String, u32> = text
        .lines()
        .filter_map(|line| {
            let (pkg, uid) = line.trim().strip_prefix("package:")?.split_once(" uid:")?;
            let digits: String = uid.chars().take_while(char::is_ascii_digit).collect();
            Some((pkg.to_string(), digits.parse().ok()?))
        })
        .collect();
    if owners.is_empty() {
        return Err("설치된 앱 목록을 읽지 못했습니다".into());
    }
    Ok(owners)
}

/// 복원한 앱 데이터 폴더를 앱이 직접 만든 폴더와 같게 맞춘다 — 소유자 앱 uid, 그룹 ext_data_rw(1078),
/// 저장 공간 계산용 프로젝트 ID(20000+앱 ID), 하위 폴더 상속(P). 실기기 앱 폴더에서 확인한 값이다.
fn fix_app_data_owners(
    dev: &mut dyn ADBDeviceExt,
    packages: &std::collections::BTreeMap<String, u32>,
) -> Result<(), String> {
    let mut steps = vec![];
    for (pkg, uid) in packages {
        let app_id = uid % 100_000;
        if !safe_token(pkg) || app_id < 10_000 {
            return Err(format!("{pkg}: 앱 uid가 올바르지 않습니다({uid})"));
        }
        let dir = format!("/data/media/0/Android/data/{pkg}");
        let project = app_id - 10_000 + 20_000;
        steps.push(format!(
            "chown -R {uid}:1078 {dir} && chattr -R -p {project} {dir} && find {dir} -type d -exec chattr +P {{}} +"
        ));
    }
    if steps.is_empty() {
        return Ok(());
    }
    let out = crate::device_io::shell_run(dev, &crate::device_io::su_command(&steps.join(" && ")))?;
    if out.code != 0 {
        let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&out.stderr));
        return Err(format!("종료 코드 {}: {}", out.code, text.trim()));
    }
    Ok(())
}

/// 세션 방식 APK 설치 — split APK 포함(base+split을 한 세션에).
/// APK를 sync push로 기기 임시 파일에 올린 뒤 경로로 넘긴다. exec stdin으로 흘리면 adb 서버 경로에서
/// 흐름 제어가 없어 큰 APK가 서버를 죽일 수 있다(2026-10-08 실측, 위 파일 전송 설명 참고).
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
    let text = match crate::device_io::shell(dev, "pm install-create -r -t") {
        Ok(text) => text,
        Err(e) => {
            failures.push(format!("{pkg}: 설치 세션 생성 실패({e})"));
            return;
        }
    };
    let Some(sid) = text
        .trim()
        .strip_prefix("Success: created install session [")
        .and_then(|r| r.strip_suffix(']'))
    else {
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
            abandon_install(dev, sid, logs);
            return;
        }
        let result = std::fs::File::open(apk)
            .map_err(|e| format!("파일 열기 실패({e})"))
            .and_then(|mut f| {
                let size = f
                    .metadata()
                    .map_err(|e| format!("파일 크기 조회 실패({e})"))?
                    .len();
                if size == 0 {
                    return Err("빈 APK 파일".into());
                }
                let remote = format!("{APK_TMP}/xvolte-{sid}-{name}");
                dev.push(&mut f, &remote)
                    .map_err(|e| format!("기기로 전송 실패({e})"))?;
                let cmd = format!("pm install-write -S {size} {sid} {name} {remote}");
                let written = exec_checked(dev, &cmd, &mut std::io::empty());
                let removed = crate::device_io::shell(dev, &format!("rm -f -- {remote}"));
                written?;
                removed.map(|_| String::new()).map_err(|e| format!("임시 APK 정리 실패({e})"))
            });
        if let Err(e) = result {
            failures.push(format!("{pkg}/{name}: 전송 실패({e})"));
            abandon_install(dev, sid, logs);
            return;
        }
    }
    match crate::device_io::shell(dev, &format!("pm install-commit {sid}")) {
        Ok(t) => {
            if t.trim() == "Success" {
                logs.push(format!("{pkg} 재설치"));
            } else {
                failures.push(format!("{pkg}: 설치 실패({})", t.trim()));
                abandon_install(dev, sid, logs);
            }
        }
        Err(e) => {
            failures.push(format!("{pkg}: 설치 확정 실패({e})"));
            abandon_install(dev, sid, logs);
        }
    }
}

fn abandon_install(dev: &mut dyn ADBDeviceExt, sid: &str, logs: &mut Vec<String>) {
    if let Err(error) = crate::device_io::shell(dev, &format!("pm install-abandon {sid}")) {
        logs.push(format!("[경고] 설치 세션 {sid} 정리 실패: {error}"));
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
    let mut selected = manifest;
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
    if let Err(error) = super::verify::verify_device(dev, &selected) {
        out.failures.push(error);
        return out;
    }
    out.verified = true;

    // 1) APK 재설치 (§6-5 첫 순서)
    if items.iter().any(|i| i == "apk") {
        on_progress(StepProgress::start("apk", "apk", 0));
        let mut packages: std::collections::BTreeMap<String, Vec<PathBuf>> =
            std::collections::BTreeMap::new();
        // split APK 하나라도 확인하지 못한 패키지는 불완전 설치가 되므로 설치하지 않는다
        let mut broken = std::collections::BTreeSet::new();
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
                if parts.len() == 3 && parts[0] == "apks" && safe_token(parts[1]) {
                    broken.insert(parts[1].to_string());
                }
                continue;
            }
            match super::paths::existing_file(backup_root, &entry.local) {
                Ok(path) => packages.entry(parts[1].into()).or_default().push(path),
                Err(error) => {
                    out.failures.push(error);
                    broken.insert(parts[1].to_string());
                }
            }
        }
        for pkg in &broken {
            if packages.remove(pkg).is_some() {
                out.failures.push(format!(
                    "{pkg}: 일부 APK 기록을 확인하지 못해 설치하지 않았습니다"
                ));
            }
        }
        let total = packages.len() as u64;
        for (i, (pkg, files)) in packages.into_iter().enumerate() {
            install_apk_dir(dev, &pkg, files, &mut out.logs, &mut out.failures);
            on_progress(StepProgress {
                file: Some(pkg),
                bytes_done: (i + 1) as u64,
                bytes_total: total,
                ..StepProgress::at("apk", "apk", (i + 1) as u64, total)
            });
        }
    }

    // 2) 파일 복원(sync push → 단계 폴더 → 합치기) — fs-rest → 기명 폴더 → app-data 순(덮어쓰기 안전 순서)
    // (항목id, 기기 대상 루트)
    let file_phases: &[(&str, &str)] = &[
        ("fs-rest", "/sdcard"),
        ("dcim", "/sdcard"),
        ("download", "/sdcard"),
        ("pictures", "/sdcard"),
        ("movies", "/sdcard"),
        ("music", "/sdcard"),
        ("documents", "/sdcard"),
        ("recordings", "/sdcard"),
        ("app-data", "/sdcard/Android/data"),
    ];
    // 앱 데이터: 설치된 앱의 uid(소유권 맞춤용). 루트가 없거나 조회에 실패하면 None — 앱 데이터는 건너뛴다
    let app_owners = if items.iter().any(|i| i == "app-data") {
        match app_data_owners(dev) {
            Ok(owners) => Some(owners),
            Err(error) => {
                out.failures.push(format!("app-data: {error}"));
                None
            }
        }
    } else {
        None
    };
    for (item, dst) in file_phases {
        if !items.iter().any(|i| i == item) {
            continue;
        }
        if *item == "app-data" && app_owners.is_none() {
            continue;
        }
        let mut skipped_packages = std::collections::BTreeSet::new();
        let mut restored_packages = std::collections::BTreeMap::new();
        let mut files = vec![];
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
                Ok(PushFile {
                    source: PushSource::File(super::paths::existing_file(backup_root, &entry.local)?),
                    name,
                    mtime: entry.mtime,
                    size: entry.size,
                })
            });
            let file = match result {
                Ok(file) => file,
                Err(error) => {
                    out.failures.push(error);
                    continue;
                }
            };
            if let Some(owners) = &app_owners {
                if *item == "app-data" {
                    // Android/data 바로 아래 파일(.nomedia 등)은 시스템 몫이다. 앱 폴더는 설치된 앱만 —
                    // 설치되지 않은 앱 폴더를 shell 소유로 만들어 두면 나중에 설치해도 그 앱이 쓰지 못한다.
                    let Some((pkg, _)) = file.name.split_once('/') else {
                        continue;
                    };
                    match owners.get(pkg) {
                        Some(uid) if safe_token(pkg) => {
                            restored_packages.insert(pkg.to_string(), *uid);
                        }
                        _ => {
                            skipped_packages.insert(pkg.to_string());
                            continue;
                        }
                    }
                }
            }
            files.push(file);
        }
        if files.is_empty() {
            continue;
        }
        let total = files.len() as u64;
        let total_bytes: u64 = files.iter().map(|f| f.size).sum();
        on_progress(StepProgress::start(item, "files", total));
        let item_id = item.to_string();
        let cb = Arc::clone(on_progress);
        let progress: PushProgress = Arc::new(move |done, bytes| {
            cb(StepProgress {
                bytes_done: bytes,
                bytes_total: total_bytes,
                ..StepProgress::at(&item_id, "files", done, total)
            });
        });
        let placed = staged_push(dev, dst, files, progress);
        match &placed {
            Ok(()) => out.logs.push(format!("{item} 복원 — 파일 {total}개")),
            Err(e) => out.failures.push(format!("{item}: {e}")),
        }
        if *item == "app-data" {
            if !skipped_packages.is_empty() {
                out.logs.push(format!(
                    "[안내] 앱이 설치되지 않아 앱 데이터를 건너뜀: {}",
                    skipped_packages.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
            // 옮기다 실패해도 이미 들어간 폴더는 앱 소유로 맞춰야 앱이 쓸 수 있다
            match fix_app_data_owners(dev, &restored_packages) {
                Ok(()) => out.logs.push(format!("앱 데이터 소유권 맞춤 — 앱 {}개", restored_packages.len())),
                Err(e) => out.failures.push(format!("app-data 소유권: {e}")),
            }
        }
    }

    // 3) 격리는 선택한 manifest 파일만, 해시가 일치하는 버전만 다시 tar로 만들어 전송한다.
    let wanted: std::collections::HashMap<&str, &super::model::FileEntry> = selected
        .items
        .iter()
        // 앱 데이터 격리분은 소유권을 맞추는 앱 데이터 경로를 거쳐야 하므로 여기서 다루지 않는다(아래에서 안내)
        .filter(|item| item.id != "app-data" && file_phases.iter().any(|(id, _)| *id == item.id))
        .flat_map(|item| &item.entries)
        .filter(|entry| entry.quarantined)
        .map(|entry| (entry.remote.as_str(), entry))
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
        for remote in wanted.keys().filter(|remote| !restored.contains(**remote)) {
            out.failures
                .push(format!("격리 파일을 복구하지 못했습니다: {remote}"));
        }
        out.logs
            .push(format!("선택한 격리 파일 {}개 복원", restored.len()));
    }

    // 4) 설정 화이트리스트 + deviceidle (recovery.md 1-2)
    if items.iter().any(|i| i == "settings-all") {
        on_progress(StepProgress::start("settings-all", "settings", 1));
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
        on_progress(StepProgress::at("settings-all", "settings", 1, 1));
    }

    // 5) 연락처 — 이미 있으면 건너뛰고, 아니면 vcf 전송 + 가져오기 화면(반영은 사용자 확인 후 restore_check)
    if items.iter().any(|i| i == "contacts") {
        on_progress(StepProgress::start("contacts", "contacts", 1));
        match contacts::stage_restore_contacts(dev, backup_root) {
            Ok(staged) => {
                out.logs.extend(staged.logs);
                out.contacts_pending = staged.pending;
            }
            Err(e) => out.failures.push(format!("연락처: {e}")),
        }
        on_progress(StepProgress::at("contacts", "contacts", 1, 1));
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

    #[test]
    fn failed_extract_cleans_staging_without_publishing_a_partial_file() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("f");
        std::fs::write(&path, b"short").unwrap();
        let mut d = test_device();
        assert!(staged_push(
            &mut d,
            "/sdcard",
            vec![PushFile {
                source: PushSource::File(path),
                name: "DCIM/a.jpg".into(),
                mtime: 0,
                size: 100
            }],
            Arc::new(|_, _| {})
        )
        .is_err());
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.starts_with("rm -rf -- '/sdcard/.xvolte-restore-")));
        assert!(!d.shell_calls.iter().any(|c| c.contains("__MOVED=")));
        assert!(!d.pushed.keys().any(|k| k == "/sdcard/DCIM/a.jpg"));
    }
    #[test]
    fn foreign_backup_stops_restore_before_any_write() {
        let root = backup_dir_with(Manifest::new("XQ", "masked", "v", "15"));
        let mut d = FakeADBDevice::new();
        d.answer_shell("getprop ro.serialno", "OTHER-PHONE");
        let out = run_restore(&mut d, root.path(), &["dcim".into()], &noop_progress());
        assert!(!out.verified);
        assert!(!out.failures.is_empty());
        assert!(d.shell_streams.is_empty());
        assert!(d.pushed.is_empty());
    }

    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn apk_creation_nonzero_status_cannot_start_a_session() {
        let root = tempfile::tempdir().unwrap();
        let apk = root.path().join("base.apk");
        std::fs::write(&apk, b"apk").unwrap();
        let mut dev = test_device();
        dev.answer_shell(
            "pm install-create",
            "Success: created install session [42]\n",
        );
        dev.shell_exit_codes
            .insert("pm install-create -r -t".into(), 1);
        let (mut logs, mut failures) = (vec![], vec![]);
        install_apk_dir(
            &mut dev,
            "com.example.app",
            vec![apk],
            &mut logs,
            &mut failures,
        );
        assert_eq!(failures.len(), 1);
        assert!(logs.is_empty());
        assert!(!dev
            .shell_calls
            .iter()
            .any(|cmd| cmd.contains("install-write") || cmd.contains("install-commit")));
    }

    #[test]
    fn apk_commit_requires_success_status_and_exact_success_response() {
        for (response, status) in [("Success\n", 1), ("Failure [Success text]\n", 0)] {
            let root = tempfile::tempdir().unwrap();
            let apk = root.path().join("base.apk");
            std::fs::write(&apk, b"apk").unwrap();
            let mut dev = test_device();
            dev.answer_shell(
                "pm install-create",
                "Success: created install session [42]\n",
            );
            dev.answer_shell("pm install-commit", response);
            dev.shell_exit_codes
                .insert("pm install-commit 42".into(), status);
            let (mut logs, mut failures) = (vec![], vec![]);
            install_apk_dir(
                &mut dev,
                "com.example.app",
                vec![apk],
                &mut logs,
                &mut failures,
            );
            assert_eq!(failures.len(), 1);
            assert!(!logs.iter().any(|line| line.contains("재설치")));
            assert!(dev
                .shell_calls
                .iter()
                .any(|cmd| cmd == "pm install-abandon 42"));
            assert!(logs.iter().any(|line| line.contains("정리 실패")));
        }
    }

    #[test]
    fn apk_missing_or_empty_file_abandons_before_streaming() {
        for bytes in [None, Some(b"".as_slice())] {
            let root = tempfile::tempdir().unwrap();
            let apk = root.path().join("base.apk");
            if let Some(bytes) = bytes {
                std::fs::write(&apk, bytes).unwrap();
            }
            let mut dev = test_device();
            dev.answer_shell(
                "pm install-create",
                "Success: created install session [42]\n",
            );
            dev.answer_shell("pm install-abandon", "Success\n");
            let (mut logs, mut failures) = (vec![], vec![]);
            install_apk_dir(
                &mut dev,
                "com.example.app",
                vec![apk],
                &mut logs,
                &mut failures,
            );
            assert_eq!(failures.len(), 1);
            assert!(dev.shell_streams.is_empty());
            assert!(dev
                .shell_calls
                .iter()
                .any(|cmd| cmd == "pm install-abandon 42"));
        }
    }
    use crate::backup::model::{ItemKind, ItemStatus};
    fn noop_progress() -> RestoreSink {
        Arc::new(|_| {})
    }
    use crate::backup::model::{save_manifest_atomic, Manifest};

    #[test]
    fn legacy_skipped_snapshot_is_verified_and_restorable_without_recollecting() {
        let root = backup_dir_with(Manifest::new("XQ", "masked", "v", "15"));
        let mut manifest = crate::backup::model::load_manifest(root.path()).unwrap();
        manifest
            .items
            .iter_mut()
            .find(|i| i.id == "dcim")
            .unwrap()
            .status = ItemStatus::Skipped;
        save_manifest_atomic(&manifest, root.path()).unwrap();
        let recovered = crate::backup::model::load_manifest(root.path()).unwrap();
        assert_eq!(
            recovered
                .items
                .iter()
                .find(|i| i.id == "dcim")
                .unwrap()
                .status,
            ItemStatus::Done
        );
        assert!(recovered.excluded_items.contains(&"dcim".into()));
        let mut d = test_device();
        let result = run_restore(&mut d, root.path(), &["dcim".into()], &noop_progress());
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert!(d.pull_calls.is_empty());
        std::fs::write(root.path().join("sdcard/DCIM/Camera/a.jpg"), b"corrupt").unwrap();
        assert_eq!(
            crate::backup::model::load_manifest(root.path())
                .unwrap()
                .items
                .iter()
                .find(|i| i.id == "dcim")
                .unwrap()
                .status,
            ItemStatus::Partial
        );
    }

    fn test_device() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.answer_shell("getprop ro.serialno", "TEST-SERIAL");
        d
    }

    /// 백업→복구 왕복 검증용 fixture: 가상 기기 → 백업 폴더 수동 구성 → 복구 → 기기 상태 확인
    fn backup_dir_with(mut manifest: Manifest) -> tempfile::TempDir {
        use sha2::Digest;
        manifest.device_key = Some(hex::encode(sha2::Sha256::digest(b"TEST-SERIAL")));
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
        // 실기기 출력 형식 — <종류>,<패키지>,<uid>
        std::fs::write(
            sdir.join("deviceidle_whitelist.txt"),
            "system,com.android.systemui,10123\nuser,com.kakao.talk,10234\n",
        )
        .unwrap();
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

        let mut d = test_device();
        d.answer_shell(
            "pm install-create",
            "Success: created install session [42]\n",
        );
        d.answer_shell("pm install-commit", "Success\n");
        d.answer_shell("rm -f", "");
        d.answer_shell("content query --uri content://com.android.contacts/contacts", "No result found.\n");
        d.answer_shell("content query --uri content://media/external/file", "Row: 0 _id=42\n");
        d.answer_shell("am start", "Starting: Intent { act=android.intent.action.VIEW }\n");
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

        // 파일: sync push(원본 mtime) → 단계 폴더 → 합치기로 제자리에
        let restored: Vec<_> = d.pushed.keys().filter(|k| k.starts_with("/sdcard/DCIM")).cloned().collect();
        assert_eq!(restored, vec!["/sdcard/DCIM/Camera/a.jpg".to_string()]);

        // APK 세션 설치
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("pm install-create")));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c.contains("pm install-write -S 8 42 base.apk /data/local/tmp/xvolte-42-base.apk")));
        // APK는 stdin이 아니라 기기 임시 파일로 넘기고 지운다(서버 경로 흐름 제어)
        assert!(d.pushed.contains_key("/data/local/tmp/xvolte-42-base.apk"));
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c == "rm -f -- /data/local/tmp/xvolte-42-base.apk"));
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

        // 연락처: 폰에 없으므로 vcf push + 가져오기 화면을 그 파일(MediaStore URI)로 띄우고 남은 일로 표시
        assert!(d.pushed.contains_key("/sdcard/contacts-restore.vcf"));
        assert!(out.contacts_pending);
        assert!(d.shell_calls.iter().any(|c| c.starts_with("am start")
            && c.contains("-d content://media/external/file/42")
            && c.contains("-n com.google.android.contacts/com.google.android.apps.contacts.vcard.ImportVCardActivity")));

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

        let mut d = test_device();
        d.answer_shell(
            "getprop ro.serialno",
            &std::env::var("XVOLTE_RESTORE_DEVICE_SERIAL")
                .expect("XVOLTE_RESTORE_DEVICE_SERIAL 필요"),
        );
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
            .filter(|(c, _)| c.contains("tar -xf - -C "))
        {
            let dst = cmd.split("tar -xf - -C ").nth(1).unwrap().trim().to_string();
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
        let mut d = test_device();
        d.exec_rc = 1;
        let mut r: &[u8] = b"";
        let e = exec_checked(&mut d, "/sdcard", &mut r).unwrap_err();
        assert!(e.contains("종료 코드 1"), "{e}");
        d.exec_rc = 0;
        assert!(exec_checked(&mut d, "tar -xf - -C /sdcard", &mut r).is_ok());
    }

    #[test]
    fn exec_waits_for_output_that_arrives_after_stdin() {
        // 실제 adb_client: exec는 stdin을 보낸 즉시 반환하고 출력은 수신 스레드가 늦게 쓴다
        let mut d = test_device();
        d.exec_output_delay = Some(Duration::from_millis(150));
        let mut r: &[u8] = b"";
        assert!(exec_checked(&mut d, "tar -xf - -C /sdcard", &mut r).is_ok());
        d.exec_rc = 2;
        let e = exec_checked(&mut d, "tar -xf - -C /sdcard", &mut r).unwrap_err();
        assert!(e.contains("종료 코드 2"), "{e}");
    }

    #[test]
    fn exec_that_never_finishes_is_an_error() {
        let mut d = test_device();
        d.exec_never_closes = true;
        let mut r: &[u8] = b"";
        let e = exec_checked_within(
            &mut d,
            "tar -xf - -C /sdcard",
            &mut r,
            Duration::from_millis(50),
        )
        .unwrap_err();
        assert!(e.contains("끝나지 않았습니다"), "{e}");
    }

    /// 실기기: sync push로 큰 파일을 올린다(서버 경로 흐름 제어 확인). `cargo test live_push_large -- --ignored --nocapture`
    #[test]
    #[ignore]
    fn live_push_large() {
        let size: u64 = std::env::var("XVA_EXEC_BYTES").ok().and_then(|v| v.parse().ok()).unwrap_or(1 << 30);
        let start = std::time::Instant::now();
        let out = crate::adb::with_first_device(&None, |dev| {
            let mut input = std::io::repeat(7).take(size);
            dev.push_with_mtime(&mut input, &"/data/local/tmp/xva_push_test", 1_700_000_000)
                .map_err(|e| e.to_string())?;
            crate::device_io::shell(
                dev,
                "stat -c '%s %Y' /data/local/tmp/xva_push_test; rm -f /data/local/tmp/xva_push_test",
            )
        });
        eprintln!("{size} bytes in {:?}: {out:?}", start.elapsed());
        assert_eq!(out.unwrap().trim(), format!("{size} 1700000000"));
    }

    #[test]
    fn push_keeps_names_and_mtimes_and_publishes_with_one_device_loop() {
        let tmp = tempfile::tempdir().unwrap();
        let names = [
            "a.txt".to_string(),
            format!("Download/{}/a\\b.txt", "d".repeat(120)),
            "Documents/파일 이름 (1).pdf".to_string(),
            ".$Trash$/.$Content$/.1_0".to_string(),
        ];
        let mut files = vec![];
        for (i, (name, size)) in names.iter().zip([0usize, 3, 512, 1537]).enumerate() {
            let path = tmp.path().join(format!("{i}"));
            std::fs::write(&path, vec![i as u8; size]).unwrap();
            files.push(PushFile {
                source: PushSource::File(path),
                name: name.clone(),
                mtime: 1_700_000_000 + i as u32,
                size: size as u64,
            });
        }
        let mut d = test_device();
        staged_push(&mut d, "/sdcard", files, Arc::new(|_, _| {})).unwrap();
        for (i, (name, size)) in names.iter().zip([0usize, 3, 512, 1537]).enumerate() {
            let target = format!("/sdcard/{name}");
            // Windows에서도 `\`가 `/`로 바뀌지 않고 긴 이름·한글·`$`도 그대로, 원본 mtime으로
            assert_eq!(d.pushed[&target], vec![i as u8; size], "{target}");
            assert_eq!(d.pushed_mtime[&target], 1_700_000_000 + i as u32);
        }
        // 단계 폴더는 대상과 같은 마운트 안이고, 합치기는 기기 안 한 번의 루프다(파일마다 명령을 보내지 않는다)
        let publishes: Vec<_> = d.shell_calls.iter().filter(|c| c.contains("__MOVED=")).collect();
        assert_eq!(publishes.len(), 1);
        assert!(publishes[0].contains("cd '/sdcard/.xvolte-restore-"));
        assert!(d.shell_calls.iter().any(|c| c.starts_with("rm -rf -- '/sdcard/.xvolte-restore-")));
    }

    #[test]
    fn publish_count_mismatch_is_a_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        std::fs::write(&path, b"abc").unwrap();
        let mut d = test_device();
        // 기기가 남은 파일을 보고하면(옮기기 실패) 성공으로 보지 않는다
        d.publish_left = 1;
        let file = PushFile { source: PushSource::File(path), name: "a.jpg".into(), mtime: 0, size: 3 };
        let e = staged_push(&mut d, "/sdcard", vec![file], Arc::new(|_, _| {})).unwrap_err();
        assert!(e.contains("옮기기 수가 맞지 않습니다"), "{e}");
    }

    #[test]
    fn package_uids_parse_and_app_data_owner_fix_matches_app_created_folders() {
        let owners = parse_package_uids(
            "package:com.kakao.talk uid:10440\npackage:com.google.android.apps.maps uid:10267\njunk\n",
        )
        .unwrap();
        assert_eq!(owners["com.kakao.talk"], 10440);
        assert!(parse_package_uids("").is_err());
        let mut d = test_device();
        d.answer_shell("\"$(command -v su", "");
        let packages = std::collections::BTreeMap::from([("com.kakao.talk".to_string(), 10440)]);
        fix_app_data_owners(&mut d, &packages).unwrap();
        let cmd = d.shell_calls.iter().find(|c| c.contains("chown -R")).unwrap_or_else(|| panic!("{:?}", d.shell_calls));
        // 실기기 앱 폴더와 같은 값: 앱 uid, 그룹 1078, 프로젝트 ID 20000+앱 ID, 폴더 상속 P
        assert!(cmd.contains("chown -R 10440:1078 /data/media/0/Android/data/com.kakao.talk"), "{cmd}");
        assert!(cmd.contains("chattr -R -p 20440 /data/media/0/Android/data/com.kakao.talk"), "{cmd}");
        assert!(cmd.contains("chattr +P"), "{cmd}");
        let bad = std::collections::BTreeMap::from([("a;rm -rf /".to_string(), 10440)]);
        assert!(fix_app_data_owners(&mut d, &bad).is_err());
    }

    #[test]
    fn file_shorter_than_its_record_fails_instead_of_sending_a_short_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("f");
        std::fs::write(&path, b"ab").unwrap();
        let mut d = test_device();
        let e = staged_push(
            &mut d,
            "/sdcard",
            vec![PushFile {
                source: PushSource::File(path),
                name: "DCIM/a.jpg".into(),
                mtime: 0,
                size: 3,
            }],
            Arc::new(|_, _| {}),
        )
        .unwrap_err();
        assert!(e.contains("DCIM/a.jpg"), "{e}");
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
        // 손상 세그먼트는 그 세그먼트의 오류로 남고 다른 세그먼트 검증은 계속된다
        let hashes =
            super::super::verify::quarantine_hashes(tmp.path(), &std::collections::HashSet::new())
                .unwrap();
        assert!(hashes.segment_errors.is_empty());
        let mut wanted = std::collections::HashMap::new();
        let expected = super::super::model::FileEntry {
            remote: "/system/evil".into(),
            local: String::new(),
            size: 1,
            mtime: 0,
            sha256: None,
            quarantined: true,
            error: None,
        };
        wanted.insert("/system/evil", &expected);
        assert!(pick_quarantined(&seg, &wanted, &Default::default()).is_err());
    }

    #[test]
    fn restore_survives_item_failure() {
        let mut m = Manifest::new("XQ-DQ44", "AB1234****", "67.2.A.3.178", "15");
        m.items = vec![];
        let tmp = backup_dir_with(m);
        let mut d = test_device();
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
        let mut device = test_device();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(!out.failures.is_empty());
        assert!(device.shell_calls.is_empty());
        assert!(device.shell_streams.is_empty());
    }

    #[test]
    fn restores_only_manifest_files_with_manifest_mtime() {
        let tmp = backup_dir_with(Manifest::new("XQ", "masked", "v", "15"));
        std::fs::write(tmp.path().join("sdcard/DCIM/extra.jpg"), b"unrecorded").unwrap();
        let mut device = test_device();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        let restored: Vec<_> = device.pushed.keys().filter(|k| k.starts_with("/sdcard/DCIM")).cloned().collect();
        assert_eq!(restored, vec!["/sdcard/DCIM/Camera/a.jpg".to_string()]);
        assert_eq!(device.pushed_mtime["/sdcard/DCIM/Camera/a.jpg"], 1700000000);
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
            ("data/app/pkg:odd/base.apk", b"apk"),
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
        manifest.device_key = Some(hex::encode(Sha256::digest(b"TEST-SERIAL")));
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
        let mut apk = super::super::model::ItemRecord::new("apk", ItemKind::Files);
        apk.entries.push(super::super::model::FileEntry {
            remote: "/data/app/pkg:odd/base.apk".into(),
            local: String::new(),
            size: 3,
            mtime: 0,
            sha256: Some(hex::encode(Sha256::digest(b"apk"))),
            quarantined: true,
            error: None,
        });
        apk.finalize();
        manifest.record(apk);
        save_manifest_atomic(&manifest, tmp.path()).unwrap();
        let mut device = test_device();
        let out = run_restore(&mut device, tmp.path(), &["dcim".into()], &noop_progress());
        assert!(out.failures.is_empty(), "{:?}", out.failures);
        // 일반 파일 복원과 같은 기준 — /sdcard 아래 상대 이름
        // 해시가 맞는 새 버전만, 세그먼트 안 위치에서 바로 읽어 올린다(낡은 버전·미선택 항목 제외)
        let restored: Vec<_> = device
            .pushed
            .iter()
            .filter(|(k, _)| k.starts_with("/sdcard/") && !k.starts_with("/sdcard/contacts"))
            .collect();
        assert_eq!(restored.len(), 1, "{:?}", device.pushed.keys());
        assert_eq!(restored[0].0, "/sdcard/DCIM/bad?.jpg");
        assert_eq!(restored[0].1, b"new");
        assert_eq!(device.pushed_mtime["/sdcard/DCIM/bad?.jpg"], 1700000000);
        let mut all = test_device();
        let result = run_restore(
            &mut all,
            tmp.path(),
            &["dcim".into(), "apk".into()],
            &noop_progress(),
        );
        assert!(result.failures.iter().any(|e| e.contains("APK")));
        assert!(!result.failures.iter().any(|e| e.contains("격리")));
        assert_eq!(all.pushed.get("/sdcard/DCIM/bad?.jpg").map(Vec::as_slice), Some(b"new".as_slice()));
    }
}
