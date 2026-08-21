//! v0.5 pattern lifecycle + reversibility + claim ledger HTTP integration.
//!
//! Covers the deliverable surface added by SDI-124:
//! 1. POST /patterns + GET /patterns/active + PATCH /patterns/:id/lifecycle
//!    with shape validation at `pending → active`.
//! 2. D24 depth cap: a chain that exceeds AutonomyPolicy.pattern_depth_cap
//!    returns 400.
//! 3. Decision rollback (D28): the rollback endpoint appends an immutable
//!    `consensus` decision whose `reversal_of` points at the original; the
//!    original is left untouched.
//! 4. Scenario claim ledger (D29): claim → active-claims surface →
//!    release round-trip; SSE event kinds carry the documented payload.
//! 5. SSE event-kind surface check: every v0.5 envelope variant
//!    (`pattern_created`, `pattern_lifecycle`, `claim_status_changed`,
//!    `rollback_initiated`, `rollback_completed`) is observable on /events.

use futures::StreamExt;
use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-v05-{}-{}",
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
    let addr = listener.local_addr().unwrap();
    let base = format!("http://{}", addr);
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;
    (base, handle)
}

fn cli() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn bootstrap(base: &str, suffix: &str) -> (String, String) {
    let c = cli();
    let key = format!("P{}", &suffix[..4]);
    let slug = format!("p-{}", suffix[..6].to_lowercase());
    let proj: serde_json::Value = c
        .post(format!("{}/projects", base))
        .json(&serde_json::json!({"key": key, "name": "PT", "slug": slug}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let project_id = proj["id"].as_str().unwrap().to_string();
    let plan_code = format!("P-{}", &suffix[..6]);
    let plan: serde_json::Value = c
        .post(format!("{}/plans", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "short_code": plan_code,
            "title": "v",
            "body": ""
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let plan_id = plan["id"].as_str().unwrap().to_string();
    (project_id, plan_id)
}

// ─── 1. CRUD + shape validation ────────────────────────────────────────────

#[tokio::test]
async fn pattern_create_and_lifecycle_workflow_happy_path() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    // workflow with 2 steps satisfies D26.
    let created: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-WF-{}", &suffix[..6]),
            "kind": "workflow",
            "applies_to": "scenario",
            "scope_id": plan_id,
            "steps": [
                {"idx": 0, "agent": "impl-coder", "action": "draft"},
                {"idx": 1, "agent": "test-runner", "action": "verify"}
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(created["kind"], "workflow");
    assert_eq!(created["lifecycle"], "pending");
    assert_eq!(created["depth"], 0);
    let pattern_id = created["id"].as_str().unwrap().to_string();

    // pending → active: shape gate passes.
    let activated: serde_json::Value = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pattern_id))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(activated["lifecycle"], "active");

    // Visible in the active surface.
    let active: serde_json::Value = c
        .get(format!("{}/patterns/active", base))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = active["patterns"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| p["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&pattern_id.as_str()));

    // Terminal transition stamps decided_at.
    let converged: serde_json::Value = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pattern_id))
        .json(&serde_json::json!({"to": "converged", "reason": "done"}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(converged["lifecycle"], "converged");
    assert!(converged["decided_at"].is_string());
    assert_eq!(converged["decided_reason"], "done");

    // Further transitions out of terminal are rejected.
    let bad = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pattern_id))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 400);
}

#[tokio::test]
async fn pattern_shape_gate_rejects_underspec_workflow_at_activation() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    // Single-step workflow — fake pattern, D26 shape gate must reject at activation.
    let created: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-WF1-{}", &suffix[..6]),
            "kind": "workflow",
            "applies_to": "scenario",
            "scope_id": plan_id,
            "steps": [
                {"idx": 0, "agent": "impl-coder", "action": "draft"}
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pattern_id = created["id"].as_str().unwrap();

    let activated = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pattern_id))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap();
    assert_eq!(activated.status().as_u16(), 400);
}

