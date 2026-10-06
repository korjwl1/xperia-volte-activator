//! 백업 경로를 읽거나 지우기 전에 루트 밖 경로와 심볼릭 링크 탈출을 거부한다.
use std::path::{Path, PathBuf};

pub(super) fn read_error(error: std::io::Error) -> String {
    if error.kind() == std::io::ErrorKind::NotFound {
        error.to_string()
    } else {
        format!("PC_READ_IO|{error}")
    }
}

pub fn relative_path(value: &str) -> Result<PathBuf, String> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.contains('\0')
        || normalized
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(format!("잘못된 백업 상대 경로: {value}"));
    }
    Ok(PathBuf::from(normalized))
}

pub fn existing_file(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("백업 루트 확인 실패: {}", read_error(e)))?;
    let path = root
        .join(relative_path(relative)?)
        .canonicalize()
        .map_err(|e| format!("백업 파일 확인 실패({relative}): {}", read_error(e)))?;
    if !path.starts_with(&root) || !path.is_file() {
        return Err(format!("백업 루트 안의 파일이 아닙니다: {relative}"));
    }
    Ok(path)
}

pub fn write_target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let target = root.join(relative_path(relative)?);
    let mut ancestor = target.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or("백업 경로 부모를 확인할 수 없습니다")?;
    }
    if !ancestor
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(&root)
    {
        return Err("백업 경로가 루트 밖을 가리킵니다".into());
    }
    Ok(target)
}

/// Archive names are validated without assuming every member will be extracted to sdcard.
pub fn archive_relative(remote: &str) -> Result<&str, String> {
    let relative = remote
        .strip_prefix('/')
        .ok_or("기기 절대 경로가 아닙니다")?;
    if relative.is_empty()
        || relative.contains('\0')
        || relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err("잘못된 기기 아카이브 경로입니다".into());
    }
    Ok(relative)
}

pub fn sdcard_relative(remote: &str) -> Result<String, String> {
    archive_relative(remote)?;
    let relative = remote
        .strip_prefix("/sdcard/")
        .ok_or("sdcard 밖의 복구 대상입니다")?;
    if relative.is_empty()
        || relative.contains('\0')
        || relative
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err("잘못된 기기 복구 경로입니다".into());
    }
    Ok(relative.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_windows_and_unix_path_escapes() {
        for value in [
            "../outside",
            "..\\outside",
            "/absolute",
            "C:\\outside",
            "\\\\host\\file",
            "safe/../outside",
        ] {
            assert!(relative_path(value).is_err(), "{value}");
        }
        assert_eq!(
            relative_path("sdcard/사진.jpg").unwrap(),
            PathBuf::from("sdcard/사진.jpg")
        );
        assert!(sdcard_relative("/data/system/file").is_err());
        assert!(sdcard_relative("/sdcard/../data/file").is_err());
    }
}
