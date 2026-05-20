# CLAUDE.md — sdi-plugin

Single-source AI context for **this repository** (`@scenario-driven/sdi-plugin`). This file is self-contained: agents working in a fresh clone of just this repo must be able to operate from this document alone (per the wrapper-level operational rule about sub-repo self-containment).

For wrapper-level coordinates (sibling repos, monorepo position), see `../CLAUDE.md` (the `scenario-driven/` wrapper). Wrapper file is **not** required for working in this repo — only useful if working across sibling repos.

---

## Identity (do not paraphrase, do not soften)

This repository is a **Claude Code plugin whose body is a Rust workspace**. The plugin is not a thin wrapper around a separate Rust project — they are the same artifact, two views of the same source tree.

- The plugin shell lives at `plugin/`.
- The Rust workspace lives at `crates/` with five crates: `cli`, `daemon`, `mcp`, `core`, `db`.
- The `sdi` binary (built from `crates/cli`) hosts the `mcp` subcommand (stdio MCP server). The plugin's `.mcp.json` invokes `sdi mcp`.
- The `sdid` binary (built from `crates/daemon`) is the long-lived daemon holding SQLite state and serving HTTP + unix socket.

Tool identity (one paragraph): **Scenario-Driven Implementation (SDI)**. Natural-language Given/When/Then scenarios are first-class citizens. Plans approve when scenarios are complete; tasks are runtime artifacts the LLM decomposes from scenarios + requirements; rounds (R1, R2, …) auto-replay prior scenarios as regression. Lineage: TDD → BDD → SDI.

Full design spec (until migrated into this repo's `docs/`): `../../clawket/plans/scenario-engine-prd.md`. Reading that PRD is mandatory before non-trivial changes.

---

## Decisions in force (D1–D12)

| # | Decision | Where enforced |
|---|---|---|
| D1 | Tool identity = Scenario-Driven Implementation engine | README, this file |
| D2 | Five first-class entities: Plan / Requirement (snapshot) / Decision (append-only) / Scenario (GWT) / Round | `crates/core/` |
| D3 | Task is a runtime artifact; LLM decomposes, humans do not author tasks directly | daemon API surface |
| D4 | Unit removed (→ scenario tag). Cycle renamed Round with redefined semantics | `crates/core/`, daemon API |
| D5 | GWT format strict: every scenario must have non-empty Given / When / Then. No free-form option | scenario CRUD validation |
| D6 | Round mode default = `strict-regression`. Option: `forward-only` (explicit) | round creation API |
| D7 | New-development mode and regression-verification mode share one engine. R1 = new, R2+ = regression | round implementation |
| D8 | Plan approve gate = scenarios ≥ 1 & all GWT valid; tasks count is irrelevant | plan approve API |
| D9 | Disruption policy default = needs-review (human confirm). `auto` option still requires confirm before applying | scenario/req/decision write paths |
| D10 | In-flight Task on `round start` defaults to pause. Flags: `--abort`, `--continue-on-noimpact` | round start API |
| D11 | Slash commands: `/scenario`, `/round`, `/plan`, `/req`, `/decide`. `/goal` is Claude Code built-in, orthogonal — do not intercept | plugin shell |
| D12 | SNAPSHOT-ONLY documents (no in-body history). Decision artifact is the only history surface | documentation policy |

---

## XDG path invariant (carried from Clawket LM-8)

User data MUST NOT resolve under `~/.claude/plugins/`.

| Area | Path |
|---|---|
| Data (SQLite) | `~/.local/share/sdi/` |
| Cache (socket / pid / port file) | `~/.cache/sdi/` |
| Config | `~/.config/sdi/` |
| State (logs) | `~/.local/state/sdi/` |

Plugin install gate may write `sdi` + `sdid` binaries under `~/.claude/plugins/sdi-*/bin/`, and that is the only place plugin-managed assets may live. SQLite, sockets, logs, config must stay in the XDG paths above. The daemon will enforce this at startup (`sdid` refuses to start if any of the five paths above resolves under `~/.claude/plugins/`), and `sdi doctor` will surface violations with exit code 1. (Both checks will be implemented alongside the daemon's path resolver.)

---

## Repo conventions

- **Single Rust workspace, resolver = 2.** New crates go under `crates/` and are added to `[workspace].members` in the same change.
- **`workspace.package` carries common metadata.** Per-crate `Cargo.toml` uses `version.workspace = true` etc.
- **One binary per binary crate.** `cli` → `sdi`; `daemon` → `sdid`. Library crates expose `src/lib.rs`.
- **Plugin shell is part of the same repo.** Editing plugin manifest, MCP config, or hooks is a normal change in this tree — not a separate repository.
- **Distribution branch.** Strategy not yet locked. Either a `dist` branch carrying built binaries + plugin manifest, or direct consumption from `main` with binaries fetched from GitHub Releases. Decision tracked separately.

---

## Verification before claiming complete

Per mechanical-overrides §4 (FORCED VERIFICATION):

```sh
cargo build               # all crates compile
cargo check --workspace   # type-check
cargo clippy --workspace -- -D warnings   # once clippy is wired
cargo test --workspace    # once tests exist
```

The current skeleton has no tests and no clippy config. State this honestly in any "done" report.

---

## Commit & release

- Claude Code agents do not commit or push without explicit instruction.
- Commit message convention and release ordering will be specified in `docs/RELEASING.md` once that document lands.

---

## What to read next

1. `README.md` — repo overview (this is the public-facing pitch).
2. `../../clawket/plans/scenario-engine-prd.md` — full PRD (canonical until migrated to `docs/` here).
3. `../../clawket/plans/scenario-engine-proposal.md` — non-technical framing for context.
4. `plugin/README.md` — what the plugin shell is and where its install gate plan stands.
