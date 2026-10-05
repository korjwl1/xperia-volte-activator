//! Developer entry point. Dispatches the SAME functions used by Tauri; no phone algorithm lives here.
use crate::events::Events;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::ffi::OsString;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const MAX_REQUEST: usize = 1024 * 1024;
const MAX_RECORD: usize = 16 * 1024 * 1024;

macro_rules! requests {
    ($($variant:ident ($name:literal, $write:expr, $enabled:expr) { $($field:ident : $ty:ty = $example:expr),* $(,)? })*) => {
        #[derive(Deserialize)]
        #[serde(tag = "command", content = "args", rename_all_fields = "camelCase", deny_unknown_fields)]
        enum Request {
            $(#[serde(rename = $name)] $variant { $($field: $ty),* },)*
        }
        impl Request {
            fn policy(&self) -> (&'static str, bool, bool) {
                match self { $(Self::$variant { .. } => ($name, $write, $enabled),)* }
            }
        }
        fn catalog() -> Value {
            json!([$(json!({"command": $name, "requiresDeviceWrite": $write,
                "enabledInBuild": $enabled, "example": {"command": $name,
                "args": {$(stringify!($field): $example),*}}})),*])
        }
    };
}

// CLI args follow Tauri camelCase. Catalog generation uses snake_case field names converted below.
requests! {
    AdbStatus("adb_status", false, true) {}
    DeviceList("device_list", false, true) {}
    UsbModes("usb_modes", false, true) {}
    EnvCheck("env_check", false, true) {}
    ToolCheck("efs_tool_check", false, true) {}
    RootCheck("root_check", false, true) { serial: String = "sha256:<serialKey>" }
    StorageSizes("storage_sizes", false, true) { serial: String = "sha256:<serialKey>" }
    AppFlags("app_flags", false, true) { serial: String = "sha256:<serialKey>" }
    SettingsOverview("settings_overview", false, true) { serial: String = "sha256:<serialKey>" }
    FirmwareVersions("firmware_versions", false, true) { serial: String = "sha256:<serialKey>" }
    FirmwareFetch("firmware_fetch", false, true) {
        serial: String = "sha256:<serialKey>", partition: String = "init_boot",
        version: Option<String> = Value::Null, dest: Option<String> = Value::Null
    }
    FirmwareDirCheck("firmware_dir_check", false, true) { dir: String = "<absolute firmware folder>", partition: String = "init_boot" }
    BootImageCheck("boot_image_check", false, true) { serial: String = "sha256:<serialKey>", path: String = "<stock image path>", fingerprint: String = "<extraction fingerprint>" }
    MagiskPrepare("magisk_prepare", false, true) {}
    MagiskPatch("magisk_patch", true, cfg!(feature = "root-write")) { request: crate::magisk::MagiskPatchRequest = json!({"serial":"sha256:<serialKey>", "apkPath":"<verified APK>", "imagePath":"<stock image>", "partition":"init_boot", "imageSha256":"<stock SHA256>", "fingerprint":"<extraction fingerprint>", "apkSha256":"<APK SHA256>"}) }
    MagiskInstall("magisk_install", true, cfg!(feature = "root-write")) { serial: String = "sha256:<serialKey>", apk_path: String = "<verified APK>", apk_sha256: String = "<APK SHA256>" }
    RootReboot("root_reboot", true, cfg!(any(feature = "root-write", feature = "fastboot-write", feature = "efs-write"))) { serial: String = "sha256:<serialKey>", target: String = "os" }
    FastbootGetvar("fastboot_getvar", false, true) {}
    FastbootUnlock("fastboot_unlock", true, cfg!(feature = "fastboot-write")) { code: String = "<unlock code, private input only>", confirm: bool = true, expected_serial: String = "sha256:<serialKey>" }
    FastbootFlash("fastboot_flash", true, cfg!(feature = "fastboot-write")) {
        partition: String = "init_boot", path: String = "<checked image>", confirm: bool = true,
        expected_serial: String = "sha256:<serialKey>", expected_sha256: String = "<checked SHA256>"
    }
    FastbootLock("fastboot_lock", true, cfg!(feature = "fastboot-write")) { confirm: bool = true, partition: String = "init_boot", stock_path: String = "<stock image>", expected_serial: String = "sha256:<serialKey>" }
    FastbootReboot("fastboot_reboot", true, cfg!(feature = "fastboot-write")) { target: String = "os", expected_serial: String = "sha256:<serialKey>" }
    RelockGateCheck("relock_gate_check", false, true) { partition: String = "init_boot", stock_path: String = "<stock image>", device_key: Option<String> = Value::Null }
    EfsValidatePresets("efs_validate_presets", false, true) { preset_dirs: Vec<String> = json!(["<approved balance preset directory>"]) }
    EfsDiagOpen("efs_diag_open", true, cfg!(feature = "efs-write")) { serial: String = "sha256:<serialKey>" }
    EfsPreflight("efs_preflight", true, cfg!(feature = "efs-write")) { port: String = "COM<number>" }
    EfsSnapshot("efs_snapshot", true, cfg!(feature = "efs-write")) { port: String = "COM<number>", preset_dir: String = "<approved balance preset>", dest: String = "<new snapshot folder>" }
    EfsUpload("efs_upload", true, cfg!(feature = "efs-write")) { port: String = "COM<number>", preset_dir: String = "<approved balance preset>" }
    EfsVerify("efs_verify", true, cfg!(feature = "efs-write")) { port: String = "COM<number>", preset_dir: String = "<approved balance preset>" }
    EfsRollback("efs_rollback", true, cfg!(feature = "efs-write")) { port: String = "COM<number>", snapshot: String = "<complete snapshot folder>" }
    VoltePropsSet("volte_props_set", true, cfg!(feature = "efs-write")) { serial: String = "sha256:<serialKey>" }
    BackupPrepare("backup_prepare", false, true) { serial: String = "sha256:<serialKey>", dest: String = "<existing backup parent>" }
    BackupRun("backup_run", false, true) { serial: String = "sha256:<serialKey>", items: Vec<String> = json!(["dcim"]), dest: String = "<existing backup parent>", resume_dir: Option<String> = Value::Null, run_id: String = "backup-test-1" }
    BackupManifestCheck("backup_manifest_check", false, true) { dir: String = "<backup folder>" }
    ContactsRestoreCheck("contacts_restore_check", false, true) { serial: String = "sha256:<serialKey>", dir: String = "<backup folder>" }
    SmsiePrepare("smsie_prepare", true, true) { serial: String = "sha256:<serialKey>", download: bool = true }
    SmsieCollect("smsie_collect", true, true) { serial: String = "sha256:<serialKey>", backup_dir: String = "<backup folder>" }
    RestoreRun("restore_run", true, true) { serial: String = "sha256:<serialKey>", dir: String = "<backup folder>", items: Vec<String> = json!(["dcim"]) }
    SmsieRestoreStage("smsie_restore_stage", true, true) { serial: String = "sha256:<serialKey>", dir: String = "<backup folder>", items: Vec<String> = json!(["sms", "calllog"]) }
    SmsieRestoreFinish("smsie_restore_finish", true, true) { serial: String = "sha256:<serialKey>" }
}

fn camel(s: &str) -> String {
    let mut parts = s.split('_');
    let mut result = parts.next().unwrap_or_default().to_string();
    for part in parts {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            result.extend(first.to_uppercase());
        }
        result.push_str(chars.as_str());
    }
    result
}

