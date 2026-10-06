//! Explicitly invoked, read-only diagnostics for failed shared-storage paths.
//! Inputs and PC report location come from environment; no app/device-specific paths.
use adb_client::ADBDeviceExt;
use serde_json::{json, Value};

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn query(dev: &mut dyn ADBDeviceExt, label: &str, command: &str) -> Value {
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let command = format!("({command}); printf '\\n__DIAG_RC__=%s\\n' \"$?\"");
    let result = dev.shell_command(&command.as_str(), Some(&mut stdout), Some(&mut stderr));
    json!({"label":label,"stdout":super::scrub(&String::from_utf8_lossy(&stdout)),
        "stderr":super::scrub(&String::from_utf8_lossy(&stderr)),
        "transportError":result.err().map(|error|super::scrub(&error.to_string()))})
}

#[test]
#[ignore = "requires explicit user authorization and a connected phone; reads only"]
fn live_backup_access_diagnostics() {
    let input = std::env::var("XVOLTE_ACCESS_INPUT").expect("XVOLTE_ACCESS_INPUT");
    let output = std::env::var("XVOLTE_ACCESS_OUTPUT").expect("XVOLTE_ACCESS_OUTPUT");
    let selector = std::env::var("XVOLTE_ACCESS_SELECTOR").expect("XVOLTE_ACCESS_SELECTOR");
    let paths: Vec<String> = serde_json::from_slice(&std::fs::read(input).unwrap()).unwrap();
    assert!(!paths.is_empty() && paths.len() <= 16);
    assert!(paths.iter().all(|path| path.starts_with("/sdcard/")
        && !path.split('/').any(|part| part == "..")
        && !path.contains('\0')));
    let _operation = crate::device_io::WriteOperation::acquire().unwrap();
    let report = crate::adb::with_first_device(&None, |dev| {
        if crate::device_io::identity_key(dev)? != selector.trim_start_matches("sha256:") {
            return Err("진단 대상 기기가 백업 원본과 다릅니다".into());
        }
        let identity = query(dev, "identity", "id; getenforce; cat /proc/self/attr/current; readlink -f /sdcard");
        let mut entries = Vec::new();
        for path in &paths {
            let parent = path.rsplit_once('/').unwrap().0;
            let target = quote(path);
            let parent = quote(parent);
            let metadata = query(dev, "metadata", &format!("ls -ldnZ -- {parent} {target}; stat -c 'type=%F mode=%a uid=%u gid=%g size=%s' -- {parent} {target}; readlink -- {target}"));
            let package = path.strip_prefix("/sdcard/Android/data/").and_then(|rest| rest.split('/').next());
            let package_uid = package.map(|package| query(dev, "package-uid", &format!("cmd package list packages -U --user 0 {}", quote(package))));
            let read = query(dev, "open-or-list", &format!("if [ -d {target} ]; then ls -A -- {target} >/dev/null; else head -c 1 -- {target} >/dev/null; fi"));
            let canonical = quote(&path.replacen("/sdcard/", "/storage/emulated/0/", 1));
            let alias = query(dev, "canonical-open-or-list", &format!("if [ -d {canonical} ]; then ls -A -- {canonical} >/dev/null; else head -c 1 -- {canonical} >/dev/null; fi"));
            entries.push(json!({"path":path,"metadata":metadata,"packageUid":package_uid,"read":read,"canonicalRead":alias}));
        }
        // Only storage/security denial records, never general application logs.
        let denials = query(dev, "security-storage-denials", "logcat -d -t 300 -b all -s auditd:I avc:I SELinux:I FuseDaemon:W MediaProvider:W '*:S' | grep -E 'avc:.*denied|[Pp]ermission.*denied|EACCES'");
        Ok(json!({"identity":identity,"entries":entries,"denials":denials}))
    }).expect("read-only device diagnostics failed");
    std::fs::write(output, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    println!("Read-only access diagnostics saved ({} paths)", paths.len());
}
