//! `sdi bypass arm | disarm | status` — emergency hook bypass marker CLI.
//!
//! These tests drive the binary directly so they catch CLI/clap surface
//! regressions and JSON-shape changes. Marker path resolution must stay
//! byte-identical with the hook's `bypassOnceFile()` — that contract is
//! checked indirectly by running both surfaces against the same `SDI_HOME`.

use std::path::PathBuf;
use std::process::Command;

fn sdi_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sdi"))
}

fn fresh_home() -> PathBuf {
    let tmp = std::env::temp_dir().join(format!(
        "sdi-bypass-cli-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    tmp
}

fn run(args: &[&str], home: &PathBuf) -> (i32, String, String) {
    let out = Command::new(sdi_bin())
        .args(args)
        .env("SDI_HOME", home)
        .output()
        .expect("run sdi");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn bypass_arm_writes_json_marker_with_ttl_and_reason() {
    let home = fresh_home();
    let (code, stdout, stderr) = run(
        &[
            "bypass",
            "arm",
            "--reason",
            "investigating self-deadlock",
            "--ttl",
            "120",
        ],
        &home,
    );
    assert_eq!(code, 0, "stdout={stdout} stderr={stderr}");
    let parsed: serde_json::Value = serde_json::from_str(&stdout)
        .unwrap_or_else(|e| panic!("bad json from arm: {e}\n{stdout}"));
    assert_eq!(parsed["armed"], true);
    assert_eq!(parsed["reason"], "investigating self-deadlock");
    assert_eq!(parsed["ttl_seconds"], 120);

    // Marker file at the hook-canonical path.
    let marker = home.join(".cache/sdi/bypass-once");
    assert!(marker.exists(), "marker must be written at hook path");

    let body = std::fs::read_to_string(&marker).unwrap();
    let body_json: serde_json::Value =
        serde_json::from_str(&body).expect("marker body must be JSON");
    assert_eq!(body_json["reason"], "investigating self-deadlock");
    assert_eq!(body_json["ttl_seconds"], 120);
    assert!(body_json["expires_at"].is_string());
    assert!(body_json["armed_at"].is_string());
}

#[test]
fn bypass_arm_defaults_ttl_to_60s() {
    let home = fresh_home();
    let (code, stdout, _stderr) = run(&["bypass", "arm"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["ttl_seconds"], 60);
}

#[test]
fn bypass_arm_rejects_nonpositive_ttl() {
    let home = fresh_home();
    let (code, _stdout, stderr) = run(&["bypass", "arm", "--ttl", "0"], &home);
    assert_ne!(code, 0, "ttl=0 must fail");
    assert!(
        stderr.contains("ttl") || stderr.contains("TTL") || stderr.contains("positive"),
        "expected ttl error in stderr, got: {stderr}"
    );
}

#[test]
fn bypass_status_reports_absent_when_no_marker() {
    let home = fresh_home();
    let (code, stdout, _stderr) = run(&["bypass", "status"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["state"], "absent");
}

#[test]
fn bypass_status_reports_armed_with_ttl_remainder() {
    let home = fresh_home();
    let (code, _stdout, _stderr) = run(
        &[
            "bypass",
            "arm",
            "--reason",
            "checking status",
            "--ttl",
            "300",
        ],
        &home,
    );
    assert_eq!(code, 0);
    let (code, stdout, _stderr) = run(&["bypass", "status"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["state"], "armed");
    assert_eq!(parsed["reason"], "checking status");
    let ttl_remaining = parsed["ttl_remaining_seconds"].as_i64().unwrap();
    // Allow generous slack for slow CI.
    assert!(
        (1..=300).contains(&ttl_remaining),
        "ttl_remaining outside expected window: {ttl_remaining}"
    );
}

#[test]
fn bypass_status_reports_expired_for_past_expires_at() {
    let home = fresh_home();
    // Manually plant a marker with a past expires_at.
    let cache = home.join(".cache/sdi");
    std::fs::create_dir_all(&cache).unwrap();
    let body = serde_json::json!({
        "reason": "stale",
        "armed_at": "2000-01-01T00:00:00Z",
        "expires_at": "2000-01-01T00:01:00Z",
        "ttl_seconds": 60,
    });
    std::fs::write(cache.join("bypass-once"), body.to_string()).unwrap();
    let (code, stdout, _stderr) = run(&["bypass", "status"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["state"], "expired");
    assert_eq!(parsed["ttl_remaining_seconds"], 0);
}

#[test]
fn bypass_status_handles_plain_text_marker_backward_compat() {
    let home = fresh_home();
    let cache = home.join(".cache/sdi");
    std::fs::create_dir_all(&cache).unwrap();
    std::fs::write(cache.join("bypass-once"), "legacy touch reason\n").unwrap();
    let (code, stdout, _stderr) = run(&["bypass", "status"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["state"], "armed");
    assert_eq!(parsed["reason"], "legacy touch reason");
    assert_eq!(parsed["format"], "plain-text");
}

#[test]
fn bypass_disarm_removes_marker() {
    let home = fresh_home();
    let (code, _stdout, _stderr) = run(&["bypass", "arm"], &home);
    assert_eq!(code, 0);
    let marker = home.join(".cache/sdi/bypass-once");
    assert!(marker.exists());

    let (code, stdout, _stderr) = run(&["bypass", "disarm"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["disarmed"], true);
    assert!(!marker.exists(), "marker must be gone after disarm");

    // Idempotent — disarm with no marker still succeeds and reports `disarmed=false`.
    let (code, stdout, _stderr) = run(&["bypass", "disarm"], &home);
    assert_eq!(code, 0);
    let parsed: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(parsed["disarmed"], false);
}
