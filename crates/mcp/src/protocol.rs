//! JSON-RPC 2.0 wire types + MCP-specific result shapes.
//!
//! The MCP spec extends JSON-RPC 2.0 — request/response/error are unchanged,
//! and the method namespace adds `initialize`, `initialized` (notification),
//! `tools/list`, `tools/call`. We mirror the bits we use rather than pulling
//! in a heavyweight rmcp dependency; the surface here is small and stable.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 request frame. `id` is absent for notifications.
#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[allow(dead_code)]
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    #[serde(default)]
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 response frame. Exactly one of `result` / `error` is `Some`.
#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
    pub id: Value,
}

impl Response {
    pub fn ok(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: Some(result),
            error: None,
            id,
        }
    }
    pub fn err(id: Value, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
            id,
        }
    }
    pub fn err_with_data(id: Value, code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: Some(data),
            }),
            id,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Standard JSON-RPC error codes (subset we actually emit).
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
}

/// Server info returned by `initialize`. We pin the MCP protocol version we
/// implement; clients may negotiate down but we don't downgrade silently.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone, Serialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: &'static str,
    pub capabilities: Capabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

#[derive(Debug, Clone, Serialize)]
pub struct Capabilities {
    pub tools: ToolsCapability,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolsCapability {
    /// Whether `tools/list` may emit notifications on tool-set changes. Our
    /// registry is static at process start, so this stays `false`.
    #[serde(rename = "listChanged")]
    pub list_changed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ServerInfo {
    pub name: &'static str,
    pub version: &'static str,
}

/// `tools/list` response payload.
#[derive(Debug, Clone, Serialize)]
pub struct ToolsListResult {
    pub tools: Vec<ToolDescriptor>,
}

/// One tool's public descriptor — what the LLM sees and what it must match
/// when it sends `tools/call`.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDescriptor {
    pub name: &'static str,
    pub description: &'static str,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// `tools/call` response payload. We always emit a single text block holding
/// the tool's JSON output — the LLM parses it back out. Structured results
/// (the optional `structuredContent` field in the latest MCP spec) can be
/// added later without breaking clients.
#[derive(Debug, Clone, Serialize)]
pub struct ToolCallResult {
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum ContentBlock {
    Text { text: String },
}

impl ToolCallResult {
    pub fn ok_json(value: &Value) -> Self {
        Self {
            content: vec![ContentBlock::Text {
                text: serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()),
            }],
            is_error: false,
        }
    }
    pub fn err_text(msg: impl Into<String>) -> Self {
        Self {
            content: vec![ContentBlock::Text { text: msg.into() }],
            is_error: true,
        }
    }
}
