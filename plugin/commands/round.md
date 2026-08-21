---
description: Manage SDI round lifecycle (create, activate, complete, results)
argument-hint: create|activate|complete|active|result [args…]
allowed-tools: Bash, Read
---

# /round — round lifecycle (D6/D7/D10)

A **round** is one iteration of scenario implementation. R1 is new-development;
R2+ default to **strict-regression** (D6) and auto-carry verdicts from the
prior round under that mode. In-flight tasks **pause** by default when a round
starts (D10).

## Subcommands

### `/round create <PLAN-ID> <SHORT-CODE>`
Defaults: `mode=strict-regression`, `in-flight=pause`, `disruption=needs-review`.

```bash
sdi round create <PLAN-ID> <SHORT-CODE> \
  --mode strict-regression \
  --in-flight pause \
  --disruption needs-review
```

`--in-flight` accepts `pause` (default), `abort`, or `continue-on-noimpact`
(PRD §6 #5). Override only with explicit user request. `--mode additive` skips
regression carry-over; `--mode disruption` requires confirmed scenario
changes. Both deviate from D6/D7 defaults — surface the consequence before
running.

### `/round activate <ROUND-ID>`
```bash
sdi round activate <ROUND-ID>
```
Activates the round; strict-regression mode carries prior verdicts into this
round's results table.

### `/round complete <ROUND-ID>`
```bash
sdi round complete <ROUND-ID>
```
Closes the round. Completion does not require every scenario to pass — failing
verdicts stay on record and feed into the next round.

### `/round active <PLAN-ID>`
```bash
sdi round active <PLAN-ID>
```
Shows the currently-active round for a plan (404 if none).

### `/round result <ROUND-ID> --scenario <SCN-ID> --result <verdict> --evidence <ref>`
```bash
sdi round result <ROUND-ID> \
  --scenario <SCN-ID> \
  --result passing \
  --evidence "file:line | test name | url"
```
Records a per-scenario verdict. Vocabulary: `passing | failing | impacted |
retired` (D6 + D9). `--evidence` is free-form but should point to something
checkable (transcript, file:line, run id).

## Failure modes

- `SCENARIOS_REQUIRED` (D8) — the plan was approved without confirmed
  scenarios; add at least one via `/scenario` first.
- `DISRUPTION_PENDING` (D9) — a disruption review is open on this plan;
  resolve it before activating the round.
- `INVALID_TRANSITION` — `active` is unique per plan; complete the previous
  round first, or you tried to activate a round not in `planning`.
