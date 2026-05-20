//! Stdio JSON-RPC dispatch loop.
//!
//! Reads newline-delimited JSON from stdin, dispatches `initialize`,
//! `tools/list`, `tools/call`, and writes one JSON response per request to
//! stdout. Notifications (`initialized`) are accepted and ignored. Any
//! parse failure produces an error response per JSON-RPC 2.0 §5.1.

use crate::protocol::{
    codes, Capabilities, InitializeResult, Request, Response, ServerInfo, ToolCallResult,
    ToolsCapability, ToolsListResult, PROTOCOL_VERSION,
};
use crate::tools::{DaemonClient, Registry};
use anyhow::Result;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Resolve the daemon base URL from `<cache>/sdid.port`. Returns
/// `http://127.0.0.1:<port>`. Errors if the daemon hasn't been started yet
/// — the CLI's `mcp` arm starts the daemon before invoking us.
pub fn daemon_base_from_paths() -> Result<String> {
    let paths = sdi_db::Paths::resolve()?;
    let port_file = paths.cache.join("sdid.port");
    let port = std::fs::read_to_string(&port_file)
        .map_err(|e| anyhow::anyhow!("read {}: {e}", port_file.display()))?;
    let port: u16 = port.trim().parse()?;
    Ok(format!("http://127.0.0.1:{}", port))
}

/// Build the full registry — read tools + write tools. Wired up in E2/E3.
pub fn build_registry(client: Arc<DaemonClient>) -> Registry {
    let mut reg = Registry::new();
    for t in crate::tools::read::all(client.clone()) {
        reg = reg.with(t);
    }
    for t in crate::tools::write::all(client.clone()) {
        reg = reg.with(t);
    }
    reg
}

/// Run the server against arbitrary async I/O. Production wiring is
/// `run_stdio` (below); tests reuse this with in-memory pipes.
pub async fn run<R, W>(reader: R, mut writer: W, registry: Registry) -> Result<()>
where
    R: tokio::io::AsyncRead + Unpin,
    W: tokio::io::AsyncWrite + Unpin,
{
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines.next_line().await? {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let response = handle_line(trimmed, &registry).await;
        if let Some(resp) = response {
            let mut buf = serde_json::to_vec(&resp)?;
            buf.push(b'\n');
            writer.write_all(&buf).await?;
            writer.flush().await?;
        }
    }
    Ok(())
}

/// Handle a single JSON-RPC line. Returns `None` for notifications (no
/// response per JSON-RPC 2.0 §4.1).
async fn handle_line(line: &str, registry: &Registry) -> Option<Response> {
    let req: Request = match serde_json::from_str(line) {
        Ok(r) => r,
        Err(e) => {
            return Some(Response::err(
                Value::Null,
                codes::PARSE_ERROR,
                format!("parse error: {e}"),
            ));
        }
    };
    // Notifications carry no id; we silently consume them.
    let id = match req.id.clone() {
        Some(id) => id,
        None => return None,
    };
    Some(dispatch(req.method.as_str(), req.params, id, registry).await)
}

async fn dispatch(method: &str, params: Value, id: Value, registry: &Registry) -> Response {
    match method {
        "initialize" => Response::ok(
            id,
            serde_json::to_value(InitializeResult {
                protocol_version: PROTOCOL_VERSION,
                capabilities: Capabilities {
                    tools: ToolsCapability { list_changed: false },
                },
                server_info: ServerInfo {
                    name: "sdi-mcp",
                    version: env!("CARGO_PKG_VERSION"),
                },
            })
            .unwrap(),
        ),
        "tools/list" => Response::ok(
            id,
            serde_json::to_value(ToolsListResult {
                tools: registry.descriptors(),
            })
            .unwrap(),
        ),
        "tools/call" => {
            let name = match params.get("name").and_then(|v| v.as_str()) {
                Some(n) => n,
                None => {
                    return Response::err(
                        id,
                        codes::INVALID_PARAMS,
                        "tools/call requires `name`",
                    );
                }
            };
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            let tool = match registry.find(name) {
                Some(t) => t,
                None => {
                    return Response::err_with_data(
                        id,
                        codes::METHOD_NOT_FOUND,
                        format!("unknown tool: {name}"),
                        json!({ "tool": name }),
                    );
                }
            };
            let result: ToolCallResult = tool.call(args).await;
            Response::ok(id, serde_json::to_value(result).unwrap())
        }
        // `initialized` is a notification — should never reach here because
        // notifications drop in `handle_line`. Defensive arm for the case
        // where a buggy client sends it with an id.
        "initialized" | "notifications/initialized" => {
            Response::ok(id, json!({}))
        }
        other => Response::err(
            id,
            codes::METHOD_NOT_FOUND,
            format!("method not found: {other}"),
        ),
    }
}

