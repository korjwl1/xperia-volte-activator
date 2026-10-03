//! 작업 중 PC 보호 (사용자 승인 2026-10-03) — 실행이 진행되는 동안에만 켜고, 끝나거나 멈추면 바로 푼다.
//! - 절전 방지: PowerCreateRequest + PowerRequestSystemRequired (화면은 꺼져도 PC는 잠들지 않음)
//! - Windows 종료·재시작·로그아웃 방지: ShutdownBlockReasonCreate + WM_QUERYENDSESSION에 FALSE 응답
//!   → Windows가 "이 앱이 종료를 막고 있습니다: <사유>"를 표시. 사용자가 "그래도 종료"를 고르면 막을 수 없다
//!   종료 요청이 오면 프론트에 알려 진행 기록을 즉시 저장한다 (이벤트 "run-guard": "query" | "end")
//! - 프로세스 강제 종료는 막지 않는다 (막는 수법은 악성코드 기법 — 진행 기록으로 대응)

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::{AppHandle, Emitter, Manager};

static ACTIVE: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

fn notify(kind: &str) {
    if let Some(app) = APP.get() {
        let _ = app.emit("run-guard", kind);
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::System::Power::{PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest};
    use windows_sys::Win32::System::Shutdown::{ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy, ShutdownBlockReasonQuery};
    use windows_sys::Win32::System::Threading::{
        POWER_REQUEST_CONTEXT_SIMPLE_STRING, REASON_CONTEXT, REASON_CONTEXT_0,
    };
    use windows_sys::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
    use windows_sys::Win32::UI::WindowsAndMessaging::{WM_ENDSESSION, WM_QUERYENDSESSION};

    /// SystemServices의 POWER_REQUEST_CONTEXT_VERSION (= 0)
    const POWER_REQUEST_CONTEXT_VERSION: u32 = 0;

    /// 절전 방지 요청 핸들 (HANDLE은 Send가 아니므로 usize로 보관)
    static POWER: Mutex<Option<usize>> = Mutex::new(None);

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(Some(0)).collect()
    }

    pub fn power(on: bool, reason: &str) -> Result<(), String> {
        let mut guard = POWER.lock().map_err(|_| "절전 방지 상태 잠금 실패")?;
        if let Some(h) = guard.take() {
            unsafe {
                PowerClearRequest(h as HANDLE, PowerRequestSystemRequired);
                CloseHandle(h as HANDLE);
            }
        }
        if !on {
            return Ok(());
        }
        let mut text = wide(reason);
        let ctx = REASON_CONTEXT {
            Version: POWER_REQUEST_CONTEXT_VERSION,
            Flags: POWER_REQUEST_CONTEXT_SIMPLE_STRING,
            Reason: REASON_CONTEXT_0 { SimpleReasonString: text.as_mut_ptr() },
        };
        let h = unsafe { PowerCreateRequest(&ctx) };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return Err("절전 방지 요청을 만들 수 없습니다".into());
        }
        if unsafe { PowerSetRequest(h, PowerRequestSystemRequired) } == 0 {
            unsafe { CloseHandle(h) };
            return Err("절전 방지 요청을 설정할 수 없습니다".into());
        }
        *guard = Some(h as usize);
        Ok(())
    }

    /// 창을 만든 스레드(메인)에서 호출해야 한다
    pub fn shutdown_block(hwnd: usize, on: bool, reason: &str) {
        let hwnd = hwnd as HWND;
        unsafe {
            if on {
                let text = wide(reason);
                ShutdownBlockReasonCreate(hwnd, text.as_ptr());
            } else {
                ShutdownBlockReasonDestroy(hwnd);
            }
            // 등록 상태 확인 로그 (사유가 있으면 길이 > 0)
            let mut len: u32 = 0;
            let set = ShutdownBlockReasonQuery(hwnd, std::ptr::null_mut(), &mut len) != 0 && len > 0;
            eprintln!("[rust] Windows 종료 방지 사유: {}", if set { "등록됨" } else { "없음" });
        }
    }

    unsafe extern "system" fn subclass(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM, _id: usize, _data: usize) -> LRESULT {
        if ACTIVE.load(Ordering::SeqCst) {
            if msg == WM_QUERYENDSESSION {
                notify("query");
                return 0; // FALSE — 종료 보류 (Windows가 사유를 표시)
            }
            if msg == WM_ENDSESSION && wp != 0 {
                notify("end"); // 사용자가 "그래도 종료"를 고른 경우 — 기록만 남긴다
            }
        }
        DefSubclassProc(hwnd, msg, wp, lp)
    }

    /// 메인 창에 종료 메시지 가로채기 설치 (메인 스레드, 앱 시작 시 1회)
    pub fn install(hwnd: usize) {
        unsafe {
            SetWindowSubclass(hwnd as HWND, Some(subclass), 0x5856, 0);
        }
    }
}

