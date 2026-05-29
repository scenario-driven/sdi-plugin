# CLAUDE.md — sdi-plugin

Single-source AI context for **this repository** (`@scenario-driven/sdi-plugin`). This file is self-contained: agents working in a fresh clone of just this repo must be able to operate from this document alone (per the wrapper-level operational rule about sub-repo self-containment).

For wrapper-level coordinates (sibling repos, monorepo position), see `../CLAUDE.md` (the `scenario-driven/` wrapper). Wrapper file is **not** required for working in this repo — only useful if working across sibling repos.

---

## Identity (do not paraphrase, do not soften)

This repository is a **Claude Code plugin whose body is a Rust workspace**. The plugin is not a thin wrapper around a separate Rust project — they are the same artifact, two views of the same source tree.

- The plugin shell lives at `plugin/`.
- The Rust workspace lives at `crates/` with five crates: `cli`, `daemon`, `mcp`, `core`, `db`.
- The `sdi` binary (built from `crates/cli`) hosts the `mcp` subcommand (stdio MCP server). The plugin's `.mcp.json` invokes `sdi mcp`.
- The `sdid` binary (built from `crates/daemon`) is the long-lived daemon holding SQLite state and serving HTTP + unix socket.

Tool identity (one paragraph): **Scenario-Driven Implementation (SDI)**. Natural-language Given/When/Then scenarios are first-class citizens. Plans approve when scenarios are complete; tasks are runtime artifacts the LLM decomposes from scenarios + requirements; rounds (R1, R2, …) auto-replay prior scenarios as regression. Lineage: TDD → BDD → SDI.

Full design spec: [`docs/PRD.md`](./docs/PRD.md). Reading that PRD is mandatory before non-trivial changes.

---

## Decisions in force (D1–D29)

