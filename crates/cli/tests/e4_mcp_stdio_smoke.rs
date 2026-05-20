//! E4: end-to-end MCP stdio smoke through the real `sdi` binary.
//!
//! Spawns `sdi mcp` as a child process under a temp `$SDI_HOME`, writes a
//! JSON-RPC `initialize` request to its stdin, reads the response from its
//! stdout, then asks for `tools/list` and asserts the full E2 (read) + E3
//! (write) surface is advertised. Proves the wiring: CLI Cmd::Mcp →
//! daemon auto-spawn → sdi_mcp::run_stdio → JSON-RPC pump.

use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

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
            "sdi-cli-e4-{}-{}",
            std::process::id(),
            ulid::Ulid::new()
        ));
        std::fs::create_dir_all(&home).unwrap();
        Harness { home }
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
fn sdi_mcp_advertises_all_nine_tools_over_stdio() {
    let h = Harness::new();

    let mut child = Command::new(sdi_bin())
        .arg("mcp")
        .env("SDI_HOME", &h.home)
        .env("PATH", bin_dir())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sdi mcp");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let mut reader = BufReader::new(stdout);

    // initialize
    let init = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });
    writeln!(stdin, "{}", init).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| {
        panic!("bad initialize response: {e}\n{line}");
    });
    assert_eq!(v["id"], 1);
    assert_eq!(v["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(v["result"]["serverInfo"]["name"], "sdi-mcp");

    // tools/list
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list"
    });
    writeln!(stdin, "{}", req).unwrap();
    stdin.flush().unwrap();
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let v: serde_json::Value = serde_json::from_str(&line).unwrap_or_else(|e| {
        panic!("bad tools/list response: {e}\n{line}");
    });
    let tools = v["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    // E2 reads + E3 writes = 9 tools.
    for expected in [
        "search_knowledge",
        "search_scenarios",
        "get_plan_context",
        "get_recent_decisions",
        "add_scenario",
        "add_requirement",
        "add_decision",
        "update_task_evidence",
        "start_round",
    ] {
        assert!(
            names.contains(&expected),
            "missing tool {expected} in {names:?}"
        );
    }

    // Close stdin so the server loop sees EOF and exits cleanly.
    drop(stdin);
    let status = child.wait().expect("child wait");
    assert!(status.success(), "sdi mcp exited non-zero: {status:?}");
}
