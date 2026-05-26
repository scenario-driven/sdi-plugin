# sdi-db

**English** · [한국어](./README.ko.md)

The SDI storage adapter — SQLite schema, connection pool, and per-entity repositories. Part of the `@scenario-driven/sdi-plugin` Rust workspace.

## What it is

`sdi-db` owns the on-disk schema and CRUD. It uses `rusqlite` behind an `r2d2` connection pool, with FTS5 for keyword search; vector search is deferred (PRD §5.2). It maps `rusqlite` / pool errors into `sdi-core`'s `DomainError` so callers never depend on `rusqlite` types.

Only the daemon links this crate at runtime. Downstream crates (cli, mcp) reach state through the daemon's HTTP/socket surface, never SQLite directly.

## Surface

| Module | Role |
|---|---|
| `paths` | XDG path resolution + the LM-8 invariant (`Paths`, `ENV_ALLOW_OVERLAP`, `ENV_HOME_OVERRIDE`). |
| `pool` | `open_pool`, `tx`, `Pool`, `PooledConn`. |
| `schema` | `ensure_schema` — idempotent migration on startup. |
| `repo/*` | One repository per entity (plan / scenario / decision / round / pattern / autonomy_policy / agent_note / agent_spec / task / event / project / …). |

## LM-8 invariant

User data resolves under XDG paths (`~/.local/share/sdi/`, `~/.cache/sdi/`, `~/.config/sdi/`, `~/.local/state/sdi/`) and **never** under `~/.claude/plugins/`. `Paths` enforces this; overlap is a fatal error surfaced by `sdi doctor`.

## Place in the workspace

```
sdi-daemon ──▶ sdi-db (this) ──▶ SQLite file (XDG data dir)
                  └── depends on sdi-core for domain types
```

## Build & verify

```sh
cargo build -p sdi-db
cargo check -p sdi-db
```

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md). Repository overview: [`../../README.md`](../../README.md).
