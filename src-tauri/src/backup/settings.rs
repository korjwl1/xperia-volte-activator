//! 설정 백업/복원 — recovery.md 1-2 절차 그대로.
//! 수집: settings list 3종 + dumpsys deviceidle whitelist + 패키지 목록 덤프.
//! 복원: 검증된 화이트리스트 키만 개별 적용(빌드 간 키 충돌 방지), adb_enabled 제외(보안 토글 — 안내만).

use crate::backup::model::{FileEntry, ItemKind, ItemRecord, ItemStatus};
use adb_client::ADBDeviceExt;
use std::path::Path;

/// 자동 복원 화이트리스트 — (namespace, key) 6종 (plan.md §6-5, recovery.md 1-2 실측)
pub const RESTORE_KEYS: &[(&str, &str)] = &[
    ("secure", "sysui_qs_tiles"),
    ("secure", "default_input_method"),
    ("system", "screen_brightness"),
    ("system", "screen_off_timeout"),
    ("system", "font_scale"),
    // global 네임스페이스 (2026-10-03 실기기 덤프 확인 — system으로 두면 항상 건너뜀)
    ("global", "stay_on_while_plugged_in"),
];

/// adb_enabled은 보안 토글이라 자동 재생하지 않는다 — 안내 문구만 (§6-5)
pub const MANUAL_KEYS_NOTE: &str =
    "USB 디버깅(adb_enabled)은 보안상 자동 복원에서 제외 — 직접 켜야 합니다";

