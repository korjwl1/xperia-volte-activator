//! 설정 백업/복원 — recovery.md 1-2 절차 + 2026-10-08 실기기 복원 점검으로 확장.
//! 수집: settings list 3종 + deviceidle + 패키지 목록 + 기본 앱(역할) + 앱 런타임 권한 + appops.
//! 복원: 기기에 묶이지 않는 사용자 설정만 개별 적용(빌드 간 키 충돌·기기 고유값 방지)하고, 적용한 것은
//! 다시 읽어 확인한다. adb_enabled은 제외(보안 토글 — 안내만).

use crate::backup::model::{FileEntry, ItemKind, ItemRecord};
use adb_client::ADBDeviceExt;
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

/// 그대로 옮겨도 되는 사용자 설정 — (namespace, key, 표시 이름).
/// 기기 고유값(android_id·SIM subId·부팅 횟수 등)과 내부 상태는 넣지 않는다. 밝기 모드는 밝기보다 먼저.
pub const RESTORE_KEYS: &[(&str, &str, &str)] = &[
    ("system", "screen_brightness_mode", "자동 밝기"),
    ("system", "screen_brightness", "화면 밝기"),
    ("system", "screen_off_timeout", "화면 꺼짐 시간"),
    ("system", "font_scale", "글꼴 크기"),
    ("system", "accelerometer_rotation", "자동 회전"),
    ("system", "peak_refresh_rate", "최대 주사율"),
    ("system", "min_refresh_rate", "최소 주사율"),
    ("system", "vibrate_when_ringing", "벨 울릴 때 진동"),
    ("system", "haptic_feedback_enabled", "터치 진동"),
    ("system", "sound_effects_enabled", "터치음"),
    ("system", "lockscreen_sounds_enabled", "화면 잠금음"),
    ("system", "keyboard_vibration_enabled", "키보드 진동"),
    ("system", "dtmf_tone", "다이얼 패드 소리"),
    ("system", "time_12_24", "시간 형식"),
    ("system", "notification_light_pulse", "알림 표시등"),
    ("secure", "double_tap_to_wake", "두 번 탭해 화면 켜기"),
    ("secure", "wake_gesture_enabled", "들어서 화면 켜기"),
    ("secure", "doze_pulse_on_pick_up", "들면 알림 표시"),
    ("secure", "lock_screen_show_notifications", "잠금 화면 알림"),
    ("secure", "lock_screen_allow_private_notifications", "잠금 화면 알림 내용"),
    ("secure", "night_display_activated", "야간 조명"),
    ("secure", "zen_duration", "방해 금지 기간"),
    // global 네임스페이스 (2026-10-03 실기기 덤프 확인 — system으로 두면 항상 건너뜀)
    ("global", "stay_on_while_plugged_in", "충전 중 화면 유지"),
    ("global", "auto_time", "자동 시간"),
    ("global", "auto_time_zone", "자동 시간대"),
];

/// 소리 설정 — 값이 `content://media/<볼륨>/audio/media/<id>?title=…`이라 id가 기기마다 다르다.
/// 같은 이름·종류의 소리를 이 폰에서 찾아 지정한다. (key, 종류 열, 표시 이름)
const SOUND_KEYS: &[(&str, &str, &str)] = &[
    ("ringtone", "is_ringtone", "벨소리"),
    ("ringtone2", "is_ringtone", "벨소리(SIM 2)"),
    ("notification_sound", "is_notification", "알림음"),
    ("alarm_alert", "is_alarm", "알람음"),
];

/// 기본 앱 역할 — 백업 때 기록하고 복원 때 설치돼 있으면 다시 지정한다
const ROLES: &[(&str, &str)] = &[
    ("android.app.role.SMS", "기본 문자 앱"),
    ("android.app.role.DIALER", "기본 전화 앱"),
    ("android.app.role.BROWSER", "기본 브라우저"),
    ("android.app.role.HOME", "기본 홈 앱"),
    ("android.app.role.ASSISTANT", "기본 지원 앱"),
    ("android.app.role.CALL_SCREENING", "통화 스크리닝 앱"),
];

