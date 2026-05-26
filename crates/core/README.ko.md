# sdi-core

[English](./README.md) · **한국어**

SDI 도메인 모델 — 순수 타입과 검증, I/O 없음. `@scenario-driven/sdi-plugin` Rust 워크스페이스의 일부다.

## 무엇인가

`sdi-core` 는 워크스페이스의 나머지가 저장·서빙·렌더링하는 엔티티를 정의한다. 데이터베이스도, HTTP 도, async 런타임도 없다 — 오직 `serde`, `ulid`, `chrono`, 그리고 검증 로직뿐이다. 다른 모든 크레이트가 이것에 의존하고, 이것은 어느 것에도 의존하지 않는다.

## 엔티티

| 모듈 | 타입 |
|---|---|
| `plan` | `Plan` — 승인된 의도 (게이트 = 유효한 GWT 시나리오 1개 이상). |
| `requirement` | `Requirement` — SNAPSHOT-ONLY 자연어 요구. |
| `decision` | `Decision` — append-only ADR (`kind ∈ {proposal, critique, consensus, dissensus}`). |
| `scenario` | `Scenario` — 엄격한 Given/When/Then, `tags`, `depends_on`, 클레임 필드 (D29). |
| `round` | `Round`, `RoundMode`, `DisruptionPolicy`, `InFlightPolicy` — R1 신규 / R2+ 회귀. |
| `autonomy_policy` | `AutonomyPolicy`, `AutonomyMode` (L3/L4/L5), `AutonomyScopeKind`. |
| `pattern` | `CollaborationPattern`, `PatternKind`, `Stance`, `ReversalPlan` + `validate_pattern_shape` / `validate_reversal_plan_json` (D26/D27/D28). |
| `task` | `Task`, `TaskEvidence`, `ScenarioEvidence` — 런타임 아티팩트. |
| `agent_note` | `AgentNote` — M1 블랙보드 저널. |
| `agent_spec` | `AgentSpec`, `STOCK_AGENTS`, `STOCK_META_AGENTS` — M5 전문가 등록. |
| 보조 | `project`, `disruption`, `usage`, `knowledge`, `run`, `ids`, `error`. |

## 워크스페이스에서의 위치

```
sdi-core (이것) ◀── sdi-db ◀── sdi-daemon ◀── sdi-cli / sdi-mcp
```

7대 1등 엔티티(D2 / D22) 와 형상/가역성 검증기(D26–D28) 가 여기에 살아서, 데몬과 저장소가 하나의 정의를 강제한다.

## 빌드 & 검증

```sh
cargo build -p sdi-core
cargo check -p sdi-core
```

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md). 저장소 개요: [`../../README.md`](../../README.md).
