//! HTTP integration: verification-run integrity.
//!
//! Covers:
//! - run-ref evidence (`SCN=verdict@run:<RUN-ID>`) is validated strictly:
//!   missing / unlinked / failed / stale runs are rejected, a fresh pass is
//!   accepted, free-text refs keep working
//! - retry budget keyed by failure signature survives task cancel / recreate /
//!   rename; exhaustion refuses starts and unproven completions with
//!   RETRY_BUDGET_EXHAUSTED; a decision releases it
//! - harness failures do not consume the product budget, cap themselves, and
//!   block round completion until a passing run exists
//! - a carried run-ref pass whose fingerprint no longer matches is demoted to
//!   needs_verification on activation (ref kept)
//! - activation records the baseline; an active round's baseline cannot be
//!   overwritten without a superseding decision
//! - round results / next report baseline progress separately from scenarios
//!   added after the baseline
//! - reuse check reports fresh vs stale

use sdi_daemon::AppState;
use sdi_db::Paths;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> String {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-verify-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(&tmp).unwrap();
    let paths = Arc::new(Paths {
        data: tmp.clone(),
        cache: tmp.clone(),
        config: tmp.clone(),
        state: tmp.clone(),
        db_file: tmp.join("sdi.db"),
        pid_file: tmp.join("sdid.pid"),
        port_file: tmp.join("sdid.port"),
        socket_file: tmp.join("sdid.sock"),
        log_file: tmp.join("sdid.log"),
        lock_file: tmp.join("sdid.lock"),
    });
    let pool = sdi_db::open(&paths).expect("open db");
    let state = AppState::new(pool, paths);
    let app = sdi_daemon::router::build(state);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    base
}

fn git_repo() -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "sdi-verify-repo-{}-{}",
        std::process::id(),
        ulid::Ulid::new()
    ));
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    for args in [
        vec!["init", "-q"],
        vec!["add", "."],
        vec![
            "-c",
            "user.email=t@t",
            "-c",
            "user.name=t",
            "commit",
            "-qm",
            "i",
        ],
    ] {
        assert!(Command::new("git")
            .arg("-C")
            .arg(&dir)
            .args(&args)
            .status()
            .unwrap()
            .success());
    }
    dir.canonicalize().unwrap()
}

struct Api {
    base: String,
    http: reqwest::Client,
}

impl Api {
    fn new(base: String) -> Self {
        Self {
            base,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(20))
                .build()
                .unwrap(),
        }
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .http
            .post(format!("{}{}", self.base, path))
            .json(&body)
            .send()
            .await
            .unwrap();
        let status = r.status().as_u16();
        (status, r.json().await.unwrap_or(Value::Null))
    }
    async fn ok(&self, path: &str, body: Value) -> Value {
        let (status, v) = self.post(path, body).await;
        assert!(status < 300, "POST {path} -> {status}: {v}");
        v
    }
    async fn get(&self, path: &str) -> Value {
        let r = self
            .http
            .get(format!("{}{}", self.base, path))
            .send()
            .await
            .unwrap();
        assert!(r.status().is_success(), "GET {path} -> {}", r.status());
        r.json().await.unwrap()
    }
    async fn err_code(&self, path: &str, body: Value) -> String {
        let (status, v) = self.post(path, body).await;
        assert!(
            status >= 400,
            "expected failure for {path}, got {status}: {v}"
        );
        v["error"]["code"].as_str().unwrap_or("").to_string()
    }
}

struct Fixture {
    api: Api,
    project_id: String,
    plan_id: String,
    round_id: String,
    scenarios: Vec<String>,
    repo: PathBuf,
}

