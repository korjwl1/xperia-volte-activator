//! PC-only advisory policy. Caller-supplied observations never grant write permission.
use crate::root_state::{Access, Engine, RootState};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Intent {
    Stock,
    Preserve,
    InstallMagisk,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub root: RootState,
    pub unlocked: Option<bool>,
    pub intent: Intent,
    pub partition: String,
    pub backup_selected: bool,
}
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub action: &'static str,
    pub blockers: Vec<&'static str>,
    pub requirements: Vec<&'static str>,
    pub warnings: Vec<&'static str>,
    pub human_after_backup: bool,
    pub write_ready: bool,
}
pub fn plan(request: Request) -> Result<Plan, String> {
    crate::boot_image::validate_partition(&request.partition)?;
    let root = request.root;
    let mut p = Plan {
        action: "stock-only",
        blockers: vec![],
        requirements: vec![
            "device-region-slot-profile-validation",
            "immutable-target-package-provenance",
            "stock-sin-via-newflasher",
            "os-target-fingerprint-and-ims-check",
        ],
        warnings: vec!["data-preservation-not-guaranteed", "backup-recommended"],
        human_after_backup: request.backup_selected,
        write_ready: false,
    };
    if root.engine == Engine::Conflicting {
        p.blockers.push("conflicting-root-engine-evidence");
    }
    let patch = match request.intent {
        Intent::Stock => {
            p.warnings.push("stock-boot-does-not-preserve-root");
            if root.access != Access::Unavailable {
                p.warnings
                    .push("review-existing-modules-before-stock-update");
            }
            false
        }
        Intent::Preserve => {
            if root.access != Access::Granted {
                p.blockers
                    .push("root-grant-required-to-identify-existing-engine");
            }
            if root.engine != Engine::Magisk {
                p.blockers.push("root-preservation-engine-not-supported");
            }
            true
        }
        Intent::InstallMagisk => {
            // Existing unknown/foreign root must not silently become an engine switch.
            if root.access != Access::Unavailable || root.engine != Engine::Unknown {
                p.blockers
                    .push("new-root-requires-separate-nonroot-verification");
            }
            p.requirements
                .push("nonroot-state-confirmation-before-install");
            true
        }
    };
    if patch {
        if request.unlocked != Some(true) {
            p.blockers.push("already-unlocked-required-no-auto-unlock");
        }
        p.action = "magisk-target-patch-then-fastboot";
        p.requirements.extend([
            "same-phone-source-and-target-fingerprint-binding",
            "fresh-target-stock-image-and-parent-hash",
            "verified-magisk-apk-and-patched-image-hash",
            "review-module-inventory-and-target-os-compatibility",
            "acknowledged-stock-update-before-patched-img",
            "validated-fastboot-mode-and-slot-provenance",
            "root-grant-and-engine-recheck",
            "modules-and-user-apps-manual-recheck",
        ]);
        p.warnings.extend([
            "patched-img-is-not-a-signed-sin",
            "no-vbmeta-or-encryption-option-changes",
            "module-installation-does-not-guarantee-app-compatibility",
        ]);
    }
    if !p.blockers.is_empty() {
        p.action = "blocked";
    }
    Ok(p)
}
#[tauri::command]
pub fn firmware_update_root_plan(request: Request) -> Result<Plan, String> {
    plan(request)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(engine: Engine, access: Access, intent: Intent, unlocked: Option<bool>) -> Request {
        Request {
            root: RootState {
                engine,
                access,
                magisk_markers: None,
                kernelsu_markers: None,
            },
            unlocked,
            intent,
            partition: "init_boot".into(),
            backup_selected: true,
        }
    }
    #[test]
    fn stock_updates_need_no_root_or_unlock_and_still_never_authorize_writes() {
        for unlocked in [Some(true), Some(false), None] {
            let p = plan(request(
                Engine::Unknown,
                Access::Unavailable,
                Intent::Stock,
                unlocked,
            ))
            .unwrap();
            assert_eq!(p.action, "stock-only");
            assert!(p.blockers.is_empty());
            assert!(!p.write_ready);
            assert!(p.human_after_backup);
        }
        let p = plan(request(
            Engine::Magisk,
            Access::Granted,
            Intent::Stock,
            Some(true),
        ))
        .unwrap();
        assert!(p.warnings.contains(&"stock-boot-does-not-preserve-root"));
    }
    #[test]
    fn preserve_requires_identified_magisk_grant_and_existing_unlock() {
        let p = plan(request(
            Engine::Magisk,
            Access::Granted,
            Intent::Preserve,
            Some(true),
        ))
        .unwrap();
        assert_eq!(p.action, "magisk-target-patch-then-fastboot");
        assert!(!p.write_ready);
        for engine in [Engine::KernelsuFamily, Engine::Conflicting, Engine::Unknown] {
            assert_eq!(
                plan(request(
                    engine,
                    Access::Granted,
                    Intent::Preserve,
                    Some(true)
                ))
                .unwrap()
                .action,
                "blocked"
            );
        }
        for access in [Access::Denied, Access::Unavailable, Access::Unknown] {
            assert_eq!(
                plan(request(
                    Engine::Magisk,
                    access,
                    Intent::Preserve,
                    Some(true)
                ))
                .unwrap()
                .action,
                "blocked"
            );
        }
        for unlocked in [Some(false), None] {
            assert_eq!(
                plan(request(
                    Engine::Magisk,
                    Access::Granted,
                    Intent::Preserve,
                    unlocked
                ))
                .unwrap()
                .action,
                "blocked"
            );
        }
    }
    #[test]
    fn installing_magisk_never_silently_switches_an_existing_engine() {
        for engine in [Engine::Magisk, Engine::KernelsuFamily, Engine::Unknown] {
            assert_eq!(
                plan(request(
                    engine,
                    Access::Granted,
                    Intent::InstallMagisk,
                    Some(true)
                ))
                .unwrap()
                .action,
                "blocked"
            );
        }
        let mut r = request(
            Engine::Unknown,
            Access::Unavailable,
            Intent::InstallMagisk,
            Some(true),
        );
        r.backup_selected = false;
        let p = plan(r).unwrap();
        assert!(!p.human_after_backup);
        assert!(!p.write_ready);
        assert!(p
            .requirements
            .contains(&"nonroot-state-confirmation-before-install"));
        let mut r = request(
            Engine::Magisk,
            Access::Granted,
            Intent::Preserve,
            Some(true),
        );
        r.partition = "vbmeta".into();
        assert!(plan(r).is_err());
    }
}
