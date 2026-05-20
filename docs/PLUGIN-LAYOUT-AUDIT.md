# Plugin Layout Audit — v0.1 baseline

Evidence record that the SDI plugin shell conforms to **(a)** the canonical
Claude Code plugin spec, **(b)** PRD §5.2, and **(c)** the only deployed market
precedent that bundles cli + daemon + mcp as compiled binaries (Clawket).

Closure of the v0.1 baseline depends on this artifact; new layout changes must
re-check this matrix before merging.

## Sources

| Source                                  | Role                                                            |
| ---                                     | ---                                                             |
| `code.claude.com/docs/en/plugins-reference` | Canonical spec — file-locations reference, schema, hooks list   |
| `anthropics/claude-code/plugins/plugin-dev` | Anthropic-shipped reference plugin — skills/commands/agents only |
| `cased/claude-code-plugins`             | Third-party marketplace — skills+commands+hooks+.mcp.json       |
| `2389-research/claude-plugins`          | 28-plugin marketplace catalog                                   |
| `clawket/clawket` (local checkout)      | Direct precedent — bundled cli + daemon + web binaries          |

The market reality is that **Clawket is the only published precedent** that
bundles compiled binaries (cli + daemon) plus a web SPA inside a single Claude
Code plugin shell. Other surveyed plugins ship skills + commands + bash scripts
only. Spec authority therefore comes from the official reference, while
multi-binary shape authority comes from Clawket.

## Comparison matrix

Rows = layout dimensions specified by the canonical reference. `✓` = present
and conformant; `—` = N/A for that exemplar.

| Dimension                          | Canonical default            | Clawket                                    | Anthropic plugin-dev      | cased marketplace          | **SDI**                                       |
| ---                                | ---                          | ---                                        | ---                       | ---                        | ---                                           |
| Plugin manifest                    | `.claude-plugin/plugin.json` | `./.claude-plugin/plugin.json`             | `./.claude-plugin/plugin.json` | `./.claude-plugin/plugin.json` | `./plugin/.claude-plugin/plugin.json` ✓       |
| Skills                             | `skills/`                    | `./skills/{clawket,pdd,…}/SKILL.md`        | `./skills/<7 dirs>/SKILL.md` | `./skills/`                | `./plugin/skills/sdi/SKILL.md` ✓              |
| Commands                           | `commands/`                  | — (uses prompts/, skills only)             | `./commands/`             | `./commands/`              | `./plugin/commands/{scenario,round,plan,req,decide,sdi-status}.md` ✓ |
| Hooks routing                      | `hooks/hooks.json`           | `./hooks/hooks.json`                       | — (no hooks)              | `./hooks/`                 | `./plugin/hooks/hooks.json` ✓                 |
| MCP wiring                         | `.mcp.json`                  | `./.mcp.json` → `clawket mcp`              | — (no MCP)                | `./.mcp.json`              | `./plugin/.mcp.json` → `sdi mcp` ✓            |
| Bundled executables                | `bin/`                       | `./bin/clawket` + `./daemon/bin/clawketd`  | — (no binaries)           | — (no binaries)            | `./plugin/bin/sdi` + `./plugin/daemon/bin/sdid` ✓ (populated by install gate at runtime) |
| Hook bodies (non-spec)             | n/a (spec leaves this open)  | `./adapters/{claude,shared}/*.cjs`         | n/a                       | n/a                        | `./plugin/adapters/{claude,shared}/*.cjs` ✓ (same shape as Clawket) |
| Install gate                       | n/a (spec leaves this open)  | `./adapters/shared/claude-hooks.cjs::ensureInstalled` invoked from `SessionStart` shim | n/a | n/a | `./plugin/adapters/shared/sdi-hooks.cjs::ensureInstalled` invoked from `SessionStart` shim ✓ |
| Manual setup shim                  | n/a                          | `./scripts/setup.cjs`                      | n/a                       | n/a                        | `./plugin/scripts/setup.cjs` ✓                |
| Plugin-side tests                  | n/a                          | `./tests/*.test.cjs` (node --test)         | n/a                       | n/a                        | `./plugin/tests/{lint,hooks,e2e}.test.cjs` ✓  |
| Component pins                     | n/a                          | `./components.json` (cli/daemon/web/desktop) | n/a                     | n/a                        | — (single Cargo workspace; `Cargo.toml [workspace.package].version` is the single SoT) |
| Compiled-source workspace          | n/a (spec is plugin-shell only) | Split across 7 git repos                | n/a                       | n/a                        | `./crates/{cli,daemon,mcp,core,db}` colocated with `./plugin/` ✓ (PRD §5.1 consolidation) |

## Where SDI matches the canonical spec exactly

- `plugin/.claude-plugin/plugin.json` ✓ — required manifest at the canonical
  location with `name`, `version`, `description`, `author`, `license`.
- `plugin/skills/sdi/SKILL.md` ✓ — `<name>/SKILL.md` shape per spec.
- `plugin/commands/*.md` ✓ — flat markdown files per spec (skills-as-commands
  fallback form).
- `plugin/hooks/hooks.json` ✓ — `hooks/hooks.json` routing manifest per spec.
  Matchers (`Edit|Write|MultiEdit|Bash|NotebookEdit` for `PreToolUse`) match the
  documented event-and-matcher form.
- `plugin/.mcp.json` ✓ — `.mcp.json` MCP wiring per spec; the `command` value
  resolves through the `sdi` binary on PATH (after install gate) and dispatches
  to the embedded MCP server via `sdi mcp`.
