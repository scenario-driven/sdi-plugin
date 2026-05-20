# Compatibility

SDI ships as one Cargo workspace whose plugin shell, CLI, and daemon move in
lock-step. The web and desktop surfaces are independent crates that ride on
the daemon's HTTP contract and ship on their own cadences.

## Workspace version is the only pin

`Cargo.toml [workspace.package].version` is the single source of truth.
The `sdi` CLI, `sdid` daemon, and `plugin/` shell all carry that one
version — there is no per-component manifest to drift from it. The install
gate (`adapters/shared/sdi-hooks.cjs::ensureInstalled`) resolves binaries
by location only (workspace `target/`, `<pluginRoot>/bin/`, then PATH), not
by tag, because there is nothing to disagree.

The release-fetch path (`SDI_RELEASE_FETCH=1`) is structurally present in
the install gate but errors out until a GitHub Release exists — distribution
is excluded from current scope.

## Wire-shape contract (sdi-web / sdi-desktop)

The separate [`sdi-web`](https://github.com/scenario-driven/sdi-web) and
[`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) repositories couple
to this daemon **only** through:

- HTTP routes: `/plans`, `/requirements`, `/decisions`, `/scenarios`,
  `/rounds`, `/tasks`, `/knowledge/*`, `/health`.
- SSE stream on `/events` carrying `scenario.*`, `round.*`, `plan.*`,
  `knowledge.*` event shapes.

Both add-on repositories ship independently. There is no shared release
manifest with this workspace, and they are not version-pinned against
each other or against the CLI/daemon. The daemon's versioned API is the
only coupling point — any break to the wire shape on a route consumed by
`sdi-web` is a workspace major bump here, but `sdi-web` / `sdi-desktop`
tags themselves move on their own cadence in their own repos.

## Breaking-change gates

Any change to one of the following triggers a workspace **major** bump:

- The wire shape on any daemon HTTP route consumed by the CLI, web, or MCP.
- The SSE event names or payload shapes on `/events`.
- The hook routing in `plugin/hooks/hooks.json` (event names, matchers, shim
  paths).
- The `${CLAUDE_PLUGIN_ROOT}/bin/` precedence or `SDI_BIN` semantics.
- The XDG path mapping (LM-8 invariant guarantees).
- The `SDI_SKILLS` array — adding/removing a skill changes the surface
  Claude Code advertises and the plugin manifest must move in lock-step
  (lint test enforces three-way sync).

## Verification

The plugin lint test
(`plugin/tests/lint.test.cjs::SDI_SKILLS array, plugin.json#skillsList, and
skills/ dirs are in lock-step`) asserts the three sources of truth for skills
never diverge.

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
