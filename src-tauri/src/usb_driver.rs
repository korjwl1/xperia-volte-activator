//! Windows fastboot USB 드라이버 — 부트로더 모드(USB\VID_0FCE&PID_0DDE)와 fastbootd 모드(USB\VID_18D1&PID_4EE0)의
//! Sony 폰은 Windows가 자동으로 붙일 드라이버가 없어(문제 코드 28) libusb가 장치를 보지도 못한다. 원본 도구는 사람이 장치 관리자에서
//! Sony 드라이버(sa0200adb.inf)를 "디스크 있음"으로 지정하게 했다. 이 모듈은 같은 일을 코드로 한다.
//!
//! 1. 폰이 보고하는 판매명(ro.semc.product.name, 예 "Xperia 1 V")으로 Sony 공식 드라이버 목록에서
//!    "<판매명> driver" 항목을 찾는다(내장 모델 표 없음). 없으면 같은 규칙으로 만든 주소를 시도한다.
//! 2. 공식 API가 주는 서명된 임시 주소로 zip을 받아 Sony WinUSB INF(VID_0FCE)를 확인한다.
//!    드라이버 서명(Microsoft WHCP 카탈로그)은 Windows가 설치할 때 검증한다.
//! 3. 관리자 권한(UAC 한 번)으로 이 실행 파일을 다시 띄워, 드라이버를 저장소에 넣고
//!    장치 관리자의 "직접 선택"과 같은 SetupAPI 호출로 그 장치에 지정한다.
use std::path::{Path, PathBuf};
use std::time::Duration;

/// fastboot 모드 폰의 하드웨어 ID와 (VID, PID) — 부트로더(언락·리락), fastbootd(부트 이미지 기록, AOSP 표준 ID)
pub const FASTBOOT_HWIDS: &[(&str, u16, u16)] = &[
    ("USB\\VID_0FCE&PID_0DDE", 0x0FCE, 0x0DDE),
    ("USB\\VID_18D1&PID_4EE0", 0x18D1, 0x4EE0),
];

/// 하드웨어 ID·인스턴스 ID가 fastboot 장치면 그 (VID, PID)
fn fastboot_hwid(id: &str) -> Option<(u16, u16)> {
    let id = id.to_ascii_uppercase();
    FASTBOOT_HWIDS
        .iter()
        .find(|(hwid, _, _)| {
            id == *hwid || id.starts_with(&format!("{hwid}&")) || id.starts_with(&format!("{hwid}\\"))
        })
        .map(|&(_, vid, pid)| (vid, pid))
}
const DRIVER_LIST_URL: &str = "https://opendevices.sony.net/aosp-on-xperia-open-devices/downloads/drivers";
const DOWNLOAD_LINK_API: &str = "https://opendevices.sony.net/api/file/download-link/file/download/";
const MAX_ZIP: u64 = 64 * 1024 * 1024;
/// 관리자 권한으로 다시 띄운 이 실행 파일이 받는 인자 — <inf> <장치 인스턴스 ID> <결과 파일>
pub const ELEVATED_ARG: &str = "--xva-bind-usb-driver";

fn normalize(text: &str) -> String {
    text.chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// 판매명으로 만드는 Sony 드라이버 주소 규칙 — "Xperia 1 V" → "xperia-1-v-driver"
pub fn rule_slug(product_name: &str) -> Option<String> {
    let words: Vec<String> = product_name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.to_ascii_lowercase())
        .collect();
    (words.first().map(String::as_str) == Some("xperia")).then(|| format!("{}-driver", words.join("-")))
}

