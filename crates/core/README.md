# sdi-core

**English** · [한국어](./README.ko.md)

The SDI domain model — pure types and validation, no I/O. Part of the `@scenario-driven/sdi-plugin` Rust workspace.

## What it is

`sdi-core` defines the entities the rest of the workspace stores, serves, and renders. It has no database, no HTTP, no async runtime — only `serde`, `ulid`, `chrono`, and validation logic. Every other crate depends on it; it depends on none of them.

## Entities

| Module | Type(s) |
|---|---|
| `plan` | `Plan` — approved intent (gate = ≥ 1 valid GWT scenario). |
| `requirement` | `Requirement` — SNAPSHOT-ONLY natural-language ask. |
| `decision` | `Decision` — append-only ADR (`kind ∈ {proposal, critique, consensus, dissensus}`). |
| `scenario` | `Scenario` — strict Given/When/Then, `tags`, `depends_on`, claim fields (D29). |
| `round` | `Round`, `RoundMode`, `DisruptionPolicy`, `InFlightPolicy` — R1 new / R2+ regression. |
| `autonomy_policy` | `AutonomyPolicy`, `AutonomyMode` (L3/L4/L5), `AutonomyScopeKind`. |
| `pattern` | `CollaborationPattern`, `PatternKind`, `Stance`, `ReversalPlan` + `validate_pattern_shape` / `validate_reversal_plan_json` (D26/D27/D28). |
| `task` | `Task`, `TaskEvidence`, `ScenarioEvidence` — runtime artifacts. |
| `agent_note` | `AgentNote` — M1 blackboard journal. |
| `agent_spec` | `AgentSpec`, `STOCK_AGENTS`, `STOCK_META_AGENTS` — M5 specialist registration. |
| supporting | `project`, `disruption`, `usage`, `knowledge`, `run`, `ids`, `error`. |

## Place in the workspace

```
sdi-core (this) ◀── sdi-db ◀── sdi-daemon ◀── sdi-cli / sdi-mcp
```

The seven first-class entities (D2 / D22) and the shape/reversibility validators (D26–D28) live here so the daemon and the store enforce one definition.

## Build & verify

```sh
cargo build -p sdi-core
cargo check -p sdi-core
```

Canonical spec: [`../../docs/PRD.md`](../../docs/PRD.md). Repository overview: [`../../README.md`](../../README.md).
