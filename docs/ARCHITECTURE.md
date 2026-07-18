# Architecture

Scenario-Driven Implementation (SDI) is delivered as **one repo = one local
agent plugin** for Claude Code and Codex. The plugin shell and the Rust
workspace are not separate artifacts — they are two views of the same source
tree.

## Layout (PRD §5.2)

```
sdi-plugin/
  Cargo.toml                    # Rust workspace root
  Cargo.lock
  crates/
    cli/                        # `sdi` user binary, ships MCP via `sdi mcp`
    daemon/                     # `sdid` long-lived service (axum + sqlite)
    mcp/                        # MCP server library, embedded by cli
    core/                       # Domain model + repository traits
    db/                          # rusqlite + sqlite-vec adapter
  .agents/plugins/
    marketplace.json            # Repo-scoped Codex marketplace
  plugin/                       # Claude Code / Codex plugin shell
    .claude-plugin/
      plugin.json               # Claude Code manifest (skills, commands, hooks)
    .codex-plugin/
      plugin.json               # Codex manifest (skills + inline MCP)
    .mcp.json                   # Claude MCP server registration (shared launcher)
    hooks/hooks.json            # Hook routing manifest
    commands/                   # /scenario /round /plan /req /decide /sdi-status
    skills/                     # `/sdi-overview` `/sdi-scenario` `/sdi-round`
                                # `/sdi-evidence` — 4 workflow skills (one per
                                # PRD §3 stage). Single-source: plugin.json
                                # #skillsList ⇄ sdi-hooks.cjs::SDI_SKILLS ⇄
                                # tests/lint.test.cjs SDI_SKILLS.
    adapters/
      claude/                   # 6 thin host shims (≤8 LOC each, .catch + exit 0)
      shared/sdi-hooks.cjs      # Single home for hook bodies + install gate
    bin/                        # Populated by install gate at runtime
    daemon/bin/                 # Same — sdid lives next to sdi
    scripts/setup.cjs           # Manual entry: shim → adapters/shared
    scripts/sdi-mcp.cjs         # Host-neutral MCP launcher → `sdi mcp`
    tests/                      # node --test (lint, hooks, e2e)
  docs/                         # This directory
  README.md
  LICENSE
  CLAUDE.md                     # AI-agent context, sub-repo self-contained
```

## Why one repo

PRD §5.1 fixes this. The reasoning, with market evidence:

