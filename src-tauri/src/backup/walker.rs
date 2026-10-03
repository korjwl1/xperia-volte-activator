//! 기기 파일 열거 — ADB SYNC list() 재귀(셸 파싱 없음, 타입 안전).
//! 항목별 제외 규칙(fs-rest 경계)은 프론트 용량 계산(backup-select.md)과 같은 상수로 고정한다.

use adb_client::ADBDeviceExt;
use adb_client::ADBListItemType;
use std::collections::BTreeMap;

/// 파일 항목 id → 기기 최상위 경로·폴더 (mock/apps.ts id와 1:1)
pub const NAMED_FILE_ITEMS: &[(&str, &str)] = &[
    ("dcim", "DCIM"),
    ("download", "Download"),
    ("pictures", "Pictures"),
    ("movies", "Movies"),
    ("music", "Music"),
    ("documents", "Documents"),
    ("recordings", "Recordings"),
];

/// fs-rest의 최상위 제외 — 기명 7폴더(체크 여부 무관) + Android는 하위 data만 제외
pub const FS_REST_ROOT: &str = "/sdcard";
/// app-data 항목 루트
pub const ANDROID_DATA_ROOT: &str = "/sdcard/Android/data";

#[derive(Debug, Clone)]
pub struct WalkedEntry {
    pub remote: String,
    pub size: u64,
    pub mtime: u32,
}

