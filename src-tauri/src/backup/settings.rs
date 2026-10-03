//! 설정 백업/복원 — recovery.md 1-2 절차 그대로.
//! 수집: settings list 3종 + dumpsys deviceidle whitelist + 패키지 목록 덤프.
//! 복원: 검증된 화이트리스트 키만 개별 적용(빌드 간 키 충돌 방지), adb_enabled 제외(보안 토글 — 안내만).

use crate::backup::model::{ItemKind, ItemRecord, ItemStatus};
use adb_client::ADBDeviceExt;
use std::path::Path;

/// 자동 복원 화이트리스트 — (namespace, key) 6종 (plan.md §6-5, recovery.md 1-2 실측)
pub const RESTORE_KEYS: &[(&str, &str)] = &[
    ("secure", "sysui_qs_tiles"),
    ("secure", "default_input_method"),
    ("system", "screen_brightness"),
    ("system", "screen_off_timeout"),
    ("system", "font_scale"),
    ("system", "stay_on_while_plugged_in"),
];

/// adb_enabled은 보안 토글이라 자동 재생하지 않는다 — 안내 문구만 (§6-5)
pub const MANUAL_KEYS_NOTE: &str = "USB 디버깅(adb_enabled)은 보안상 자동 복원에서 제외 — 직접 켜야 합니다";

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
    let mut out = Vec::new();
    let mut err = Vec::new();
    dev.shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| format!("{e}: {}", String::from_utf8_lossy(&err).trim()))?;
    if !err.is_empty() {
        // 셸 v1에서 stderr는 종종 비정상 종료와 함께 온다 — exit code 확인
        return Err(String::from_utf8_lossy(&err).trim().to_string());
    }
    String::from_utf8(out).map_err(|e| format!("출력 해석 실패: {e}"))
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
    let dir = backup_root.join("settings");
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
                match std::fs::write(&path, out.as_bytes()) {
                    Ok(()) => {
                        rec.files += 1;
                        rec.bytes += out.len() as u64;
                        rec.artifacts.push(format!("settings/{name}"));
                    }
                    Err(e) => rec.errors.push(format!("{name} 기록 실패: {e}")),
                }
            }
            Err(e) => rec.errors.push(format!("{cmd} 실패: {e}")),
        }
    }
    rec.status = if rec.errors.is_empty() { ItemStatus::Done } else { ItemStatus::Partial };
    rec
}

/// 백업된 설정 덤프에서 화이트리스트 키의 마지막 값을 찾는다
fn dumped_value(backup_root: &Path, namespace: &str, key: &str) -> Option<String> {
    let file = backup_root.join("settings").join(format!("settings_{namespace}.txt"));
    let raw = std::fs::read_to_string(file).ok()?;
    // key=value (값에 '=' 포함 가능 — 첫 구분자만)
    raw.lines()
        .find_map(|l| l.split_once('=').filter(|(k, _)| k.trim() == key))
        .map(|(_, v)| v.trim().to_string())
}

/// 화이트리스트 복원 — 덤프에 있는 키만, 값이 있을 때만 적용. 결과 문구를 반환(진행 로그용)
pub fn restore_settings(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    for (ns, key) in RESTORE_KEYS {
        match dumped_value(backup_root, ns, key) {
            Some(value) if !value.is_empty() => {
                let cmd = format!("settings put {ns} {key} {}", shell_quote(&value));
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
pub fn restore_deviceidle(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<Vec<String>, String> {
    let path = backup_root.join("settings").join("deviceidle_whitelist.txt");
    let raw = std::fs::read_to_string(path).map_err(|e| format!("whitelist 덤프 읽기 실패: {e}"))?;
    // 시스템 접두사는 제외(이미 시스템 예외) — 서드파티만 재적용
    const SYSTEM_PREFIXES: &[&str] = &[
        "com.android.", "com.google.android.", "com.sony.", "com.sonyericsson.", "com.qualcomm.",
        "jp.co.sony.", "com.sec.", "android.ext.services", "com.omtp.",
    ];
    let mut applied = Vec::new();
    for line in raw.lines() {
        let pkg = line
            .trim()
            .trim_start_matches(['+', '='])
            .split(|c| c == '=' || c == ' ')
            .next()
            .unwrap_or("")
            .trim()
            .to_string();
        // 백업 파일에서 읽은 값이 셸 명령에 들어가므로 패키지 이름 형식만 허용
        if pkg.is_empty() || !pkg.contains('.') || !pkg.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_') {
            continue;
        }
        if SYSTEM_PREFIXES.iter().any(|p| pkg.starts_with(p)) {
            continue;
        }
        let cmd = format!("dumpsys deviceidle whitelist +{pkg}");
        if run(dev, &cmd).is_ok() {
            applied.push(pkg);
        }
    }
    Ok(applied)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_quote("internet,bt"), "\"internet,bt\"");
        assert_eq!(shell_quote("a\"b$c`d"), "\"a\\\"b\\$c\\`d\"");
    }

    #[test]
    fn collect_and_restore_roundtrip() {
        let mut d = FakeADBDevice::new();
        d.answer_shell("settings list system", "screen_brightness=31\nscreen_off_timeout=30000\nfont_scale=1.1\nstay_on_while_plugged_in=7\n");
        d.answer_shell("settings list global", "airplane_mode_on=0\n");
        d.answer_shell("settings list secure", "sysui_qs_tiles=internet,bt,rotation\ndefault_input_method=com.estsoft.android.keyboard/com.estmob.broccoli.KeyboardService\n");
        d.answer_shell("dumpsys deviceidle whitelist", "+com.android.systemui=u:persistent\n+com.kakao.talk=u:persistent\n+com.friendscube.somoim\n");
        d.answer_shell("pm list packages -3 -f", "package:/data/app/~~abc/com.kakao.talk-XYZ/base.apk=com.kakao.talk\n");
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
        let calls: Vec<&String> = d.shell_calls.iter().collect();
        let qs = calls.iter().find(|c| c.contains("sysui_qs_tiles")).unwrap();
        assert!(qs.contains("settings put secure sysui_qs_tiles \"internet,bt,rotation\""));

        // deviceidle — 서드파티만(kakao, friendscube), systemui 제외
        let applied = restore_deviceidle(&mut d, tmp.path()).unwrap();
        assert_eq!(applied, vec!["com.kakao.talk".to_string(), "com.friendscube.somoim".to_string()]);
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
