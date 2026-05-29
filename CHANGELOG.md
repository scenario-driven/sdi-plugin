# Changelog

All notable user-visible changes to the SDI plugin (CLI + daemon + plugin shell)
are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/),
and the project adheres to [Semantic Versioning](https://semver.org/).

Scope: commands, hooks, MCP read tools, and breaking wire-shape changes. The
workspace `[workspace.package].version` is the single source of truth and is
mirrored by the plugin manifest (`plugin/.claude-plugin/plugin.json`).

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

[0.1.3]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.3
[0.1.2]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.2
[0.1.1]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.1
[0.1.0]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.0
