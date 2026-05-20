# SDI — Scenario-Driven Implementation

> Natural-language GWT scenarios as first-class citizens. LLM implements, verifies, and auto-regresses across rounds.

**Status**: pre-alpha skeleton. Repo just bootstrapped. No working features yet.

---

## What is this?

SDI is the LLM-era successor to TDD (1990s) and BDD (2000s). The lineage:

| | spec form | verifier | who reads it |
|---|---|---|---|
| TDD | test code | the test runner | humans + the runner |
| BDD | Gherkin DSL | step definitions + runner | humans, with step glue maintained by humans |
| **SDI** | **natural language Given/When/Then** | **LLM agent** | **the LLM directly — no compilation step** |

The unit of work is the **scenario**. A plan locks in a set of scenarios. The LLM decomposes runtime tasks, implements them, and verifies each scenario passes. The next round auto-replays prior scenarios as regression.

Identity & design rationale: see **`../clawket/plans/scenario-engine-proposal.md`** (prose, general audience) and **`../clawket/plans/scenario-engine-prd.md`** (PRD, engineering audience). Those documents are the source of truth until they are migrated into this repository under `docs/`.

---

## Repository shape

This is a **Claude Code plugin whose body is a Rust workspace**. The plugin shell, the cli, and the daemon are all surfaces of the same repository.

```
sdi-plugin/
├── Cargo.toml               # workspace root (resolver = 2)
├── crates/
│   ├── cli/                 # `sdi` binary — user/LLM entry point. Hosts `sdi mcp` subcommand.
│   ├── daemon/              # `sdid` binary — background daemon (HTTP + unix socket).
│   ├── mcp/                 # stdio MCP server library, embedded into cli.
│   ├── core/                # Domain model: Plan / Requirement / Decision / Scenario / Round / Task.
│   └── db/                  # SQLite (rusqlite + sqlite-vec) storage adapter.
├── plugin/                  # Claude Code plugin shell
│   ├── .claude-plugin/plugin.json
│   ├── .mcp.json
│   ├── hooks/hooks.json
│   └── README.md
├── README.md                # this file
├── CLAUDE.md                # AI context for contributors / agents
├── LICENSE                  # MIT
└── .gitignore
```

Add-on repositories (separate org repos):

- [`sdi-web`](https://github.com/scenario-driven/sdi-web) — Vite/React dashboard SPA. Consumes the `sdid` HTTP API + `/events` SSE.
- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — Tauri 2 shell. Bundles `sdi-web/dist` and spawns `sdid` as a sidecar.

---

## Build

```sh
cargo build
```

Builds two binaries: `sdi` (cli) and `sdid` (daemon). Both are skeletons at this point — they print version and exit with code 2 on unimplemented subcommands.

---

## Prior work

SDI is the direct successor to **Clawket v3.0** (operated for roughly one month). Clawket validated that LLMs can carry long-running work state through a local SQLite + daemon + MCP architecture, but its task-centric Jira-lineage model did not enable LLM-driven verification or automatic regression. SDI re-centers on scenarios to close that gap.

Migration mapping is in the PRD §9. SDI is a new tool in a new org, not a Clawket version bump.

---

## License

MIT. See `LICENSE`.
