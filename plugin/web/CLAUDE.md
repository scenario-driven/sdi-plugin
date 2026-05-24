# CLAUDE.md — plugin/web

Self-contained AI context for the dashboard SPA that lives inside the
`@scenario-driven/sdi-plugin` repository under `plugin/web/`. The parent
repository is `sdi-plugin`; this directory is part of that repo, not its own.

## Identity

The **SDI dashboard SPA**. Vite 6 + React 19 + Tailwind 4. Five views (Summary
/ Board / Backlog / Timeline / Wiki) plus the v0.5 governance panels (Patterns,
Autonomy, AgentNotes). **One-way dependency**: the SPA fetches from the local
`sdid` HTTP API + `/events` SSE. No write paths bypass the daemon.

## Position in sdi-plugin

```
sdi-plugin/
├── crates/        # Rust workspace (cli, daemon, mcp, core, db)
├── plugin/
│   ├── adapters/  # Claude Code hook shims + shared module
│   ├── agents/    # AgentSpec frontmatter files
│   ├── commands/  # /scenario, /round, /plan, /req, /decide, /pattern
│   ├── skills/    # bundled skills (sdi-overview, sdi-scenario, ...)
│   ├── web/       # ← this directory (Vite SPA)
│   └── daemon/    # daemon launcher bits (release-built binaries land here)
└── docs/          # PRD + ARCHITECTURE + RELEASING
```

`plugin/web/dist/` is the build artifact. It is `.gitignore`d at the
sdi-plugin root and rebuilt by `pnpm --dir plugin/web build`. The release
pipeline (`.github/workflows/release.yml`) runs that command before packaging.

## Source of truth for the API surface

The daemon router under `../crates/daemon/src/router/` is the canonical
endpoint inventory. This SPA's code is allowed to assume only what that router
exposes. If a response schema changes there, fix the SPA fetcher to match —
never patch around the daemon.

## Conventions

- TypeScript strict. ESLint with `--max-warnings 0`.
- Components live under `src/components/`, views under `src/views/`.
- Build output goes to `./dist` and is served by `sdid` via tower-http
  `ServeDir`.
- No backend code, no Rust. Those live in `../../crates/`.

## Provenance

`SNAPSHOT.json` records the upstream sdi-web commit this tree was imported
from. Per the absorption decision (U2 of the SDI-144 plan), `plugin/web/` is
the only source of truth going forward; the upstream `scenario-driven/sdi-web`
repository is archived (read-only).

## Verification before claiming complete

```sh
pnpm --dir plugin/web install
pnpm --dir plugin/web typecheck
pnpm --dir plugin/web lint
pnpm --dir plugin/web build
```

All four must pass. State explicitly if any are skipped.

## Commit & release

- Agents do not commit or push without explicit instruction.
- Conventional Commits style, scoped `feat(web): ...` / `fix(web): ...`.
