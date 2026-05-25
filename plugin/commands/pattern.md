---
description: Manage CollaborationPattern (D22). Sub: create/list/show/transition/abort/tree.
argument-hint: create --plan <ID> --kind <K> --applies-to <E> --scope-id <ID> [--steps-json …] [--reviewers-json …] [--fan-out-json …] [--peers-json …] [--parent <ID>] | list --plan <ID> [--active] [--kind <K>] | show <ID> | transition <ID> --to <LIFECYCLE> [--reason …] | abort <ID> [--reason …] | tree --plan <ID>
allowed-tools: Bash, Read
---

# /pattern — CollaborationPattern lifecycle (D22 / D26 / D27)

CollaborationPattern is the **seventh first-class entity** (D22). Every work
entity (`plan` / `requirement` / `scenario` / `task` / `decision` / `round`)
carries `produced_via_pattern_id` so the daemon can reason about *how* a
decision was made, not just *what* was decided. `direct` is the explicit
solo-flow marker (D23 anti-pattern badge), **not** an escape hatch.

## Kinds and their D26 shape gates

| Kind | Minimum shape (daemon enforces at `pending → active`) | Default autonomy (D25) |
|---|---|---|
| `workflow` | `steps.len() ≥ 2` | L5 |
| `graph` | distinct `(reviewer.name, reviewer.stance)` tuples `≥ 2` — sybil-blocked | L5 |
| `swarm` | `fan_out.len() ≥ 2` | L4 |
| `agents-as-tools` | `peer_registration.len() ≥ 1` | L4 |
| `direct` | (no shape requirement) | L3 forced |

## Lifecycle FSM

```
pending ── (D26 shape valid) ──▶ active ──▶ converged | dissensus | aborted
```

Terminal states (`converged` / `dissensus` / `aborted`) stamp `decided_at`
automatically and reject further transitions.

## Create a pattern

```bash
# Workflow (sequential steps; ≥2 required).
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-1 \
  --kind workflow --applies-to plan --scope-id <PLAN-ID> \
  --steps-json '[{"idx":0,"agent":"impl-coder","action":"draft"},
                 {"idx":1,"agent":"test-runner","action":"verify"}]'

# Graph (distinct (name, stance) tuples ≥ 2).
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-2 \
  --kind graph --applies-to decision --scope-id <DEC-ID> \
  --reviewers-json '[{"name":"impl-coder","stance":"proposer"},
                     {"name":"schema-architect","stance":"devil_advocate"}]'

# Swarm (fan_out ≥ 2). Useful for parallel specialist execution.
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-3 \
  --kind swarm --applies-to task --scope-id <TASK-ID> \
  --fan-out-json '["impl-coder","impl-coder","test-runner"]'

# Agents-as-tools (peer registration ≥ 1).
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-4 \
  --kind agents-as-tools --applies-to task --scope-id <TASK-ID> \
  --peers-json '[{"caller":"impl-coder","callee":"schema-architect"}]'

# Direct — explicit solo-flow marker. Anti-pattern badge.
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-DIRECT \
  --kind direct --applies-to task --scope-id <TASK-ID>
```

Each shape-bearing flag also accepts `--<field>-from-file <path>` (e.g.
`--steps-from-file ./steps.json`) so large manifests don't have to fit on
one shell line.

## Nest patterns (D24)

```bash
sdi pattern create \
  --plan <PLAN-ID> --short-code PAT-2A \
  --kind swarm --applies-to task --scope-id <SUBTASK-ID> \
  --parent <PARENT-PAT-ID> \
  --fan-out-json '["impl-coder","impl-coder"]'
```

`depth` is `parent.depth + 1`. The daemon rejects parents that would push
`depth > AutonomyPolicy.pattern_depth_cap` (default 3) and blocks cycles.

## Transition lifecycle

```bash
# pending → active — D26 shape gate runs here.
sdi pattern transition <PAT-ID> --to active

# Resolve to a terminal state.
sdi pattern transition <PAT-ID> --to converged --reason "consensus reached"
sdi pattern abort <PAT-ID> --reason "scope dropped"
```

## Inspect

```bash
sdi pattern list --plan <PLAN-ID>            # plan view
sdi pattern list --active                    # cross-plan active rows
sdi pattern list --plan <PLAN-ID> --kind swarm   # kind filter (client-side)
sdi pattern show <PAT-ID>                    # full row + manifest
sdi pattern tree --plan <PLAN-ID>            # parent→child indented tree
```

## Failure modes

- `workflow shape gate: steps ≥ 2 required` — submitted a 1-step workflow.
  Add the second step or switch to `direct` (and accept the L3 cap).
- `graph shape gate: distinct (name, stance) tuples ≥ 2 required` — sybil
  attempt (two agents with identical `(name, stance)`). Vary the stance.
- `pattern depth N exceeds cap M` — D24 cap hit; lift via
  `sdi autonomy set <PRJ> --scope pattern_kind --pattern-kind <K> --mode L5`
  if the project policy allows, otherwise rethink the nesting.
- `pattern <ID> is terminal` — converged / dissensus / aborted are absorbing;
  create a new pattern row instead.

## Lineage

D22 promotes patterns from runtime constants to *data*. D23 makes
`produced_via_pattern_id` NOT NULL on every new work entity (auto `direct`
fallback). D24 chains them into a DAG. D26 + D27 close the "fake pattern"
escape hatches at both create and transition. Together they let the daemon
reason about *how* decisions were reached, not just *what* was decided.
