# sdi-daemon

**English** · [한국어](./README.ko.md)

The `sdid` background daemon — the only process that touches SQLite. Part of the `@scenario-driven/sdi-plugin` Rust workspace.

## What it is

`sdi-daemon` builds one binary, `sdid`, plus a `sdi_daemon` library. It runs an `axum` HTTP API and an SSE event bus on a single `tokio` runtime, holding all state through [`sdi-db`](../db/). The CLI, MCP server, and dashboard SPA are all clients of this surface — none of them open the database directly.

## Surface

| Module | Role |
|---|---|
| `state` | Shared `AppState` — db pool + event broadcaster + resolved paths. |
| `router` | axum router assembly, one submodule per entity (plan / scenario / decision / round / pattern / autonomy / agent_note / task / project / aggregate / …). |
| `events` | tokio broadcast channel + `/events` SSE handler. |
| `error` | `DomainError` → JSON HTTP response mapping. |
| `lifecycle` | pid / port / socket file management. |

## Place in the workspace

```
sdi-cli / sdi-mcp / dashboard SPA ──HTTP + SSE──▶ sdi-daemon (this) ──▶ sdi-db ──▶ SQLite
```

Because the daemon is the sole writer, it is where the autonomy gates, consensus rules, pattern shape validation, and multi-session claim routing are enforced at runtime. It also serves `plugin/web/dist/` over tower-http `ServeDir`.

## Build & verify

```sh
cargo build -p sdi-daemon   # produces target/debug/sdid
cargo check -p sdi-daemon
```

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md). Repository overview: [`../../README.md`](../../README.md).
