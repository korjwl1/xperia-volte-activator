//! Windows 비호환 파일명 판정 — plan.md §6-3
//! 금지 문자, 예약 이름(CON/PRN/AUX/NUL/COM1~9/LPT1~9), 끝 점·공백, 제어 문자,
//! NTFS 대소문자 충돌(같은 폴더 a.txt/A.txt). 실제 경로 한도는 puller에서 측정해 기록한다.

/// 파일명(경로 성분 1개) 검사 결과
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WinNameIssue {
    /// `: ? " * < > |` 와 제어 문자
    ForbiddenChar(char),
    /// CON, PRN, AUX, NUL, COM1~9, LPT1~9 (확장자가 있어도 예약 — "CON.txt"도 안 됨)
    ReservedName,
    /// 이름이 끝 점 또는 공백으로 끝남
    TrailingDotOrSpace,
    /// 성분이 너무 김(NTFS 255자 — 여유를 두고 240자부터 격리)
    TooLong(usize),
    /// 같은 폴더에 대소문자만 다른 이름이 이미 있음(NTFS 대소문자 무시 충돌)
    CaseConflict,
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// 파일명 성분 1개 검사 — 문제가 있으면 그 사유
pub fn check_component(name: &str) -> Option<WinNameIssue> {
    if name.is_empty() {
        return Some(WinNameIssue::ForbiddenChar('\0'));
    }
    for c in name.chars() {
        if matches!(c, ':' | '?' | '"' | '*' | '<' | '>' | '|') || c.is_control() {
            return Some(WinNameIssue::ForbiddenChar(c));
        }
    }
    // 예약 이름은 확장자와 무관하게 성급(stem) 기준
    let stem = name.split('.').next().unwrap_or("");
    if RESERVED.contains(&stem.to_ascii_uppercase().as_str()) {
        return Some(WinNameIssue::ReservedName);
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Some(WinNameIssue::TrailingDotOrSpace);
    }
    // UTF-16 기준 길이(Windows 파일명 한도 계산) — 240자부터 격리해 경로 여유를 둔다
    let len = name.encode_utf16().count();
    if len > 240 {
        return Some(WinNameIssue::TooLong(len));
    }
    None
}

/// 기기 경로(/sdcard/...) → 백업 루트 상대 로컬 경로가 Windows에 안전한지 전수 검사.
/// 대소문자 충돌은 폴더별로 이미 본 이름들과 비교한다(NTFS는 대소문자 무시 파일시스템).
/// 안전하면 None, 문제가 있으면 사유 반환.
pub fn check_relative_path(
    rel: &str,
    seen: &mut std::collections::HashMap<String, std::path::PathBuf>,
) -> Option<WinNameIssue> {
    let path = std::path::Path::new(rel);
    let mut current = std::path::PathBuf::new();
    for comp in path.components() {
        let Some(name) = comp.as_os_str().to_str() else {
            return Some(WinNameIssue::ForbiddenChar('\u{fffd}')); // UTF-16 변환 불가 문자
        };
        if let Some(issue) = check_component(name) {
            return Some(issue);
        }
        current.push(name);
        let key = current.to_string_lossy().to_lowercase();
        match seen.get(&key) {
            // 같은 폴더(부모 같음)에서 대소문자만 다른 이름 — 충돌. 완전히 같은 경로(재검사)는 제외
            Some(prev) if prev != &current && prev.parent() == current.parent() => {
                return Some(WinNameIssue::CaseConflict)
            }
            Some(_) => {}
            None => {
                seen.insert(key, current.clone());
            }
        }
    }
    None
}

/// 사유를 manifest 오류 문구로
pub fn issue_reason(issue: &WinNameIssue) -> String {
    match issue {
        WinNameIssue::ForbiddenChar(c) => format!("Windows에서 쓸 수 없는 문자({c:?})"),
        WinNameIssue::ReservedName => "Windows 예약 이름(CON, COM1 등)".into(),
        WinNameIssue::TrailingDotOrSpace => "이름이 점·공백으로 끝남".into(),
        WinNameIssue::TooLong(n) => format!("이름이 너무 김({n}자)"),
        WinNameIssue::CaseConflict => "같은 폴더에 대소문자만 다른 이름이 있음".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_characters() {
        assert_eq!(check_component("a:b.jpg"), Some(WinNameIssue::ForbiddenChar(':')));
        assert_eq!(check_component("what?.png"), Some(WinNameIssue::ForbiddenChar('?')));
        assert_eq!(check_component("a<b"), Some(WinNameIssue::ForbiddenChar('<')));
        assert!(check_component("2026-10-03 12;30.txt").is_none()); // ; 는 허용
    }

    #[test]
    fn reserved_names() {
        assert_eq!(check_component("CON"), Some(WinNameIssue::ReservedName));
        assert_eq!(check_component("con.txt"), Some(WinNameIssue::ReservedName));
        assert_eq!(check_component("lpt9.log"), Some(WinNameIssue::ReservedName));
        assert!(check_component("CONNECT.txt").is_none());
        assert!(check_component("console.png").is_none());
    }

    #[test]
    fn trailing_and_length() {
        assert_eq!(check_component("name."), Some(WinNameIssue::TrailingDotOrSpace));
        assert_eq!(check_component("name "), Some(WinNameIssue::TrailingDotOrSpace));
        assert!(check_component("name..txt").is_none()); // 끝이 t — 허용
        let long = "a".repeat(241);
        assert!(matches!(check_component(&long), Some(WinNameIssue::TooLong(241))));
        let ok = "한글".repeat(100); // UTF-16 기준 200자
        assert!(check_component(&ok).is_none());
    }

    #[test]
    fn case_conflict_in_same_dir() {
        let mut seen = std::collections::HashMap::new();
        assert!(check_relative_path("DCIM/a.txt", &mut seen).is_none());
        // 같은 폴더, 대소문자만 다름 — 충돌
        assert!(check_relative_path("DCIM/A.txt", &mut seen).is_some());
        // 다른 폴더의 같은 이름 — 충돌 아님
        assert!(check_relative_path("Download/a.txt", &mut seen).is_none());
        // 완전히 같은 경로 재검사(재시도 상황) — 충돌 아님
        assert!(check_relative_path("DCIM/a.txt", &mut seen).is_none());
    }
}