/// 공식 목록 페이지에서 제목이 "<판매명> driver"와 같은 항목의 주소(slug)를 찾는다.
/// 페이지에는 `"title":"Xperia 1 V driver","permalink":"/file/download/xperia-1-v-driver"` 형태가 들어 있다.
pub fn slug_from_listing(html: &str, product_name: &str) -> Option<String> {
    let wanted = normalize(&format!("{product_name} driver"));
    if wanted == "driver" {
        return None;
    }
    let marker = "\"permalink\":\"/file/download/";
    let mut from = 0;
    while let Some(found) = html[from..].find(marker) {
        let start = from + found + marker.len();
        from = start;
        let Some(end) = html[start..].find('"') else {
            break;
        };
        let slug = &html[start..start + end];
        if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
            continue;
        }
        // 같은 객체 안의 바로 앞 제목(최대 400자 이내)
        let window = &html[start.saturating_sub(400)..start];
        let Some(title_at) = window.rfind("\"title\":\"") else {
            continue;
        };
        let title = &window[title_at + 9..];
        let title = title.split('"').next().unwrap_or_default();
        if normalize(title) == wanted {
            return Some(slug.to_string());
        }
    }
    None
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout_read(Duration::from_secs(60))
        .user_agent("xperia-volte-activator")
        .build()
}

/// 공식 목록에서 찾고, 없으면 규칙 주소. 둘 다 없으면 오류(내장 모델 표는 쓰지 않는다)
pub fn resolve_slug(product_name: &str) -> Result<String, String> {
    let listing = agent()
        .get(DRIVER_LIST_URL)
        .call()
        .map_err(|e| format!("Sony 드라이버 목록 조회 실패: {e}"))?
        .into_string()
        .map_err(|e| format!("Sony 드라이버 목록 읽기 실패: {e}"))?;
    slug_from_listing(&listing, product_name)
        .or_else(|| rule_slug(product_name))
        .ok_or_else(|| format!("'{product_name}'에 맞는 Sony 드라이버를 찾지 못했습니다"))
}

/// 공식 드라이버 zip을 받아 dest/<slug>/에 풀고, Sony WinUSB INF 경로를 돌려준다.
pub fn download(slug: &str, dest: &Path) -> Result<PathBuf, String> {
    if slug.is_empty() || !slug.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-') {
        return Err("드라이버 주소 형식 오류".into());
    }
    let agent = agent();
    let body = agent
        .get(&format!("{DOWNLOAD_LINK_API}{slug}?captcha=null"))
        .call()
        .map_err(|e| format!("Sony 드라이버 다운로드 주소 요청 실패: {e}"))?
        .into_string()
        .map_err(|e| format!("Sony 드라이버 다운로드 주소 읽기 실패: {e}"))?;
    let api: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("Sony 드라이버 다운로드 주소 해석 실패: {e}"))?;
    let link = api["fileDownloadLink"]
        .as_str()
        .ok_or("Sony 드라이버 다운로드 주소를 받지 못했습니다")?;
    let host = link
        .strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?']).next())
        .unwrap_or_default();
    if !host.ends_with(".amazonaws.com") {
        return Err("예상하지 못한 드라이버 다운로드 주소입니다".into());
    }
    let response = agent
        .get(link)
        .call()
        .map_err(|e| format!("Sony 드라이버 다운로드 실패: {e}"))?;
    let mut bytes = Vec::new();
    std::io::Read::read_to_end(&mut std::io::Read::take(response.into_reader(), MAX_ZIP + 1), &mut bytes)
        .map_err(|e| format!("Sony 드라이버 다운로드 실패: {e}"))?;
    if bytes.len() as u64 > MAX_ZIP {
        return Err("드라이버 파일이 너무 큽니다".into());
    }
    let root = dest.join(slug);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).map_err(|e| format!("드라이버 폴더 생성 실패: {e}"))?;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))
        .map_err(|e| format!("드라이버 압축 해석 실패: {e}"))?;
    for index in 0..archive.len() {
        let mut file = archive.by_index(index).map_err(|e| e.to_string())?;
        let Some(name) = file.enclosed_name() else {
            continue; // 폴더 밖을 가리키는 이름은 풀지 않는다
        };
        let target = root.join(name);
        if file.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| e.to_string())?;
        std::io::copy(&mut file, &mut out).map_err(|e| e.to_string())?;
    }
    find_sony_winusb_inf(&root).ok_or_else(|| "받은 드라이버에서 Sony WinUSB INF를 찾지 못했습니다".into())
}

