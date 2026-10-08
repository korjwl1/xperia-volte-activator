//! Native Rust EFS/NV. No subprocess, .NET dependency or automatic DIAG switching.
//! Every phone operation (including reads/setup) is behind default-disabled efs-write.
pub mod config;
mod device;
mod diag;
mod engine;
mod error;
mod hdlc;
mod manifest;
mod session;
#[cfg(test)]
mod tests;
mod transport;
pub mod volte;
mod wire;

use crate::events::Events;
use error::{Error, Result};
use serde::Serialize;
use session::Session;
use std::{
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::Duration,
};
static OWNER: Mutex<Option<Arc<AtomicBool>>> = Mutex::new(None);
/// DIAG 포트 자동 선택(2026-10-08 사용자 결정) — 설정의 포트가 `AUTO`면 Qualcomm(05C6) 직렬 포트 중
/// hello/query에 응답하는 첫 포트를 쓴다(XQ-DQ44: MSM·MDM·CNSS 중 MSM). 같은 실행 동안은 찾은 포트를 유지하고 DIAG 전환 때 비운다.
static DETECTED_PORT: Mutex<Option<String>> = Mutex::new(None);
pub const AUTO_PORT: &str = "AUTO";
fn detected_port() -> Option<String> {
    DETECTED_PORT.lock().ok().and_then(|p| p.clone())
}
fn diag_candidates() -> Vec<String> {
    let mut ports: Vec<(u32, String)> = serialport::available_ports()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| matches!(&p.port_type, serialport::SerialPortType::UsbPort(u) if u.vid == 0x05C6))
        .filter_map(|p| Some((p.port_name.strip_prefix("COM")?.parse().ok()?, p.port_name)))
        .collect();
    ports.sort();
    ports.into_iter().map(|(_, name)| name).collect()
}
fn resolve_port(port: &str, cancel: &Arc<AtomicBool>) -> Result<String> {
    if port != AUTO_PORT {
        return Ok(port.to_string());
    }
    if let Some(found) = detected_port() {
        return Ok(found);
    }
    // DIAG 전환 직후에는 포트가 늦게 나타난다 — 최대 60초 동안 다시 찾는다
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    loop {
        for name in diag_candidates() {
            if cancel.load(Ordering::Acquire) {
                return Err(Error::new("cancelled", "DIAG port", "Cancelled while searching DIAG port"));
            }
            let Ok(io) = transport::Com::open(&name) else { continue };
            let mut session = Session::new(io, cancel.clone(), Duration::from_millis(3000));
            let ok = session.initialize().and_then(|_| session.query()).is_ok();
            let _ = session.cleanup();
            if ok {
                if let Ok(mut slot) = DETECTED_PORT.lock() {
                    *slot = Some(name.clone());
                }
                return Ok(name);
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error::new(
                "portNotFound",
                "DIAG port",
                "응답하는 Qualcomm DIAG 포트를 찾지 못했습니다 — DIAG 드라이버(qcser) 설치와 폰의 DIAG 전환을 확인하세요",
            ));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn gate() -> Result<()> {
    if cfg!(feature = "efs-write") {
        Ok(())
    } else {
        Err(Error::new(
            "disabled",
            "EFS",
            "Device execution disabled: efs-write is off",
        ))
    }
}
struct Operation {
    cancel: Arc<AtomicBool>,
    _device: crate::device_io::WriteOperation,
}
impl Operation {
    fn acquire() -> Result<Self> {
        let mut owner = OWNER
            .lock()
            .map_err(|_| Error::new("internal", "EFS", "Ownership lock poisoned"))?;
        if owner.is_some() {
            return Err(Error::new(
                "busy",
                "EFS",
                "Another EFS operation owns the session",
            ));
        }
        let device = crate::device_io::WriteOperation::acquire()
            .map_err(|e| Error::new("busy", "EFS", e))?;
        let cancel = Arc::new(AtomicBool::new(false));
        *owner = Some(cancel.clone());
        Ok(Self {
            cancel,
            _device: device,
        })
    }
}
/// Filesystem parsing and hashing never block the async command executor.
async fn offline<R: Send + 'static>(
    work: impl FnOnce() -> Result<R> + Send + 'static,
) -> Result<R> {
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|e| Error::io("EFS preparation worker", e))?
}
impl Drop for Operation {
    fn drop(&mut self) {
        if let Ok(mut owner) = OWNER.lock() {
            *owner = None;
        }
    }
}
async fn execute<R: Send + 'static>(
    port: String,
    operation: Operation,
    f: impl FnOnce(&mut Session<transport::Com>) -> Result<R> + Send + 'static,
) -> Result<R> {
    tauri::async_runtime::spawn_blocking(move || {
        if operation.cancel.load(Ordering::Acquire) {
            return Err(Error::new("cancelled", "EFS", "Cancelled before port open"));
        }
        let port = resolve_port(&port, &operation.cancel)?;
        let io = transport::Com::open(&port)?;
        let mut session = Session::new(io, operation.cancel.clone(), Duration::from_millis(7000));
        let result = f(&mut session);
        let cleanup = session.cleanup();
        match result {
            Ok(v) if cleanup.is_empty() => Ok(v),
            Ok(_) => {
                let mut e = Error::new("cleanupFailed", "EFS", "Descriptors could not be closed");
                e.cleanup = cleanup;
                Err(e)
            }
            Err(mut e) => {
                e.cleanup.extend(cleanup);
                Err(e)
            }
        }
        // operation drop releases ownership only after session and port are dropped.
    })
    .await
    .map_err(|e| Error::io("EFS worker", e))?
}
fn progress(app: Events) -> impl FnMut(engine::Progress) {
    move |p| {
        let _ = app.emit("efs:progress", &p);
        let _ = app.emit(
            "efs:log",
            serde_json::json!({"cmd":p.operation,"line":format!("{} ({}/{})",p.file,p.n,p.total)}),
        );
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCheck {
    version: &'static str,
    path: &'static str,
    native: bool,
    device_execution: bool,
    root_execution: bool,
    fastboot_execution: bool,
}
#[tauri::command]
pub async fn efs_tool_check() -> Result<ToolCheck> {
    Ok(ToolCheck {
        version: "native-rust-v1",
        path: "built-in",
        native: true,
        device_execution: cfg!(feature = "efs-write"),
        root_execution: cfg!(feature = "root-write"),
        fastboot_execution: cfg!(feature = "fastboot-write"),
    })
}
#[derive(Serialize)]
pub struct PreflightOut {
    log: Vec<String>,
    errors: Vec<String>,
    warnings: Vec<String>,
    parameters: Vec<u32>,
}
#[tauri::command]
pub async fn efs_preflight(port: String) -> Result<PreflightOut> {
    gate()?;
    let owner = Operation::acquire()?;
    execute(port, owner, |s| {
        let warnings = s.initialize()?;
        let parameters = s.query()?;
        Ok(PreflightOut {
            log: vec!["Native EFS hello/query succeeded".into()]
                .into_iter()
                .chain(detected_port().map(|p| format!("DIAG 포트 {p} 자동 선택")))
                .collect(),
            errors: vec![],
            warnings,
            parameters,
        })
    })
    .await
}
#[tauri::command]
pub async fn efs_upload(
    app: tauri::AppHandle,
    port: String,
    preset_dir: String,
) -> Result<engine::UploadOut> {
    efs_upload_with_events(Events::desktop(app), port, preset_dir).await
}

pub(crate) async fn efs_upload_with_events(
    app: Events,
    port: String,
    preset_dir: String,
) -> Result<engine::UploadOut> {
    gate()?;
    let owner = Operation::acquire()?;
    let plan = offline(move || manifest::Plan::load_approved(&PathBuf::from(preset_dir))).await?;
    execute(port, owner, move |s| {
        let setup = s.initialize()?;
        let mut out = engine::upload(s, &plan, &mut progress(app))?;
        out.warnings.extend(setup_warnings(setup));
        Ok(out)
    })
    .await
}
#[tauri::command]
pub async fn efs_verify(
    app: tauri::AppHandle,
    port: String,
    preset_dir: String,
) -> Result<engine::VerifyReport> {
    efs_verify_with_events(Events::desktop(app), port, preset_dir).await
}

pub(crate) async fn efs_verify_with_events(
    app: Events,
    port: String,
    preset_dir: String,
) -> Result<engine::VerifyReport> {
    gate()?;
    let owner = Operation::acquire()?;
    let plan = offline(move || manifest::Plan::load_approved(&PathBuf::from(preset_dir))).await?;
    execute(port, owner, move |s| {
        let setup = s.initialize()?;
        let mut out = engine::verify(s, &plan, &mut progress(app))?;
        out.warnings.extend(setup_warnings(setup));
        Ok(out)
    })
    .await
}
#[tauri::command]
pub async fn efs_snapshot(
    app: tauri::AppHandle,
    port: String,
    preset_dir: String,
    dest: String,
) -> Result<engine::SnapshotOut> {
    efs_snapshot_with_events(Events::desktop(app), port, preset_dir, dest).await
}

pub(crate) async fn efs_snapshot_with_events(
    app: Events,
    port: String,
    preset_dir: String,
    dest: String,
) -> Result<engine::SnapshotOut> {
    gate()?;
    let owner = Operation::acquire()?;
    let plan = offline(move || manifest::Plan::load_approved(&PathBuf::from(preset_dir))).await?;
    execute(port, owner, move |s| {
        let setup = s.initialize()?;
        let mut out = engine::snapshot(s, &plan, &PathBuf::from(dest), &mut progress(app))?;
        out.warnings.extend(setup_warnings(setup));
        Ok(out)
    })
    .await
}
#[tauri::command]
pub async fn efs_rollback(
    app: tauri::AppHandle,
    port: String,
    snapshot: String,
) -> Result<engine::UploadOut> {
    efs_rollback_with_events(Events::desktop(app), port, snapshot).await
}

pub(crate) async fn efs_rollback_with_events(
    app: Events,
    port: String,
    snapshot: String,
) -> Result<engine::UploadOut> {
    gate()?;
    let owner = Operation::acquire()?;
    // Validate all before-images and hashes before opening the port or mutating anything.
    let plan = offline(move || engine::load_snapshot(&PathBuf::from(snapshot))).await?;
    execute(port, owner, move |s| {
        let setup = s.initialize()?;
        let mut out = engine::rollback(s, &plan, &mut progress(app))?;
        out.warnings.extend(setup_warnings(setup));
        Ok(out)
    })
    .await
}
fn setup_warnings(warnings: Vec<String>) -> Vec<engine::Warning> {
    warnings
        .into_iter()
        .map(|message| engine::Warning {
            code: "setupUnsupported".into(),
            target: "DIAG setup".into(),
            message,
        })
        .collect()
}
#[tauri::command]
pub async fn efs_validate_presets(preset_dirs: Vec<String>) -> Result<()> {
    if preset_dirs.len() > 2 {
        return Err(Error::new("limit", "preflight", "At most two slot presets"));
    }
    offline(move || {
        let plans = preset_dirs
            .into_iter()
            .map(|dir| manifest::Plan::load_approved(&PathBuf::from(dir)))
            .collect::<Result<Vec<_>>>()?;
        manifest::validate_set(&plans)
    })
    .await
}
#[tauri::command]
pub async fn efs_diag_open(serial: String) -> Result<()> {
    gate()?;
    let owner = Operation::acquire()?;
    if serial.is_empty() {
        return Err(Error::new(
            "invalidDevice",
            "DIAG switch",
            "Explicit ADB serial required",
        ));
    }
    // 새 DIAG 전환이면 포트를 다시 찾는다
    if let Ok(mut slot) = DETECTED_PORT.lock() {
        *slot = None;
    }
    tauri::async_runtime::spawn_blocking(move || {
        crate::adb::with_first_device(&Some(serial), |dev| diag::open(dev, &owner.cancel)).map_err(
            |message| {
                Error::new(
                    if owner.cancel.load(Ordering::Acquire) {
                        "cancelled"
                    } else {
                        "io"
                    },
                    "DIAG switch",
                    message,
                )
            },
        )
    })
    .await
    .map_err(|e| Error::io("DIAG worker", e))?
}
#[tauri::command]
pub async fn efs_cancel() -> Result<()> {
    // Cancellation remains available even in a disabled build; never accesses a device.
    let owner = OWNER
        .lock()
        .map_err(|_| Error::new("internal", "cancel", "Ownership lock poisoned"))?;
    if let Some(token) = owner.as_ref() {
        token.store(true, Ordering::Release);
    }
    Ok(())
}
