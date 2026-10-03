//! 기기 I/O 결과를 공통 규칙으로 해석한다. Tauri와 연결 선택에 의존하지 않는다.
use adb_client::ADBDeviceExt;

pub fn shell(dev: &mut dyn ADBDeviceExt, command: &str) -> Result<String, String> {
    let mut stdout = vec![];
    let mut stderr = vec![];
    let status = dev
        .shell_command(&command, Some(&mut stdout), Some(&mut stderr))
        .map_err(|e| format!("기기 명령 실패: {e}"))?;
    if status.is_some_and(|code| code != 0) || (status.is_none() && !stderr.is_empty()) {
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
    fn nonzero_status_cannot_become_empty_success() {
        let mut device = FakeADBDevice::new();
        assert!(shell(&mut device, "unhandled command").is_err());
        device.answer_shell("valid", "value");
        assert_eq!(shell(&mut device, "valid").unwrap(), "value");
    }
}