/// 앱별로 기록·복원하는 특수 접근(appops)
const APP_OPS: &[&str] = &[
    "RUN_ANY_IN_BACKGROUND",
    "SYSTEM_ALERT_WINDOW",
    "GET_USAGE_STATS",
    "MANAGE_EXTERNAL_STORAGE",
    "REQUEST_INSTALL_PACKAGES",
    "SCHEDULE_EXACT_ALARM",
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

fn roles_dump_cmd() -> String {
    let mut s = String::new();
    for (role, _) in ROLES {
        s.push_str(&format!("echo \"{role}=$(cmd role get-role-holders {role})\"; "));
    }
    s
}

/// 서드파티 앱의 허용된 런타임 권한 — `<패키지> <권한>` 줄
const PERMISSIONS_DUMP: &str = "for p in $(pm list packages -3 | cut -d: -f2); do \
dumpsys package \"$p\" | sed -n '/runtime permissions:/,/^ *[a-zA-Z]* *[a-zA-Z]*:$/p' | grep 'granted=true' \
| sed \"s/^ *//; s/:.*//; s/^/$p /\"; done";

fn appops_dump_cmd() -> String {
    format!(
        "for p in $(pm list packages -3 | cut -d: -f2); do cmd appops get \"$p\" | grep -E '^({}):' | sed \"s/^/$p /\"; done",
        APP_OPS.join("|")
    )
}

/// 설정 덤프 수집 — settings/<이름>.txt (recovery.md Phase A + 기본 앱·권한·appops)
pub fn collect_settings(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> ItemRecord {
    let mut rec = ItemRecord::new("settings-all", ItemKind::Dump);
    let dir = match super::paths::write_target(backup_root, "settings/settings_system.txt") {
        Ok(path) => match path.parent() {
            Some(dir) => dir.to_path_buf(),
            None => return rec.fail("설정 폴더 경로를 확인할 수 없습니다".into()),
        },
        Err(e) => return rec.fail(e),
    };
    let roles = roles_dump_cmd();
    let appops = appops_dump_cmd();
    let dumps: &[(&str, &str)] = &[
        ("settings_system.txt", "settings list system"),
        ("settings_global.txt", "settings list global"),
        ("settings_secure.txt", "settings list secure"),
        ("deviceidle_whitelist.txt", "dumpsys deviceidle whitelist"),
        ("packages.txt", "pm list packages -3 -f"),
        // 앱별 원래 설치 출처 — 복원 때 같은 출처로 설치해야 Play 스토어가 업데이트한다
        ("installers.txt", "pm list packages -3 -i"),
        ("roles.txt", &roles),
        ("permissions.txt", PERMISSIONS_DUMP),
        ("appops.txt", &appops),
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
            Err(e) => rec.errors.push(format!("{name} 수집 실패: {e}")),
        }
    }
    rec.finalize();
    rec
}

/// 덤프를 한 번만 읽고 key=value를 해석한다. 중복 키는 마지막 값을 사용한다.
fn read_settings(backup_root: &Path, namespace: &str) -> Result<HashMap<String, String>, String> {
    let file =
        super::paths::existing_file(backup_root, &format!("settings/settings_{namespace}.txt"))?;
    let raw = std::fs::read_to_string(file)
        .map_err(|e| format!("{namespace} 설정 덤프 읽기 실패: {e}"))?;
    Ok(parse_kv(&raw))
}

fn parse_kv(raw: &str) -> HashMap<String, String> {
    raw.lines()
        .filter_map(|line| line.split_once('='))
        .map(|(key, value)| (key.trim().to_string(), value.trim().to_string()))
        .collect()
}

/// 예전 백업에는 없을 수 있는 덤프 — 없으면 None
fn read_optional(backup_root: &Path, name: &str) -> Result<Option<String>, String> {
    match super::paths::existing_file(backup_root, &format!("settings/{name}")) {
        Ok(path) => std::fs::read_to_string(path)
            .map(Some)
            .map_err(|e| format!("{name} 읽기 실패: {e}")),
        Err(_) => Ok(None),
    }
}

fn installed_packages(dev: &mut dyn ADBDeviceExt) -> Result<BTreeSet<String>, String> {
    Ok(run(dev, "pm list packages")?
        .lines()
        .filter_map(|l| l.trim().strip_prefix("package:"))
        .map(str::to_string)
        .collect())
}

/// 패키지/컴포넌트 이름 — 셸에 넣기 전 형식 검사
fn valid_component(c: &str) -> bool {
    match c.split_once('/') {
        Some((pkg, cls)) => {
            super::sms_role::valid_package(pkg)
                && !cls.is_empty()
                && cls.chars().all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '$'))
        }
        None => false,
    }
}

