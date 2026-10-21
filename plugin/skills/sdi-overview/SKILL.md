---
name: sdi-overview
description: SDI orientation — entities, lifecycle, GWT discipline, regression model, evidence rules, MCP tool map, and failure-code recovery. Read this first when working in an SDI project. SDI is Scenario-Driven Implementation, the LLM-era successor to TDD/BDD.
---

# SDI — Scenario-Driven Implementation

This skill is the cold-read entry point for operating inside an SDI project:
when to create scenarios vs. requirements vs. decisions, how rounds carry
verdicts across iterations, and which tool to reach for at each step.

> **Lineage**: TDD (1990s — test code as spec) → BDD (2000s — Gherkin DSL +
> step glue) → **SDI** (now — natural-language GWT, the LLM reads it
> directly, no compilation step).

---

## The five first-class entities

| Entity | Purpose | Mutability |
|---|---|---|
| **Plan** | Unit of approval. `draft → active → completed`. | Body snapshot-only — overwrite in place; never keep version history in the body. |
| **Requirement** | Snapshot constraint or input fact shaping scenarios. | Overwrite in place. |
| **Scenario** | Given/When/Then behavior the system must exhibit. | Body snapshot; verdict tracked per round. |
| **Decision** | ADR-style rationale. **Append-only** with supersession chain. | Never edited; supersede instead. |
| **Round** | One iteration (R1, R2, …). R2+ default to strict-regression. | Status `planning → active → completed`. |

**Task** is a *runtime artifact*, not a first-class entity. The LLM decomposes
tasks from scenarios + requirements at round activation; humans do not author
tasks directly.

---

## The canonical workflow

```
register project
  └─ /plan create  (status: draft)
      └─ /req create       (constraints)
      └─ /scenario create  (confirmed GWT)
      └─ /decide create    (rationale, optional)
  └─ /plan approve  (gate: ≥1 confirmed scenario)
      └─ /round create R1
      └─ /round activate R1
          └─ [LLM decomposes tasks from scenarios + reqs]
          └─ [LLM implements]
          └─ /round result … --result passing --evidence …
      └─ /round complete R1
  └─ next iteration:
      └─ /round create R2  (mode: strict-regression by default)
      └─ /round activate R2 (prior verdicts auto-carry)
      └─ … verify new + regression
```

**Tasks** require **evidence** to complete (`sdi task complete <TASK-ID>
--evidence <SCN-ID>=<verdict>@<ref>`, one row per linked scenario). Test
evidence is a recorded run: `sdi run verify --scenario <SCN-ID> -- <cmd…>`,
then `<SCN-ID>=passing@run:<RUN-ID>`, which the daemon validates (run exists,
linked, exited 0, fingerprint fresh). Free-text refs (file:line, URL, decision
id) are only for non-test evidence. Use the `sdi-evidence` skill at completion
time.

---

## When to use each surface

| You want to record | Surface | Why |
|---|---|---|
| A behavior the system must exhibit | `/scenario` | GWT-strict, verifiable per round. |
| A constraint, input fact, or interface contract | `/req` | Snapshot — shapes scenarios but isn't a behavior. |
| Why we chose X over Y | `/decide` | Append-only ADR; the only history surface. |
| Current state of plans / tasks / rounds | `/sdi-status` | Read-only dashboard view. |

If you find yourself rewriting a plan body to record "we changed our minds
because…", that rationale belongs in a **decision**, not in the plan body.
Plan bodies are snapshots.

---

## GWT is non-empty (enforced by daemon)

Every scenario MUST have non-empty `given`, `when`, `then` clauses in natural
language. The daemon enforces this and returns `GWT_EMPTY` otherwise. Do not
compress a behavior into a single sentence; the three-part structure is the
discipline that makes the scenario verifiable.

✗ Bad: `--given "" --when "user submits the form" --then "it works"`
✓ Good:
```
--given "the user is signed in and on /profile"
--when  "they click 'save' with name = 'Aria'"
--then  "the API returns 200 and the persisted record shows name = 'Aria'"
```

Full normalisation procedure lives in the `sdi-scenario` skill.

---

## Strict-regression default (R2+)

Round R2 and later default to `mode=strict-regression`. Under this mode, the
latest completed round's verdicts carry into the new round automatically;
failing scenarios stay failed until you re-record a passing verdict. This is
the auto-regression property that distinguishes SDI from TDD/BDD.

A carried `passing` is only as good as its run: the daemon recomputes the
evidencing run's input fingerprint against the workspace at carry time, and a
pass whose run is stale becomes `needs_verification` (daemon-assigned, never
submittable) —
it is neither kept as `passing` nor turned into `failing`; it joins the
needs-verification set and a fresh run decides it.

The one alternative mode:
- `forward-only` (alias `additive`) — skips carry-over (only new scenarios
  verify in this round). Surface the consequence: prior regressions go
  untracked this round.

Disruption review is not a mode. A change that impacts existing scenarios
opens a disruption review; the daemon returns `DISRUPTION_PENDING` on round
activation until a human resolves it, under any mode and any `--disruption`
policy.

Default to strict-regression. Switch only on explicit user request. Round
lifecycle, in-flight-task policy, and task auto-decomposition at activation
live in the `sdi-round` skill.

---

## Plan approve gate (≥1 confirmed scenario)

`sdi plan approve <PLAN>` returns `SCENARIOS_REQUIRED` if the plan has zero
confirmed scenarios. Task count does **not** factor into approval — tasks are
decomposed *after* approval as runtime artifacts. The contract is "a plan with
no verifiable behaviors is not ready," not "a plan with no work items."

---

## In-flight tasks on round start

When a new round starts, any task in `in_progress` from the previous round is
**paused** by default (the task flips to `blocked`). Override by choosing a
different policy at round creation:

