---
description: Read or post an SDI AgentNote (M1 blackboard + M2 hand-off receipts)
argument-hint: append <PRJ-ID> --scope <SCOPE> --kind <KIND> --from <AGENT> [--to <AGENT>] "<body>" | list … | handoffs <TO-AGENT> | ack <NOTE-ID> | retire <NOTE-ID> --reason "…"
allowed-tools: Bash, Read
---

# /agent-note — M1 blackboard + M2 hand-off

The blackboard is the **substrate** that multi-agent flows talk through (D19).
Every specialist publishes notes anchored to a plan / round / scenario / task;
hand-offs travel as `kind=handoff` notes addressed to a `to_agent` and require
an explicit ack to clear from the pending queue (M2).

## Append a note

```bash
# Plain blackboard observation (no addressee).
sdi agent-note append <PRJ-ID> \
  --scope plan --plan-id <PLAN-ID> \
  --kind observation --from impl-coder \
  "schema migration 006 changed unique key on autonomy_policy"

# Hand-off (kind=handoff requires --to).
sdi agent-note append <PRJ-ID> \
  --scope task --task-id <TASK-ID> \
  --kind handoff --from impl-coder --to test-runner \
  "please verify ON CONFLICT branch on autonomy_policy.upsert"
```

`kind` ∈ {`handoff`, `observation`, `question`, `answer`, `warning`, `summary`}.
`scope` ∈ {`plan`, `round`, `scenario`, `task`, `global`}.

## Inspect

```bash
# Active notes on one anchor (e.g. a plan).
sdi agent-note list --scope plan --anchor <PLAN-ID>

# Pending hand-offs addressed to one agent.
sdi agent-note handoffs <AGENT-NAME>
```

## Acknowledge / retire

```bash
sdi agent-note ack <NOTE-ID>            # clears it from pending hand-offs
sdi agent-note retire <NOTE-ID> --reason "duplicated by NOTE-XYZ"
```

`retire` is non-destructive — the row stays for audit, but `list` and
`handoffs` hide it.

## When to use

- Communication substrate runs **mode-independent** (D19). It never blocks on
  autonomy mode — even at L3, agents talk freely.
- Hand-offs are the supported way for an agent to ping another agent at a
  specific anchor; don't try to chain agents through tool output.
- If you find yourself writing prose comments about "what the other agent
  should do next", that prose belongs in a handoff note.
