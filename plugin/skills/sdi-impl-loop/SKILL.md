---
name: sdi-impl-loop
description: Drive the inner loop — implementation convergence over rounds (D30/D31). After plan approve, open a round, fan out decompose → impl-coder → test-runner under a pattern, and on a failing/impacted verdict do bounded-N retry then escalate. When all DetailScenarios pass, complete the round; when a regression against a prior round is detected, AUTO-OPEN the next round and replay — looping until regressions=0 and all pass. Use after sdi-converge has approved the plan, when the user wants implementation driven to green.
---

# /impl-loop — the inner loop (implementation convergence)

The **inner loop** is the round as a *verb* (D30): a deterministic orchestrator
runs `decompose → impl → test → verdict`, retries failures within a bound,
**completes** a round when every DetailScenario passes, and **auto-opens the
next round** the moment a regression against a prior round appears — looping
until regressions = 0 and all pass (D34-4). v1's round was a noun nobody
advanced; v2 turns the wheel automatically. This skill is the LLM-side
authority on the auto-rotation + bounded-retry behaviour; it composes the
existing round / pattern / impl / test / regression agents rather than
replacing them.

> **Lineage**: v1 `round complete` only flipped a status — nothing opened R2,
> so people stopped at R1 (PRD-v2 §1.1). v2's inner loop is the missing
> orchestrator that keeps rotating until the implementation is regression-free.

---

## When to invoke

Trigger after the spec is converged and the plan approved (D34):

- "drive this to green" / "implement the approved plan"
- "run the rounds until there's no regression" / "구현 루프 돌려서 끝까지 통과시켜"
- "verify R2 and auto-advance if anything regressed"

Don't invoke this for:
- **Spec building / decision questions** → that's the outer loop, `sdi-converge`.
- **A single round's manual create/activate semantics** → see `sdi-round` (this
  skill drives it on a loop; `sdi-round` documents the per-round mechanics).

---

## Preconditions

- Plan is `active` (approved through the D34 gate). If not, run `sdi-converge`
  first — implementing against an unconverged spec has no oracle to converge to.
- Daemon up (`sdi daemon start`). The inner loop uses the existing `sdi` CLI
  surface end-to-end (round / task / pattern); no raw HTTP needed here.

---

## The loop (D31 inner — auto-rotate + bounded retry)

```
round = open(plan)                       # R1 = new dev; R2+ = regression (D7)
loop:
  1. pattern   → pattern-orchestrator designs the fan-out over needs-verification; pattern-critic gates it
  2. decompose → sdi task create … --produced-via-pattern <PAT>  (one+ tasks per needs-verification scenario)
  3. fan out   → impl-coder (parallel) → test-runner → per-scenario verdict
  4. for each verdict in {failing, impacted}:
        retry impl  (bounded: N attempts)  |  on exhaustion → escalate
  5. all DetailScenarios pass?            → sdi round complete <ROUND>
  6. regression vs a prior round?         → round = open(next)   (AUTO, D30)  → replay (regression-runner)
  7. regressions = 0 AND all pass         → exit (implementation converged)
```

### 1–2. Open, pattern, decompose

```bash
sdi round create <PLAN-ID> <SHORT-CODE>          # e.g. R1 ; defaults strict-regression@R≥2, pause-in-flight
sdi round activate <ROUND-ID>                     # carries prior verdicts; emits scenarios_needing_verification
```

Before the first `sdi task create`, spawn **pattern-orchestrator** on the
needs-verification set so the fan-out shape is a written decision (D13/D23) —
otherwise the daemon back-fills a `direct` sentinel and the whole round is
capped at L3. `pattern-critic` validates the shape (D26), then bind every task:

```bash
sdi task create <ROUND-ID> <SHORT-CODE> "<one-line desc>" \
  --scenario <SCN-ID> --produced-via-pattern <PAT-ID>
```

(Full per-round mechanics — modes, in-flight policy, disruption gate — live in
`sdi-round`; this skill drives them on a loop.)

### 3–4. Fan out, verdict, BOUNDED retry, escalate

For each task: **impl-coder** implements the smallest change satisfying the
linked scenarios, then **test-runner** emits one evidence row per scenario with
a verdict from the fixed vocab `passing | failing | impacted | retired`.