- `sdi round create … --in-flight abort` — cancel in-flight tasks (use when
  scenarios changed and the work no longer applies).
- `sdi round create … --in-flight continue-on-noimpact` — continue tasks whose
  parent scenarios didn't change between rounds.

Default to pause. Document the reason in a decision if you override.

---

## Snapshot-only bodies, append-only decisions

Plan / Requirement / Scenario bodies are **snapshot**: updates overwrite in
place, no version history in the body. The history surface is the
**decision** log (`/decide`), which is append-only with supersession chains.

✗ Forbidden in plan/req/scenario bodies:
- "Previously we did X, now we do Y"
- "v1: A → v2: B"
- Strikethrough of older content
- Sidebar notes like "(originally A, changed to B after feedback)"

✓ Correct pattern:
```
sdi decision supersede <OLD-DECISION-ID> \
  --plan-id <PLAN-ID> --short-code <NEW-CODE> --title "Switch from X to Y" \
  --body "Context… Decision: Y. Consequences: …"
```

---

## Read once: handoffs and CLI signatures

This is the canonical rule; every other skill and agent points here.

- **Handoffs.** Read the handoff payload (comment, agent-note, or
  `sdi task brief`) once, when the step starts, then work from the reference it
  gave you (receipt or run reference). Do not poll or re-read it each step;
  re-read only when the referenced artifact changes, for example when a new
  receipt id is announced.
- **CLI signatures.** Take command signatures from the context the
  SessionStart hook injects, or from `sdi task brief <TASK-ID>`, which already
  carries the evidence format. If a signature is still unknown, read its
  `--help` at most once per session and reuse the result — never run `--help`
  again before each command.

---

## MCP tools

The MCP server (`sdi mcp`) exposes 11 tools. Knowledge reads force
`scope=rag` at the URL layer — they never leak `reference` or `archive`
content to the LLM. The 6 **read** tools are for planning and orientation:

| Tool | Use it for |
|---|---|
| `search_knowledge` | Search rag-scoped knowledge entries. |
| `search_scenarios` | Find scenarios by keyword within one plan. |
| `get_plan_context` | Composite snapshot: plan + scenarios + tasks-in-flight + decisions. |
| `get_recent_decisions` | Tail the ADR log. |
| `get_autonomy_state` | Autonomy policy rows + circuit-breaker state for a project. |
| `list_agent_notes` | Agent-note blackboard: pending hand-offs or notes at an anchor. |

The 5 **write** tools are LLM-callable mutations that mirror the CLI:

| Tool | Mirrors |
|---|---|
| `add_scenario` | `sdi scenario create` |
| `add_requirement` | `sdi req create` |
| `add_decision` | `sdi decision create` |
| `update_task_evidence` | `sdi task complete --evidence …` (completes the task; see `sdi-evidence`) |
| `start_round` | `sdi round activate` |

Tools appear in the MCP client under the server name `sdi` (some clients
display them as `sdi__search_knowledge`, others namespace differently — match
the client's convention). Prefer slash commands when the user is driving;
prefer MCP when you are driving (decompose, verify, record).

---

## Failure modes you will see

| Code | Meaning | Recovery |
|---|---|---|
| `GWT_EMPTY` | A scenario clause was empty. | Fill the missing clause; resubmit. See `sdi-scenario`. |
| `SCENARIOS_REQUIRED` | Plan has zero confirmed scenarios. | `/scenario create … --confirmed`. |
| `EVIDENCE_REQUIRED` | `sdi task complete` arrived without evidence. | Re-send with a checkable evidence ref per linked scenario. See `sdi-evidence`. |
| `DISRUPTION_PENDING` | A disruption review is pending on the plan. | A human resolves it on the daemon HTTP seam before the round activates. See `sdi-round` → "Disruption review gate". |
| `RETRY_BUDGET_EXHAUSTED` | The same failure signature hit the retry cap for a scenario (task renames / re-creates do not reset it). | Stop retrying and escalate; release only with `sdi run release-budget --scenario <SCN-ID> --decision <DEC-ID>`. See `sdi-impl-loop`. |
| `HARNESS_FAILURE_UNRESOLVED` | Round complete refused: a scenario's latest run failed for harness/environment reasons (runner crash, missing toolchain, timeout, signal, OOM). | Repair and record a passing `sdi run verify`; harness failures never consume the product retry budget. See `sdi-impl-loop`. |
| `EVIDENCE_RUN_INVALID` | A `run:` evidence ref does not exist, is not linked to the scenario, has not finished, or its outcome contradicts the verdict. | Cite the run that verified the scenario with the verdict it supports. See `sdi-evidence`. |
| `EVIDENCE_STALE` | The cited run's input fingerprint no longer matches the workspace. | Re-run `sdi run verify` against the current inputs and cite the new run. See `sdi-evidence`. |
| `BASELINE_LOCKED` | The active round's baseline was overwritten or rebased without a superseding decision. | `sdi round baseline <ROUND-ID> … --decision <DEC-ID>` after recording that decision. See `sdi-round`. |
| `INVALID_TRANSITION` | Lifecycle rule violated (e.g. only one active round per plan). | Complete the predecessor first. |
| `INVALID_ROUND_MODE` | `--mode` was not `strict-regression` / `forward-only` (alias `additive`). | Use a valid mode; omit `--mode` at R1, where there is nothing to carry. |
| `NOT_FOUND` | Entity does not exist. | Verify the id; register the cwd via `sdi project create <KEY> "<name>" --cwd "$(pwd)"` if it's a project lookup. |
| `PATH_INVARIANT_VIOLATION` | User data resolves under `~/.claude/plugins/`. | Fix XDG paths; see `sdi doctor`. |
