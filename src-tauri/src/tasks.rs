//! 블로킹 I/O와 읽기 질의의 시간 제한. 시간 초과가 작업 취소를 의미하지는 않는다.
//! 규칙: 기기 읽기 질의 → `guarded`(동시 질의 상한 포함), PC·네트워크 I/O → `timed`(상한만),
//! 기기 변경 → `WriteOperation` + `blocking`.
use std::sync::{mpsc, Mutex};
use std::time::Duration;

pub async fn blocking<T: Send + 'static>(
    label: &'static str,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| format!("{label} 작업 스레드 오류: {e}"))?
}

// 멈춘 기기 질의가 반복될 때 무제한 스레드를 만들지 않는다.
static PROBES: Mutex<usize> = Mutex::new(0);
const MAX_PROBES: usize = 4;

struct ProbePermit;
impl ProbePermit {
    fn acquire() -> Result<Self, String> {
        let mut count = PROBES.lock().map_err(|_| "기기 질의 상태 잠금 실패")?;
        if *count >= MAX_PROBES {
            return Err("기기 질의가 아직 진행 중입니다".into());
        }
        *count += 1;
        Ok(Self)
    }
}
impl Drop for ProbePermit {
    fn drop(&mut self) {
        if let Ok(mut count) = PROBES.lock() {
            *count -= 1;
        }
    }
}

/// 작업을 별도 스레드에서 실행하고 `limit` 안에 끝나지 않으면 `timeout_message`로 실패한다.
/// 시간 초과 뒤에도 작업 스레드는 끝날 때까지 계속 돈다(결과는 버린다).
pub async fn timed<T: Send + 'static>(
    label: &'static str,
    timeout_message: &'static str,
    limit: Duration,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name(label.into())
        .spawn(move || {
            let _ = tx.send(work());
        })
        .map_err(|e| format!("{label} 스레드 생성 실패: {e}"))?;
    wait_result(label, timeout_message, limit, rx).await
}

async fn wait_result<T: Send + 'static>(
    label: &'static str,
    timeout_message: &'static str,
    limit: Duration,
    rx: mpsc::Receiver<Result<T, String>>,
) -> Result<T, String> {
    blocking(label, move || match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err(timeout_message.into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err(format!("{label} 중 내부 오류가 발생했습니다"))
        }
    })
    .await
}

/// 기기 읽기 질의 — 동시에 살아 있는 질의 수를 제한하고 시간 제한을 둔다
pub async fn guarded<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let permit = ProbePermit::acquire()?;
    let (tx, rx) = mpsc::channel();
    std::thread::Builder::new()
        .name("device-probe".into())
        .spawn(move || {
            let _permit = permit;
            let _ = tx.send(work());
        })
        .map_err(|e| format!("기기 질의 스레드 생성 실패: {e}"))?;
    wait_result("기기 질의", "기기 응답 시간 초과", limit, rx).await
}