| # | Decision | Where enforced |
|---|---|---|
| D1 | Tool identity = Scenario-Driven Implementation engine | README, this file |
| D2 | Seven first-class entities: Plan / Requirement (snapshot) / Decision (append-only) / Scenario (GWT) / Round / AutonomyPolicy / CollaborationPattern (D22) | `crates/core/` |
| D3 | Task is a runtime artifact; LLM decomposes, humans do not author tasks directly | daemon API surface |
| D4 | Unit removed (→ scenario tag). Cycle renamed Round with redefined semantics | `crates/core/`, daemon API |
| D5 | GWT format strict: every scenario must have non-empty Given / When / Then. No free-form option | scenario CRUD validation |
| D6 | Round mode default = `strict-regression`. Option: `forward-only` (explicit) | round creation API |
| D7 | New-development mode and regression-verification mode share one engine. R1 = new, R2+ = regression | round implementation |
| D8 | Plan approve gate = scenarios ≥ 1 & all GWT valid; tasks count is irrelevant | plan approve API |
| D9 | Disruption policy default = needs-review (human confirm). `auto` option still requires confirm before applying | scenario/req/decision write paths |
| D10 | In-flight Task on `round start` defaults to pause. Flags: `--abort`, `--continue-on-noimpact` | round start API |
| D11 | Slash commands: `/scenario`, `/round`, `/plan`, `/req`, `/decide`, `/pattern` (D22). `/goal` is Claude Code built-in, orthogonal — do not intercept | plugin shell |
| D12 | SNAPSHOT-ONLY documents (no in-body history). Decision artifact is the only history surface | documentation policy |
| D13 | Multi-agent orchestration is the body. Single-`@main` solo flow is anti-pattern; every new flow considers multi-agent collaboration as first-class | PRD §2, §4, §5 |
| D14 | AutonomyPolicy is the sixth first-class entity. Per-scope (plan / decision-kind / pattern-kind) autonomy mode persisted, gates Decision application | `crates/core/`, PRD §3.7 |
| D15 | Four multi-agent patterns built in (Workflow / Graph / Swarm / Agents-as-Tools). External A2A protocol excluded from v1 | PRD §4 matrix, §5 |
| D16 | Default = act with policy, not default = ask. User toggles intervention windows; L4 ↔ L5 switchable mid-flow | PRD §5 Layer 0, AutonomyPolicy entity |
| D17 | Mode default: new plan = L5; plan with external surface = L4; decision-kind ∈ {architecture, schema, naming-canonical} forced L4 | PRD §2 D17, AutonomyPolicy validation |
| D18 | Circuit breaker always on. Single-action UI demotes all autonomy modes to L3 instantly; in-flight decisions apply at next gate | UI surface + daemon, PRD §2 D18 |
| D19 | Communication substrate (M1~M5) runs mode-independent. Autonomy mode controls user gate position of consensus only, never blocks agent communication | PRD §5 Layer 2.5 |
| D20 | Consensus / dissensus is the unit of autonomy gate. Single-agent = L3 max; multi-agent consensus unlocks L4 / L5; dissensus always escalates | PRD §3 Decision.kind, daemon gate logic |
| D21 | Mandatory Delegation Gate — orchestrator (main session) cannot call execution tools (`Edit`/`Write`/`NotebookEdit`/mutating `Bash`); PreToolUse hook blocks them unless `hookInput.agent_id` indicates an Agent-spawned specialist sub-agent. D13's mechanical enforcement face | `plugin/hooks/`, PRD §2 D21, §5 Layer 1.5, §6 #15 |
| D22 | CollaborationPattern is the seventh first-class entity. Kind ∈ {workflow, graph, swarm, agents-as-tools, direct}, applies_to ∈ {plan, requirement, scenario, task, decision, round}, lifecycle pending → active → converged \| dissensus \| aborted. D15's entity representation | `crates/core/` (v0.5), PRD §3.9 |
| D23 | Pattern provenance NOT NULL — every new work entity (plan/requirement/scenario/task/decision/round) carries `produced_via_pattern_id`. `direct` is the explicit solo-flow marker (anti-pattern badge), not an escape | DB migration, daemon write paths, PRD §6 #16 |
| D24 | Pattern recursion — `parent_pattern_id` self-FK forms DAG (cycle blocked); `depth ≤ AutonomyPolicy.pattern_depth_cap` (default 3). A pattern's step can spawn sub-pattern | daemon topological sort, PRD §3.9 |
| D25 | Pattern-scoped autonomy — AutonomyPolicy.scope_kind adds `pattern_kind`. Defaults: workflow=L5, graph=L5, swarm=L4, agents-as-tools=L4, direct=L3 forced. Strictest of (plan-mode, pattern-mode) wins | `crates/core/`, PRD §3.7 |
| D26 | Four-pattern integrity gates — PreToolUse hook validates active pattern.kind: workflow (steps ≥ 2 + sequential evidence), graph (consensus needs distinct **(AgentSpec.name, AgentSpec.stance) tuples ≥ 2** — sybil-blocked), swarm (depth + self-spawn + fan_out ≥ 2), agents-as-tools (peer registered + peer ≥ 1). D21's pattern-aware extension | `plugin/hooks/`, daemon /patterns/active, PRD §5 Layer 2.6, §6 #17 #18 |
| D27 | Pattern shape & selection gate — `produced_via_pattern_id` NOT NULL at creation (auto `direct` row if absent). `pending → active` transition enforces D26 shape validation. Fake patterns (1-step workflow, 1-instance swarm) cannot bypass `direct`'s L3 cap | daemon write/transition paths, PRD §6 #17 |
| D28 | Reversibility first-class — Decision.reversal_plan + blast_radius_score required. L5 unlock = (a) pattern shape valid AND (b) reversal_plan NOT NULL + format valid AND (c) blast_radius_score ≤ AutonomyPolicy.l5_threshold (default 5). reversal-runner specialist handles rollback as append-only Decision (kind=consensus, reversal_of=<id>) | `crates/core/`, daemon /decisions/<id>/rollback, PRD §5 Layer 2.7, §6 #19 #20 #21 |
| D29 | Multi-session resource claims — Scenario.claimed_resources_json (path globs) + claim_status. PreToolUse hook queries daemon /scenarios/active-claims; cross-session overlap blocked + user prompt. Optional AutonomyPolicy.plan_single_session_lock. daemon-centric multi-session extends storage consistency to decision consistency | `plugin/hooks/`, daemon claim ledger, PRD §5 Layer 2.8, §6 #22 |

