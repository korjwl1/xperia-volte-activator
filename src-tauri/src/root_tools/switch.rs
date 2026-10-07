use crate::{
    boot_image, device_io,
    root_state::{Access, Engine},
};
use adb_client::ADBDeviceExt;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Switch {
    pub stage: String,
    pub target: String,
    pub fingerprint: String,
    pub stock_sha256: String,
    pub partition: String,
    pub modules: Vec<super::modules::Module>,
    pub boot_id: String,
    pub history_offset: u64,
}
fn read(dir: &Path, key: &str) -> Result<Switch, String> {
    let raw = crate::storage::read_bounded(&super::key_path(dir, key, "root-switch")?, 512 * 1024)?
        .ok_or("엔진 전환 준비 기록 없음")?;
    let record: Switch = serde_json::from_slice(&raw).map_err(|_| "엔진 전환 기록 손상")?;
    if ![
        "cleanup-intent",
        "cleaned-awaiting-stock",
        "stock-verified",
        "complete",
    ]
    .contains(&record.stage.as_str())
        || !["magisk", "resukisu"].contains(&record.target.as_str())
        || record.stock_sha256.len() != 64
        || !record.stock_sha256.bytes().all(|b| b.is_ascii_hexdigit())
        || record.fingerprint.is_empty()
    {
        return Err("엔진 전환 기록 값 손상".into());
    }
    boot_image::validate_partition(&record.partition)?;
    Ok(record)
}
const CLEAN:&str=crate::device_io::su!("'rm -rf /data/adb/modules /data/adb/modules_update /data/adb/metamodule /data/adb/magisk /data/adb/magisk.db /data/adb/ksu /data/adb/ksud || exit; for p in /data/adb/modules /data/adb/modules_update /data/adb/metamodule /data/adb/magisk /data/adb/magisk.db /data/adb/ksu /data/adb/ksud; do test ! -e \"$p\" || exit 2; done'");
fn check_stock(dev: &mut dyn ADBDeviceExt, path: &Path) -> Result<boot_image::ImageOrigin, String> {
    let bytes = boot_image::read(path)?;
    let origin = boot_image::load_origin(path, &bytes, "")?;
    if origin.source_sha256.is_some() {
        return Err("순정 이미지가 필요합니다".into());
    }
    boot_image::check_fits_partition(&bytes, &origin.partition)?;
    boot_image::verify_fingerprint(dev, &origin.fingerprint)?;
    Ok(origin)
}
pub(crate) fn require_module_install_stage(dir: &Path, key: &str) -> Result<(), String> {
    let path = super::key_path(dir, key, "root-switch")?;
    if path.exists() && read(dir, key)?.stage != "complete" {
        return Err("진행 중인 루트 엔진 전환을 먼저 완료하세요 — 모듈 설치는 보류됩니다".into());
    }
    Ok(())
}
pub(crate) fn require_install_stage(
    dev: &mut dyn ADBDeviceExt,
    dir: &Path,
    target: &str,
    stock_sha: &str,
) -> Result<(), String> {
    let key = device_io::identity_key(dev)?;
    let path = super::key_path(dir, &key, "root-switch")?;
    if path.exists() {
        let r = read(dir, &key)?;
        if r.stage != "complete"
            && (r.stage != "stock-verified" || r.target != target || r.stock_sha256 != stock_sha)
        {
            return Err("진행 중인 엔진 전환의 순정 복원·대상 엔진을 확인하세요".into());
        }
    }
    let root = crate::root_state::inspect(dev)?;
    match (root.access,root.engine,target) {
        (Access::Unavailable,Engine::Unknown,_) | (Access::Granted,Engine::Magisk,"magisk")=>Ok(()),
        _=>Err("다른 엔진·권한 불명 상태에서는 바로 새 엔진을 기록할 수 없습니다 — 엔진 전환 절차를 이용하세요".into()),
    }
}
pub fn prepare_work(
    dev: &mut dyn ADBDeviceExt,
    dir: &Path,
    stock: &Path,
    target: &str,
) -> Result<Switch, String> {
    if !["magisk", "resukisu"].contains(&target) {
        return Err("지원하지 않는 전환 대상".into());
    }
    let state = crate::root_state::inspect(dev)?;
    if state.access != Access::Granted
        || ![Engine::Magisk, Engine::KernelsuFamily].contains(&state.engine)
    {
        return Err("현재 루트 권한과 단일 엔진을 먼저 확인하세요".into());
    }
    if state.engine == Engine::Magisk && target == "magisk" {
        return Err("현재 엔진과 동일합니다".into());
    }
    if device_io::shell(dev, "getprop ro.boot.flash.locked")?.trim() != "0" {
        return Err("이미 언락된 기기만 엔진 전환할 수 있습니다".into());
    }
    let origin = check_stock(dev, stock)?;
    if target == "resukisu" && origin.partition != "init_boot" {
        return Err(
            "ReSukiSU 엔진 전환은 init_boot 기종만 지원합니다 — 기존 모듈은 정리하지 않았습니다"
                .into(),
        );
    }
    let key = device_io::identity_key(dev)?;
    let path = super::key_path(dir, &key, "root-switch")?;
    if let Ok(previous) = read(dir, &key) {
        if previous.stage != "complete" {
            return Err("끝나지 않은 전환 기록이 있습니다 — 같은 작업을 재개하세요".into());
        }
    } else if path.exists() {
        return Err("기존 전환 기록을 읽지 못했습니다".into());
    }
    boot_image::save_device_check(dir, &key, &origin)?;
    let history_offset =
        crate::storage::read_bounded(&dir.join("flash-history.jsonl"), 16 * 1024 * 1024)?
            .map_or(0, |b| b.len() as u64);
    let mut r = Switch {
        stage: "cleanup-intent".into(),
        target: target.into(),
        fingerprint: origin.fingerprint,
        stock_sha256: origin.sha256,
        partition: origin.partition,
        modules: super::modules::list(dev)?,
        boot_id: super::modules::boot_id(dev)?,
        history_offset,
    };
    super::save(&path, &r)?;
    // Only these fixed engine/module paths. Manager apps are removed manually (hidden packages included).
    device_io::shell_write(dev, CLEAN)?;
    super::modules::reset_after_switch(dir, &key)?;
    r.stage = "cleaned-awaiting-stock".into();
    super::save(&path, &r)?;
    Ok(r)
}
#[tauri::command]
pub async fn root_switch_prepare(
    serial: String,
    stock_path: String,
    target: String,
    confirm: bool,
) -> Result<Switch, String> {
    super::write_gate(&serial, confirm)?;
    if !cfg!(feature = "fastboot-write") {
        return Err("순정 복원에는 fastboot-write가 필요합니다".into());
    }
    let operation = device_io::WriteOperation::acquire()?;
    let dir = super::data_dir()?;
    crate::tasks::blocking("Root engine cleanup", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |d| {
            prepare_work(d, &dir, Path::new(&stock_path), &target)
        })
    })
    .await
}
pub fn verify_stock_work(dev: &mut dyn ADBDeviceExt, dir: &Path) -> Result<Switch, String> {
    let key = device_io::identity_key(dev)?;
    let mut r = read(dir, &key)?;
    if r.stage != "cleaned-awaiting-stock" {
        return Err("순정 복원 검증 단계가 아닙니다".into());
    }
    boot_image::verify_fingerprint(dev, &r.fingerprint)?;
    if super::modules::boot_id(dev)? == r.boot_id {
        return Err("순정 기록 후 OS 재부팅이 아직 확인되지 않았습니다".into());
    }
    // Denied su is not evidence that the previous root engine was removed.
    let root = crate::root_state::inspect(dev)?;
    if root.access != Access::Unavailable {
        return Err(
            "순정 부팅·루트 미사용 상태 확인 실패(권한 거부는 순정으로 간주하지 않음)".into(),
        );
    }
    // Both-slot stock ACK history is required, not just an operator checkbox.
    let history = crate::storage::read_bounded(&dir.join("flash-history.jsonl"), 16 * 1024 * 1024)?
        .ok_or("순정 기록 이력 없음")?;
    let after = history
        .get(usize::try_from(r.history_offset).map_err(|_| "기록 위치 오류")?..)
        .ok_or("플래시 이력이 삭제·교체됐습니다")?;
    let text = std::str::from_utf8(after).map_err(|_| "플래시 이력 손상")?;
    let entries = crate::fastboot::relock::parse_history(text)?;
    let gate = crate::fastboot::relock::verify(&entries, &r.partition, &r.stock_sha256, Some(&key));
    if !gate.ok {
        return Err("같은 기기의 양 슬롯 순정 기록 이력을 확인할 수 없습니다".into());
    }
    r.stage = "stock-verified".into();
    super::save(&super::key_path(dir, &key, "root-switch")?, &r)?;
    Ok(r)
}
#[tauri::command]
pub async fn root_switch_status(serial: String, verify_stock: bool) -> Result<Switch, String> {
    if serial.trim().is_empty() {
        return Err("기기를 선택하세요".into());
    }
    let dir = super::data_dir()?;
    let operation = if verify_stock {
        Some(device_io::WriteOperation::acquire()?)
    } else {
        None
    };
    let work = move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            if verify_stock {
                return verify_stock_work(dev, &dir);
            }
            let key = device_io::identity_key(dev)?;
            read(&dir, &key)
        })
    };
    if verify_stock {
        crate::tasks::blocking("Root stock verification", work).await
    } else {
        crate::tasks::guarded(std::time::Duration::from_secs(60), work).await
    }
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Imported {
    pub path: String,
    pub sha256: String,
    pub partition: String,
    pub fingerprint: String,
}
pub fn import_work(
    dev: &mut dyn ADBDeviceExt,
    dir: &Path,
    stock: &Path,
    patched: &Path,
) -> Result<Imported, String> {
    let origin = check_stock(dev, stock)?;
    let key = device_io::identity_key(dev)?;
    if origin.partition != "init_boot" {
        return Err(
            "ReSukiSU 외부 패치 경로는 init_boot 기종만 준비됐습니다 — boot 기종은 별도 검증 필요"
                .into(),
        );
    }
    require_install_stage(dev, dir, "resukisu", &origin.sha256)?;
    let bytes = boot_image::read(patched)?;
    boot_image::check_fits_partition(&bytes, &origin.partition)?;
    if bytes.len() > boot_image::read(stock)?.len() || boot_image::sha256(&bytes) == origin.sha256 {
        return Err("패치 이미지 크기/변경 검증 실패".into());
    }
    let hash = boot_image::sha256(&bytes);
    let path = dir.join("root-images").join(format!("resukisu-{hash}.img"));
    crate::storage::atomic_write(&path, &bytes)?;
    let patched_origin = boot_image::save_patched_origin(&path, &bytes, &origin)?;
    boot_image::save_device_check(dir, &key, &patched_origin)?;
    Ok(Imported {
        path: path.to_string_lossy().into(),
        sha256: hash,
        partition: origin.partition,
        fingerprint: origin.fingerprint,
    })
}
#[tauri::command]
pub async fn root_external_patch_import(
    serial: String,
    stock_path: String,
    patched_path: String,
    confirm_same_phone: bool,
) -> Result<Imported, String> {
    super::write_gate(&serial, confirm_same_phone)?;
    if !confirm_same_phone || serial.trim().is_empty() {
        return Err("동일 폰·순정 이미지로 직접 패치한 결과라는 확인이 필요합니다".into());
    }
    let dir = super::data_dir()?;
    let operation = device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("Root image import", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            import_work(dev, &dir, Path::new(&stock_path), Path::new(&patched_path))
        })
    })
    .await
}
#[tauri::command]
pub async fn resukisu_install(serial: String, sha256: String, confirm: bool) -> Result<(), String> {
    super::write_gate(&serial, confirm)?;
    let dir = super::data_dir()?;
    let operation = device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("ReSukiSU manager installation", move || {
        let _operation = operation;
        let (p, bytes) = super::packages::load(&dir, &sha256)?;
        if p.id != "resukisu" {
            return Err("ReSukiSU 매니저 APK가 필요합니다".into());
        }
        crate::adb::with_first_device(&Some(serial), |dev| {
            if crate::device_io::shell(dev, "getprop ro.product.cpu.abi")?.trim() != "arm64-v8a" {
                return Err("arm64-v8a 기기만 지원합니다".into());
            }
            // Install reads a path: recheck content after load, immediately before Android signature validation.
            let path = dir.join("root-packages").join(format!("{}.apk", p.sha256));
            if crate::storage::read_bounded(&path, 256 * 1024 * 1024)?.as_deref()
                != Some(bytes.as_slice())
            {
                return Err("설치 직전 APK 내용 변경".into());
            }
            dev.install(&path, None)
                .map_err(|_| "ReSukiSU 매니저 설치 실패".into())
        })
    })
    .await
}
#[tauri::command]
pub async fn root_switch_finish(serial: String) -> Result<Switch, String> {
    if serial.trim().is_empty() {
        return Err("기기를 선택하세요".into());
    }
    let dir = super::data_dir()?;
    let operation = device_io::WriteOperation::acquire()?;
    crate::tasks::blocking("Root switch completion", move || {
        let _operation = operation;
        crate::adb::with_first_device(&Some(serial), |dev| {
            let key = device_io::identity_key(dev)?;
            let mut r = read(&dir, &key)?;
            if r.stage != "stock-verified" {
                return Err("순정 복원 검증부터 완료하세요".into());
            }
            boot_image::verify_fingerprint(dev, &r.fingerprint)?;
            let state = crate::root_state::inspect(dev)?;
            let expected = if r.target == "magisk" {
                Engine::Magisk
            } else {
                Engine::KernelsuFamily
            };
            if state.access != Access::Granted || state.engine != expected {
                return Err("새 엔진의 루트 권한·마커 확인 실패".into());
            }
            r.stage = "complete".into();
            super::save(&super::key_path(&dir, &key, "root-switch")?, &r)?;
            Ok(r)
        })
    })
    .await
}
#[cfg(test)]
mod tests {
    use super::*;
    fn fake() -> crate::backup::fake_device::FakeADBDevice {
        let mut d = crate::backup::fake_device::FakeADBDevice::new();
        d.answer_shell(crate::device_io::su!("id 2>&1"), "uid=0(root)");
        d.answer_shell(crate::root_state::VERSION, "30.7:MAGISKSU");
        d.answer_shell(crate::root_state::MARKERS, "uid=0(root)\nMAGISK=1\nKSU=0\n");
        d.answer_shell("getprop ro.serialno", "TEST-PHONE");
        d.answer_shell("getprop ro.boot.flash.locked", "0");
        d.answer_shell("getprop ro.build.fingerprint", "Sony/current");
        d.answer_shell(
            "cat /proc/sys/kernel/random/boot_id",
            "11111111-1111-1111-1111-111111111111",
        );
        d.answer_shell(crate::device_io::su!("'test -d /data/adb || exit 2; for d in /data/adb/modules/*; do test -d \"$d\" || continue; n=${d##*/}; if test -e \"$d/disable\"; then echo \"$n disabled\"; elif test -e \"$d/remove\"; then echo \"$n removing\"; else echo \"$n enabled\"; fi; done'"),"neozygisk enabled\n");
        d.answer_shell(CLEAN, "");
        d
    }
    fn stock(dir: &Path) -> std::path::PathBuf {
        let p = dir.join("stock.img");
        boot_image::save_with_origin(
            &p,
            &boot_image::test_image(8192, 0x41),
            "init_boot",
            "Sony/current",
        )
        .unwrap();
        p
    }
    fn history(key: &str, sha: &str) -> String {
        ["init_boot_a", "init_boot_b"]
            .iter()
            .map(|p| {
                format!(
                    "{}\n",
                    serde_json::json!({"deviceKey":key,"partition":p,"sha256":sha,"status":"done"})
                )
            })
            .collect()
    }
    #[test]
    fn cleanup_saves_module_list_before_writes_and_old_history_cannot_prove_new_stock_restore() {
        let dir = tempfile::tempdir().unwrap();
        let stock = stock(dir.path());
        let mut d = fake();
        let key = device_io::identity_key(&mut d).unwrap();
        let sha = boot_image::sha256(&boot_image::read(&stock).unwrap());
        let old = history(&key, &sha);
        std::fs::write(dir.path().join("flash-history.jsonl"), &old).unwrap();
        let r = prepare_work(&mut d, dir.path(), &stock, "resukisu").unwrap();
        assert_eq!(r.stage, "cleaned-awaiting-stock");
        assert_eq!(r.modules.len(), 1);
        assert!(verify_stock_work(&mut d, dir.path()).is_err());
        d.answer_shell(
            "cat /proc/sys/kernel/random/boot_id",
            "22222222-2222-2222-2222-222222222222",
        );
        let su = crate::device_io::su!("id 2>&1");
        d.shell_exit_codes.insert(su.into(), 127);
        assert!(verify_stock_work(&mut d, dir.path()).is_err());
        std::fs::write(
            dir.path().join("flash-history.jsonl"),
            format!("{old}{}", history(&key, &sha)),
        )
        .unwrap();
        assert_eq!(
            verify_stock_work(&mut d, dir.path()).unwrap().stage,
            "stock-verified"
        );
        assert!(require_install_stage(&mut d, dir.path(), "magisk", &sha).is_err());
        assert!(require_install_stage(&mut d, dir.path(), "resukisu", &sha).is_ok());
    }
    #[test]
    fn cleanup_failure_leaves_intent_and_unconfirmed_root_or_mismatched_firmware_never_runs_cleanup(
    ) {
        let dir = tempfile::tempdir().unwrap();
        let stock = stock(dir.path());
        let mut d = fake();
        d.answer_shell("getprop ro.build.fingerprint", "Sony/different");
        assert!(prepare_work(&mut d, dir.path(), &stock, "resukisu").is_err());
        assert!(!d.shell_calls.iter().any(|c| c.starts_with(CLEAN)));
        d.answer_shell("getprop ro.build.fingerprint", "Sony/current");
        d.shell_exit_codes.insert(CLEAN.into(), 1);
        assert!(prepare_work(&mut d, dir.path(), &stock, "resukisu").is_err());
        let key = device_io::identity_key(&mut d).unwrap();
        assert_eq!(read(dir.path(), &key).unwrap().stage, "cleanup-intent");
        assert!(prepare_work(&mut d, dir.path(), &stock, "resukisu").is_err());
        assert_eq!(
            d.shell_calls
                .iter()
                .filter(|c| c.starts_with(CLEAN))
                .count(),
            1
        );
        let saved = read(dir.path(), &key).unwrap();
        for field in ["stage", "target", "hash"] {
            let mut corrupt = saved.clone();
            match field {
                "stage" => corrupt.stage = "complete-invalid".into(),
                "target" => corrupt.target = "unknown-engine".into(),
                _ => corrupt.stock_sha256 = "invalid".into(),
            }
            super::super::save(
                &super::super::key_path(dir.path(), &key, "root-switch").unwrap(),
                &corrupt,
            )
            .unwrap();
            assert!(read(dir.path(), &key).is_err());
            assert!(prepare_work(&mut d, dir.path(), &stock, "resukisu").is_err());
        }
    }
    #[test]
    fn unsupported_resukisu_boot_switch_is_rejected_before_cleanup_or_any_saved_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("boot.img");
        let bytes = boot_image::test_boot_kernel_image(8192, 0x41);
        boot_image::save_with_origin(&path, &bytes, "boot", "Sony/current").unwrap();
        let mut d = fake();
        assert!(prepare_work(&mut d, dir.path(), &path, "resukisu")
            .err()
            .unwrap()
            .contains("init_boot"));
        assert!(!d.shell_calls.iter().any(|c| c.starts_with(CLEAN)));
        assert!(!dir.path().join("root-switch").exists());
        assert!(!dir.path().join("image-checks").exists());
        // KernelSU-family -> Magisk on this partition remains a supported conversion.
        d.answer_shell(crate::root_state::VERSION, "4.2.0:KernelSU");
        d.answer_shell(crate::root_state::MARKERS, "uid=0(root)\nMAGISK=0\nKSU=1\n");
        assert_eq!(
            prepare_work(&mut d, dir.path(), &path, "magisk")
                .unwrap()
                .stage,
            "cleaned-awaiting-stock"
        );
    }
    #[test]
    fn external_image_import_is_bound_to_stock_device_and_changed_image_not_just_magic() {
        let dir = tempfile::tempdir().unwrap();
        let stock = stock(dir.path());
        let patch = dir.path().join("patched.img");
        let mut d = fake();
        std::fs::write(&patch, boot_image::test_image(8192, 0x42)).unwrap();
        assert!(import_work(&mut d, dir.path(), &stock, &patch).is_err()); // existing Magisk is not a silent switch
        d.shell_exit_codes
            .insert(crate::device_io::su!("id 2>&1").into(), 127);
        let out = import_work(&mut d, dir.path(), &stock, &patch).unwrap();
        let key = device_io::identity_key(&mut d).unwrap();
        let bytes = boot_image::read(Path::new(&out.path)).unwrap();
        assert!(boot_image::require_device_check(
            dir.path(),
            &key,
            Path::new(&out.path),
            &bytes,
            "init_boot",
            false
        )
        .is_ok());
        assert!(boot_image::require_device_check(
            dir.path(),
            &"a".repeat(64),
            Path::new(&out.path),
            &bytes,
            "init_boot",
            false
        )
        .is_err());
        std::fs::copy(&stock, &patch).unwrap();
        assert!(import_work(&mut d, dir.path(), &stock, &patch).is_err());
    }
}
