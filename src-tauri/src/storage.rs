//! 공통 파일 저장 — 호출마다 고유 임시 파일을 쓰고 sync 뒤 교체한다.
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

pub fn io_error(e: &std::io::Error) -> String {
    if e.kind() == std::io::ErrorKind::StorageFull
        || cfg!(windows) && matches!(e.raw_os_error(), Some(112 | 39))
    {
        format!("NO_SPACE|PC 저장 공간이 부족합니다: {e}")
    } else {
        e.to_string()
    }
}

pub fn adb_io_error(e: &adb_client::RustADBError) -> String {
    match e {
        adb_client::RustADBError::IOError(e) => io_error(e),
        _ => e.to_string(),
    }
}

/// 크기 상한을 실제 읽기에 적용한다. 파일이 커지더라도 메모리 사용량을 제한한다.
pub fn read_bounded(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, String> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(format!("파일 읽기 실패: {}", crate::storage::io_error(&e))),
    };
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("파일 읽기 실패: {}", crate::storage::io_error(&e)))?;
    if bytes.len() > limit {
        return Err("파일이 허용 크기를 초과했습니다".into());
    }
    Ok(Some(bytes))
}

pub fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    atomic_file(path, |file, _| {
        file.write_all(data)
            .map_err(|e| format!("파일 쓰기 실패: {}", crate::storage::io_error(&e)))
    })
}

/// Streaming writes share the same publish-after-success rule as small records.
pub fn atomic_file<T>(
    path: &Path,
    write: impl FnOnce(&mut std::fs::File, &Path) -> Result<T, String>,
) -> Result<T, String> {
    let parent = path.parent().ok_or("저장 경로에 부모 폴더가 없습니다")?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("저장 폴더 생성 실패: {}", crate::storage::io_error(&e)))?;
    // 비정상 종료로 남은 임시 파일과 PID가 겹칠 수 있다 — 다음 번호로 다시 시도한다.
    // 남의 파일을 지우지 않도록 생성에 성공한 이름만 정리 대상으로 삼는다.
    let (temporary, mut file) = {
        let mut attempt = 0;
        loop {
            let temporary = parent.join(format!(
                ".xvolte-{}-{}.tmp",
                std::process::id(),
                NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
            ));
            match std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && attempt < 16 => {
                    attempt += 1;
                }
                Err(e) => {
                    return Err(format!(
                        "임시 파일 생성 실패: {}",
                        crate::storage::io_error(&e)
                    ))
                }
            }
        }
    };
    let result = write(&mut file, &temporary).and_then(|value| {
        file.sync_all()
            .map_err(|e| format!("파일 쓰기 실패: {}", crate::storage::io_error(&e)))?;
        Ok(value)
    });
    drop(file);
    let result = result.and_then(|value| {
        std::fs::rename(&temporary, path)
            .map_err(|e| format!("파일 교체 실패: {}", crate::storage::io_error(&e)))?;
        Ok(value)
    });
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

pub fn hash_reader(mut reader: impl Read) -> Result<(String, u64), String> {
    let mut hash = Sha256::new();
    let mut count = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("해시 읽기 실패: {}", crate::storage::io_error(&e)))?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        count += n as u64;
    }
    Ok((hex::encode(hash.finalize()), count))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_read_distinguishes_missing_empty_and_oversized() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("record");
        assert!(read_bounded(&file, 4).unwrap().is_none());
        std::fs::write(&file, b"").unwrap();
        assert_eq!(read_bounded(&file, 4).unwrap(), Some(vec![]));
        std::fs::write(&file, b"1234").unwrap();
        assert_eq!(read_bounded(&file, 4).unwrap(), Some(b"1234".to_vec()));
        std::fs::write(&file, b"12345").unwrap();
        assert!(read_bounded(&file, 4).is_err());
    }
    #[test]
    fn replacing_an_existing_file_leaves_no_temporary_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("기록.json");
        atomic_write(&path, b"old").unwrap();
        atomic_write(&path, b"new").unwrap();
        assert_eq!(std::fs::read(path).unwrap(), b"new");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    #[test]
    fn failed_stream_keeps_previous_copy_and_cleans_only_its_temporary() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("copy");
        atomic_write(&path, b"good").unwrap();
        let result: Result<(), String> = atomic_file(&path, |file, _| {
            file.write_all(b"partial").unwrap();
            Err("interrupted".into())
        });
        assert!(result.is_err());
        assert_eq!(std::fs::read(path).unwrap(), b"good");
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
    }
}
