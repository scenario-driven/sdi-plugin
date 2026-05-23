# SDI — Scenario-Driven Implementation

> Natural-language GWT scenarios as first-class citizens. Multiple LLM agents propose, critique, and reach consensus on what to build, then implement, verify, and auto-regress across rounds — under a per-scope autonomy policy the user controls without staying tethered to the prompt.

---

## What is this?

SDI is the LLM-era successor to TDD (1990s) and BDD (2000s). The lineage:

| | spec form | verifier | who reads it |
|---|---|---|---|
| TDD | test code | the test runner | humans + the runner |
| BDD | Gherkin DSL | step definitions + runner | humans, with step glue maintained by humans |
| **SDI** | **natural language Given/When/Then** | **LLM agent** | **the LLM directly — no compilation step** |

The unit of work is the **scenario**. A plan locks in a set of scenarios. Specialist agents decompose runtime tasks, propose implementations, critique each other, and only then converge on consensus that is gated by an autonomy policy. The next round auto-replays prior scenarios as regression.

Identity & full spec: **[`docs/PRD.md`](./docs/PRD.md)** — the canonical PRD lives in this repository, decisions D1–D29 in §2.

---

## Seven first-class entities

| Entity | Role |
|---|---|
| **Plan** | Approved intent. Approval gate = ≥ 1 scenario with valid GWT (Task count irrelevant). |
| **Requirement** | SNAPSHOT-ONLY natural-language ask. Latest snapshot is the only truth; change history lives in Decision. |
| **Decision** | Append-only ADR with `kind ∈ {proposal, critique, consensus, dissensus}`. Consensus is the gate-passing form. Carries `reversal_plan` + `blast_radius_score` (D28). |
| **Scenario** | Strict Given/When/Then. Carries `tags`, `depends_on` DAG, `produced_by`/`verified_by` agents (M4 contract), plus `claimed_resources_json` + `claim_status` for multi-session safety (D29). |
| **Round** | R1 is new development; R2+ is regression. Default mode `strict-regression` replays every prior passing scenario. |
| **AutonomyPolicy** | Per-scope (plan / decision_kind / pattern_kind / global) mode ∈ {L3, L4, L5} + `l5_threshold` + `pattern_depth_cap` + `plan_single_session_lock`. Decides where the human gate sits. |
| **CollaborationPattern** *(D22, v0.5)* | Kind ∈ {workflow, graph, swarm, agents-as-tools, direct}, applies_to ∈ {plan, requirement, scenario, task, decision, round}. Persisted manifest with per-kind shape (steps/reviewers/fan_out/peer_registration). Every work entity records `produced_via_pattern_id`. |

Two non-first-class persistent entities back the multi-agent substrate: **AgentNote** (M1 blackboard, append-only journal) and **AgentSpec** (M5 runtime specialist registration, with `stance ∈ {proposer, devil_advocate, schema_guardian, performance_reviewer, security_reviewer, neutral}` for D26 sybil-fix).

---

## Multi-agent governance (D13–D29)

- **D13 — multi-agent is the body.** Single-`@main` solo execution is anti-pattern. Every flow assumes specialist agents communicating.
- **D14 — AutonomyPolicy is a first-class entity.** Per-scope mode is persisted in SQLite and gates Decision application.
- **D15 — four built-in patterns.** Workflow, Graph, Swarm, Agents-as-Tools live inside SDI. External A2A protocol is out of v1.
- **D16 — default = act with policy.** Not "ask every time" — the user toggles intervention windows, not per-decision prompts.
- **D17 — mode defaults.** New plan defaults to **L5**; plans with external surface (publish/deploy/external API) default to **L4**; `decision_kind ∈ {architecture, schema, naming-canonical}` is forced to **L4** regardless of plan mode.
- **D18 — circuit breaker always on.** One UI action demotes every policy row to L3 immediately; in-flight decisions apply at the next gate.
- **D19 — substrate runs mode-independent.** M1 blackboard, M2 hand-off, M3 negotiation, M4 scenario-as-contract, M5 self-organization keep working in any mode; mode only positions the user gate on consensus.
- **D20 — consensus is the gate unit.** Single-agent decision = L3 max. Multi-agent consensus unlocks L4/L5. Dissensus always escalates regardless of mode.
- **D21 — mandatory delegation gate.** Orchestrator (main session) is forbidden from calling execution tools (`Edit`/`Write`/`NotebookEdit`/mutating `Bash`). PreToolUse hook detects `hookInput.agent_id` absence and blocks the call — the only legitimate path is `Agent`-spawned specialist sub-agents. This is D13's mechanical enforcement face: the anti-pattern is structurally impossible, not just documented.
- **D22 — CollaborationPattern as the seventh entity.** AWS's four patterns (Workflow / Graph / Swarm / Agents-as-Tools) become persistent DB rows with lifecycle (pending → active → converged | dissensus | aborted). `direct` is the anti-pattern marker, not an escape.
- **D23 — pattern provenance is NOT NULL.** Every new work entity carries `produced_via_pattern_id`; main sessions that omit it get an auto `direct` row with a red dashboard badge + L3 cap + audit log.
- **D24 — pattern recursion via DAG.** `parent_pattern_id` self-FK, depth ≤ `AutonomyPolicy.pattern_depth_cap` (default 3). A pattern's step can spawn sub-patterns; cycles are blocked.
- **D25 — pattern-scoped autonomy.** Defaults: workflow=L5, graph=L5, swarm=L4, agents-as-tools=L4, direct=L3. The strictest of (plan-mode, pattern-mode) wins.
- **D26 — four-pattern integrity gates with sybil fix.** Graph consensus requires `≥ 2 distinct (AgentSpec.name, AgentSpec.stance) tuples` — two `impl-coder` instances with identical stance no longer fake diversity. Workflow needs sequential evidence and `steps ≥ 2`; swarm needs `fan_out ≥ 2` plus spawn-depth and self-spawn-loop blocks; agents-as-tools needs peer registration and `peer ≥ 1`.
- **D27 — pattern shape & selection gate.** Shape validation runs at `pending → active`. Fake patterns (1-step workflow, single-instance swarm, registry-empty agents-as-tools) cannot bypass `direct`'s L3 cap.
- **D28 — reversibility as a first-class constraint on L5.** Decision.reversal_plan (inverse migration / git revert SHA / fs snapshot / compensating action) + Decision.blast_radius_score gate L5 auto-apply: shape valid AND reversal_plan present AND blast_radius_score ≤ `AutonomyPolicy.l5_threshold` (default 5). The reversal-runner specialist executes rollbacks as append-only Decisions.
- **D29 — multi-session resource claims.** Scenario.claimed_resources_json (path globs) + claim_status give the daemon a decision-router role: cross-session overlap is blocked at PreToolUse with a merge-or-wait prompt. Optional `plan_single_session_lock` for high-conflict plans.

