# SDI — Claude Code / Codex plugin shell

**English** · [한국어](./README.ko.md)

This directory is the **Claude Code and Codex plugin surface** for SDI (Scenario-Driven
Implementation). It is part of the same repository as the SDI body
(`crates/` workspace: `cli` + `daemon` + `mcp` + `core` + `db`). The plugin
is not a separate package — it is one of the faces of this repository.

Canonical spec: [`../docs/PRD.md`](../docs/PRD.md) (decisions D1–D29).

## Contents

| Path | Role |
|---|---|
| `.claude-plugin/plugin.json` | Claude Code plugin manifest. Declares `commands/`, `agents/`, `skills/`, and marketplace metadata. |
| `.codex-plugin/plugin.json` | Codex plugin manifest. Points at shared `skills/` and inline MCP config for the shared launcher. |
| `.mcp.json` | Claude MCP server registration. Spawns the shared MCP launcher. |
| `hooks/hooks.json` | Hook routing for `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`, `SubagentStop`. Codex loads this default hook file too. |
| `adapters/claude/*.cjs` | Thin host wrappers that delegate to `shared/sdi-hooks.cjs`. Codex receives compatibility env vars for these plugin hooks. |
| `adapters/shared/sdi-hooks.cjs` | Single source of install logic + hook bodies (idempotent `ensureInstalled`, daemon spawn, active-task / delegation / pattern / claim guards). |
| `commands/*.md` | Slash commands (D11 + v0.5): `/plan`, `/req`, `/scenario`, `/round`, `/decide`, `/consensus`, `/autonomy`, `/agent-note`, `/pattern`, `/sdi-status`. |
| `agents/*.md` | Specialist sub-agent definitions (see below). |
| `skills/{sdi-overview,sdi-scenario,sdi-round,sdi-evidence}/SKILL.md` | Four task-scoped skills covering orientation, GWT conversion, round lifecycle, and evidence recording. |
| `scripts/setup.cjs` | Manual / CI entry into `ensureInstalled` (same code path as `SessionStart`). |
| `scripts/sdi-mcp.cjs` | Host-neutral MCP launcher. Resolves `sdi` via the shared install-gate policy, then execs `sdi mcp`. |
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

D26 pattern integrity (advisory): two complementary triggers steer the session
toward a real CollaborationPattern before work fans out under the L3-capped
`direct` marker.

- **Dispatch trigger** — when an `Agent` or `Task` dispatch carries multi-agent
  intent tokens (`specialist team`, `parallel`, `swarm`, `graph review`,
  `fan-out`, `agents-as-tools`, `multi-agent`) or a `pattern_id`, the hook
  queries `/patterns/active` and warns when no row exists.
- **Decompose trigger (D13)** — on the structural seam of round decompose: `sdi
  round activate <R>` (main session) and the first `sdi task create <R>` of a
  round (decomposer sub-agent). When no non-`direct` active pattern governs the
  round's plan and the create carries no `--produced-via-pattern`, the hook
  nudges the session to run the pattern-orchestrator first. This is what catches
  ordinary decompose — the dispatch trigger alone fires only once an intent
  token is already present.

Both are non-blocking; the daemon back-fills a `direct` row (L3 cap) whenever a
work entity is produced without a pattern. Bind a chosen pattern at creation
with `sdi task create … --produced-via-pattern <PAT-ID>`; the daemon validates
the reference is `active` and scope-compatible, or rejects it (no silent
`direct` degrade).

D29 multi-session claims: for `Edit` / `Write` / `NotebookEdit`, the hook
queries `/scenarios/active-claims`. Cross-session overlap exits with code 2
and a structured `{ block: 'sdi_claim_overlap', target_path, my_scenario,
holders, hint }` payload. Daemon unreachable → proceed (so an offline daemon
never locks the editor).

Emergency bypass: `sdi bypass arm --reason "<short reason>"` writes a marker
at `~/.cache/sdi/bypass-once` (XDG cache) that unlocks every mutating gate
(D21 / active-task / D29) for the next single tool invocation, then
auto-consumes. The TTL defaults to 60s (`--ttl <seconds>` to override).
`sdi` is on the D21 read-only Bash whitelist so the main session can arm
the marker directly; specialists are unnecessary for the bypass path
itself. `sdi bypass status` inspects state and TTL remainder; `sdi bypass
disarm` clears the marker. Every arm + consumption is recorded in the hook
audit log; routine use is a protocol violation.

Startup-time fallbacks (only effective when exported from the shell that
launches Claude Code): `SDI_DELEGATION_BYPASS=1` mirrors the marker for
D21 alone, `SDI_BYPASS_HOOKS=1` short-circuits the entire `PreToolUse`
chain, `SDI_HOOK_V05_DISABLE=1` disables the D26 advisory + D29 claim
block. These do not catch inline `VAR=1 cmd` prefixes (Claude Code spawns
the hook before the shell expands them).

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

## Related surfaces

- [`web/`](./web/) — the dashboard SPA (Vite/React 19/Tailwind 4) lives in this same repository; `sdid` serves its `dist/` over `/` and feeds it via the HTTP API + `/events` SSE.
- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — separate org repo. Tauri 2 shell that bundles `plugin/web/dist` and spawns `sdid` as a sidecar.
- [`sdi-docs`](https://github.com/scenario-driven/sdi-docs) — separate org repo. Astro/Starlight landing + bilingual (ko / en) guide site mirroring the repo's `docs/PRD.md`.

For the full identity statement and D1–D29 invariants, see the repository root
[`README.md`](../README.md) and [`CLAUDE.md`](../CLAUDE.md).
