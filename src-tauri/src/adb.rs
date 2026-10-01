//! 읽기 전용 기기 질의 — `adb_client` 크레이트로 ADB 프로토콜 직접 통신.
//! - 1순위: 실행 중인 adb 서버(localhost:5037)에 연결 — Android Studio 등이 띄운 서버 재사용
//! - 2순위: USB 직접 연결 — 서버가 없을 때 크레이트가 프로토콜을 직접 구현해 통신
//! adb 바이너리 설치/경로 탐색이 필요 없다.
//! 규칙: 기기·PC에 영향을 주는(쓰기·설치·삭제·플래시) 명령은 이 모듈에 작성 금지.

use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use adb_client::usb::{find_all_connected_adb_devices, ADBUSBDevice};
use adb_client::ADBDeviceExt;
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Mutex, MutexGuard, OnceLock, TryLockError};
use std::time::{Duration, Instant};

// ── 연결 관리 ──

const SERVER_ADDR: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 5037);

/// 존재하지 않는 adb 경로. adb_client는 로컬 서버에 연결할 때마다 `adb start-server`를 실행하므로
/// 이 경로를 넘겨 바이너리 실행을 원천 차단한다 (AGENTS: adb 바이너리 직접 실행 금지)
const NO_ADB_BINARY: &str = r"\\?\xvolte\no-adb-binary";

/// 이미 실행 중인 adb 서버가 있는지 포트만 확인 (서버를 새로 띄우지 않음)
fn server_reachable() -> bool {
    TcpStream::connect_timeout(&SocketAddr::V4(SERVER_ADDR), Duration::from_millis(300)).is_ok()
}

/// adb 서버가 보고한 기기 (상태 포함: device / unauthorized / offline …)
struct ServerEntry {
    serial: String,
    state: String,
}

impl ServerEntry {
    fn ready(&self) -> bool {
        self.state.eq_ignore_ascii_case("device")
    }
    fn handle(&self) -> ADBServerDevice {
        ADBServerDevice::new(self.serial.clone(), Some(SERVER_ADDR))
    }
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

// ── ADB 인증 키 (USB 직접 연결용) ──
// adb_client는 키 파일이 없으면 연결할 때마다 임의 키를 새로 만들어 폰에 "USB 디버깅 허용" 팝업이 매번 뜬다.
// → 표준 키(~/.android/adbkey)가 있으면 그대로 쓰고, 없으면 앱 데이터 폴더에 1회 생성해 재사용 (사용자 승인 2026-10-02)

static APP_KEY_DIR: OnceLock<PathBuf> = OnceLock::new();

/// 앱 시작 시 앱 데이터 폴더 지정 (lib.rs setup)
pub fn set_key_dir(dir: PathBuf) {
    let _ = APP_KEY_DIR.set(dir);
}

fn standard_adb_key() -> Option<PathBuf> {
    let base = std::env::var_os("ANDROID_USER_HOME")
        .map(|h| PathBuf::from(h).join("android"))
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".android")))?;
    let key = base.join("adbkey");
    key.exists().then_some(key)
}

fn adb_key_path() -> Result<PathBuf, String> {
    if let Some(key) = standard_adb_key() {
        return Ok(key);
    }
    let dir = APP_KEY_DIR.get().ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let path = dir.join("adbkey");
    if !path.exists() {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        std::fs::create_dir_all(dir).map_err(|e| format!("인증 키 폴더 생성 실패: {e}"))?;
        let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048)
            .map_err(|e| format!("인증 키 생성 실패: {e}"))?;
        let pem = key.to_pkcs8_pem(LineEnding::LF).map_err(|e| format!("인증 키 변환 실패: {e}"))?;
        std::fs::write(&path, pem.as_bytes()).map_err(|e| format!("인증 키 저장 실패: {e}"))?;
        eprintln!("[rust] ADB 인증 키 생성: 앱 데이터 폴더");
    }
    Ok(path)
}

fn open_usb() -> Result<ADBUSBDevice, String> {
    let devices =
        find_all_connected_adb_devices().map_err(|e| format!("USB 기기 검색 실패: {e}"))?;
    if devices.is_empty() {
        return Err("연결된 기기 없음".into());
    }
    // 여러 대면 첫 번째 (동일 모델 중복 연결의 구분은 서버 모드에서만 가능 — 한계)
    let info = &devices[0];
    ADBUSBDevice::new_with_custom_private_key(info.vendor_id, info.product_id, adb_key_path()?)
        .map_err(|e| format!("기기 연결 실패: {e}"))
}

fn ensure_usb(guard: &mut MutexGuard<'static, Option<ADBUSBDevice>>) -> Result<(), String> {
    if guard.is_none() {
        **guard = Some(open_usb()?);
    }
    Ok(())
}

