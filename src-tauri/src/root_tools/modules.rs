use crate::root_state::{Access, Engine};
use adb_client::ADBDeviceExt;
use serde::{Deserialize, Serialize};
use std::path::Path;

const LIST:&str=crate::device_io::su!("'test -d /data/adb || exit 2; for d in /data/adb/modules/*; do test -d \"$d\" || continue; n=${d##*/}; if test -e \"$d/disable\"; then echo \"$n disabled\"; elif test -e \"$d/remove\"; then echo \"$n removing\"; else echo \"$n enabled\"; fi; done'");
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub id: String,
    pub state: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Inventory {
    pub engine: Engine,
    pub modules: Vec<Module>,
    pub reboot_required: bool,
    pub uncertain: bool,
}
#[derive(Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct Record {
    pub boot_id: String,
    pub pending: bool,
    pub uncertain: bool,
    pub installed: std::collections::BTreeMap<String, String>,
    #[serde(default)]
    pub pending_module: Option<String>,
}
fn read_record(path: &Path) -> Result<Record, String> {
    match crate::storage::read_bounded(path, 256 * 1024)? {
        Some(raw) => serde_json::from_slice(&raw).map_err(|_| "모듈 기록 손상".into()),
        None => Ok(Record::default()),
    }
}
pub fn boot_id(dev: &mut dyn ADBDeviceExt) -> Result<String, String> {
    let v = crate::device_io::shell(dev, "cat /proc/sys/kernel/random/boot_id")?;
    let v = v.trim();
    if v.len() != 36 || !v.bytes().all(|b| b.is_ascii_hexdigit() || b == b'-') {
        return Err("부팅 식별값을 확인할 수 없습니다".into());
    }
    Ok(v.into())
}
pub(crate) fn list(dev: &mut dyn ADBDeviceExt) -> Result<Vec<Module>, String> {
    let text = crate::device_io::shell(dev, LIST)?;
    if text.len() > 256 * 1024 {
        return Err("모듈 목록 크기 초과".into());
    }
    let mut modules = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for line in text.lines() {
        let parts: Vec<_> = line.split_ascii_whitespace().collect();
        if parts.len() != 2
            || parts[0].len() > 128
            || !parts[0]
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
            || !["enabled", "disabled", "removing"].contains(&parts[1])
            || !seen.insert(parts[0])
        {
            return Err("모듈 목록 형식 오류".into());
        }
        modules.push(Module {
            id: parts[0].into(),
            state: parts[1].into(),
        });
    }
    Ok(modules)
}
fn active(modules: &[Module], id: &str) -> bool {
    modules
        .iter()
        .any(|m| m.id.eq_ignore_ascii_case(id) && m.state == "enabled")
}
fn category(modules: &[Module], r: &Record, category: &str, aliases: &[&str]) -> bool {
    r.installed
        .get(category)
        .is_some_and(|id| active(modules, id))
        || aliases.iter().any(|id| {
            active(modules, id)
                && !r
                    .installed
                    .iter()
                    .any(|(other, owned)| other != category && owned.eq_ignore_ascii_case(id))
        })
}
fn dependencies(id: &str, engine: Engine, modules: &[Module], r: &Record) -> Result<(), String> {
    // Current NeoZygisk/Next share zygisksu; PIF/Integrity Box share playintegrityfix.
    // Without this tool's installation receipt, shared IDs cannot prove which project is active.
    for (shared, choices) in [
        (
            "zygisksu",
            &["neozygisk", "rezygisk", "zygisk-next", "shamiko"][..],
        ),
        (
            "playintegrityfix",
            &["play-integrity-fork", "integrity-box"][..],
        ),
    ] {
        if choices.contains(&id)
            && active(modules, shared)
            && !r
                .installed
                .values()
                .any(|owned| owned.eq_ignore_ascii_case(shared))
        {
            return Err("같은 ID를 쓰는 기존 모듈의 종류를 확인할 수 없습니다 — 먼저 해당 모듈을 끄고 재부팅하세요".into());
        }
    }
    let has = |cat, aliases: &[&str]| category(modules, r, cat, aliases);
    if ["neozygisk", "rezygisk", "zygisk-next"].contains(&id) {
        for (other, aliases) in [
            ("neozygisk", &["neozygisk", "zygisksu"][..]),
            ("rezygisk", &["rezygisk"][..]),
            ("zygisk-next", &["zygisk_next", "zygisksu"][..]),
        ] {
            if other != id && has(other, aliases) {
                return Err("Zygisk 구현체는 하나만 설치할 수 있습니다".into());
            }
        }
    }
    if engine == Engine::KernelsuFamily
        && id != "overlayfs"
        && !has(
            "overlayfs",
            &["meta-overlayfs", "meta-overlayfsx", "overlayfs"],
        )
    {
        return Err("KernelSU 계열은 먼저 OverlayFS MetaModule을 설치하고 재부팅하세요".into());
    }
    if id == "overlayfs" && engine != Engine::KernelsuFamily {
        return Err("OverlayFS MetaModule은 KernelSU 계열 전용입니다".into());
    }
    if id == "shamiko"
        && (engine != Engine::Magisk || !has("zygisk-next", &["zygisk_next", "zygisksu"]))
    {
        return Err("Shamiko는 Magisk + Zygisk Next 조합에서만 제공됩니다".into());
    }
    if [
        "play-integrity-fork",
        "integrity-box",
        "hma",
        "zygisk-assistant",
    ]
    .contains(&id)
        && !has("neozygisk", &["neozygisk", "zygisksu"])
        && !has("rezygisk", &["rezygisk"])
        && !has("zygisk-next", &["zygisk_next", "zygisksu"])
    {
        return Err("먼저 Zygisk 구현체를 설치하고 재부팅하세요".into());
    }
    if id == "play-integrity-fork"
        && has(
            "integrity-box",
            &[
                "integritybox",
                "IntegrityBox",
                "integrity_box",
                "playintegrityfix",
            ],
        )
        || id == "integrity-box"
            && has(
                "play-integrity-fork",
                &["playintegrityfix", "playintegrityfork"],
            )
    {
        return Err("PlayIntegrityFork와 Integrity Box는 함께 설치할 수 없습니다".into());
    }
    if id == "tricky-addon" && !has("tricky-store", &["tricky_store"]) {
        return Err("먼저 TrickyStore를 설치하고 재부팅하세요".into());
    }
    Ok(())
}
fn inventory_work(dev: &mut dyn ADBDeviceExt, dir: &Path) -> Result<Inventory, String> {
    let root = crate::root_state::inspect(dev)?;
    if root.access != Access::Granted
        || ![Engine::Magisk, Engine::KernelsuFamily].contains(&root.engine)
    {
        return Err("루트 권한과 단일 엔진을 확인하세요".into());
    }
    let key = crate::device_io::identity_key(dev)?;
    let r = read_record(&super::key_path(dir, &key, "root-modules")?)?;
    let boot = boot_id(dev)?;
    let modules = list(dev)?;
    let missing = r.pending
        && r.boot_id != boot
        && r.pending_module
            .as_deref()
            .is_some_and(|id| !active(&modules, id));
    Ok(Inventory {
        engine: root.engine,
        modules,
        reboot_required: r.pending && r.boot_id == boot,
        uncertain: r.uncertain || missing,
    })
}
#[tauri::command]
pub async fn root_modules_inspect(serial: String) -> Result<Inventory, String> {
    if serial.trim().is_empty() {
        return Err("기기를 선택하세요".into());
    }
    let dir = super::data_dir()?;
    crate::tasks::guarded(std::time::Duration::from_secs(60), move || {
        crate::adb::with_first_device(&Some(serial), |dev| inventory_work(dev, &dir))
    })
    .await
}
pub fn install_work(
    dev: &mut dyn ADBDeviceExt,
    dir: &Path,
    hash: &str,
    confirm_external: bool,
) -> Result<Inventory, String> {
    let (package, bytes) = super::packages::load(dir, hash)?;
    if package.id == "resukisu" {
        return Err("매니저 APK는 모듈 설치 입력이 아닙니다".into());
    }
    if package.external && !confirm_external {
        return Err("외부 배포 모듈 실행 확인이 필요합니다".into());
    }
    let state = inventory_work(dev, dir)?;
    if state.reboot_required || state.uncertain {
        return Err(
            "이전 설치 후 재부팅·결과 확인이 필요합니다 — 불확정 작업을 자동 재시도하지 않습니다"
                .into(),
        );
    }
    let key = crate::device_io::identity_key(dev)?;
    let path = super::key_path(dir, &key, "root-modules")?;
    let mut r = read_record(&path)?;
    dependencies(&package.id, state.engine, &state.modules, &r)?;
    r.boot_id = boot_id(dev)?;
    r.pending = true;
    r.uncertain = true;
    r.pending_module = package.module_id.clone();
    // Durable intent before staging/install. A failed ACK leaves the operation uncertain.
    super::save(&path, &r)?;
    const REMOTE: &str = "/data/local/tmp/xvolte-module.zip";
    let mut cursor = std::io::Cursor::new(&bytes);
    dev.push(&mut cursor, &REMOTE)
        .map_err(|_| "모듈 전송 실패")?;
    let verify = crate::device_io::shell(dev, "sha256sum /data/local/tmp/xvolte-module.zip")?;
    if verify.split_ascii_whitespace().next() != Some(package.sha256.as_str()) {
        return Err("폰에 전달한 모듈 해시 불일치".into());
    }
    let command = match state.engine {
        Engine::Magisk => {
            crate::device_io::su!("'magisk --install-module /data/local/tmp/xvolte-module.zip'")
        }
        Engine::KernelsuFamily => crate::device_io::su!(
            "'/data/adb/ksud module install /data/local/tmp/xvolte-module.zip'"
        ),
        _ => return Err("루트 엔진 불명".into()),
    };
    crate::device_io::shell_write(dev, command)?;
    crate::device_io::shell_write(dev, "rm -f /data/local/tmp/xvolte-module.zip")?;
    let installed_id = package.module_id.ok_or("모듈 ID 없음")?;
    r.installed
        .retain(|_, owned| !owned.eq_ignore_ascii_case(&installed_id));
    r.installed.insert(package.id, installed_id);
    r.uncertain = false;
    super::save(&path, &r)?;
    Ok(Inventory {
        engine: state.engine,
        modules: state.modules,
        reboot_required: true,
        uncertain: false,
    })
}
#[tauri::command]
pub async fn root_module_action(
    serial: String,
    module_id: String,
    action: String,
    confirm: bool,
) -> Result<(), String> {
    super::write_gate(&serial, confirm)?;
    if module_id.is_empty()
        || module_id.len() > 128
        || [".", ".."].contains(&module_id.as_str())
        || !module_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
        || !["disable", "remove"].contains(&action.as_str())
    {
        return Err("모듈 작업 형식 오류".into());
    }
    let operation = crate::device_io::WriteOperation::acquire()?;
    let dir = super::data_dir()?;
    crate::tasks::blocking("Module recovery", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            let root = crate::root_state::inspect(dev)?;
            if root.access != Access::Granted
                || ![Engine::Magisk, Engine::KernelsuFamily].contains(&root.engine)
            {
                return Err("현재 엔진을 확인하세요".into());
            }
            if !list(dev)?.iter().any(|m| m.id == module_id) {
                return Err("설치된 모듈 목록에 없는 대상입니다".into());
            }
            let key = crate::device_io::identity_key(dev)?;
            let path = super::key_path(&dir, &key, "root-modules")?;
            let mut r = read_record(&path)?;
            r.pending = true;
            r.uncertain = true;
            r.pending_module = None;
            r.boot_id = boot_id(dev)?;
            super::save(&path, &r)?;
            // Strict module ID alphabet; no arbitrary caller path or shell text.
            let command = format!(
                "{} -c 'touch /data/adb/modules/{module_id}/{action}'",
                crate::device_io::su_path!()
            );
            crate::device_io::shell_write(dev, &command)?;
            r.uncertain = false;
            super::save(&path, &r)
        })
    })
    .await
}
pub(crate) fn reset_after_switch(dir: &Path, key: &str) -> Result<(), String> {
    super::save(
        &super::key_path(dir, key, "root-modules")?,
        &Record::default(),
    )
}
#[tauri::command]
pub async fn root_module_reconcile(serial: String, confirm: bool) -> Result<Inventory, String> {
    if !confirm || serial.trim().is_empty() {
        return Err("재부팅 후 모듈 목록·오류 원인을 검토했다는 확인이 필요합니다".into());
    }
    let dir = super::data_dir()?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    crate::tasks::guarded(std::time::Duration::from_secs(60), move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            let state = inventory_work(dev, &dir)?;
            let key = crate::device_io::identity_key(dev)?;
            let path = super::key_path(&dir, &key, "root-modules")?;
            let mut r = read_record(&path)?;
            if r.boot_id == boot_id(dev)? {
                return Err("새 OS 부팅을 먼저 확인하세요".into());
            }
            r.pending = false;
            r.uncertain = false;
            r.pending_module = None;
            super::save(&path, &r)?;
            Ok(Inventory {
                reboot_required: false,
                uncertain: false,
                ..state
            })
        })
    })
    .await
}
#[tauri::command]
pub async fn root_module_install(
    serial: String,
    sha256: String,
    confirm: bool,
    confirm_external: bool,
) -> Result<Inventory, String> {
    super::write_gate(&serial, confirm)?;
    let operation = crate::device_io::WriteOperation::acquire()?;
    let dir = super::data_dir()?;
    crate::tasks::blocking("Module installation", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            install_work(dev, &dir, &sha256, confirm_external)
        })
    })
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_public_module_ids_require_receipts_and_never_authorize_the_other_project() {
        let shared = Module {
            id: "zygisksu".into(),
            state: "enabled".into(),
        };
        let mut r = Record::default();
        for id in ["neozygisk", "rezygisk", "zygisk-next", "shamiko"] {
            assert!(dependencies(id, Engine::Magisk, std::slice::from_ref(&shared), &r).is_err());
        }
        r.installed.insert("neozygisk".into(), "zygisksu".into());
        assert!(dependencies(
            "neozygisk",
            Engine::Magisk,
            std::slice::from_ref(&shared),
            &r
        )
        .is_ok());
        assert!(dependencies(
            "zygisk-next",
            Engine::Magisk,
            std::slice::from_ref(&shared),
            &r
        )
        .is_err());
        assert!(
            dependencies("shamiko", Engine::Magisk, std::slice::from_ref(&shared), &r).is_err()
        );
        r.installed.clear();
        r.installed.insert("zygisk-next".into(), "zygisksu".into());
        assert!(dependencies("shamiko", Engine::Magisk, std::slice::from_ref(&shared), &r).is_ok());
        let mut modules = vec![
            shared,
            Module {
                id: "playintegrityfix".into(),
                state: "enabled".into(),
            },
        ];
        assert!(dependencies("play-integrity-fork", Engine::Magisk, &modules, &r).is_err());
        r.installed
            .insert("integrity-box".into(), "playintegrityfix".into());
        assert!(dependencies("play-integrity-fork", Engine::Magisk, &modules, &r).is_err());
        assert!(dependencies("integrity-box", Engine::Magisk, &modules, &r).is_ok());
        modules[1].state = "disabled".into();
        assert!(dependencies("play-integrity-fork", Engine::Magisk, &modules, &r).is_ok());
    }
    #[test]
    fn module_dependencies_exclusivity_and_disabled_dependencies_are_enforced() {
        let r = Record::default();
        let neo = Module {
            id: "neozygisk".into(),
            state: "enabled".into(),
        };
        assert!(dependencies("play-integrity-fork", Engine::Magisk, &[], &r).is_err());
        assert!(dependencies(
            "play-integrity-fork",
            Engine::Magisk,
            std::slice::from_ref(&neo),
            &r
        )
        .is_ok());
        assert!(dependencies("rezygisk", Engine::Magisk, std::slice::from_ref(&neo), &r).is_err());
        assert!(dependencies("neozygisk", Engine::KernelsuFamily, &[], &r).is_err());
        assert!(dependencies("overlayfs", Engine::Magisk, &[], &r).is_err());
        assert!(dependencies("shamiko", Engine::Magisk, &[neo.clone()], &r).is_err());
        let integrity = Module {
            id: "integritybox".into(),
            state: "enabled".into(),
        };
        assert!(
            dependencies("play-integrity-fork", Engine::Magisk, &[neo, integrity], &r).is_err()
        );
        assert!(dependencies("tricky-addon", Engine::Magisk, &[], &r).is_err());
        let mut overlay = Module {
            id: "meta-overlayfs".into(),
            state: "disabled".into(),
        };
        assert!(dependencies(
            "neozygisk",
            Engine::KernelsuFamily,
            std::slice::from_ref(&overlay),
            &r
        )
        .is_err());
        overlay.state = "enabled".into();
        assert!(dependencies("neozygisk", Engine::KernelsuFamily, &[overlay], &r).is_ok());
    }
    fn fake() -> crate::backup::fake_device::FakeADBDevice {
        let mut d = crate::backup::fake_device::FakeADBDevice::new();
        d.answer_shell(crate::device_io::su!("id 2>&1"), "uid=0(root)");
        d.answer_shell(crate::root_state::VERSION, "30.7:MAGISKSU");
        d.answer_shell(crate::root_state::MARKERS, "uid=0(root)\nMAGISK=1\nKSU=0\n");
        d.answer_shell("getprop ro.serialno", "TEST-PHONE");
        d.answer_shell(
            "cat /proc/sys/kernel/random/boot_id",
            "11111111-1111-1111-1111-111111111111",
        );
        d.answer_shell(LIST, "");
        d
    }
    fn prepared(dir: &Path) -> String {
        use std::io::Write;
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(vec![]));
        zip.start_file("module.prop", zip::write::SimpleFileOptions::default())
            .unwrap();
        writeln!(zip, "id=neozygisk").unwrap();
        let bytes = zip.finish().unwrap().into_inner();
        let hash = crate::apk_verify::sha256_hex(&bytes);
        let path = dir.join("root-packages").join(format!("{hash}.zip"));
        crate::storage::atomic_write(&path, &bytes).unwrap();
        let p = super::super::packages::Prepared {
            id: "neozygisk".into(),
            version: "synthetic".into(),
            path: path.to_string_lossy().into(),
            sha256: hash.clone(),
            module_id: Some("neozygisk".into()),
            external: false,
        };
        super::super::save(&path.with_extension("json"), &p).unwrap();
        hash
    }
    #[test]
    fn install_waits_for_real_reboot_and_installed_module_observation() {
        let dir = tempfile::tempdir().unwrap();
        let hash = prepared(dir.path());
        let mut d = fake();
        d.answer_shell(
            "sha256sum /data/local/tmp/xvolte-module.zip",
            &format!("{hash}  /data/local/tmp/xvolte-module.zip"),
        );
        d.answer_shell(
            crate::device_io::su!("'magisk --install-module /data/local/tmp/xvolte-module.zip'"),
            "",
        );
        d.answer_shell("rm -f /data/local/tmp/xvolte-module.zip", "");
        assert!(
            install_work(&mut d, dir.path(), &hash, true)
                .unwrap()
                .reboot_required
        );
        let before = d.pushed.len();
        assert!(install_work(&mut d, dir.path(), &hash, true).is_err());
        assert_eq!(d.pushed.len(), before);
        d.answer_shell(
            "cat /proc/sys/kernel/random/boot_id",
            "22222222-2222-2222-2222-222222222222",
        );
        assert!(inventory_work(&mut d, dir.path()).unwrap().uncertain);
        d.answer_shell(LIST, "neozygisk enabled\n");
        let state = inventory_work(&mut d, dir.path()).unwrap();
        assert!(!state.reboot_required);
        assert!(!state.uncertain);
    }
    #[test]
    fn install_failure_retains_durable_uncertainty_and_no_auto_retry() {
        let dir = tempfile::tempdir().unwrap();
        let hash = prepared(dir.path());
        let mut d = fake();
        d.answer_shell(
            "sha256sum /data/local/tmp/xvolte-module.zip",
            &format!("{hash}  /data/local/tmp/xvolte-module.zip"),
        );
        let command =
            crate::device_io::su!("'magisk --install-module /data/local/tmp/xvolte-module.zip'");
        d.answer_shell(command, "failure");
        d.shell_exit_codes.insert(command.into(), 1);
        assert!(install_work(&mut d, dir.path(), &hash, true).is_err());
        assert!(inventory_work(&mut d, dir.path()).unwrap().uncertain);
        assert!(install_work(&mut d, dir.path(), &hash, true).is_err());
        assert_eq!(
            d.shell_calls
                .iter()
                .filter(|c| c.starts_with(command))
                .count(),
            1
        );
    }
}