/// 기기 셸 인자용 최소 quoting — 큰따옴표로 감싸고 내부 특수문자 이스케이프.
/// argv 배열 실행 원칙(§12.5)에 따라 우리가 조립하는 유일한 셸 문자열은 이렇게 보호한다.
pub fn shell_quote(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for c in value.chars() {
        if matches!(c, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

fn run(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    crate::device_io::shell(dev, cmd)
}

/// 설정 덤프 수집 — settings/<이름>.txt 5종 (recovery.md Phase A)
pub fn collect_settings(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> ItemRecord {
    let mut rec = ItemRecord {
        id: "settings-all".into(),
        kind: ItemKind::Dump,
        status: ItemStatus::Pending,
        files: 0,
        bytes: 0,
        entries: vec![],
        artifacts: vec![],
        errors: vec![],
    };
    let dir = match super::paths::write_target(backup_root, "settings/settings_system.txt") {
        Ok(path) => path.parent().expect("settings 부모").to_path_buf(),
        Err(e) => {
            rec.errors.push(e);
            rec.status = ItemStatus::Partial;
            return rec;
        }
    };
    if let Err(e) = std::fs::create_dir_all(&dir) {
        rec.errors.push(format!("폴더 생성 실패: {e}"));
        rec.status = ItemStatus::Partial;
        return rec;
    }
    let dumps: &[(&str, &str)] = &[
        ("settings_system.txt", "settings list system"),
        ("settings_global.txt", "settings list global"),
        ("settings_secure.txt", "settings list secure"),
        ("deviceidle_whitelist.txt", "dumpsys deviceidle whitelist"),
        ("packages.txt", "pm list packages -3 -f"),
    ];
    for (name, cmd) in dumps {
        match run(dev, cmd) {
            Ok(out) => {
                let path = dir.join(name);
                match crate::storage::atomic_write(&path, out.as_bytes()) {
                    Ok(()) => {
                        rec.files += 1;
                        rec.bytes += out.len() as u64;
                        rec.artifacts.push(format!("settings/{name}"));
                        use sha2::Digest;
                        rec.entries.push(FileEntry {
                            remote: format!("dump:{name}"),
                            local: format!("settings/{name}"),
                            size: out.len() as u64,
                            mtime: 0,
                            sha256: Some(hex::encode(sha2::Sha256::digest(out.as_bytes()))),
                            quarantined: false,
                            error: None,
                        });
                    }
                    Err(e) => rec.errors.push(format!("{name} 기록 실패: {e}")),
                }
            }
            Err(e) => rec.errors.push(format!("{cmd} 실패: {e}")),
        }
    }
    rec.status = if rec.errors.is_empty() {
        ItemStatus::Done
    } else {
        ItemStatus::Partial
    };
    rec
}

/// 덤프를 한 번만 읽고 key=value를 해석한다. 중복 키는 마지막 값을 사용한다.
fn read_settings(
    backup_root: &Path,
    namespace: &str,
) -> Result<std::collections::HashMap<String, String>, String> {
    let file =
        super::paths::existing_file(backup_root, &format!("settings/settings_{namespace}.txt"))?;
    let raw = std::fs::read_to_string(file)
        .map_err(|e| format!("{namespace} 설정 덤프 읽기 실패: {e}"))?;
    Ok(raw
        .lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect())
}

/// 화이트리스트 복원 — 덤프에 있는 키만, 값이 있을 때만 적용. 결과 문구를 반환(진행 로그용)
pub fn restore_settings(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    // 모든 입력을 먼저 읽어, 뒤쪽 덤프 오류 때문에 일부 키만 적용되는 것을 막는다.
    let mut dumps = std::collections::HashMap::new();
    for (ns, _) in RESTORE_KEYS {
        if !dumps.contains_key(ns) {
            dumps.insert(*ns, read_settings(backup_root, ns)?);
        }
    }
    for (ns, key) in RESTORE_KEYS {
        match dumps.get(ns).and_then(|dump| dump.get(*key)) {
            Some(value) if !value.is_empty() => {
                let cmd = format!("settings put {ns} {key} {}", shell_quote(value));
                run(dev, &cmd)?;
                log.push(format!("{ns}/{key} 복원"));
            }
            _ => log.push(format!("{ns}/{key} — 백업에 값이 없어 건너뜀")),
        }
    }
    log.push(MANUAL_KEYS_NOTE.into());
    Ok(log)
}

/// deviceidle whitelist 재적용 — 덤프에서 서드파티 패키지만 골라 추가 (recovery.md 1-2)
pub fn restore_deviceidle(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
) -> Result<Vec<String>, String> {
    let path = super::paths::existing_file(backup_root, "settings/deviceidle_whitelist.txt")?;
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("whitelist 덤프 읽기 실패: {e}"))?;
    // 시스템 접두사는 제외(이미 시스템 예외) — 서드파티만 재적용
    const SYSTEM_PREFIXES: &[&str] = &[
        "com.android.",
        "com.google.android.",
        "com.sony.",
        "com.sonyericsson.",
        "com.qualcomm.",
        "jp.co.sony.",
        "com.sec.",
        "android.ext.services",
        "com.omtp.",
    ];
    let mut applied = Vec::new();
    for line in raw.lines() {
        let pkg = line
            .trim()
            .trim_start_matches(['+', '='])
            .split(['=', ' '])
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        // 백업 파일에서 읽은 값이 셸 명령에 들어가므로 패키지 이름 형식만 허용
        if pkg.is_empty()
            || !pkg.contains('.')
            || !pkg
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        {
            continue;
        }
        if SYSTEM_PREFIXES.iter().any(|p| pkg.starts_with(p)) {
            continue;
        }
        let cmd = format!("dumpsys deviceidle whitelist +{pkg}");
        run(dev, &cmd).map_err(|e| format!("{pkg} 예외 복원 실패: {e}"))?;
        applied.push(pkg);
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn unreadable_dump_prevents_any_settings_write() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("settings")).unwrap();
        std::fs::write(
            root.path().join("settings/settings_secure.txt"),
            b"sysui_qs_tiles=internet",
        )
        .unwrap();
        std::fs::write(root.path().join("settings/settings_system.txt"), [255]).unwrap();
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("settings put", "");
        assert!(restore_settings(&mut dev, root.path()).is_err());
        assert!(dev.shell_calls.is_empty());
    }

    #[test]
    fn dump_last_value_and_embedded_equals_are_preserved() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("settings")).unwrap();
        for ns in ["secure", "system", "global"] {
            std::fs::write(root.path().join(format!("settings/settings_{ns}.txt")), "").unwrap();
        }
        std::fs::write(
            root.path().join("settings/settings_secure.txt"),
            "sysui_qs_tiles=old\nsysui_qs_tiles=new=with=equals\n",
        )
        .unwrap();
        let mut dev = FakeADBDevice::new();
        dev.answer_shell("settings put", "");
        restore_settings(&mut dev, root.path()).unwrap();
        assert_eq!(
            dev.shell_calls,
            vec!["settings put secure sysui_qs_tiles \"new=with=equals\""]
        );
    }

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_quote("internet,bt"), "\"internet,bt\"");
        assert_eq!(shell_quote("a\"b$c`d"), "\"a\\\"b\\$c\\`d\"");
    }

    #[test]
    fn collect_and_restore_roundtrip() {
        let mut d = FakeADBDevice::new();
        d.answer_shell(
            "settings list system",
            "screen_brightness=31\nscreen_off_timeout=30000\nfont_scale=1.1\n",
        );
        d.answer_shell(
            "settings list global",
            "airplane_mode_on=0\nstay_on_while_plugged_in=7\n",
        );
        d.answer_shell("settings list secure", "sysui_qs_tiles=internet,bt,rotation\ndefault_input_method=com.estsoft.android.keyboard/com.estmob.broccoli.KeyboardService\n");
        d.answer_shell("dumpsys deviceidle whitelist", "+com.android.systemui=u:persistent\n+com.kakao.talk=u:persistent\n+com.friendscube.somoim\n");
        d.answer_shell(
            "pm list packages -3 -f",
            "package:/data/app/~~abc/com.kakao.talk-XYZ/base.apk=com.kakao.talk\n",
        );
        // 복원 명령 응답
        d.answer_shell("settings put", "");
        d.answer_shell("dumpsys deviceidle whitelist +", "");

        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_settings(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Done, "{:?}", rec.errors);
        assert_eq!(rec.artifacts.len(), 5);

        // 복원 — 6키 적용 + adb_enabled 안내
        let log = restore_settings(&mut d, tmp.path()).unwrap();
        assert!(log.iter().any(|l| l.contains("sysui_qs_tiles 복원")));
        assert!(log.iter().any(|l| l.contains("default_input_method 복원")));
        assert!(log.iter().any(|l| l.contains("screen_brightness 복원")));
        assert!(log.iter().any(|l| l.contains("adb_enabled")));
        assert!(log
            .iter()
            .any(|l| l.contains("global/stay_on_while_plugged_in 복원")));
        let calls: Vec<&String> = d.shell_calls.iter().collect();
        let qs = calls.iter().find(|c| c.contains("sysui_qs_tiles")).unwrap();
        assert!(qs.contains("settings put secure sysui_qs_tiles \"internet,bt,rotation\""));

        // deviceidle — 서드파티만(kakao, friendscube), systemui 제외
        let applied = restore_deviceidle(&mut d, tmp.path()).unwrap();
        assert_eq!(
            applied,
            vec![
                "com.kakao.talk".to_string(),
                "com.friendscube.somoim".to_string()
            ]
        );
    }

    #[test]
    fn dump_error_makes_partial() {
        let mut d = FakeADBDevice::new(); // 응답 없음 → 전부 실패
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_settings(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Partial);
        assert_eq!(rec.errors.len(), 5);
    }
}