#[derive(Debug, Clone)]
pub struct Skip {
    pub remote: String,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct WalkResult {
    pub files: Vec<WalkedEntry>,
    /// 심볼릭 링크·특수 파일 — 백업하지 않고 기록만(§6-2 소스 변경·누락 투명화)
    pub skipped: Vec<Skip>,
    /// 열거 실패(권한 등) — 오류 0이 아니면 완결 불가
    pub errors: Vec<String>,
}

#[cfg(test)]
impl WalkResult {
    pub fn total_bytes(&self) -> u64 {
        self.files.iter().map(|f| f.size).sum()
    }
}

/// `root` 아래 전수 열거(파일만). `skip`은 각 항목 경로(디렉터리 포함)에 대해 호출,
/// true면 그 항목과 하위 전체를 건너뛴다(제외 사유는 호출부에서 기록).
/// 심볼릭 링크는 따라가지 않는다(사이클·기기 외 경로 방지).
pub fn walk(dev: &mut dyn ADBDeviceExt, root: &str, skip: &dyn Fn(&str) -> bool) -> WalkResult {
    let mut out = WalkResult::default();
    let mut queue: Vec<String> = vec![root.to_string()];
    let mut depth: BTreeMap<String, u32> = BTreeMap::new();
    depth.insert(root.to_string(), 0);
    while let Some(dir) = queue.pop() {
        let entries = match dev.list(&dir) {
            Ok(l) => l,
            Err(e) => {
                out.errors.push(format!(
                    "{}: 열거 실패({e})",
                    crate::backup::scrub(&e.to_string())
                ));
                continue;
            }
        };
        for e in entries {
            let item = match e {
                ADBListItemType::File(i) => i,
                ADBListItemType::Directory(i) => {
                    // "." / ".." 방어 — SYNC list에는 없지만 안전하게
                    if i.name == "." || i.name == ".." {
                        continue;
                    }
                    let path = join(&dir, &i.name);
                    if skip(&path) {
                        continue;
                    }
                    let d = depth[&dir];
                    if d >= 64 {
                        out.errors.push(format!("{path}: 깊이 한도 초과(64)"));
                        continue;
                    }
                    depth.insert(path.clone(), d + 1);
                    queue.push(path);
                    continue;
                }
                ADBListItemType::Symlink(i) => {
                    let path = join(&dir, &i.name);
                    out.skipped.push(Skip {
                        remote: path,
                        reason: "심볼릭 링크".into(),
                    });
                    continue;
                }
                ADBListItemType::Fifo(i)
                | ADBListItemType::CharacterDevice(i)
                | ADBListItemType::BlockDevice(i)
                | ADBListItemType::Socket(i)
                | ADBListItemType::Other(i) => {
                    let path = join(&dir, &i.name);
                    out.skipped.push(Skip {
                        remote: path,
                        reason: "특수 파일".into(),
                    });
                    continue;
                }
            };
            let path = join(&dir, &item.name);
            if skip(&path) {
                continue;
            }
            out.files.push(WalkedEntry {
                remote: path,
                size: item.size as u64,
                mtime: item.time,
            });
        }
    }
    out.files.sort_by(|a, b| a.remote.cmp(&b.remote));
    out
}

/// fs-rest용 제외 집합 — 기명 7폴더 + /sdcard/Android/data 하위트리.
/// Android/media·Android/obb는 fs-rest에 포함(backup-select.md 정의와 동일).
pub fn fs_rest_skip(path: &str) -> bool {
    if path == FS_REST_ROOT {
        return false;
    }
    let top = path.strip_prefix("/sdcard/").unwrap_or(path);
    let first = top.split('/').next().unwrap_or(top);
    if NAMED_FILE_ITEMS.iter().any(|(_, dir)| *dir == first) {
        return true;
    }
    // Android/data만 제외 — Android 자체·media·obb는 포함
    path == "/sdcard/Android/data" || path.starts_with("/sdcard/Android/data/")
}

pub fn join(dir: &str, name: &str) -> String {
    let trimmed = dir.strip_suffix('/').unwrap_or(dir);
    format!("{trimmed}/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    fn dev_with_sdcard() -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.add_dir("/sdcard");
        d.add_dir("/sdcard/DCIM/Camera");
        d.add_file(
            "/sdcard/DCIM/Camera/a.jpg",
            b"jpeg-bytes",
            1700000000,
            0o644,
        );
        d.add_file("/sdcard/DCIM/b:bad.jpg", b"x", 1700000001, 0o644);
        d.add_dir("/sdcard/Download");
        d.add_file("/sdcard/Download/note.txt", b"hello", 1700000002, 0o644);
        d.add_dir("/sdcard/Android/data/com.kakao.talk");
        d.add_file(
            "/sdcard/Android/data/com.kakao.talk/db.bin",
            b"k",
            1700000003,
            0o600,
        );
        d.add_dir("/sdcard/Android/media/com.Slack");
        d.add_file(
            "/sdcard/Android/media/com.Slack/m.png",
            b"m",
            1700000004,
            0o644,
        );
        d.add_symlink("/sdcard/DCIM/latest", "/sdcard/DCIM/Camera");
        d
    }

    #[test]
    fn fs_rest_excludes_named_and_android_data() {
        let mut d = dev_with_sdcard();
        let r = walk(&mut d, FS_REST_ROOT, &fs_rest_skip);
        let paths: Vec<&str> = r.files.iter().map(|f| f.remote.as_str()).collect();
        assert!(paths.contains(&"/sdcard/Android/media/com.Slack/m.png")); // media 포함
        assert!(!paths.iter().any(|p| p.starts_with("/sdcard/DCIM"))); // 기명 제외
        assert!(!paths.iter().any(|p| p.starts_with("/sdcard/Download")));
        assert!(!paths.iter().any(|p| p.starts_with("/sdcard/Android/data"))); // data 제외
        assert!(r.errors.is_empty());
        // 기명 폴더 하위는 통째로 건너뛰므로 그 안의 심볼릭 링크도 보이지 않는다
        assert_eq!(r.skipped.len(), 0);
    }

    #[test]
    fn named_item_walk_only_own_tree() {
        let mut d = dev_with_sdcard();
        let r = walk(&mut d, "/sdcard/DCIM", &|_| false);
        assert_eq!(r.files.len(), 2);
        assert_eq!(r.total_bytes(), (b"jpeg-bytes".len() + 1) as u64);
        assert_eq!(r.skipped.len(), 1); // 심볼릭 링크는 기록만
                                        // 기명 항목 자체는 fs-rest 필터 없이 자기 폴더만 걷는다(Download에 fs-rest 필터를 쓰면 자기 내용이 스킵됨)
        let r2 = walk(&mut d, "/sdcard/Download", &|_| false);
        assert_eq!(r2.files.len(), 1);
    }

    #[test]
    fn enum_error_recorded() {
        let mut d = FakeADBDevice::new();
        d.fail_list("/sdcard/Music");
        let r = walk(&mut d, "/sdcard/Music", &|_| false);
        assert!(!r.errors.is_empty());
        assert!(r.errors[0].contains("/sdcard/Music"));
    }
}
