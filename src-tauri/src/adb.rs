//! 기기 연결 선택과 ADB 읽기 질의 — `adb_client` 크레이트로 ADB 프로토콜 직접 통신.
//! - 1순위: 실행 중인 adb 서버(localhost:5037)에 연결 — Android Studio 등이 띄운 서버 재사용
//! - 2순위: USB 직접 연결 — 서버가 없을 때 크레이트가 프로토콜을 직접 구현해 통신
//!
//! adb 바이너리 설치/경로 탐색이 필요 없다.
//! 역할: 연결 선택(`with_first_device`)은 모든 엔진(백업·복원·루팅 등)이 함께 쓰는 공용 통로다.
//! 이 모듈의 Tauri 명령은 읽기 질의와 설정 화면 열기(값 변경 없음)뿐이며, 기기 변경 명령은 각 엔진 모듈에 둔다.
//! USB 직접 연결용 인증 키는 앱 데이터 폴더에 한 번 만든다(사용자 승인 2026-10-02).

use crate::ims::{diagnostics as ims_diagnostics, parse_ims_voice, ImsDiagnostic};
use crate::tasks::guarded;
use adb_client::server::ADBServer;
use adb_client::server_device::ADBServerDevice;
use adb_client::usb::{find_all_connected_adb_devices, ADBUSBDevice};
use adb_client::{ADBDeviceExt, RustADBError, UNAUTHORIZED_MARKER};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, TcpStream};
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, TryLockError};
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

fn standard_adb_key() -> Option<PathBuf> {
    // ANDROID_USER_HOME은 설정 폴더 자체(기본 ~/.android)를 가리킨다
    let base = std::env::var_os("ANDROID_USER_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(|h| PathBuf::from(h).join(".android")))?;
    let key = base.join("adbkey");
    key.exists().then_some(key)
}

fn adb_key_path() -> Result<PathBuf, String> {
    if let Some(key) = standard_adb_key() {
        return Ok(key);
    }
    let dir = crate::app_paths::data_dir().ok_or("앱 데이터 폴더를 확인할 수 없습니다")?;
    let path = dir.join("adbkey");
    if !path.exists() {
        use rsa::pkcs8::{EncodePrivateKey, LineEnding};
        std::fs::create_dir_all(&dir).map_err(|e| format!("인증 키 폴더 생성 실패: {e}"))?;
        let key = rsa::RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048)
            .map_err(|e| format!("인증 키 생성 실패: {e}"))?;
        let pem = key
            .to_pkcs8_pem(LineEnding::LF)
            .map_err(|e| format!("인증 키 변환 실패: {e}"))?;
        crate::storage::atomic_write(&path, pem.as_bytes())
            .map_err(|e| format!("인증 키 저장 실패: {e}"))?;
        eprintln!("[rust] ADB 인증 키 생성: 앱 데이터 폴더");
    }
    Ok(path)
}

/// Xperia만 다룬다 — USB 직접 연결 시 Sony(VID 0x0FCE) ADB 기기만 (사용자 지시 2026-10-03)
const SONY_VID: u16 = 0x0FCE;

fn sony_usb_adb_devices() -> Result<Vec<adb_client::usb::ADBDeviceInfo>, String> {
    Ok(find_all_connected_adb_devices()
        .map_err(|e| format!("USB 기기 검색 실패: {e}"))?
        .into_iter()
        .filter(|d| d.vendor_id == SONY_VID)
        .collect())
}

/// USB 직접 연결에서 폰의 "USB 디버깅 허용" 창이 아직 승인되지 않은 상태
const USB_UNAUTHORIZED: &str =
    "폰에서 USB 디버깅 허용을 기다리는 중입니다 — 폰 화면에서 허용을 눌러 주세요";

/// 연결 실패를 사용자 문구로 — 허용 대기(공개 키 전송 후 무응답)는 일반 실패와 구분한다
fn usb_open_error(e: RustADBError) -> String {
    match e {
        RustADBError::ADBRequestFailed(m) if m == UNAUTHORIZED_MARKER => USB_UNAUTHORIZED.into(),
        other => format!("기기 연결 실패: {other}"),
    }
}

fn open_usb() -> Result<ADBUSBDevice, String> {
    let devices = sony_usb_adb_devices()?;
    if devices.is_empty() {
        return Err("연결된 기기 없음".into());
    }
    if devices.len() != 1 {
        return Err("여러 대의 기기가 연결되어 있습니다".into());
    }
    let info = &devices[0];
    ADBUSBDevice::new_with_custom_private_key(info.vendor_id, info.product_id, adb_key_path()?)
        .map_err(usb_open_error)
}

