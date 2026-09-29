//! 읽기 전용 기기 질의 — `adb_client` 크레이트로 ADB 프로토콜 직접 통신.
//! - 1순위: 실행 중인 adb 서버(localhost:5037)에 연결 — Android Studio 등이 띄운 서버 재사용
//! - 2순위: USB 직접 연결 — 서버가 없을 때 크레이트가 프로토콜을 직접 구현해 통신
//! adb 바이너리 설치/경로 탐색이 필요 없다.
//! 규칙: 기기·PC에 영향을 주는(쓰기·설치·삭제·플래시) 명령은 이 모듈에 작성 금지.

use adb_client::server::{ADBServer, DeviceState};
use adb_client::server_device::ADBServerDevice;
use adb_client::usb::{find_all_connected_adb_devices, ADBUSBDevice};
use adb_client::ADBDeviceExt;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, TryLockError};
use std::time::{Duration, Instant};

// ── 연결 관리 ──

fn adb_server() -> ADBServer {
    ADBServer::new(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5037))
}

/// USB 직접 연결 (재사용 — 매번 핸드셰이크하지 않도록 보관)
static USB_CONN: Mutex<Option<ADBUSBDevice>> = Mutex::new(None);

/// 데드락 방지: 다른 작업이 점유 중이면 기다렸다가 한도 초과 시 에러
fn lock_usb(wait: Duration) -> Result<MutexGuard<'static, Option<ADBUSBDevice>>, String> {
    let start = Instant::now();
    loop {
        match USB_CONN.try_lock() {
            Ok(g) => return Ok(g),
            Err(TryLockError::Poisoned(p)) => return Ok(p.into_inner()),
            Err(TryLockError::WouldBlock) => {
                if start.elapsed() >= wait {
                    return Err("기기 연결이 다른 작업을 처리하는 중입니다".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    }
}

fn open_usb() -> Result<ADBUSBDevice, String> {
    let devices =
        find_all_connected_adb_devices().map_err(|e| format!("USB 기기 검색 실패: {e}"))?;
    if devices.is_empty() {
        return Err("연결된 기기 없음".into());
    }
    // 여러 대면 첫 번째 (동일 모델 중복 연결의 구분은 서버 모드에서만 가능 — 한계)
    let info = &devices[0];
    ADBUSBDevice::new(info.vendor_id, info.product_id).map_err(|e| format!("기기 연결 실패: {e}"))
}

fn ensure_usb(guard: &mut MutexGuard<'static, Option<ADBUSBDevice>>) -> Result<(), String> {
    if guard.is_none() {
        **guard = Some(open_usb()?);
    }
    Ok(())
}

fn is_ready_state(state: &DeviceState) -> bool {
    // adb 서버의 "device" 상태 = 통신 가능한 준비 상태
    state.to_string().eq_ignore_ascii_case("device")
}

/// 실행 중인 adb 서버에 연결된 (식별자, 기기 핸들) 목록.
/// - 서버 미실행 → None (USB 직접 연결로 폴백)
/// - 서버 실행 중 + 준비 기기 없음 → Some(빈 목록) (서버가 기기를 점유 중이므로 USB 폴백 금지)
fn server_devices() -> Option<Vec<(String, ADBServerDevice)>> {
    let mut server = adb_server();
    let list = match server.devices() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rust] adb 서버 미연결(USB 직접 연결로 전환): {e}");
            return None;
        }
    };
    let summary: Vec<String> = list
        .iter()
        .map(|d| format!("{}={}", mask_serial(&d.identifier), d.state))
        .collect();
    eprintln!("[rust] adb 서버 기기: {}", summary.join(", "));

    let ready: Vec<String> = list
        .iter()
        .filter(|d| is_ready_state(&d.state))
        .map(|d| d.identifier.clone())
        .collect();
    Some(
        ready
            .into_iter()
            .filter_map(|serial| match server.get_device_by_name(serial.as_str()) {
                Ok(dev) => Some((serial, dev)),
                Err(e) => {
                    eprintln!("[rust] 서버 기기 핸들 취득 실패: {e}");
                    None
                }
            })
            .collect(),
    )
}

