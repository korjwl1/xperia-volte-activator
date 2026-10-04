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
    /// 다른 기기 파일이 이미 같은 로컬 경로를 차지함(덮어쓰기 방지)
    PathCollision,
}

/// 예약 장치 이름 — COM0/LPT0, 위첨자 ¹²³ 변형, 콘솔 장치까지 (Win32 파일 이름 규칙)
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$", "COM0", "COM1", "COM2", "COM3", "COM4",
    "COM5", "COM6", "COM7", "COM8", "COM9", "COM¹", "COM²", "COM³", "LPT0", "LPT1", "LPT2", "LPT3",
    "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "LPT¹", "LPT²", "LPT³",
];

/// 대소문자 충돌 판정에 쓰는 기록 — 소문자 경로 → (실제 경로, 그 경로를 만든 기기 파일).
/// 폴더 성분은 기기 파일이 None.
pub type SeenPaths = std::collections::HashMap<String, (std::path::PathBuf, Option<String>)>;

/// 파일명 성분 1개 검사 — 문제가 있으면 그 사유
pub fn check_component(name: &str) -> Option<WinNameIssue> {
    if name.is_empty() {
        return Some(WinNameIssue::ForbiddenChar('\0'));
    }
    for c in name.chars() {
        // `\`·`/`는 Windows에서 폴더 구분자 — 기기 파일명 안에 있으면 다른 경로가 된다
        if matches!(c, ':' | '?' | '"' | '*' | '<' | '>' | '|' | '\\' | '/') || c.is_control() {
            return Some(WinNameIssue::ForbiddenChar(c));
        }
    }
    // 예약 이름은 확장자와 무관하게 성급(stem) 기준. Windows는 끝 공백·점을 떼고 해석한다("nul .txt"도 NUL)
    let stem = name
        .split('.')
        .next()
        .unwrap_or("")
        .trim_end_matches([' ', '.']);
    if RESERVED.contains(&stem.to_uppercase().as_str()) {
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

/// 기기 파일 `remote`의 백업 루트 상대 로컬 경로(`/` 구분)가 Windows에 안전한지 전수 검사.
/// 성분은 `/`로만 나눈다 — 기기 파일명 안의 `\`는 구분자가 아니라 금지 문자다.
/// 대소문자 충돌은 이미 본 이름들과 비교한다(NTFS는 대소문자 무시 파일시스템).
/// 안전하면 None, 문제가 있으면 사유 반환.
pub fn check_relative_path(rel: &str, remote: &str, seen: &mut SeenPaths) -> Option<WinNameIssue> {
    let parts: Vec<&str> = rel.split('/').collect();
    // 먼저 성분 전부를 검사해, 쓰지 않을 경로가 충돌 기록에 남지 않게 한다
    if let Some(issue) = parts.iter().find_map(|name| check_component(name)) {
        return Some(issue);
    }
    let mut current = std::path::PathBuf::new();
    for (i, name) in parts.iter().enumerate() {
        current.push(name);
        let owner = (i + 1 == parts.len()).then(|| remote.to_string());
        let key = current.to_string_lossy().to_lowercase();
        match seen.get(&key) {
            // 대소문자만 다른 이름 — 충돌
            Some((prev, _)) if prev != &current => return Some(WinNameIssue::CaseConflict),
            // 같은 경로를 다른 기기 파일(또는 폴더)이 이미 차지 — 같은 파일의 재검사(재시도)만 허용
            Some((_, prev_owner)) if prev_owner != &owner => {
                return Some(WinNameIssue::PathCollision)
            }
            Some(_) => {}
            None => {
                seen.insert(key, (current.clone(), owner));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn forbidden_characters() {
        assert_eq!(
            check_component("a:b.jpg"),
            Some(WinNameIssue::ForbiddenChar(':'))
        );
        assert_eq!(
            check_component("what?.png"),
            Some(WinNameIssue::ForbiddenChar('?'))
        );
        assert_eq!(
            check_component("a<b"),
            Some(WinNameIssue::ForbiddenChar('<'))
        );
        assert!(check_component("2026-10-03 12;30.txt").is_none()); // ; 는 허용
    }

    #[test]
    fn reserved_names() {
        assert_eq!(check_component("CON"), Some(WinNameIssue::ReservedName));
        assert_eq!(check_component("con.txt"), Some(WinNameIssue::ReservedName));
        assert_eq!(
            check_component("lpt9.log"),
            Some(WinNameIssue::ReservedName)
        );
        assert!(check_component("CONNECT.txt").is_none());
        assert!(check_component("console.png").is_none());
    }

    #[test]
    fn trailing_and_length() {
        assert_eq!(
            check_component("name."),
            Some(WinNameIssue::TrailingDotOrSpace)
        );
        assert_eq!(
            check_component("name "),
            Some(WinNameIssue::TrailingDotOrSpace)
        );
        assert!(check_component("name..txt").is_none()); // 끝이 t — 허용
        let long = "a".repeat(241);
        assert!(matches!(
            check_component(&long),
            Some(WinNameIssue::TooLong(241))
        ));
        let ok = "한글".repeat(100); // UTF-16 기준 200자
        assert!(check_component(&ok).is_none());
    }

    #[test]
    fn case_conflict_in_same_dir() {
        let mut seen = SeenPaths::new();
        assert!(check_relative_path("DCIM/a.txt", "/sdcard/DCIM/a.txt", &mut seen).is_none());
        // 같은 폴더, 대소문자만 다름 — 충돌
        assert!(check_relative_path("DCIM/A.txt", "/sdcard/DCIM/A.txt", &mut seen).is_some());
        // 다른 폴더의 같은 이름 — 충돌 아님
        assert!(
            check_relative_path("Download/a.txt", "/sdcard/Download/a.txt", &mut seen).is_none()
        );
        // 완전히 같은 경로 재검사(재시도 상황) — 충돌 아님
        assert!(check_relative_path("DCIM/a.txt", "/sdcard/DCIM/a.txt", &mut seen).is_none());
        // 대소문자가 다른 폴더 아래의 파일도 충돌
        assert_eq!(
            check_relative_path("dcim/b.txt", "/sdcard/dcim/b.txt", &mut seen),
            Some(WinNameIssue::CaseConflict)
        );
    }

    #[test]
    fn separators_inside_a_name_are_not_folders() {
        let mut seen = SeenPaths::new();
        // 기기 파일 "a\b.jpg"는 Windows에서 a 폴더 아래 b.jpg가 되면 안 된다
        assert_eq!(
            check_relative_path("Download/a\\b.jpg", "/sdcard/Download/a\\b.jpg", &mut seen),
            Some(WinNameIssue::ForbiddenChar('\\'))
        );
        // 거부된 경로는 기록되지 않아 진짜 a/b.jpg는 정상 처리된다
        assert!(
            check_relative_path("Download/a/b.jpg", "/sdcard/Download/a/b.jpg", &mut seen)
                .is_none()
        );
    }

    #[test]
    fn different_remote_on_the_same_local_path_collides() {
        let mut seen = SeenPaths::new();
        assert!(
            check_relative_path("apks/p/base.apk", "/data/app/p/base.apk", &mut seen).is_none()
        );
        assert_eq!(
            check_relative_path("apks/p/base.apk", "/data/app/p/lib/base.apk", &mut seen),
            Some(WinNameIssue::PathCollision)
        );
        // 폴더로 쓰인 경로에 파일을 만들 수도 없다
        assert_eq!(
            check_relative_path("apks/p", "/data/app/x/p", &mut seen),
            Some(WinNameIssue::PathCollision)
        );
    }

    #[test]
    fn extended_reserved_names() {
        for name in [
            "COM0",
            "lpt0.txt",
            "COM¹.log",
            "conin$",
            "CONOUT$.txt",
            "nul .txt",
            "Aux..jpg",
        ] {
            assert_eq!(
                check_component(name),
                Some(WinNameIssue::ReservedName),
                "{name}"
            );
        }
        assert!(check_component("COM10").is_none());
    }
}
