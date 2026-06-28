---
description: Show M3 4-stage negotiation status for a plan's decisions (D20)
argument-hint: <PLAN-ID> [--proposal-id <DEC-ID>]
allowed-tools: Bash, Read
---

# /consensus — D20 4-stage M3 view

D20 says a **decision** moves through four stages:

1. `proposal`  — first speaker writes the claim
2. `critique`  — at least one critic must respond against the same `proposal_id`
3. `consensus` — emitted when proposal + ≥1 critique have been reconciled
4. `dissensus` — emitted when reconciliation fails; auto-escalates with `escalated_at`

The daemon enforces the ordering (PRD §3 / M3 gate): `consensus` without a
prior `critique` against the same proposal returns HTTP 400.

## View status

```bash
# All proposals on a plan, bucketed by stage.
sdi consensus status <PLAN-ID>

# Drill into one proposal.
sdi consensus status <PLAN-ID> --proposal-id <DEC-ID>
```

The output shape (per proposal):

```json
{
  "proposal_id": "DEC-…",
  "proposal":   { … decision row … },
  "critiques":  [ { … }, { … } ],
  "consensus":  null | { … },
  "dissensus":  null | { … },
  "stage":      "proposal" | "critique" | "consensus" | "dissensus",
  "critique_count": 2
}
```

## Recovery patterns

- `stage = proposal` for too long → no critic engaged. Run a hand-off
  (`/agent-note append --kind handoff --to schema-architect`).
- `stage = dissensus` → auto-escalation already fired. Convene the user; do
  not try to override autonomously even at L5.
- `stage = consensus` but autonomy mode = L3 → user still has to apply the
  decision (see `/autonomy`).