fn commands() -> Value {
    let mut list = catalog();
    for command in list.as_array_mut().expect("catalog array") {
        let args = command["example"]["args"]
            .as_object_mut()
            .expect("args object");
        *args = std::mem::take(args)
            .into_iter()
            .map(|(k, v)| (camel(&k), v))
            .collect();
    }
    list
}

fn decode(value: Value) -> Result<Request, String> {
    // Never echo a serde error containing an operator's unlock code/serial.
    let req: Request = serde_json::from_value(value.clone())
        .map_err(|_| "요청 command/args 형식이 잘못됐습니다. commands의 예제를 확인하세요")?;
    fn validate_ids(value: &Value) -> Result<(), String> {
        if let Some(obj) = value.as_object() {
            for (key, value) in obj {
                if matches!(key.as_str(), "serial" | "expectedSerial")
                    && value.as_str().is_none_or(|s| s.trim().is_empty())
                {
                    return Err("작업 대상 serial 또는 sha256:serialKey를 명시하세요".into());
                }
                validate_ids(value)?;
            }
        }
        Ok(())
    }
    validate_ids(&value)?;
    Ok(req)
}

fn authorize(req: &Request, allow_write: bool) -> Result<(), String> {
    let (_, writes, enabled) = req.policy();
    if !enabled {
        return Err("이 단계의 쓰기 Cargo feature가 꺼져 있습니다".into());
    }
    if writes && !allow_write {
        return Err("기기 변경 단계에는 --allow-device-write를 명시하세요".into());
    }
    Ok(())
}

