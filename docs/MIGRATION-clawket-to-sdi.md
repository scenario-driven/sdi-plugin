# Migration: Clawket v3.0 → SDI

> **Out of v1 PRD scope** (PRD §9 explicitly labels migration as "별도 트랙").
> This document captures the domain mapping that informs SDI's design choices.
> No migration tool ships in v0.1; that work is tracked separately.

## Domain mapping

PRD §9 names the canonical mapping. SDI's data model and command surface
descend from Clawket but reinterpret each entity's role.

| Clawket v3                       | SDI                                       | Note                                                                                            |
| ---                              | ---                                       | ---                                                                                             |
| `Plan`                           | `Plan`                                    | Kept verbatim. Same `draft → active → completed` lifecycle. Approve gate semantics reinterpreted (§6 #2). |
| `Unit`                           | `Scenario.tag`                            | Demoted from entity to tag. SDI's atomic grouping is the **Scenario**, not Unit.                |
| `Cycle`                          | `Round`                                   | Renamed and resemanticized. A Round is a regression-aware execution wave (§6 #3, #4, #5).       |
| `Task`                           | `Task` (runtime artifact)                 | Same lifecycle states but D3-runtime: Tasks are scaffolding to satisfy Scenarios, not first-class deliverables. |
| `type=decision` artifact         | `Decision` (1st-class entity)             | Promoted. Append-only ADR with provenance to the Scenario/Requirement it resolves.              |
| `--evidence "string"`            | Structured evidence (per-scenario)        | PRD §6 #6: free string rejected. Each Task `done` carries `{scenario_id, status: pass|fail, ref}[]`. |
| `--evidence` ad-hoc semantics    | `Requirement` (snapshot-only)             | PRD §6 #7: separate entity. Body cannot carry history traces; that's the Decision's job.        |

GWT scenarios are **new**. Clawket has no analog — its Tasks describe work,
not behavior. The migration tooling, when it ships, will use an LLM-assisted
extraction from Task body to GWT candidates (PRD §9 closing paragraph).

## Why the resemanticization matters

Three of the renames are not cosmetic:

1. **Unit → Scenario.tag.** Clawket's Unit existed as a grouping container with
   its own approval semantics in earlier drafts. v3 stripped Unit of state but
   kept the entity. SDI drops the entity entirely — Scenarios are tagged with
   strings, no parent grouping object. This eliminates a class of cascade bugs
   and matches Gherkin's tag model.

2. **Cycle → Round.** Clawket's Cycle is a Kanban-style WIP boundary. SDI's
   Round is a regression wave: when R2 starts, every Scenario that passed in
   R1 must be carried into the R2 queue (PRD §6 #3). Disruption analysis
   (PRD §6 #4) and in-flight pause (PRD §6 #5) bind to Round entry, not
   Cycle activation. The semantics are different enough that keeping the
   Clawket name would mislead.

3. **Decision promotion.** Clawket stores decisions as
   `type=decision, scope=rag` artifacts. In SDI, Decision is a first-class
   entity with append-only history, provenance, and a dedicated slash command
   (`/decide`). The rule is structural: Requirements carry **current truth**
   (snapshot), Decisions carry **history** (append-only). PRD §6 #7 enforces
   this on the data layer — any history trace in a Requirement body is
   rejected with `MOVE_TO_DECISION` and the Decision endpoint.

## What does not carry over

- **Tier policy (`low / med / high` on Task).** Clawket v3 carries this as
  advisory metadata; SDI v0.1 does not adopt it. If reintroduced, it lives on
  Task with the same advisory semantics. v4-style hard-enforce is not on the
  SDI roadmap.
- **Multi-user / cloud sync.** PRD §8 makes this an explicit non-goal. Local
  SQLite remains the only store.
- **Cucumber compatibility.** PRD §8 also excludes external BDD tool
  integration. The GWT syntax is SDI's, not Gherkin's.
- **`/goal` slash command.** Not migrated. PRD §6 #10 requires
  **orthogonality**: SDI must not intercept `/goal`. The absence of
  `plugin/commands/goal.md` is the contract.

## When a migration tool does ship

The shape is sketched in PRD §9 and §11:

- Read Clawket v3 SQLite (`~/.local/share/clawket/clawket.db`) directly.
- Map Plan/Unit/Cycle/Task per the table above.
- Use an LLM pass over `Task.body` to propose GWT candidates; surface them
  in a review queue, not auto-accepted.
- Convert `type=decision` artifacts to Decisions with `created_at` preserved.
- Emit a migration log under `~/.local/state/sdi/migration.log` (single audit
  channel, same as `hook.log`).

This is not on the v0.1 critical path. Self-hosting on a fresh SDI database
is the v0.1 acceptance bar (PRD §11 step 4).
