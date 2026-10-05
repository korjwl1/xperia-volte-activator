//! PC(호스트) 측 읽기 전용 질의(드라이브 여유 공간). PC에 쓰는 작업은 각 기능 모듈(storage·journal·backup)에 있다.

use std::path::Path;

/// 백업 저장 위치가 속한 드라이브의 여유 공간(바이트)
fn disk_free_work(path: &str) -> Result<u64, String> {
    let p = Path::new(path);
    if path.trim().is_empty() || !p.exists() {
        return Err("경로를 찾을 수 없습니다".into());
    }
    fs2::available_space(p).map_err(|e| format!("여유 공간 조회 실패: {e}"))
}

/// 네트워크 드라이브·잠든 외장 디스크는 수 초 걸리거나 멈출 수 있으므로 메인 스레드 밖에서 시간 제한을 두고 조회한다
#[tauri::command]
pub async fn disk_free(path: String) -> Result<u64, String> {
    crate::tasks::timed(
        "여유 공간 조회",
        "여유 공간 조회 시간 초과 — 드라이브 연결 상태를 확인해 주세요",
        std::time::Duration::from_secs(15),
        move || disk_free_work(&path),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_free_existing_dir() {
        let dir = std::env::temp_dir();
        assert!(disk_free_work(dir.to_str().unwrap()).unwrap() > 0);
    }

    #[test]
    fn disk_free_missing_path() {
        assert!(disk_free_work("Z:\\없는\\경로 테스트").is_err());
        assert!(disk_free_work("").is_err());
    }
}