fn packed<T: serde::Serialize, E: serde::Serialize>(result: Result<T, E>) -> Result<Value, Value> {
    match result {
        Ok(value) => serde_json::to_value(value).map_err(|_| json!("결과 직렬화 실패")),
        Err(error) => Err(serde_json::to_value(error).unwrap_or(json!("오류 직렬화 실패"))),
    }
}

async fn dispatch(req: Request, events: Events) -> Result<Value, Value> {
    use crate::{adb, backup, boot_image, efs, env, fastboot, firmware, magisk, usbmode};
    match req {
        Request::AdbStatus {} => packed(adb::adb_status().await),
        Request::DeviceList {} => packed(adb::device_list().await),
        Request::UsbModes {} => packed(usbmode::usb_modes().await),
        Request::EnvCheck {} => packed(env::env_check().await),
        Request::ToolCheck {} => packed(efs::efs_tool_check().await),
        Request::RootCheck { serial } => packed(adb::root_check(Some(serial)).await),
        Request::StorageSizes { serial } => packed(adb::storage_sizes(Some(serial)).await),
        Request::AppFlags { serial } => packed(adb::app_flags(Some(serial)).await),
        Request::SettingsOverview { serial } => packed(adb::settings_overview(Some(serial)).await),
        Request::FirmwareVersions { serial } => {
            packed(firmware::firmware_versions(Some(serial)).await)
        }
        Request::FirmwareFetch {
            serial,
            partition,
            version,
            dest,
        } => packed(firmware::firmware_fetch(Some(serial), partition, version, dest).await),
        Request::FirmwareDirCheck { dir, partition } => {
            packed(firmware::firmware_dir_check(dir, partition).await)
        }
        Request::BootImageCheck {
            serial,
            path,
            fingerprint,
        } => packed(boot_image::boot_image_check(serial, path, fingerprint).await),
        Request::MagiskPrepare {} => packed(magisk::magisk_prepare().await),
        Request::MagiskPatch { request } => {
            packed(magisk::magisk_patch_with_events(events, request).await)
        }
        Request::MagiskInstall {
            serial,
            apk_path,
            apk_sha256,
        } => packed(magisk::magisk_install(Some(serial), apk_path, apk_sha256).await),
        Request::RootReboot { serial, target } => {
            packed(magisk::root_reboot(Some(serial), target).await)
        }
        Request::FastbootGetvar {} => packed(fastboot::fastboot_getvar_with_events(events).await),
        Request::FastbootUnlock {
            code,
            confirm,
            expected_serial,
        } => packed(
            fastboot::fastboot_unlock_with_events(events, code, confirm, expected_serial).await,
        ),
        Request::FastbootFlash {
            partition,
            path,
            confirm,
            expected_serial,
            expected_sha256,
        } => packed(
            fastboot::fastboot_flash_with_events(
                events,
                partition,
                path,
                confirm,
                expected_serial,
                expected_sha256,
            )
            .await,
        ),
        Request::FastbootLock {
            confirm,
            partition,
            stock_path,
            expected_serial,
        } => packed(
            fastboot::fastboot_lock_with_events(
                events,
                confirm,
                partition,
                stock_path,
                expected_serial,
            )
            .await,
        ),
        Request::FastbootReboot {
            target,
            expected_serial,
        } => packed(fastboot::fastboot_reboot_with_events(events, target, expected_serial).await),
        Request::RelockGateCheck {
            partition,
            stock_path,
            device_key,
        } => packed(fastboot::relock_gate_check(partition, stock_path, device_key).await),
        Request::EfsValidatePresets { preset_dirs } => {
            packed(efs::efs_validate_presets(preset_dirs).await)
        }
        Request::EfsDiagOpen { serial } => packed(efs::efs_diag_open(serial).await),
        Request::EfsPreflight { port } => packed(efs::efs_preflight(port).await),
        Request::EfsSnapshot {
            port,
            preset_dir,
            dest,
        } => packed(efs::efs_snapshot_with_events(events, port, preset_dir, dest).await),
        Request::EfsUpload { port, preset_dir } => {
            packed(efs::efs_upload_with_events(events, port, preset_dir).await)
        }
        Request::EfsVerify { port, preset_dir } => {
            packed(efs::efs_verify_with_events(events, port, preset_dir).await)
        }
        Request::EfsRollback { port, snapshot } => {
            packed(efs::efs_rollback_with_events(events, port, snapshot).await)
        }
        Request::VoltePropsSet { serial } => {
            packed(efs::volte::volte_props_set(Some(serial)).await)
        }
        Request::BackupPrepare { serial, dest } => {
            packed(backup::backup_prepare(Some(serial), dest).await)
        }
        Request::BackupRun {
            serial,
            items,
            dest,
            resume_dir,
            run_id,
        } => packed(
            backup::backup_run_with_events(events, Some(serial), items, dest, resume_dir, run_id)
                .await,
        ),
        Request::BackupManifestCheck { dir } => packed(backup::backup_manifest_check(dir).await),
        Request::ContactsRestoreCheck { serial, dir } => {
            packed(backup::contacts_restore_check(Some(serial), dir).await)
        }
        Request::SmsiePrepare { serial, download } => {
            packed(backup::smsie_prepare(Some(serial), download).await)
        }
        Request::SmsieCollect { serial, backup_dir } => {
            packed(backup::smsie_collect(Some(serial), backup_dir).await)
        }
        Request::RestoreRun { serial, dir, items } => {
            packed(backup::restore_run_with_events(events, Some(serial), dir, items).await)
        }
        Request::SmsieRestoreStage { serial, dir, items } => {
            packed(backup::smsie_restore_stage(Some(serial), dir, items).await)
        }
        Request::SmsieRestoreFinish { serial } => {
            packed(backup::smsie_restore_finish(Some(serial)).await)
        }
    }
}

