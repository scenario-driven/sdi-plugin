//! Tool registry and the trait every tool implements.
//!
//! A tool is a name + schema + async invocation. Read tools call the daemon
//! and filter results to `scope=rag`; write tools call the daemon and surface
//! its structured error envelope. The registry holds them in a flat vec so
//! `tools/list` and `tools/call` dispatch are O(n) — fine at our tool count.

use crate::protocol::{ToolCallResult, ToolDescriptor};
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

pub mod read;
pub mod write;

/// Single tool surface contract.
///
/// `descriptor()` is read by `tools/list`; `call(args)` is dispatched by
/// `tools/call`. Implementations are owned by the registry and shared via
/// `Arc` so the server can dispatch concurrently without cloning logic.
#[async_trait]
pub trait Tool: Send + Sync {
    fn descriptor(&self) -> ToolDescriptor;
    async fn call(&self, args: Value) -> ToolCallResult;
}

/// Daemon client used by every tool. Concrete `BaseClient` is small enough
/// that we expose its accessor here rather than building a separate facade.
pub struct DaemonClient {
    base: String,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(base: String) -> Self {
        Self {
            base,
            http: reqwest::Client::new(),
        }
    }
    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }
    pub fn http(&self) -> &reqwest::Client {
        &self.http
    }
    /// GET a JSON path, decoding the daemon's `{ error: { code, message } }`
    /// envelope into an `Err(String)` so tools can surface it verbatim.
    pub async fn get_json(&self, path: &str) -> Result<Value, String> {
        let resp = self
            .http
            .get(self.url(path))
            .send()
            .await
            .map_err(|e| format!("daemon GET {path}: {e}"))?;
        decode(resp).await
    }
    pub async fn post_json(&self, path: &str, body: &Value) -> Result<Value, String> {
        let resp = self
            .http
            .post(self.url(path))
            .json(body)
            .send()
            .await
            .map_err(|e| format!("daemon POST {path}: {e}"))?;
        decode(resp).await
    }
    pub async fn post_empty(&self, path: &str) -> Result<Value, String> {
        let resp = self
            .http
            .post(self.url(path))
            .send()
            .await
            .map_err(|e| format!("daemon POST {path}: {e}"))?;
        decode(resp).await
    }
}

async fn decode(resp: reqwest::Response) -> Result<Value, String> {
    let status = resp.status();
    let text = resp
        .text()
        .await
        .map_err(|e| format!("read daemon body: {e}"))?;
    if status.is_success() {
        serde_json::from_str(&text).map_err(|e| format!("bad daemon JSON: {e}\n{text}"))
    } else {
        // Surface the daemon's structured error if present so the LLM sees
        // codes like SCENARIOS_REQUIRED / GWT_EMPTY / EVIDENCE_REQUIRED.
        if let Ok(v) = serde_json::from_str::<Value>(&text) {
            if let Some(err) = v.get("error") {
                let code = err
                    .get("code")
                    .and_then(|c| c.as_str())
                    .unwrap_or("DAEMON_ERROR");
                let msg = err
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or(text.as_str());
                return Err(format!("{} {}: {}", status, code, msg));
            }
        }
        Err(format!("HTTP {}: {}", status, text))
    }
}

/// Static registry. Built once at server start; mutation after that is not
/// supported (matches the `listChanged: false` capability we advertise).
pub struct Registry {
    tools: Vec<Arc<dyn Tool>>,
}

impl Registry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }
    pub fn with(mut self, tool: Arc<dyn Tool>) -> Self {
        self.tools.push(tool);
        self
    }
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.tools.iter().map(|t| t.descriptor()).collect()
    }
    pub fn find(&self, name: &str) -> Option<Arc<dyn Tool>> {
        self.tools
            .iter()
            .find(|t| t.descriptor().name == name)
            .cloned()
    }
    pub fn len(&self) -> usize {
        self.tools.len()
    }
    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}
