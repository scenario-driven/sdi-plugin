# Compatibility

SDI ships as one Cargo workspace whose plugin shell, CLI, daemon, and dashboard
SPA (`plugin/web/`) move in lock-step. The desktop surface is an independent
repository that rides on the daemon's HTTP contract and ships on its own cadence.

## Workspace version is the only pin

`Cargo.toml [workspace.package].version` is the single source of truth.
The `sdi` CLI, `sdid` daemon, and `plugin/` shell all carry that one
version. The Claude Code and Codex plugin manifests must keep that version in
lock-step; they are host descriptors for the same artifact, not component
versions. The install gate and MCP launcher
(`adapters/shared/sdi-hooks.cjs::ensureInstalled`,
`plugin/scripts/sdi-mcp.cjs`) resolve binaries by location only (workspace
`target/`, `<pluginRoot>/bin/`, then PATH), not by tag, because there is
nothing to disagree.

The release-fetch path (`SDI_RELEASE_FETCH=1`) is structurally present in
the install gate but errors out until a GitHub Release exists — distribution
is excluded from current scope.

## Wire-shape contract (sdi-desktop)

The dashboard SPA at `plugin/web/` ships inside this workspace, but it still
couples to the daemon **only** through the wire shape below — there is no
compile-time link to the Rust crates. The separate
[`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) repository
couples through the same contract:

- HTTP routes: `/plans`, `/requirements`, `/decisions`, `/scenarios`,
  `/rounds`, `/tasks`, `/knowledge/*`, `/health`.
- SSE stream on `/events` carrying `scenario.*`, `round.*`, `plan.*`,
  `knowledge.*` event shapes.

`sdi-desktop` ships independently: no shared release manifest with this
workspace, not version-pinned against the CLI/daemon. The daemon's versioned
API is the only coupling point — any break to the wire shape on a consumed
route is a workspace major bump here, while `sdi-desktop` tags move on their
own cadence in their own repo.

## Breaking-change gates

Any change to one of the following triggers a workspace **major** bump:

- The wire shape on any daemon HTTP route consumed by the CLI, web, or MCP.
- The SSE event names or payload shapes on `/events`.
- The hook routing in `plugin/hooks/hooks.json` (event names, matchers, shim
  paths).
- The MCP launcher path (`${PLUGIN_ROOT}/scripts/sdi-mcp.cjs`,
  `${CLAUDE_PLUGIN_ROOT}/scripts/sdi-mcp.cjs`) or `SDI_BIN` / binary
  resolution semantics.
- The XDG path mapping (LM-8 invariant guarantees).
- The `SDI_SKILLS` array — adding/removing a skill changes the surface local
  agent hosts advertise. Claude Code's `skillsList`, Codex's `skills` pointer,
  and the on-disk `skills/` tree must move in lock-step (lint tests enforce
  this).

## Verification

The plugin lint tests assert that the Claude manifest, Codex manifest, and
on-disk `skills/` tree never diverge.

`sdi doctor` is the user-facing diagnostic:

```
$ sdi doctor
[Workspace]
  sdi      v0.1.0 (target/release/sdi)        ✓
  sdid     v0.1.0 (target/release/sdid)       ✓
[Path separation invariant (LM-8)]
  data     ~/.local/share/sdi      ✓ outside ~/.claude/plugins
  cache    ~/.cache/sdi            ✓
  config   ~/.config/sdi           ✓
  state    ~/.local/state/sdi      ✓
  db       ~/.local/share/sdi/sdi.db ✓
```

In the pre-release state, the binary source resolves to `target/debug` — that
is the expected `sdi doctor` output for local development.