fn serial_key(s: &str) -> String {
    hex::encode(Sha256::digest(s.trim().as_bytes()))
}

#[derive(Clone, Default)]
struct Redactor {
    secrets: Vec<String>,
}
impl Redactor {
    fn learn(&mut self, value: &Value) {
        match value {
            Value::Object(obj) => {
                for (key, val) in obj {
                    if private_key(key)
                        || (key == "code" && val.as_str().is_some_and(unlock_secret))
                    {
                        if let Some(s) = val
                            .as_str()
                            .filter(|s| !s.is_empty() && !s.starts_with("sha256:"))
                        {
                            self.secrets.push(s.to_owned());
                            if let Some(code) =
                                s.strip_prefix("0x").or_else(|| s.strip_prefix("0X"))
                            {
                                self.secrets.push(code.to_owned());
                            }
                        }
                    }
                    self.learn(val);
                }
            }
            Value::Array(array) => {
                for val in array {
                    self.learn(val);
                }
            }
            _ => {}
        }
    }
    fn clean(&self, value: &Value) -> Value {
        match value {
            Value::Object(obj) => {
                let mut output = serde_json::Map::new();
                for (key, val) in obj {
                    if matches!(key.as_str(), "serial" | "serialno") {
                        if let Some(serial) = val.as_str().filter(|s| !s.is_empty()) {
                            output.insert("serialKey".into(), json!(serial_key(serial)));
                        }
                        output.insert(key.clone(), json!("[마스킹]"));
                    } else if private_key(key)
                        || (key == "code" && val.as_str().is_some_and(unlock_secret))
                    {
                        output.insert(key.clone(), json!("[마스킹]"));
                    } else {
                        output.insert(key.clone(), self.clean(val));
                    }
                }
                Value::Object(output)
            }
            Value::Array(array) => Value::Array(array.iter().map(|v| self.clean(v)).collect()),
            Value::String(s) => {
                let mut s = s.clone();
                for secret in &self.secrets {
                    let lower = s.to_ascii_lowercase();
                    for (at, _) in lower
                        .match_indices(&secret.to_ascii_lowercase())
                        .collect::<Vec<_>>()
                        .into_iter()
                        .rev()
                    {
                        s.replace_range(at..at + secret.len(), "[마스킹]");
                    }
                }
                json!(s)
            }
            _ => value.clone(),
        }
    }
}