async fn fixture(n_scenarios: usize) -> Fixture {
    let api = Api::new(spawn_server().await);
    let suffix = ulid::Ulid::new().to_string();
    let repo = git_repo();
    let project = api
        .ok(
            "/projects",
            json!({"key": format!("V{}", &suffix[..4]), "name": "v",
                   "slug": format!("v-{}", suffix[..8].to_lowercase()),
                   "cwds": [repo.to_string_lossy()]}),
        )
        .await;
    let project_id = project["id"].as_str().unwrap().to_string();
    let plan = api
        .ok(
            "/plans",
            json!({"project_id": project_id, "short_code": format!("P-{}", &suffix[..6]),
                   "title": "v", "body": ""}),
        )
        .await;
    let plan_id = plan["id"].as_str().unwrap().to_string();
    let mut scenarios = Vec::new();
    for i in 0..n_scenarios {
        let s = api
            .ok(
                "/scenarios",
                json!({"plan_id": plan_id, "short_code": format!("SC-{i}"),
                       "given": format!("g{i}"), "when": "w", "then": "t", "confirmed": true}),
            )
            .await;
        scenarios.push(s["id"].as_str().unwrap().to_string());
    }
    api.ok(&format!("/plans/{plan_id}/approve"), json!({}))
        .await;
    let round = api
        .ok("/rounds", json!({"plan_id": plan_id, "short_code": "R1"}))
        .await;
    let round_id = round["id"].as_str().unwrap().to_string();
    api.ok(&format!("/rounds/{round_id}/activate"), json!({}))
        .await;
    Fixture {
        api,
        project_id,
        plan_id,
        round_id,
        scenarios,
        repo,
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.repo);
    }
}

impl Fixture {
    async fn task(&self, short: &str, scenario: &str) -> String {
        let t = self
            .api
            .ok(
                "/tasks",
                json!({"round_id": self.round_id, "short_code": short, "description": short,
                       "parent_scenario_ids": [scenario]}),
            )
            .await;
        t["id"].as_str().unwrap().to_string()
    }

    async fn start(&self, task: &str) -> (u16, Value) {
        self.api
            .post(
                &format!("/tasks/{task}/status"),
                json!({"status": "in_progress"}),
            )
            .await
    }

    /// Record a verification run: start (daemon fingerprints the repo), write
    /// the report the CLI would have captured, finish with the exit status.
    async fn run(&self, scenario: &str, output: &str, finish: Value) -> Value {
        let started = self
            .api
            .ok(
                "/runs",
                json!({"scenario": scenario, "command": ["cargo", "test"],
                       "cwd": self.repo.to_string_lossy(), "input_globs": ["src/**"]}),
            )
            .await;
        assert!(started["input_fingerprint"]
            .as_str()
            .unwrap()
            .starts_with("sha256:"));
        let report = started["report_path"].as_str().unwrap();
        std::fs::write(report, output).unwrap();
        let id = started["id"].as_str().unwrap();
        self.api.ok(&format!("/runs/{id}/finish"), finish).await
    }

    async fn pass(&self, scenario: &str) -> String {
        let r = self
            .run(scenario, "test ok ... ok\n", json!({"exit_code": 0}))
            .await;
        assert_eq!(r["result"], "success");
        assert_eq!(r["failure_class"], "none");
        r["id"].as_str().unwrap().to_string()
    }

    async fn product_failure(&self, scenario: &str) -> Value {
        let out = "running 1 test\ntest api::creates_user ... FAILED\n\
                   thread 'api::creates_user' panicked at src/lib.rs:1:1:\n\
                   assertion `left == right` failed\n";
        self.run(scenario, out, json!({"exit_code": 101})).await
    }

    async fn complete(&self, task: &str, evidence: &str) -> (u16, Value) {
        let (scn, rest) = evidence.split_once('=').unwrap();
        let (verdict, r) = rest.split_once('@').unwrap();
        self.api
            .post(
                &format!("/tasks/{task}/complete"),
                json!({"evidence": {"scenarios": [
                    {"scenario_id": scn, "result": verdict, "evidence_ref": r}
                ]}}),
            )
            .await
    }

    async fn decision(&self, short: &str) -> String {
        let d = self
            .api
            .ok(
                "/decisions",
                json!({"plan_id": self.plan_id, "short_code": short, "title": short,
                       "body": "escalated"}),
            )
            .await;
        d["id"].as_str().unwrap().to_string()
    }
}

