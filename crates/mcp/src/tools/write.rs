//! Write tools (PRD §5.4) — mediated mutations onto the daemon.
//!
//! Every write tool maps the LLM-friendly args onto the daemon's HTTP body,
//! POSTs it, and surfaces success or the daemon's structured error envelope.
//! The daemon stays the single source of truth for validation (D5 GWT_EMPTY,
//! D8 SCENARIOS_REQUIRED, PRD §6.6 EVIDENCE_REQUIRED, etc.) — we do not
//! re-validate here so error messages stay in one place.

use crate::protocol::{ToolCallResult, ToolDescriptor};
use crate::tools::{DaemonClient, Tool};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn all(client: Arc<DaemonClient>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(AddScenario {
            client: client.clone(),
        }),
        Arc::new(AddRequirement {
            client: client.clone(),
        }),
        Arc::new(AddDecision {
            client: client.clone(),
        }),
        Arc::new(UpdateTaskEvidence {
            client: client.clone(),
        }),
        Arc::new(StartRound { client }),
    ]
}

/// Pull a required string arg; return a tool error if it's missing.
fn require_str<'a>(args: &'a Value, name: &str) -> Result<&'a str, ToolCallResult> {
    args.get(name)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolCallResult::err_text(format!("missing required arg: {name}")))
}

// ─── add_scenario ──────────────────────────────────────────────────────────