fn private_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("imei")
        || key.contains("meid")
        || matches!(
            key.as_str(),
            "serial" | "serialno" | "serial-number" | "expectedserial" | "unlockcode"
        )
}

fn unlock_secret(value: &str) -> bool {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    value.len() == 16 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

// An explicit hashed selector lets AI select a device without printing/saving its raw serial.
async fn resolve_targets(value: &mut Value, events: Events) -> Result<(), String> {
    let args = value
        .get_mut("args")
        .and_then(Value::as_object_mut)
        .ok_or("args 객체가 필요합니다")?;
    let target_args = if args.contains_key("request") {
        args.get_mut("request")
            .and_then(Value::as_object_mut)
            .ok_or("request 객체가 필요합니다")?
    } else {
        args
    };
    for (name, target) in target_args {
        if !matches!(name.as_str(), "serial" | "expectedSerial") {
            continue;
        }
        let token = target.as_str().ok_or("serial은 문자열이어야 합니다")?;
        let Some(key) = token.strip_prefix("sha256:") else {
            continue;
        };
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err("serialKey는 64자리 SHA256이어야 합니다".into());
        }
        let serial = if name == "expectedSerial" {
            let vars = crate::fastboot::fastboot_getvar_with_events(events.clone()).await?;
            vars.get("serialno")
                .filter(|s| serial_key(s).eq_ignore_ascii_case(key))
                .cloned()
                .ok_or("선택한 fastboot 기기와 serialKey가 다릅니다")?
        } else {
            let devices = serde_json::to_value(crate::adb::device_list().await?)
                .map_err(|e| e.to_string())?;
            let matches: Vec<&str> = devices
                .as_array()
                .ok_or("기기 목록 형식 오류")?
                .iter()
                .filter_map(|d| d.get("serial").and_then(Value::as_str))
                .filter(|s| serial_key(s).eq_ignore_ascii_case(key))
                .collect();
            if matches.len() != 1 {
                return Err("serialKey에 맞는 ADB 기기 한 대를 확인하지 못했습니다".into());
            }
            matches[0].to_string()
        };
        *target = json!(serial);
    }
    Ok(())
}

struct RunLog {
    file: std::fs::File,
    redactor: Redactor,
    io_error: Option<String>,
}
impl RunLog {
    fn emit(&mut self, value: Value) -> Result<(), String> {
        let line = self.append(value)?;
        // Broken stdout must not interrupt an in-flight flash; the file is the durable result.
        let _ = std::io::stdout().lock().write_all(line.as_bytes());
        Ok(())
    }

    fn append(&mut self, value: Value) -> Result<String, String> {
        self.redactor.learn(&value);
        let value = self.redactor.clean(&value);
        let line = format!("{value}\n");
        if let Err(e) = self
            .file
            .write_all(line.as_bytes())
            .and_then(|_| self.file.flush())
        {
            let error = format!("실행 로그 저장 실패: {e}");
            self.io_error = Some(error.clone());
            return Err(error);
        }
        Ok(line)
    }
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}
fn write_record(path: &Path, value: &Value) -> Result<(), String> {
    crate::storage::atomic_write(
        path,
        &serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?,
    )
}
fn build_info() -> Value {
    json!({"version":env!("CARGO_PKG_VERSION"), "debug":cfg!(debug_assertions),
        "features":{"devCli":true,"fastbootWrite":cfg!(feature="fastboot-write"),"rootWrite":cfg!(feature="root-write"),"efsWrite":cfg!(feature="efs-write")}})
}

