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
(PRD §6 #5). Override only with explicit user request. `--mode forward-only`
(alias `additive`) skips regression carry-over — it deviates from the D6/D7
default, so surface the consequence before running. Disruption review is not a
mode: `--disruption needs-review|auto` only changes how resolutions are
proposed, and a pending review blocks activation under every mode.

### `/round activate <ROUND-ID>`
```bash
sdi round activate <ROUND-ID>
```
Activates the round; strict-regression mode carries the latest completed
round's verdicts into this round's results table. A carried `passing` whose
evidence input fingerprint no longer matches is carried as
`needs_verification`, not as `passing` and not as `failing`. The response lists
`scenarios_needing_verification`.

### `/round complete <ROUND-ID>`
```bash
sdi round complete <ROUND-ID>
```
Closes the round. The daemon does not require every scenario to pass — failing
verdicts stay on record and feed into the next round — but it refuses while a
harness/environment failure is unresolved (`HARNESS_FAILURE_UNRESOLVED`). The
implementation loop (`sdi-impl-loop`) completes a round only when it is green.

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
  --evidence run:<RUN-ID>
```
Records a per-scenario verdict. Vocabulary: `passing | failing | impacted |
retired` (D6 + D9); `needs_verification` is daemon-assigned and cannot be
submitted. Test verdicts cite `run:<RUN-ID>` from `sdi run verify`; the daemon
validates it (run exists, linked to the scenario, exited 0 for `passing`,
fingerprint fresh — `EVIDENCE_RUN_INVALID` / `EVIDENCE_STALE` otherwise). A
free-text ref (file:line, URL, decision id) is accepted only for non-test
evidence.

## Failure modes

- `SCENARIOS_REQUIRED` (D8) — the plan was approved without confirmed
  scenarios; add at least one via `/scenario` first.
- `DISRUPTION_PENDING` (D9) — a disruption review is pending on this plan; a
  human resolves it before the round activates (see the `sdi-round` skill,
  "Disruption review gate").
- `INVALID_ROUND_MODE` — `--mode` is not `strict-regression` /
  `forward-only` (alias `additive`).
- `INVALID_TRANSITION` — `active` is unique per plan; complete the previous
  round first, or you tried to activate a round not in `planning`.