#[tokio::test]
async fn pattern_shape_gate_rejects_sybil_graph() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    // Two reviewers, same (name, stance) tuple — sybil, D26 rejects.
    let created: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-GR-{}", &suffix[..6]),
            "kind": "graph",
            "applies_to": "decision",
            "scope_id": plan_id,
            "reviewers": [
                {"name": "schema-architect", "stance": "schema_guardian"},
                {"name": "schema-architect", "stance": "schema_guardian"}
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pattern_id = created["id"].as_str().unwrap();
    let bad = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pattern_id))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 400);

    // Now upgrade the manifest contract by recreating with distinct tuples and
    // confirm the gate accepts. (Patterns are append-only; re-issue under a
    // fresh short_code.)
    let ok: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-GR2-{}", &suffix[..6]),
            "kind": "graph",
            "applies_to": "decision",
            "scope_id": plan_id,
            "reviewers": [
                {"name": "schema-architect", "stance": "schema_guardian"},
                {"name": "impl-coder", "stance": "devil_advocate"}
            ]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pid = ok["id"].as_str().unwrap();
    let activated = c
        .patch(format!("{}/patterns/{}/lifecycle", base, pid))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap();
    assert!(activated.status().is_success());
}

// ─── 2. D24 depth cap ──────────────────────────────────────────────────────

#[tokio::test]
async fn pattern_depth_cap_blocks_chain_over_limit() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (project_id, plan_id) = bootstrap(&base, &suffix).await;

    // Tighten the cap to 2 for this project so a 3-deep chain trips.
    let resp = c
        .post(format!("{}/autonomy_policies", base))
        .json(&serde_json::json!({
            "project_id": project_id,
            "scope_kind": "global",
            "mode": "L5",
            "pattern_depth_cap": 2,
        }))
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Build chain root → child1 → child2 (depth 0,1,2 ok). The next level
    // would be depth 3 which exceeds the cap of 2.
    let root: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-R-{}", &suffix[..6]),
            "kind": "direct",
            "applies_to": "plan",
            "scope_id": plan_id,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let root_id = root["id"].as_str().unwrap().to_string();

    let child1: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-C1-{}", &suffix[..6]),
            "kind": "direct",
            "applies_to": "plan",
            "scope_id": plan_id,
            "parent_pattern_id": root_id,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let child1_id = child1["id"].as_str().unwrap().to_string();
    assert_eq!(child1["depth"], 1);

    let child2: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-C2-{}", &suffix[..6]),
            "kind": "direct",
            "applies_to": "plan",
            "scope_id": plan_id,
            "parent_pattern_id": child1_id,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let child2_id = child2["id"].as_str().unwrap().to_string();
    assert_eq!(child2["depth"], 2);

    // depth 3 exceeds cap of 2 — daemon rejects with 400.
    let over = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-C3-{}", &suffix[..6]),
            "kind": "direct",
            "applies_to": "plan",
            "scope_id": plan_id,
            "parent_pattern_id": child2_id,
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(over.status().as_u16(), 400);
}

// ─── 3. D28 rollback ───────────────────────────────────────────────────────

