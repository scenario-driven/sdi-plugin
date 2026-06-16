# Changelog

All notable user-visible changes to the SDI plugin (CLI + daemon + plugin shell)
are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/),
and the project adheres to [Semantic Versioning](https://semver.org/).

Scope: commands, hooks, MCP read tools, and breaking wire-shape changes. The
workspace `[workspace.package].version` is the single source of truth and is
mirrored by the plugin manifest (`plugin/.claude-plugin/plugin.json`).

## [0.6.0] - 2026-06-16

### Added
- **Decompose-time collaboration-pattern decision (D13 made reachable).**
  Until now every CollaborationPattern in practice was `direct` — the solo-flow
  anti-pattern — even though SDI's identity (D13) is that multi-agent
  orchestration is the *body*. The cause was two missing wires, not a data bug:
  1. **`sdi task create … --produced-via-pattern <PAT-ID>`** — `POST /tasks`
     gained an optional `produced_via_pattern_id`, so a task can finally be
     created **under** a chosen pattern. The daemon validates the reference
     resolves in the task's plan, is `active` (past the D27 shape gate), and is
     scope-compatible (plan-scoped → this plan; round-scoped → this round);
     otherwise it is rejected rather than silently degrading to `direct`.
     Omitting the flag keeps the `direct` back-fill (the honest solo marker).
  2. **Structural decompose advisory** — the prior D26 advisory only fired on
     `Agent`/`Task` dispatches that already contained a multi-agent intent token
     (`swarm`, `parallel`, …), a chicken-and-egg that ordinary decompose never
     tripped. A new advisory fires on the *structural* seam instead — `sdi round
     activate <R>` (main session) and the first `sdi task create <R> …` of a
     round (decomposer sub-agent) — and nudges the LLM to run the
     pattern-orchestrator when no non-`direct` active pattern governs the round's
     plan. Non-blocking; silent once a real pattern exists or the create already
     carries the binding flag.

  The `sdi-round` skill now makes the pattern decision an explicit step **before**
  decompose, and the pattern-orchestrator / `/pattern` docs describe the binding
  hand-off. The orchestrator, pattern-critic, patterns table, and autonomy
  unlock (workflow/graph=L5, swarm/agents-as-tools=L4) were already complete —
  this release connects them to the everyday round loop so the multi-agent body
  is actually used instead of structurally bypassed.

## [0.5.3] - 2026-06-16

### Fixed
- **Daemon no longer orphans on `daemon stop` when a dashboard tab is open.**
  axum's graceful shutdown waited for every in-flight connection to finish, but
  the `/events` SSE stream is long-lived and never completes — so SIGTERM closed
  the listener (the port stopped responding, so `sdi daemon stop` reported
  success) yet the process hung forever, leaving an orphan still holding the
  SQLite DB. Each stop-while-dashboard-open leaked a daemon. The daemon now
  bounds graceful shutdown: after the signal it waits a 3s grace window for
  in-flight work, then force-exits. No open SSE → still exits immediately
  (~0.1s); SSE open → exits within the grace window instead of hanging.

## [0.5.2] - 2026-06-16

### Changed
- The **SessionStart banner** is now a Clawket-style work summary: the active
  plan, scenario counts (confirmed / draft / retired), in-flight + backlog task
  counts with in-progress detail, decisions (with a provisional flag), recent
  activity, and — the headline — the daemon-computed **next step** (`sdi next`,
  #15) with its reason and any provisional decisions to revisit. Built from one
  `/handoff` + one `/next` fetch; degrades gracefully when the daemon can't
  supply a field.

### Fixed
- The old SessionStart banner printed `undefined` for in-flight tasks (it read
  a non-existent `title` field instead of `description`) and suggested commands
  that don't exist (`sdi scenario add`, `sdi plan create --project --title`).
  Corrected to the real positional signatures.

## [0.5.1] - 2026-06-16

### Added (dashboard)
- The dashboard now reflects the v0.5.0 daemon data it was missing:
  - **Retired scenarios** (#8) carry a `retired` badge, are dimmed, show the
    exclusion note, and have **Retire / Restore** actions in ScenarioDetail.
  - **Provisional decisions** (#16) carry a `provisional` badge and their
    "revisit when …" condition in DecisionTimeline and DecisionDetail.
  - A new **Next** view surfaces `sdi next` (the computed next step +
    provisional decisions to revisit) and the in-progress task's
    `sdi task brief` (linked GWT, verification baseline, evidence format,
    prohibitions).

### Fixed (dashboard)
- The `ScenarioStatus` TS type was `'proposed' | 'confirmed'`, but the daemon
  serializes `'draft' | 'confirmed'` — so the "Confirm scenario" button never
  appeared and scenario status glyphs/counts keyed off a value that never
  arrived. Corrected to `'draft'` across ScenarioDetail, PlanDetail, PlanTree,
  and SummaryView.

## [0.5.0] - 2026-06-16

Resolves the 13 dogfooding issues #4–#16. Decision rationale (with primary
sources) is recorded in `docs/decisions/ADR-001-issues-4-16-resolution.md`.

### Fixed — D21 delegation gate (#4, #9, #10, #11, #14)
- Unregistered `agent_type` (e.g. Claude Code's built-in `general-purpose`)
  is no longer hard-blocked. It acts at **L3** (read + execution work) with a
  one-line advisory; D26 consensus autonomy stays out of reach (needs a
  registered `(name, stance)` tuple). The old deny was a deadlock with no
  escape hatch (#11).
- The specialist registry is read from three roots — project `.claude/agents`,
  user `~/.claude/agents`, and the plugin — matching Claude Code's own
  subagent discovery, with mtime cache invalidation (#4/#11).
- The `sdi` read-only exemption is split by subcommand: the main session may
  author plan/scenario/round/decide (the spec, per D2/D3) and read tasks, but
  task lifecycle mutation delegates. Absolute paths to the bundled binary,
  read-only `gh`, `cd`/`export`/`VAR=` prefixes, and `/dev/null` redirects are
  recognised; `Monitor` is gated like `Bash` (#4/#10).
- The active-task gate reads **daemon state** (an in_progress task in the
  active plan) instead of the unsatisfiable `SDI_ACTIVE_TASK` env, and points
  at the real `sdi task start` (#9).
- The machine-global bypass marker's concurrency limit is documented: per-
  `(session, agent)` scoping is impossible because Claude Code exposes neither
  id as an env var (#14). #9 removes the routine driver of bypass.

### Fixed — data integrity (#12, #13)
- `task complete` is now atomic: the done transition, evidence write, and
  round-result mirror commit together or not at all. Each evidence
  `scenario_id` is resolved (SCN ULID **or** plan-scoped short code) and must
  be one of the task's parent scenarios — ghost / non-parent / short-code
  references are rejected up front instead of FK-failing after a partial
  commit.

### Fixed — doc/CLI alignment (#5, #6, #7)
- Round mode is `strict-regression | forward-only` (D6); `additive` is now
  accepted as an alias for `forward-only` (the CLI help and sdi-round skill
  advertised it but the daemon rejected it). Disruption is a `--disruption`
  policy, not a mode (#5).
- `round activate` returns `scenarios_needing_verification` (new +
  carried-failing/blocked, GWT inline) (#7).
- The sdi-round skill's `sdi task create --title --tier` example is corrected
  to the real positional form; priority lives in scenario tags. The short_code
  409 states that cancelled/terminal entities keep their code (#6).

### Added
- **Scenario retirement** (#8): `sdi scenario retire | unretire`
  (POST `/scenarios/:id/retire` | `/unretire`). Reversible, history-
  preserving, orthogonal to the draft/confirmed status (preserved for exact
  restore). Retired scenarios drop out of the approve count, the
  needs-verification set, and strict-regression carry-over.
- **`sdi next`** (#15): the single mechanical next step computed from daemon
  state, plus provisional decisions to revisit.
- **`sdi task brief <TASK-ID>`** (#15): linked scenarios' GWT inline + round
  baseline + evidence format + report schema + prohibitions.
- **`sdi round baseline <ROUND-ID> [--set <json>]`** (#15): per-round
  verification baseline, surfaced in the brief.
- **Provisional decisions** (#16): `sdi decide create … --supersede-when
  "<condition>"`. The decision stays accepted and in effect but is flagged for
  revisit; the provisional set is `supersede_when IS NOT NULL` (no new status).

### Migration
- Automatic. 012 adds `scenarios.retired_at`; 013 adds
  `decisions.supersede_when`; 014 adds `rounds.baseline_json` — all
  `ALTER ADD COLUMN`, no table rebuild.

## [0.4.2] - 2026-06-10

### Changed
- The detail drawer is now an **overlay** above the main content (Clawket
  parity) instead of an inline panel that squeezed the board: fixed to
  the right edge with a dimmed backdrop, click-outside closes, slide-in
  animation, and the drag-resize handle from 0.4.0 carries over (width
  still persists; max widens to 90vw now that no side-by-side layout
  constrains it).

## [0.4.1] - 2026-06-10

### Fixed
- **Migration 011 crashed on databases holding runtime-minted `direct`
  sentinels.** 011 guarded its sentinel INSERT by 009's deterministic id
  scheme (`CP-DIRECT-<plan_id>`), but `ensure_direct_pattern` mints
  runtime sentinels as `CP-<ulid>` — so a plan that already had a runtime
  sentinel passed the guard and the INSERT collided with the
  `(plan_id, short_code)` UNIQUE that migration 010 had just introduced,
  aborting daemon startup (the migration rolled back cleanly; no data was
  affected). 011 now resolves the solo-flow marker by its semantic key —
  `(plan_id, kind = 'direct')` — regardless of id scheme, reuses an
  existing runtime sentinel instead of duplicating it, and links
  NULL-provenance rows to whichever sentinel the plan carries.
  **v0.4.0 is marked pre-release**: any database that ever ran a
  workspace daemon alongside the 0.3.0 plugin can hit the crash. Upgrade
  straight to 0.4.1; databases where 0.4.0's migration succeeded are
  unaffected (the outcomes converge).

## [0.4.0] - 2026-06-10

### Fixed
- **Live dashboard updates restored.** The daemon's `/events` SSE stream
  named every event (`event: <kind>`), but the dashboard consumes the
  stream via `EventSource.onmessage` — which only receives UNNAMED
  (default `message`-type) events. The result was a permanently silent
  live channel: task status changes never reflected without a manual
  reload. Events are now sent unnamed; the `kind` field inside the JSON
  envelope remains the dispatch key (GH dogfooding find). **Wire-shape
  note:** consumers that dispatched on the SSE `event:` line must fall
  back to the envelope's `kind` — `sdi-desktop` is updated accordingly
  and accepts both shapes.
- **D21 delegation gate is quote-aware** (#3). The read-only Bash check
  treated metacharacters inside quoted arguments as shell operators, so
  natural-language GWT clauses (`--given "a user (admin)…"`) and even the
  gate's own escape hatch (`sdi bypass arm --reason "(…)"`) were blocked.
  Quoted spans are now masked before operator detection (single quotes
  fully inert; `$`/backtick stay live inside double quotes), pure fd
  duplication (`2>&1`) is allowed, and chains split on unquoted
  `&&`/`||`/`;`/`|` pass only when EVERY segment is whitelisted —
  `ls && grep` passes, `sdi … && rm -rf` does not.
- **`short_code` uniqueness is per-plan, as documented** (#2). The schema
  declared a global single-column `UNIQUE` on seven entities
  (plans / requirements / decisions / scenarios / rounds / tasks /
  collaboration_patterns), so a fresh plan could not mint `SC-1` once any
  other plan owned it. Migration 010 rebuilds the tables with composite
  uniqueness — `(project_id, short_code)` for plans, `(plan_id,
  short_code)` for the rest — and tasks gain a denormalized `plan_id`
  column (backfilled through rounds) to carry the constraint. The 409
  now names the conflicting scope ("short_code already used within this
  plan") instead of leaking raw SQLite constraint text.
- **Daemon zombie / liveness misread** (#1). `is_running` trusted
  `kill(pid, 0)`, which is also true for a `<defunct>` zombie, so a
  crashed daemon under a long-lived `sdi mcp` parent read as alive and
  blocked restart. Liveness is now a TCP connect probe against the
  daemon's port, and autostart double-forks so `sdid` reparents to
  init(1) and can never become a zombie.
- **Active-patterns badge counts the current project only.** The topbar
  badge fetched the unscoped `/patterns/active` and summed rows from
  every project in the database (12 shown while the open project's
  Patterns view was empty). The badge is now project-scoped and counts
  real orchestration only — permanently-active `direct` solo-flow
  markers are excluded from the number and signaled by the red dot
  instead.

### Added
- `GET /patterns/active?project_id=<PROJ-…>` — optional project scoping
  (through plans) for the active-pattern listing. The unscoped form is
  unchanged for hook gates and CLI consumers.
- Detail drawer is drag-resizable from its left edge (parity with the
  Clawket dashboard); width persists across sessions
  (`localStorage["sdi.drawer.width"]`, min 360px).

### Migration
- Automatic, two steps on first daemon start under v0.4.0. Migration 010
  rebuilds the seven `short_code` tables (existing rows were globally
  unique, hence already unique in the narrower scopes — no data risk;
  FTS5 mirrors are rebuilt and verified by `PRAGMA foreign_key_check`).
  Migration 011 re-runs the idempotent D23 direct-provenance backfill to
  repair rows created by pre-v0.4 daemons after migration 009 had
  already been consumed.

## [0.3.0] - 2026-05-29

### Added
- Project entity gains three first-class metadata fields: `description`
  (free-form text, nullable), `enabled` (soft-disable flag, default
  `true`), `wiki_paths` (array of wiki tree roots, default `["docs"]`).
  Migration 008 backfills existing rows in place — `enabled=1`,
  `description=NULL`, `wiki_paths_json='["docs"]'` — so no manual
  intervention is required.
- `sdi project disable <ID>` / `sdi project enable <ID>` (idempotent,
  emit `project.disabled` / `project.enabled` SSE events) and
  `sdi project delete <ID> [--force]` (cascades every PROJ-scoped row:
  plans, requirements, scenarios, rounds, tasks, decisions, comments,
  questions, knowledge, agent notes, autonomy policies, collaboration
  patterns, activity events). Delete refuses by default when any task
  under the project is `in_progress`; `--force` overrides.
- `sdi project update <ID>` now accepts `--description "<text>"`
  (empty string clears) and repeatable `--wiki-path <p>` alongside the
  existing `--name`. PATCH-style — every flag is independently optional.
- Daemon: `POST /projects/:id/disable`, `POST /projects/:id/enable`,
  `DELETE /projects/:id[?force=true]`. `PUT /projects/:id` is now
  PATCH-style — `name`, `description`, `enabled`, `wiki_paths` all
  optional; `description: null` clears the field.
- Dashboard SPA gains `ProjectSettings` (standalone view) and
  `ProjectSettingsModal` (lifted into App.tsx). Project switcher rows
  carry a gear icon to open settings, render disabled projects with a
  muted `off` badge, and dim the entry text. Delete-cascade hands the
  selection back to the next available project automatically.

### Changed
- `PUT /projects/:id` migrates from `{name}` only to PATCH-style. The
  pre-v0.3 single-field shape still works (a body with just `name` is a
  one-field PATCH), so existing CLI / web callers stay compatible.
- PreToolUse hook honors `project.enabled === false` (or legacy integer
  `0`) by short-circuiting every mutating gate (D21, active-task, D29).
  Audit row `pre_tool_use_skip` carries `reason: 'project-disabled'`
  plus the project id so the user can grep for sessions that ran in
  ungoverned mode.

### Migration
- Automatic. The new project columns are added with SQL `DEFAULT`s, so
  existing databases backfill cleanly on the first daemon start under
  v0.3.0. `sdi doctor` reports the migration in the standard schema
  version output. The pre-v0.3 `PUT /projects/:id {name: "x"}` request
  shape continues to work without code changes on the client side.

## [0.2.1] - 2026-05-29

### Changed
- Documentation-only release. `plugin/README.md`, `plugin/README.ko.md`, and
  the `sdi bypass` clap `--help` text are now aligned with the v0.2.0
  behavioral surface — `sdi bypass arm` is presented as the primary emergency
  override, env switches (`SDI_DELEGATION_BYPASS`, `SDI_BYPASS_HOOKS`,
  `SDI_HOOK_V05_DISABLE`) as startup-time shell-rc fallbacks. No behavioral
  change vs v0.2.0. Cut to ship the refreshed guides + CLI help to installed
  clients on the next marketplace pull.

## [0.2.0] - 2026-05-29

### Added
- `sdi bypass arm | disarm | status` — emergency hook-bypass marker is now a
  first-class CLI verb. The previous on-disk surface (`touch
  ~/.cache/sdi/bypass-once`) was self-deadlocking: the only way for the main
  session to arm the marker was a mutating Bash call, which D21 already
  blocked. `sdi` is on the D21 read-only Bash whitelist, so the new
  subcommand is the substrate the main session can reach without delegation.
  `arm` accepts `--reason "<text>"` (recorded in the marker body and the
  hook's audit log) and `--ttl <seconds>` (default 60s — expired markers are
  cleaned up but do NOT open the gate). `status` reports `armed | expired |
  absent` with TTL remainder; `disarm` removes the marker idempotently. The
  marker body shape is now `{reason, armed_at, expires_at, ttl_seconds}`;
  legacy plain-text bodies from v0.1.4 stay readable for backward
  compatibility.

### Changed
- One armed bypass marker now unlocks every mutating PreToolUse gate (D21
  delegation, active-task, D29 claim overlap) in a single invocation.
  Previously each gate had its own override surface — clearing one only to
  hit the next was a fresh self-deadlock with extra steps. The marker is
  still consumed exactly once per PreToolUse call regardless of how many
  gates would have blocked, and each gate emits its own audit event
  (`pre_tool_use_delegation_bypass`, `pre_tool_use_active_task_bypass`,
  `pre_tool_use_claim_bypass`).
- Hook deny messages across D21, active-task, and D29 now point at `sdi
  bypass arm --reason "<short reason>"` as the recommended override surface.
  `SDI_DELEGATION_BYPASS=1` and `SDI_BYPASS_HOOKS=1` remain as startup-time
  env switches for shell-rc exports, but the deny messages no longer
  advertise the `touch` substrate the main session can't reach.

### Fixed
- D21 emergency-bypass self-deadlock. The hook recommended `touch
  ~/.cache/sdi/bypass-once` to arm the override, but that command is itself
  mutating Bash — D21 blocked it from the main session, and the delegation
  required to clear the gate was the same delegation the override was
  supposed to bypass. The new `sdi bypass arm` verb breaks the loop: `sdi`
  itself is unconditionally allowed by the D21 read-only Bash whitelist.

## [0.1.4] - 2026-05-29

### Fixed
- D21 PreToolUse hook wrongly blocked every plugin-namespaced specialist
  (`sdi:impl-coder`, `sdi:test-runner`, …) with a `rogue-specialist` error. The
  registered AgentSpec set stores bare names (frontmatter `name:`) while Claude
  Code dispatches namespaced types; the comparison now normalizes the dispatched
  value before lookup. Activity feed records the canonical bare name on
  SubagentStart / SubagentStop so `/activity` rows group correctly.
- `POST /decisions` and `POST /decisions/:id/rollback` accepted any string for
  `agent_name`, letting namespace-prefixed or otherwise unregistered identifiers
  leak into the append-only Decision log. Both endpoints now reject anything
  outside `STOCK_AGENTS ∪ STOCK_META_AGENTS` at the gate.

### Added
- One-shot D21 delegation-bypass marker file at
  `~/.cache/sdi/bypass-once`. Inline `SDI_DELEGATION_BYPASS=1 cmd` never
  reached the hook because Claude Code's hook subprocess does not inherit
  the user shell's per-command env. The marker file is a substrate both
  sides own: presence consumes one main-session block and the hook deletes
  it before honoring the bypass, so the bypass is naturally one-shot and
  audit-symmetric with the env path. Optional reason text written into the
  marker body is recorded in the audit log. The original
  `SDI_DELEGATION_BYPASS=1` env path is preserved for users who export it
  in their shell rc before launching Claude Code.

## [0.1.3] - 2026-05-26

### Fixed
- `sdi dashboard` (and `sdi summary`) reported a `task_status` histogram counted
  across **all** projects in the shared database, while every sibling count on
  the same payload was project-scoped. In a multi-project install the field
  leaked other projects' task counts. The histogram is now scoped to the matched
  project by joining `tasks → rounds → plans`; the server-wide `/metrics` gauge
  keeps its global count.

### Added
- `sdi task stats` now selects a project the same three ways every other
  project-scoped command does — a positional id/key, `--project <id|key>`, or
  `--cwd <path>` — defaulting to the current directory. It previously returned a
  global, cross-project histogram with no way to scope it.

### Changed
- Project selection is unified across `dashboard`, `summary`, `handoff`,
  `board`, `wiki`, `timeline`, and `task stats` via a shared selector: a
  positional id/key, `--project`, or `--cwd` (mutually exclusive), defaulting to
  the current directory. `handoff`, `board`, `wiki`, and `timeline` previously
  required an explicit project id.

## [0.1.2] - 2026-05-25

### Fixed
- `sdi daemon start` failed with `spawn sdid: No such file or directory` on
  marketplace installs. The daemon-binary resolver only looked for `sdid` as a
  sibling of `sdi`, but the distribution layout splits them across directories
  (`<root>/bin/sdi` vs `<root>/daemon/bin/sdid`) and the resolver ignored the
  `SDI_DAEMON_BIN` hint the install gate already sets. Resolution now honors
  `SDI_DAEMON_BIN` first, then the sibling (dev/workspace) path, then the
  distribution `daemon/bin/sdid` path, then `PATH` — keeping the CLI and the
  Node install-gate resolvers in lock-step.

## [0.1.1] - 2026-05-25

### Added
- `sdi usage rollup <PLAN_ID>` — aggregated per-plan usage rollup. The previous
  spelling `sdi usage plan` is retained as a backward-compatible alias.

### Changed
- CLI `--help` text overhauled across all 37 subcommands: filled in the
  previously blank command and positional-argument descriptions, removed
  internal vocabulary (design-decision IDs, PRD section refs, internal layer
  acronyms, implementation comments) from user-facing output, expanded
  Given/When/Then on first use, and standardized option-enum and ID phrasing.
- Global `--format` / `--quiet` descriptions shortened.
- `--cwd` and `--project` are now mutually exclusive on `dashboard` and
  `summary` (clap rejects the combination instead of silently picking one).
- `metrics`, `doctor`, and `config` now state that `--format` is ignored.

### Fixed
- Workspace version manifest desync: `Cargo.toml [workspace.package].version`
  was left at `0.0.0` for the `v0.1.0` tag while `plugin.json` carried `0.1.0`.
  Both manifests are now `0.1.1` and move in lock-step.

## [0.1.0] - 2026-05-24

Initial public release. Built `sdi` + `sdid` binaries (macOS + Linux ×
x86_64 + aarch64) attached to the GitHub Release; the `dist` branch carries the
plugin shell + binaries that Claude Code's marketplace pulls from.

[0.3.0]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.3.0
[0.2.1]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.2.1
[0.2.0]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.2.0
[0.1.4]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.4
[0.1.3]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.3
[0.1.2]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.2
[0.1.1]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.1
[0.1.0]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.0