/// Sony(VID_0FCE) WinUSB INF인지 — 다른 종류의 드라이버를 장치에 지정하지 않는다
pub fn is_sony_winusb_inf(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("usb\\vid_0fce&pid_") && lower.contains("winusb")
}

fn find_sony_winusb_inf(root: &Path) -> Option<PathBuf> {
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).ok()?.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("inf"))
                && std::fs::read(&path)
                    .map(|b| is_sony_winusb_inf(&String::from_utf8_lossy(&b)))
                    .unwrap_or(false)
            {
                return Some(path);
            }
        }
    }
    None
}

/// 연결된 fastboot 장치의 상태 — 인스턴스 ID와 드라이버(서비스) 연결 여부
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FastbootPnp {
    pub instance_id: String,
    pub has_driver: bool,
    pub vendor_id: u16,
    pub product_id: u16,
}

#[cfg(windows)]
fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Windows 장치 목록에서 부트로더 모드 Sony 폰을 찾는다(libusb는 드라이버 없는 장치를 보지 못한다)
#[cfg(windows)]
pub fn find_fastboot_device() -> Option<FastbootPnp> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    unsafe {
        let set = SetupDiGetClassDevsW(
            std::ptr::null(),
            wide("USB").as_ptr(),
            std::ptr::null_mut(),
            DIGCF_ALLCLASSES | DIGCF_PRESENT,
        );
        if set == -1 {
            return None;
        }
        let mut found = None;
        let mut index = 0;
        loop {
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiEnumDeviceInfo(set, index, &mut dev) == 0 {
                break;
            }
            index += 1;
            let mut buffer = [0u8; 2048];
            let mut kind = 0u32;
            if SetupDiGetDeviceRegistryPropertyW(set, &dev, SPDRP_HARDWAREID, &mut kind, buffer.as_mut_ptr(), buffer.len() as u32, std::ptr::null_mut()) == 0 {
                continue;
            }
            let ids: Vec<u16> = buffer.chunks_exact(2).map(|c| u16::from_le_bytes([c[0], c[1]])).collect();
            let ids = String::from_utf16_lossy(&ids).to_ascii_uppercase();
            let Some((vendor_id, product_id)) = ids.split('\0').find_map(fastboot_hwid) else {
                continue;
            };
            let mut instance = [0u16; 512];
            if SetupDiGetDeviceInstanceIdW(set, &dev, instance.as_mut_ptr(), instance.len() as u32, std::ptr::null_mut()) == 0 {
                continue;
            }
            let end = instance.iter().position(|&c| c == 0).unwrap_or(instance.len());
            let mut service = [0u8; 512];
            let has_driver = SetupDiGetDeviceRegistryPropertyW(set, &dev, SPDRP_SERVICE, &mut kind, service.as_mut_ptr(), service.len() as u32, std::ptr::null_mut()) != 0;
            found = Some(FastbootPnp { instance_id: String::from_utf16_lossy(&instance[..end]), has_driver, vendor_id, product_id });
            break;
        }
        SetupDiDestroyDeviceInfoList(set);
        found
    }
}

#[cfg(not(windows))]
pub fn find_fastboot_device() -> Option<FastbootPnp> {
    None
}