fn main_hwnd(app: &AppHandle) -> Option<usize> {
    #[cfg(windows)]
    {
        app.get_webview_window("main")?.hwnd().ok().map(|h| h.0 as usize)
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        None
    }
}

/// 앱 시작 시 (setup — 메인 스레드)
pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    #[cfg(windows)]
    if let Some(h) = main_hwnd(app) {
        win::install(h);
    }
}

/// 작업 보호 켜기/끄기 — 절전 방지 + Windows 종료 방지
#[tauri::command]
pub fn run_guard(app: AppHandle, active: bool, reason: Option<String>) -> Result<(), String> {
    let reason = reason.filter(|r| !r.trim().is_empty()).unwrap_or_else(|| "VoLTE 작업 진행 중".into());
    ACTIVE.store(active, Ordering::SeqCst);
    #[cfg(windows)]
    {
        win::power(active, &reason)?;
        if let Some(h) = main_hwnd(&app) {
            app.run_on_main_thread(move || win::shutdown_block(h, active, &reason))
                .map_err(|e| format!("종료 방지 설정 실패: {e}"))?;
        }
    }
    #[cfg(not(windows))]
    let _ = (app, reason);
    eprintln!("[rust] run_guard: {}", if active { "켜짐" } else { "꺼짐" });
    Ok(())
}

#[cfg(all(test, windows))]
mod tests {
    use super::win;

    #[test]
    fn power_request_on_off() {
        win::power(true, "xvolte 테스트").unwrap();
        win::power(true, "xvolte 테스트(갱신)").unwrap(); // 다시 켜도 핸들이 쌓이지 않음
        win::power(false, "").unwrap();
        win::power(false, "").unwrap();
    }

    /// 실제로 Windows에 등록되는지 확인용 (cargo test -- --ignored live_power_request --nocapture)
    /// 관리자 권한 없이 시스템 실행 상태(ES_SYSTEM_REQUIRED = 0x1) 비트로 확인
    /// (다른 프로그램이 이미 깨워 두고 있으면 전·후 차이가 보이지 않음 — 정확한 확인은 관리자 powercfg /requests)
    #[test]
    #[ignore]
    fn live_power_request() {
        use windows_sys::Win32::System::Power::{CallNtPowerInformation, SystemExecutionState};
        let state = || {
            let mut v: u32 = 0;
            let st = unsafe { CallNtPowerInformation(SystemExecutionState, std::ptr::null(), 0, &mut v as *mut u32 as _, 4) };
            assert_eq!(st, 0);
            v
        };
        let before = state();
        win::power(true, "xvolte 보호 확인").unwrap();
        let on = state();
        win::power(false, "").unwrap();
        let off = state();
        eprintln!("실행 상태: 전 {before:#x} → 켬 {on:#x} → 끔 {off:#x}");
        assert!(on & 0x1 != 0, "켰을 때 SYSTEM_REQUIRED 비트가 서야 함");
    }
}
