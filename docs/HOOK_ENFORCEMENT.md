# Hook Enforcement

SDI's hook layer is the operational embodiment of PRD §2's design decisions
D5 / D6 / D8 / D10. This document maps each PRD acceptance criterion in §6 to
the file that implements it (or to its planned implementation point).

## Routing (plugin/hooks/hooks.json)

Six Claude Code events are wired. Each routes to a single shim under
`plugin/adapters/claude/` which delegates to one function exported from
`plugin/adapters/shared/sdi-hooks.cjs`. The split is structural:

- **adapters/claude/** — Claude Code coupling lives here. ≤8 LOC. `.catch()` +
  `process.exit(0)`. No business logic. Lint test enforces all three rules.
- **adapters/shared/sdi-hooks.cjs** — Hook bodies, daemon HTTP, install gate,
  audit log. Zero runtime deps (Node 20 built-ins only).

| Event              | Matcher                  | Shim                     | Body                            |
| ---                | ---                      | ---                      | ---                             |
| `SessionStart`     | (none — fires once)      | `session-start.cjs`      | `ensureInstalled` + banner      |
| `UserPromptSubmit` | (none)                   | `user-prompt-submit.cjs` | Active-task context injection   |
| `PreToolUse`       | `Edit\|Write\|MultiEdit\|Bash\|NotebookEdit\|Agent\|Task\|TeamCreate\|SendMessage` | `pre-tool-use.cjs` | Active-task gate |
| `PostToolUse`      | `Edit\|Write`            | `post-tool-use.cjs`      | File-change → Task audit        |
| `SubagentStart`    | (none)                   | `subagent-start.cjs`     | Sub-agent → Task bind           |
| `SubagentStop`     | (none)                   | `subagent-stop.cjs`      | Sub-agent result → Task evidence|

## Trust boundary

The hook layer is **not** an arbitrary RPC surface. It enforces two trust
invariants:

1. **Active-task gate** (PreToolUse). No file-mutation tool (Edit / Write /
   MultiEdit / NotebookEdit / Bash) and no agent-spawning tool (Agent / Task /
   TeamCreate / SendMessage) proceeds without an active Task. The shim emits a
   `permissionDecision: deny` payload with a self-recovery hint that names
   `/scenario new` and `/round start` so the LLM can self-route. The escape
   hatch for environments that genuinely cannot register a task (test
   fixtures, emergencies) is `sdi bypass arm --reason "<short reason>"`
   (XDG-cache marker, one-shot, default TTL 60s, audit-logged); production
   sessions must not arm it routinely. The startup-time `SDI_BYPASS_HOOKS=1`
   env switch remains for shell-rc exports but does not catch inline
   `VAR=1 cmd` prefixes.

2. **LM-8 path invariant** (every event, structurally). Plugin code writes only
   under `pluginRoot` (version marker) and via `appendHookLog()` under XDG
   state. Daemon paths (`~/.cache/sdi/`, `~/.local/share/sdi/`) are owned by
   the daemon; the plugin only **reads** the port file. See
   [ARCHITECTURE.md](./ARCHITECTURE.md#data-location-lm-8-invariant).

## PRD §6 acceptance criteria — enforcement map

| # | Criterion                          | Enforcement point                     | Status        |
| - | ---                                | ---                                   | ---           |
| 1 | GWT 강제 (`/scenario add`)         | `crates/daemon::router::scenario::create` rejects empty `then` with `GWT_EMPTY`; CLI `sdi scenario create` mirrors the validation. Test: `http_scenario_round_task::scenario_gwt_strict_d5`. | implemented |
| 2 | Plan approve gate ≥1 scenario      | `crates/daemon::router::plan::approve` returns `SCENARIOS_REQUIRED` until ≥1 confirmed scenario exists. Test: `http_scenario_round_task::plan_approve_unlocks_after_scenario_confirmed`. | implemented |
| 3 | R2+ auto-regression carry-over     | `crates/daemon::router::round::activate` calls `repo::carry_over_results` when `RoundMode::StrictRegression`. Unevaluated scenarios from prior round are excluded (inner join, not `COALESCE`). Tests: `r2_auto_regression_carries_results`, `carry_over_excludes_unevaluated_scenarios_prd_6_3`. | implemented |
| 4 | Disruption needs-review            | `crates/daemon::router::round::activate` calls `disruption_repo::has_pending` and returns `DISRUPTION_PENDING` when reviews are open. `/disruption-reviews/:id/resolve` clears them. Tests: `http_disruption_review::*` (3 cases). | implemented |
| 5 | In-flight Task pause default       | `crates/daemon::router::round::activate` honors `in_flight_policy` (Pause → Blocked / Abort → Cancelled / ContinueOnNoimpact → no-op). Default is Pause. Tests: `round_activate_pauses_in_flight_tasks_prd_6_5`, `round_activate_abort_cancels_in_flight_tasks_prd_6_5`, `round_activate_continue_on_noimpact_leaves_tasks_prd_6_5`. | implemented |
| 6 | Structured evidence (Task done)    | `crates/daemon::router::task::complete` requires `TaskEvidence::scenarios[]` (each with `scenario_id` + `result` + non-empty `evidence_ref`) and mirrors every entry into `round.scenario_results` via `round_repo::upsert_result`. `/tasks/:id/status status=done` is fail-closed (returns `EVIDENCE_REQUIRED`). Tests: `task_done_requires_evidence_prd_6_6`, `task_complete_mirrors_evidence_into_round_results_d6`. | implemented |
| 7 | SNAPSHOT-ONLY Requirement          | `crates/daemon::router::requirement::{create,update}` writes a single row keyed by `(plan_id, short_code)` with `ON CONFLICT … DO UPDATE`; no version history is retained. POST validates body against history-leak phrases (`기존 안:`, `~~`, `이전엔`, etc.). The change trail lives in append-only `decisions`. Tests: `http_project_plan_req::requirement_snapshot_overwrite`, `requirement_rejects_history_traces`. | implemented |
| 8 | LM-8 path invariant runtime guard  | `crates/db::paths::ensure_no_plugin_overlap` checks data/cache/config/state/db_file against `~/.claude/plugins/` overlap at every `Paths::resolve`. `sdi doctor` mirrors the check at exit 1. `CLAWKET_ALLOW_PLUGIN_OVERLAP=1` is the only bypass. | implemented |
| 9 | MCP read tools force scope=rag     | `crates/mcp` read tool handlers (`search_knowledge`, `find_similar_tasks`, `get_recent_decisions`, `get_plan_context`) filter by `scope='rag'` in SQL. Test: `read_tools::search_knowledge_never_leaks_reference_scope`. | implemented |
| 10| `/goal` orthogonality              | `plugin/commands/` ships no `goal.md`; `/goal` is reserved as a Claude Code stop-hook contract, never an SDI workflow surface. `plugin/commands/` currently exposes `decide / plan / req / round / scenario / sdi-status` only. | implemented |

All ten criteria are now backed by code + a green integration test. Future
divergences are guarded by `cargo test --workspace` in CI.

## Failure semantics

- **Shim crash** → `.catch()` swallows, `process.exit(0)` returns control to
  Claude Code. The user sees nothing; the audit log records the crash.
- **Daemon down on PreToolUse** → install gate attempts a spawn. If the spawn
  fails, the shim still exits 0 (degrade rather than block).
- **Install gate failure** → reported on `stderr` so the SessionStart banner is
  visible; subsequent hooks proceed but the active-task gate may have no daemon
  to query. The audit log captures the failure mode for `sdi doctor`.

## Audit log

`appendHookLog(event, payload)` writes JSON-lines to
`~/.local/state/sdi/hook.log`. This is the **only** plugin write channel under
XDG paths — every other XDG write goes through the daemon. The single channel
makes LM-8 violations grep-able: any `fs.writeFile` / `fs.appendFile` in
`adapters/` outside `appendHookLog` is a structural bug.