/// 관리자 권한 실행 안에서 — 드라이버를 저장소에 넣고 그 장치에 직접 지정한다(장치 관리자의 "직접 선택"과 같다).
#[cfg(windows)]
fn bind_driver(inf: &Path, instance_id: &str) -> Result<(), String> {
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::*;
    use windows_sys::Win32::Foundation::GetLastError;
    let inf_text = std::fs::read(inf).map_err(|e| format!("INF 읽기 실패: {e}"))?;
    if !is_sony_winusb_inf(&String::from_utf8_lossy(&inf_text)) {
        return Err("Sony WinUSB INF가 아닙니다".into());
    }
    if fastboot_hwid(instance_id).is_none() {
        return Err("fastboot 모드 장치가 아닙니다".into());
    }
    let inf_w = wide(&inf.to_string_lossy());
    unsafe {
        // 1) 드라이버 저장소에 추가 — Windows가 카탈로그 서명을 검증한다
        let mut stored = [0u16; 260];
        if SetupCopyOEMInfW(inf_w.as_ptr(), std::ptr::null(), SPOST_PATH, 0, stored.as_mut_ptr(), stored.len() as u32, std::ptr::null_mut(), std::ptr::null_mut()) == 0 {
            return Err(format!("드라이버 저장소 추가 실패(오류 {})", GetLastError()));
        }
        let set = SetupDiCreateDeviceInfoList(std::ptr::null(), std::ptr::null_mut());
        if set == -1 {
            return Err(format!("장치 목록 생성 실패(오류 {})", GetLastError()));
        }
        let result = (|| {
            let mut dev: SP_DEVINFO_DATA = std::mem::zeroed();
            dev.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
            if SetupDiOpenDeviceInfoW(set, wide(instance_id).as_ptr(), std::ptr::null_mut(), 0, &mut dev) == 0 {
                return Err(format!("장치 열기 실패(오류 {})", GetLastError()));
            }
            // 2) 장치 종류를 INF의 종류로 맞춘다 — 그래야 그 종류의 드라이버 목록에 이 INF가 나온다
            let mut class_guid: windows_sys::core::GUID = std::mem::zeroed();
            let mut class_name = [0u16; 64];
            if SetupDiGetINFClassW(inf_w.as_ptr(), &mut class_guid, class_name.as_mut_ptr(), class_name.len() as u32, std::ptr::null_mut()) == 0 {
                return Err(format!("INF 종류 확인 실패(오류 {})", GetLastError()));
            }
            let guid_text = wide(&format!(
                "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
                class_guid.data1, class_guid.data2, class_guid.data3,
                class_guid.data4[0], class_guid.data4[1], class_guid.data4[2], class_guid.data4[3],
                class_guid.data4[4], class_guid.data4[5], class_guid.data4[6], class_guid.data4[7]
            ));
            if SetupDiSetDeviceRegistryPropertyW(set, &mut dev, SPDRP_CLASSGUID, guid_text.as_ptr() as *const u8, (guid_text.len() * 2) as u32) == 0 {
                return Err(format!("장치 종류 지정 실패(오류 {})", GetLastError()));
            }
            // 3) 이 INF 하나에서만, 하드웨어 ID가 목록에 없어도 고를 수 있게 드라이버 목록을 만든다
            let mut params: SP_DEVINSTALL_PARAMS_W = std::mem::zeroed();
            params.cbSize = std::mem::size_of::<SP_DEVINSTALL_PARAMS_W>() as u32;
            if SetupDiGetDeviceInstallParamsW(set, &dev, &mut params) == 0 {
                return Err(format!("설치 설정 읽기 실패(오류 {})", GetLastError()));
            }
            params.Flags |= DI_ENUMSINGLEINF;
            params.FlagsEx |= DI_FLAGSEX_ALLOWEXCLUDEDDRVS;
            let path = inf_w.iter().take(259).copied().collect::<Vec<_>>();
            params.DriverPath[..path.len()].copy_from_slice(&path);
            params.DriverPath[path.len()] = 0;
            if SetupDiSetDeviceInstallParamsW(set, &dev, &params) == 0 {
                return Err(format!("설치 설정 실패(오류 {})", GetLastError()));
            }
            if SetupDiBuildDriverInfoList(set, &mut dev, SPDIT_CLASSDRIVER) == 0 {
                return Err(format!("드라이버 목록 생성 실패(오류 {})", GetLastError()));
            }
            let mut driver: SP_DRVINFO_DATA_V2_W = std::mem::zeroed();
            driver.cbSize = std::mem::size_of::<SP_DRVINFO_DATA_V2_W>() as u32;
            if SetupDiEnumDriverInfoW(set, &dev, SPDIT_CLASSDRIVER, 0, &mut driver) == 0 {
                return Err(format!("INF에서 드라이버를 찾지 못했습니다(오류 {})", GetLastError()));
            }
            if SetupDiSetSelectedDriverW(set, &mut dev, &mut driver) == 0 {
                return Err(format!("드라이버 선택 실패(오류 {})", GetLastError()));
            }
            // 4) 설치 — Windows가 서명을 검증하고 장치에 붙인다
            let mut reboot = 0;
            if DiInstallDevice(std::ptr::null_mut(), set, &dev, &driver, 0, &mut reboot) == 0 {
                return Err(format!("드라이버 설치 실패(오류 {})", GetLastError()));
            }
            Ok(())
        })();
        SetupDiDestroyDeviceInfoList(set);
        result
    }
}

