//! Read tools (PRD §5.4) — RAG-only surface.
//!
//! Every read tool returns only artifacts an LLM is allowed to see:
//! `scope=rag` knowledge, scenarios, decisions, and plan/task metadata.
//! `scope=reference` and `scope=archive` knowledge MUST NOT leak — we force
//! the `scope=rag` query parameter even when the caller (LLM) tries to
//! override it. This mirrors the Clawket LM invariant.

use crate::protocol::{ToolCallResult, ToolDescriptor};
use crate::tools::{DaemonClient, Tool};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

pub fn all(client: Arc<DaemonClient>) -> Vec<Arc<dyn Tool>> {
    vec![
        Arc::new(SearchKnowledge {
            client: client.clone(),
        }),
        Arc::new(SearchScenarios {
            client: client.clone(),
        }),
        Arc::new(GetPlanContext {
            client: client.clone(),
        }),
        Arc::new(GetRecentDecisions {
            client: client.clone(),
        }),
        Arc::new(GetAutonomyState {
            client: client.clone(),
        }),
        Arc::new(ListAgentNotes { client }),
    ]
}

fn urlencode(s: &str) -> String {
    // Same minimal encoder as the CLI — keeps reqwest's url crate out of
    // our dep tree and matches the daemon's URL parsing expectations.
    s.bytes()
        .map(|b| {
            if matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~') {
                (b as char).to_string()
            } else {
                format!("%{:02X}", b)
            }
        })
        .collect()
}

// ─── search_knowledge ──────────────────────────────────────────────────────

