# Architecture

Scenario-Driven Implementation (SDI) is delivered as **one repo = one Claude Code
plugin**. The plugin shell and the Rust workspace are not separate artifacts —
they are two views of the same source tree.

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
  plugin/                       # Claude Code plugin shell
    .claude-plugin/
      plugin.json               # Plugin manifest (skills, commands, hooks)
    .mcp.json                   # MCP server registration (`sdi mcp`)
    hooks/hooks.json            # Hook routing manifest
    commands/                   # /scenario /round /plan /req /decide /sdi-status
    skills/                     # `/sdi-overview` `/sdi-scenario` `/sdi-round`
                                # `/sdi-evidence` — 4 workflow skills (one per
                                # PRD §3 stage). Single-source: plugin.json
                                # #skillsList ⇄ sdi-hooks.cjs::SDI_SKILLS ⇄
                                # tests/lint.test.cjs SDI_SKILLS.
    adapters/
      claude/                   # 6 thin hook shims (≤8 LOC each, .catch + exit 0)
      shared/sdi-hooks.cjs      # Single home for hook bodies + install gate
    bin/                        # Populated by install gate at runtime
    daemon/bin/                 # Same — sdid lives next to sdi
    scripts/setup.cjs           # Manual entry: shim → adapters/shared
    tests/                      # node --test (lint, hooks, e2e)
  docs/                         # This directory
  README.md
  LICENSE
  CLAUDE.md                     # AI-agent context, sub-repo self-contained
```

## Why one repo

PRD §5.1 fixes this. The reasoning, with market evidence:

1. **Claude Code plugin cache traps the plugin dir as the only stable root.**
   The official plugin spec
   ([code.claude.com/docs/en/plugins-reference](https://code.claude.com/docs/en/plugins-reference))
   states marketplace plugins are copied into `~/.claude/plugins/cache/<plugin>/`.
   Path traversal outside that dir (`../crates`) does **not** work post-install.
   Anything the runtime needs must live inside the plugin dir.
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
Hook crash safety is a structural property of the shim layer, not a runtime
choice. See [HOOK_ENFORCEMENT.md](./HOOK_ENFORCEMENT.md) for the enforcement
semantics.

## Surfaces inside this repo

The CLI / daemon / MCP / core / db quintet is the load-bearing surface
and ships under one workspace version (`Cargo.toml [workspace.package].version`).
This is the entirety of `sdi-plugin`. There are no `crates/web` or
`crates/desktop` inside this repository.

Two add-on surfaces live in **separate repositories** under the same
GitHub org and consume the daemon's public HTTP contract:

- **[`sdi-web`](https://github.com/scenario-driven/sdi-web)** — React + Vite SPA.
  Talks only to the daemon over HTTP + SSE; no compile-time coupling to
  the Rust crates here.
- **[`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop)** — Tauri 2 shell.
  Hosts the `sdi-web` build (`../sdi-web/dist` by sibling convention) in a
  native window and spawns `sdid` as a child process via the resolver in
  its `src/daemon.rs` (env / plugin layout / XDG / PATH). The desktop
  binary embeds no daemon code; it is a thin launcher.

Because both surfaces ride on the daemon's stable HTTP contract, they are
not version-pinned against this workspace and have no shared release
manifest with each other. Each ships on its own cadence; the daemon's
versioned API is the only coupling point.
