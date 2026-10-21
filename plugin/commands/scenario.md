---
description: Add a Given/When/Then scenario to the active SDI plan
argument-hint: <PLAN-ID> <SHORT-CODE> --given "..." --when "..." --then "..." [--confirmed]
allowed-tools: Bash, Read
---

# /scenario — add a GWT scenario (D5)

The user invokes this to add a scenario to a plan. SDI scenarios are
**GWT-strict** (D5): all three of `--given`, `--when`, `--then` are required
and must be non-empty natural-language sentences. `--confirmed` flips the
scenario to confirmed immediately (otherwise it lands in `draft`).

## What to do

1. Resolve the active plan if `<PLAN-ID>` is omitted:
   - `sdi project by-cwd "$(pwd)"` → `project.id`
   - `sdi plan active <PROJECT-ID>` → `plan.id`
2. Pick or accept a short code (`<SHORT-CODE>`) the user supplies. The code is
   the human ticket suffix (e.g. `SC-12`); the daemon enforces uniqueness per
   plan.
3. Quote the GWT clauses verbatim. Do **not** rewrite them — the natural
   language is the spec.
4. Run:

```bash
sdi scenario create <PLAN-ID> <SHORT-CODE> \
  --given "<given clause>" \
  --when "<when clause>" \
  --then "<then clause>" \
  --confirmed
```

5. On success, print the returned `id` and remind the user that the plan
   approve gate (D8) requires ≥1 confirmed scenario.

## Failure modes

- `GWT_EMPTY` — one of the three clauses was empty or whitespace; ask the user
  to fill it in.
- `CONFLICT` — short_code already taken on this plan; increment the suffix.
- `NOT_FOUND` — verify the plan id; only existing plans accept scenarios.

## Lineage

This is the LLM-era successor to a BDD `.feature` file. Unlike Gherkin, no
step definitions are required — the LLM reads the GWT directly.