// ── 타임아웃 보장 실행 ──

/// `work`를 별도 스레드에서 실행하고 `limit` 안에 끝나지 않으면 시간 초과로 반환.
/// (기기 응답이 멈춰도 UI/IPC가 죽지 않도록)
async fn guarded<T: Send + 'static>(
    limit: Duration,
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(work());
    });
    let blocking = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(limit)
            .unwrap_or_else(|_| Err("기기 응답 시간 초과".into()))
    });
    match blocking.await {
        Ok(r) => r,
        Err(e) => Err(format!("작업 스레드 오류: {e}")),
    }
}

// ── 셸 실행 ──

fn shell(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    let mut out = Vec::new();
    let mut err = Vec::new();
    dev.shell_command(&cmd, Some(&mut out), Some(&mut err))
        .map_err(|e| {
            let e = e.to_string();
            if err.is_empty() {
                e
            } else {
                format!("{e}: {}", String::from_utf8_lossy(&err).trim())
            }
        })?;
    String::from_utf8(out).map_err(|e| format!("출력 해석 실패: {e}"))
}

/// 서버 모드 우선, 없으면 USB 직접 연결로 `f` 실행 (오류 시 연결 재수립 1회)
fn with_first_device<T>(
    serial: &Option<String>,
    mut f: impl FnMut(&mut dyn ADBDeviceExt) -> Result<T, String>,
) -> Result<T, String> {
    // 1) adb 서버 모드
    if let Some(mut devs) = server_devices() {
        let idx = serial
            .as_deref()
            .filter(|s| !s.is_empty())
            .and_then(|s| devs.iter().position(|(id, _)| id == s))
            .unwrap_or(0);
        let (_, dev) = devs.get_mut(idx).ok_or("연결된 기기가 없습니다")?;
        return f(dev);
    }

    // 2) USB 직접 연결
    let found = find_all_connected_adb_devices().map_err(|e| format!("USB 검색 실패: {e}"))?;
    if found.is_empty() {
        return Err("연결된 기기가 없습니다".into());
    }
    let mut guard = lock_usb(Duration::from_secs(5))?;
    ensure_usb(&mut guard)?;
    let first = f(guard.as_mut().expect("연결 보장됨"));
    if first.is_err() {
        // 커넥션이 끊겼을 수 있으므로 버리고 재연결 후 1회 재시도
        *guard = None;
        ensure_usb(&mut guard)?;
        return f(guard.as_mut().expect("재연결 보장됨"));
    }
    first
}

// ── adb 설치/연결 상태 ──

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdbStatus {
    pub available: bool,
    /// "adb-server" | "usb-direct" | "none"
    pub mode: String,
    pub detail: Option<String>,
}

fn status_work() -> AdbStatus {
    // 서버 모드: 서버 도달 가능하면 그대로 사용
    if let Some(devs) = server_devices() {
        return AdbStatus {
            available: true,
            mode: "adb-server".into(),
            detail: Some(format!("준비된 기기 {}대", devs.len())),
        };
    }
    // USB 직접 연결 수단 점검
    match find_all_connected_adb_devices() {
        Ok(list) if !list.is_empty() => AdbStatus {
            available: true,
            mode: "usb-direct".into(),
            detail: Some(format!("USB 기기 {}대 감지", list.len())),
        },
        Ok(_) => AdbStatus {
            available: true,
            mode: "none".into(),
            detail: Some("연결된 기기 없음".into()),
        },
        Err(e) => AdbStatus {
            available: false,
            mode: "none".into(),
            detail: Some(format!("USB 접근 불가: {e}")),
        },
    }
}

/// 기기 연결 상태 — 없으면 프론트에서 재시도 팝업
#[tauri::command]
pub async fn adb_status() -> Result<AdbStatus, String> {
    guarded(Duration::from_secs(10), || Ok(status_work())).await
}