async fn run(
    mut value: Value,
    data_dir: &Path,
    step: &str,
    allow_write: bool,
) -> Result<bool, String> {
    let req = decode(value.clone())?;
    authorize(&req, allow_write)?; // Before any hardware lookup, even hashed selectors.
    let command = req.policy().0;
    std::fs::create_dir_all(data_dir).map_err(|e| e.to_string())?;
    let data_dir = data_dir.canonicalize().map_err(|e| e.to_string())?;
    crate::app_paths::init(data_dir.clone());
    let runs = data_dir.join("dev-runs");
    std::fs::create_dir_all(&runs).map_err(|e| e.to_string())?;
    let dir = runs.join(format!(
        "{}-{}",
        chrono::Utc::now().format("%Y%m%dT%H%M%S%.9fZ"),
        std::process::id()
    ));
    std::fs::create_dir(&dir).map_err(|e| format!("실행 폴더 생성 실패: {e}"))?;
    let record_path = dir.join("record.json");
    let mut record = json!({"version":1,"command":command,"step":step,"status":"running","startedAt":now(),"build":build_info(),"recordPath":record_path});
    write_record(&record_path, &record)?;
    let mut redactor = Redactor::default();
    redactor.learn(&value);
    let log = Arc::new(Mutex::new(RunLog {
        file: std::fs::File::create(dir.join("events.jsonl")).map_err(|e| e.to_string())?,
        redactor,
        io_error: None,
    }));
    log.lock()
        .map_err(|_| "로그 잠금 실패")?
        .emit(json!({"type":"started","record":record}))?;
    let output = log.clone();
    let events = Events::callback(move |name, payload| {
        output
            .lock()
            .map_err(|_| "로그 잠금 실패".to_string())?
            .emit(json!({"type":"event","at":now(),"event":name,"payload":payload}))
    });
    let result = async {
        let _power = crate::guard::CliPowerGuard::acquire().map_err(|e| json!(e))?;
        resolve_targets(&mut value, events.clone())
            .await
            .map_err(|e| json!(e))?;
        log.lock()
            .map_err(|_| json!("로그 잠금 실패"))?
            .redactor
            .learn(&value);
        let request = decode(value).map_err(|e| json!(e))?;
        dispatch(request, events).await
    }
    .await;
    let mut log = log.lock().map_err(|_| "로그 잠금 실패")?;
    let (mut ok, result) = match result {
        Ok(value) => (outcome_ok(command, &value), json!({"value":value})),
        Err(error) => (false, json!({"error":error})),
    };
    log.redactor.learn(&result);
    record["finishedAt"] = json!(now());
    record["result"] = log.redactor.clean(&result);
    if let Some(error) = &log.io_error {
        ok = false;
        record["logError"] = json!(error);
    }
    record["status"] = json!(if ok { "done" } else { "failed" });
    // A result reaches stdout only once logs AND the final receipt have been persisted.
    let line = match log
        .append(json!({"type":"result","ok":ok,"record":record}))
        .and_then(|line| log.file.sync_all().map(|_| line).map_err(|e| e.to_string()))
    {
        Ok(line) => line,
        Err(error) => {
            record["status"] = json!("failed");
            record["logError"] = json!(error);
            write_record(&record_path, &record)?;
            return Err(error);
        }
    };
    write_record(&record_path, &record)?;
    let _ = std::io::stdout().lock().write_all(line.as_bytes());
    Ok(ok)
}

fn outcome_ok(command: &str, value: &Value) -> bool {
    for key in ["errors", "failures"] {
        if value
            .get(key)
            .and_then(Value::as_array)
            .is_some_and(|a| !a.is_empty())
        {
            return false;
        }
    }
    !(matches!(command, "efs_verify" | "relock_gate_check")
        && value.get("ok") == Some(&json!(false)))
}

