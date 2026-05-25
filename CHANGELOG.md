# Changelog

All notable user-visible changes to the SDI plugin (CLI + daemon + plugin shell)
are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/),
and the project adheres to [Semantic Versioning](https://semver.org/).

Scope: commands, hooks, MCP read tools, and breaking wire-shape changes. The
workspace `[workspace.package].version` is the single source of truth and is
mirrored by the plugin manifest (`plugin/.claude-plugin/plugin.json`).

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

[0.1.1]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.1
[0.1.0]: https://github.com/scenario-driven/sdi-plugin/releases/tag/v0.1.0