/// 설정 복원 — 결과 문구를 반환(진행 로그용). 적용한 값은 다시 읽어 확인하고, 반영되지 않은 것은
/// `[확인 필요]`로 모은다(성공으로 보고하지 않는다).
pub fn restore_settings(dev: &mut dyn ADBDeviceExt, backup_root: &Path) -> Result<Vec<String>, String> {
    let mut log = Vec::new();
    // 모든 입력을 먼저 읽어, 뒤쪽 덤프 오류 때문에 일부 키만 적용되는 것을 막는다.
    let mut dumps = HashMap::new();
    for ns in ["system", "secure", "global"] {
        dumps.insert(ns, read_settings(backup_root, ns)?);
    }
    let roles = read_optional(backup_root, "roles.txt")?;
    let permissions = read_optional(backup_root, "permissions.txt")?;
    let appops = read_optional(backup_root, "appops.txt")?;
    let installed = installed_packages(dev)?;
    let mut applied = vec![];
    let mut not_applied = vec![];

    // 1) 일반 사용자 설정 — 값이 다를 때만 쓰고 다시 읽는다
    for (ns, key, label) in RESTORE_KEYS {
        let Some(value) = dumps[ns].get(*key).filter(|v| !v.is_empty() && *v != "null") else {
            continue;
        };
        let current = run(dev, &format!("settings get {ns} {key}"))?;
        if current.trim() == value {
            continue;
        }
        run(dev, &format!("settings put {ns} {key} {}", shell_quote(value)))?;
        if run(dev, &format!("settings get {ns} {key}"))?.trim() == value {
            applied.push(label.to_string());
        } else {
            not_applied.push(label.to_string());
        }
    }

    // 빠른 설정 타일 — 시스템 UI가 직접 관리해 settings put은 즉시 되돌린다(Android 15 실측). 공식 명령으로 지정
    if let Some(tiles) = dumps["secure"].get("sysui_qs_tiles").filter(|v| !v.is_empty() && *v != "null") {
        if run(dev, "settings get secure sysui_qs_tiles")?.trim() != tiles {
            run(dev, &format!("cmd statusbar set-tiles {}", shell_quote(tiles)))?;
            if run(dev, "settings get secure sysui_qs_tiles")?.trim() == tiles {
                applied.push("빠른 설정 타일".into());
            } else {
                not_applied.push("빠른 설정 타일".into());
            }
        }
    }

    // 2) 소리 — 같은 이름·종류의 소리를 이 폰에서 찾아 지정
    for (key, kind, label) in SOUND_KEYS {
        let Some(value) = dumps["system"].get(*key) else { continue };
        let Some((volume, title)) = parse_media_uri(value) else { continue };
        let current = run(dev, &format!("settings get system {key}"))?;
        if parse_media_uri(current.trim()).as_ref().is_some_and(|(_, t)| t == &title) {
            continue;
        }
        match find_sound(dev, &volume, kind, &title)? {
            Some(id) => {
                let uri = format!(
                    "content://media/{volume}/audio/media/{id}?title={}&canonical=1",
                    encode_title(&title)
                );
                run(dev, &format!("settings put system {key} {}", shell_quote(&uri)))?;
                if run(dev, &format!("settings get system {key}"))?.trim() == uri {
                    applied.push(format!("{label}({title})"));
                } else {
                    not_applied.push(format!("{label}({title})"));
                }
            }
            None => not_applied.push(format!("{label}({title} — 이 폰에서 같은 소리를 찾지 못함)")),
        }
    }

    // 3) 키보드 — 사용 목록에 켠 뒤 기본으로 지정(기본만 바꾸면 시스템이 되돌린다, 2026-10-08 실측)
    if let Some(default) = dumps["secure"].get("default_input_method").filter(|v| valid_component(v)) {
        let current = run(dev, "settings get secure default_input_method")?;
        let pkg = default.split('/').next().unwrap_or_default();
        if current.trim() != default {
            if !installed.contains(pkg) {
                not_applied.push(format!("기본 키보드({pkg} — 앱이 설치되지 않음)"));
            } else {
                let enabled = dumps["secure"].get("enabled_input_methods").cloned().unwrap_or_default();
                for id in enabled.split(':').filter_map(|e| e.split(';').next()) {
                    if valid_component(id) && installed.contains(id.split('/').next().unwrap_or_default()) {
                        run(dev, &format!("ime enable {id}"))?;
                    }
                }
                run(dev, &format!("ime enable {default}"))?;
                run(dev, &format!("ime set {default}"))?;
                if run(dev, "settings get secure default_input_method")?.trim() == default {
                    applied.push("기본 키보드".into());
                } else {
                    not_applied.push("기본 키보드".into());
                }
            }
        }
    }

    // 4) 다크 모드 — 설정 키는 재부팅 후에야 반영되므로 uimode로 바로 적용(0 자동, 1 끔, 2 켬)
    if let Some(mode) = dumps["secure"].get("ui_night_mode") {
        let want = match mode.as_str() {
            "0" => Some(("auto", "auto")),
            "1" => Some(("no", "no")),
            "2" => Some(("yes", "yes")),
            _ => None,
        };
        if let Some((arg, shown)) = want {
            run(dev, &format!("cmd uimode night {arg}"))?;
            if run(dev, "cmd uimode night")?.contains(&format!("Night mode: {shown}")) {
                applied.push("다크 모드".into());
            } else {
                not_applied.push("다크 모드".into());
            }
        }
    }

    // 5) 알림 접근·접근성 서비스 — 사용자가 허용했던 앱 중 설치된 것만
    if let Some(listeners) = dumps["secure"].get("enabled_notification_listeners") {
        for comp in listeners.split(':').filter(|c| valid_component(c)) {
            if !installed.contains(comp.split('/').next().unwrap_or_default()) {
                continue;
            }
            run(dev, &format!("cmd notification allow_listener {}", shell_quote(comp)))?;
            if run(dev, "settings get secure enabled_notification_listeners")?.contains(comp) {
                applied.push(format!("알림 접근({})", comp.split('/').next().unwrap_or_default()));
            } else {
                not_applied.push(format!("알림 접근({comp})"));
            }
        }
    }
    if let Some(services) = dumps["secure"].get("enabled_accessibility_services") {
        let keep: Vec<&str> = services
            .split(':')
            .filter(|c| valid_component(c) && installed.contains(c.split('/').next().unwrap_or_default()))
            .collect();
        if !keep.is_empty() {
            let value = keep.join(":");
            run(dev, &format!("settings put secure enabled_accessibility_services {}", shell_quote(&value)))?;
            run(dev, "settings put secure accessibility_enabled 1")?;
            if run(dev, "settings get secure enabled_accessibility_services")?.trim() == value {
                applied.push(format!("접근성 서비스 {}개", keep.len()));
            } else {
                not_applied.push("접근성 서비스".into());
            }
        }
    }

    // 6) 기본 앱(역할)
    match roles {
        None => log.push("[안내] 이 백업에는 기본 앱 기록이 없습니다 — 설정 → 앱 → 기본 앱에서 직접 지정해 주세요".into()),
        Some(raw) => {
            let wanted = parse_kv(&raw);
            for (role, label) in ROLES {
                let Some(pkg) = wanted.get(*role).and_then(|v| v.split(';').next()).filter(|p| super::sms_role::valid_package(p)) else {
                    continue;
                };
                if run(dev, &format!("cmd role get-role-holders {role}"))?.trim() == pkg {
                    continue;
                }
                if !installed.contains(pkg) {
                    not_applied.push(format!("{label}({pkg} — 앱이 설치되지 않음)"));
                    continue;
                }
                run(dev, &format!("cmd role add-role-holder {role} {pkg}"))?;
                if run(dev, &format!("cmd role get-role-holders {role}"))?.split(';').any(|h| h.trim() == pkg) {
                    applied.push(format!("{label}({pkg})"));
                } else {
                    not_applied.push(format!("{label}({pkg})"));
                }
            }
        }
    }

    // 7) 앱 런타임 권한 — 설치된 앱만. 바꿀 수 없는 권한은 건너뛴다(시스템 고정 등)
    match permissions {
        None => log.push("[안내] 이 백업에는 앱 권한 기록이 없습니다 — 앱을 처음 열 때 다시 허용해 주세요".into()),
        Some(raw) => {
            let grants: Vec<(String, String)> = raw
                .lines()
                .filter_map(|l| l.trim().split_once(' '))
                .filter(|(p, perm)| {
                    installed.contains(*p)
                        && super::sms_role::valid_package(p)
                        && perm.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_'))
                })
                .map(|(p, perm)| (p.to_string(), perm.to_string()))
                .collect();
            let mut failed = 0usize;
            for chunk in grants.chunks(80) {
                let script = chunk
                    .iter()
                    .map(|(p, perm)| format!("pm grant {p} {perm} >/dev/null 2>&1 || echo FAIL"))
                    .collect::<Vec<_>>()
                    .join("; ");
                failed += run(dev, &script)?.lines().filter(|l| l.trim() == "FAIL").count();
            }
            if !grants.is_empty() {
                log.push(format!(
                    "앱 권한 {}개 다시 허용{}",
                    grants.len() - failed,
                    if failed > 0 { format!(" (바꿀 수 없는 권한 {failed}개 건너뜀)") } else { String::new() }
                ));
            }
        }
    }

    // 8) 특수 접근·배터리 제한(appops)
    if let Some(raw) = appops {
        let mut set = 0usize;
        let mut cmds = vec![];
        for line in raw.lines() {
            // "<패키지> <OP>: <mode>; ..." — 사용자가 바꾼 값만(default는 그대로 둔다)
            let Some((pkg, rest)) = line.trim().split_once(' ') else { continue };
            let Some((op, mode)) = rest.split_once(':') else { continue };
            let mode = mode.trim().split(';').next().unwrap_or("").trim();
            if !installed.contains(pkg)
                || !super::sms_role::valid_package(pkg)
                || !APP_OPS.contains(&op.trim())
                || !matches!(mode, "allow" | "ignore" | "deny" | "foreground")
            {
                continue;
            }
            cmds.push(format!("cmd appops set {pkg} {} {mode}", op.trim()));
            set += 1;
        }
        for chunk in cmds.chunks(80) {
            run(dev, &chunk.join("; "))?;
        }
        if set > 0 {
            log.push(format!("특수 접근·배터리 제한 {set}건 적용"));
        }
    }

    if !applied.is_empty() {
        log.push(format!("설정 복원 {}개: {}", applied.len(), applied.join(", ")));
    }
    if !not_applied.is_empty() {
        log.push(format!("[확인 필요] 반영되지 않은 설정: {}", not_applied.join(", ")));
    }
    log.push(MANUAL_KEYS_NOTE.into());
    Ok(log)
}

