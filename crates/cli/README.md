# sdi-cli

**English** · [한국어](./README.ko.md)

The `sdi` command-line binary — the entry point that users and LLM agents drive. Part of the `@scenario-driven/sdi-plugin` Rust workspace.

## What it is

`sdi-cli` builds one binary, `sdi`, plus a thin `sdi_cli` library so integration tests can drive the clap app, daemon-lifecycle helpers, doctor checks, and HTTP client without spawning a subprocess.

The CLI never touches SQLite directly. It speaks to the `sdid` daemon over its HTTP surface (`reqwest`) and renders the result. The daemon owns all state.

## Subcommands

| Group | Commands |
|---|---|
| First-class entities | `plan`, `req`, `scenario`, `round`, `decide`, `consensus`, `autonomy`, `agent-note`, `pattern` |
| Runtime | `task`, `run`, `project` |
| Aggregates & ops | `aggregate` (dashboard / summary / board / wiki / timeline), `usage`, `knowledge`, `comment`, `question`, `impexp`, `ops`, `doctor` |
| MCP | `sdi mcp` — hosts the stdio MCP server (delegates to [`sdi-mcp`](../mcp/)). The plugin's `.mcp.json` invokes exactly this. |

## Place in the workspace

```
sdi-cli (this) ──HTTP──▶ sdi-daemon (sdid) ──▶ sdi-db (SQLite)
    └── embeds sdi-mcp (stdio MCP server, `sdi mcp`)
```

Depends on `sdi-core` (domain types), `sdi-db` (for shared types), and `sdi-mcp` (MCP subcommand).

## Build & verify

```sh
cargo build -p sdi-cli      # produces target/debug/sdi
cargo check -p sdi-cli
```

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md). Repository overview: [`../../README.md`](../../README.md).
