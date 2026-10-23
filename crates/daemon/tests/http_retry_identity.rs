//! Retry identity is exercised through the actual finish and work-start routes.
use sdi_db::Paths;
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command, sync::Arc};

struct Fixture {
    root: PathBuf,
    paths: Arc<Paths>,
    server: tokio::task::JoinHandle<()>,
    base: String,
    client: reqwest::Client,
    plan: String,
    scenario: String,
    round: String,
}

async fn serve(paths: Arc<Paths>) -> (String, tokio::task::JoinHandle<()>) {
    let pool = sdi_db::open(&paths).unwrap();
    let app = sdi_daemon::router::build(sdi_daemon::AppState::new(pool, paths));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (base, server)
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.server.abort();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Fixture {
    async fn new() -> Self {
        let root = std::env::temp_dir().join(format!("sdi-cause-{}", ulid::Ulid::new()));
        std::fs::create_dir_all(root.join("repo/src")).unwrap();
        std::fs::write(root.join("repo/src/lib.rs"), "pub fn f() {}\n").unwrap();
        for args in [
            vec!["init", "-q"],
            vec!["add", "."],
            vec![
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "-qm",
                "fixture",
            ],
        ] {
            assert!(Command::new("git")
                .arg("-C")
                .arg(root.join("repo"))
                .args(args)
                .status()
                .unwrap()
                .success());
        }
        let paths = Arc::new(Paths {
            data: root.clone(),
            cache: root.clone(),
            config: root.clone(),
            state: root.clone(),
            db_file: root.join("db.sqlite"),
            pid_file: root.join("pid"),
            port_file: root.join("port"),
            socket_file: root.join("sock"),
            log_file: root.join("log"),
            lock_file: root.join("lock"),
        });
        let (base, server) = serve(paths.clone()).await;
        let mut f = Self {
            root,
            paths,
            base,
            server,
            client: reqwest::Client::new(),
            plan: String::new(),
            scenario: String::new(),
            round: String::new(),
        };
        let p = f
            .ok(
                "/projects",
                json!({"key":"IDENTITY", "name":"identity", "slug":"identity"}),
            )
            .await;
        let plan = f
            .ok(
                "/plans",
                json!({"project_id":p["id"], "short_code":"P", "title":"identity"}),
            )
            .await;
        f.plan = plan["id"].as_str().unwrap().to_owned();
        let s = f.ok("/scenarios", json!({"plan_id":f.plan, "short_code":"S", "given":"a failure", "when":"finished", "then":"book once", "confirmed":true})).await;
        f.scenario = s["id"].as_str().unwrap().to_owned();
        f.ok(&format!("/plans/{}/approve", f.plan), json!({})).await;
        let r = f
            .ok("/rounds", json!({"plan_id":f.plan, "short_code":"R1"}))
            .await;
        f.round = r["id"].as_str().unwrap().to_owned();
        f.ok(&format!("/rounds/{}/activate", f.round), json!({}))
            .await;
        f
    }
    async fn post(&self, path: &str, body: Value) -> (u16, Value) {
        let r = self
            .client
            .post(format!("{}{path}", self.base))
            .json(&body)
            .send()
            .await
            .unwrap();
        (r.status().as_u16(), r.json().await.unwrap())
    }
    async fn ok(&self, path: &str, body: Value) -> Value {
        let (status, body) = self.post(path, body).await;
        assert!(status < 300, "{path}: {status} {body}");
        body
    }
    async fn get(&self, path: &str) -> Value {
        self.client
            .get(format!("{}{path}", self.base))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap()
    }
    async fn begin(&self, report: &str) -> Value {
        let r = self.ok("/runs",json!({"scenario":self.scenario,"command":["cargo","test"],"cwd":self.root.join("repo"),"input_globs":["src/**"]})).await;
        std::fs::write(r["report_path"].as_str().unwrap(), report).unwrap();
        r
    }
    async fn fail(&self, report: &str, hint: &str) -> Value {
        let r = self.begin(report).await;
        self.ok(
            &format!("/runs/{}/finish", r["id"].as_str().unwrap()),
            json!({"exit_code":101,"failure_signature":hint}),
        )
        .await
    }
    async fn budgets(&self) -> Value {
        self.get(&format!(
            "/retry-budgets?plan_id={}&scenario={}",
            self.plan, self.scenario
        ))
        .await
    }
    async fn task(&self, name: &str) -> String {
        self.ok("/tasks",json!({"round_id":self.round,"short_code":name,"description":name,"parent_scenario_ids":[self.scenario]})).await["id"].as_str().unwrap().to_owned()
    }
    async fn restart(&mut self) {
        self.server.abort();
        let _ = (&mut self.server).await;
        (self.base, self.server) = serve(self.paths.clone()).await;
    }
    async fn seed(&self, key: &str, count: usize, class: sdi_core::verification::FailureClass) {
        for _ in 0..count {
            let r = self.ok("/runs", json!({"scenario":self.scenario})).await;
            let saved = self.ok(&format!("/runs/{}/finish",r["id"].as_str().unwrap()),json!({"result":"failure","failure_class":class.to_string(),"failure_signature":key})).await;
            assert!(saved["failure_identity"].is_null());
        }
    }
}

fn report(cause: &str, wrapper: &str) -> String {
    format!("test {wrapper} ... FAILED\nthread '{wrapper}' panicked at src/lib.rs:12:3:\nstartup failed: {cause}\n")
}

const SOCKET: &str = "Unix socket path exceeds SUN_LEN at /tmp/session-one/core.sock";
const ARTIFACT: &str = "artifact protocol/schema mismatch: expected protocol 3, got protocol 2";
const OWNER: &str = "journal database already owned by another process";

#[tokio::test]
async fn distinct_primary_causes_book_independently_and_preserve_reports() {
    let f = Fixture::new().await;
    let mut keys = std::collections::BTreeSet::new();
    for cause in [SOCKET, ARTIFACT, OWNER] {
        let output = report(cause, "restart_fixture");
        let r = f.fail(&output, "caller-may-not-choose").await;
        assert_eq!(r["failure_class"], "product");
        assert_eq!(r["error_class"], "assertion");
        assert_eq!(r["retry_budget"]["count"], 1);
        assert_eq!(r["retry_budget"]["total_failures"], 1);
        assert_eq!(r["failure_identity"]["scope"], "primary-only");
        assert_eq!(
            r["failure_identity"]["booked_signature"],
            r["failure_signature"]
        );
        keys.insert(r["failure_signature"].as_str().unwrap().to_owned());
        assert_eq!(
            std::fs::read_to_string(r["report_path"].as_str().unwrap()).unwrap(),
            output
        );
        assert_eq!(
            r["report_sha256"],
            format!(
                "sha256:{}",
                sdi_core::verification::sha256_hex(output.as_bytes())
            )
        );
        let saved = f.get(&format!("/runs/{}", r["id"].as_str().unwrap())).await;
        assert_eq!(saved["failure_identity"], r["failure_identity"]);
    }
    assert_eq!(keys.len(), 3);
    let b = f.budgets().await;
    assert_eq!(b["budgets"].as_array().unwrap().len(), 3);
    assert_eq!(b["exhausted"], 0);
}

#[tokio::test]
async fn changed_hints_wrappers_and_restart_cannot_reset_the_same_cause() {
    let mut f = Fixture::new().await;
    let mut key = Value::Null;
    for i in 0..3 {
        let task = f.task(&format!("renamed-{i}")).await;
        f.ok(
            &format!("/tasks/{task}/status"),
            json!({"status":"in_progress"}),
        )
        .await;
        let r = f
            .fail(
                &report(
                    &format!("Unix socket path exceeds SUN_LEN at /tmp/root-{i}/core.sock"),
                    &format!("wrapper_{i}"),
                ),
                &format!("hint-{i}"),
            )
            .await;
        if i == 0 {
            key = r["failure_signature"].clone();
        }
        assert_eq!(r["failure_signature"], key);
        assert_eq!(r["retry_budget"]["count"], i + 1);
        assert_eq!(
            r["failure_identity"]["caller_hint"],
            format!("caller:hint-{i}")
        );
        f.ok(
            &format!("/tasks/{task}/status"),
            json!({"status":"cancelled"}),
        )
        .await;
        if i == 1 {
            f.restart().await;
        }
    }
    let task = f.task("recreated").await;
    let (status, body) = f
        .post(
            &format!("/tasks/{task}/status"),
            json!({"status":"in_progress"}),
        )
        .await;
    assert_eq!(status, 409);
    assert_eq!(body["error"]["code"], "RETRY_BUDGET_EXHAUSTED");
}

#[tokio::test]
async fn unknown_and_diagnosed_transitions_preserve_outstanding_authority() {
    for unknown_first in [true, false] {
        let f = Fixture::new().await;
        let first = if unknown_first {
            "opaque failure".to_owned()
        } else {
            report(SOCKET, "wrapper")
        };
        let r = f.fail(&first, "one").await;
        let next = if unknown_first {
            report(SOCKET, "renamed")
        } else {
            "different opaque output 1234".to_owned()
        };
        let r2 = f.fail(&next, "two").await;
        assert_eq!(r["failure_signature"], r2["failure_signature"]);
        assert_eq!(r2["retry_budget"]["count"], 2);
        assert!(r2["failure_identity"]["compatibility_bridge"].is_string());
        let r3 = f.fail(&report(SOCKET, "another"), "three").await;
        assert_eq!(r3["failure_signature"], r["failure_signature"]);
        assert_eq!(r3["retry_budget"]["count"], 3);
        let task = f.task("blocked-after-transition").await;
        let (status, body) = f
            .post(
                &format!("/tasks/{task}/status"),
                json!({"status":"in_progress"}),
            )
            .await;
        assert_eq!(status, 409);
        assert_eq!(body["error"]["code"], "RETRY_BUDGET_EXHAUSTED");
    }
}

#[tokio::test]
async fn legacy_bridge_prioritizes_exhaustion_then_count_then_key_without_reset() {
    use sdi_core::verification::FailureClass::Product;
    let f = Fixture::new().await;
    f.seed("legacy-0", 1, Product).await;
    f.seed("legacy-b", 2, Product).await;
    f.seed("legacy-a", 2, Product).await;
    let r = f.fail(&report(SOCKET, "wrapper"), "hint").await;
    assert_eq!(r["failure_signature"], "caller:legacy-a");
    assert_eq!(r["retry_budget"]["count"], 3);
    assert_eq!(r["retry_budget"]["total_failures"], 3);
    let r2 = f.fail(&report(OWNER, "other"), "new-hint").await;
    assert_eq!(r2["failure_signature"], "caller:legacy-a");
    assert_eq!(r2["retry_budget"]["total_failures"], 4);
    assert_ne!(
        r["failure_identity"]["computed"]["signature"],
        r2["failure_identity"]["computed"]["signature"]
    );
    let b = f.budgets().await;
    assert_eq!(
        b["budgets"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["total_failures"].as_i64().unwrap())
            .sum::<i64>(),
        7
    );
}

#[tokio::test]
async fn completion_is_atomic_and_concurrent_or_duplicate_finish_debits_once() {
    let f = Fixture::new().await;
    let started = f.begin(&report(SOCKET, "wrapper")).await;
    let path = format!("/runs/{}/finish", started["id"].as_str().unwrap());
    let pool = sdi_db::open(&f.paths).unwrap();
    let conn = pool.get().unwrap();
    conn.execute_batch("CREATE TRIGGER fail_debit BEFORE INSERT ON retry_budgets BEGIN SELECT RAISE(ABORT, 'injected accounting failure'); END;").unwrap();
    let (status, _) = f.post(&path, json!({"exit_code":101})).await;
    assert!(status >= 400);
    let saved = f
        .get(&format!("/runs/{}", started["id"].as_str().unwrap()))
        .await;
    assert!(saved["finished_at"].is_null());
    assert!(saved["failure_identity"].is_null());
    conn.execute_batch("DROP TRIGGER fail_debit;").unwrap();
    let (a, b) = tokio::join!(
        f.post(&path, json!({"exit_code":101})),
        f.post(&path, json!({"exit_code":101}))
    );
    assert_eq!([a.0, b.0].iter().filter(|s| **s < 300).count(), 1);
    assert!(f.post(&path, json!({"exit_code":101})).await.0 >= 400);
    assert_eq!(f.budgets().await["budgets"][0]["total_failures"], 1);
}

#[tokio::test]
async fn distinct_typescript_diagnostics_reach_independent_daemon_budgets() {
    let f = Fixture::new().await;
    let a = f.fail("src/view.ts(12,3): error TS2540: Cannot assign to 'coreEpoch' because it is a read-only property.","a").await;
    let b = f.fail("src/adapter.ts(44,1): error TS2322: Type 'DeliveryView' is not assignable to type 'JsonValue'.", "b").await;
    assert_ne!(a["failure_signature"], b["failure_signature"]);
    assert_eq!(a["retry_budget"]["count"], 1);
    assert_eq!(b["retry_budget"]["count"], 1);
    assert_eq!(a["error_class"], "compile-error");
    assert_eq!(b["error_class"], "compile-error");
}

#[tokio::test]
async fn truncated_reports_keep_full_digest_and_use_conservative_fallback() {
    let f = Fixture::new().await;
    let first = f.fail(&report(SOCKET, "first"), "a").await;
    let output = format!(
        "{}{}{}",
        report(SOCKET, "head"),
        "x".repeat(4 * 1024 * 1024),
        report(OWNER, "tail")
    );
    let r = f.fail(&output, "b").await;
    assert_eq!(
        r["failure_identity"]["diagnostic_unavailable"],
        "report-scan-truncated"
    );
    assert!(r["failure_identity"]["computed"]["signature"]
        .as_str()
        .unwrap()
        .starts_with("sig:v2:fallback:"));
    assert_eq!(r["failure_signature"], first["failure_signature"]);
    assert_eq!(r["retry_budget"]["count"], 2);
    assert_eq!(
        r["report_sha256"],
        format!(
            "sha256:{}",
            sdi_core::verification::sha256_hex(output.as_bytes())
        )
    );
    assert_eq!(
        std::fs::read_to_string(r["report_path"].as_str().unwrap()).unwrap(),
        output
    );
}

#[tokio::test]
async fn changed_secondary_diagnostics_do_not_split_primary_accounting() {
    let f = Fixture::new().await;
    let a = f
        .fail(
            &format!("{}\n{}", report(SOCKET, "one"), report(OWNER, "secondary")),
            "a",
        )
        .await;
    let b = f
        .fail(
            &format!(
                "{}\n{}",
                report(SOCKET, "two"),
                report(ARTIFACT, "different_secondary")
            ),
            "b",
        )
        .await;
    assert_eq!(a["failure_signature"], b["failure_signature"]);
    assert_eq!(b["retry_budget"]["count"], 2);
    assert_eq!(f.budgets().await["budgets"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn manual_signature_cannot_impersonate_computed_identity() {
    for historical in [false, true] {
        let f = Fixture::new().await;
        f.seed(
            "sig:v2:forged",
            1,
            sdi_core::verification::FailureClass::Product,
        )
        .await;
        let key = if historical {
            // Model a pre-upgrade manual signature. Existing rows remain legacy
            // authority even when their caller-chosen text resembles version 2.
            let pool = sdi_db::open(&f.paths).unwrap();
            let conn = pool.get().unwrap();
            conn.execute("UPDATE runs SET failure_signature = 'sig:v2:forged' WHERE failure_signature = 'caller:sig:v2:forged'", []).unwrap();
            conn.execute("UPDATE retry_budgets SET failure_signature = 'sig:v2:forged' WHERE failure_signature = 'caller:sig:v2:forged'", []).unwrap();
            "sig:v2:forged"
        } else {
            "caller:sig:v2:forged"
        };
        let budgets = f.budgets().await;
        assert_eq!(budgets["budgets"][0]["failure_signature"], key);
        let first_id = budgets["budgets"][0]["first_run_id"].as_str().unwrap();
        let original = f.get(&format!("/runs/{first_id}")).await;
        for i in 2..=3 {
            let r = f.fail(&report(SOCKET, "wrapper"), "different-hint").await;
            assert_eq!(r["failure_signature"], key);
            assert_eq!(r["retry_budget"]["count"], i);
        }
        let preserved = f.get(&format!("/runs/{first_id}")).await;
        assert_eq!(original, preserved);
    }
}