The full L3/L4/L5 semantics, scope matrix, circuit-breaker triggers, delegation-gate tool classification, and pattern integrity rules are spelled out in **[`docs/PRD.md`](./docs/PRD.md)** §3.7, §3.9, §5 Layer 0 / 1.5 / 2.6 / 2.7 / 2.8 / 3.

---

## Slash commands

| Command | Owner | Purpose |
|---|---|---|
| `/scenario` | this plugin | Create / list / retire scenarios (strict GWT). |
| `/round` | this plugin | Start R1 or R2+ with regression auto-replay. |
| `/plan` | this plugin | Create plan, manage Requirements, approve gate. |
| `/req` | this plugin | Snapshot requirements (SNAPSHOT-ONLY). |
| `/decide` | this plugin | Append Decision with `kind` (proposal → critique → consensus / dissensus). Carries `reversal_plan` + `blast_radius_score` (D28). |
| `/autonomy` | this plugin | Inspect / change AutonomyPolicy per scope; surface circuit breaker. Includes `pattern_kind`, `l5_threshold`, `pattern_depth_cap`, `plan_single_session_lock`. |
| `/note` | this plugin | Append AgentNote (M1 blackboard) — hypothesis / observation / question / handoff. |
| `/pattern` *(D22, v0.5)* | this plugin | Create / list / advance CollaborationPattern. Sub-commands for workflow / graph / swarm / agents-as-tools manifests. |
| `/goal` | Claude Code built-in | Orthogonal. SDI does not intercept it. |

---

## Repository shape

This is a **Claude Code plugin whose body is a Rust workspace**. The plugin shell, the cli, and the daemon are all surfaces of the same repository.

```
sdi-plugin/
├── Cargo.toml               # workspace root (resolver = 2)
├── crates/
│   ├── cli/                 # `sdi` binary — user/LLM entry point. Hosts `sdi mcp` subcommand.
│   ├── daemon/              # `sdid` binary — background daemon (HTTP + unix socket).
│   ├── mcp/                 # stdio MCP server library, embedded into cli.
│   ├── core/                # Domain model: Plan / Requirement / Decision / Scenario / Round / AutonomyPolicy / CollaborationPattern + AgentNote / AgentSpec.
│   └── db/                  # SQLite (rusqlite + sqlite-vec) storage adapter.
├── plugin/                  # Claude Code plugin shell
│   ├── .claude-plugin/plugin.json
│   ├── .mcp.json
│   ├── hooks/hooks.json
│   └── README.md
├── docs/
│   ├── PRD.md               # canonical product spec (D1–D29)
│   ├── ARCHITECTURE.md      # this repo's architecture + multi-agent layers
│   └── …
├── README.md                # this file
├── CLAUDE.md                # AI context for contributors / agents
├── LICENSE                  # MIT
└── .gitignore
```

Add-on repositories (separate org repos):

- [`sdi-web`](https://github.com/scenario-driven/sdi-web) — Vite/React dashboard SPA. Consumes the `sdid` HTTP API + `/events` SSE. Surfaces the autonomy panel, decision timeline, and agent-notes blackboard.
- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — Tauri 2 shell. Bundles `sdi-web/dist` and spawns `sdid` as a sidecar. Mirrors the resolved autonomy mode into the window title and tray, and exposes the circuit breaker as a global shortcut (Cmd+Shift+L / Ctrl+Shift+L).

---

## Build

```sh
cargo build
```

Builds two binaries: `sdi` (cli) and `sdid` (daemon).

---

## Prior work

SDI is the direct successor to **Clawket v3.0** (operated for roughly one month). Clawket validated that LLMs can carry long-running work state through a local SQLite + daemon + MCP architecture, but its task-centric Jira-lineage model did not enable LLM-driven verification, automatic regression, or multi-agent governance. SDI re-centers on scenarios and adds the multi-agent substrate to close those gaps.

Migration mapping is in [`docs/PRD.md`](./docs/PRD.md) §9. SDI is a new tool in a new org, not a Clawket version bump.

---

## License

MIT. See `LICENSE`.
