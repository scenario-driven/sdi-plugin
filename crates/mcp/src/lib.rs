//! SDI MCP server (stdio JSON-RPC 2.0).
//!
//! Exposed to LLM clients via `sdi mcp`. The transport is newline-delimited
//! JSON on stdin/stdout (MCP stdio variant — no `Content-Length` framing;
//! that header belongs to the HTTP variant).
//!
//! Tool surface per PRD §5.4:
//! - **read**  : `search_knowledge`, `search_scenarios`, `get_plan_context`,
//!   `get_recent_decisions` — RAG-only; results MUST be `scope=rag` so the
//!   LLM never sees `reference`/`archive` artifacts (LM invariant carried
//!   from Clawket).
//! - **write** : `add_scenario`, `add_requirement`, `add_decision`,
//!   `update_task_evidence`, `start_round` — mediated mutations that map
//!   straight onto the daemon's HTTP routes.
//!
//! The crate is a library with a single entry point — `run_stdio` — that the
//! `sdi mcp` subcommand calls. Keeping the entry as a function (not a `main`)
//! lets the integration tests drive it without spawning a subprocess.

pub mod protocol;
pub mod server;
pub mod tools;

pub use server::run_stdio;
