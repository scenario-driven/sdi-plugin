---
description: Set or inspect the SDI autonomy policy (D14 / D17 / D18 — L3 / L4 / L5)
argument-hint: set --project <PRJ-ID> --scope <SCOPE> --mode <L3|L4|L5> [--plan-id …] [--decision-kind …] | get … | list <PRJ-ID> | circuit-breaker <PRJ-ID> --reason "…"
allowed-tools: Bash, Read
---

# /autonomy — D14 / D17 / D18 policy control

The autonomy policy is the **sixth first-class entity** (D14). It controls
*when SDI asks the user* vs. *when SDI acts on its own*. Three modes:

| Mode | Semantics | Default for |
|---|---|---|
| `L3` | Always ask before applying a decision. Single-agent fallback. | Solo `@main` flows |
| `L4` | Ask only for "important" decision kinds (architecture / schema / naming-canonical). Multi-agent consensus unlocks this. | Plans with external surface |
| `L5` | Apply consensus automatically. | New plans (D17) |

Resolution order is **plan > decision_kind > global** — the most specific
matching policy wins (PRD §3).

## Set a policy

```bash
# Global default for the project.
sdi autonomy set <PRJ-ID> --scope global --mode L5

# Override for one plan.
sdi autonomy set <PRJ-ID> --scope plan --mode L4 --plan-id <PLAN-ID>

# Force a decision kind to ask (D17 forced-L4 kinds: architecture, schema, naming-canonical).
sdi autonomy set <PRJ-ID> --scope decision_kind --mode L4 --decision-kind architecture
```

D17 invariant: `decision_kind` ∈ {architecture, schema, naming-canonical}
**must** be at most L4. L5 on a forced kind is rejected with HTTP 403.

## Inspect

```bash
sdi autonomy get <PRJ-ID> --plan-id <PLAN-ID>          # resolve effective policy
sdi autonomy get <PRJ-ID> --decision-kind architecture # ditto, decision-kind scope
sdi autonomy list <PRJ-ID>                              # every row on the project
```

## Circuit breaker (D18)

The panic switch. Demotes every policy in the project to L3 in one transaction
and emits `circuit_breaker.triggered`:

```bash
sdi autonomy circuit-breaker <PRJ-ID> --reason "panic — bad decision pending"
```

In-flight decisions apply at the **next** consensus gate (already-applied
decisions are not rolled back; that is a Decision supersession).

## When to flip modes

- **New plan, exploratory phase**: leave global L5; you get fast autonomous
  iteration. Forced kinds still gate.
- **Pre-release / production change**: drop global to L4 or set a plan-scoped
  L4. Architecture / schema decisions surface as user prompts.
- **Incident in progress**: hit the circuit breaker. Everything drops to L3.
  Restore individual modes after the situation calms.

## Failure modes

- `FORCED_L4` — tried to set L5 on architecture / schema / naming-canonical.
  Choose L4 or change the decision_kind.
- `MISSING_PLAN_ID` — `--scope plan` requires `--plan-id`.
- `MISSING_DECISION_KIND` — `--scope decision_kind` requires `--decision-kind`.