#[tokio::test]
async fn run_ref_evidence_is_validated_strictly() {
    let f = fixture(2).await;
    let (s0, s1) = (&f.scenarios[0], &f.scenarios[1]);
    let task = f.task("T-1", s0).await;
    f.start(&task).await;

    // Missing run.
    let (st, v) = f
        .complete(&task, &format!("{s0}=passing@run:RUN-NOPE"))
        .await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (400, Some("EVIDENCE_RUN_INVALID"))
    );

    // Run linked to another scenario.
    let other = f.pass(s1).await;
    let (_, v) = f
        .complete(&task, &format!("{s0}=passing@run:{other}"))
        .await;
    assert_eq!(v["error"]["code"], "EVIDENCE_RUN_INVALID");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("not linked"));

    // Failed run cannot evidence a pass.
    let failed = f.product_failure(s0).await;
    let failed_id = failed["id"].as_str().unwrap();
    let (_, v) = f
        .complete(&task, &format!("{s0}=passing@run:{failed_id}"))
        .await;
    assert_eq!(v["error"]["code"], "EVIDENCE_RUN_INVALID");
    assert!(v["error"]["message"]
        .as_str()
        .unwrap()
        .contains("did not pass"));

    // Stale run: a passing run, then an input changes.
    let good = f.pass(s0).await;
    std::fs::write(f.repo.join("src/lib.rs"), "pub fn f() -> u8 { 2 }\n").unwrap();
    let (st, v) = f.complete(&task, &format!("{s0}=passing@run:{good}")).await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (409, Some("EVIDENCE_STALE"))
    );
    // The task is untouched by the rejected completions.
    let t = f.api.get(&format!("/tasks/{task}")).await;
    assert_eq!(t["status"], "in_progress");

    // Restoring the inputs makes the same run fresh again; it is accepted
    // and the verdict mirrors into the round with its run ref.
    std::fs::write(f.repo.join("src/lib.rs"), "pub fn f() -> u8 { 1 }\n").unwrap();
    let (st, v) = f.complete(&task, &format!("{s0}=passing@run:{good}")).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["status"], "done");
    let results = f.api.get(&format!("/rounds/{}/results", f.round_id)).await;
    let row = results["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["scenario_id"] == s0.as_str())
        .unwrap();
    assert_eq!(row["evidence_ref"], format!("run:{good}"));

    // Free-text refs keep working (backward compatible).
    let task2 = f.task("T-2", s1).await;
    f.start(&task2).await;
    let (st, _) = f
        .complete(&task2, &format!("{s1}=passing@src/lib.rs:1"))
        .await;
    assert_eq!(st, 200);

    // The daemon-assigned verdict cannot be submitted.
    let code = f
        .api
        .err_code(
            &format!("/rounds/{}/results", f.round_id),
            json!({"scenario_id": s1, "result": "needs_verification"}),
        )
        .await;
    assert_eq!(code, "VALIDATION");
}

#[tokio::test]
async fn retry_budget_survives_task_cancel_recreate_and_rename() {
    let f = fixture(1).await;
    let s0 = &f.scenarios[0];

    // Three same-cause product failures under three different tasks.
    let mut sigs = Vec::new();
    for (i, short) in ["FIX-1", "FIX-1-again", "totally-new-name"]
        .iter()
        .enumerate()
    {
        let task = f.task(short, s0).await;
        let (st, v) = f.start(&task).await;
        assert_eq!(st, 200, "start #{i}: {v}");
        let r = f.product_failure(s0).await;
        assert_eq!(r["failure_class"], "product");
        assert_eq!(r["error_class"], "assertion");
        assert_eq!(r["failing_test"], "api::creates_user");
        sigs.push(r["failure_signature"].as_str().unwrap().to_string());
        assert_eq!(r["retry_budget"]["count"], (i + 1) as i64);
        f.api
            .ok(
                &format!("/tasks/{task}/status"),
                json!({"status": "cancelled"}),
            )
            .await;
    }
    assert!(
        sigs.windows(2).all(|w| w[0] == w[1]),
        "same cause, same signature"
    );

    // Budget spent: a brand-new task (new name) cannot start.
    let fresh = f.task("rewrite-from-scratch", s0).await;
    let (st, v) = f.start(&fresh).await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (409, Some("RETRY_BUDGET_EXHAUSTED"))
    );
    assert!(v["error"]["message"].as_str().unwrap().contains("decision"));

    let budgets = f
        .api
        .get(&format!(
            "/retry-budgets?plan_id={}&scenario={s0}",
            f.plan_id
        ))
        .await;
    assert_eq!(budgets["exhausted"], 1);
    assert_eq!(budgets["budgets"][0]["count"], 3);

    // `sdi next` steers to escalation.
    let next = f.api.get(&format!("/projects/{}/next", f.project_id)).await;
    assert!(next["command"].as_str().unwrap().contains("release-budget"));

    // A decision releases it; work can resume.
    let ghost = f
        .api
        .err_code(
            "/retry-budgets/release",
            json!({"scenario": s0, "decision_id": "DEC-NOPE"}),
        )
        .await;
    assert_eq!(ghost, "VALIDATION");
    let dec = f.decision("ESC-1").await;
    let rel = f
        .api
        .ok(
            "/retry-budgets/release",
            json!({"scenario": s0, "decision_id": dec}),
        )
        .await;
    assert_eq!(rel["released"], 1);
    assert_eq!(rel["budgets"][0]["count"], 0);
    assert_eq!(rel["budgets"][0]["total_failures"], 3);
    let (st, v) = f.start(&fresh).await;
    assert_eq!(st, 200, "{v}");
}

