# Releasing

> **Active.** Releases are cut by pushing a `v*.*.*` tag; `.github/workflows/release.yml`
> then builds the four-target binaries, creates the GitHub Release, and force-pushes
> the `dist` branch the marketplace pulls from. `v0.1.0` is the first published release.

## One workspace, one version

SDI is a single Cargo workspace (`crates/cli`, `crates/daemon`, `crates/mcp`,
`crates/core`, `crates/db`) plus the plugin shell at `plugin/` — including the
dashboard SPA at `plugin/web/`. There is no per-component manifest:
`Cargo.toml [workspace.package].version` is the single source of truth for the
CLI + daemon + plugin tag, and the plugin shell (SPA included) ships in
lock-step with the workspace it lives in. The release pipeline builds
`plugin/web/dist` before packaging.

[`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) lives in a
separate repository, rides on the daemon's HTTP contract, and ships on an
independent cadence. It is not tagged together with this workspace and carries
no shared release manifest.

## Release order

When the first tagged release ships, the order is **fixed**:

1. **Workspace release** — `cargo build --release` from the Rust workspace,
   then `gh release create v0.x.0` with `sdi` and `sdid` binaries attached
   (macOS + Linux × x86_64 + aarch64). The same tag is the plugin tag.
2. **Plugin dist branch** — push the built binaries under `plugin/bin/` and
   `plugin/daemon/bin/` to the `dist` branch (see Branch model below).
   Claude Code's marketplace pulls from this branch.
3. **Desktop** (independent) — the separate `sdi-desktop` repository
   releases on its own cadence against this daemon's HTTP contract. No
   shared release manifest with this workspace; coordination is done at
   the wire-shape level only. (The dashboard SPA at `plugin/web/` is part
   of this workspace and ships with the plugin tag, not independently.)

## Branch model

PRD §5.1 specifies two branches in this repo:

- `main` — source. The Rust workspace + plugin shell **source** only. No
  binaries, no built artifacts. `.gitignore` enforces this for
  `plugin/bin/` and `plugin/daemon/bin/`.
- `dist` — distribution. Built `sdi` + `sdid` binaries under
  `plugin/bin/` and `plugin/daemon/bin/`. Tagged. This is the branch
  Claude Code's plugin marketplace pulls from.

The split exists because the plugin cache (`~/.claude/plugins/cache/`) copies
the marketplace plugin tree verbatim — source has no place in the user's
install, and binaries have no place in the source review.

## Tag conventions

- Workspace (CLI + daemon + plugin shell): `v<major>.<minor>.<patch>`
  (e.g., `v0.1.0`). Tagged from `main`, attached to a GitHub Release with
  both binaries; the `dist` branch carries the same tag with the built
  artifacts checked in.
- `sdi-desktop` (separate repo): its own tags, its own cadence. Independent.

## Pre-release checklist

Before tagging any release:

- [ ] `cargo test --workspace` green on macOS + Linux × x86_64 + aarch64.
- [ ] `node --test "plugin/tests/*.test.cjs"` green.
- [ ] `cargo build --release --workspace` produces working `sdi` and `sdid`.
- [ ] `sdi doctor` passes on a fresh `SDI_HOME=$(mktemp -d)` shell.
- [ ] PRD §6 acceptance criteria 1–10 verified against built artifacts.
- [ ] `~/.local/share/sdi/` survives a `/plugin uninstall` + `/plugin install`
      cycle (LM-8 fixture).
- [ ] CHANGELOG entry covers the user-visible surface only (commands, hooks,
      MCP read tools, breaking wire-shape changes).

## What is NOT released

- The wrapper directory (`scenario-driven/`) — coordinates only, not a published artifact.
- `target/` — never committed, never released; build output only.
- `~/.local/share/sdi/` — user data, never bundled. Survives all
  install/uninstall cycles by LM-8 design.
- `crates/cli/tests/`, `plugin/tests/` — test fixtures, not user-facing.

## Yanking

If a release ships a wire-shape break that violates SemVer (the daemon refuses
an old CLI, or vice versa — should be impossible since they share a workspace,
but the case is still recorded for symmetry), the corrective action is:

1. Tag the next patch release that restores the contract.
2. Push the corrected binaries to the `dist` branch under the new tag.
3. Mark the broken release on GitHub as "pre-release" so the marketplace
   stops advertising it.

No `git tag -d` on a public tag. Once published, a tag is permanent.
