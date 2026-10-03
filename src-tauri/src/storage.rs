//! 공통 파일 저장 — 호출마다 고유 임시 파일을 쓰고 sync 뒤 교체한다.
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    let parent = path.parent().ok_or("저장 경로에 부모 폴더가 없습니다")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("저장 폴더 생성 실패: {e}"))?;
    let temporary = parent.join(format!(
        ".xvolte-{}-{}.tmp",
        std::process::id(),
        NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
    ));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)
            .map_err(|e| format!("임시 파일 생성 실패: {e}"))?;
        file.write_all(data)
            .and_then(|_| file.sync_all())
            .map_err(|e| format!("파일 쓰기 실패: {e}"))?;
        drop(file);
        std::fs::rename(&temporary, path).map_err(|e| format!("파일 교체 실패: {e}"))
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replacing_an_existing_file_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("기록.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