fn history(data_dir: &Path) -> Result<Value, String> {
    let runs = data_dir.join("dev-runs");
    if !runs.exists() {
        return Ok(json!([]));
    }
    let mut result = Vec::new();
    for entry in std::fs::read_dir(runs).map_err(|e| e.to_string())? {
        let path = entry.map_err(|e| e.to_string())?.path().join("record.json");
        if let Some(bytes) = crate::storage::read_bounded(&path, MAX_RECORD)? {
            let record: Value =
                serde_json::from_slice(&bytes).map_err(|_| "실행 기록 형식 오류")?;
            result.push(json!({"command":record["command"],"step":record["step"],"status":record["status"],"startedAt":record["startedAt"],"finishedAt":record["finishedAt"],"recordPath":path}));
        }
    }
    result.sort_by(|a, b| a["startedAt"].as_str().cmp(&b["startedAt"].as_str()));
    Ok(json!(result))
}

fn read_request(path: &Path) -> Result<Value, String> {
    let bytes = if path == Path::new("-") {
        let mut bytes = Vec::new();
        std::io::stdin()
            .take(MAX_REQUEST as u64 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > MAX_REQUEST {
            return Err("요청이 1MiB를 초과했습니다".into());
        }
        bytes
    } else {
        crate::storage::read_bounded(path, MAX_REQUEST)?.ok_or("요청 파일이 없습니다")?
    };
    let bytes = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes); // PowerShell UTF-8 BOM
    serde_json::from_slice(bytes)
        .map_err(|e| format!("요청 JSON 문법 오류 (줄 {}, 열 {})", e.line(), e.column()))
}

struct Options {
    action: String,
    request: Option<PathBuf>,
    data_dir: Option<PathBuf>,
    step: String,
    allow_write: bool,
}
fn options(args: impl Iterator<Item = OsString>) -> Result<Options, String> {
    let mut args = args;
    let action = args
        .next()
        .unwrap_or_else(|| "help".into())
        .into_string()
        .map_err(|_| "명령은 UTF-8이어야 합니다")?;
    let mut opts = Options {
        action,
        request: None,
        data_dir: None,
        step: String::new(),
        allow_write: false,
    };
    let mut seen = std::collections::HashSet::new();
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|_| "옵션 이름은 UTF-8이어야 합니다")?;
        if !seen.insert(arg.clone()) {
            return Err("같은 옵션을 두 번 지정했습니다".into());
        }
        match arg.as_str() {
            "--request" => {
                opts.request = Some(args.next().ok_or("--request 값이 필요합니다")?.into())
            }
            "--data-dir" => {
                opts.data_dir = Some(args.next().ok_or("--data-dir 값이 필요합니다")?.into())
            }
            "--step" => {
                opts.step = args
                    .next()
                    .ok_or("--step 값이 필요합니다")?
                    .into_string()
                    .map_err(|_| "step은 UTF-8이어야 합니다")?
            }
            "--allow-device-write" => opts.allow_write = true,
            _ => return Err("알 수 없는 옵션입니다. help를 확인하세요".into()),
        }
    }
    Ok(opts)
}

