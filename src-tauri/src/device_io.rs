//! 기기 I/O 결과를 공통 규칙으로 해석한다. Tauri와 연결 선택에 의존하지 않는다.
use adb_client::ADBDeviceExt;
use std::sync::atomic::{AtomicBool, Ordering};

static WRITING: AtomicBool = AtomicBool::new(false);
/// ADB 서버 경로도 엔진 간 쓰기 충돌을 막는다. I/O 종료 전에는 실행권을 반환하지 않는다.
pub struct WriteOperation;
impl WriteOperation {
    pub fn acquire() -> Result<Self, String> {
        WRITING
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .map_err(|_| "다른 기기 변경 작업이 진행 중입니다")?;
        Ok(Self)
    }
}
impl Drop for WriteOperation {
    fn drop(&mut self) {
        WRITING.store(false, Ordering::SeqCst);
    }
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

pub fn shell(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<String, String> {
    shell_result(dev, command, false)
}

/// 기기 변경 명령은 종료 코드를 확인하지 못한 경우에도 실패한다.
pub fn shell_write(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<String, String> {
    shell_result(dev, command, true)
}

fn shell_result(dev: &mut dyn ADBDeviceExt, command: &str, strict: bool) -> Result<String, String> {
    let mut stdout = vec![];
    let mut stderr = vec![];
    let status = dev
        .shell_command(&command, Some(&mut stdout), Some(&mut stderr))
        .map_err(|e| format!("기기 명령 실패: {e}"))?;
    if status.is_some_and(|code| code != 0) || (status.is_none() && (strict || !stderr.is_empty()))
    {
        return Err(format!(
            "기기 명령 종료 코드 {:?}: {}",
            status,
            String::from_utf8_lossy(&stderr).trim()
        ));
    }
    String::from_utf8(stdout).map_err(|e| format!("출력 해석 실패: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
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
        device.shell_unknown_status.insert("valid".into());
        assert_eq!(shell(&mut device, "valid").unwrap(), "value");
        assert!(shell_write(&mut device, "valid").is_err());
    }
}
