//! 블로킹 I/O와 읽기 질의의 시간 제한. 시간 초과가 작업 취소를 의미하지는 않는다.
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
    blocking("기기 질의", move || match rx.recv_timeout(limit) {
        Ok(result) => result,
        Err(mpsc::RecvTimeoutError::Timeout) => Err("기기 응답 시간 초과".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => {
            Err("기기 질의 중 내부 오류가 발생했습니다".into())
        }
    })
    .await
}
