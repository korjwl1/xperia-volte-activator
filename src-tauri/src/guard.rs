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
/// 메인 창에 종료 메시지 가로채기(subclass)가 설치됐는지 — 없으면 종료 방지를 켤 수 없다
#[cfg_attr(not(windows), allow(dead_code))]
static SUBCLASSED: AtomicBool = AtomicBool::new(false);
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
    use windows_sys::Win32::Foundation::{
        CloseHandle, HANDLE, HWND, INVALID_HANDLE_VALUE, LPARAM, LRESULT, WPARAM,
    };
    use windows_sys::Win32::System::Power::{
        PowerClearRequest, PowerCreateRequest, PowerRequestSystemRequired, PowerSetRequest,
    };
    use windows_sys::Win32::System::Shutdown::{
        ShutdownBlockReasonCreate, ShutdownBlockReasonDestroy, ShutdownBlockReasonQuery,
    };
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
            // SAFETY: h는 이 모듈이 PowerCreateRequest로 만들어 보관한 유효한 핸들이며, take()로 한 번만 정리한다
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
            Reason: REASON_CONTEXT_0 {
                SimpleReasonString: text.as_mut_ptr(),
            },
        };
        // SAFETY: ctx와 그 안의 사유 문자열(text)은 이 호출 동안 살아 있다
        let h = unsafe { PowerCreateRequest(&ctx) };
        if h.is_null() || h == INVALID_HANDLE_VALUE {
            return Err("절전 방지 요청을 만들 수 없습니다".into());
        }
        // SAFETY: h는 바로 위에서 만든 유효한 요청 핸들이다
        if unsafe { PowerSetRequest(h, PowerRequestSystemRequired) } == 0 {
            // SAFETY: 설정에 실패한 핸들을 한 번만 닫는다
            unsafe { CloseHandle(h) };
            return Err("절전 방지 요청을 설정할 수 없습니다".into());
        }
        *guard = Some(h as usize);
        Ok(())
    }

    /// 창을 만든 스레드(메인)에서 호출해야 한다
    pub fn shutdown_block(hwnd: usize, on: bool, reason: &str) -> Result<(), String> {
        let hwnd = hwnd as HWND;
        // SAFETY: hwnd는 메인 창 핸들이고 이 함수는 그 창을 만든 메인 스레드에서만 호출된다.
        // 사유 문자열(text)은 NUL로 끝나며 호출 동안 살아 있고, 길이 조회는 NULL 버퍼 규약을 따른다
        unsafe {
            if on {
                let text = wide(reason);
                if ShutdownBlockReasonCreate(hwnd, text.as_ptr()) == 0 {
                    return Err("Windows 종료 방지 사유 등록 실패".into());
                }
            } else {
                ShutdownBlockReasonDestroy(hwnd);
            }
            // 등록 상태 확인 로그 (사유가 있으면 길이 > 0)
            let mut len: u32 = 0;
            let set =
                ShutdownBlockReasonQuery(hwnd, std::ptr::null_mut(), &mut len) != 0 && len > 0;
            eprintln!(
                "[rust] Windows 종료 방지 사유: {}",
                if set { "등록됨" } else { "없음" }
            );
            if set != on {
                return Err("Windows 종료 방지 상태 확인 실패".into());
            }
        }
        Ok(())
    }

    /// SAFETY: Windows가 메인 창 메시지 처리 중 호출한다. 받은 인자를 그대로 DefSubclassProc에 넘긴다
    unsafe extern "system" fn subclass(
        hwnd: HWND,
        msg: u32,
        wp: WPARAM,
        lp: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
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

    /// 메인 창에 종료 메시지 가로채기 설치 (메인 스레드, 앱 시작 시 1회). 설치되면 true
    pub fn install(hwnd: usize) -> bool {
        // SAFETY: hwnd는 메인 창 핸들이고 setup(메인 스레드)에서 호출된다. subclass는 정적 함수다
        unsafe { SetWindowSubclass(hwnd as HWND, Some(subclass), 0x5856, 0) != 0 }
    }
}