- `plugin/bin/` and `plugin/daemon/bin/` ✓ — spec's "Executables added to the
  Bash tool's PATH" location. Populated by `ensureInstalled` at runtime, not
  checked in (Clawket precedent).

## Where SDI extends beyond the spec

The canonical reference is silent on **how** hook bodies are organized and
**how** a plugin bootstraps bundled binaries. SDI inherits Clawket's
battle-tested pattern verbatim:

1. **Thin-shim hooks** — `plugin/adapters/claude/<event>.cjs` is ≤8 LOC, wraps
   the shared handler with `.catch(…)` and exits 0 on failure. Hook crash
   safety becomes a structural property of the shim layer rather than a runtime
   choice.
2. **Single install gate** — `plugin/adapters/shared/sdi-hooks.cjs::ensureInstalled`
   is the only code path that writes `bin/sdi` and `daemon/bin/sdid`. It is
   idempotent (re-runs are no-ops once both binaries resolve, the skill
   files check out, and the daemon's `/health` responds), and is invoked
   exclusively from `session-start.cjs` plus `scripts/setup.cjs` (the CI /
   manual entry point). Both call the same function — there is no parallel
   install logic to drift.

These extensions are unambiguous: they live alongside the canonical surfaces,
not in place of them. A user with no knowledge of Clawket could still install
SDI via the standard `/plugin install` flow, and the canonical spec keeps
working.

## Where SDI diverges from Clawket — and why

| Divergence                               | Justification                                                                                                                                                                                                                                              |
| ---                                      | ---                                                                                                                                                                                                                                                        |
| **One repo containing `crates/` + `plugin/`** vs. Clawket's 7 separate repos | PRD §5.1 fixes single-repo as a hard requirement. The Claude Code plugin cache traps `<pluginRoot>` as the only stable path post-install — any sibling repo (`../crates`) becomes unreachable. Single-repo, two-branch (`main` / `dist`) keeps the source/binary correspondence atomic and reviewable. (See `docs/ARCHITECTURE.md`.) |
| **No top-level locale tree, no `prompts/`** | PRD §5.4 v0.1 surface scope: 5 slash commands (`/scenario`, `/round`, `/plan`, `/req`, `/decide`) + `/sdi-status` + `/sdi` skill. i18n + prompt library are not in v0.1 — adding them would violate snapshot-only scope discipline. |
| **6 hook events vs. Clawket's 7 (no `ExitPlanMode`)** | Clawket's `plan-sync.cjs` writes plan markdown to disk on ExitPlanMode. SDI's plan storage is the daemon DB (`crates/db`) via `sdi plan create/approve`, not files. The PRD does not require an ExitPlanMode hook; adding one without a use case would be speculative scaffolding. |
| **No `agents/` directory**               | SDI v0.1 has no plugin-shipped subagents. PRD §5.4 does not include subagent vocabulary in the v0.1 surface. Spec auto-discovery from `agents/` would have nothing to find, so the directory is omitted (matches cased/`piglet`, `kit-cli` precedent). |
| **`plugin.json` has no `commands` field** | The canonical schema treats `commands` as an array of explicit file paths used to override the default location. Since SDI's commands live in the default `commands/` directory, auto-discovery is the correct form — declaring `"commands": "./commands/"` was a misread of the schema (where `commands` expects `string[]`, not `string`). Removed during the audit. |
| **No `components.json`** | Clawket carries `components.json` because its cli / daemon / web / desktop live in 7 separate repos and need explicit version pins. SDI is a single Cargo workspace, so `Cargo.toml [workspace.package].version` already pins cli + daemon + plugin shell in lock-step; the separate `sdi-web` and `sdi-desktop` repositories ride the daemon HTTP contract and ship on their own cadences, with no shared release manifest. A second pinning surface would only invite drift. |

## v0.1 baseline closure

All of the following are satisfied as of the audit:

1. **Layout matches PRD §5.2** — every path in the §5.2 listing has a
   corresponding file or directory at the expected location (verified line by
   line against `docs/ARCHITECTURE.md` §Layout).
2. **Layout matches canonical Claude Code plugin spec** — every required
   canonical surface (`plugin.json`, `skills/`, `commands/`, `hooks/hooks.json`,
   `.mcp.json`, `bin/`) is present in the form documented at
   `code.claude.com/docs/en/plugins-reference`.
3. **Layout matches the only deployed multi-binary precedent (Clawket)** —
   `bin/` + `daemon/bin/` + `adapters/{claude,shared}/` + `scripts/setup.cjs`
   + `tests/*.test.cjs` mirror Clawket's tree, with the PRD §5.1 single-repo
   consolidation as the deliberate divergence (which also removes Clawket's
   `components.json` — a single Cargo workspace replaces multi-repo pinning).
4. **Surface contract is enforced by tests, not promises** —
   `crates/cli/tests/slash_command_contract.rs` walks every `sdi …` invocation
   in `plugin/commands/*.md` + `plugin/skills/sdi/SKILL.md` and parses it
   through the real clap App. Any future doc drift fails the test.
5. **Test suite is fully green** — `cargo test --workspace` reports 83 passed
   / 0 failed across 20 binaries/integration suites; `cargo clippy --workspace
   --tests -- -D warnings` is clean; `node --test 'plugin/tests/*.test.cjs'`
   reports 25 passed / 0 failed.

### v0.1 baseline is closed.

Future moves (web add-on, desktop add-on, GitHub Release distribution) are
scoped out of v0.1 by PRD §5.1 and §5.4. They can land without touching this
matrix — but any change that *does* touch a row above must update this audit
in the same diff.
