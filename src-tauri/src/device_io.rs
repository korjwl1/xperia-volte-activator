//! 기기 I/O 결과를 공통 규칙으로 해석한다. Tauri와 연결 선택에 의존하지 않는다.
use adb_client::ADBDeviceExt;
use std::sync::atomic::{AtomicBool, Ordering};

static WRITING: AtomicBool = AtomicBool::new(false);
/// ADB 서버 경로도 엔진 간 쓰기 충돌을 막는다. I/O 종료 전에는 실행권을 반환하지 않는다.
pub struct WriteOperation {
    // GUI and developer CLI are separate processes. The OS releases this lock on exit/crash.
    _process_lock: std::fs::File,
}
impl WriteOperation {
    pub fn acquire() -> Result<Self, String> {
        WRITING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "다른 기기 변경 작업이 진행 중입니다")?;
        match process_lock(&std::env::temp_dir().join("xperia-volte-device-io.lock")) {
            Ok(file) => Ok(Self {
                _process_lock: file,
            }),
            Err(error) => {
                WRITING.store(false, Ordering::SeqCst);
                Err(error)
            }
        }
    }
}
impl Drop for WriteOperation {
    fn drop(&mut self) {
        WRITING.store(false, Ordering::SeqCst);
    }
}

fn process_lock(path: &std::path::Path) -> Result<std::fs::File, String> {
    use fs2::FileExt;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path)
        .map_err(|e| format!("기기 작업 잠금 파일 열기 실패: {e}"))?;
    file.try_lock_exclusive()
        .map_err(|_| "다른 앱 또는 개발 CLI가 기기 작업을 실행 중입니다")?;
    Ok(file)
}

/// 원본 시리얼을 파일 이름이나 기록에 남기지 않고 기기를 구분한다.
pub fn identity_key(dev: &mut dyn ADBDeviceExt) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let serial = shell(dev, "getprop ro.serialno")?;
    let serial = serial.trim();
    if serial.is_empty() {
        return Err("기기 식별 정보를 확인할 수 없습니다".into());
    }
    Ok(hex::encode(Sha256::digest(serial.as_bytes())))
}

/// 종료 코드 표식. USB 직접 연결(adb_client "shell:" 서비스)은 종료 코드를 돌려주지 않고
/// 서버 shell v2도 스트림이 끊기면 None을 돌려주므로, 셸에서 직접 덧붙여 확인한다.
pub const RC_MARK: &str = "__XV_RC=";

/// 셸 실행 결과 — stdout(표식 제거), stderr(USB 직접 연결에서는 stdout에 섞여 비어 있음), 종료 코드.
pub struct ShellOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: u8,
}

/// 명령 뒤에 종료 코드 표식을 붙여 실행한다. 표식이 없으면 출력이 잘린 것으로 보고 실패한다.
/// 개행으로 잇기 때문에 명령이 `;`나 `&`로 끝나도 문법이 깨지지 않는다.
pub fn shell_run(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<ShellOutput, String> {
    let mut stdout = vec![];
    let mut stderr = vec![];
    // 전송 계층의 상태는 마지막 echo의 것이므로 쓰지 않는다.
    dev.shell_command(
        &format!("{command}\necho {RC_MARK}$?"),
        Some(&mut stdout),
        Some(&mut stderr),
    )
    .map_err(|e| format!("기기 명령 실패: {e}"))?;
    let (body_len, code) = split_rc(&stdout).ok_or_else(|| {
        format!(
            "기기 명령 종료 상태를 확인할 수 없습니다(출력 끊김): {}",
            String::from_utf8_lossy(&stderr).trim()
        )
    })?;
    stdout.truncate(body_len);
    Ok(ShellOutput {
        stdout,
        stderr,
        code,
    })
}

/// 출력 끝의 `__XV_RC=<n>` 표식 → (본문 길이, 종료 코드)
fn split_rc(out: &[u8]) -> Option<(usize, u8)> {
    let mark = RC_MARK.as_bytes();
    let at = out.windows(mark.len()).rposition(|w| w == mark)?;
    let code = std::str::from_utf8(&out[at + mark.len()..])
        .ok()?
        .trim()
        .parse::<u8>()
        .ok()?;
    Some((at, code))
}

/// 종료 코드 0만 성공. 읽기·쓰기 모두 같은 규칙(종료 상태를 모르면 실패).
pub fn shell(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<String, String> {
    let out = shell_run(dev, command)?;
    if out.code != 0 {
        let detail = if out.stderr.is_empty() {
            String::from_utf8_lossy(&out.stdout).trim().to_string()
        } else {
            String::from_utf8_lossy(&out.stderr).trim().to_string()
        };
        return Err(format!("기기 명령 종료 코드 {}: {detail}", out.code));
    }
    String::from_utf8(out.stdout).map_err(|e| format!("출력 해석 실패: {e}"))
}

/// 기기 변경 명령 — 판정 규칙은 `shell`과 같다. 호출부에서 쓰기 의도를 드러내기 위한 이름.
pub fn shell_write(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<String, String> {
    shell(dev, command)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    #[test]
    fn separate_file_handles_cannot_own_the_device_lock_together() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("device.lock");
        let first = process_lock(&path).unwrap();
        assert!(process_lock(&path).is_err());
        drop(first);
        assert!(process_lock(&path).is_ok());
    }
    #[test]
    fn write_permission_is_exclusive_until_the_operation_finishes() {
        let first = WriteOperation::acquire().unwrap();
        assert!(WriteOperation::acquire().is_err());
        drop(first);
        assert!(WriteOperation::acquire().is_ok());
    }
    #[test]
    fn nonzero_status_cannot_become_empty_success() {
        let mut device = FakeADBDevice::new();
        assert!(shell(&mut device, "unhandled command").is_err());
        device.answer_shell("valid", "value");
        assert_eq!(shell(&mut device, "valid").unwrap(), "value");
        // USB 직접 연결처럼 전송 계층이 상태를 주지 않아도 표식으로 판정한다
        device.shell_unknown_status.insert("valid".into());
        assert_eq!(shell(&mut device, "valid").unwrap(), "value");
        assert_eq!(shell_write(&mut device, "valid").unwrap(), "value");
        device.shell_exit_codes.insert("valid".into(), 3);
        assert!(shell(&mut device, "valid").is_err());
        // 표식 없이 끝난 출력(끊김)은 읽기에서도 성공이 아니다
        device.shell_exit_codes.clear();
        device.shell_truncated.insert("valid".into());
        assert!(shell(&mut device, "valid").is_err());
    }

    #[test]
    fn rc_marker_is_split_even_without_trailing_newline() {
        assert_eq!(split_rc(b"value__XV_RC=0\n"), Some((5, 0)));
        assert_eq!(split_rc(b"a\n__XV_RC=127\r\n"), Some((2, 127)));
        assert_eq!(split_rc(b"no marker"), None);
        assert_eq!(split_rc(b"__XV_RC=x"), None);
    }
}
