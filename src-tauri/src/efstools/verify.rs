//! 전수 리드백 검증 — 프리셋 폴더의 모든 파일을 기기에서 다시 받은 결과와 SHA-256으로 비교.
//! EfsTools 종료 코드가 무의미하기 때문에 이 비교가 업로드 성공 판정의 유일한 최종 근거다.

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::Path;

#[derive(Debug, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyReport {
    pub ok: bool,
    pub files: u32,
    pub matched: u32,
    pub mismatches: Vec<String>,
    pub missing: Vec<String>,
}

fn sha256_file(path: &Path) -> Result<String, String> {
    let data = std::fs::read(path).map_err(|e| e.to_string())?;
    Ok(hex::encode(Sha256::digest(&data)))
}

/// 프리셋 폴더 트리의 상대 경로 전수 나열
fn walk_files(dir: &Path, base: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if p.is_dir() {
            walk_files(&p, base, out);
        } else if let Ok(rel) = p.strip_prefix(base) {
            out.push(rel.to_path_buf());
        }
    }
}

/// 프리셋(로컬 원본) vs 리드백(기기에서 받은 사본) 비교.
/// downloaded_root는 프리셋의 최상위 구조를 그대로 담고 있어야 한다
/// (uploadDirectory -i preset -o / 의 역연산: downloadDirectory -i /<top> -o root).
pub fn compare(preset_dir: &Path, downloaded_root: &Path) -> VerifyReport {
    let mut files = Vec::new();
    walk_files(preset_dir, preset_dir, &mut files);
    files.sort();
    let mut report = VerifyReport {
        ok: false,
        files: files.len() as u32,
        ..Default::default()
    };
    for rel in &files {
        let expect = preset_dir.join(rel);
        let actual = downloaded_root.join(rel);
        if !actual.is_file() {
            report.missing.push(rel.to_string_lossy().replace('\\', "/"));
            continue;
        }
        match (sha256_file(&expect), sha256_file(&actual)) {
            (Ok(a), Ok(b)) if a == b => report.matched += 1,
            (Ok(_), Ok(_)) => report.mismatches.push(rel.to_string_lossy().replace('\\', "/")),
            (e1, e2) => {
                let why = e1.err().or(e2.err()).unwrap_or_default();
                report.mismatches.push(format!(
                    "{} ({why})",
                    rel.to_string_lossy().replace('\\', "/")
                ));
            }
        }
    }
    report.ok = report.matched == report.files && report.files > 0;
    report
}

/// 프리셋 최상위 폴더들(중복 제거) — 폴더별 downloadDirectory 대상
pub fn top_level_dirs(preset_dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(rd) = std::fs::read_dir(preset_dir) {
        for e in rd.filter_map(|e| e.ok()) {
            if e.path().is_dir() {
                if let Some(n) = e.file_name().to_str() {
                    out.push(n.to_string());
                }
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(dir: &Path, rel: &str, data: &[u8]) {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }

    fn setup() -> (tempfile::TempDir, tempfile::TempDir) {
        let preset = tempfile::tempdir().unwrap();
        let down = tempfile::tempdir().unwrap();
        (preset, down)
    }

    #[test]
    fn full_match_passes() {
        let (preset, down) = setup();
        write(preset.path(), "policyman/a.xml", b"<a/>");
        write(preset.path(), "policyman/sub/b.xml", b"<b/>");
        write(preset.path(), "nvm/nums", b"1234");
        write(down.path(), "policyman/a.xml", b"<a/>");
        write(down.path(), "policyman/sub/b.xml", b"<b/>");
        write(down.path(), "nvm/nums", b"1234");
        let r = compare(preset.path(), down.path());
        assert!(r.ok);
        assert_eq!(r.matched, 3);
        assert_eq!(r.files, 3);
    }

    #[test]
    fn mismatch_and_missing_fail() {
        let (preset, down) = setup();
        write(preset.path(), "policyman/a.xml", b"original");
        write(preset.path(), "nvm/nums", b"data");
        write(down.path(), "policyman/a.xml", b"TAMPERED"); // 불일치
        // nvm/nums 누락
        let r = compare(preset.path(), down.path());
        assert!(!r.ok);
        assert_eq!(r.mismatches, vec!["policyman/a.xml".to_string()]);
        assert_eq!(r.missing, vec!["nvm/nums".to_string()]);
        assert_eq!(r.matched, 0);
    }

    #[test]
    fn empty_preset_fails_closed() {
        let (preset, down) = setup();
        let r = compare(preset.path(), down.path());
        assert!(!r.ok); // 검증할 파일 없음 = 성공 아님(위장 성공 금지)
        assert_eq!(r.files, 0);
    }

    #[test]
    fn extra_files_in_device_are_ignored() {
        // 기기에 추가로 있는 파일(다른 통신사 잔재 등)은 프리셋 비교 대상 아님 — 판정은 프리셋 전수만
        let (preset, down) = setup();
        write(preset.path(), "policyman/a.xml", b"x");
        write(down.path(), "policyman/a.xml", b"x");
        write(down.path(), "other/extra", b"y");
        let r = compare(preset.path(), down.path());
        assert!(r.ok);
    }

    #[test]
    fn top_level_dirs_unique_sorted() {
        let (preset, _) = setup();
        write(preset.path(), "b/f", b"1");
        write(preset.path(), "a/g", b"2");
        write(preset.path(), "a/h", b"3");
        write(preset.path(), "loose.txt", b"4"); // 루트 파일 — 폴더 아님
        assert_eq!(top_level_dirs(preset.path()), vec!["a".to_string(), "b".to_string()]);
    }
}