/// No Tauri app/window, background daemon, adb/EfsTools subprocess or mock fallback is started.
pub fn main_entry(args: impl Iterator<Item = OsString>) -> i32 {
    let result = (|| {
        let opts = options(args)?;
        match opts.action.as_str() {
            "help" | "--help" | "-h" => {
                println!("xva-dev commands\nxva-dev validate --request <JSON file|->\nxva-dev run --request <JSON file|-> --data-dir <absolute folder> [--step <label>] [--allow-device-write]\nxva-dev history --data-dir <absolute folder>\nOne invocation executes ONE engine command. Rebuild and run only the next/repaired command. Never auto-replay writes. Output: JSON Lines; exit 0 success, 1 engine/verification failure, 2 input/recording failure.");
                Ok(true)
            }
            "commands" => {
                println!("{}", json!({"build":build_info(),"commands":commands()}));
                Ok(true)
            }
            "validate" => {
                let value = read_request(opts.request.as_deref().ok_or("--request가 필요합니다")?)?;
                let req = decode(value)?;
                let (command, writes, enabled) = req.policy();
                println!(
                    "{}",
                    json!({"valid":true,"command":command,"requiresDeviceWrite":writes,"enabledInBuild":enabled})
                );
                Ok(true)
            }
            "history" | "run" => {
                let dir = opts
                    .data_dir
                    .ok_or("--data-dir가 필요합니다. 개발 세션 폴더를 명시하세요")?;
                if !dir.is_absolute() {
                    return Err("--data-dir는 절대 경로여야 합니다".into());
                }
                if opts.action == "history" {
                    println!("{}", history(&dir)?);
                    Ok(true)
                } else {
                    let value =
                        read_request(opts.request.as_deref().ok_or("--request가 필요합니다")?)?;
                    tauri::async_runtime::block_on(run(value, &dir, &opts.step, opts.allow_write))
                }
            }
            _ => Err("알 수 없는 명령입니다. help를 확인하세요".into()),
        }
    })();
    match result {
        Ok(true) => 0,
        Ok(false) => 1,
        Err(error) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "{}",
                json!({"type":"cli-error","error":error})
            );
            2
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn catalog_examples_use_the_same_schema_as_dispatch() {
        for spec in commands().as_array().unwrap() {
            assert!(
                decode(spec["example"].clone()).is_ok(),
                "{}",
                spec["command"]
            );
        }
    }
    #[test]
    fn requests_reject_unknown_commands_args_and_missing_target() {
        for value in [
            json!({"command":"adb_shell","args":{}}),
            json!({"command":"device_list","args":{"extra":true}}),
            json!({"command":"root_check","args":{}}),
            json!({"command":"root_check","args":{"serial":" "}}),
        ] {
            assert!(decode(value).is_err());
        }
    }
    #[test]
    fn writes_require_runtime_opt_in_even_when_the_feature_is_enabled() {
        let req = decode(json!({"command":"restore_run","args":{"serial":"TEST","dir":"backup","items":["dcim"]}})).unwrap();
        assert!(authorize(&req, false).is_err());
        assert!(authorize(&req, true).is_ok());
        let req = decode(json!({"command":"efs_diag_open","args":{"serial":"TEST"}})).unwrap();
        assert_eq!(authorize(&req, true).is_ok(), cfg!(feature = "efs-write"));
    }
    #[test]
    fn private_values_are_scrubbed_in_keys_nested_logs_and_errors() {
        let value = json!({"serial":"SERIAL001", "code":"1234567890abcdef", "nested":[{"imei":"123456789012345", "log":"SERIAL001 1234567890ABCDEF 123456789012345"}]});
        let mut redactor = Redactor::default();
        redactor.learn(&value);
        let cleaned = redactor.clean(&value);
        let output = cleaned.to_string();
        for secret in ["SERIAL001", "1234567890ABCDEF", "123456789012345"] {
            assert!(!output.contains(secret));
        }
        assert_eq!(cleaned["serialKey"], serial_key("SERIAL001"));
        // Diagnostic error codes are not operator unlock codes.
        assert_eq!(
            redactor.clean(&json!({"code":"io","message":"port error"}))["code"],
            "io"
        );
    }
    #[test]
    fn failed_log_writes_are_retained_for_the_final_receipt() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let mut log = RunLog {
            file: std::fs::File::open(file.path()).unwrap(),
            redactor: Redactor::default(),
            io_error: None,
        };
        assert!(log.append(json!({"type":"event"})).is_err());
        assert!(log.io_error.is_some());
    }
    #[test]
    fn failed_verification_is_not_a_successful_step() {
        assert!(!outcome_ok("efs_verify", &json!({"ok":false})));
        assert!(!outcome_ok("restore_run", &json!({"failures":["install"]})));
        assert!(outcome_ok("root_check", &json!(false)));
    }
    #[test]
    fn unfinished_attempts_survive_and_retries_do_not_overwrite_them() {
        let dir = tempfile::tempdir().unwrap();
        for (id, status) in [("1", "running"), ("2", "failed"), ("3", "done")] {
            write_record(
                &dir.path().join("dev-runs").join(id).join("record.json"),
                &json!({"command":"efs_verify","step":"slot1","status":status,"startedAt":id}),
            )
            .unwrap();
        }
        let attempts = history(dir.path()).unwrap();
        assert_eq!(attempts.as_array().unwrap().len(), 3);
        assert_eq!(attempts[0]["status"], "running");
        assert_eq!(attempts[2]["status"], "done");
    }
}
