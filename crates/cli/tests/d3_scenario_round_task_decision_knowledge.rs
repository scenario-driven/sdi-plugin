//! D3 smoke: drives the full Plan→Scenario(confirmed)→Approve→Round.activate→
//! Task→complete-with-evidence→Round.complete→Plan.complete→Decision(+supersede)
//! →Knowledge happy path through the `sdi` binary against an auto-spawned
//! daemon under a temp $SDI_HOME.

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
            "sdi-cli-d3-{}-{}",
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
fn full_sdi_flow_via_cli() {
    let h = Harness::new();
    let suffix = ulid::Ulid::new().to_string();
    let key = format!("E{}", &suffix[..4]);
    let slug = format!("d3-{}", suffix[..6].to_lowercase());

    let project = h.run_ok(&[
        "project", "create", &key, "D3 E2E", "--slug", &slug, "--cwd", "/tmp/d3",
    ]);
    let project_id = project["id"].as_str().unwrap().to_string();

    let plan_code = format!("D3-{}", &suffix[..6]);
    let plan = h.run_ok(&[
        "plan",
        "create",
        &project_id,
        &plan_code,
        "v0.1 — D3 e2e",
        "--body",
        "ship via CLI",
    ]);
    let plan_id = plan["id"].as_str().unwrap().to_string();

    // requirement
    let req_code = format!("REQ-{}", &suffix[..6]);
    let _req = h.run_ok(&[
        "req", "create", &plan_id, &req_code, "Login", "--body", "user must log in",
    ]);

    // scenario (confirmed)
    let scn_code = format!("SCN-{}", &suffix[..6]);
    let scn = h.run_ok(&[
        "scenario", "create", &plan_id, &scn_code,
        "--given", "a registered user",
        "--when", "they submit valid credentials",
        "--then", "they receive a session token",
        "--confirmed",
    ]);
    let scn_id = scn["id"].as_str().unwrap().to_string();
    assert_eq!(scn["status"], "confirmed");

    // approve plan (D8 gate satisfied)
    let approved = h.run_ok(&["plan", "approve", &plan_id]);
    assert_eq!(approved["status"], "active");

    // round 1 create + activate
    let r1_code = format!("R1-{}", &suffix[..6]);
    let r1 = h.run_ok(&["round", "create", &plan_id, &r1_code]);
    let r1_id = r1["id"].as_str().unwrap().to_string();
    assert_eq!(r1["status"], "planning");
    assert_eq!(r1["mode"], "strict-regression");

    let act = h.run_ok(&["round", "activate", &r1_id]);
    assert_eq!(act["round"]["status"], "active");
    assert_eq!(act["carried_results"].as_u64().unwrap(), 0);

    // task create + start + complete (with evidence)
    let task_code = format!("T-{}", &suffix[..6]);
    let task = h.run_ok(&[
        "task", "create", &r1_id, &task_code,
        "implement /auth/login",
        "--scenario", &scn_id,
    ]);
    let task_id = task["id"].as_str().unwrap().to_string();
    let _started = h.run_ok(&["task", "start", &task_id]);
    let done = h.run_ok(&[
        "task", "complete", &task_id,
        "--evidence", &format!("{}=passing@auth.rs:42", scn_id),
        "--summary", "endpoint live",
    ]);
    assert_eq!(done["status"], "done");
    assert!(done["evidence_at"].is_string());

    // round result, complete round, complete plan
    let _r1_res = h.run_ok(&[
        "round", "result", &r1_id,
        "--scenario", &scn_id,
        "--result", "passing",
        "--evidence", "auth.rs:42",
    ]);
    let _r1_done = h.run_ok(&["round", "complete", &r1_id]);
    let plan_done = h.run_ok(&["plan", "complete", &plan_id]);
    assert_eq!(plan_done["status"], "completed");

    // decision + supersession
    let d1_code = format!("DEC-{}-A", &suffix[..6]);
    let d1 = h.run_ok(&[
        "decision", "create", &plan_id, &d1_code, "Use JWT", "--body", "v0.1 JWT",
    ]);
    let d1_id = d1["id"].as_str().unwrap().to_string();
    assert_eq!(d1["status"], "accepted");

    let d2_code = format!("DEC-{}-B", &suffix[..6]);
    let _d2 = h.run_ok(&[
        "decision", "supersede", &d1_id,
        "--plan-id", &plan_id,
        "--short-code", &d2_code,
        "--title", "Switch to opaque tokens",
        "--body", "JWT replay risk",
    ]);
    let d1_fresh = h.run_ok(&["decision", "view", &d1_id]);
    assert_eq!(d1_fresh["status"], "superseded");

    // knowledge: rag + reference + scope filter
    let _k_rag = h.run_ok(&[
        "knowledge", "create", &project_id,
        "--scope", "rag",
        "--kind", "decision",
        "Token strategy",
        "--body", "Opaque tokens beat JWT after replay risk.",
        "--tag", "auth",
        "--tag", "tokens",
    ]);
    let _k_ref = h.run_ok(&[
        "knowledge", "create", &project_id,
        "--scope", "reference",
        "--kind", "runbook",
        "Auth runbook",
        "--body", "Manual procedures.",
    ]);

    let all = h.run_ok(&["knowledge", "list", &project_id]);
    assert_eq!(all["knowledge"].as_array().unwrap().len(), 2);

    let rag_only = h.run_ok(&["knowledge", "list", &project_id, "--scope", "rag"]);
    let arr = rag_only["knowledge"].as_array().unwrap();
    assert_eq!(arr.len(), 1);
    assert_eq!(arr[0]["scope"], "rag");

    // search (FTS5)
    let hits = h.run_ok(&["knowledge", "search", &project_id, "opaque"]);
    let arr = hits["knowledge"].as_array().unwrap();
    assert!(!arr.is_empty());
}
