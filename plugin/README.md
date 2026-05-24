# SDI — Claude Code plugin shell

This directory is the **Claude Code plugin surface** for SDI (Scenario-Driven
Implementation). It is part of the same repository as the SDI body
(`crates/` workspace: `cli` + `daemon` + `mcp` + `core` + `db`). The plugin
is not a separate package — it is one of the faces of this repository.

Canonical spec: [`../docs/PRD.md`](../docs/PRD.md) (decisions D1–D29).

## Contents

| Path | Role |
|---|---|
| `.claude-plugin/plugin.json` | Plugin manifest. Declares `commands/`, `agents/`, `skills/`, and marketplace metadata. |
| `.mcp.json` | MCP server registration. Spawns `sdi mcp` (the CLI's stdio MCP subcommand). |
| `hooks/hooks.json` | Hook routing for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`, `SubagentStop`. |
| `adapters/claude/*.cjs` | Thin Claude-Code-specific wrappers that delegate to `shared/sdi-hooks.cjs`. |
| `adapters/shared/sdi-hooks.cjs` | Single source of install logic + hook bodies (idempotent `ensureInstalled`, daemon spawn, active-task / delegation / pattern / claim guards). |
| `commands/*.md` | Slash commands (D11 + v0.5): `/plan`, `/req`, `/scenario`, `/round`, `/decide`, `/consensus`, `/autonomy`, `/agent-note`, `/pattern`, `/sdi-status`. |
| `agents/*.md` | Specialist sub-agent definitions (see below). |
| `skills/{sdi-overview,sdi-scenario,sdi-round,sdi-evidence}/SKILL.md` | Four task-scoped skills covering orientation, GWT conversion, round lifecycle, and evidence recording. |
| `scripts/setup.cjs` | Manual / CI entry into `ensureInstalled` (same code path as `SessionStart`). |
| `bin/`, `daemon/bin/` | Install targets for the bundled `sdi` and `sdid` binaries (populated by `ensureInstalled` when the release-bundle layout is in use). |

## Specialist agents

The plugin ships 11 specialist agents under `agents/`. Multi-agent collaboration
is the body of SDI (D13), so the orchestrator never executes — it delegates.

| Agent | Role |
|---|---|
| `scenario-decomposer` | Decomposes a plan's intent into GWT scenarios. |
| `gwt-converter` | Converts free-form asks into strict Given / When / Then form (D5). |
| `impl-coder` | Implements a scenario; runs under one of the four collaboration patterns. |
| `test-runner` | Executes verification and emits evidence. |
| `regression-runner` | Replays prior passing scenarios in R2+ rounds (D7). |
| `disruption-analyst` | Classifies disruption when a scenario / requirement / decision changes (D9). |
| `schema-architect` | Owns schema-shaped decisions (forced L4 per D17). |
| `decision-resolver` | Drives consensus / dissensus resolution on Decision rows. |
| `pattern-orchestrator` *(v0.5)* | Selects and activates a CollaborationPattern; enforces shape gates (D26 / D27). |
| `pattern-critic` *(v0.5)* | Provides the second distinct `(AgentSpec.name, AgentSpec.stance)` tuple required by D26 graph-consensus sybil-fix. |
| `reversal-runner` *(v0.5)* | Executes rollback as append-only Decision (`kind=consensus, reversal_of=<id>`) per D28. |

## Hooks and gates

The hook chain layered on top of Claude Code:

| Hook | Behavior |
|---|---|
| `SessionStart` | Invokes `ensureInstalled` (idempotent), spawns `sdid`, injects dashboard context. |
| `UserPromptSubmit` | Resolves and injects active scenario context. |
| `PreToolUse` | Four gates in order: active-task / **delegation (D21)** / **pattern shape (D26 advisory)** / **resource claim (D29)**. Matches `Edit`, `Write`, `MultiEdit`, `Bash`, `NotebookEdit`, `Agent`, `Task`, `TeamCreate`, `SendMessage`. |
| `PostToolUse` | Records file changes on the active scenario / task; matches `Edit`, `Write`, `MultiEdit`, `NotebookEdit`. |
| `SubagentStart` / `SubagentStop` | Binds the sub-agent run to the active scenario; on stop, persists the result summary. |

D21 delegation gate: the orchestrator (main session) is blocked from calling
execution tools (`Edit` / `Write` / `MultiEdit` / `NotebookEdit` / mutating
`Bash`). Only `Agent`-spawned specialists carry the `hookInput.agent_id` that
satisfies the gate.

D26 pattern integrity (advisory): when an `Agent` or `Task` dispatch carries
multi-agent intent tokens (`specialist team`, `parallel`, `swarm`, `graph
review`, `fan-out`, `agents-as-tools`, `multi-agent`) or a `pattern_id`, the
hook queries `/patterns/active`. Missing rows trigger a non-blocking warning;
the daemon auto-creates a `direct` row that caps autonomy at L3.

D29 multi-session claims: for `Edit` / `Write` / `NotebookEdit`, the hook
queries `/scenarios/active-claims`. Cross-session overlap exits with code 2
and a structured `{ block: 'sdi_claim_overlap', target_path, my_scenario,
holders, hint }` payload. Daemon unreachable → proceed (so an offline daemon
never locks the editor).

Emergency bypass: `SDI_HOOK_V05_DISABLE=1` is a single-invocation escape that
is audit-logged on every use. Routine use is a protocol violation.

The active scenario currently flows through the `SDI_ACTIVE_SCENARIO` env
var until the daemon gains the `AgentRun ↔ Scenario` edge.

## Install gate

`adapters/shared/sdi-hooks.cjs::ensureInstalled` is the **single** install
entry. `SessionStart` invokes it; `scripts/setup.cjs` delegates to it for
manual / CI flows. The gate is idempotent: when both binaries resolve, the
skill files check out, and the daemon's `/health` responds, it returns
immediately. SDI ships as one workspace, so cli/daemon versioning is governed
by `Cargo.toml [workspace.package].version` — there is no per-component
manifest.

## LM-8 invariant

User data resolves under XDG paths (`~/.local/share/sdi/`, `~/.cache/sdi/`,
`~/.config/sdi/`, `~/.local/state/sdi/`) — **never** under
`~/.claude/plugins/`. The daemon enforces this at startup; `sdi doctor`
reports overlap as a fatal error. The plugin gate only writes binaries /
bundles under `pluginRoot`, never user state.

## Sibling repositories

- [`sdi-web`](https://github.com/scenario-driven/sdi-web) — Vite/React dashboard SPA over `sdid` HTTP + SSE.
- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — Tauri 2 shell that bundles `sdi-web` and spawns `sdid`.
- [`sdi-docs`](https://github.com/scenario-driven/sdi-docs) — Landing + bilingual guide site.

For the full identity statement and D1–D29 invariants, see the repository root
[`README.md`](../README.md) and [`CLAUDE.md`](../CLAUDE.md).