/// `content://media/<볼륨>/audio/media/<id>?title=<제목>&…` → (볼륨, 제목)
fn parse_media_uri(value: &str) -> Option<(String, String)> {
    let rest = value.strip_prefix("content://media/")?;
    let (volume, rest) = rest.split_once("/audio/media/")?;
    let title = rest.split_once("title=")?.1.split('&').next()?;
    if !matches!(volume, "internal" | "external" | "external_primary") || title.is_empty() {
        return None;
    }
    Some((volume.to_string(), decode_title(title)))
}

fn decode_title(t: &str) -> String {
    let bytes = t.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3]).ok();
            if let Some(b) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(if bytes[i] == b'+' { b' ' } else { bytes[i] });
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn encode_title(t: &str) -> String {
    t.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// 같은 제목·종류의 소리 id — 정확히 하나일 때만
fn find_sound(dev: &mut dyn ADBDeviceExt, volume: &str, kind: &str, title: &str) -> Result<Option<String>, String> {
    if title.contains(['\'', '"', '\\', '$', '`']) {
        return Ok(None);
    }
    let out = run(
        dev,
        &format!(
            "content query --uri content://media/{volume}/audio/media --projection _id --where \"title='{title}' AND {kind}=1\""
        ),
    )?;
    let ids: Vec<String> = out
        .lines()
        .filter_map(|l| l.split("_id=").nth(1))
        .map(|r| r.chars().take_while(char::is_ascii_digit).collect::<String>())
        .filter(|id| !id.is_empty())
        .collect();
    Ok(if ids.len() == 1 { ids.into_iter().next() } else { None })
}

/// `dumpsys deviceidle whitelist` 덤프 → 사용자가 지정한 예외 패키지.
/// 실제 출력은 `<종류>,<패키지>,<uid>` 줄이다(system-excidle·system·user). 시스템 항목은 이미
/// 시스템 예외이므로 사용자 지정(`user,`)만 고른다 — adb.rs 설정 개요의 개수 세기와 같은 기준.
fn user_whitelist(raw: &str) -> Vec<String> {
    let mut packages: Vec<String> = Vec::new();
    for line in raw.lines() {
        let mut fields = line.trim().split(',');
        if fields.next() != Some("user") {
            continue;
        }
        let pkg = fields.next().unwrap_or("").trim();
        // 백업 파일에서 읽은 값이 셸 명령에 들어가므로 패키지 이름 형식만 허용
        if super::sms_role::valid_package(pkg) && !packages.iter().any(|p| p == pkg) {
            packages.push(pkg.to_string());
        }
    }
    packages
}

/// deviceidle whitelist 재적용 — 덤프의 사용자 지정 예외만 추가 (recovery.md 1-2)
pub fn restore_deviceidle(
    dev: &mut dyn ADBDeviceExt,
    backup_root: &Path,
) -> Result<Vec<String>, String> {
    let path = super::paths::existing_file(backup_root, "settings/deviceidle_whitelist.txt")?;
    let raw =
        std::fs::read_to_string(path).map_err(|e| format!("whitelist 덤프 읽기 실패: {e}"))?;
    let packages = user_whitelist(&raw);
    for pkg in &packages {
        let cmd = format!("dumpsys deviceidle whitelist +{pkg}");
        run(dev, &cmd).map_err(|e| format!("{pkg} 예외 복원 실패: {e}"))?;
    }
    Ok(packages)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backup::fake_device::FakeADBDevice;
    use crate::backup::model::ItemStatus;

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

    fn backup_with(files: &[(&str, &str)]) -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("settings")).unwrap();
        for ns in ["secure", "system", "global"] {
            std::fs::write(root.path().join(format!("settings/settings_{ns}.txt")), "").unwrap();
        }
        for (name, body) in files {
            std::fs::write(root.path().join(format!("settings/{name}")), body).unwrap();
        }
        root
    }

    fn phone(installed: &str) -> FakeADBDevice {
        let mut d = FakeADBDevice::new();
        d.settings = Some(Default::default());
        d.answer_shell("pm list packages", installed);
        d.answer_shell("ime enable", "");
        d
    }

    #[test]
    fn changed_user_settings_are_written_read_back_and_unapplied_ones_reported() {
        let root = backup_with(&[
            ("settings_system.txt", "screen_brightness=31\naccelerometer_rotation=1\nfont_scale=1.0\n"),
            ("settings_secure.txt", "sysui_qs_tiles=old\nsysui_qs_tiles=new=with=equals\nandroid_id=abc\n"),
        ]);
        let mut d = phone("");
        d.settings.as_mut().unwrap().insert("system font_scale".into(), "1.0".into());
        // 시스템이 되돌리는 설정은 성공으로 보고하지 않는다
        d.settings_ignore.insert("system accelerometer_rotation".into());
        let log = restore_settings(&mut d, root.path()).unwrap();
        let store = d.settings.as_ref().unwrap();
        assert_eq!(store["system screen_brightness"], "31");
        // 마지막 값·값 안의 '=' 유지
        assert_eq!(store["secure sysui_qs_tiles"], "new=with=equals");
        // 기기 고유값은 옮기지 않고, 이미 같은 값은 쓰지 않는다
        assert!(!store.contains_key("secure android_id"));
        assert!(!d.shell_calls.iter().any(|c| c.starts_with("settings put system font_scale")));
        assert!(log.iter().any(|l| l.contains("화면 밝기") && l.starts_with("설정 복원")));
        assert!(log.iter().any(|l| l.starts_with("[확인 필요]") && l.contains("자동 회전")));
        // 예전 백업 — 기본 앱·권한 기록 없음 안내
        assert!(log.iter().any(|l| l.contains("기본 앱 기록이 없습니다")));
    }

    #[test]
    fn keyboard_is_enabled_before_it_is_set_and_sounds_are_matched_by_title() {
        let root = backup_with(&[
            (
                "settings_secure.txt",
                "default_input_method=com.est.kb/com.est.Svc\nenabled_input_methods=com.est.kb/com.est.Svc;123:com.google.android.inputmethod.latin/com.android.inputmethod.latin.LatinIME\n",
            ),
            ("settings_system.txt", "ringtone=content://media/internal/audio/media/154?title=Xperia&canonical=1\n"),
        ]);
        let mut d = phone("package:com.est.kb\npackage:com.google.android.inputmethod.latin\n");
        d.answer_shell("content query --uri content://media/internal/audio/media", "Row: 0 _id=123\n");
        let log = restore_settings(&mut d, root.path()).unwrap();
        let enable = d.shell_calls.iter().position(|c| c == "ime enable com.est.kb/com.est.Svc").unwrap();
        let set = d.shell_calls.iter().position(|c| c == "ime set com.est.kb/com.est.Svc").unwrap();
        assert!(enable < set);
        let store = d.settings.as_ref().unwrap();
        assert_eq!(store["secure default_input_method"], "com.est.kb/com.est.Svc");
        assert_eq!(store["system ringtone"], "content://media/internal/audio/media/123?title=Xperia&canonical=1");
        assert!(d.shell_calls.iter().any(|c| c.contains("title='Xperia' AND is_ringtone=1")));
        assert!(log.iter().any(|l| l.contains("기본 키보드") && l.contains("벨소리(Xperia)")));
    }

    #[test]
    fn roles_permissions_and_appops_apply_only_to_installed_apps() {
        let root = backup_with(&[
            ("roles.txt", "android.app.role.SMS=com.example.sms\nandroid.app.role.HOME=com.missing.home\n"),
            (
                "permissions.txt",
                "com.example.sms android.permission.READ_SMS\ncom.missing.home android.permission.CAMERA\nbad;rm android.permission.X\n",
            ),
            ("appops.txt", "com.example.sms RUN_ANY_IN_BACKGROUND: ignore; time=+1d\ncom.example.sms CAMERA: allow\n"),
        ]);
        let mut d = phone("package:com.example.sms\n");
        d.sms_role_holder = Some(Some("com.google.android.apps.messaging".into()));
        d.answer_shell("cmd role get-role-holders android.app.role.HOME", "com.sonymobile.launcher");
        d.answer_shell("pm grant", "");
        d.answer_shell("cmd appops set", "");
        let log = restore_settings(&mut d, root.path()).unwrap();
        assert_eq!(d.sms_role_holder, Some(Some("com.example.sms".into())));
        assert!(log.iter().any(|l| l.contains("기본 문자 앱(com.example.sms)")));
        assert!(log.iter().any(|l| l.starts_with("[확인 필요]") && l.contains("com.missing.home — 앱이 설치되지 않음")));
        let grants: Vec<_> = d.shell_calls.iter().filter(|c| c.contains("pm grant")).collect();
        assert_eq!(grants.len(), 1);
        assert!(grants[0].contains("pm grant com.example.sms android.permission.READ_SMS"));
        assert!(!grants[0].contains("CAMERA") && !grants[0].contains("bad;rm"));
        let ops: Vec<_> = d.shell_calls.iter().filter(|c| c.contains("cmd appops set")).collect();
        assert_eq!(ops.len(), 1);
        assert!(ops[0].contains("cmd appops set com.example.sms RUN_ANY_IN_BACKGROUND ignore"));
        assert!(!ops[0].contains("CAMERA"), "기록 대상이 아닌 op는 쓰지 않는다");
    }

    #[test]
    fn media_uri_titles_round_trip() {
        assert_eq!(
            parse_media_uri("content://media/internal/audio/media/154?title=Xperia&canonical=1"),
            Some(("internal".into(), "Xperia".into()))
        );
        assert_eq!(
            parse_media_uri("content://media/external/audio/media/9?title=My%20Song&canonical=1").unwrap().1,
            "My Song"
        );
        assert_eq!(encode_title("My Song"), "My%20Song");
        assert!(parse_media_uri("null").is_none());
    }

    #[test]
    fn shell_quote_escapes() {
        assert_eq!(shell_quote("internet,bt"), "\"internet,bt\"");
        assert_eq!(shell_quote("a\"b$c`d"), "\"a\\\"b\\$c\\`d\"");
    }

    #[test]
    fn collect_records_roles_permissions_and_appops_and_deviceidle_restores_user_entries() {
        let mut d = FakeADBDevice::new();
        d.answer_shell("settings list system", "screen_brightness=31\n");
        d.answer_shell("settings list global", "stay_on_while_plugged_in=7\n");
        d.answer_shell("settings list secure", "sysui_qs_tiles=internet,bt,rotation\n");
        // 실기기 출력 형식 — <종류>,<패키지>,<uid>
        d.answer_shell(
            "dumpsys deviceidle whitelist",
            "system-excidle,com.android.systemui,10123\nsystem,com.google.android.gms,10089\nuser,com.kakao.talk,10234\nuser,com.friendscube.somoim,10301\nuser,bad;rm -rf,10302\n",
        );
        d.answer_shell(
            "pm list packages -3 -f",
            "package:/data/app/~~abc/com.kakao.talk-XYZ/base.apk=com.kakao.talk\n",
        );
        d.answer_shell("echo \"android.app.role.SMS=", "android.app.role.SMS=com.google.android.apps.messaging\n");
        d.answer_shell(
            "for p in $(pm list packages -3 | cut -d: -f2); do dumpsys",
            "com.kakao.talk android.permission.CAMERA\n",
        );
        d.answer_shell(
            "for p in $(pm list packages -3 | cut -d: -f2); do cmd appops",
            "com.kakao.talk RUN_ANY_IN_BACKGROUND: allow\n",
        );
        d.answer_shell("dumpsys deviceidle whitelist +", "");
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_settings(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Done, "{:?}", rec.errors);
        assert_eq!(rec.artifacts.len(), 9);
        for name in ["roles.txt", "permissions.txt", "appops.txt", "installers.txt"] {
            assert!(rec.artifacts.contains(&format!("settings/{name}")), "{name}");
        }
        // deviceidle — 사용자 지정(kakao, friendscube)만, 시스템 항목·잘못된 이름 제외
        let applied = restore_deviceidle(&mut d, tmp.path()).unwrap();
        assert!(d
            .shell_calls
            .iter()
            .any(|c| c == "dumpsys deviceidle whitelist +com.kakao.talk"));
        assert_eq!(
            applied,
            vec!["com.kakao.talk".to_string(), "com.friendscube.somoim".to_string()]
        );
    }

    #[test]
    fn dump_error_makes_partial() {
        let mut d = FakeADBDevice::new(); // 응답 없음 → 전부 실패
        d.fail_shell.insert("echo \"android.app.role.".into());
        d.fail_shell.insert("for p in ".into());
        d.fail_shell.insert("pm list packages -3 -i".into());
        let tmp = tempfile::tempdir().unwrap();
        let rec = collect_settings(&mut d, tmp.path());
        assert_eq!(rec.status, ItemStatus::Partial);
        assert_eq!(rec.errors.len(), 9);
    }
}