// ── 기기 상태 ──

fn mask_serial(serial: &str) -> String {
    if serial.len() > 4 {
        format!("{}****", &serial[..4])
    } else {
        serial.to_string()
    }
}

/// 모델명 → 제품명 (devices.json 이전 예정)
fn product_name(model: &str) -> String {
    match model {
        "XQ-DQ44" | "XQ-DQ72" | "XQ-DQ54" => "Xperia 1 V".into(),
        "XQ-DE44" | "XQ-DE54" => "Xperia 5 V".into(),
        "XQ-DX72" => "Xperia 1 VI".into(),
        "XQ-EC72" => "Xperia 10 VI".into(),
        _ => model.to_string(),
    }
}

/// `getprop` 덤프를 key→value 맵으로
fn parse_getprop(out: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in out.lines() {
        // 형식: [key]: [value]
        let Some(rest) = line.strip_prefix('[') else { continue };
        let Some(mid) = rest.find("]: ") else { continue };
        let key = &rest[..mid];
        let val = rest[mid + 3..].trim_start_matches('[');
        let val = val.strip_suffix(']').unwrap_or(val);
        map.insert(key.to_string(), val.to_string());
    }
    map
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SimOut {
    slot: u8,
    #[serde(rename = "type")]
    sim_type: &'static str,
    carrier: Option<String>,
    volte_enabled: bool,
    #[serde(rename = "_plmn")]
    plmn: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UsbOut {
    topology: String,
    controller: String,
    link_speed: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceOut {
    serial: String,
    serial_masked: String,
    model: String,
    product_name: String,
    firmware: String,
    android: String,
    mode: String,
    bootloader: String,
    rooted: bool,
    sims: Vec<SimOut>,
    usb: UsbOut,
}

/// 기기 상태 조회 (getprop + which su — 모두 읽기 전용)
fn device_status(dev: &mut dyn ADBDeviceExt, serial_hint: &str) -> Result<DeviceOut, String> {
    let raw = shell(dev, "getprop; echo __SU__; which su || true")?;
    let (props_raw, su_raw) = raw.split_once("__SU__").unwrap_or((&raw, ""));
    let p = parse_getprop(props_raw);
    let get = |k: &str| p.get(k).cloned().unwrap_or_default();

    let mut serial = get("ro.serialno");
    if serial.is_empty() {
        serial = get("ro.boot.serialno");
    }
    if serial.is_empty() {
        serial = serial_hint.to_string();
    }
    if serial.is_empty() {
        serial = "unknown".into();
    }

    let bootloader = match get("ro.boot.flash.locked").as_str() {
        "1" => "locked",
        "0" => "unlocked",
        _ => "unknown",
    };
    let rooted = !su_raw.trim().is_empty();

    let sim_state = get("gsm.sim.state");
    let states: Vec<&str> = sim_state.split(',').collect();
    let alphas = [get("gsm.sim.operator.alpha"), get("gsm.sim.operator.alpha.2")];
    let numerics = [get("gsm.sim.operator.numeric"), get("gsm.sim.operator.numeric.2")];
    let volte_avail = get("persist.dbg.volte_avail_ovr") == "1";

    let sims = (1..=2u8)
        .map(|slot| {
            let idx = (slot - 1) as usize;
            let loaded = states.get(idx).map(|s| s.trim()) == Some("LOADED");
            SimOut {
                slot,
                sim_type: if slot == 1 { "physical" } else { "esim" },
                carrier: loaded.then(|| alphas[idx].clone()),
                volte_enabled: volte_avail && loaded,
                plmn: numerics[idx].clone(),
            }
        })
        .collect();

    let model = get("ro.product.model");
    let serial_masked = mask_serial(&serial);
    Ok(DeviceOut {
        serial,
        serial_masked,
        product_name: product_name(&model),
        model,
        firmware: get("ro.build.display.id"),
        android: get("ro.build.version.release"),
        mode: "android".into(),
        bootloader: bootloader.into(),
        rooted,
        sims,
        usb: UsbOut {
            topology: String::new(),
            controller: String::new(),
            link_speed: String::new(),
        },
    })
}

fn device_list_work() -> Result<Vec<DeviceOut>, String> {
    // 1) adb 서버 모드 — 준비된 기기 전부
    if let Some(devs) = server_devices() {
        let mut out = Vec::with_capacity(devs.len());
        for (serial, mut dev) in devs {
            match device_status(&mut dev, &serial) {
                Ok(d) => out.push(d),
                Err(e) => eprintln!(
                    "[rust] 기기 정보 읽기 실패({}): {e}",
                    mask_serial(&serial)
                ),
            }
        }
        eprintln!("[rust] device_list(server) -> {} device(s)", out.len());
        return Ok(out);
    }

    // 2) USB 직접 연결 — 기기 감지부터
    let found = find_all_connected_adb_devices().map_err(|e| format!("USB 검색 실패: {e}"))?;
    if found.is_empty() {
        eprintln!("[rust] device_list(usb) -> 0 device(s)");
        return Ok(vec![]);
    }
    let mut guard = lock_usb(Duration::from_secs(5))?;
    ensure_usb(&mut guard)?;
    let first = device_status(guard.as_mut().expect("연결 보장됨"), "");
    let result = match first {
        Ok(d) => Ok(vec![d]),
        Err(e) => {
            // 커넥션 오류 가능성 → 재연결 후 1회 재시도
            eprintln!("[rust] USB 기기 상태 읽기 실패, 재연결 시도: {e}");
            *guard = None;
            ensure_usb(&mut guard)?;
            let d = device_status(guard.as_mut().expect("재연결 보장됨"), "")?;
            Ok(vec![d])
        }
    };
    if let Ok(list) = &result {
        eprintln!("[rust] device_list(usb) -> {} device(s)", list.len());
    }
    result
}

/// 연결된 기기의 상태 목록 (읽기 전용)
#[tauri::command]
pub async fn device_list() -> Result<Vec<DeviceOut>, String> {
    guarded(Duration::from_secs(30), || device_list_work()).await
}

// ── 백업 경로별 용량 ──

const FOLDER_PATHS: [(&str, &str); 9] = [
    ("dcim", "/sdcard/DCIM"),
    ("download", "/sdcard/Download"),
    ("pictures", "/sdcard/Pictures"),
    ("movies", "/sdcard/Movies"),
    ("music", "/sdcard/Music"),
    ("documents", "/sdcard/Documents"),
    ("recordings", "/sdcard/Recordings"),
    ("android-data", "/sdcard/Android/data"),
    // /sdcard는 심볼릭 링크라 du가 0을 반환 — 실제 경로 사용 (UI엔 경로 미표시)
    ("sdcard-total", "/storage/emulated/0"),
];

/// 한 번의 셸 실행으로 측정: 폴더별 du(병렬) → 3자 앱 APK 크기 → 여유 공간
fn build_storage_script() -> String {
    let mut s = String::from("(");
    for (i, (_, path)) in FOLDER_PATHS.iter().enumerate() {
        if i > 0 {
            s.push(' ');
        }
        s.push_str(&format!("du -sk {path} &"));
    }
    s.push_str(
        " wait); echo __APK__; \
         for p in $(pm list packages -3 | cut -d: -f2); do \
         a=$(pm path $p | cut -d: -f2 | head -n1); \
         if [ -n \"$a\" ]; then stat -c %s \"$a\"; fi; done; \
         echo __DF__; df -k /sdcard",
    );
    s
}

fn parse_storage_output(raw: &str) -> Value {
    let mut sizes: HashMap<String, u64> =
        FOLDER_PATHS.iter().map(|(id, _)| (id.to_string(), 0u64)).collect();
    let mut section = 0u8; // 0=du, 1=apk, 2=df
    let mut apk_sum = 0u64;
    let mut apk_n = 0u64;
    let mut free_kb = 0u64;

    for line in raw.lines() {
        let t = line.trim();
        match t {
            "__APK__" => {
                section = 1;
                continue;
            }
            "__DF__" => {
                section = 2;
                continue;
            }
            _ => {}
        }
        match section {
            0 => {
                // "1234\t/sdcard/DCIM"
                let mut it = t.split_whitespace();
                if let (Some(kb), Some(path)) = (it.next(), it.next()) {
                    if let (Ok(kb), Some(id)) = (
                        kb.parse::<u64>(),
                        FOLDER_PATHS.iter().find(|(_, p)| *p == path).map(|(id, _)| *id),
                    ) {
                        sizes.insert(id.to_string(), kb * 1024);
                    }
                }
            }
            1 => {
                if let Ok(b) = t.parse::<u64>() {
                    apk_sum += b;
                    apk_n += 1;
                }
            }
            2 => {
                if !t.starts_with("Filesystem") && !t.is_empty() {
                    let parts: Vec<&str> = t.split_whitespace().collect();
                    if parts.len() >= 4 {
                        if let Ok(kb) = parts[3].parse::<u64>() {
                            free_kb = kb;
                        }
                    }
                }
            }
            _ => {}
        }
    }

    let mut out = serde_json::Map::new();
    for (k, v) in sizes {
        out.insert(k, json!(v));
    }
    out.insert("apk-total".into(), json!(apk_sum));
    out.insert("apk-count".into(), json!(apk_n));
    out.insert("apk-sampled".into(), json!(apk_n));
    out.insert("sdcard-free".into(), json!(free_kb * 1024));
    eprintln!(
        "[rust] storage_sizes -> {} keys, apk {} bytes ({} pkgs)",
        out.len(),
        apk_sum,
        apk_n
    );
    Value::Object(out)
}

fn storage_sizes_work(serial: Option<String>) -> Result<Value, String> {
    let script = build_storage_script();
    let raw = with_first_device(&serial, |dev| shell(dev, &script))?;
    Ok(parse_storage_output(&raw))
}

/// 백업 경로별 실제 용량 (모두 읽기 전용)
#[tauri::command]
pub async fn storage_sizes(serial: Option<String>) -> Result<Value, String> {
    guarded(Duration::from_secs(180), move || {
        storage_sizes_work(serial)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 실기기(XQ-DQ44)에서 캡처한 출력 기반 — 섹션 마커/병렬 du/권한 오류 혼재/df 파싱 검증
    #[test]
    fn parse_storage_output_real_sample() {
        let raw = "\
136533361\t/sdcard/DCIM\n\
326551\t/sdcard/Download\n\
8\t/sdcard/Recordings\n\
0\t/sdcard/Android/data-not-there\n\
du: /sdcard/Android/data/x: Permission denied\n\
42064776\t/sdcard/Android/data\n\
180041842\t/storage/emulated/0\n\
__APK__\n\
68440742\n\
187393515\n\
__DF__\n\
Filesystem     1K-blocks      Used Available Use% Mounted on\n\
/dev/fuse      476013204 244145624 231736508  52% /storage/emulated\n";
        let v = parse_storage_output(raw);
        let get = |k: &str| v[k].as_u64().unwrap();

        assert_eq!(get("dcim"), 136533361 * 1024);
        assert_eq!(get("download"), 326551 * 1024);
        assert_eq!(get("recordings"), 8 * 1024);
        assert_eq!(get("android-data"), 42064776 * 1024);
        assert_eq!(get("sdcard-total"), 180041842 * 1024);
        // 폴더 목록에 없는 경로·권한 오류 줄은 무시, 미측정 키는 0
        assert_eq!(get("music"), 0);
        assert_eq!(get("sdcard-free"), 231736508 * 1024);
        assert_eq!(get("apk-total"), 68440742 + 187393515);
        assert_eq!(get("apk-count"), 2);
        assert_eq!(get("apk-sampled"), 2);
    }
}