/// 실행 중인 adb 서버가 보고한 기기 목록 (준비 안 된 기기 포함).
/// - 서버 미실행 → None (USB 직접 연결로 폴백, 서버를 새로 띄우지 않음)
/// - 서버 실행 중 → Some(목록) (서버가 USB를 점유 중이므로 USB 폴백 금지)
fn server_devices() -> Option<Vec<ServerEntry>> {
    if !server_reachable() {
        return None;
    }
    let mut server = ADBServer::new_from_path(SERVER_ADDR, Some(NO_ADB_BINARY.into()));
    let list = match server.devices() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rust] adb 서버 응답 오류(USB 직접 연결로 전환): {e}");
            return None;
        }
    };
    let entries: Vec<ServerEntry> = list
        .into_iter()
        .map(|d| ServerEntry { serial: d.identifier, state: d.state.to_string() })
        .collect();
    let summary: Vec<String> =
        entries.iter().map(|d| format!("{}={}", mask_serial(&d.serial), d.state)).collect();
    eprintln!("[rust] adb 서버 기기: {}", summary.join(", "));
    Some(entries)
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
    let blocking = tauri::async_runtime::spawn_blocking(move || match rx.recv_timeout(limit) {
        Ok(r) => r,
        Err(mpsc::RecvTimeoutError::Timeout) => Err("기기 응답 시간 초과".into()),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err("기기 질의 중 내부 오류가 발생했습니다".into()),
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
    let wanted = serial.as_deref().filter(|s| !s.is_empty());

    // 1) adb 서버 모드 — serial 지정 시 정확히 일치하는 기기만, 미지정 시 준비된 기기가 1대일 때만
    if let Some(devs) = server_devices() {
        let ready: Vec<&ServerEntry> = devs.iter().filter(|d| d.ready()).collect();
        let entry = match wanted {
            Some(s) => *ready.iter().find(|d| d.serial == s).ok_or("지정한 기기를 찾을 수 없습니다")?,
            None if ready.len() > 1 => return Err("여러 대의 기기가 연결되어 있습니다".into()),
            None => *ready.first().ok_or("연결된 기기가 없습니다")?,
        };
        return f(&mut entry.handle());
    }

    // 2) USB 직접 연결 — 기기 구분 수단이 없으므로 1대일 때만, serial 지정 시 연결 후 일치 확인
    let found = find_all_connected_adb_devices().map_err(|e| format!("USB 검색 실패: {e}"))?;
    if found.is_empty() {
        return Err("연결된 기기가 없습니다".into());
    }
    if found.len() > 1 {
        return Err("여러 대의 기기가 연결되어 있습니다".into());
    }
    let mut guard = lock_usb(Duration::from_secs(30))?;
    let mut run = |guard: &mut MutexGuard<'static, Option<ADBUSBDevice>>| -> Result<T, String> {
        ensure_usb(guard)?;
        let dev = guard.as_mut().expect("연결 보장됨");
        if let Some(s) = wanted {
            let actual = shell(dev, "getprop ro.serialno")?;
            if actual.trim() != s {
                return Err("지정한 기기를 찾을 수 없습니다".into());
            }
        }
        f(dev)
    };
    let first = run(&mut guard);
    if first.is_err() {
        // 커넥션이 끊겼을 수 있으므로 버리고 재연결 후 1회 재시도
        *guard = None;
        return run(&mut guard);
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
            detail: Some(format!("준비된 기기 {}대", devs.iter().filter(|d| d.ready()).count())),
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

/// 일련번호 부분 마스킹 — 앞 6자만 표시 (AGENTS 규칙 5, 예: AB1234****)
fn mask_serial(serial: &str) -> String {
    let n = serial.chars().count();
    if n > 6 {
        format!("{}****", serial.chars().take(6).collect::<String>())
    } else {
        serial.to_string()
    }
}

/// 제품명 — 기기가 보고하는 `ro.semc.product.name`(실측 "Xperia 1 V") 우선, 없으면 모델명 표
fn product_name(reported: &str, model: &str) -> String {
    if !reported.trim().is_empty() {
        return reported.trim().to_string();
    }
    match model {
        "XQ-DQ44" | "XQ-DQ72" | "XQ-DQ54" => "Xperia 1 V".into(),
        "XQ-DE44" | "XQ-DE54" => "Xperia 5 V".into(),
        "XQ-EC72" | "XQ-EC54" | "XQ-EC44" => "Xperia 1 VI".into(),
        _ => model.to_string(),
    }
}

/// 부트로더 상태 — `ro.boot.flash.locked`와 `ro.boot.vbmeta.device_state`가 일치할 때만 확정.
/// 루팅(su 존재)인데 잠김으로 보고되면 위장(PIF 등) 가능성 → 판별 불가
fn bootloader_state(flash_locked: &str, vbmeta_state: &str, rooted: bool) -> &'static str {
    let a = match flash_locked {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    };
    let b = match vbmeta_state {
        "locked" => Some(true),
        "unlocked" => Some(false),
        _ => None,
    };
    let locked = match (a, b) {
        (Some(x), Some(y)) if x == y => x,
        (Some(x), None) | (None, Some(x)) => x,
        _ => return "unknown",
    };
    if locked && rooted {
        return "unknown";
    }
    if locked { "locked" } else { "unlocked" }
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

/// TelephonyDebugService 덤프(필터링)에서 슬롯별 VoLTE 사용 가능 여부.
/// 전화 앱 *#*#4636#*#* 휴대전화 정보의 IMS 상태와 같은 출처 — ImsPhone의 MmTel 음성 기능(Voice)이
/// true면 IMS 음성(VoLTE) 등록 상태. phoneId 0 = 슬롯 1.
fn parse_ims_voice(out: &str) -> HashMap<u8, bool> {
    let mut map = HashMap::new();
    let mut phone: Option<u8> = None;
    for line in out.lines() {
        let t = line.trim();
        if let Some(v) = t.strip_prefix("mPhoneId=") {
            phone = v.trim().parse::<i32>().ok().filter(|n| *n >= 0).map(|n| (n + 1) as u8);
        } else if t.starts_with("mMmTelCapabilities=") {
            if let Some(slot) = phone {
                map.entry(slot).or_insert(t.contains("Voice: true"));
            }
        }
    }
    map
}

/// 듀얼 SIM 프롭에서 슬롯 값 — 실기기는 "[,SK Telecom]"처럼 쉼표로 슬롯을 구분해 한 프롭에 담음.
/// 쉼표 구분 값이 비어 있으면 `<key>.2` 형식(일부 기종)으로 폴백
fn slot_prop(p: &HashMap<String, String>, key: &str, idx: usize) -> String {
    let v = p
        .get(key)
        .and_then(|s| s.split(',').nth(idx))
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    if !v.is_empty() || idx == 0 {
        return v;
    }
    p.get(&format!("{key}.{}", idx + 1)).map(|s| s.trim().to_string()).unwrap_or_default()
}

/// `dumpsys isub`의 "simSlotIndex=N portIndex=P isEmbedded=E" 줄에서 슬롯별 eSIM 여부
fn parse_embedded_slots(out: &str) -> HashMap<u8, bool> {
    let mut map = HashMap::new();
    for line in out.lines() {
        let mut slot: Option<i32> = None;
        let mut embedded: Option<bool> = None;
        for tok in line.split_whitespace() {
            if let Some(v) = tok.strip_prefix("simSlotIndex=") {
                slot = v.parse().ok();
            } else if let Some(v) = tok.strip_prefix("isEmbedded=") {
                embedded = Some(v == "1");
            }
        }
        if let (Some(s), Some(e)) = (slot, embedded) {
            if s >= 0 {
                map.insert((s + 1) as u8, e);
            }
        }
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
    /// gsm.sim.state 원값 (LOADED / ABSENT / PIN_REQUIRED / …), 비어 있으면 ABSENT
    state: String,
    /// "on" = IMS 음성 등록 / "off" = 미등록 / "unknown" = 판별 불가
    volte: &'static str,
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
    /// adb 연결 상태: "device"(준비) / "unauthorized"(USB 디버깅 허용 대기) / "offline" / "usb"(다중 USB 자리표시)
    state: String,
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
    let raw = shell(
        dev,
        "getprop; echo __SU__; which su || true; echo __ISUB__; \
         dumpsys isub | sed -n '/^Active subscriptions:/,/^All subscriptions:/p' \
           | grep -oE 'simSlotIndex=-?[0-9]+ portIndex=-?[0-9]+ isEmbedded=[01]' || true; echo __IMS__; \
         dumpsys activity service com.android.phone/.TelephonyDebugService \
           | grep -E 'mPhoneId=|mMmTelCapabilities=' || true",
    )?;
    let (props_raw, rest) = raw.split_once("__SU__").unwrap_or((&raw, ""));
    let (su_raw, rest) = rest.split_once("__ISUB__").unwrap_or((rest, ""));
    let (isub_raw, ims_raw) = rest.split_once("__IMS__").unwrap_or((rest, ""));
    let embedded = parse_embedded_slots(isub_raw);
    let ims_voice = parse_ims_voice(ims_raw);
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

    let rooted = !su_raw.trim().is_empty();
    let bootloader =
        bootloader_state(&get("ro.boot.flash.locked"), &get("ro.boot.vbmeta.device_state"), rooted);

    let sim_state = get("gsm.sim.state");
    let states: Vec<&str> = sim_state.split(',').collect();
    let alphas = [slot_prop(&p, "gsm.sim.operator.alpha", 0), slot_prop(&p, "gsm.sim.operator.alpha", 1)];
    let numerics = [slot_prop(&p, "gsm.sim.operator.numeric", 0), slot_prop(&p, "gsm.sim.operator.numeric", 1)];

    let sims = (1..=2u8)
        .map(|slot| {
            let idx = (slot - 1) as usize;
            let state = states
                .get(idx)
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .unwrap_or("ABSENT")
                .to_string();
            let loaded = state == "LOADED";
            // 활성 구독이 있으면 실측값, 없으면 슬롯 1=물리 / 2=eSIM (XQ-DQ44 구성) 가정
            let is_esim = embedded.get(&slot).copied().unwrap_or(slot == 2);
            SimOut {
                slot,
                sim_type: if is_esim { "esim" } else { "physical" },
                carrier: loaded.then(|| {
                    if alphas[idx].is_empty() { "통신사 확인 불가".to_string() } else { alphas[idx].clone() }
                }),
                state,
                // IMS 음성 등록 상태(실측) 우선 — 덤프에서 못 읽으면 판별 불가
                volte: match (loaded, ims_voice.get(&slot)) {
                    (true, Some(true)) => "on",
                    (true, Some(false)) => "off",
                    _ => "unknown",
                },
                plmn: numerics[idx].clone(),
            }
        })
        .collect();

    let model = get("ro.product.model");
    let serial_masked = mask_serial(&serial);
    // 펌웨어: ro.build.id(실측 "67.2.A.3.178") — display.id는 " release-keys"가 붙음
    let mut firmware = get("ro.build.id");
    if firmware.is_empty() {
        firmware = get("ro.build.display.id").trim_end_matches(" release-keys").to_string();
    }
    Ok(DeviceOut {
        state: "device".into(),
        serial,
        serial_masked,
        product_name: product_name(&get("ro.semc.product.name"), &model),
        model,
        firmware,
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

/// 셸을 열 수 없는 기기의 자리표시 항목 — 다중 USB 기기 경고 / USB 디버깅 미승인 안내용
fn placeholder(state: &str, serial: String, serial_masked: String, name: &str) -> DeviceOut {
    DeviceOut {
        state: state.into(),
        serial,
        serial_masked,
        model: String::new(),
        product_name: name.to_string(),
        firmware: String::new(),
        android: String::new(),
        mode: "android".into(),
        bootloader: "unknown".into(),
        rooted: false,
        sims: vec![],
        usb: UsbOut {
            topology: String::new(),
            controller: String::new(),
            link_speed: String::new(),
        },
    }
}

fn device_list_work() -> Result<Vec<DeviceOut>, String> {
    // 1) adb 서버 모드 — 준비된 기기는 상태 조회, 미승인/오프라인 기기는 상태만
    if let Some(devs) = server_devices() {
        let mut out = Vec::with_capacity(devs.len());
        for entry in &devs {
            if !entry.ready() {
                out.push(placeholder(&entry.state, entry.serial.clone(), mask_serial(&entry.serial), "Android 기기"));
                continue;
            }
            match device_status(&mut entry.handle(), &entry.serial) {
                Ok(d) => out.push(d),
                Err(e) => eprintln!(
                    "[rust] 기기 정보 읽기 실패({}): {e}",
                    mask_serial(&entry.serial)
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
    if found.len() > 1 {
        // USB 직접 연결은 기기별 셸을 구분해 열 수 없음 → 식별 정보만 담아 반환 (프론트 다중 기기 경고용)
        eprintln!("[rust] device_list(usb) -> {} device(s), 다중 연결", found.len());
        return Ok(found
            .iter()
            .enumerate()
            .map(|(i, info)| {
                let name = info.device_description.trim();
                placeholder(
                    "usb",
                    format!("usb-{i}"),
                    format!("USB #{}", i + 1),
                    if name.is_empty() { "알 수 없는 기기" } else { name },
                )
            })
            .collect());
    }
    let mut guard = lock_usb(Duration::from_secs(30))?;
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

/// 한 번의 셸 실행으로 측정 — 폴더별 du와 3자 앱 APK 크기를 모두 병렬로, 끝나면 여유 공간.
/// 출력 형식으로 구분: "KB<TAB>경로" = du / "P" = 앱 1개 / 숫자만 = APK 파일 크기(split 포함).
/// APK는 `pm list packages -3 -f`의 설치 폴더에서 *.apk를 한 번에 stat (앱마다 pm path 호출 시 실측 8.8s → 1.9s).
/// 합산은 Rust에서 (기기 셸 산술은 32비트라 넘침)
fn build_storage_script() -> String {
    let mut s = String::from("(");
    for (_, path) in FOLDER_PATHS.iter() {
        s.push_str(&format!("du -sk {path} & "));
    }
    s.push_str(
        "(for l in $(pm list packages -3 -f); do a=${l#package:}; a=${a%=*}; \
         echo P; stat -c %s ${a%/*}/*.apk; done) & \
         wait); echo __DF__; df -k /sdcard",
    );
    s
}

fn parse_storage_output(raw: &str) -> Value {
    let mut sizes: HashMap<String, u64> =
        FOLDER_PATHS.iter().map(|(id, _)| (id.to_string(), 0u64)).collect();
    let mut section = 0u8; // 0=du·apk(병렬, 형식으로 구분), 2=df
    let mut apk_sum = 0u64;
    let mut apk_n = 0u64;
    let mut free_kb = 0u64;

    for line in raw.lines() {
        let t = line.trim();
        if t == "__DF__" {
            section = 2;
            continue;
        }
        match section {
            0 => {
                let mut it = t.split_whitespace();
                match (it.next(), it.next()) {
                    // "1234\t/sdcard/DCIM" — du
                    (Some(kb), Some(path)) => {
                        if let (Ok(kb), Some(id)) = (
                            kb.parse::<u64>(),
                            FOLDER_PATHS.iter().find(|(_, p)| *p == path).map(|(id, _)| *id),
                        ) {
                            sizes.insert(id.to_string(), kb * 1024);
                        }
                    }
                    // "P" — 앱 1개 / 숫자 하나 — APK 파일 크기
                    (Some("P"), None) => apk_n += 1,
                    (Some(n), None) => {
                        if let Ok(b) = n.parse::<u64>() {
                            apk_sum += b;
                        }
                    }
                    _ => {}
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

// ── 앱별 백업 자격 (recovery.md 2-2 판정 신호) ──

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AppFlagOut {
    pkg: String,
    /// ALLOW_BACKUP 플래그 — 구글 백업 자격(복구 성립의 증명은 아님, plan §6-4)
    allow_backup: bool,
    /// /sdcard/Android/data/<pkg> 존재 — "앱 데이터" 백업 항목에 포함되는 외부 데이터
    has_external_data: bool,
    /// 구글 백업에 실제로 백업된 기록 존재 (`dumpsys backup`의 현재 구글 전송 대상 중 state bytes > 0)
    google_backed_up: bool,
}

/// `dumpsys backup`에서 현재(*) 구글 백업 전송의 "pkg - N state bytes" 중 N > 0인 패키지
fn parse_google_backed_up(raw: &str) -> std::collections::HashSet<String> {
    let mut set = std::collections::HashSet::new();
    let mut in_google = false;
    for line in raw.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("* ") {
            // "* " = 현재 선택된 전송 — 구글 백업 전송일 때만 블록 시작
            in_google = rest.contains("com.google.android.gms/.backup.BackupTransportService");
            continue;
        }
        if !line.starts_with(' ') {
            in_google = false; // 들여쓰기 없는 줄(다음 섹션) → 블록 종료
            continue;
        }
        if !in_google {
            continue;
        }
        if let Some(rest) = t.strip_suffix(" state bytes") {
            if let Some((pkg, n)) = rest.rsplit_once(" - ") {
                if n.trim().parse::<u64>().unwrap_or(0) > 0 {
                    set.insert(pkg.trim().to_string());
                }
            }
        } else if t.contains("/.") {
            in_google = false; // 다른 전송 헤더
        }
    }
    set
}

/// `pm list packages -3` 결과(3자 앱) + `dumpsys package packages`의 Package/pkgFlags 줄
/// + `ls /sdcard/Android/data` (외부 데이터 폴더) + `dumpsys backup` (구글 백업 기록)
fn parse_app_flags(raw: &str) -> Vec<AppFlagOut> {
    let (raw, gbackup_raw) = raw.split_once("__GBACKUP__").unwrap_or((raw, ""));
    let google = parse_google_backed_up(gbackup_raw);
    let (raw, data_raw) = raw.split_once("__DATA__").unwrap_or((raw, ""));
    let data_dirs: std::collections::HashSet<&str> = data_raw.lines().map(|l| l.trim()).collect();
    let (pkgs_raw, flags_raw) = raw.split_once("__FLAGS__").unwrap_or((raw, ""));
    let mut allow: HashMap<String, bool> = HashMap::new();
    let mut current: Option<String> = None;
    for line in flags_raw.lines() {
        let t = line.trim();
        if let Some(rest) = t.strip_prefix("Package [") {
            current = rest.split(']').next().map(|s| s.to_string());
        } else if let Some(flags) = t.strip_prefix("pkgFlags=") {
            if let Some(pkg) = current.take() {
                allow.insert(pkg, flags.split_whitespace().any(|f| f == "ALLOW_BACKUP"));
            }
        }
    }
    let mut out: Vec<AppFlagOut> = pkgs_raw
        .lines()
        .map(|l| l.trim().trim_start_matches("package:").to_string())
        .filter(|p| !p.is_empty())
        .map(|pkg| AppFlagOut {
            allow_backup: allow.get(&pkg).copied().unwrap_or(false),
            has_external_data: data_dirs.contains(pkg.as_str()),
            google_backed_up: google.contains(&pkg),
            pkg,
        })
        .collect();
    out.sort_by(|a, b| a.pkg.cmp(&b.pkg));
    out
}

/// 3자 앱 목록과 ALLOW_BACKUP 여부 (읽기 전용)
#[tauri::command]
pub async fn app_flags(serial: Option<String>) -> Result<Vec<AppFlagOut>, String> {
    guarded(Duration::from_secs(60), move || {
        let raw = with_first_device(&serial, |dev| {
            shell(
                dev,
                "pm list packages -3; echo __FLAGS__; \
                 dumpsys package packages | grep -E '^  Package \\[|^    pkgFlags='; \
                 echo __DATA__; ls /sdcard/Android/data 2>/dev/null || true; \
                 echo __GBACKUP__; dumpsys backup 2>/dev/null || true",
            )
        })?;
        let list = parse_app_flags(&raw);
        eprintln!("[rust] app_flags -> {} pkgs", list.len());
        Ok(list)
    })
    .await
}

// ── 설정 백업 개요 (recovery.md 1-2: 전체 덤프 + 화이트리스트 복원) ──

/// 초기화 후 자동 복원 대상 설정 키 (plan §6-5 화이트리스트)
const RESTORE_SETTINGS: [(&str, &str, &str); 7] = [
    ("secure", "sysui_qs_tiles", "빠른 설정 타일 순서"),
    ("secure", "default_input_method", "기본 키보드"),
    ("system", "screen_brightness", "화면 밝기"),
    ("system", "screen_brightness_mode", "자동 밝기"),
    ("system", "screen_off_timeout", "화면 자동 꺼짐 시간"),
    ("system", "font_scale", "글꼴 크기"),
    ("global", "stay_on_while_plugged_in", "충전 중 화면 켜짐 유지"),
];

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingValueOut {
    namespace: String,
    key: String,
    label: String,
    /// 현재 값 원문 ("null" = 미설정)
    value: String,
}

#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SettingsOverviewOut {
    /// 전체 덤프로 보관되는 키 개수
    system_count: u32,
    secure_count: u32,
    global_count: u32,
    /// 자동 복원 대상 키의 현재 값
    restore_items: Vec<SettingValueOut>,
    /// 배터리 최적화 예외로 사용자가 지정한 앱 수 (deviceidle user 항목)
    battery_exempt_apps: u32,
}

fn build_settings_script() -> String {
    let mut s = String::from(
        "for n in system secure global; do echo \"C|$n|$(settings list $n | wc -l)\"; done; ",
    );
    for (ns, key, _) in RESTORE_SETTINGS.iter() {
        s.push_str(&format!("echo \"V|{ns}|{key}|$(settings get {ns} {key})\"; "));
    }
    s.push_str("echo \"I|$(dumpsys deviceidle whitelist | grep -c '^user,')\"");
    s
}

fn parse_settings_overview(raw: &str) -> SettingsOverviewOut {
    let mut out = SettingsOverviewOut {
        system_count: 0,
        secure_count: 0,
        global_count: 0,
        restore_items: vec![],
        battery_exempt_apps: 0,
    };
    for line in raw.lines() {
        let parts: Vec<&str> = line.trim().splitn(4, '|').collect();
        match parts.as_slice() {
            ["C", ns, n] => {
                let n = n.trim().parse().unwrap_or(0);
                match *ns {
                    "system" => out.system_count = n,
                    "secure" => out.secure_count = n,
                    "global" => out.global_count = n,
                    _ => {}
                }
            }
            ["V", ns, key, value] => {
                if let Some((_, _, label)) = RESTORE_SETTINGS.iter().find(|(n, k, _)| n == ns && k == key) {
                    out.restore_items.push(SettingValueOut {
                        namespace: ns.to_string(),
                        key: key.to_string(),
                        label: label.to_string(),
                        value: value.to_string(),
                    });
                }
            }
            ["I", n] => out.battery_exempt_apps = n.trim().parse().unwrap_or(0),
            _ => {}
        }
    }
    out
}

/// 설정 백업 개요 (읽기 전용 — settings list/get, dumpsys deviceidle)
#[tauri::command]
pub async fn settings_overview(serial: Option<String>) -> Result<SettingsOverviewOut, String> {
    guarded(Duration::from_secs(30), move || {
        let script = build_settings_script();
        let raw = with_first_device(&serial, |dev| shell(dev, &script))?;
        Ok(parse_settings_overview(&raw))
    })
    .await
}

#[cfg(test)]
mod tests {
    /// 실기기(XQ-DQ44) 출력 기반
    #[test]
    fn parse_settings_overview_real_sample() {
        let raw = "\
C|system|100\n\
C|secure|210\n\
C|global|295\n\
V|secure|sysui_qs_tiles|internet,bt,custom(com.google.android.gms/.nearby.sharing.SharingTileService),night\n\
V|system|screen_off_timeout|600000\n\
V|global|stay_on_while_plugged_in|0\n\
I|6\n";
        let o = parse_settings_overview(raw);
        assert_eq!((o.system_count, o.secure_count, o.global_count), (100, 210, 295));
        assert_eq!(o.battery_exempt_apps, 6);
        assert_eq!(o.restore_items.len(), 3);
        assert_eq!(o.restore_items[0].label, "빠른 설정 타일 순서");
        assert!(o.restore_items[0].value.contains("custom(com.google.android.gms/.nearby.sharing.SharingTileService)"));
        assert_eq!(o.restore_items[1].value, "600000");
    }

    /// 실기기(XQ-DQ44) 출력 형식 기반 — 3자 앱만, 시스템 앱 플래그는 무시
    #[test]
    fn parse_app_flags_real_sample() {
        let raw = "\
package:kr.co.tmoney.tia\n\
package:com.instagram.android\n\
__FLAGS__\n\
  Package [kr.co.tmoney.tia] (5d7dc9c):\n\
    pkgFlags=[ HAS_CODE ALLOW_CLEAR_USER_DATA ]\n\
  Package [com.google.android.networkstack.tethering] (f11a062):\n\
    pkgFlags=[ SYSTEM HAS_CODE PERSISTENT ALLOW_CLEAR_USER_DATA ALLOW_BACKUP ]\n\
  Package [com.instagram.android] (1a2b3c4):\n\
    pkgFlags=[ HAS_CODE ALLOW_CLEAR_USER_DATA ALLOW_BACKUP ]\n\
__DATA__\n\
com.instagram.android\n\
com.kakao.talk\n\
__GBACKUP__\n";
        // 실기기 dumpsys backup 형식 (들여쓰기 보존)
        let gbackup = concat!(
            "Available transports:\n",
            "    com.android.localtransport/.LocalTransport\n",
            "       destination: Backing up to debug-only private cache\n",
            "  * com.google.android.gms/.backup.BackupTransportService\n",
            "       destination: someone@gmail.com\n",
            "       @pm@ - 12568 state bytes\n",
            "       kr.co.tmoney.tia - 80 state bytes\n",
            "       com.instagram.android - 0 state bytes\n",
            "    com.google.android.gms/.backup.migrate.service.D2dTransport\n",
            "       com.example.other - 50 state bytes\n",
            "Transport clients created: 1\n",
        );
        let raw = format!("{raw}{gbackup}");
        let g = parse_google_backed_up(gbackup);
        assert!(g.contains("kr.co.tmoney.tia") && g.contains("@pm@"));
        assert!(!g.contains("com.instagram.android") && !g.contains("com.example.other"));
        let v = parse_app_flags(&raw);
        assert_eq!(v.len(), 2);
        // instagram: 구글 전송에 있지만 0 bytes → 백업 기록 없음
        assert_eq!(v[0], AppFlagOut { pkg: "com.instagram.android".into(), allow_backup: true, has_external_data: true, google_backed_up: false });
        assert_eq!(v[1], AppFlagOut { pkg: "kr.co.tmoney.tia".into(), allow_backup: false, has_external_data: false, google_backed_up: true });
    }

    use super::*;

    /// 실기기(XQ-DQ44, VoLTE 미패치) TelephonyDebugService 필터 출력 + 등록된 경우 가정
    #[test]
    fn parse_ims_voice_real_sample() {
        let raw = "\
       mPhoneId=0\n\
       mPhoneId=0\n\
       mMmTelCapabilities=MmTel Capabilities - [Voice: false Video: false UT: false SMS: false CALL_COMPOSER: false BUSINESS_COMPOSER_ONLY: false]\n\
       mPhoneId=1\n\
       mPhoneId=1\n\
       mMmTelCapabilities=MmTel Capabilities - [Voice: true Video: false UT: true SMS: true CALL_COMPOSER: false BUSINESS_COMPOSER_ONLY: false]\n\
                mPhoneId=1\n";
        let m = parse_ims_voice(raw);
        assert_eq!(m.get(&1), Some(&false));
        assert_eq!(m.get(&2), Some(&true));
        assert!(parse_ims_voice("").is_empty());
    }

    /// 실기기(XQ-DQ44) getprop — 슬롯 1 비어 있음, 슬롯 2 eSIM SKT
    #[test]
    fn slot_prop_comma_separated() {
        let p = parse_getprop(
            "[gsm.sim.operator.alpha]: [,SK Telecom]\n\
             [gsm.sim.operator.numeric]: [,45005]\n",
        );
        assert_eq!(slot_prop(&p, "gsm.sim.operator.alpha", 0), "");
        assert_eq!(slot_prop(&p, "gsm.sim.operator.alpha", 1), "SK Telecom");
        assert_eq!(slot_prop(&p, "gsm.sim.operator.numeric", 1), "45005");
        // .2 형식 기종 폴백
        let q = parse_getprop("[gsm.sim.operator.alpha]: [KT]\n[gsm.sim.operator.alpha.2]: [LG U+]\n");
        assert_eq!(slot_prop(&q, "gsm.sim.operator.alpha", 0), "KT");
        assert_eq!(slot_prop(&q, "gsm.sim.operator.alpha", 1), "LG U+");
    }

    /// 실기기(XQ-DQ44) `dumpsys isub` 캡처 — 슬롯 2 eSIM 활성, 비활성 구독은 simSlotIndex=-1
    #[test]
    fn parse_embedded_slots_real_sample() {
        let raw = "\
simSlotIndex=1 portIndex=0 isEmbedded=1\n\
simSlotIndex=-1 portIndex=-1 isEmbedded=0\n\
simSlotIndex=-1 portIndex=-1 isEmbedded=0\n\
simSlotIndex=1 portIndex=0 isEmbedded=1\n";
        let m = parse_embedded_slots(raw);
        assert_eq!(m.get(&2), Some(&true));
        assert_eq!(m.get(&1), None);
        assert_eq!(m.len(), 1);
    }

    /// 실기기(XQ-DQ44)에서 캡처한 출력 기반 — 섹션 마커/병렬 du/권한 오류 혼재/df 파싱 검증
    #[test]
    fn parse_storage_output_real_sample() {
        let raw = "\
136533361\t/sdcard/DCIM\n\
P\n\
326551\t/sdcard/Download\n\
68440742\n\
8\t/sdcard/Recordings\n\
P\n\
0\t/sdcard/Android/data-not-there\n\
187393515\n\
du: /sdcard/Android/data/x: Permission denied\n\
1200000\n\
42064776\t/sdcard/Android/data\n\
180041842\t/storage/emulated/0\n\
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
        // 두 번째 앱은 base + split APK 2개 — 앱 수는 2, 크기는 split 포함 합산
        assert_eq!(get("apk-total"), 68440742 + 187393515 + 1200000);
        assert_eq!(get("apk-count"), 2);
    }

    /// 생성 키가 adb_client가 읽는 형식(PKCS8 PEM, RsaPrivateKey::from_pkcs8_pem)과 맞는지
    #[test]
    fn generated_key_is_pkcs8_readable() {
        use rsa::pkcs8::{DecodePrivateKey, EncodePrivateKey, LineEnding};
        let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
        let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
        let back = rsa::RsaPrivateKey::from_pkcs8_pem(&pem).unwrap();
        assert_eq!(key, back);
    }

    #[test]
    fn mask_serial_keeps_six() {
        assert_eq!(mask_serial("AB1234CDEF"), "AB1234****");
        assert_eq!(mask_serial("ABC"), "ABC");
        assert_eq!(mask_serial("가나다라마바사아"), "가나다라마바****");
    }

    #[test]
    fn bootloader_state_cases() {
        // 실측: 잠긴 정상 기기
        assert_eq!(bootloader_state("1", "locked", false), "locked");
        assert_eq!(bootloader_state("0", "unlocked", true), "unlocked");
        // 잠김 위장(루팅인데 locked) / 두 값 불일치 → 판별 불가
        assert_eq!(bootloader_state("1", "locked", true), "unknown");
        assert_eq!(bootloader_state("1", "unlocked", false), "unknown");
        assert_eq!(bootloader_state("", "", false), "unknown");
        assert_eq!(bootloader_state("0", "", false), "unlocked");
    }

    #[test]
    fn product_name_prefers_reported() {
        assert_eq!(product_name("Xperia 1 V", "XQ-DQ44"), "Xperia 1 V");
        assert_eq!(product_name("", "XQ-EC72"), "Xperia 1 VI");
        assert_eq!(product_name("", "XQ-ZZ99"), "XQ-ZZ99");
    }
}
