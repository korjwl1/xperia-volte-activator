//! PC(호스트) 측 읽기 전용 질의.
//! 규칙: PC에 영향을 주는(쓰기·설치·삭제) 명령은 이 모듈에 작성 금지.

use std::path::Path;

/// 백업 저장 위치가 속한 드라이브의 여유 공간(바이트)
fn disk_free_work(path: &str) -> Result<u64, String> {
    let p = Path::new(path);
    if path.trim().is_empty() || !p.exists() {
        return Err("경로를 찾을 수 없습니다".into());
    }
    fs2::available_space(p).map_err(|e| format!("여유 공간 조회 실패: {e}"))
}

#[tauri::command]
pub fn disk_free(path: String) -> Result<u64, String> {
    disk_free_work(&path)
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