/// 보관 중인 연결을 쓰거나 새로 연다. 보관 연결은 재부팅·케이블 재연결 뒤 끊겨 있을 수 있으므로
/// 가벼운 명령으로 살아 있는지 확인하고, 실패하면 버리고 한 번 다시 연다(작업 자체는 아직 실행 전).
fn ensure_usb(guard: &mut MutexGuard<'static, Option<ADBUSBDevice>>) -> Result<(), String> {
    if let Some(dev) = guard.as_mut() {
        if crate::device_io::shell(dev, "true").is_ok() {
            return Ok(());
        }
        eprintln!("[rust] 보관된 USB 연결이 끊어져 다시 연결합니다");
        **guard = None;
    }
    **guard = Some(open_usb()?);
    Ok(())
}

/// adb 서버 상태 — 실행 중이 아니면 USB 직접 연결로, 응답 오류면 오류를 그대로 알린다
/// (서버가 USB 인터페이스를 점유하므로 서버 실행 중 USB 직접 연결 폴백은 실패한다)
enum Server {
    NotRunning,
    Failed(String),
    Devices(Vec<ServerEntry>),
}

/// 실행 중인 adb 서버가 보고한 기기 목록 (준비 안 된 기기 포함). 서버를 새로 띄우지 않는다.
fn server_devices() -> Server {
    if !server_reachable() {
        return Server::NotRunning;
    }
    let mut server = ADBServer::new_from_path(SERVER_ADDR, Some(NO_ADB_BINARY.into()));
    let list = match server.devices() {
        Ok(l) => l,
        Err(e) => {
            eprintln!("[rust] adb 서버 응답 오류: {e}");
            return Server::Failed(format!(
                "실행 중인 adb 서버가 응답하지 않습니다 — adb 서버를 종료하거나 다시 시작해 주세요 ({e})"
            ));
        }
    };
    let entries: Vec<ServerEntry> = list
        .into_iter()
        .map(|d| ServerEntry {
            serial: d.identifier,
            state: d.state.to_string(),
        })
        .collect();
    let summary: Vec<String> = entries
        .iter()
        .map(|d| format!("{}={}", mask_serial(&d.serial), d.state))
        .collect();
    eprintln!("[rust] adb 서버 기기: {}", summary.join(", "));
    Server::Devices(entries)
}

// ── 셸 실행 ──

pub(crate) fn shell(dev: &mut dyn ADBDeviceExt, cmd: &str) -> Result<String, String> {
    crate::device_io::shell(dev, cmd)
}

/// 서버 모드 우선, 없으면 USB 직접 연결로 실행. `f`는 많아야 한 번 실행하며,
/// 오류가 나면 USB 연결만 버리고 작업을 재실행하지 않는다(쓰기 작업 중복 방지).
pub(crate) fn with_first_device<T>(
    serial: &Option<String>,
    f: impl FnOnce(&mut dyn ADBDeviceExt) -> Result<T, String>,
) -> Result<T, String> {
    let wanted = serial.as_deref().filter(|s| !s.is_empty());

    // 1) adb 서버 모드 — serial 지정 시 정확히 일치하는 기기만, 미지정 시 준비된 기기가 1대일 때만
    match server_devices() {
        Server::Failed(e) => return Err(e),
        Server::Devices(devs) => {
            let ready: Vec<&ServerEntry> = devs.iter().filter(|d| d.ready()).collect();
            let entry = match wanted {
                Some(s) => *ready
                    .iter()
                    .find(|d| d.serial == s)
                    .ok_or("지정한 기기를 찾을 수 없습니다")?,
                None if ready.len() > 1 => return Err("여러 대의 기기가 연결되어 있습니다".into()),
                None => *ready.first().ok_or("연결된 기기가 없습니다")?,
            };
            let mut device = entry.handle();
            let manufacturer = shell(&mut device, "getprop ro.product.manufacturer")
                .map_err(|e| scrub_serial(e, &entry.serial))?;
            if !manufacturer.trim().eq_ignore_ascii_case("sony") {
                return Err("Sony 기기만 작업할 수 있습니다".into());
            }
            return f(&mut device).map_err(|e| scrub_serial(e, &entry.serial));
        }
        Server::NotRunning => {}
    }

    // 2) USB 직접 연결 — 기기 구분 수단이 없으므로 1대일 때만, serial 지정 시 연결 후 일치 확인
    let found = sony_usb_adb_devices()?;
    if found.is_empty() {
        return Err("연결된 기기가 없습니다".into());
    }
    if found.len() > 1 {
        return Err("여러 대의 기기가 연결되어 있습니다".into());
    }
    let mut guard = lock_usb(Duration::from_secs(30))?;
    let result = run_usb(&mut guard, wanted, f);
    if result.is_err() {
        *guard = None;
    }
    result.map_err(|e| match wanted {
        Some(s) => scrub_serial(e, s),
        None => e,
    })
}

