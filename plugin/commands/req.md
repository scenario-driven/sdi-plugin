---
description: Capture a requirement under the active SDI plan (D12 snapshot)
argument-hint: <PLAN-ID> <SHORT-CODE> "<TITLE>" [--body <markdown>] [--source <ref>]
allowed-tools: Bash, Read
---

# /req — requirement capture (D12 SNAPSHOT)

A **requirement** is a snapshot constraint or input fact. Snapshot semantics
(D12): updates **overwrite in place**, no version history. The append-only
log surface is `/decide`, not `/req`.

## What to do

```bash
sdi req create <PLAN-ID> <SHORT-CODE> "<title>" \
  --body "$(cat requirement-body.md)" \
  --source "<file:line | ticket | url>"
```

Use `--body -` to read body from stdin. `--source` is free-form but should
point to something checkable — a file:line, an upstream ticket, or a URL.

## When to write a requirement vs. a scenario

- **Scenario** (`/scenario`) — a behavior the system must exhibit, in
  Given/When/Then form. Verifiable per round.
- **Requirement** (`/req`) — a constraint, fact, or interface contract that
  shapes scenarios but isn't a behavior itself (e.g. "the daemon binds to
  127.0.0.1 only", "Node 20+ is required").

Don't compress a behavior into a requirement to skip the GWT discipline.

## Failure modes

- `SHORT_CODE_TAKEN` — pick a different code.
- `PLAN_NOT_FOUND` — confirm the plan id; only existing plans accept
  requirements (any status).

## Listing / inspection

```bash
sdi req list <PLAN-ID>
sdi req view <REQ-ID>
```
