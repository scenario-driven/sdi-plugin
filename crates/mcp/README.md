# sdi-mcp

**English** · [한국어](./README.ko.md)

The SDI MCP server — a stdio JSON-RPC 2.0 surface for LLM clients. Part of the `@scenario-driven/sdi-plugin` Rust workspace.

## What it is

`sdi-mcp` is a library, not a binary. Its single entry point `run_stdio` is invoked by the `sdi mcp` subcommand (keeping it a function, not a `main`, lets integration tests drive it without a subprocess). The transport is newline-delimited JSON on stdin/stdout (the MCP stdio variant — no `Content-Length` framing). The plugin's `.mcp.json` wires `sdi mcp` as its MCP server.

## Tool surface (PRD §5.4)

| Kind | Tools |
|---|---|
| **read** | `search_knowledge`, `search_scenarios`, `get_plan_context`, `get_recent_decisions` — RAG-only. Results MUST be `scope=rag`; the LLM never sees `reference` / `archive` artifacts (LM invariant carried from Clawket). |
| **write** | `add_scenario`, `add_requirement`, `add_decision`, `update_task_evidence`, `start_round` — mediated mutations that map straight onto the daemon's HTTP routes. |

## Place in the workspace

```
LLM client ──stdio JSON-RPC──▶ sdi mcp (sdi-mcp, this) ──HTTP──▶ sdi-daemon ──▶ sdi-db
```

Writes never bypass the daemon — `sdi-mcp` calls the same HTTP routes the CLI uses (`reqwest`), so the daemon's gates apply uniformly.

## Build & verify

```sh
cargo build -p sdi-mcp
cargo check -p sdi-mcp
```

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md). Repository overview: [`../../README.md`](../../README.md).