fn run_usb<T>(
    guard: &mut MutexGuard<'static, Option<ADBUSBDevice>>,
    wanted: Option<&str>,
    f: impl FnOnce(&mut dyn ADBDeviceExt) -> Result<T, String>,
) -> Result<T, String> {
    ensure_usb(guard)?;
    let dev = guard.as_mut().ok_or("기기 연결을 확인할 수 없습니다")?;
    if let Some(s) = wanted {
        let actual = shell(dev, "getprop ro.serialno")?;
        if actual.trim() != s {
            return Err("지정한 기기를 찾을 수 없습니다".into());
        }
    }
    f(dev)
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
    // 서버 모드: 서버 도달 가능하면 그대로 사용(응답 오류는 USB로 대신하지 않고 알린다)
    match server_devices() {
        Server::Devices(devs) => {
            return AdbStatus {
                available: true,
                mode: "adb-server".into(),
                detail: Some(format!(
                    "준비된 기기 {}대",
                    devs.iter().filter(|d| d.ready()).count()
                )),
            }
        }
        Server::Failed(e) => {
            return AdbStatus {
                available: false,
                mode: "adb-server".into(),
                detail: Some(e),
            }
        }
        Server::NotRunning => {}
    }
    // USB 직접 연결 수단 점검 — device_list와 같은 기준(Sony 기기만)으로 센다
    match sony_usb_adb_devices() {
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
/// 오류 문자열에 섞인 전체 시리얼을 마스킹 (adb 오류 "device '<serial>' not found" 등 — 로그·진행 기록 유출 방지)
pub(crate) fn scrub_serial(e: String, serial: &str) -> String {
    if serial.chars().count() > 6 && e.contains(serial) {
        e.replace(serial, &mask_serial(serial))
    } else {
        e
    }
}

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

/// 루팅 여부 — 프론트 TriState(true | false | "unknown")와 같은 JSON으로 보낸다
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rooted {
    Yes,
    No,
    Unknown,
}

impl Serialize for Rooted {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Rooted::Yes => s.serialize_bool(true),
            Rooted::No => s.serialize_bool(false),
            Rooted::Unknown => s.serialize_str("unknown"),
        }
    }
}

/// `which su` 출력에 su 실행 파일 경로가 있는지 (USB 직접 연결은 오류 문구도 섞여 오므로 경로 형태만 인정)
fn su_visible(su_raw: &str) -> bool {
    su_raw
        .lines()
        .map(str::trim)
        .any(|l| l.starts_with('/') && l.ends_with("/su"))
}

/// su가 보이면 루팅. 안 보여도 부트로더가 잠김으로 확정된 경우에만 "아님" —
/// 언락 상태에서는 su를 셸에 숨긴 Magisk(앱 전용 권한 등)를 구분할 수 없어 판별 불가로 둔다.
/// su 구간 표식이 없으면(출력 끊김) 판별 불가.
fn root_state(su_raw: Option<&str>, bootloader: &str) -> Rooted {
    match su_raw {
        None => Rooted::Unknown,
        Some(raw) if su_visible(raw) => Rooted::Yes,
        Some(_) if bootloader == "locked" => Rooted::No,
        Some(_) => Rooted::Unknown,
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
        _ => return "unknown",
    };
    if locked && rooted {
        return "unknown";
    }
    if locked {
        "locked"
    } else {
        "unlocked"
    }
}

