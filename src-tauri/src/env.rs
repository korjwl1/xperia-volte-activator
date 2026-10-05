//! PC 환경 점검(읽기 전용) — 관리자 권한 없이 확인할 수 있는 항목만.
//! 드라이버는 설치하지 않는다(사용자 결정 2026-10-04: 감지 + 안내만).

use serde::Serialize;

/// 프론트 `EnvCheckItem`과 같은 모양
#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvCheckItem {
    pub id: String,
    pub label: String,
    /// "pass" | "warn" | "fail" | "info"
    pub state: String,
    pub detail: String,
    pub fixable: bool,
}

/// VoLTE 적용(EFS)은 폰의 DIAG 포트를 COM 포트로 연다 — 이 포트를 만드는 Qualcomm 시리얼 드라이버 패키지
const DIAG_DRIVER_INF: &str = "qcser.inf";

/// 드라이버 저장소 패키지 이름 목록 → DIAG 드라이버 점검 항목.
/// 패키지 키 이름은 `<inf 이름>_<아키텍처>_<해시>` 형식이다(예: qcser.inf_amd64_d9159d204b993759).
fn diag_driver_item(packages: Option<&[String]>) -> EnvCheckItem {
    let prefix = format!("{DIAG_DRIVER_INF}_");
    let (state, detail) = match packages {
        None => (
            "info",
            "드라이버 설치 여부를 확인하지 못했습니다".to_string(),
        ),
        Some(list)
            if list
                .iter()
                .any(|name| name.to_ascii_lowercase().starts_with(&prefix)) =>
        {
            // 저장소에 패키지가 있다는 뜻 — 실제 포트 연결은 VoLTE 적용 때 따로 확인해야 한다
            ("pass", "Qualcomm 시리얼(DIAG) 드라이버 패키지가 설치되어 있습니다".to_string())
        }
        Some(_) => (
            "warn",
            "Qualcomm 시리얼(DIAG) 드라이버가 없습니다 — VoLTE 적용 전에 Qualcomm USB 드라이버를 설치한 뒤 [다시 확인]을 눌러 주세요".to_string(),
        ),
    };
    EnvCheckItem {
        id: "diag-driver".into(),
        label: "VoLTE 적용용 드라이버".into(),
        state: state.into(),
        detail,
        fixable: false,
    }
}

/// Windows 드라이버 저장소에 등록된 패키지 이름(레지스트리 읽기 전용) — 확인할 수 없으면 None
#[cfg(windows)]
fn driver_packages() -> Option<Vec<String>> {
    use windows_sys::Win32::Foundation::{ERROR_MORE_DATA, ERROR_NO_MORE_ITEMS, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumKeyExW, RegOpenKeyExW, HKEY, HKEY_LOCAL_MACHINE, KEY_READ,
    };

    struct Key(HKEY);
    impl Drop for Key {
        fn drop(&mut self) {
            // SAFETY: RegOpenKeyExW가 성공해 얻은 핸들을 한 번만 닫는다
            unsafe { RegCloseKey(self.0) };
        }
    }

    let path: Vec<u16> = "SYSTEM\\DriverDatabase\\DriverPackages\0"
        .encode_utf16()
        .collect();
    let mut raw: HKEY = std::ptr::null_mut();
    // SAFETY: path는 NUL로 끝나는 UTF-16, raw는 유효한 출력 포인터
    let rc = unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, path.as_ptr(), 0, KEY_READ, &mut raw) };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let key = Key(raw);
    let mut names = Vec::new();
    let mut buf = vec![0u16; 256]; // 레지스트리 키 이름 최대 255자
    for index in 0u32.. {
        let mut len = buf.len() as u32;
        // SAFETY: buf/len은 이 호출 동안 유효하고, 나머지 선택 인자는 null
        let rc = unsafe {
            RegEnumKeyExW(
                key.0,
                index,
                buf.as_mut_ptr(),
                &mut len,
                std::ptr::null(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        };
        match rc {
            ERROR_SUCCESS => names.push(String::from_utf16_lossy(&buf[..len as usize])),
            ERROR_NO_MORE_ITEMS => break,
            // 이름이 버퍼보다 긴 키는 드라이버 패키지 형식이 아니다 — 건너뛴다
            ERROR_MORE_DATA => continue,
            _ => return None,
        }
    }
    Some(names)
}

#[cfg(not(windows))]
fn driver_packages() -> Option<Vec<String>> {
    None
}

/// PC 환경 점검 — 지금은 VoLTE 적용용 DIAG 드라이버만(읽기 전용)
#[tauri::command]
pub async fn env_check() -> Result<Vec<EnvCheckItem>, String> {
    crate::tasks::blocking("환경 점검", || {
        let packages = driver_packages();
        Ok(vec![diag_driver_item(packages.as_deref())])
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diag_driver_is_detected_by_package_name_only() {
        let installed = vec![
            "android_winusb.inf_amd64_b7959ee935b85619".to_string(),
            "QCSER.INF_amd64_d9159d204b993759".to_string(),
        ];
        assert_eq!(diag_driver_item(Some(&installed)).state, "pass");
        // 이름 일부만 같은 다른 패키지는 인정하지 않는다
        let others = vec![
            "qcserial.inf_amd64_0000".to_string(),
            "qcmdm.inf_amd64_5a5ae5f1be6358ce".to_string(),
        ];
        let missing = diag_driver_item(Some(&others));
        assert_eq!(missing.state, "warn");
        assert!(missing.detail.contains("다시 확인"));
        // 확인 불가는 없다고 단정하지 않는다
        assert_eq!(diag_driver_item(None).state, "info");
    }

    /// 이 PC의 드라이버 저장소를 실제로 읽는다(기기·환경 의존 — 수동 실행: cargo test -- --ignored live_driver_store)
    #[cfg(windows)]
    #[test]
    #[ignore]
    fn live_driver_store_readable_without_admin() {
        // 일반 권한으로도 목록을 읽을 수 있어야 한다(설치 여부와 무관)
        assert!(driver_packages().is_some_and(|list| !list.is_empty()));
    }
}

#[tauri::command]
pub fn engine_capabilities() -> serde_json::Value {
    serde_json::json!({"fastbootWrite": cfg!(feature = "fastboot-write"), "rootWrite": cfg!(feature = "root-write"), "efsWrite": cfg!(feature = "efs-write")})
}