1. **Plugin caches trap the plugin dir as the only stable root.**
   The official plugin spec
   ([code.claude.com/docs/en/plugins-reference](https://code.claude.com/docs/en/plugins-reference))
   states marketplace plugins are copied into `~/.claude/plugins/cache/<plugin>/`.
   Codex also installs marketplace plugins into its own plugin cache. Path
   traversal outside the installed plugin root (`../crates`) is not a runtime
   contract. Anything the runtime needs must live inside the plugin dir.
2. **`bin/` is the Claude Code standard for executables.** The plugin spec
   exposes `<pluginRoot>/bin/` on the Bash tool PATH. `plugin/bin/sdi` is the
   intended runtime location for the user binary post-install.
3. **`src` and `dist` separate by branch, not by repo.** PRD §5.1 names two
   branches: `main` (source) and `dist` (built binaries + manifest). One repo,
   two branches — keeps the source/binary correspondence atomic and reviewable.
   Biome / Deno / rust-analyzer follow the same single-repo Rust workspace
   pattern.
4. **Clawket exemplar.** The Clawket plugin shell at
   `clawket/clawket/{bin,daemon/bin,web,adapters,hooks,skills,scripts,tests}`
   is the closest published precedent for a Claude Code plugin that bundles
   CLI + daemon + MCP. SDI follows the same shape, with the CLI workspace
   colocated in the same repo (Clawket splits across 7 repos; PRD §5.1
   consolidates).

## Binary resolution

`plugin/adapters/shared/sdi-hooks.cjs::resolveSdiBin` checks in this order:

1. `SDI_BIN` env var (caller-supplied; honored unconditionally).
2. `<pluginRoot>/bin/sdi` — release tarball layout (post-install).
3. Workspace `target/release/sdi` — locally built (`cargo build --release`).
4. Workspace `target/debug/sdi` — locally built (`cargo build`).
5. `which sdi` — PATH lookup.

`sdid` resolves alongside `sdi` (same dir, then `<pluginRoot>/daemon/bin/sdid`,
then PATH). The release-fetch path
(`SDI_RELEASE_FETCH=1`) is structurally present but errors out until a
GitHub Release exists — distribution is excluded from current scope.

Both host MCP declarations call `plugin/scripts/sdi-mcp.cjs`, not `bin/sdi`
directly. The wrapper uses the resolver above, sets `SDI_DAEMON_BIN` when it
can resolve `sdid`, and then execs `sdi mcp`. That keeps MCP usable from local
source checkouts (`target/debug`) and release bundles (`plugin/bin` +
`plugin/daemon/bin`).

## Data location (LM-8 invariant)

Plugin code may write **only** under `pluginRoot` (the plugin dir) and via the
single audit-log channel `appendHookLog()` into XDG state paths:

| Surface | Path                          | Owner          |
| ---     | ---                           | ---            |
| Data    | `~/.local/share/sdi/`          | daemon         |
| Cache   | `~/.cache/sdi/`                | daemon         |
| Config  | `~/.config/sdi/`               | user           |
| State   | `~/.local/state/sdi/hook.log`  | plugin (append-only) |

The daemon enforces this at startup (`paths::ensure_no_plugin_overlap`) and
`sdi doctor` re-checks it. `~/.claude/plugins/cache/sdi/` may carry the
distributed plugin tree but must **never** carry user data — `/plugin install`
re-creates that tree, which would silently destroy the SSoT.

`SDI_HOME` env overrides the XDG root, used by the test suite to isolate
per-test homes.

## Host manifests

The host-specific manifests are intentionally thin:

- `plugin/.claude-plugin/plugin.json` keeps the Claude Code `skillsList`
  surface and the legacy command/agent discovery conventions.
- `plugin/.codex-plugin/plugin.json` points at the same `skills/` tree and
  registers the shared MCP launcher inline with `${PLUGIN_ROOT}/scripts/sdi-mcp.cjs`.
- `.agents/plugins/marketplace.json` exposes the existing `plugin/` directory
  as the repo-local Codex marketplace entry; there is no duplicated
  `plugins/sdi/` copy.

## Hook surface

Six events wired in `plugin/hooks/hooks.json`:

| Event             | Shim                              | Responsibility                                    |
| ---               | ---                               | ---                                               |
| `SessionStart`    | `session-start.cjs`               | `ensureInstalled` + dashboard banner              |
| `UserPromptSubmit`| `user-prompt-submit.cjs`          | Inject active Plan/Round/Task context             |
| `PreToolUse`      | `pre-tool-use.cjs`                | Deny Edit/Write/Bash/Agent without active Task    |
| `PostToolUse`     | `post-tool-use.cjs`               | Audit file changes against active Task            |
| `SubagentStart`   | `subagent-start.cjs`              | Bind sub-agent to Task                            |
| `SubagentStop`    | `subagent-stop.cjs`               | Append sub-agent result to Task evidence          |

Each shim is ≤8 LOC, wraps the shared call with `.catch()`, exits 0 on failure.
Codex loads the same `hooks/hooks.json` default hook file and provides
compatibility env vars for existing plugin hooks; `sdi-hooks.cjs` also prefers
`PLUGIN_ROOT` when present. Hook crash safety is a structural property of the
shim layer, not a runtime choice. See [HOOK_ENFORCEMENT.md](./HOOK_ENFORCEMENT.md)
for the enforcement semantics.

## Surfaces inside this repo

The CLI / daemon / MCP / core / db quintet is the load-bearing surface
and ships under one workspace version (`Cargo.toml [workspace.package].version`).
There are no `crates/web` or `crates/desktop` inside this repository.

The **dashboard SPA lives in this repository** at `plugin/web/` (React +
Vite + Tailwind). It talks only to the daemon over HTTP + SSE — no
compile-time coupling to the Rust crates — and `sdid` serves its `dist/`
over tower-http `ServeDir`. The plugin shell and SPA ship together under
the one workspace version.

One add-on surface lives in a **separate repository** under the same
GitHub org and consumes the daemon's public HTTP contract:

- **[`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop)** — Tauri 2 shell.
  Hosts the `plugin/web/dist` bundle in a native window and spawns `sdid`
  as a child process via the resolver in its `src/daemon.rs` (env / plugin
  layout / XDG / PATH). The desktop binary embeds no daemon code; it is a
  thin launcher.

Because it rides on the daemon's stable HTTP contract, `sdi-desktop` is
not version-pinned against this workspace and has no shared release
manifest with it. It ships on its own cadence; the daemon's versioned API
is the only coupling point.

## Multi-agent governance (PRD §5 Layer 0 / 1 / 1.5 / 2 / 2.5 / 2.6 / 2.7 / 2.8 / 3 / 4)

Above the physical components (`sdi` cli + `sdid` daemon + plugin shell)
sits a **logical agent topology** that the daemon enforces and the SPA /
desktop surface to the user. Each layer has an orthogonal responsibility;
together they implement decisions D13–D29 of the PRD.

### Layer 0 — Autonomy Mode (where the human gate sits)

`AutonomyPolicy` (first-class entity, table `autonomy_policy`) stores the
**mode** for a `(scope_kind, scope_id, decision_kind)` triple:

| Mode | Meaning | Default in |
|---|---|---|
| **L3** (always ask) | Every consensus needs explicit user confirm | Cautious plans; circuit-breaker fallout |
| **L4** (notify + timed auto-apply) | Consensus is announced; if no veto in `timeout_ms`, it applies | External-surface plans; D17 forced kinds |
| **L5** (immediate apply + post-hoc evidence) | Consensus applies instantly, evidence appended after | New internal-only plans |

Defaults (D17, enforced in `crates/core` validation):

- New plan, no external surface → **L5**
- Plan with external surface (publish / deploy / external API) → **L4**
- `decision_kind ∈ {architecture, schema, naming-canonical}` → **L4, forced=true** (cannot be demoted to L5)

Dissensus (D20) and circuit-breaker (D18) escalate to the human gate
regardless of mode. Mode controls **gate position**, never agent
communication.

### Layer 1 — Orchestrator (thin spawn / monitoring only)

The orchestrator agent inside Claude Code is intentionally thin (D16).
Allowed: spawn initial specialists at plan/round start, poll
`AgentNote` for stagnation, aggregate evidence for round progress,
detect circuit-breaker triggers. **Forbidden**: deciding scenario
decomposition, choosing implementation approach, resolving disruption,
writing Decision bodies. Those originate in Layer 2 consensus.

### Layer 1.5 — Delegation Enforcement (D21 mechanical gate)

The Layer 1 allowed/forbidden split is enforced at the **PreToolUse**
hook layer, not just by convention. Detection signal (Claude Code
official hook contract): `hookInput.agent_id` is present **only** when
the hook fires inside an Agent-spawned sub-agent; absence = main
orchestrator session.

| Caller | Tools blocked | Tools allowed |
|---|---|---|
| Main (`agent_id` absent) | `Edit`, `Write`, `NotebookEdit`, mutating `Bash` (not in read-only allowlist, not in destructive blacklist) | `Read`, `Grep`, `Glob`, `WebSearch`, `WebFetch`, `Agent`, `TaskCreate/Update/List`, `SendMessage`, `ScheduleWakeup`, `Skill`, MCP read tools, read-only `Bash` (status / log / diff / typecheck / lint / cargo check) |
| Specialist (`agent_id` present, `agent_type` ∈ AgentSpec registry) | nothing | full toolset |
| Specialist (`agent_id` present, `agent_type` ∉ registry) | every execution tool with `rogue-specialist` code | read-only tools only |

Bypass paths (all audited):
- Circuit-breaker trigger (Layer 3) → main session temporarily allowed
  to execute; `audit=circuit-override` is appended to the activity log
  for every tool call until the breaker resets.
- `sdi bypass arm --reason "<short reason>" [--ttl <seconds>]` →
  primary one-shot escape. Writes a JSON marker
  (`{reason, armed_at, expires_at, ttl_seconds}`) at
  `~/.cache/sdi/bypass-once`. One armed marker unlocks every mutating
  PreToolUse gate (D21 delegation, active-task, D29 claim overlap) for
  the next single tool invocation, then auto-consumes; the hook deletes
  the marker before honoring it. TTL default 60s; expired markers are
  cleaned up but do NOT open the gate. `sdi` is on the D21 read-only
  Bash whitelist so the main session can arm the marker directly — no
  specialist required for the bypass path itself. `sdi bypass status`
  reports state ∈ {`armed`, `expired`, `absent`} + TTL remainder;
  `sdi bypass disarm` removes the marker (idempotent).
- `SDI_DELEGATION_BYPASS=1` env-var → startup-time fallback, scoped to
  Claude Code sessions launched from a shell that exported the var.
  Does NOT catch inline `VAR=1 cmd` prefixes — Claude Code spawns the
  hook before any user shell expands the prefix.
- All bypasses emit a stderr warning + a gate-specific audit row
  (`pre_tool_use_delegation_bypass`, `pre_tool_use_active_task_bypass`,
  `pre_tool_use_claim_bypass`, with `source` ∈ {`marker`, `env`}) in
  `~/.local/state/sdi/hook.log`. Routine use is a protocol violation
  tracked by the auditor.

Hook implementation lives at `plugin/hooks/pre-tool-use.cjs` and
delegates to the shared helper in `plugin/hooks/lib/delegation.cjs`.
Manifest entry: `plugin/hooks/hooks.json`. Test fixture:
`plugin/tests/delegation.test.cjs`.

### Layer 2 — Specialist Sub-agents (peer, no hierarchy)

Eight static specialists are peers, not subordinates of the orchestrator:
`gwt-converter`, `scenario-decomposer`, `impl-coder`, `test-runner`,
`regression-runner`, `disruption-analyst`, `decision-resolver`,
`schema-architect`. None can override another's output — consensus
forms by accumulation of proposal + critique Decisions (M3).

Runtime specialists may be added via `AgentSpec` (M5 self-organization)
when the static set proves insufficient for a domain.

### Layer 2.5 — Communication Substrate (M1–M5)

Mode-independent (D19). Every flow except the natural-language → GWT
normalization in PRD §4.2 uses at least one of these:

| Mechanism | What it is | Backing storage |
|---|---|---|
| **M1 Blackboard** | Async append + poll | `agent_note` (append-only, retired-not-deleted) |
| **M2 Peer hand-off** | 1:1 explicit ack | `agent_note` with `to_agent` + `receipt_acknowledged_at` |
| **M3 Negotiation** | proposal → critique → consensus / dissensus | `decision.kind` four-stage append-only |
| **M4 Scenario-as-Contract** | Scenario.depends_on DAG = agent interface | `scenario.depends_on` + `produced_by` / `verified_by` |
| **M5 Self-organization** | Dynamic specialist spawn from blackboard signals | `agent_spec` (runtime registration) |

`AgentNote.retired_at` is a soft tombstone — rows are never physically
removed (audit invariant).

### Layer 2.6 — Pattern Enforcement (D26 four-pattern integrity gates)

The `CollaborationPattern` first-class entity (D22, table
`collaboration_pattern`) makes AWS's four patterns persistent rows
instead of code constants. Every work entity (plan / requirement /
scenario / task / decision / round) carries `produced_via_pattern_id`
(D23). The PreToolUse hook queries the daemon for the active pattern of
the sub-agent's scope and applies the matching gate:

| Pattern.kind | Shape gate (pending → active) | Runtime gate (per-tool) |
|---|---|---|
| **workflow** | `steps_json` length ≥ 2 | Sequential evidence: step N requires step N-1 evidence row before allowing step N's specialist to execute |
| **graph** | `reviewers_json` distinct `(AgentSpec.name, AgentSpec.stance)` tuples ≥ 2 | `Decision.kind='consensus'` apply requires distinct tuple count ≥ 2 on `proposers_json` — same-name same-stance siblings count as one (sybil block) |
| **swarm** | `fan_out_json` length ≥ 2 | parent chain depth ≤ `AutonomyPolicy.pattern_depth_cap`; self-spawn of same `agent_type` blocked |
| **agents-as-tools** | `peer_registration_json` length ≥ 1 | callee must appear in caller's `peer_registration_json` |
| **direct** | (none) | Anti-pattern marker — auto L3 cap on AutonomyPolicy, red dashboard badge, `audit=direct-pattern-marker` |

D27 closes the escape hatch: `pending → active` transition enforces the
shape gate, so fake patterns (1-step workflow, single-instance swarm,
empty agents-as-tools registry) cannot reach `active` and thus never
unlock L4/L5 mode. The `direct` row is the only legitimate path for
solo-flow entities, and it pays the L3 cap + visible badge cost.

The pattern row also drives the dashboard timeline view (`plugin/web/`)
— each `/events` `pattern_lifecycle` payload moves the entity through the
dashboard's pattern-tree visualization.

### Layer 2.7 — Reversibility (D28 L5 recovery-cost gate)

L5 auto-apply's real blocker is the recovery cost of a wrong decision,
not the consensus mechanism. Two `decision` columns gate it:

- `reversal_plan` (JSON): one of `{type: 'migration_sql', sql, dependencies}` / `{type: 'git_revert', sha}` / `{type: 'fs_snapshot', snapshot_ref}` / `{type: 'compensating_action', action_spec}`. Produced jointly by `impl-coder` + `schema-architect` during proposal/critique; format-validated by `decision-resolver` before consensus admission.
- `blast_radius_score` (0–10): static per-kind by default (architecture=10, schema=8, naming-canonical=4, impl-internal=3, doc-only=1); extendable via `AgentSpec.blast_radius_rules_json`.

L5 unlock requires all three: (a) active pattern is shape-valid, (b)
`reversal_plan` not null + format valid, (c) `blast_radius_score ≤
AutonomyPolicy.l5_threshold` (default 5). Failing any → L4 timed gate.

Rollback (`POST /decisions/<id>/rollback`) dispatches a new
**`reversal-runner` specialist** (registered at v0.5) that runs the
plan's `type`-specific handler. The rollback is itself appended as a
new `Decision` row (`kind='consensus'`, `reversal_of=<original id>`) —
the original row is never mutated (D12 SNAPSHOT-ONLY). Failures append
a `kind='dissensus'` Decision and escalate to Layer 0.

### Layer 2.8 — Resource Claims (D29 multi-session decision consistency)

Daemon-centric multi-session inherits storage consistency from SQLite
ACID; decision consistency needs an additional layer. The `Scenario`
entity owns the claim:

- `claimed_resources_json` (path glob list, e.g. `["crates/db/migrations/*.sql", "plugin/agents/*.md"]`) — declared at scenario creation.
- `claim_status` (`none → requested → active → released`).

Transitions: `confirmed` scenario auto-issues `requested`; daemon
checks all other `active` claims for glob-overlap; zero overlap →
`active`; overlap → stays `requested` + user prompt ("scenario A vs B:
merge or wait"). Round completion or explicit `release` returns to
`released`.

PreToolUse hook (after the Layer 1.5 D21 check and Layer 2.6 D26
check) queries `/scenarios/active-claims` on every `Edit` / `Write` /
`MultiEdit`. Two failure modes:

1. The path is not in the caller's active scenario claim → block with
   `out-of-claim` code.
2. The path overlaps another session's active claim → block with
   `cross-session-conflict` code + emit the merge-or-wait prompt.

`AutonomyPolicy.plan_single_session_lock=true` is the optional
strictest mode: a plan with one active scenario cannot have another
session activate a sibling scenario at all. Off by default (multi-
session collaboration is the natural pattern); on for
high-conflict single-developer plans.

The daemon's role here is **decision router**: every session on the
same plan connects to the same `sdid`, so the daemon sees every
attempted resource overlap. Sessions on separate daemons (forbidden by
the LM-8 XDG invariant + the single-daemon assumption) would silently
race.

### Layer 3 — Per-decision-kind policy (Circuit breaker host)

Layer 0 sets the plan default. Layer 3 refines per `(scope, decision_kind)`
via `AutonomyPolicy` rows. The **circuit breaker** lives at this layer:
one user action (tray menu / SPA panel / global shortcut on desktop)
updates every policy row to `mode='L3'`, `set_by='circuit-breaker'`,
in-flight decisions resume at the next gate. Triggers also fire
automatically on dissensus accumulation, M5 spawn-loop, or AgentNote
write-rate spikes — see PRD §5 Layer 3.

### Layer 4 — Sub-agent System Prompts (allowed / forbidden patterns)

`AgentSpec.system_prompt` content rules (PRD §5 Layer 4):

- **Forbidden**: "if unsure, ask the human" patterns (breaks D18 peer
  relationship), "delegate the final call to the orchestrator" (breaks
  D16 thin-orchestrator invariant), "ask the user directly" (Layer 0 /
  3 own all human escalation paths).
- **Required**: cite evidence on proposal (file:line / scenario id /
  alternative comparison); read peer critiques before promoting your
  own proposal to consensus; tag every AgentNote with its scope
  (plan / round / scenario / task).

Changing `AgentSpec.system_prompt` is itself a Decision of
`decision_kind = agent-spec-change`, gated at L4.

## How the surfaces render this

- **`sdid` (daemon)** owns the SQLite tables (`autonomy_policy`,
  `decision`, `agent_note`, `agent_spec`, `collaboration_pattern`,
  plus the v0.5 columns on `scenario` and `decision`), validates D17
  forced kinds + D26/D27 pattern shape gates + D28 reversal_plan
  format + D29 claim overlap on write, exposes
  `/autonomy_policies`, `/decisions`, `/decisions/<id>/rollback`,
  `/agent_notes`, `/patterns`, `/patterns/active`,
  `/scenarios/active-claims`, and
  `/autonomy_policies/circuit_breaker` over HTTP, and emits matching
  SSE event kinds on `/events`
  (`autonomy_policy_changed`, `decision_appended`, `agent_note_appended`,
  `pattern_lifecycle`, `claim_status_changed`, `rollback_completed`,
  `circuit_breaker_tripped`).
- **`sdi` (cli)** wraps each of the above as a subcommand for
  unattended use (`sdi pattern create / show / advance`, `sdi decision
  rollback`, `sdi scenario claim`) and as the MCP server via `sdi mcp`
  for in-IDE LLM consumption.
- **the dashboard SPA (`plugin/web/`)** renders the AutonomyPanel (per-scope
  L3/L4/L5 chips + circuit-breaker button), the Decision timeline with
  `kind` + `reversal_plan` + `blast_radius_score` badges, the
  AgentNotesPanel (live blackboard / hand-off inbox), the new
  **PatternTimeline** (pending → active → converged/dissensus/aborted
  per CollaborationPattern row, with kind chip + depth indent), the
  **PatternTree** (parent_pattern_id recursive view), the
  **ReversibilityView** (one-click rollback trigger per Decision with
  visible blast_radius_score), and the **ClaimsLedger** (per-scenario
  claimed_resources_json with cross-session overlap highlighting), all
  on top of `/events` SSE.
- **`sdi-desktop` (add-on Tauri)** mirrors the resolved autonomy mode
  into the window title and tray menu, registers Cmd+Shift+L /
  Ctrl+Shift+L as a global circuit-breaker shortcut, and shows a
  **tray badge** with active CollaborationPattern count (red dot when
  a `direct` pattern is active in any plan) — all state read through
  the daemon's HTTP API, no back-channel.
