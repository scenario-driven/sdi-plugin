//! D4 smoke: exercises error/exit-code semantics on the CLI surface.
//!
//! The happy path is covered by d3_scenario_round_task_decision_knowledge.rs.
//! This suite exercises the *failure* paths so we know the CLI properly
//! surfaces the daemon's structured errors (D5 GWT_EMPTY, D8 SCENARIOS_REQUIRED,
//! PRD §6.6 EVIDENCE_REQUIRED) and clap's own argument errors with stable
//! non-zero exit codes — i.e. that scripts using `sdi …` can branch on failure.

use std::path::PathBuf;
use std::process::Command;

fn sdi_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_sdi"))
}

fn bin_dir() -> PathBuf {
    sdi_bin().parent().unwrap().to_path_buf()
}

struct Harness {
    home: PathBuf,
}

impl Harness {
    fn new() -> Self {
        let home = std::env::temp_dir().join(format!(
            "sdi-cli-d4-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(&home).unwrap();
        Harness { home }
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let out = Command::new(sdi_bin())
            .args(args)
            .env("SDI_HOME", &self.home)
            .env("PATH", bin_dir())
            .output()
            .expect("run sdi");
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
            String::from_utf8_lossy(&out.stderr).into_owned(),
        )
    }
    fn run_ok(&self, args: &[&str]) -> serde_json::Value {
        let (code, stdout, stderr) = self.run(args);
        assert_eq!(code, 0, "args={args:?} stdout={stdout} stderr={stderr}");
        serde_json::from_str(&stdout).unwrap_or_else(|e| panic!("bad json: {e}\n{stdout}"))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = Command::new(sdi_bin())
            .args(["daemon", "stop"])
            .env("SDI_HOME", &self.home)
            .env("PATH", bin_dir())
            .output();
    }
}

#[test]
fn task_complete_without_evidence_flag_is_clap_error() {
    let h = Harness::new();
    // No --evidence → clap should bail at argument parsing with exit 2 and a
    // helpful "required" message; we never touch the daemon.
    let (code, _stdout, stderr) = h.run(&["task", "complete", "TASK-FAKE"]);
    assert_eq!(code, 2);
    assert!(
        stderr.contains("required") || stderr.contains("--evidence"),
        "expected clap usage error mentioning --evidence, got: {stderr}"
    );
}

#[test]
fn task_complete_with_malformed_evidence_is_anyhow_error() {
    let h = Harness::new();
    // "bare-passing-no-equals" doesn't match SCN-ID=result@evidence_ref, so
    // the CLI parser should reject it before any HTTP call.
    let (code, _stdout, stderr) = h.run(&[
        "task", "complete", "TASK-FAKE", "--evidence", "garbage",
    ]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("SCN-ID=result@evidence_ref") || stderr.contains("evidence"),
        "expected parse hint in: {stderr}"
    );
}

#[test]
fn plan_approve_without_confirmed_scenario_surfaces_d8_error() {
    let h = Harness::new();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("E{}", &suffix[..4]);
    let slug = format!("d4-{}", suffix[..6].to_lowercase());

    let project = h.run_ok(&["project", "create", &key, "D4 Err", "--slug", &slug]);
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan = h.run_ok(&[
        "plan",
        "create",
        &project_id,
        &format!("D4-{}", &suffix[..6]),
        "draft plan",
    ]);
    let plan_id = plan["id"].as_str().unwrap().to_string();

    let (code, _stdout, stderr) = h.run(&["plan", "approve", &plan_id]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("SCENARIOS_REQUIRED"),
        "expected SCENARIOS_REQUIRED in: {stderr}"
    );
    assert!(
        stderr.contains("400"),
        "expected 400 status in: {stderr}"
    );
}

#[test]
fn scenario_create_with_empty_then_surfaces_d5_gwt_empty() {
    let h = Harness::new();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("G{}", &suffix[..4]);

    let project = h.run_ok(&["project", "create", &key, "D4 GWT"]);
    let project_id = project["id"].as_str().unwrap().to_string();
    let plan = h.run_ok(&[
        "plan",
        "create",
        &project_id,
        &format!("D4-{}", &suffix[..6]),
        "p",
    ]);
    let plan_id = plan["id"].as_str().unwrap().to_string();

    let (code, _stdout, stderr) = h.run(&[
        "scenario", "create", &plan_id,
        &format!("SCN-{}", &suffix[..6]),
        "--given", "x",
        "--when", "y",
        "--then", "   ",
    ]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("GWT_EMPTY"),
        "expected GWT_EMPTY in: {stderr}"
    );
}

#[test]
fn doctor_exit_code_is_zero_on_clean_home() {
    let h = Harness::new();
    let (code, stdout, _stderr) = h.run(&["doctor"]);
    assert_eq!(code, 0);
    let v: serde_json::Value = serde_json::from_str(&stdout).unwrap();
    assert_eq!(v["paths"]["lm8"]["ok"], true);
    assert_eq!(v["db"]["ok"], true);
}
