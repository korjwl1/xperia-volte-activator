//! EfsTools 서브프로세스 실행기 — argv 배열 실행(§12.5), 출력 실시간 스트림, 에러 패턴 분류.
//! EfsTools의 종료 코드는 항상 0이라(Program.cs Main이 void + 최상위 catch 무시) 성공 판정에
//! 종료 코드를 쓸 수 없다 — 에러 라인 분류와 전수 리드백(verify.rs)이 판정을 담당한다.

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// 진행 중 EfsTools 프로세스 취소 플래그 — efs_cancel 명령이 설정
pub static EFS_CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Default)]
pub struct RunOutcome {
    pub lines: Vec<String>,
    pub errors: Vec<String>,
    pub killed: bool,
}

/// 에러 라인 판정 — 실측·이슈 #7 기준 문양(영어 리소스 폴백). 정상 라인은 오류 아님.
pub fn is_error_line(line: &str) -> bool {
    let t = line.trim();
    t.starts_with("Critical error")
        || t.starts_with("Error on ") && t.contains(" file")
        || t.contains("EFS error. Code =")
        || t.contains("Unhandled exception")
}

/// 출력 라인 전체에 대해 에러만 추출
pub fn extract_errors(lines: &[String]) -> Vec<String> {
    lines.iter().filter(|l| is_error_line(l)).cloned().collect()
}

/// 자식 프로세스가 취소 플래그를 확인하며 종료를 기다린다 — 설정 시 kill
fn wait_with_cancel(mut child: Child) -> (std::process::ExitStatus, bool) {
    loop {
        if EFS_CANCEL.load(Ordering::Relaxed) {
            let _ = child.kill();
            return match child.wait() {
                Ok(s) => (s, true),
                Err(_) => (std::process::ExitStatus::default(), true),
            };
        }
        match child.try_wait() {
            Ok(Some(status)) => return (status, false),
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(100)),
            Err(_) => return (std::process::ExitStatus::default(), false),
        }
    }
}

/// EfsTools.exe 경로 탐색 — 환경변수 → 앱 실행 파일 옆 → 개발 리포지토리 vendor
pub fn resolve_tool_dir() -> Result<PathBuf, String> {
    if let Ok(dir) = std::env::var("XVOLTE_EFSTOOLS_DIR") {
        let p = PathBuf::from(dir);
        if p.join("EfsTools.exe").is_file() {
            return Ok(p);
        }
    }
    // 앱 실행 파일 옆 (설치 레이아웃)
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let p = dir.join("vendor").join("efstools");
            if p.join("EfsTools.exe").is_file() {
                return Ok(p);
            }
        }
    }
    // 개발 리포지토리 (src-tauri/../vendor/efstools)
    let dev = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("vendor").join("efstools");
    if dev.join("EfsTools.exe").is_file() {
        return Ok(dev);
    }
    Err("EfsTools를 찾을 수 없습니다 — scripts/build-efstools.ps1로 vendor/efstools를 먼저 빌드해 주세요".into())
}

/// argv 실행 — 셸 보간 없음. 모든 출력 라인을 on_line으로 흘리고 에러를 모은다.
/// 종료 코드는 판정에 쓰지 않는다(항상 0) — 에러 라인이 유일한 직접 신호.
pub fn run_args(
    dir: &Path,
    args: &[&str],
    on_line: &mut dyn FnMut(&str),
) -> Result<RunOutcome, String> {
    let exe = dir.join("EfsTools.exe");
    if !exe.is_file() {
        return Err("EfsTools.exe가 없습니다 — vendor 빌드를 먼저 실행해 주세요".into());
    }
    let mut child = Command::new(&exe)
        .args(args)
        .current_dir(dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null())
        .spawn()
        .map_err(|e| format!("EfsTools 실행 실패: {e}"))?;

    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");

    // stderr는 별도 스레드에서 드레인(파이프 막힘 방지) — Critical error가 여기로 올 수 있다
    let err_lines: Arc<std::sync::Mutex<Vec<String>>> = Arc::new(std::sync::Mutex::new(Vec::new()));
    let err_sink = Arc::clone(&err_lines);
    let stderr_thread = std::thread::spawn(move || {
        let reader = BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            err_sink.lock().map(|mut v| v.push(line)).ok();
        }
    });

    let mut outcome = RunOutcome::default();
    {
        let reader = BufReader::new(stdout);
        for line in reader.lines().map_while(Result::ok) {
            on_line(&line);
            outcome.lines.push(line.clone());
        }
    }
    let (status, killed) = wait_with_cancel(child);
    outcome.killed = killed;
    let _ = status; // 판정에 사용하지 않음 — 문서화된 제약
    if let Ok(extra) = err_lines.lock() {
        for line in extra.iter() {
            on_line(line);
            outcome.lines.push(line.clone());
        }
    }
    let _ = stderr_thread.join();
    outcome.errors = extract_errors(&outcome.lines);
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_real_error_patterns() {
        // 실측(2026-10-04, 기기 미연결 efsInfo): Critical error. Port not found: COM7
        assert!(is_error_line("Critical error. Port not found: COM7"));
        // 이슈 #7: 파일별 업로드 실패
        assert!(is_error_line("Error on upload file '/data/andsf.xml'. EFS error. Code = 306"));
        assert!(is_error_line("Error on download file '/ims/imshandoverconfig'. EFS error. Code = 306"));
        assert!(is_error_line("Some prefix Error on delete file '/x'. EFS error. Code = 12"));
        assert!(is_error_line("EFS error. Code = 200"));
        assert!(is_error_line("Unhandled exception. Foo"));
    }

    #[test]
    fn normal_lines_are_not_errors() {
        assert!(!is_error_line("EfsTools 0.14.0.14"));
        assert!(!is_error_line("Upload file '/policyman/carrier_policy.xml'"));
        assert!(!is_error_line("Version: 4, MaxDirectories: 64"));
        assert!(!is_error_line(""));
        // 에러 코드 이야기가 아니라면 "error" 단어만으로 오판하지 않는다
        assert!(!is_error_line("no error detected"));
    }

    #[test]
    fn extract_errors_collects_only_failures() {
        let lines = vec![
            "EfsTools 0.14.0.14".into(),
            "Critical error. Port not found: COM7".into(),
            "Uploading...".into(),
            "Error on upload file '/a'. EFS error. Code = 306".into(),
        ];
        let errors = extract_errors(&lines);
        assert_eq!(errors.len(), 2);
    }

    /// vendor에 실제 바이너리가 있을 때만 (cargo test -- --ignored efs_live_version)
    #[test]
    #[ignore = "vendor/efstools 바이너리 필요"]
    fn efs_live_version() {
        let dir = resolve_tool_dir().unwrap();
        let mut got = String::new();
        let out = run_args(&dir, &["version"], &mut |l| got.push_str(l)).unwrap();
        assert!(got.contains("0.14"), "{got}");
        assert!(out.errors.is_empty());
    }
}
