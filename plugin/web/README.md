# plugin/web — SDI dashboard SPA

**English** · [한국어](./README.ko.md)

Dashboard SPA bundled into the `@scenario-driven/sdi-plugin` Claude Code plugin.
One-way consumer of the `sdid` HTTP API + `/events` SSE — surfaces every
first-class entity the daemon owns and the multi-agent governance surface laid
out in PRD D14–D29.

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md).

## What it shows

| View | Purpose |
|---|---|
| **SummaryView** | High-level dashboard: active plan, autonomy mode, active rounds, recent decisions. |
| **BoardView** | Kanban over scenarios + runtime tasks (todo → in_progress → done / cancelled). |
| **BacklogView** | Scenario backlog grouped by plan, with `tags` and `depends_on` DAG hints. |
| **TimelineView** | Append-only Decision feed — `proposal → critique → consensus / dissensus` (M3, D20). Carries `reversal_plan` + `blast_radius_score` chips (D28). |
| **WikiView** | Per-plan / per-scenario long-form context. |
| **PatternsView** *(v0.5)* | Active CollaborationPattern rows with kind badge, lifecycle, depth, fan-out, and `direct`-pattern anti-marker (D22 / D23 / D27). |
| **AutonomyPanel** | Per-scope `AutonomyPolicy` rows (L3 / L4 / L5 chips, `external_surface` / `forced` flags, `l5_threshold`, `pattern_depth_cap`, `plan_single_session_lock`). **Circuit breaker** action demotes every row to L3 in one click (D18). |
| **AgentNotesPanel** | Live M1 blackboard / M2 hand-off inbox over SSE — append observation / hypothesis / question / handoff / dissent / evidence; retire (soft) without losing audit trail. |

Everything renders off the daemon's resolved state — no client-side autonomy
decisions, no back-channel. Multi-agent diversity is rendered through
`AgentSpec.stance` (`proposer / devil_advocate / schema_guardian /
performance_reviewer / security_reviewer / neutral`) so the D26 sybil-fix is
visible end-to-end.

## Stack

Vite 6 · React 19 · Tailwind 4 · TypeScript 5.7 · react-router-dom 7 · @dnd-kit.

## How it is served

`sdid` serves `plugin/web/dist/` over the daemon's HTTP listener (tower-http
`ServeDir`). The plugin's `session-start` hook checks for the built bundle on
first install and prompts the user to opt in; declining keeps the dashboard
disabled with zero impact on the CLI / MCP surface. See `SNAPSHOT.json` for the
upstream commit this tree was snapshot-imported from.

## Develop

```sh
pnpm install
pnpm dev          # vite dev server (default port from vite.config.ts)
pnpm build        # → ./dist (consumed by sdid)
pnpm typecheck
pnpm lint
```

`pnpm dev` assumes `sdid` is reachable — either run `sdi daemon start` in a
second terminal or let the Claude Code session-start hook spawn it.

## License

MIT.