---

## XDG path invariant (carried from Clawket LM-8)

User data MUST NOT resolve under `~/.claude/plugins/`.

| Area | Path |
|---|---|
| Data (SQLite) | `~/.local/share/sdi/` |
| Cache (socket / pid / port file) | `~/.cache/sdi/` |
| Config | `~/.config/sdi/` |
| State (logs) | `~/.local/state/sdi/` |

Plugin install gate may write `sdi` + `sdid` binaries under `~/.claude/plugins/sdi-*/bin/`, and that is the only place plugin-managed assets may live. SQLite, sockets, logs, config must stay in the XDG paths above. The daemon will enforce this at startup (`sdid` refuses to start if any of the five paths above resolves under `~/.claude/plugins/`), and `sdi doctor` will surface violations with exit code 1. (Both checks will be implemented alongside the daemon's path resolver.)

---

## Repo conventions

- **Single Rust workspace, resolver = 2.** New crates go under `crates/` and are added to `[workspace].members` in the same change.
- **`workspace.package` carries common metadata.** Per-crate `Cargo.toml` uses `version.workspace = true` etc.
- **One binary per binary crate.** `cli` → `sdi`; `daemon` → `sdid`. Library crates expose `src/lib.rs`.
- **Plugin shell is part of the same repo.** Editing plugin manifest, MCP config, or hooks is a normal change in this tree — not a separate repository.
- **Distribution branch.** Strategy not yet locked. Either a `dist` branch carrying built binaries + plugin manifest, or direct consumption from `main` with binaries fetched from GitHub Releases. Decision tracked separately.
- **v0.5 PreToolUse gates.** Two new gates layer onto the existing delegation + active-task + autonomy chain. **D26 pattern shape advisory** — for `Agent` / `Task` dispatches whose prompt carries multi-agent intent tokens (`specialist team`, `parallel`, `swarm`, `graph review`, `fan-out`, `agents-as-tools`, `multi-agent`) or a `pattern_id` field, the hook queries `/patterns/active` and warns on stderr if no row exists (non-blocking; daemon auto-creates a `direct` row, L3-capped). **D29 resource claim block** — for `Edit` / `Write` / `NotebookEdit`, the hook queries `/scenarios/active-claims` and emits exit code 2 with a structured `{ block: 'sdi_claim_overlap', target_path, my_scenario, holders, hint }` JSON payload when another scenario's claim covers the target path. Daemon unreachable → proceed (don't lock the editor when the daemon is down). The unified emergency bypass surface is `sdi bypass arm --reason "<short reason>"` — one armed marker (XDG-cache, default TTL 60s) unlocks D21 delegation, active-task, and D29 claim overlap for the next single tool invocation. The startup-time env switch `SDI_HOOK_V05_DISABLE=1` remains for shell-rc exports that want to disable D26 + D29 specifically (effective only when Claude Code launches from a shell that already exported it). Routine bypass is a protocol violation and audit-logged on every invocation. Active scenario binding currently flows through the `SDI_ACTIVE_SCENARIO` env var until the daemon gains the AgentRun↔Scenario edge.

---

## Verification before claiming complete

Per mechanical-overrides §4 (FORCED VERIFICATION):

```sh
cargo build               # all crates compile
cargo check --workspace   # type-check
cargo clippy --workspace -- -D warnings   # once clippy is wired
cargo test --workspace    # once tests exist
```

The current skeleton has no tests and no clippy config. State this honestly in any "done" report.

---

## Commit & release

- Claude Code agents do not commit or push without explicit instruction.
- Commit message convention and release ordering will be specified in `docs/RELEASING.md` once that document lands.

---

## What to read next

1. `README.md` — repo overview (public-facing pitch).
2. [`docs/PRD.md`](./docs/PRD.md) — canonical PRD (D1–D29, seven first-class entities, multi-agent layers).
3. [`docs/ARCHITECTURE.md`](./docs/ARCHITECTURE.md) — physical components + logical agent topology.
4. `plugin/README.md` — what the plugin shell is and where its install gate plan stands.