/// 관리자 권한으로 이 실행 파일을 다시 띄워 드라이버를 지정한다 — UAC 창이 한 번 뜬다
#[cfg(windows)]
pub fn bind_elevated(inf: &Path, instance_id: &str) -> Result<(), String> {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject};
    use windows_sys::Win32::UI::Shell::{ShellExecuteExW, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW};
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let result_file = std::env::temp_dir().join(format!("xva-usb-driver-{}.txt", std::process::id()));
    let _ = std::fs::remove_file(&result_file);
    let quote = |s: &str| format!("\"{}\"", s.replace('"', ""));
    let params = format!(
        "{ELEVATED_ARG} {} {} {}",
        quote(&inf.to_string_lossy()),
        quote(instance_id),
        quote(&result_file.to_string_lossy())
    );
    let (verb, file, params_w) = (wide("runas"), wide(&exe.to_string_lossy()), wide(&params));
    unsafe {
        let mut info: SHELLEXECUTEINFOW = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<SHELLEXECUTEINFOW>() as u32;
        info.fMask = SEE_MASK_NOCLOSEPROCESS;
        info.lpVerb = verb.as_ptr();
        info.lpFile = file.as_ptr();
        info.lpParameters = params_w.as_ptr();
        info.nShow = 0; // SW_HIDE
        if ShellExecuteExW(&mut info) == 0 || info.hProcess.is_null() {
            return Err("관리자 권한 요청이 취소되었거나 실패했습니다 — 드라이버를 설치하지 않았습니다".into());
        }
        WaitForSingleObject(info.hProcess, 180_000);
        let mut code = 1u32;
        GetExitCodeProcess(info.hProcess, &mut code);
        CloseHandle(info.hProcess);
        let message = std::fs::read_to_string(&result_file).unwrap_or_default();
        let _ = std::fs::remove_file(&result_file);
        if code == 0 {
            Ok(())
        } else if message.is_empty() {
            Err(format!("드라이버 설치가 끝나지 않았습니다(종료 코드 {code})"))
        } else {
            Err(message)
        }
    }
}

#[cfg(not(windows))]
pub fn bind_elevated(_inf: &Path, _instance_id: &str) -> Result<(), String> {
    Err("fastboot 드라이버 설치는 Windows 전용입니다".into())
}

/// 실행 파일 진입점에서 먼저 확인 — 관리자 권한 설치용으로 다시 띄워진 경우 처리하고 종료 코드를 돌려준다
pub fn elevated_entry(args: &[std::ffi::OsString]) -> Option<i32> {
    if args.get(1).map(|a| a.to_string_lossy()) != Some(ELEVATED_ARG.into()) {
        return None;
    }
    let get = |i: usize| args.get(i).map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
    let (inf, instance, result_file) = (get(2), get(3), get(4));
    #[cfg(windows)]
    let outcome = bind_driver(Path::new(&inf), &instance);
    #[cfg(not(windows))]
    let outcome: Result<(), String> = Err(format!("Windows 전용: {inf} {instance}"));
    let code = match &outcome {
        Ok(()) => 0,
        Err(_) => 1,
    };
    if !result_file.is_empty() {
        let _ = std::fs::write(&result_file, outcome.err().unwrap_or_default());
    }
    Some(code)
}

