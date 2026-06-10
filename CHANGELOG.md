# Changelog

All notable user-visible changes to the SDI plugin (CLI + daemon + plugin shell)
are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/),
and the project adheres to [Semantic Versioning](https://semver.org/).

Scope: commands, hooks, MCP read tools, and breaking wire-shape changes. The
workspace `[workspace.package].version` is the single source of truth and is
mirrored by the plugin manifest (`plugin/.claude-plugin/plugin.json`).

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