#[tokio::test]
async fn exhausted_budget_refuses_unproven_completion_but_accepts_fresh_pass() {
    let f = fixture(1).await;
    let s0 = &f.scenarios[0];
    let task = f.task("T-1", s0).await;
    f.start(&task).await;
    for _ in 0..3 {
        f.product_failure(s0).await;
    }
    // A free-text pass on an exhausted scenario is the loop the budget stops.
    let (st, v) = f.complete(&task, &format!("{s0}=passing@trust-me")).await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (409, Some("RETRY_BUDGET_EXHAUSTED"))
    );
    // A failing completion likewise.
    let (_, v) = f.complete(&task, &format!("{s0}=failing@log.txt")).await;
    assert_eq!(v["error"]["code"], "RETRY_BUDGET_EXHAUSTED");
    // A fresh, run-proven pass ends the loop.
    let good = f.pass(s0).await;
    let (st, v) = f.complete(&task, &format!("{s0}=passing@run:{good}")).await;
    assert_eq!(st, 200, "{v}");
}

#[tokio::test]
async fn harness_failures_do_not_consume_product_budget_but_cap_themselves() {
    let f = fixture(1).await;
    let s0 = &f.scenarios[0];
    let task = f.task("T-1", s0).await;
    f.start(&task).await;

    // Two product failures.
    for _ in 0..2 {
        f.product_failure(s0).await;
    }
    // Harness: command not found (127) → environment; timeout → harness.
    let missing = f
        .run(
            s0,
            "sh: cargo: command not found\n",
            json!({"exit_code": 127}),
        )
        .await;
    assert_eq!(missing["failure_class"], "environment");
    let timeout = f
        .run(s0, "running…\n", json!({"timed_out": true, "signal": 9}))
        .await;
    assert_eq!(timeout["failure_class"], "harness");
    assert_eq!(timeout["error_class"], "timeout");

    let budgets = f
        .api
        .get(&format!(
            "/retry-budgets?scenario={s0}&plan_id={}",
            f.plan_id
        ))
        .await;
    let rows = budgets["budgets"].as_array().unwrap();
    let product = rows
        .iter()
        .find(|b| b["failure_class"] == "product")
        .unwrap();
    assert_eq!(
        product["count"], 2,
        "harness failures must not consume the product budget"
    );
    assert_eq!(budgets["exhausted"], 0);

    // The harness loop caps itself under its own signature.
    for _ in 0..2 {
        f.run(s0, "running…\n", json!({"timed_out": true})).await;
    }
    let budgets = f
        .api
        .get(&format!(
            "/retry-budgets?scenario={s0}&plan_id={}",
            f.plan_id
        ))
        .await;
    let harness = budgets["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["failure_class"] == "harness")
        .unwrap()
        .clone();
    assert_eq!(harness["count"], 3);
    assert_eq!(harness["exhausted"], true);
    let product = budgets["budgets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["failure_class"] == "product")
        .unwrap()
        .clone();
    assert_eq!(product["count"], 2);
    let other = f.task("T-2", s0).await;
    let (_, v) = f.start(&other).await;
    assert_eq!(v["error"]["code"], "RETRY_BUDGET_EXHAUSTED");

    // Round completion is blocked while the latest run is a harness failure…
    let code = f
        .api
        .err_code(&format!("/rounds/{}/complete", f.round_id), json!({}))
        .await;
    assert_eq!(code, "HARNESS_FAILURE_UNRESOLVED");
    // …and unblocked once a passing run exists.
    f.pass(s0).await;
    f.api
        .ok(&format!("/rounds/{}/complete", f.round_id), json!({}))
        .await;
}

#[tokio::test]
async fn caller_overrides_class_and_signature() {
    let f = fixture(1).await;
    let s0 = &f.scenarios[0];
    let r = f
        .run(
            s0,
            "weird output\n",
            json!({"exit_code": 1, "failure_class": "spec", "failure_signature": "ambiguous-then"}),
        )
        .await;
    assert_eq!(r["failure_class"], "spec");
    assert_eq!(r["failure_signature"], "caller:ambiguous-then");
    // A success cannot carry a failure class.
    let started = f
        .api
        .ok(
            "/runs",
            json!({"scenario": s0, "command": ["true"], "cwd": f.repo.to_string_lossy()}),
        )
        .await;
    let code = f
        .api
        .err_code(
            &format!("/runs/{}/finish", started["id"].as_str().unwrap()),
            json!({"exit_code": 0, "failure_class": "product"}),
        )
        .await;
    assert_eq!(code, "VALIDATION");
}

#[tokio::test]
async fn stale_fingerprint_demotes_carried_pass_on_activation() {
    let f = fixture(2).await;
    let (s0, s1) = (&f.scenarios[0], &f.scenarios[1]);
    let t0 = f.task("T-0", s0).await;
    f.start(&t0).await;
    let good = f.pass(s0).await;
    assert_eq!(
        f.complete(&t0, &format!("{s0}=passing@run:{good}")).await.0,
        200
    );
    let t1 = f.task("T-1", s1).await;
    f.start(&t1).await;
    assert_eq!(
        f.complete(&t1, &format!("{s1}=passing@notes.md")).await.0,
        200
    );
    f.api
        .ok(&format!("/rounds/{}/complete", f.round_id), json!({}))
        .await;

    // Inputs change between rounds.
    std::fs::write(f.repo.join("src/lib.rs"), "pub fn f() -> u8 { 9 }\n").unwrap();
    let r2 = f
        .api
        .ok("/rounds", json!({"plan_id": f.plan_id, "short_code": "R2"}))
        .await;
    let r2_id = r2["id"].as_str().unwrap();
    let act = f
        .api
        .ok(
            &format!("/rounds/{r2_id}/activate"),
            json!({"commit": "abc123"}),
        )
        .await;
    assert_eq!(act["carried_results"], 2);
    let stale = act["stale_carried"].as_array().unwrap();
    assert_eq!(stale.len(), 1);
    assert_eq!(stale[0]["scenario_id"], s0.as_str());
    assert_eq!(stale[0]["run_id"], good.as_str());

    let results = f.api.get(&format!("/rounds/{r2_id}/results")).await;
    let by = |sid: &str| {
        results["results"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["scenario_id"] == sid)
            .unwrap()
            .clone()
    };
    let row0 = by(s0);
    assert_eq!(row0["result"], "needs_verification");
    assert_eq!(
        row0["evidence_ref"],
        format!("run:{good}"),
        "ref is kept, not dropped"
    );
    assert!(row0["note"].as_str().unwrap().contains("stale"));
    // Free-text passes carry verbatim with their ref.
    let row1 = by(s1);
    assert_eq!(row1["result"], "passing");
    assert_eq!(row1["evidence_ref"], "notes.md");
    // The demoted scenario is back in the verification queue.
    let needs = act["scenarios_needing_verification"].as_array().unwrap();
    assert!(needs.iter().any(|n| n["scenario_id"] == s0.as_str()));
    // Activation baseline recorded the commit and the post-carry verdicts.
    assert_eq!(act["baseline"]["commit"], "abc123");
    let snap = act["baseline"]["scenarios"].as_array().unwrap();
    assert!(snap
        .iter()
        .any(|e| e["scenario_id"] == s0.as_str() && e["verdict"] == "needs_verification"));
}

#[tokio::test]
async fn active_round_baseline_overwrite_requires_superseding_decision() {
    let f = fixture(1).await;
    let rid = &f.round_id;
    let got = f.api.get(&format!("/rounds/{rid}/baseline")).await;
    assert!(got["activation"]["snapshot"]["scenarios"].is_array());

    // First write of the caller blob on an active round is not an overwrite.
    f.api
        .ok(
            &format!("/rounds/{rid}/baseline"),
            json!({"baseline_json": {"green_tests": 10}}),
        )
        .await;
    // Overwrite without a decision is refused.
    let (st, v) = f
        .api
        .post(
            &format!("/rounds/{rid}/baseline"),
            json!({"baseline_json": {"green_tests": 99}}),
        )
        .await;
    assert_eq!(
        (st, v["error"]["code"].as_str()),
        (409, Some("BASELINE_LOCKED"))
    );
    let got = f.api.get(&format!("/rounds/{rid}/baseline")).await;
    assert_eq!(got["baseline"]["green_tests"], 10);
    // Rebase without a decision is refused too.
    let code = f
        .api
        .err_code(&format!("/rounds/{rid}/baseline"), json!({"rebase": true}))
        .await;
    assert_eq!(code, "BASELINE_LOCKED");

    // With a superseding decision of the plan: allowed and recorded.
    let dec = f.decision("BASE-1").await;
    let v = f
        .api
        .ok(
            &format!("/rounds/{rid}/baseline"),
            json!({"baseline_json": {"green_tests": 99}, "decision_id": dec}),
        )
        .await;
    assert_eq!(v["baseline"]["green_tests"], 99);
    assert_eq!(v["superseded_by_decision_id"], dec.as_str());
}

#[tokio::test]
async fn progress_reports_baseline_separately_from_added_scenarios() {
    let f = fixture(2).await;
    let (s0, s1) = (&f.scenarios[0], &f.scenarios[1]);

    // A scenario added after activation, verified immediately.
    let added = f
        .api
        .ok(
            "/scenarios",
            json!({"plan_id": f.plan_id, "short_code": "SC-new", "given": "g", "when": "w",
                   "then": "t", "confirmed": true}),
        )
        .await;
    let added_id = added["id"].as_str().unwrap().to_string();
    let ta = f.task("T-A", &added_id).await;
    f.start(&ta).await;
    let run = f.pass(&added_id).await;
    assert_eq!(
        f.complete(&ta, &format!("{added_id}=passing@run:{run}"))
            .await
            .0,
        200
    );

    let res = f.api.get(&format!("/rounds/{}/results", f.round_id)).await;
    let p = &res["progress"];
    assert_eq!(p["baseline_recorded"], true);
    assert_eq!(p["baseline"]["total"], 2);
    assert_eq!(p["baseline"]["passing"], 0);
    assert_eq!(
        p["baseline"]["percent"], 0.0,
        "added scenario must not inflate baseline %"
    );
    assert_eq!(p["added_after_baseline"]["total"], 1);
    assert_eq!(p["added_after_baseline"]["passing"], 1);

    let t0 = f.task("T-0", s0).await;
    f.start(&t0).await;
    let r0 = f.pass(s0).await;
    assert_eq!(
        f.complete(&t0, &format!("{s0}=passing@run:{r0}")).await.0,
        200
    );
    let next = f.api.get(&format!("/projects/{}/next", f.project_id)).await;
    assert_eq!(next["progress"]["baseline"]["percent"], 50.0);
    assert_eq!(next["progress"]["added_after_baseline"]["percent"], 100.0);
    let _ = s1;
}

#[tokio::test]
async fn reuse_check_reports_fresh_then_stale() {
    let f = fixture(1).await;
    let s0 = &f.scenarios[0];
    let none = f.api.ok("/runs/reuse-check", json!({"scenario": s0})).await;
    assert_eq!(none["status"], "none");
    let run = f.pass(s0).await;
    let fresh = f.api.ok("/runs/reuse-check", json!({"scenario": s0})).await;
    assert_eq!(fresh["status"], "fresh");
    assert_eq!(fresh["reuse_allowed"], true);
    assert_eq!(fresh["run_id"], run.as_str());
    // Candidate mode: identical command + inputs → reuse.
    let cand = f
        .api
        .ok(
            "/runs/reuse-check",
            json!({"scenario": "SC-0", "plan_id": f.plan_id, "cwd": f.repo.to_string_lossy(),
                   "command": ["cargo", "test"], "input_globs": ["src/**"]}),
        )
        .await;
    assert_eq!(cand["reuse_allowed"], true);
    // A different command is a different fingerprint.
    let other = f
        .api
        .ok(
            "/runs/reuse-check",
            json!({"scenario": s0, "cwd": f.repo.to_string_lossy(),
                   "command": ["cargo", "test", "--release"], "input_globs": ["src/**"]}),
        )
        .await;
    assert_eq!(other["status"], "stale");
    std::fs::write(f.repo.join("src/new.rs"), "x\n").unwrap();
    let stale = f.api.ok("/runs/reuse-check", json!({"scenario": s0})).await;
    assert_eq!(stale["status"], "stale");
    assert_eq!(stale["reuse_allowed"], false);
    let _ = Path::new("");
}