/// 부트로더 모드 폰에 드라이버가 없으면 공식 드라이버를 받아 지정한다. 이미 있으면 아무것도 하지 않는다.
pub fn ensure_fastboot_driver(product_name: &str, dest: &Path) -> Result<String, String> {
    let Some(device) = find_fastboot_device() else {
        return Err("fastboot 모드의 폰이 연결돼 있지 않습니다".into());
    };
    if device.has_driver {
        return Ok("fastboot 드라이버가 이미 연결돼 있습니다".into());
    }
    let slug = resolve_slug(product_name)?;
    let inf = download(&slug, dest)?;
    bind_elevated(&inf, &device.instance_id)?;
    match find_fastboot_device() {
        Some(after) if after.has_driver => Ok(format!("Sony 공식 드라이버({slug})를 fastboot 장치에 연결했습니다")),
        _ => Err("드라이버를 설치했지만 fastboot 장치에 연결되지 않았습니다 — 장치 관리자를 확인해 주세요".into()),
    }
}

/// fastboot 드라이버 확인·설치 — 부트로더 모드 폰에 드라이버가 없으면 Sony 공식 드라이버를 받아 지정한다(UAC 한 번)
#[tauri::command]
pub async fn fastboot_driver_ensure(product_name: String) -> Result<String, String> {
    let dest = crate::app_paths::data_dir()
        .map(|dir| dir.join("drivers"))
        .unwrap_or_else(|| std::env::temp_dir().join("xva-drivers"));
    crate::tasks::blocking("fastboot 드라이버", move || ensure_fastboot_driver(&product_name, &dest)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listing_title_is_matched_exactly_by_product_name() {
        let html = r#"{"title":"Xperia 1 VI driver","permalink":"/file/download/xperia-1-vi-driver"},{"title":"Xperia 1 V driver","permalink":"/file/download/xperia-1-v-driver"},{"title":"Xperia 10 V driver","permalink":"/file/download/xperia-10-v-driver"}"#;
        assert_eq!(slug_from_listing(html, "Xperia 1 V").as_deref(), Some("xperia-1-v-driver"));
        assert_eq!(slug_from_listing(html, "xperia 1 vi").as_deref(), Some("xperia-1-vi-driver"));
        assert_eq!(slug_from_listing(html, "Xperia 5 V"), None, "no prefix/partial match");
        assert_eq!(slug_from_listing(html, ""), None);
    }

    #[test]
    fn rule_slug_follows_sony_naming_and_rejects_non_xperia_names() {
        assert_eq!(rule_slug("Xperia 1 V").as_deref(), Some("xperia-1-v-driver"));
        assert_eq!(rule_slug("Xperia PRO-I").as_deref(), Some("xperia-pro-i-driver"));
        assert_eq!(rule_slug("XQ-DQ44"), None);
    }

    #[test]
    fn only_sony_winusb_infs_are_accepted() {
        assert!(is_sony_winusb_inf("%CompositeAdbInterface% = USB_Install, USB\\VID_0FCE&PID_320D\n; required for WinUsb"));
        assert!(!is_sony_winusb_inf("USB\\VID_18D1&PID_4EE0 winusb"));
        assert!(!is_sony_winusb_inf("USB\\VID_0FCE&PID_320D usbser.sys"));
    }

    #[test]
    fn bootloader_and_fastbootd_ids_are_recognized_only() {
        assert_eq!(fastboot_hwid("USB\\VID_0FCE&PID_0DDE&REV_0100"), Some((0x0FCE, 0x0DDE)));
        assert_eq!(fastboot_hwid("USB\\VID_18D1&PID_4EE0\\QV7709TEST"), Some((0x18D1, 0x4EE0)));
        assert_eq!(fastboot_hwid("usb\\vid_18d1&pid_4ee0"), Some((0x18D1, 0x4EE0)));
        assert_eq!(fastboot_hwid("USB\\VID_18D1&PID_4EE7"), None, "adb 등 다른 Google ID");
        assert_eq!(fastboot_hwid("USB\\VID_0FCE&PID_0DDEX"), None);
    }

    #[test]
    fn elevated_entry_ignores_normal_launches() {
        let args: Vec<std::ffi::OsString> = vec!["app.exe".into(), "--something".into()];
        assert_eq!(elevated_entry(&args), None);
    }
}
