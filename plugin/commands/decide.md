---
description: Log an SDI decision (D12 append-only ADR; supersession-chain)
argument-hint: <PLAN-ID> <SHORT-CODE> "<TITLE>" [--body <markdown>] | supersede <DEC-ID> [args…]
allowed-tools: Bash, Read
---

# /decide — append-only decision log (D12)

A **decision** is an ADR-style record. The decision log is **append-only**:
new facts about a topic add a new decision and (optionally) flip the
predecessor to `superseded`. This is the only history surface in SDI; plans
and requirements are snapshot-only.

## Create a new decision

```bash
sdi decision create <PLAN-ID> <SHORT-CODE> "<title>" \
  --body "$(cat decision-body.md)"
```

The body should explain:
- **Context** — what triggered the decision.
- **Decision** — the choice in one sentence.
- **Consequences** — what becomes easier / harder / impossible.

## Supersede an existing decision

```bash
sdi decision supersede <PRIOR-DECISION-ID> \
  --plan-id <PLAN-ID> \
  --short-code <NEW-SHORT-CODE> \
  --title "<title>" \
  --body "$(cat new-body.md)"
```

This creates a new `accepted` decision and flips the predecessor to
`superseded`. The supersession chain is queryable — never edit history in
place.

## When to use /decide vs /req vs /scenario

| Surface | Semantics | When |
|---|---|---|
| `/req` | snapshot | Constraint shaping the design (e.g. "Node 20+") |
| `/scenario` | snapshot, GWT-strict | Verifiable behavior the system must exhibit |
| `/decide` | append-only | "Why did we choose X over Y?" — durable rationale |

If you find yourself rewriting a plan or requirement body to record "we
changed our minds because…", that rationale belongs in a decision instead.

## Listing / inspection

```bash
sdi decision list <PLAN-ID>
sdi decision view <DECISION-ID>
```
