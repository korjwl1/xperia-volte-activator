#![cfg(feature = "dev-cli")]

use serde_json::{json, Value};
use std::process::{Command, Output};

fn cli(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_xva-dev"))
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn standalone_cli_records_an_offline_engine_result_and_lists_it_after_restart() {
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    std::fs::write(
        &request,
        json!({"command":"efs_tool_check","args":{}}).to_string(),
    )
    .unwrap();
    let state = dir.path().join("session");
    let result = cli(&[
        "run",
        "--request",
        request.to_str().unwrap(),
        "--data-dir",
        state.to_str().unwrap(),
        "--step",
        "capabilities",
    ]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let stdout = String::from_utf8(result.stdout).unwrap();
    let lines: Vec<Value> = stdout
        .lines()
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    assert_eq!(lines[0]["type"], "started");
    let result = lines.last().unwrap();
    assert_eq!(result["ok"], true);
    assert_eq!(result["record"]["result"]["value"]["native"], true);
    assert_eq!(
        result["record"]["result"]["value"]["deviceExecution"],
        cfg!(feature = "efs-write")
    );
    let history = cli(&["history", "--data-dir", state.to_str().unwrap()]);
    let history: Value = serde_json::from_slice(&history.stdout).unwrap();
    assert_eq!(history[0]["step"], "capabilities");
    assert_eq!(history[0]["status"], "done");
    let record = std::fs::read(history[0]["recordPath"].as_str().unwrap()).unwrap();
    let record: Value = serde_json::from_slice(&record).unwrap();
    assert_eq!(record, result["record"]);
}

#[test]
fn denied_write_and_malformed_input_exit_before_creating_a_session_or_probing_a_device() {
    let dir = tempfile::tempdir().unwrap();
    let request = dir.path().join("request.json");
    let state = dir.path().join("session");
    // Restore has no Cargo write feature, so runtime opt-in is always required, including all-features.
    std::fs::write(&request, json!({"command":"restore_run","args":{"serial":"sha256:bad", "dir":"missing", "items":["dcim"]}}).to_string()).unwrap();
    let output = cli(&[
        "run",
        "--request",
        request.to_str().unwrap(),
        "--data-dir",
        state.to_str().unwrap(),
    ]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!state.exists());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert!(error["error"]
        .as_str()
        .unwrap()
        .contains("--allow-device-write"));
    std::fs::write(&request, "{\"command\":\"fastboot_unlock\",\"args\":{\"code\":\"PRIVATE-INVALID-CODE\",\"serial\":true}}").unwrap();
    let output = cli(&["validate", "--request", request.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("PRIVATE-INVALID-CODE"));
    if !cfg!(feature = "efs-write") {
        std::fs::write(
            &request,
            json!({"command":"efs_diag_open","args":{"serial":"sha256:bad"}}).to_string(),
        )
        .unwrap();
        let output = cli(&[
            "run",
            "--request",
            request.to_str().unwrap(),
            "--data-dir",
            state.to_str().unwrap(),
            "--allow-device-write",
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(!state.exists());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert!(error["error"].as_str().unwrap().contains("Cargo feature"));
    }
}
