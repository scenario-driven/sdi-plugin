//! HTTP integration: the D34 oracle gate (PRD-v2).
//!
//! Proves the NEW approve semantics that supersede D8:
//! - A plan that targets a UserFlow is "oracle-scoped" → approve is blocked
//!   until every step of the (confirmed) flow is covered by a confirmed
//!   DetailScenario anchored to that step.
//! - The project-level `/oracle/verify` reports L0 / L1 / question completeness.

use sdi_daemon::AppState;
use sdi_db::Paths;
use std::sync::Arc;
use std::time::Duration;

async fn spawn_server() -> (String, tokio::task::JoinHandle<()>) {
    let tmp = std::env::temp_dir().join(format!(
        "sdid-oracle-{}-{}",
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

fn c() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .unwrap()
}

async fn post(cli: &reqwest::Client, url: String, body: serde_json::Value) -> reqwest::Response {
    cli.post(url).json(&body).send().await.unwrap()
}

async fn id_of(resp: reqwest::Response) -> String {
    let v: serde_json::Value = resp.json().await.unwrap();
    v["id"].as_str().expect("id in response").to_string()
}

#[tokio::test]
async fn d34_gate_blocks_until_flow_steps_covered() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let sfx = ulid::Ulid::new().to_string();

    // project + draft plan
    let project_id = id_of(
        post(
            &cli,
            format!("{}/projects", base),
            serde_json::json!({
                "key": format!("O{}", &sfx[..3]),
                "name": "Oracle",
                "slug": format!("o-{}", sfx[..6].to_lowercase()),
            }),
        )
        .await,
    )
    .await;
    let plan_id = id_of(
        post(
            &cli,
            format!("{}/plans", base),
            serde_json::json!({
                "project_id": project_id, "short_code": format!("O-{}", &sfx[..6]), "title": "p",
            }),
        )
        .await,
    )
    .await;

    // L0 nodes: persona + capability
    let persona_id = id_of(
        post(
            &cli,
            format!("{}/projects/{}/ssot-nodes", base, project_id),
            serde_json::json!({ "short_code": format!("SN-P-{}", &sfx[..6]), "kind": "Persona", "title": "Buyer", "facets_json": "{\"business\":{\"purpose\":\"구매를 완료하려는 사용자\"}}" }),
        )
        .await,
    )
    .await;
    let cap_id = id_of(
        post(
            &cli,
            format!("{}/projects/{}/ssot-nodes", base, project_id),
            serde_json::json!({ "short_code": format!("SN-C-{}", &sfx[..6]), "kind": "Capability", "title": "Checkout", "facets_json": "{\"business\":{\"purpose\":\"장바구니를 결제로 전환한다\"}}" }),
        )
        .await,
    )
    .await;

    // L1 flow with two steps, covering the capability, then confirm it
    let flow_id = id_of(
        post(
            &cli,
            format!("{}/projects/{}/user-flows", base, project_id),
            serde_json::json!({
                "short_code": format!("UF-{}", &sfx[..6]),
                "persona_id": persona_id,
                "purpose": "결제를 완료한다",
                "steps_json": "[{\"idx\":0,\"description\":\"cart\"},{\"idx\":1,\"description\":\"pay\"}]",
                "covers_capabilities_json": format!("[\"{}\"]", cap_id),
            }),
        )
        .await,
    )
    .await;
    let r = cli
        .post(format!("{}/user-flows/{}/confirm", base, flow_id))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // target the flow → plan becomes oracle-scoped (D34)
    let r = cli
        .post(format!(
            "{}/plans/{}/target-flows/{}",
            base, plan_id, flow_id
        ))
        .send()
        .await
        .unwrap();
    assert!(r.status().is_success());

    // approve now → BLOCKED (both steps uncovered)
    let r = cli
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_client_error(),
        "expected D34 block, got {}",
        r.status()
    );

    // cover step 0 with a confirmed scenario
    post(
        &cli,
        format!("{}/scenarios", base),
        serde_json::json!({
            "plan_id": plan_id, "short_code": format!("SCN-0-{}", &sfx[..6]),
            "given": "g", "when": "w", "then": "t", "confirmed": true,
            "belongs_to_flow_id": flow_id, "covers_flow_step": "0",
        }),
    )
    .await;

    // still BLOCKED (step 1 uncovered)
    let r = cli
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_client_error(),
        "expected block on uncovered step 1, got {}",
        r.status()
    );

    // cover step 1
    post(
        &cli,
        format!("{}/scenarios", base),
        serde_json::json!({
            "plan_id": plan_id, "short_code": format!("SCN-1-{}", &sfx[..6]),
            "given": "g", "when": "w", "then": "t", "confirmed": true,
            "belongs_to_flow_id": flow_id, "covers_flow_step": "1",
        }),
    )
    .await;

    // now approve SUCCEEDS
    let r = cli
        .post(format!("{}/plans/{}/approve", base, plan_id))
        .send()
        .await
        .unwrap();
    assert!(
        r.status().is_success(),
        "expected approve, got {}",
        r.status()
    );
    let plan: serde_json::Value = r.json().await.unwrap();
    assert_eq!(plan["status"], "active");
    assert!(plan["approved_at"].is_string());

    // oracle verify: L0 + L1 complete, no open questions
    let verify: serde_json::Value = cli
        .get(format!("{}/projects/{}/oracle/verify", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(verify["l0"]["complete"], true);
    assert_eq!(verify["l1"]["complete"], true);
    assert_eq!(verify["questions"]["open"], 0);
    assert_eq!(verify["oracle_complete"], true);
}

/// D35 — answering a decision question deterministically compiles into the
/// oracle: it closes the targeted OPEN marker (and sets the facet) atomically,
/// so the oracle converges with each answer.
#[tokio::test]
async fn answer_compiles_into_oracle_closing_open_marker() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let sfx = ulid::Ulid::new().to_string();

    let project_id = id_of(
        post(
            &cli,
            format!("{}/projects", base),
            serde_json::json!({
                "key": format!("Q{}", &sfx[..3]),
                "name": "QEngine",
                "slug": format!("q-{}", sfx[..6].to_lowercase()),
            }),
        )
        .await,
    )
    .await;

    // a node carrying one OPEN marker → facet-incomplete
    let node_id = id_of(
        post(
            &cli,
            format!("{}/projects/{}/ssot-nodes", base, project_id),
            serde_json::json!({
                "short_code": format!("SN-{}", &sfx[..6]),
                "kind": "Persona",
                "title": "Buyer",
                "open_markers_json": "[{\"id\":\"m1\",\"field\":\"purpose\",\"description\":\"확정 필요\"}]",
            }),
        )
        .await,
    )
    .await;

    let v1: serde_json::Value = cli
        .get(format!("{}/projects/{}/oracle/verify", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v1["l0"]["facet_incomplete_nodes"], 1);
    assert_eq!(v1["oracle_complete"], false);

    // a preference question scoped to that marker
    let q_id = id_of(
        post(
            &cli,
            format!("{}/projects/{}/decision-questions", base, project_id),
            serde_json::json!({
                "short_code": format!("DQ-{}", &sfx[..6]),
                "qtype": "preference",
                "context_md": "이 페르소나의 목적은 무엇인가",
                "scope_ref": format!("{}#m1", node_id),
            }),
        )
        .await,
    )
    .await;

    // answer → deterministic compile: close marker m1 + set the decided facet
    let r = post(
        &cli,
        format!("{}/decision-questions/{}/answer", base, q_id),
        serde_json::json!({
            "free_text": "재시도 유도",
            "apply_node_id": node_id,
            "resolve_marker_id": "m1",
            "apply_facets_json": "{\"business\":{\"purpose\":\"재시도 유도\"}}",
        }),
    )
    .await;
    assert!(r.status().is_success());

    // marker closed + question cleared (the answer→compile mechanism)
    let v2: serde_json::Value = cli
        .get(format!("{}/projects/{}/oracle/verify", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v2["l0"]["facet_incomplete_nodes"], 0);
    assert_eq!(v2["questions"]["open"], 0);
    // …but the oracle is still NOT complete: a lone persona with zero
    // capabilities has no backbone, so the D34 vacuous-complete guard keeps the
    // verdict false (closing a marker must not green-light an unspecified product).
    assert_eq!(v2["l1"]["has_backbone"], false);
    assert_eq!(v2["oracle_complete"], false);
}

/// A brand-new project (zero nodes) must not read as complete. Without the
/// backbone guard, 0 personas × 0 capabilities yields 0 uncovered pairs / 0
/// incomplete facets / 0 open questions → a vacuous `oracle_complete:true` that
/// would let an empty product definition pass the D34 approve gate.
#[tokio::test]
async fn verify_empty_oracle_is_not_vacuously_complete() {
    let (base, _h) = spawn_server().await;
    let cli = c();
    let sfx = ulid::Ulid::new().to_string();

    let project_id = id_of(
        post(
            &cli,
            format!("{}/projects", base),
            serde_json::json!({
                "key": format!("E{}", &sfx[..3]),
                "name": "Empty",
                "slug": format!("e-{}", sfx[..6].to_lowercase()),
            }),
        )
        .await,
    )
    .await;

    let v: serde_json::Value = cli
        .get(format!("{}/projects/{}/oracle/verify", base, project_id))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(v["l1"]["has_backbone"], false);
    assert_eq!(v["l1"]["persona_count"], 0);
    assert_eq!(v["l1"]["capability_count"], 0);
    assert_eq!(v["oracle_complete"], false);
}
