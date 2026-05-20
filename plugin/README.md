# SDI — Claude Code plugin shell

This directory is the **Claude Code plugin surface** for SDI (Scenario-Driven
Implementation).

It is part of the same repository as the SDI body (`crates/` workspace: cli +
daemon + mcp + core + db). The plugin is not a separate package — it is one of
the faces of this repository.

## Contents

| Path | Role |
|---|---|
| `.claude-plugin/plugin.json` | Plugin manifest. Declares `commands/`, the bundled `sdi` skill, and the marketplace metadata. |
| `.mcp.json` | MCP server registration. Spawns `sdi mcp` (the CLI's stdio MCP subcommand). |
| `hooks/hooks.json` | Hook routing for SessionStart, UserPromptSubmit, PreToolUse, PostToolUse, SubagentStart, SubagentStop. |
| `adapters/claude/*.cjs` | Thin Claude-Code-specific wrappers that delegate to `shared/sdi-hooks.cjs`. |
| `adapters/shared/sdi-hooks.cjs` | Single source of install logic + hook bodies (idempotent `ensureInstalled`, daemon spawn, active-task guards). |
| `commands/*.md` | D11 slash commands: `/plan`, `/scenario`, `/round`, `/req`, `/decide`, `/sdi-status`. |
| `skills/sdi/SKILL.md` | The `sdi` skill — D1–D12 workflow + failure-mode catalog. |
| `scripts/setup.cjs` | Manual / CI entry into `ensureInstalled` (same code path as SessionStart). |
| `bin/`, `daemon/bin/` | Install targets for the bundled `sdi` and `sdid` binaries (populated by `ensureInstalled` when the release-bundle layout is in use). |

## Install gate

`adapters/shared/sdi-hooks.cjs::ensureInstalled` is the **single** install
entry. SessionStart invokes it; `scripts/setup.cjs` delegates to it for
manual / CI flows. The gate is idempotent: when both binaries resolve, the
skill files check out, and the daemon's `/health` responds, it returns
immediately. SDI ships as one workspace, so cli/daemon versioning is
governed by `Cargo.toml [workspace.package].version` — there is no
per-component manifest.

## LM-8 invariant

User data resolves under XDG paths (`~/.local/share/sdi/`, `~/.cache/sdi/`,
`~/.config/sdi/`, `~/.local/state/sdi/`) — **never** under `~/.claude/plugins/`.
The daemon enforces this at startup; `sdi doctor` reports overlap as a fatal
error. The plugin gate only writes binaries / bundles under `pluginRoot`, never
user state.

## Pointer to overall identity

See repository root `README.md` and `CLAUDE.md` for what SDI is and the D1–D12
invariants that shape every entity in this surface.