fn main_hwnd(app: &AppHandle) -> Option<usize> {
    #[cfg(windows)]
    {
        app.get_webview_window("main")?
            .hwnd()
            .ok()
            .map(|h| h.0 as usize)
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
    {
        let installed = main_hwnd(app).is_some_and(win::install);
        SUBCLASSED.store(installed, Ordering::SeqCst);
        if !installed {
            eprintln!("[rust] Windows 종료 가로채기를 설치하지 못했습니다 — 작업 중 종료 방지를 켤 수 없습니다");
        }
    }
}

/// 작업 보호 켜기/끄기 — 절전 방지 + Windows 종료 방지
#[tauri::command]
pub async fn run_guard(app: AppHandle, active: bool, reason: Option<String>) -> Result<(), String> {
    let reason = reason
        .filter(|r| !r.trim().is_empty())
        .unwrap_or_else(|| "VoLTE 작업 진행 중".into());
    #[cfg(windows)]
    {
        // 가로채기가 없으면 종료 사유만 등록되고 실제로는 막히지 않는다 — 켰다고 보고하지 않는다
        if active && !SUBCLASSED.load(Ordering::SeqCst) {
            let _ = win::power(false, "");
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(
                "Windows 종료 가로채기가 설치되지 않아 작업 중 종료 방지를 켤 수 없습니다".into(),
            );
        }
        if let Err(e) = win::power(active, &reason) {
            // 부분 적용 방지 — 켜기에 실패하면 전부 끈 상태로
            let _ = win::power(false, "");
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(e);
        }
        let result = if let Some(h) = main_hwnd(&app) {
            let r = reason.clone();
            let (tx, rx) = std::sync::mpsc::channel();
            app.run_on_main_thread(move || {
                let result = win::shutdown_block(h, active, &r);
                ACTIVE.store(active && result.is_ok(), Ordering::SeqCst);
                let _ = tx.send(result);
            })
            .map_err(|e| format!("종료 방지 설정 실패: {e}"))
            .map(|_| rx)
        } else if active {
            Err("종료 방지를 적용할 메인 창을 찾을 수 없습니다".into())
        } else {
            ACTIVE.store(false, Ordering::SeqCst);
            return Ok(());
        };
        let result = match result {
            Ok(rx) => {
                crate::tasks::blocking("종료 방지 확인", move || {
                    rx.recv().map_err(|e| e.to_string())?
                })
                .await
            }
            Err(e) => Err(e),
        };
        if let Err(e) = result {
            let _ = win::power(false, "");
            ACTIVE.store(false, Ordering::SeqCst);
            return Err(e);
        }
    }
    ACTIVE.store(active, Ordering::SeqCst);
    #[cfg(not(windows))]
    let _ = (app, reason);
    eprintln!("[rust] run_guard: {}", if active { "켜짐" } else { "꺼짐" });
    Ok(())
}

/// CLI owns no window, so it shares sleep prevention but cannot promise GUI shutdown blocking.
#[cfg(feature = "dev-cli")]
pub(crate) struct CliPowerGuard;

#[cfg(feature = "dev-cli")]
impl CliPowerGuard {
    pub fn acquire() -> Result<Self, String> {
        #[cfg(windows)]
        win::power(true, "Xperia 개발 CLI 단계 실행 중")?;
        Ok(Self)
    }
}

#[cfg(feature = "dev-cli")]
impl Drop for CliPowerGuard {
    fn drop(&mut self) {
        #[cfg(windows)]
        let _ = win::power(false, "");
    }
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
            // SAFETY: 입력 없음(NULL·0), 출력은 u32 하나를 가리키는 유효한 포인터와 그 크기
            let st = unsafe {
                CallNtPowerInformation(
                    SystemExecutionState,
                    std::ptr::null(),
                    0,
                    &mut v as *mut u32 as _,
                    4,
                )
            };
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