On `failing` or `impacted`, retry implementation **within a bound** — do not
spin forever:

- **Bound: N = 3 attempts** per scenario by default (re-read the failure
  evidence, re-implement, re-test). Each attempt must consume the previous
  attempt's concrete failure (file:line / test name), not retry blindly.
- **On exhaustion (attempt N still red): escalate, do not loop.** Route by cause:
  - broken-by-a-deliberate-behaviour-change (`impacted`) → **disruption-analyst**
    (surface the cross-scenario blast; the user sees it) and, if it stems from a
    decision, **decision-resolver** to propose a supersession.
  - genuine `failing` the impl cannot satisfy → **decision-resolver** /
    **schema-architect** (if it needs a schema/architecture decision, D17
    forced-L4) — the spec or a decision is wrong, not just the code.

A scenario must never be silently left red to "fix next round". The verdict is
sticky in both directions under strict-regression (D6).

### 5. Complete when green

When every DetailScenario in the needs-verification set has a `passing` verdict
with checkable evidence:

```bash
sdi round complete <ROUND-ID>
```

The daemon rejects `done` without evidence (`EVIDENCE_REQUIRED`) and refuses
completion while any linked task lacks a verdict — so "complete" genuinely
means "all green".

### 6. Auto-open the next round on regression (D30 — the defining behaviour)

After completing a round, check the prior round's passing scenarios against the
new code. Under strict-regression (D6 default at R≥2) the daemon carries prior
verdicts at the *next* activation, but the **decision to open that next round is
the orchestrator's, and it is automatic**: if a previously-`passing` scenario
now regresses, open the next round without waiting for a human:

```bash
sdi round create <PLAN-ID> <NEXT-SHORT-CODE>      # e.g. R2 ; strict-regression default at R≥2
sdi round activate <ROUND-ID>                      # auto-carries prior verdicts
```

Then **regression-runner** replays every prior-`passing` scenario under the new
code and posts verdicts:

```bash
sdi round result <ROUND-ID> --scenario <SCN-ID> --result <verdict> --evidence <ref>
```

A regressed scenario is marked `failing` (not softened to `impacted` — that is
reserved for a deliberate behaviour change), which re-enters the loop at step 3.

### 7. Exit when stable

The loop terminates only when a completed round shows **all DetailScenarios
passing AND zero regression** against the prior round (D34-4). Then the
implementation has converged.

---

## Invariants

- **The wheel turns itself.** Regression detection auto-opens the next round
  (D30); never stop at R1 and wait for the user to type `sdi round create`.
- **Bounded retry, then escalate — never infinite, never silent-red.** N=3
  attempts, each consuming the prior failure, then escalate to the right
  specialist. Leaving a scenario red "for next round" is a protocol violation.
- **Pattern before fan-out.** Decide the CollaborationPattern before the first
  task create, or the round silently L3-caps via the `direct` sentinel (D23).
- **Evidence is checkable.** Verdicts carry file:line / test-name / log refs;
  a broken evidence ref demotes the carried verdict at the next activation.
- **Effort is a fact, not a brake.** "This round is taking long" is reported,
  never a reason to complete a round with a red scenario
  (product-quality-first §2).

---

## Failure recovery

| Code | Meaning | Recovery |
|---|---|---|
| `EVIDENCE_REQUIRED` | A task hit `done` without evidence; blocks round complete. | Record a checkable evidence item per linked scenario (`sdi-evidence`), then retry complete. |
| `INVALID_TRANSITION` | Another round on the plan is already `active`. | `sdi round complete <PREDECESSOR>` first, then activate the next. |
| `DISRUPTION_PENDING` | The plan has unresolved scenario changes. | `sdi disruption resolve <REVIEW-ID> --approve\|--reject`, then re-activate (see `sdi-round`). |
| `MODE_REJECTED_AT_R1` | `strict-regression` requested at R1 (nothing to carry). | Omit `--mode` at R1 (or `--mode forward-only`). |
| Retry bound (N=3) exhausted | The code cannot satisfy the scenario. | Escalate — `disruption-analyst` / `decision-resolver` / `schema-architect`. Do not raise N silently; a persistent red means the spec or a decision is wrong. |