/// Entry point invoked by `sdi mcp`. Resolves the daemon base, builds the
/// registry, and pumps stdin → dispatch → stdout until EOF.
pub async fn run_stdio() -> Result<()> {
    let base = daemon_base_from_paths()?;
    let client = Arc::new(DaemonClient::new(base));
    let registry = build_registry(client);
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();
    run(stdin, stdout, registry).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::duplex;

    /// Smoke test: drive `initialize` against an empty registry over an
    /// in-memory pipe. Verifies the framing and the response envelope.
    #[tokio::test]
    async fn initialize_round_trip_over_in_memory_pipe() {
        let (mut client_w, server_r) = duplex(8192);
        let (server_w, mut client_r) = duplex(8192);
        let req = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {}
        });
        let mut bytes = serde_json::to_vec(&req).unwrap();
        bytes.push(b'\n');
        client_w.write_all(&bytes).await.unwrap();
        drop(client_w); // EOF so server loop exits

        let registry = Registry::new();
        run(server_r, server_w, registry).await.unwrap();

        let mut out = String::new();
        use tokio::io::AsyncReadExt;
        client_r.read_to_string(&mut out).await.unwrap();
        let line = out.lines().next().expect("at least one response line");
        let v: Value = serde_json::from_str(line).unwrap();
        assert_eq!(v["jsonrpc"], "2.0");
        assert_eq!(v["id"], 1);
        assert_eq!(v["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(v["result"]["serverInfo"]["name"], "sdi-mcp");
    }

    /// `tools/list` against an empty registry returns an empty array — not
    /// null, not absent. The MCP spec is strict about that.
    #[tokio::test]
    async fn tools_list_returns_empty_array_when_registry_empty() {
        let (mut client_w, server_r) = duplex(8192);
        let (server_w, mut client_r) = duplex(8192);
        let req = json!({ "jsonrpc": "2.0", "id": 7, "method": "tools/list" });
        let mut bytes = serde_json::to_vec(&req).unwrap();
        bytes.push(b'\n');
        client_w.write_all(&bytes).await.unwrap();
        drop(client_w);

        run(server_r, server_w, Registry::new()).await.unwrap();

        let mut out = String::new();
        use tokio::io::AsyncReadExt;
        client_r.read_to_string(&mut out).await.unwrap();
        let v: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(v["result"]["tools"], json!([]));
    }

    /// Unknown method must produce a -32601 error with the offending name.
    #[tokio::test]
    async fn unknown_method_returns_method_not_found() {
        let (mut client_w, server_r) = duplex(8192);
        let (server_w, mut client_r) = duplex(8192);
        let req = json!({ "jsonrpc": "2.0", "id": 9, "method": "does/not/exist" });
        let mut bytes = serde_json::to_vec(&req).unwrap();
        bytes.push(b'\n');
        client_w.write_all(&bytes).await.unwrap();
        drop(client_w);

        run(server_r, server_w, Registry::new()).await.unwrap();

        let mut out = String::new();
        use tokio::io::AsyncReadExt;
        client_r.read_to_string(&mut out).await.unwrap();
        let v: Value = serde_json::from_str(out.lines().next().unwrap()).unwrap();
        assert_eq!(v["error"]["code"], codes::METHOD_NOT_FOUND);
    }

    /// Notifications (no `id`) MUST NOT produce a response.
    #[tokio::test]
    async fn notification_produces_no_response() {
        let (mut client_w, server_r) = duplex(8192);
        let (server_w, mut client_r) = duplex(8192);
        let n1 = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        let r1 = json!({ "jsonrpc": "2.0", "id": 2, "method": "tools/list" });
        for v in [n1, r1] {
            let mut bytes = serde_json::to_vec(&v).unwrap();
            bytes.push(b'\n');
            client_w.write_all(&bytes).await.unwrap();
        }
        drop(client_w);

        run(server_r, server_w, Registry::new()).await.unwrap();

        let mut out = String::new();
        use tokio::io::AsyncReadExt;
        client_r.read_to_string(&mut out).await.unwrap();
        let lines: Vec<&str> = out.lines().collect();
        assert_eq!(lines.len(), 1, "exactly one response (for tools/list)");
        let v: Value = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(v["id"], 2);
    }
}