/// `getprop` 덤프를 key→value 맵으로
pub(crate) fn parse_getprop(out: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for line in out.lines() {
        // 형식: [key]: [value]
        let Some(rest) = line.strip_prefix('[') else {
            continue;
        };
        let Some(mid) = rest.find("]: ") else {
            continue;
        };
        let key = &rest[..mid];
        let val = rest[mid + 3..].trim_start_matches('[');
        let val = val.strip_suffix(']').unwrap_or(val);
        map.insert(key.to_string(), val.to_string());
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
    p.get(&format!("{key}.{}", idx + 1))
        .map(|s| s.trim().to_string())
        .unwrap_or_default()
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
    /// "on" = 셀룰러 IMS 음성(VoLTE) / "wifi" = Wi-Fi 통화로만 등록 / "off" = 미등록 / "unknown" = 판별 불가
    volte: &'static str,
    ims: ImsDiagnostic,
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
    /// ro.build.fingerprint — 업데이트 확인에서 같은 기기·지역·버전인지 대조(시리얼 없음)
    fingerprint: String,
    baseband: String,
    observed_at_ms: u64,
    android: String,
    mode: String,
    bootloader: String,
    rooted: Rooted,
    sims: Vec<SimOut>,
    usb: UsbOut,
    /// 언락 사전 조건 (판별 불가 시 None)
    prep: PrepOut,
    /// ro.product.manufacturer가 Sony인지 — Xperia가 아니면 목록에서 제외
    #[serde(skip)]
    sony: bool,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct PrepOut {
    /// 개발자 옵션 활성화 (settings global development_settings_enabled)
    developer_options: Option<bool>,
    /// USB 디버깅 (settings global adb_enabled — ADB로 연결됐다면 사실상 켜짐)
    usb_debugging: Option<bool>,
    /// OEM 잠금 해제 허용 (getprop sys.oem_unlock_allowed)
    oem_unlock_allowed: Option<bool>,
}

fn flag(v: &str) -> Option<bool> {
    match v.trim() {
        "1" => Some(true),
        "0" => Some(false),
        _ => None,
    }
}

/// 기기 상태 조회 (getprop + which su — 모두 읽기 전용)
fn device_status(dev: &mut dyn ADBDeviceExt, serial_hint: &str) -> Result<DeviceOut, String> {
    let raw = shell(
        dev,
        "getprop; echo __SU__; which su || true; echo __ISUB__; \
         dumpsys isub | sed -n '/^Active subscriptions:/,/^All subscriptions:/p' \
           | grep -oE 'simSlotIndex=-?[0-9]+ portIndex=-?[0-9]+ isEmbedded=[01]' || true; echo __IMS__; \
         ims_dump=$(dumpsys activity service com.android.phone/.TelephonyDebugService 2>&1); ims_status=$?; \
         case \"$ims_dump\" in *'Permission Denial'*|*'not found'*|*'No services match'*|*'Error dumping'*|*'Exception'*) ims_status=1;; esac; \
         echo __IMS_STATUS__=$ims_status; printf '%s\\n' \"$ims_dump\" \
           | grep -E 'mPhoneId=|mMmTelCapabilities=|mImsMmTelRegistrationState|mImsRegistrationTech|handleImsRegistered|handleImsRegistering|handleImsUnregistered' || true; echo __DEV__; \
         settings get global development_settings_enabled; settings get global adb_enabled",
    )?;
    let (props_raw, rest) = raw.split_once("__SU__").unwrap_or((&raw, ""));
    // su 구간은 두 표식이 모두 있어야 유효(없으면 루팅 판별 불가)
    let su_raw = rest.split_once("__ISUB__").map(|(su, _)| su);
    let (_, rest) = rest.split_once("__ISUB__").unwrap_or((rest, ""));
    let (isub_raw, rest) = rest.split_once("__IMS__").unwrap_or((rest, ""));
    let (ims_raw, dev_raw) = rest.split_once("__DEV__").unwrap_or((rest, ""));
    let mut dev_lines = dev_raw.lines().map(|l| l.trim()).filter(|l| !l.is_empty());
    let (dev_opt, adb_on) = (
        dev_lines.next().unwrap_or(""),
        dev_lines.next().unwrap_or(""),
    );
    let embedded = parse_embedded_slots(isub_raw);
    let ims_voice = parse_ims_voice(ims_raw);
    let ims_query_ok = ims_raw.lines().any(|l| l.trim() == "__IMS_STATUS__=0");
    let p = parse_getprop(props_raw);
    let get = |k: &str| p.get(k).cloned().unwrap_or_default();

    // 기기 지정 식별자: 서버 모드는 adb 전송 식별자(serial_hint) — with_first_device가 같은 값으로 찾는다.
    // USB 직접 연결(serial_hint 없음)은 ro.serialno — with_first_device가 연결 후 같은 프롭과 대조한다
    let mut serial = serial_hint.to_string();
    if serial.is_empty() {
        serial = get("ro.serialno");
    }
    if serial.is_empty() {
        serial = get("ro.boot.serialno");
    }
    if serial.is_empty() {
        serial = "unknown".into();
    }

    let bootloader = bootloader_state(
        &get("ro.boot.flash.locked"),
        &get("ro.boot.vbmeta.device_state"),
        su_raw.is_some_and(su_visible),
    );
    let rooted = root_state(su_raw, bootloader);

    let sim_state = get("gsm.sim.state");
    let states: Vec<&str> = sim_state.split(',').collect();
    let alphas = [
        slot_prop(&p, "gsm.sim.operator.alpha", 0),
        slot_prop(&p, "gsm.sim.operator.alpha", 1),
    ];
    let numerics = [
        slot_prop(&p, "gsm.sim.operator.numeric", 0),
        slot_prop(&p, "gsm.sim.operator.numeric", 1),
    ];

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
            let ims = ims_diagnostics(&state, ims_query_ok, ims_voice.get(&slot));
            SimOut {
                slot,
                sim_type: if is_esim { "esim" } else { "physical" },
                carrier: loaded.then(|| {
                    if alphas[idx].is_empty() {
                        "통신사 확인 불가".to_string()
                    } else {
                        alphas[idx].clone()
                    }
                }),
                state,
                // IMS 음성 등록 상태(실측) 우선 — 덤프에서 못 읽으면 판별 불가
                volte: match (loaded, ims_voice.get(&slot)) {
                    (true, Some(v)) if ims_query_ok => v.volte(),
                    _ => "unknown",
                },
                ims,
                plmn: numerics[idx].clone(),
            }
        })
        .collect();

    let model = get("ro.product.model");
    let serial_masked = mask_serial(&serial);
    // 펌웨어: ro.build.id(실측 "67.2.A.3.178") — display.id는 " release-keys"가 붙음
    let mut firmware = get("ro.build.id");
    if firmware.is_empty() {
        firmware = get("ro.build.display.id")
            .trim_end_matches(" release-keys")
            .to_string();
    }
    Ok(DeviceOut {
        sony: get("ro.product.manufacturer").eq_ignore_ascii_case("sony"),
        state: "device".into(),
        serial,
        serial_masked,
        product_name: product_name(&get("ro.semc.product.name"), &model),
        model,
        firmware,
        fingerprint: get("ro.build.fingerprint"),
        baseband: get("gsm.version.baseband"),
        observed_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
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
        prep: PrepOut {
            developer_options: flag(dev_opt),
            usb_debugging: flag(adb_on),
            oem_unlock_allowed: flag(&get("sys.oem_unlock_allowed")),
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
        fingerprint: String::new(),
        baseband: String::new(),
        observed_at_ms: 0,
        android: String::new(),
        mode: "android".into(),
        bootloader: "unknown".into(),
        rooted: Rooted::Unknown,
        sims: vec![],
        usb: UsbOut {
            topology: String::new(),
            controller: String::new(),
            link_speed: String::new(),
        },
        prep: PrepOut::default(),
        sony: true,
    }
}

fn device_list_work() -> Result<Vec<DeviceOut>, String> {
    // 1) adb 서버 모드 — 준비된 기기는 상태 조회, 미승인/오프라인 기기는 상태만
    let devs = match server_devices() {
        Server::Failed(e) => return Err(e),
        Server::Devices(devs) => Some(devs),
        Server::NotRunning => None,
    };
    if let Some(devs) = devs {
        let mut out = Vec::with_capacity(devs.len());
        // 미승인 기기는 제조사를 알 수 없음 → PC에 Sony USB 장치가 있을 때만 안내 대상으로.
        // USB 장치 검색은 준비 안 된 기기가 있을 때만 한 번(매 폴링마다 디스크립터를 읽지 않게)
        let mut sony_usb: Option<bool> = None;
        let mut failed = 0usize;
        for entry in &devs {
            if !entry.ready() {
                if *sony_usb.get_or_insert_with(crate::usbmode::sony_usb_present) {
                    out.push(placeholder(
                        &entry.state,
                        entry.serial.clone(),
                        mask_serial(&entry.serial),
                        "Xperia",
                    ));
                }
                continue;
            }
            match device_status(&mut entry.handle(), &entry.serial) {
                Ok(d) if d.sony => out.push(d),
                Ok(_) => eprintln!(
                    "[rust] Xperia가 아닌 기기 제외({})",
                    mask_serial(&entry.serial)
                ),
                Err(e) => {
                    failed += 1;
                    eprintln!(
                        "[rust] 기기 정보 읽기 실패({}): {}",
                        mask_serial(&entry.serial),
                        scrub_serial(e, &entry.serial)
                    )
                }
            }
        }
        eprintln!(
            "[rust] device_list(server) -> {} device(s), 실패 {failed}",
            out.len()
        );
        // 준비된 기기를 하나도 읽지 못했으면 "기기 없음"이 아니라 조회 실패 — 프론트는 연속 실패 시에만 카드를 지운다
        if out.is_empty() && failed > 0 {
            return Err("기기 정보를 읽지 못했습니다".into());
        }
        return Ok(out);
    }

    // 2) USB 직접 연결 — 기기 감지부터
    let found = sony_usb_adb_devices()?;
    if found.is_empty() {
        eprintln!("[rust] device_list(usb) -> 0 device(s)");
        return Ok(vec![]);
    }
    if found.len() > 1 {
        // USB 직접 연결은 기기별 셸을 구분해 열 수 없음 → 식별 정보만 담아 반환 (프론트 다중 기기 경고용)
        eprintln!(
            "[rust] device_list(usb) -> {} device(s), 다중 연결",
            found.len()
        );
        return Ok(found
            .iter()
            .enumerate()
            .map(|(i, info)| {
                let name = info.device_description.trim();
                placeholder(
                    "usb",
                    format!("usb-{i}"),
                    format!("USB #{}", i + 1),
                    if name.is_empty() {
                        "알 수 없는 기기"
                    } else {
                        name
                    },
                )
            })
            .collect());
    }
    let mut guard = lock_usb(Duration::from_secs(30))?;
    // 보관 연결이 끊겼으면 ensure_usb가 다시 연다(읽기 전용 질의라 상태 조회 실패 시 연결만 버린다)
    match ensure_usb(&mut guard) {
        Ok(()) => {}
        // 서버 모드처럼 "USB 디버깅 허용 대기" 안내를 보여 주도록 자리표시 항목으로 돌려준다
        Err(e) if e == USB_UNAUTHORIZED => {
            eprintln!("[rust] device_list(usb) -> USB 디버깅 허용 대기");
            return Ok(vec![placeholder(
                "unauthorized",
                "usb-0".into(),
                "USB #1".into(),
                "Xperia",
            )]);
        }
        Err(e) => return Err(e),
    }
    let dev = guard.as_mut().ok_or("기기 연결을 확인할 수 없습니다")?;
    let result = device_status(dev, "").map(|d| if d.sony { vec![d] } else { vec![] });
    if result.is_err() {
        *guard = None;
    }
    if let Ok(list) = &result {
        eprintln!("[rust] device_list(usb) -> {} device(s)", list.len());
    }
    result
}

/// 연결된 기기의 상태 목록 (읽기 전용)
#[tauri::command]
pub async fn device_list() -> Result<Vec<DeviceOut>, String> {
    guarded(Duration::from_secs(30), device_list_work).await
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
        // 없는 폴더는 0으로 명시, 있는데 du가 결과를 못 내면 줄이 없어 "측정 불가"로 남는다
        s.push_str(&format!(
            "(if [ -e {path} ]; then du -sk {path} 2>/dev/null; else printf '0\t{path}\n'; fi) & "
        ));
    }
    s.push_str(
        "(for l in $(pm list packages -3 -f); do a=${l#package:}; a=${a%=*}; \
         echo P; stat -c %s ${a%/*}/*.apk; done) & \
         wait); echo __DF__; df -k /sdcard",
    );
    s
}

fn parse_storage_output(raw: &str) -> Value {
    // 측정된 폴더만 키를 갖는다 — 빠진 키는 측정 실패(프론트에서 "측정 불가")
    let mut sizes: HashMap<String, u64> = HashMap::new();
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
                            FOLDER_PATHS
                                .iter()
                                .find(|(_, p)| *p == path)
                                .map(|(id, _)| *id),
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
            2 if !t.starts_with("Filesystem") && !t.is_empty() => {
                let parts: Vec<&str> = t.split_whitespace().collect();
                if parts.len() >= 4 {
                    if let Ok(kb) = parts[3].parse::<u64>() {
                        free_kb = kb;
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
    guarded(Duration::from_secs(180), move || storage_sizes_work(serial)).await
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

// ── 폰 설정 화면 열기 (언락 조건 안내 보조) ──
// 폰 화면에 설정 화면만 띄운다 (설정 값은 바꾸지 않음 — 사용자 승인 2026-10-03)

/// screen: "developer" = 개발자 옵션 / "about" = 휴대전화 정보(빌드번호 연타로 개발자 옵션 활성화)
#[tauri::command]
pub async fn open_settings_screen(serial: Option<String>, screen: String) -> Result<(), String> {
    let action = match screen.as_str() {
        "developer" => "android.settings.APPLICATION_DEVELOPMENT_SETTINGS",
        "about" => "android.settings.DEVICE_INFO_SETTINGS",
        _ => return Err("알 수 없는 설정 화면입니다".into()),
    };
    guarded(Duration::from_secs(15), move || {
        with_first_device(&serial, |dev| {
            shell(dev, &format!("am start -a {action}")).map(|_| ())
        })
    })
    .await
}

// ── IMEI 1 (언락 코드 발급용) ──
// Sony 언락 페이지는 듀얼 SIM이면 IMEI 1(SIM 슬롯 1)을 요구한다.
// iphonesubinfo의 트랜잭션 번호는 Android 버전마다 다르고 공식 문서가 없다 → 실측 확인된 호출만 쓰고,
// 결과가 15자리 + Luhn 검증을 통과할 때만 채택 (엉뚱한 값을 IMEI로 쓰지 않도록). IMEI는 로그에 남기지 않는다.

/// (호출, 확인 환경) — getDeviceIdForPhone(phoneId=0 → 슬롯 1)
const IMEI1_CALLS: [(&str, &str); 1] = [(
    "service call iphonesubinfo 4 i32 0 s16 com.android.shell",
    "Android 15 / XQ-DQ44 실측",
)];

/// `service call` Parcel 출력에서 문자열 부분(따옴표 안)의 숫자만
fn parcel_digits(out: &str) -> String {
    let mut s = String::new();
    for line in out.lines() {
        if let (Some(a), Some(b)) = (line.find('\''), line.rfind('\'')) {
            if b > a {
                s.extend(line[a + 1..b].chars().filter(|c| c.is_ascii_digit()));
            }
        }
    }
    s
}

fn luhn_ok(n: &str) -> bool {
    if n.len() != 15 || !n.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let sum: u32 = n
        .bytes()
        .rev()
        .enumerate()
        .map(|(i, b)| {
            let d = (b - b'0') as u32;
            if i % 2 == 1 {
                let x = d * 2;
                if x > 9 {
                    x - 9
                } else {
                    x
                }
            } else {
                d
            }
        })
        .sum();
    sum.is_multiple_of(10)
}

/// 루트 권한 확인 — `su -c id`가 uid=0이면 승인됨 (Magisk 허용 창이 뜨면 사용자가 허용해야 함, 기기 변경 없음)
#[tauri::command]
pub async fn root_check(serial: Option<String>) -> Result<bool, String> {
    guarded(Duration::from_secs(30), move || {
        with_first_device(&serial, |dev| {
            // su 없음(127)·거부(1)는 조회 실패가 아니라 "루트 아님"이다
            let out = crate::device_io::shell_run(dev, "su -c id 2>&1")?;
            Ok(out.code == 0 && String::from_utf8_lossy(&out.stdout).contains("uid=0"))
        })
    })
    .await
}

/// IMEI 1 (전체 값 — 프론트는 마스킹 표시, 복사 버튼에만 사용)
#[tauri::command]
pub async fn read_imei1(serial: Option<String>) -> Result<String, String> {
    guarded(Duration::from_secs(15), move || {
        with_first_device(&serial, |dev| {
            for (call, _) in IMEI1_CALLS.iter() {
                if let Ok(out) = shell(dev, call) {
                    let d = parcel_digits(&out);
                    if luhn_ok(&d) {
                        return Ok(d);
                    }
                }
            }
            Err("IMEI를 읽을 수 없습니다 — 설정 > 휴대전화 정보 > IMEI(SIM 슬롯 1)에서 확인해 주세요".into())
        })
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
    (
        "global",
        "stay_on_while_plugged_in",
        "충전 중 화면 켜짐 유지",
    ),
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
        s.push_str(&format!(
            "echo \"V|{ns}|{key}|$(settings get {ns} {key})\"; "
        ));
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
                if let Some((_, _, label)) = RESTORE_SETTINGS
                    .iter()
                    .find(|(n, k, _)| n == ns && k == key)
                {
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
    #[test]
    fn device_status_exposes_ims_query_failure_without_stale_success() {
        use crate::backup::fake_device::FakeADBDevice;
        for status in ["0", "1", "missing"] {
            let mut dev = FakeADBDevice::new();
            let status_line = if status == "missing" {
                String::new()
            } else {
                format!("__IMS_STATUS__={status}\n")
            };
            dev.answer_shell("getprop;", &format!("[ro.product.manufacturer]: [Sony]\n[ro.product.model]: [XQ-DQ44]\n[gsm.sim.state]: [LOADED,ABSENT]\n[gsm.version.baseband]: [test-baseband]\n__SU__\n__ISUB__\n__IMS__\n{status_line}mPhoneId=0\nmMmTelCapabilities=MmTel Capabilities - [Voice: true SMS: false]\nmImsMmTelRegistrationState = 2\nhandleImsRegistered: imsTransportType=WWAN\n__DEV__\n1\n1\n"));
            let output =
                serde_json::to_value(super::device_status(&mut dev, "SELECTED").unwrap()).unwrap();
            assert_eq!(output["baseband"], "test-baseband");
            assert!(output["observedAtMs"].as_u64().unwrap() > 0);
            assert_eq!(
                output["sims"][0]["volte"],
                if status == "0" { "on" } else { "unknown" }
            );
            assert_eq!(
                output["sims"][0]["ims"]["status"],
                if status == "0" {
                    "registered"
                } else {
                    "query-failed"
                }
            );
            assert_eq!(output["sims"][1]["ims"]["status"], "no-sim");
            assert!(dev
                .shell_calls
                .iter()
                .all(|command| !command.contains("setprop") && !command.contains("reboot")));
        }
    }

    #[test]
    fn usb_approval_wait_is_reported_as_unauthorized() {
        use super::*;
        let waiting = usb_open_error(RustADBError::ADBRequestFailed(UNAUTHORIZED_MARKER.into()));
        assert_eq!(waiting, USB_UNAUTHORIZED);
        let other = usb_open_error(RustADBError::ADBRequestFailed("closed".into()));
        assert!(other.starts_with("기기 연결 실패"));
    }

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
        assert_eq!(
            (o.system_count, o.secure_count, o.global_count),
            (100, 210, 295)
        );
        assert_eq!(o.battery_exempt_apps, 6);
        assert_eq!(o.restore_items.len(), 3);
        assert_eq!(o.restore_items[0].label, "빠른 설정 타일 순서");
        assert!(o.restore_items[0]
            .value
            .contains("custom(com.google.android.gms/.nearby.sharing.SharingTileService)"));
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
        assert_eq!(
            v[0],
            AppFlagOut {
                pkg: "com.instagram.android".into(),
                allow_backup: true,
                has_external_data: true,
                google_backed_up: false
            }
        );
        assert_eq!(
            v[1],
            AppFlagOut {
                pkg: "kr.co.tmoney.tia".into(),
                allow_backup: false,
                has_external_data: false,
                google_backed_up: true
            }
        );
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
        assert_eq!(m[&1].volte(), "off");
        assert_eq!(m[&2].volte(), "unknown"); // capability alone cannot prove registration
        assert!(parse_ims_voice("").is_empty());
    }

    /// 실기기(XQ-DQ44) 덤프 형식 — 등록 로그로 셀룰러/Wi-Fi 통화 구분
    #[test]
    fn parse_ims_voice_wifi_calling() {
        let raw = "\
       mPhoneId=1\n\
       mMmTelCapabilities=MmTel Capabilities - [Voice: true Video: false UT: true SMS: true CALL_COMPOSER: false BUSINESS_COMPOSER_ONLY: false]\n\
        mImsMmTelRegistrationState = 2\n\
        2026-10-03T20:50:08.839782 - handleImsRegistered: onImsMmTelConnected imsRadioTech=WWAN\n\
        2026-10-03T21:10:00.000000 - handleImsRegistered: onImsMmTelConnected imsRadioTech=WLAN\n";
        let m = parse_ims_voice(raw);
        assert_eq!(m[&2].reg_state, Some(2));
        assert_eq!(m[&2].volte(), "wifi"); // 마지막 등록이 WLAN → VoLTE 아님
        let cellular = raw.replace(
            "21:10:00.000000 - handleImsRegistered: onImsMmTelConnected imsRadioTech=WLAN",
            "21:10:00.000000 - handleImsRegistered: onImsMmTelConnected imsRadioTech=WWAN",
        );
        assert_eq!(parse_ims_voice(&cellular)[&2].volte(), "on");
    }

    #[test]
    fn ims_capability_and_old_radio_logs_cannot_prove_current_registration() {
        let dump = "mPhoneId=0\nmMmTelCapabilities=MmTel Capabilities - [Voice: true]\nhandleImsRegistered: imsRadioTech=WWAN\n";
        for (state, expected) in [(0, "off"), (1, "unknown"), (2, "on"), (-1, "unknown")] {
            let raw = format!("{dump}mImsMmTelRegistrationState = {state}\n");
            assert_eq!(parse_ims_voice(&raw)[&1].volte(), expected);
        }
        assert_eq!(parse_ims_voice(dump)[&1].volte(), "unknown");
        let unknown_radio = "mPhoneId=0\nmMmTelCapabilities=MmTel Capabilities - [Voice: true]\nmImsMmTelRegistrationState = 2\n";
        assert_eq!(parse_ims_voice(unknown_radio)[&1].volte(), "unknown");
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
        let q =
            parse_getprop("[gsm.sim.operator.alpha]: [KT]\n[gsm.sim.operator.alpha.2]: [LG U+]\n");
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
        // 폴더 목록에 없는 경로·권한 오류 줄은 무시, 측정 결과가 없는 폴더는 키 없음(측정 불가)
        assert!(v.get("music").is_none());
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

    /// service call 출력 형식 (값은 Luhn을 만족하는 테스트용 번호)
    #[test]
    fn parcel_digits_and_luhn() {
        let out = concat!(
            "Result: Parcel(\n",
            "  0x00000000: 00000000 0000000f 00390034 00310030 '........4.9.0.1.'\n",
            "  0x00000010: 00340035 00300032 00320033 00370033 '5.4.2.0.3.2.3.7.'\n",
            "  0x00000020: 00310035 00000038                   '5.1.8.....      ')\n",
        );
        assert_eq!(parcel_digits(out), "490154203237518");
        assert!(luhn_ok("490154203237518")); // 표준 Luhn 예시 IMEI
        assert!(!luhn_ok("490154203237519"));
        assert!(!luhn_ok("12345"));
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
        assert_eq!(bootloader_state("0", "", false), "unknown"); // 한쪽만으로는 확정하지 않음
    }

    #[test]
    fn root_state_is_unknown_unless_su_is_seen_or_bootloader_is_locked() {
        assert_eq!(
            root_state(Some("/system/bin/su\n"), "unlocked"),
            Rooted::Yes
        );
        assert_eq!(root_state(Some("\n"), "locked"), Rooted::No);
        // 언락 상태에서 su가 안 보이면 셸에 숨긴 루팅과 구분할 수 없다
        assert_eq!(root_state(Some(""), "unlocked"), Rooted::Unknown);
        assert_eq!(root_state(Some(""), "unknown"), Rooted::Unknown);
        // USB 직접 연결에서 섞여 오는 오류 문구는 su 경로가 아니다
        assert_eq!(
            root_state(Some("/system/bin/sh: which: not found\n"), "locked"),
            Rooted::No
        );
        // 출력이 끊겨 su 구간이 없으면 판별 불가
        assert_eq!(root_state(None, "locked"), Rooted::Unknown);
        assert_eq!(
            serde_json::to_value(Rooted::Unknown).unwrap(),
            json!("unknown")
        );
        assert_eq!(serde_json::to_value(Rooted::No).unwrap(), json!(false));
    }

    #[test]
    fn product_name_prefers_reported() {
        assert_eq!(product_name("Xperia 1 V", "XQ-DQ44"), "Xperia 1 V");
        assert_eq!(product_name("", "XQ-EC72"), "Xperia 1 VI");
        assert_eq!(product_name("", "XQ-ZZ99"), "XQ-ZZ99");
    }
}
