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

**Tasks** require **evidence** on the `done` transition. The evidence is the
durable artifact that proves the scenario passed — file:line, test transcript,
run id, anything checkable. Use the `sdi-evidence` skill at done-time.

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

Round R2 and later default to `mode=strict-regression`. Under this mode, every
prior verdict carries into the new round automatically; failing scenarios stay
failed until you re-record a passing verdict. This is the auto-regression
property that distinguishes SDI from TDD/BDD.

Two alternative modes exist:
- `additive` — skips carry-over (only new scenarios verify in this round).
  Surface the consequence: prior regressions go untracked this round.
- `disruption` — used after a confirmed change to existing scenarios; requires
  human review before activation (the daemon flips the plan to needs-review
  and returns `DISRUPTION_PENDING` until the review is resolved).

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
/decide supersede <OLD-DECISION-ID> <NEW-CODE> "Switch from X to Y"
  --body "Context… Decision: Y. Consequences: …"
```

---

## MCP tools (rag-scope only)

The MCP server (`sdi mcp`) exposes 9 tools. The 4 **read** tools force
`scope=rag` at the URL layer — they never leak `reference` or `archive`
content to the LLM. Use them when planning, never for current task state
(that lives behind HTTP + slash commands):

| Tool | Use it for |
|---|---|
| `search_knowledge` | Search rag-scoped knowledge entries. |
| `search_scenarios` | Find scenarios by keyword across the active plan. |
| `get_plan_context` | Composite snapshot: plan + scenarios + tasks-in-flight + decisions. |
| `get_recent_decisions` | Tail the ADR log. |

The 5 **write** tools are LLM-callable mutations that mirror the slash
commands:

| Tool | Mirrors |
|---|---|
| `add_scenario` | `/scenario create` |
| `add_requirement` | `/req create` |
| `add_decision` | `/decide create` |
| `update_task_evidence` | task `done` evidence record (see `sdi-evidence`) |
| `start_round` | `/round activate` |

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
| `EVIDENCE_REQUIRED` | Task `done` transition arrived without evidence. | Re-send with a checkable evidence ref. See `sdi-evidence`. |
| `DISRUPTION_PENDING` | A disruption review is open on the plan. | Resolve via `/disruption resolve` before activating a round. See `sdi-round`. |
| `INVALID_TRANSITION` | Lifecycle rule violated (e.g. only one active round per plan). | Complete the predecessor first. |
| `MODE_REJECTED_AT_R1` | `strict-regression` is rejected at R1 (nothing to carry). | Use `additive` or omit `--mode` at R1. |
| `NOT_FOUND` | Entity does not exist. | Verify the id; register the cwd via `sdi project create <KEY> "<name>" --cwd "$(pwd)"` if it's a project lookup. |
| `PATH_INVARIANT_VIOLATION` | User data resolves under `~/.claude/plugins/`. | Fix XDG paths; see `sdi doctor`. |