pub struct SearchKnowledge {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for SearchKnowledge {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "search_knowledge",
            description: "Full-text search across project knowledge artifacts. Returns only \
                 scope=rag entries (reference and archive scopes are intentionally \
                 invisible to LLMs).",
            input_schema: json!({
                "type": "object",
                "required": ["project_id", "q"],
                "properties": {
                    "project_id": { "type": "string", "description": "Project ID (PRJ-…)" },
                    "q": { "type": "string", "description": "FTS5 MATCH expression" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: project_id"),
        };
        let q = match args.get("q").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: q"),
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
        // LM invariant: `scope=rag` is FORCED regardless of caller args.
        let path = format!(
            "/knowledge/search?project_id={}&q={}&limit={}&scope=rag",
            urlencode(project_id),
            urlencode(q),
            limit
        );
        match self.client.get_json(&path).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── search_scenarios ──────────────────────────────────────────────────────

pub struct SearchScenarios {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for SearchScenarios {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "search_scenarios",
            description: "FTS5 search over scenarios within one plan. Returns matches with \
                 their Given/When/Then text and confirmation status.",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id", "q"],
                "properties": {
                    "plan_id": { "type": "string", "description": "Plan ID (PLN-…)" },
                    "q": { "type": "string", "description": "FTS5 MATCH expression" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match args.get("plan_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: plan_id"),
        };
        let q = match args.get("q").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: q"),
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10);
        let path = format!(
            "/scenarios/search?plan_id={}&q={}&limit={}",
            urlencode(plan_id),
            urlencode(q),
            limit
        );
        match self.client.get_json(&path).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

// ─── get_plan_context ──────────────────────────────────────────────────────

pub struct GetPlanContext {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for GetPlanContext {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "get_plan_context",
            description: "Composite plan snapshot: plan metadata, all scenarios on the plan, \
                 tasks currently in flight, and recent decisions. The shape an LLM \
                 needs to reason about a plan without issuing four calls.",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id"],
                "properties": {
                    "plan_id": { "type": "string", "description": "Plan ID (PLN-…)" }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match args.get("plan_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: plan_id"),
        };
        let plan = match self.client.get_json(&format!("/plans/{}", plan_id)).await {
            Ok(v) => v,
            Err(e) => return ToolCallResult::err_text(format!("plan: {e}")),
        };
        let scenarios = self
            .client
            .get_json(&format!("/scenarios?plan_id={}", urlencode(plan_id)))
            .await
            .map(|v| v.get("scenarios").cloned().unwrap_or(json!([])))
            .unwrap_or_else(|_| json!([]));
        let tasks_in_flight = self
            .client
            .get_json(&format!("/plans/{}/tasks/in-flight", plan_id))
            .await
            .map(|v| v.get("tasks").cloned().unwrap_or(json!([])))
            .unwrap_or_else(|_| json!([]));
        let decisions = self
            .client
            .get_json(&format!("/decisions?plan_id={}", urlencode(plan_id)))
            .await
            .map(|v| v.get("decisions").cloned().unwrap_or(json!([])))
            .unwrap_or_else(|_| json!([]));
        ToolCallResult::ok_json(&json!({
            "plan": plan,
            "scenarios": scenarios,
            "tasks_in_flight": tasks_in_flight,
            "decisions": decisions,
        }))
    }
}

// ─── get_recent_decisions ──────────────────────────────────────────────────

pub struct GetRecentDecisions {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for GetRecentDecisions {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "get_recent_decisions",
            description: "Append-only ADR log for a plan. Returns decisions ordered most-recent \
                 first, including `superseded` status so the LLM sees the full chain.",
            input_schema: json!({
                "type": "object",
                "required": ["plan_id"],
                "properties": {
                    "plan_id": { "type": "string", "description": "Plan ID (PLN-…)" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 50, "default": 10 }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let plan_id = match args.get("plan_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: plan_id"),
        };
        let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
        let path = format!("/decisions?plan_id={}", urlencode(plan_id));
        let mut v = match self.client.get_json(&path).await {
            Ok(v) => v,
            Err(e) => return ToolCallResult::err_text(e),
        };
        // Daemon returns full list ordered by repo. Trim at our layer so the
        // LLM doesn't get flooded; the repo already sorts most-recent first.
        if let Some(arr) = v.get_mut("decisions").and_then(|d| d.as_array_mut()) {
            arr.truncate(limit);
        }
        ToolCallResult::ok_json(&v)
    }
}

// ─── get_autonomy_state ────────────────────────────────────────────────────

pub struct GetAutonomyState {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for GetAutonomyState {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "get_autonomy_state",
            description: "D14/D17 autonomy snapshot for a project: every policy row plus \
                 the resolved effective policy at the caller-supplied scope. The \
                 resolve order is plan > decision_kind > global (PRD §3).",
            input_schema: json!({
                "type": "object",
                "required": ["project_id"],
                "properties": {
                    "project_id": { "type": "string" },
                    "plan_id": { "type": "string" },
                    "decision_kind": { "type": "string" }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        let project_id = match args.get("project_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => return ToolCallResult::err_text("missing required arg: project_id"),
        };
        let list_path = format!("/autonomy_policies?project_id={}", urlencode(project_id));
        let policies = match self.client.get_json(&list_path).await {
            Ok(v) => v.get("policies").cloned().unwrap_or(json!([])),
            Err(e) => return ToolCallResult::err_text(e),
        };
        let mut resolve_path = format!(
            "/autonomy_policies/resolve?project_id={}",
            urlencode(project_id)
        );
        if let Some(p) = args.get("plan_id").and_then(|v| v.as_str()) {
            resolve_path.push_str(&format!("&plan_id={}", urlencode(p)));
        }
        if let Some(k) = args.get("decision_kind").and_then(|v| v.as_str()) {
            resolve_path.push_str(&format!("&decision_kind={}", urlencode(k)));
        }
        let resolved = match self.client.get_json(&resolve_path).await {
            Ok(v) => v.get("policy").cloned().unwrap_or(Value::Null),
            Err(e) => return ToolCallResult::err_text(e),
        };
        ToolCallResult::ok_json(&json!({
            "policies": policies,
            "resolved": resolved,
        }))
    }
}

// ─── list_agent_notes ──────────────────────────────────────────────────────

pub struct ListAgentNotes {
    pub client: Arc<DaemonClient>,
}

#[async_trait]
impl Tool for ListAgentNotes {
    fn descriptor(&self) -> ToolDescriptor {
        ToolDescriptor {
            name: "list_agent_notes",
            description: "M1 blackboard view. Either `to_agent` (pending hand-offs addressed \
                 to that agent) or the pair `scope_kind` + `anchor_id` (active notes \
                 on a plan/round/scenario/task). Mutually exclusive.",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "to_agent": { "type": "string" },
                    "scope_kind": {
                        "type": "string",
                        "enum": ["plan", "round", "scenario", "task", "global"]
                    },
                    "anchor_id": { "type": "string" },
                    "kind": { "type": "string" }
                },
                "additionalProperties": false
            }),
        }
    }
    async fn call(&self, args: Value) -> ToolCallResult {
        if let Some(to) = args.get("to_agent").and_then(|v| v.as_str()) {
            let path = format!("/agent_notes/handoffs?to_agent={}", urlencode(to));
            return match self.client.get_json(&path).await {
                Ok(v) => ToolCallResult::ok_json(&v),
                Err(e) => ToolCallResult::err_text(e),
            };
        }
        let scope = match args.get("scope_kind").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolCallResult::err_text(
                    "either `to_agent` or `scope_kind`+`anchor_id` is required",
                );
            }
        };
        let anchor = match args.get("anchor_id").and_then(|v| v.as_str()) {
            Some(s) => s,
            None => {
                return ToolCallResult::err_text("`anchor_id` is required with `scope_kind`");
            }
        };
        let mut path = format!(
            "/agent_notes?scope_kind={}&anchor_id={}",
            urlencode(scope),
            urlencode(anchor)
        );
        if let Some(k) = args.get("kind").and_then(|v| v.as_str()) {
            path.push_str(&format!("&kind={}", urlencode(k)));
        }
        match self.client.get_json(&path).await {
            Ok(v) => ToolCallResult::ok_json(&v),
            Err(e) => ToolCallResult::err_text(e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_tools_register_all_six_with_unique_names() {
        let client = Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string()));
        let tools = all(client);
        let names: Vec<&'static str> = tools.iter().map(|t| t.descriptor().name).collect();
        assert_eq!(
            names,
            vec![
                "search_knowledge",
                "search_scenarios",
                "get_plan_context",
                "get_recent_decisions",
                "get_autonomy_state",
                "list_agent_notes"
            ]
        );
    }

    #[test]
    fn search_knowledge_schema_advertises_required_args() {
        let t = SearchKnowledge {
            client: Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string())),
        };
        let schema = t.descriptor().input_schema;
        assert_eq!(schema["required"], json!(["project_id", "q"]));
        assert_eq!(schema["properties"]["limit"]["maximum"], 50);
    }

    #[tokio::test]
    async fn list_agent_notes_rejects_when_neither_to_agent_nor_anchor_is_set() {
        let t = ListAgentNotes {
            client: Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string())),
        };
        let r = t.call(json!({})).await;
        assert!(r.is_error);
        let text = match &r.content[0] {
            crate::protocol::ContentBlock::Text { text } => text.clone(),
        };
        assert!(text.contains("to_agent") || text.contains("scope_kind"));
    }

    #[tokio::test]
    async fn search_knowledge_rejects_missing_project_id_without_calling_daemon() {
        // Pointing at port 1 (which never accepts connections) confirms the
        // arg-validation branch returns before any HTTP attempt.
        let t = SearchKnowledge {
            client: Arc::new(DaemonClient::new("http://127.0.0.1:1".to_string())),
        };
        let r = t.call(json!({ "q": "anything" })).await;
        assert!(r.is_error);
        assert!(matches!(
            &r.content[0],
            crate::protocol::ContentBlock::Text { text } if text.contains("project_id")
        ));
    }
}
