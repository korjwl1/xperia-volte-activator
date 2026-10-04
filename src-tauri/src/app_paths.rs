//! 앱 경로는 기기 통신 모듈과 독립적으로 관리한다.
use std::path::PathBuf;
use std::sync::OnceLock;

static APP_DATA: OnceLock<PathBuf> = OnceLock::new();

pub fn init(dir: PathBuf) {
    let _ = APP_DATA.set(dir);
}

pub fn data_dir() -> Option<PathBuf> {
    APP_DATA.get().cloned()
}
