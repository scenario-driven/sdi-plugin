//! D2 smoke: drives `sdi project / plan / req` against a daemon auto-started
//! under a temp $SDI_HOME. Proves the CLI ↔ daemon contract end-to-end for
//! project + plan + requirement.

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
            "sdi-cli-d2-{}-{}",
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
        // Best-effort stop so we don't leak detached daemons.
        let _ = Command::new(sdi_bin())
            .args(["daemon", "stop"])
            .env("SDI_HOME", &self.home)
            .env("PATH", bin_dir())
            .output();
    }
}

#[test]
fn project_plan_req_happy_path() {
    let h = Harness::new();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("K{}", &suffix[..4]);
    let slug = format!("d2-{}", suffix[..6].to_lowercase());

    // project create
    let project = h.run_ok(&[
        "project", "create", &key, "D2 Test", "--slug", &slug, "--cwd", "/tmp/d2",
    ]);
    let project_id = project["id"].as_str().unwrap().to_string();
    assert_eq!(project["key"], key);
    assert_eq!(project["slug"], slug);

    // project list
    let projects = h.run_ok(&["project", "list"]);
    let arr = projects["projects"].as_array().unwrap();
    assert!(arr.iter().any(|p| p["id"] == project_id));

    // project by-cwd
    let by = h.run_ok(&["project", "by-cwd", "/tmp/d2"]);
    assert_eq!(by["project"]["id"], project_id);

    // project view
    let viewed = h.run_ok(&["project", "view", &project_id]);
    assert_eq!(viewed["id"], project_id);

    // project update --name
    let renamed = h.run_ok(&["project", "update", &project_id, "--name", "D2 Renamed"]);
    assert_eq!(renamed["name"], "D2 Renamed");

    // plan create
    let plan_code = format!("D2-{}", &suffix[..6]);
    let plan = h.run_ok(&[
        "plan",
        "create",
        &project_id,
        &plan_code,
        "v0.1",
        "--body",
        "initial body",
    ]);
    let plan_id = plan["id"].as_str().unwrap().to_string();
    assert_eq!(plan["status"], "draft");

    // plan list
    let plans = h.run_ok(&["plan", "list", &project_id]);
    assert!(plans["plans"]
        .as_array()
        .unwrap()
        .iter()
        .any(|p| p["id"] == plan_id));

    // plan update --body only (preserves title)
    let updated = h.run_ok(&["plan", "update", &plan_id, "--body", "v2 body"]);
    assert_eq!(updated["title"], "v0.1");
    assert_eq!(updated["body"], "v2 body");

    // requirement create
    let req_code = format!("REQ-{}", &suffix[..6]);
    let req = h.run_ok(&[
        "req",
        "create",
        &plan_id,
        &req_code,
        "Auth required",
        "--body",
        "users must log in",
    ]);
    let req_id = req["id"].as_str().unwrap().to_string();
    assert_eq!(req["title"], "Auth required");

    // requirement update --body only (SNAPSHOT overwrite)
    let req2 = h.run_ok(&["req", "update", &req_id, "--body", "users must SSO"]);
    assert_eq!(req2["body"], "users must SSO");
    assert_eq!(req2["title"], "Auth required");

    // requirement list
    let reqs = h.run_ok(&["req", "list", &plan_id]);
    assert_eq!(reqs["requirements"].as_array().unwrap().len(), 1);

    // plan approve — should fail with SCENARIOS_REQUIRED (no confirmed scenario)
    let (code, _stdout, stderr) = h.run(&["plan", "approve", &plan_id]);
    assert_ne!(code, 0);
    assert!(
        stderr.contains("SCENARIOS_REQUIRED"),
        "expected SCENARIOS_REQUIRED in: {stderr}"
    );

    // requirement delete
    let deleted = h.run_ok(&["req", "delete", &req_id]);
    assert_eq!(deleted["deleted"], true);
}
