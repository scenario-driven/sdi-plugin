---
description: Create / approve / inspect an SDI plan (D1/D8 lifecycle)
argument-hint: create|approve|complete|active|view [args…]
allowed-tools: Bash, Read
---

# /plan — plan lifecycle (D1/D8)

A **plan** is the unit of approval. Lifecycle: `draft → active → completed`.
Approval gate (D8): **≥1 confirmed scenario** is required; task count is
irrelevant — tasks are runtime artifacts (D3) decomposed later.

## Subcommands

### `/plan create <PROJECT-ID> <SHORT-CODE> "<TITLE>" [--body <markdown>]`
```bash
sdi plan create <PROJECT-ID> <SHORT-CODE> "<title>" --body "$(cat plan-body.md)"
```
Use `--body -` to read body from stdin. The plan lands in `draft`; add at
least one confirmed scenario before approving.

### `/plan approve <PLAN-ID>`
```bash
sdi plan approve <PLAN-ID>
```
Flips `draft → active`. Returns `SCENARIOS_REQUIRED` if no confirmed scenario
exists. Only **one active plan per project** is allowed.

### `/plan complete <PLAN-ID>`
```bash
sdi plan complete <PLAN-ID>
```
Closes the plan. Does **not** require all scenarios to pass — verdicts remain
attached to the rounds, not the plan status.

### `/plan active <PROJECT-ID>`
```bash
sdi plan active <PROJECT-ID>
```
Shows the active plan for the project (404 if none).

### `/plan view <PLAN-ID>`
```bash
sdi plan view <PLAN-ID>
```

## Failure modes

- `SCENARIOS_REQUIRED` (D8) — add a confirmed scenario via `/scenario` first.
- `NOT_FOUND` — verify the project id, or register the cwd via
  `sdi project create <KEY> "<name>" --cwd "$(pwd)"`.
- `INVALID_TRANSITION` — only one active plan per project; complete the
  previous one first, or the target plan is not in `draft`.

## Snapshot policy (D12)

Plan body updates are **in-place overwrites** — no version history is kept in
the body. Change rationale belongs in a decision (`/decide`), which is the
append-only history surface.
