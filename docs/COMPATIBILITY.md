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

## Project command exceptions and external orchestration

Claude Code and Codex share the [project command policy](design/command-policy.md).
Projects declare exceptions in `.sdi/settings.json`; personal additions use the
Git-excluded `.sdi/settings.local.json`. The two `permissions.allow` arrays are
combined on each invocation. Native host permission files remain independent.

A matching command passes only the main-session shell delegation restriction.
Users are responsible for the execution and write effects of their allowed
commands. A policy match does not relabel a mutating command as read-only,
grant editor access, authenticate a specialist, or remove active-task, resource,
autonomy, or other applicable gates.

For example, users may explicitly permit Orca guide retrieval, queue consumption,
acknowledgment, message delivery, worker start, or completion reporting. These
operations have different effects: retrieval inspects data, queue consumption
and acknowledgment change delivery state, messaging enqueues data, and worker
start/completion change task lifecycle state. Configure each required command
and argument prefix rather than treating all Orca commands as reads. The policy
contract defines exact and terminal ` *` matching, compound-command boundaries,
project isolation, and invalid-configuration behavior.

### Supported execution procedure

Run a supported Claude Code or Codex host as the external coordinating session.
Configure only the shell commands that this project intends that main session
to execute. The session reads the project contract and uses native specialist
dispatch for implementation and independent verification. Specialists check their
task brief, project, and workspace; recorded SDI runs supply acceptance evidence.
The coordinator may use policy-authorized shell commands to communicate the
result through its external orchestration tool.

Command permission does not establish an external-worker identity protocol.
Copied IDs, environment variables, active-task hints, and declared specialist
types do not authenticate an external worker or bind it to a task. An ordinary
external session retains main-session editing restrictions. Implementation
requires the native specialist identity supplied by the host; external queue or
task identifiers do not replace it. SDI's native delegation boundary remains
consistent with [the external-protocol scope](PRD.md#d15-aws-4패턴-내장-a2a-v1-제외).
Gate disabling, bypasses, and fabricated identities are not integration steps.

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