pub struct AddScenario {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for AddScenario {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "add_scenario",
            description: "Create a Given/When/Then scenario on a plan. The daemon rejects empty \
                 GWT fields (D5 GWT_EMPTY). Set `confirmed: true` to skip the explicit \
                 confirm step when the scenario is already vetted.",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id", "short_code", "given", "when", "then"],
                "properties": {
                    "plan_id": { "type": "string" },
                    "short_code": { "type": "string" },
                    "given": { "type": "string" },
                    "when": { "type": "string" },
                    "then": { "type": "string" },
                    "confirmed": { "type": "boolean", "default": false }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match require_str(&args, "plan_id") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let short_code = match require_str(&args, "short_code") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let given = match require_str(&args, "given") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let when_clause = match require_str(&args, "when") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let then_clause = match require_str(&args, "then") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let confirmed = args
            .get("confirmed")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        let body = json!({
            "plan_id": plan_id,
            "short_code": short_code,
            "given": given,
            "when": when_clause,
            "then": then_clause,
            "confirmed": confirmed,
        });
        match self.client.post_json("/scenarios", &body).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── add_requirement ───────────────────────────────────────────────────────

pub struct AddRequirement {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for AddRequirement {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "add_requirement",
            description: "Author a requirement snapshot on a plan (D12 SNAPSHOT-only — there is \
                 no history, the row carries today's truth). Use `source` to record \
                 where the requirement came from (interview, ticket, etc.).",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id", "short_code", "title", "body"],
                "properties": {
                    "plan_id": { "type": "string" },
                    "short_code": { "type": "string" },
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "source": { "type": "string", "description": "Origin (free text)" }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match require_str(&args, "plan_id") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let short_code = match require_str(&args, "short_code") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let title = match require_str(&args, "title") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let body_txt = match require_str(&args, "body") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let mut body = json!({
            "plan_id": plan_id,
            "short_code": short_code,
            "title": title,
            "body": body_txt,
        });
        if let Some(src) = args.get("source").and_then(|v| v.as_str()) {
            body["source"] = Value::String(src.to_string());
        }
        match self.client.post_json("/requirements", &body).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── add_decision ──────────────────────────────────────────────────────────

pub struct AddDecision {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for AddDecision {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "add_decision",
            description: "Append a decision (ADR) to a plan's append-only log. Set \
                 `supersedes_id` to chain a replacement: the daemon auto-flips the \
                 predecessor to `superseded` (D12).",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id", "short_code", "title", "body"],
                "properties": {
                    "plan_id": { "type": "string" },
                    "short_code": { "type": "string" },
                    "title": { "type": "string" },
                    "body": { "type": "string" },
                    "supersedes_id": {
                        "type": "string",
                        "description": "ID of the prior decision being replaced"
                    }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match require_str(&args, "plan_id") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let short_code = match require_str(&args, "short_code") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let title = match require_str(&args, "title") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let body_txt = match require_str(&args, "body") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let mut body = json!({
            "plan_id": plan_id,
            "short_code": short_code,
            "title": title,
            "body": body_txt,
        });
        if let Some(sup) = args.get("supersedes_id").and_then(|v| v.as_str()) {
            body["supersedes_id"] = Value::String(sup.to_string());
        }
        match self.client.post_json("/decisions", &body).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── update_task_evidence ──────────────────────────────────────────────────

pub struct UpdateTaskEvidence {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for UpdateTaskEvidence {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "update_task_evidence",
            description: "Complete a task with evidence (PRD §6.6 EVIDENCE_REQUIRED). Each \
                 evidence entry maps one scenario to a verdict (`passing` / `failing` \
                 / `impacted` / `retired`) and an evidence reference (file:line, URL, \
                 log path). Verdict vocab matches the daemon's ScenarioResult — no \
                 `skipped`; use `impacted` for a scenario the change broke or \
                 `retired` for one superseded by a new flow.",
            input_schema: json!({
                "type": "object",
                "required": ["task_id", "scenarios"],
                "properties": {
                    "task_id": { "type": "string" },
                    "summary": { "type": "string" },
                    "scenarios": {
                        "type": "array",
                        "minItems": 1,
                        "items": {
                            "type": "object",
                            "required": ["scenario_id", "result", "evidence_ref"],
                            "properties": {
                                "scenario_id": { "type": "string" },
                                "result": {
                                    "type": "string",
                                    "enum": ["passing", "failing", "impacted", "retired"]
                                },
                                "evidence_ref": { "type": "string" }
                            },
                            "additionalProperties": false
                        }
                    }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let task_id = match require_str(&args, "task_id") {
            Ok(s) => s,
            Err(e) => return e,
        };
        let scenarios = match args.get("scenarios").and_then(|v| v.as_array()) {
            Some(a) if !a.is_empty() => a.clone(),
            _ => return ToolCallResult::err_text("scenarios must be a non-empty array"),
        };
        let mut body = json!({ "evidence": { "scenarios": scenarios } });
        if let Some(s) = args.get("summary").and_then(|v| v.as_str()) {
            body["evidence"]["summary"] = Value::String(s.to_string());
        }
        match self
            .client
            .post_json(&format!("/tasks/{}/complete", task_id), &body)
            .await
        {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── start_round ───────────────────────────────────────────────────────────

pub struct StartRound {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for StartRound {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "start_round",
            description: "Activate a round. In strict-regression mode (D6 default) the daemon \
                 carries `passing` results from the previous completed round forward; \
                 the response includes `carried_results` so the LLM sees the count.",
            input_schema: json!({
                "type": "object",
                "required": ["round_id"],
                "properties": {
                    "round_id": { "type": "string" }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let round_id = match require_str(&args, "round_id") {
            Ok(s) => s,
            Err(e) => return e,
        };
        match self
            .client
            .post_empty(&format!("/rounds/{}/activate", round_id))
            .await
        {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_tools_register_all_five_with_unique_names() {
        let client = Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string()));
        let tools = all(client);
        let names: Vec<&'static str> = tools.iter().map(|t| t.descriptor().name).collect();
        assert_eq!(
            names,
            vec![
                "add_scenario",
                "add_requirement",
                "add_decision",
                "update_task_evidence",
                "start_round"
            ]
        );
    }

    #[tokio::test]
    async fn update_task_evidence_rejects_empty_scenarios_before_http() {
        let t = UpdateTaskEvidence {
            client: Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string())),
        };
        let r = t
            .call(json!({ "task_id": "TASK-X", "scenarios": [] }))
            .await;
        assert!(r.is_error);
        let text = match &r.content[0] {
            crate::protocol::ContentBlock::Text { text } => text.clone(),
        };
        assert!(text.contains("non-empty"), "got: {text}");
    }
}