#[tokio::test]
async fn decision_rollback_appends_immutable_consensus_with_reversal_of() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    // Seed a decision to be the rollback target. The minimal viable shape is
    // an accepted standalone proposal (kind=proposal, status defaults to
    // accepted in the daemon).
    let orig: serde_json::Value = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-O-{}", &suffix[..6]),
            "title": "to roll back",
            "body": "b",
            "kind": "proposal",
            "reversal_plan": "{\"type\":\"git_revert\",\"sha\":\"abc1234\"}",
            "blast_radius_score": 3,
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let orig_id = orig["id"].as_str().unwrap().to_string();
    assert_eq!(orig["status"], "accepted");
    assert_eq!(orig["reversal_of"], serde_json::Value::Null);

    // Rollback. Daemon appends a new consensus row whose reversal_of = orig.
    let rb: serde_json::Value = c
        .post(format!("{}/decisions/{}/rollback", base, orig_id))
        .json(&serde_json::json!({
            "short_code": format!("DEC-RB-{}", &suffix[..6]),
            "title": "revert it",
            "body": "rolling back",
            "reversal_plan": "{\"type\":\"git_revert\",\"sha\":\"abc1234\"}",
            "blast_radius_score": 2,
            "agent_name": "reversal-runner",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(rb["kind"], "consensus");
    assert_eq!(rb["reversal_of"], orig_id);
    assert_eq!(rb["plan_id"], plan_id);
    assert_eq!(rb["agent_name"], "reversal-runner");

    // Original is untouched (D12 snapshot-only).
    let after: serde_json::Value = c
        .get(format!("{}/decisions/{}", base, orig_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(after["status"], "accepted");
    assert_eq!(after["reversal_of"], serde_json::Value::Null);
}

#[tokio::test]
async fn decision_rollback_rejects_malformed_reversal_plan() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    let orig: serde_json::Value = c
        .post(format!("{}/decisions", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("DEC-MR-{}", &suffix[..6]),
            "title": "t",
            "body": "b",
            "kind": "proposal",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let orig_id = orig["id"].as_str().unwrap();
    // Empty SHA — discriminated-union validator must reject.
    let bad = c
        .post(format!("{}/decisions/{}/rollback", base, orig_id))
        .json(&serde_json::json!({
            "short_code": format!("DEC-MR2-{}", &suffix[..6]),
            "title": "t",
            "body": "b",
            "reversal_plan": "{\"type\":\"git_revert\",\"sha\":\"  \"}",
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status().as_u16(), 400);
}

// ─── 4. D29 claim ledger ───────────────────────────────────────────────────

#[tokio::test]
async fn scenario_claim_release_roundtrips_and_surfaces_active_ledger() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_project_id, plan_id) = bootstrap(&base, &suffix).await;

    let scn: serde_json::Value = c
        .post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("US-SCN-{}", &suffix[..6]),
            "given": "g",
            "when": "w",
            "then": "t",
            "tags": ["v05"],
            "claimed_resources_json": "[\"src/foo/**\"]",
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let scn_id = scn["id"].as_str().unwrap().to_string();
    assert_eq!(scn["claim_status"], "none");
    assert_eq!(scn["claimed_resources_json"], "[\"src/foo/**\"]");

    // Claim → active surface picks it up.
    let claimed: serde_json::Value = c
        .post(format!("{}/scenarios/{}/claim", base, scn_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(claimed["claim_status"], "active");

    let active: serde_json::Value = c
        .get(format!(
            "{}/scenarios/active-claims?plan_id={}",
            base, plan_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = active["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&scn_id.as_str()));

    // Release → active surface drops it.
    let released: serde_json::Value = c
        .post(format!("{}/scenarios/{}/release", base, scn_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(released["claim_status"], "released");

    let after: serde_json::Value = c
        .get(format!(
            "{}/scenarios/active-claims?plan_id={}",
            base, plan_id
        ))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = after["scenarios"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s["id"].as_str().unwrap())
        .collect();
    assert!(!ids.contains(&scn_id.as_str()));
}

// ─── 5. SSE event-kind surface ─────────────────────────────────────────────

/// Subscribe to /events first, then drive the daemon through the v0.5
/// envelope-emitting calls and assert every documented `event:` line shows up.
#[tokio::test]
async fn sse_emits_all_v05_event_kinds() {
    let (base, _h) = spawn_server().await;

    // Open SSE first — `event:` lines are line-delimited; we just scan the
    // text stream for the type tokens.
    let stream_client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();
    let resp = stream_client
        .get(format!("{}/events", base))
        .send()
        .await
        .expect("sse connect");
    assert!(resp.status().is_success());
    let mut stream = resp.bytes_stream();

    // Driver: spawn off all the v0.5 calls in parallel with the listener so
    // events buffer into the broadcast channel.
    let drive_base = base.clone();
    let driver = tokio::spawn(async move {
        let c = cli();
        let suffix = ulid::Ulid::new().to_string();
        let (_project_id, plan_id) = bootstrap(&drive_base, &suffix).await;

        // 1) pattern_created + pattern_lifecycle.
        let pat: serde_json::Value = c
            .post(format!("{}/patterns", drive_base))
            .json(&serde_json::json!({
                "plan_id": plan_id,
                "short_code": format!("CP-SSE-{}", &suffix[..6]),
                "kind": "workflow",
                "applies_to": "scenario",
                "scope_id": plan_id,
                "steps": [
                    {"idx": 0, "agent": "a", "action": "x"},
                    {"idx": 1, "agent": "b", "action": "y"}
                ]
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let pid = pat["id"].as_str().unwrap().to_string();
        c.patch(format!("{}/patterns/{}/lifecycle", drive_base, pid))
            .json(&serde_json::json!({"to": "active"}))
            .send()
            .await
            .unwrap();

        // 2) claim_status_changed via scenario claim.
        let scn: serde_json::Value = c
            .post(format!("{}/scenarios", drive_base))
            .json(&serde_json::json!({
                "plan_id": plan_id,
                "short_code": format!("US-SSE-{}", &suffix[..6]),
                "given": "g",
                "when": "w",
                "then": "t",
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let scn_id = scn["id"].as_str().unwrap().to_string();
        c.post(format!("{}/scenarios/{}/claim", drive_base, scn_id))
            .send()
            .await
            .unwrap();

        // 3) rollback_initiated + rollback_completed.
        let dec: serde_json::Value = c
            .post(format!("{}/decisions", drive_base))
            .json(&serde_json::json!({
                "plan_id": plan_id,
                "short_code": format!("DEC-SSE-{}", &suffix[..6]),
                "title": "t",
                "body": "b",
                "kind": "proposal",
            }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        let did = dec["id"].as_str().unwrap().to_string();
        c.post(format!("{}/decisions/{}/rollback", drive_base, did))
            .json(&serde_json::json!({
                "short_code": format!("DEC-SSE-RB-{}", &suffix[..6]),
                "title": "rb",
                "body": "b",
                "reversal_plan": "{\"type\":\"git_revert\",\"sha\":\"deadbee\"}",
            }))
            .send()
            .await
            .unwrap();
    });

    // Events are UNNAMED (default `message` type): the kind travels inside
    // the JSON envelope, which is what the dashboard's `onmessage` consumer
    // dispatches on. A named event (`event: <kind>`) is delivered only to
    // addEventListener('<kind>') listeners and silently starves onmessage —
    // the live-update regression this asserts against.
    let needed: &[&str] = &[
        "\"kind\":\"pattern_created\"",
        "\"kind\":\"pattern_lifecycle\"",
        "\"kind\":\"claim_status_changed\"",
        "\"kind\":\"rollback_initiated\"",
        "\"kind\":\"rollback_completed\"",
    ];
    let mut buffer = String::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if needed.iter().all(|n| buffer.contains(n)) {
            break;
        }
        if tokio::time::Instant::now() >= deadline {
            panic!(
                "missing SSE event kinds: {:?}\nseen so far:\n{}",
                needed
                    .iter()
                    .filter(|n| !buffer.contains(**n))
                    .collect::<Vec<_>>(),
                buffer
            );
        }
        let chunk = tokio::time::timeout(Duration::from_millis(200), stream.next()).await;
        if let Ok(Some(Ok(bytes))) = chunk {
            buffer.push_str(&String::from_utf8_lossy(&bytes));
        }
    }
    driver.await.unwrap();
}

// ─── 6. Task pattern binding (D23 decompose-time) ──────────────────────────
//
// Regression for the "everything is `direct`" gap: a task could only ever
// resolve the plan's `direct` sentinel because POST /tasks had no way to carry
// a chosen pattern. With `produced_via_pattern_id`, the orchestrator's
// validated pattern binds; bad/unscoped/non-active references are rejected so
// they cannot silently inherit the L3 cap.

async fn approve_and_activate_round(base: &str, plan_id: &str, suffix: &str) -> String {
    let c = cli();
    c.post(format!("{}/scenarios", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("SCN-{}", &suffix[..6]),
            "given": "g", "when": "w", "then": "t", "confirmed": true
        }))
        .send()
        .await
        .unwrap();
    c.post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    let r: serde_json::Value = c
        .post(format!("{}/rounds", base))
        .json(
            &serde_json::json!({"plan_id": plan_id, "short_code": format!("R1-{}", &suffix[..6])}),
        )
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let r_id = r["id"].as_str().unwrap().to_string();
    c.post(format!("{}/rounds/{}/activate", base, r_id))
        .send()
        .await
        .unwrap();
    r_id
}

async fn mk_active_swarm(base: &str, plan_id: &str, scope_id: &str, suffix: &str) -> String {
    let c = cli();
    let created: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-SW-{}", &suffix[..6]),
            "kind": "swarm",
            "applies_to": "round",
            "scope_id": scope_id,
            "fan_out": ["impl-coder", "impl-coder", "test-runner"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let id = created["id"].as_str().unwrap().to_string();
    c.patch(format!("{}/patterns/{}/lifecycle", base, id))
        .json(&serde_json::json!({"to": "active"}))
        .send()
        .await
        .unwrap();
    id
}

#[tokio::test]
async fn task_binds_explicit_active_pattern() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = bootstrap(&base, &suffix).await;
    let round_id = approve_and_activate_round(&base, &plan_id, &suffix).await;
    let pat_id = mk_active_swarm(&base, &plan_id, &round_id, &suffix).await;

    let task: serde_json::Value = c
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "parallel fan-out work",
            "produced_via_pattern_id": pat_id
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();

    // Bound to the chosen swarm — NOT the direct sentinel.
    assert_eq!(task["produced_via_pattern_id"], pat_id);
}

#[tokio::test]
async fn task_without_pattern_falls_back_to_direct() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = bootstrap(&base, &suffix).await;
    let round_id = approve_and_activate_round(&base, &plan_id, &suffix).await;

    let task: serde_json::Value = c
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "solo work"
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let bound = task["produced_via_pattern_id"]
        .as_str()
        .unwrap()
        .to_string();

    // The resolved pattern is the plan's direct sentinel.
    let pat: serde_json::Value = c
        .get(format!("{}/patterns/{}", base, bound))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(pat["kind"], "direct");
}

#[tokio::test]
async fn task_rejects_unresolvable_pattern() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = bootstrap(&base, &suffix).await;
    let round_id = approve_and_activate_round(&base, &plan_id, &suffix).await;

    let resp = c
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "bad ref",
            "produced_via_pattern_id": "PAT-does-not-exist"
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "unresolvable pattern ref must be rejected"
    );
}

#[tokio::test]
async fn task_rejects_pending_pattern() {
    let (base, _h) = spawn_server().await;
    let c = cli();
    let suffix = ulid::Ulid::new().to_string();
    let (_pid, plan_id) = bootstrap(&base, &suffix).await;
    let round_id = approve_and_activate_round(&base, &plan_id, &suffix).await;

    // Created but NOT transitioned to active → still `pending`.
    let created: serde_json::Value = c
        .post(format!("{}/patterns", base))
        .json(&serde_json::json!({
            "plan_id": plan_id,
            "short_code": format!("CP-PEND-{}", &suffix[..6]),
            "kind": "swarm",
            "applies_to": "round",
            "scope_id": round_id,
            "fan_out": ["impl-coder", "impl-coder"]
        }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pat_id = created["id"].as_str().unwrap().to_string();

    let resp = c
        .post(format!("{}/tasks", base))
        .json(&serde_json::json!({
            "round_id": round_id,
            "short_code": format!("T-{}", &suffix[..6]),
            "description": "binds unshaped pending",
            "produced_via_pattern_id": pat_id
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(
        resp.status(),
        400,
        "pending (unshaped) pattern must be rejected"
    );
}
