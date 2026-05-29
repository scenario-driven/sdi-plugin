# (가칭) 시나리오 엔진 — PRD

> 대상 독자: 2년차 경력 개발자. 이 문서는 신규 도구의 요구사항·아키텍처·인수기준을 다룬다. 기획 배경은 별도 기획서(`scenario-engine-proposal.md`) 참조.

---

## 0. 메타

- **상태**: Draft (팀 의견 수렴 중)
- **선행 도구**: Clawket v3.0 (지난 약 1개월 운영, 본 도구의 직계 선조)
- **이름**: 미정 (가칭 "시나리오 엔진")
- **새 GitHub 조직**: 미정 (이름 결정 후 생성)
- **타깃 사용자**: LLM 코딩 에이전트(1차) / 사람 개발자(보조)

---

## 1. 문제 정의

### 1.1 현재 상태 (Clawket v3.0)

Clawket v3.0은 LLM 네이티브 작업 관리 + 로컬 RAG 도구다. 워크플로우 계층은 `Project → Plan(approve) → Unit → Task(backlog) → Cycle(activate) → Start` 이고, 로컬 SQLite + Rust 데몬 + CLI + MCP 서버 + 웹 대시보드로 구성된다.

운영하면서 다음 한계가 명확해졌다.

1. **Plan/Unit/Task/Cycle 4계층이 Jira 의 인간 중심 모델을 그대로 답습.** Unit 은 grouping 외 역할이 없고, Cycle 은 스프린트 메타포가 LLM 협업 흐름과 맞지 않는다.
2. **Task 의 "done" 기준이 자유 형식 evidence string.** 강제력은 있지만(EVIDENCE_REQUIRED), evidence 내용 자체의 형식이 자유로워 회귀 검증을 자동화할 단위가 없다.
3. **회귀(이전 작업이 새 작업에 의해 깨지는 현상) 검증은 사람이 수동으로 트리거.** LLM 의 sub-agent 병렬 실행 능력을 못 쓰고 있다.
4. **`scope=rag` artifact 가 결정 기록·시나리오·문서 모두를 수용.** 시맨틱 검색은 잘 되지만, "이 결정의 영향을 받는 시나리오 N개" 같은 구조적 추적은 안 된다.

### 1.2 풀려는 핵심 문제

- LLM 이 자연어로 적힌 동작 규약(시나리오) 을 1등 시민으로 다루며 코드 생성·검증·회귀 점검을 자율 수행한다.
- 사람은 자유 자연어로 요구를 던지고, LLM 이 정형 자연어(Given/When/Then) 로 정돈해 DB에 적재한다.
- 다음 작업 시작 시점에 이전 시나리오 전부를 자동 회귀 점검하며, 영향 시나리오는 사람 확인 게이트를 거친다.

### 1.3 비교 대상 / 포지셔닝

| 대안 | 한계 | 본 도구의 차별 |
|---|---|---|
| Jira/Linear/Notion | 인간 중심 Task. LLM 검증 가능한 단위 부재. | LLM 검증 가능한 GWT 시나리오를 1등 시민화. |
| TDD | 검증 단위가 코드라 자연어 ↔ 의도 갭이 사람 머리에 있음. | 검증 단위가 자연어. LLM 이 직접 읽음. |
| BDD (Cucumber 등) | Gherkin 컴파일 단계 필요. step definition 유지보수 비용. | 컴파일 단계 제거. LLM 이 자연어 GWT 를 직접 실행 가이드로 사용. |
| Claude Code `/goal` | 단발성 명령. 영속 상태 / 회귀 점검 / 다중 시나리오 관리 부재. | 영속 상태 + 회귀 점검. `/goal` 과 직교(orthogonal) 공존. |
| Clawket v3.0 (선행) | Task 중심·자유 evidence·수동 회귀. | 시나리오 중심·GWT 강제·자동 회귀. |

---

## 2. 핵심 결정 사항 (Decisions)

설계 시점에 확정된 29개 결정. 본 PRD 의 모든 요구사항은 이 결정들 위에 서 있다.

| # | 영역 | 결정 |
|---|---|---|
| D1 | 정체성 | 본 도구는 시나리오 주도 구현(Scenario-Driven Implementation) 엔진이다. TDD/BDD 의 LLM 시대 후속. |
| D2 | 1등 시민 | Plan / Requirement(스냅샷) / Decision(append-only ADR) / Scenario(GWT) / Round(R1, R2, …) / AutonomyPolicy 6종. |
| D3 | 런타임 산출물 | Task. LLM 이 시나리오·요구사항을 보고 런타임에 자율 분해. 사람이 직접 만들지 않는다. |
| D4 | 제거 | Unit 제거(시나리오의 string tag 로 격하). Cycle → Round 로 개명 + 의미 재정의. |
| D5 | GWT 강제 | Given/When/Then 형식 강제. 자유 형식 옵션 없음. 자연어 → GWT 변환은 LLM 보조. |
| D6 | Round 모드 | 기본값: strict-regression(이전 모든 R 의 모든 시나리오 재실행). 옵션: forward-only. |
| D7 | 신규 개발과 회귀 검증 흐름 통합 | 두 모드는 같은 엔진. R1 = 신규 개발 모드, R2+ = 회귀 검증 흐름 모드. 분기 없음. |
| D8 | Plan approve 게이트 | 시나리오 완비 필수. Task 0개여도 승인 가능(런타임에 LLM 이 생성). |
| D9 | Disruption 정책 | 기본: needs-review(사람 확인). 옵션: auto(LLM 자율, but 모두 confirm 필요). |
| D10 | In-flight Task 정책 | 진행 중 Task 가 있는 채로 새 Round 시작 요청 시 기본 pause. 옵션: abort / continue-on-noimpact. |
| D11 | 슬래시 명령 | `/scenario`, `/round`, `/plan`, `/req`, `/decide` 신설. `/goal` 은 Claude Code 내장으로 보존 + 직교 공존. |
| D12 | 문서 정책 | SNAPSHOT-ONLY 본문(이력 흔적 금지). 변경 이력은 Decision artifact 전용. |
| D13 | Multi-agent 본체 | Multi-agent orchestration 이 본체. 단일 @main 1인극 패턴은 anti-pattern. 모든 신규 flow 는 multi-agent 협업 가능성을 first-class 로 고려. |
| D14 | AutonomyPolicy 1등 시민 | AutonomyPolicy 가 6번째 1등 시민 entity. per scope (plan/decision-kind/global) 의 autonomy mode 를 영구 저장 + Decision 의 게이트로 작동. |
| D15 | 패턴 내장 / A2A 제외 | AWS 4 패턴(Workflow / Graph / Swarm / Agents-as-Tools) 을 내부에 내장. 외부 agent 통합 프로토콜(A2A) 은 v1 범위 제외 — Claude Code 내부 sub-agent 간 협업으로 시작. |
| D16 | 게이트 디폴트 | "default = ask" 거부. "default = act with policy" 채택 — 사전 정의된 autonomy policy 에 따라 자동 실행, 사용자는 중간 개입 구멍만 토글. L4 ↔ L5 전환 가능. |
| D17 | Mode 디폴트 | 신규 plan default = L5(즉시 적용 + 사후 evidence). external surface(publish/deploy/external API) plan default = L4(notify + timed auto-apply). decision-kind ∈ {architecture, schema, naming-canonical} 는 L4 강제. |
| D18 | Circuit breaker | 사용자는 언제든 즉시 모든 autonomy mode 를 L3(always ask) 로 강등 가능. 트리거는 UI 단일 액션, 즉시 발효. inflight 결정은 next gate 에서 적용. |
| D19 | Substrate 항시 작동 | M1~M5(§5 Layer 2.5) 는 autonomy mode 와 독립적으로 항상 작동. autonomy mode 는 합의된 결정의 사용자 게이트 위치만 통제. 단순 task delegation 만 일어나는 패턴은 anti-pattern. |
| D20 | Consensus 게이트 | 단일 agent 결정 = L3 max(항상 사용자 확인). multi-agent consensus(≥2 agent 일치) = autonomy mode 에 따라 L4/L5 unlock. dissensus(결정 충돌) = mode 무관 항상 사용자 escalation. |
| D21 | Delegation 강제 | 메인 세션은 plan/decompose/dispatch 만. execution 도구(`Edit`/`Write`/`NotebookEdit`/mutating `Bash`) 는 PreToolUse hook 에서 차단 → specialist sub-agent 위임 강제. D13 의 메커니컬 enforcement 면. |
| D22 | CollaborationPattern 1등 시민 | 7번째 1등 시민 entity. `kind` ∈ {workflow, graph, swarm, agents-as-tools, direct}, `applies_to` ∈ {plan, requirement, scenario, task, decision, round}. lifecycle = pending → active → converged \| dissensus \| aborted. AWS 4패턴 (D15) 의 entity 표현. |
| D23 | Pattern provenance | 모든 work entity (plan/requirement/scenario/task/decision/round) 는 `produced_via_pattern_id` 컬럼을 보유 (nullable FK → collaboration_pattern). `direct` 패턴은 메인 1인극 명시 마커 — anti-pattern 인지 가능. |
| D24 | Pattern 재귀 | `parent_pattern_id` 자기참조 FK + depth cap (default 3). DAG 만 허용 (cycle 차단). 한 패턴이 sub-pattern 을 spawn 가능 — 패턴 자체가 또 다른 패턴의 산출물이 될 수 있음. |
| D25 | Pattern-scoped autonomy | AutonomyPolicy.scope_kind 에 `pattern_kind` 추가. workflow/graph/swarm/agents-as-tools 별로 L0~L5 독립 설정. 패턴 별 game-theoretic 안전성 차이를 정책으로 반영 (e.g., swarm 기본 L4, workflow 기본 L5). |
| D26 | 4-pattern integrity gates | PreToolUse hook 이 active pattern 의 kind 별로 검증 강제: Workflow(선행 step 미완료 차단 + step ≥ 2 shape), Graph(consensus 미달 시 decision.apply 차단; distinct **(agent_type, stance)** ≥ 2 — sybil 차단), Swarm(spawn depth 초과 / self-spawn loop / fan_out ≥ 2 shape 차단), Agents-as-Tools(peer 미등록 tool 호출 / peer ≥ 1 shape 차단). D21 의 패턴별 확장 면. |
| D27 | Pattern shape & selection 게이트 | 새 work entity 생성 시 `produced_via_pattern_id` NOT NULL 강제 (마이그레이션 row 만 NULL 허용). `pending → active` 전이 시 pattern shape validation (D26 의 step/distinctness/fan_out/peer 기준) 통과 강제. `direct` 는 shape 미통과 escape 가 아니라 명시적 1인극 마커 (자동 L3 cap + 대시보드 빨간 배지). 1-step workflow 같은 가짜 패턴은 D26 shape 게이트가 pending 단계에서 거부. |
| D28 | Reversibility 1등 시민 | 모든 Decision 은 `reversal_plan` (inverse migration / git revert SHA / fs snapshot ref) + `blast_radius_score` (kind 별 정적 점수: architecture=10, schema=8, naming=4, doc=1) 보유 강제. L5 unlock 의 추가 조건 = (a) pattern shape valid (D26/D27) AND (b) reversal_plan present AND (c) blast_radius_score ≤ AutonomyPolicy.l5_threshold (default 5). L5 의 실 병목이 합의 메커니즘이 아니라 "틀린 결정의 회복 비용" 이라는 진단의 직접 대응. |
| D29 | Multi-session resource claims | Scenario 는 `claimed_resources` (path glob 배열) 필드 보유. PreToolUse hook 이 매 Edit/Write 호출 시 daemon 에 query → 다른 active scenario 의 claim 과 overlap 시 차단 + 사용자 prompt ("merge or wait"). Plan-level advisory lock 도 옵션. 2 main session 이 같은 plan 의 다른 scenario 에서 동시 작업 시 같은 파일을 모순되게 변경하는 race 차단. daemon-centric 의 multi-session 지원이 storage 일관성에서 멈추지 않고 의사결정 일관성까지 확장. |

용어 풀이:

- **GWT** = Given/When/Then (상황·행동·결과). BDD 에서 유래한 시나리오 정형 형식.
- **Round** = 한 회차의 구현+검증 단위. R1 은 첫 구현, R2+ 는 이전 회차 시나리오 회귀를 포함한 검증.
- **Disruption** = 새 시나리오/요구사항/결정이 기존 시나리오를 무효화/수정해야 하는 상황.
- **In-flight Task** = 어떤 시나리오에 대해 LLM 이 이미 실행 중인 작업.

D13~D29 은 multi-agent 협업 결정군. 본문 정의는 아래에 등재.

### D13. Multi-agent orchestration is the body

multi-agent orchestration 이 부산물이 아니라 본체. 단일 @main 1인극 패턴은 anti-pattern. 모든 flow 는 multi-agent 협업 가능성을 first-class 로 고려한다. 진짜 multi-agent 협업은 agent 끼리의 정보 공유 · 협상 · 합의 · 분기 결정을 포함한다 — sub-agent 를 단순 "task delegation (위임)" 으로만 쓰는 패턴은 anti-pattern.

### D14. AutonomyPolicy 1등 시민

**AutonomyPolicy** 는 1등 시민 entity (Plan / Requirement / Decision / Scenario / Round / AutonomyPolicy) 의 일원이며, per scope (plan / unit / decision-kind) 의 autonomy mode 를 영구 저장하고 Decision 의 사용자 게이트 위치를 통제한다.

### D15. AWS 4패턴 내장, A2A v1 제외

AWS 의 4가지 multi-agent 패턴 — **Workflow**(고정 순서), **Graph**(DAG 기반 의존), **Swarm**(peer 무계층), **Agents-as-Tools**(다른 agent 를 도구로 호출) — 을 SDI 내부 first-class 패턴으로 내장. 외부 agent 통합 프로토콜 (A2A, Agent-to-Agent) 은 v1 범위에서 제외 — Claude Code 내부 sub-agent 간 협업으로 시작한다.

### D16. Act-with-policy 게이트

"default = ask (모든 결정마다 사용자 확인)" 패턴 거부. **"default = act with policy (사전 정의된 autonomy policy 에 따라 자동 실행, 사용자는 중간 개입 구멍만 토글)"** 채택. L5 를 목표로 하되 중간중간 사용자 개입 구멍을 만들어 두고 on/off 로 L4/L5 전환이 가능한 워크플로우. 사용자가 매 결정마다 prompt 를 주입하지 않아도 진행되는 것이 핵심 가치 명제 (= "사용자가 PC 앞에 묶이지 않음").

### D17. Mode 디폴트 (L5 신규 / L4 외부 노출 / L4 강제 kind)

- **신규 plan default = L5** (immediate apply + evidence only) — 완전 신규 개발 사이드는 L5 막을 이유가 없다.
- **External surface (publish / deploy / external API) 보유 plan default = L4** (notify + timed auto-apply, 사용자가 timeout 내 거부 가능).
- **decision-kind ∈ {architecture, schema, naming-canonical} 는 L4 강제** (mode 무관). 되돌리기 비용이 큰 결정군은 합의 후에도 사용자 검토 게이트가 필요하다.

### D18. Circuit breaker 항시 활성

사용자는 언제든 즉시 모든 autonomy mode 를 **L3 (always ask)** 로 강등 가능. 트리거는 UI 단일 액션. 강등은 즉시 발효되며, 이미 in-flight 한 결정은 다음 게이트에서 적용된다. autonomy 가 너무 자율적이라는 사용자의 직관이 들면 즉시 사용자 손에 통제권이 돌아오는 안전 장치.

### D19. Agent communication substrate (M1~M5) 가 mode 무관 항상 작동

§5 Layer 2.5 의 M1~M5 (Blackboard / Peer Hand-off / Negotiation / Scenario-as-Contract / Self-organization) 는 autonomy mode 와 독립적으로 항상 작동한다. autonomy mode 는 **합의된 결정의 사용자 게이트 위치만** 통제하며, agent 간 소통 자체는 어떤 mode 에서도 차단되지 않는다. 단순 task delegation 만 일어나고 substrate 가 죽은 채로 진행되는 패턴은 anti-pattern.

### D20. Consensus / Dissensus 가 autonomy gate 의 단위

- **단일 agent 결정 = L3 max** — 단독 의견은 mode 무관 항상 사용자 확인.
- **Multi-agent consensus** (≥ 2 agent 일치) = autonomy mode 에 따라 L4 / L5 자동 적용 unlock 가능.
- **Dissensus** (agent 간 결정 충돌) = mode 무관 즉시 사용자 escalation.

autonomy 의 자율성은 "합의 후 게이트" 단위이지 "단일 의견" 단위가 아니다. consensus / dissensus 의 entity 표현은 §3 의 Decision.kind 필드로 처리한다.

### D21. Mandatory Delegation Gate — execution 은 항상 specialist sub-agent

orchestrator (메인 세션) 는 **plan / decompose / dispatch** 만 수행한다. 실제 코드·문서 변경, mutating shell, 외부 부수효과는 **반드시 Layer 2 specialist sub-agent 에 위임** 해야 한다. 메인 세션이 execution 도구를 직접 호출하면 PreToolUse hook 이 거부한다.

- **메커니컬 enforcement**: PreToolUse hook 에서 `hookInput.agent_id` 부재 = 메인 세션 → execution 도구 (`Edit`, `Write`, `NotebookEdit`, mutating `Bash`) 차단. `agent_id` 존재 = Agent 도구로 spawn 된 sub-agent → 통과. 신호는 Claude Code 공식 hook contract (`code.claude.com/docs/en/hooks`).
- **read-only / planning 도구는 항상 허용**: `Read`, `Grep`, `Glob`, `WebSearch`, `WebFetch`, `Agent`, `TaskCreate/Update/List`, `SendMessage`, `ScheduleWakeup`, MCP read-only — 메인이 분해 / 위임 수행에 필요.
- **read-only Bash 허용**: `git status`, `cargo check`, `pnpm typecheck` 등 부수효과 없는 명령. 화이트리스트 매칭 통과 시 메인도 직접 실행 가능. 매칭 실패 = 차단 → specialist 위임 필요.
- **specialist 정합성**: sub-agent 의 `agent_type` 은 AgentSpec (§3.8) 에 등록된 specialist 이름과 일치해야 한다. 미등록 type 으로 spawn 된 sub-agent 의 execution 도 차단 (rogue agent 방지).
- **L3 (circuit breaker / 신중 모드) 우회**: Layer 3 circuit breaker 가 트리거되면 메인에 한해 임시로 차단 해제 (사용자가 직접 통제 모드). 단 모든 차단 해제 호출은 `audit=manual-override` 로 활동 로그에 적재.
- **emergency bypass**: 권장 surface 는 `sdi bypass arm --reason "<짧은 사유>"` — daemon-친화 CLI 가 `~/.cache/sdi/bypass-once` 에 JSON 마커(`{reason, armed_at, expires_at, ttl_seconds}`) 를 쓴다. 한 마커가 변경성 PreToolUse 게이트 전체(D21 위임, 활성 태스크, D29 클레임 겹침) 를 다음 한 번의 도구 호출 동안 해제하고 hook 이 honor 직전 파일을 삭제. TTL 기본 60초 (`--ttl <초>`), 만료 마커는 정리만 되고 게이트는 열지 않음. `sdi` 는 D21 read-only Bash 화이트리스트에 있어 메인 세션이 직접 무장 가능 — 우회 substrate 가 우회를 다시 위임에 가두는 self-deadlock 을 구조적으로 차단. `sdi bypass status` 로 상태 / TTL 잔여 / reason 확인, `sdi bypass disarm` 으로 제거(멱등). 매 무장 + 소비는 stderr 경고 + 게이트별 audit 이벤트(`pre_tool_use_delegation_bypass`, `pre_tool_use_active_task_bypass`, `pre_tool_use_claim_bypass`) 로 적재. 본 surface 도달이 불가능한 환경(예: 셸 rc 에 export 가 강제된 CI) 용 startup-time fallback: `SDI_DELEGATION_BYPASS=1` env-var — Claude Code 를 해당 env 가 export 된 셸에서 새로 띄울 때만 작동. 인라인 `VAR=1 cmd` 프리픽스는 hook spawn 전에 expand 되지 않아 닿지 않음. routine bypass 는 protocol violation — audit 대상.

D21 은 D13 ("multi-agent orchestration is the body, single-@main solo flow is anti-pattern") 의 메커니컬 enforcement 면이다. D13 이 문서 규약, D21 은 그 규약을 런타임 게이트로 승격.

### D22. CollaborationPattern 1등 시민 (7번째 entity)

D15 가 AWS 4 패턴을 "내장 개념" 으로 박았다면, D22 는 그 패턴을 **DB 의 영구 row** 로 격상한다. 매 work entity (plan/scenario/task/...) 가 어떤 패턴 위에서 산출됐는지 추적 가능.

- **스키마**: `CollaborationPattern { id, short_code, kind, applies_to, scope_id, lifecycle, parent_pattern_id, depth, ... }` — 상세 §3.9.
- **`kind`**: `workflow` (고정 순서 step) / `graph` (DAG 의존) / `swarm` (peer 무계층) / `agents-as-tools` (1 agent 가 다른 agent 를 도구로 호출) / `direct` (메인 1인극 — anti-pattern 명시 표기).
- **`applies_to`**: 어떤 entity 종류에 적용되는 패턴인지. `plan` / `requirement` / `scenario` / `task` / `decision` / `round` — 패턴은 entity 크기와 무관하게 적용 가능 (사용자 thesis: "지라의 에픽 / 스토리 / 태스크 / 하위 태스크처럼 SDI 의 모든 작업 크기에 패턴 적용").
- **lifecycle**: `pending` (생성됐으나 미시작) → `active` (step 진행 중) → `converged` (모든 step 합의 + 산출물 적재) / `dissensus` (합의 실패, Layer 0 escalate) / `aborted` (사용자 또는 circuit breaker 종료).

D22 의 의의: 패턴이 코드 상수가 아니라 데이터로 존재해야 — (a) entity 별 추적, (b) 런타임 게이트 (D26), (c) 사후 분석 (어떤 패턴이 어떤 entity 에서 잘 작동했는가) 이 모두 가능해진다.

### D23. Pattern provenance — 모든 work entity 가 출처 패턴 기록

모든 work entity 의 `produced_via_pattern_id` 컬럼이 그 entity 의 산출 경로를 기록한다. NULL 은 v0.4 이하 마이그레이션 row (legacy) 만 허용; v0.5 이후 신규 entity 는 NOT NULL.

- **컬럼 추가 대상**: `plan` / `requirement` / `scenario` / `task` / `decision` / `round` 6 테이블.
- **NULL 허용 정책**: 마이그레이션 시점에 존재한 row 는 NULL 허용. 신규 row 는 D27 게이트가 NOT NULL 강제 (단, `direct` 패턴 row 를 자동 부여하는 escape — anti-pattern 마커가 명시 표기되도록).
- **`direct` 의 의미**: 메인 세션이 1인극 (= solo flow, D13 anti-pattern) 으로 entity 를 만든 케이스. UI / 대시보드는 `direct` row 를 **명시적으로 빨간 배지** 로 표기 — 안티패턴이 보이지 않게 묻히지 않게.

D23 은 D22 의 entity 화의 자연 귀결: 패턴이 데이터로 존재하면, work entity 는 어느 패턴에서 나왔는지 기록해야 한다. 기록이 없으면 D26 의 런타임 게이트가 작동할 근거가 사라진다.

### D24. Pattern 재귀 — 패턴이 sub-pattern 을 spawn

`parent_pattern_id` (self-FK) 가 패턴 트리를 형성. 한 패턴의 step 이 또 다른 패턴을 호출 가능 — 예: workflow 의 step 3 이 "swarm 으로 합의 형성 후 다음 step 진행". 재귀 깊이는 `depth` 컬럼 + AutonomyPolicy 의 `pattern_depth_cap` (default 3) 으로 제한.

- **DAG 강제**: `parent_pattern_id` 순환 차단. 자식 패턴이 자기 조상 패턴을 parent 로 가질 수 없음 (daemon 가 topological sort 로 검증).
- **depth cap 의 의미**: 무한 spawn 루프 방지. 3 단계 깊이는 "전체 plan = workflow / plan 의 한 step = swarm / swarm 의 한 step = agents-as-tools" 까지 표현 가능 — 사용자가 실제 사용 데이터로 한계가 부족하면 AutonomyPolicy 로 상향 (단 5 초과는 circuit breaker 의 dissensus escalate 후보).
- **`produced_via_pattern_id` 와의 관계**: 한 entity 의 `produced_via_pattern_id` 는 그 entity 를 직접 산출한 패턴 — sub-pattern 일 수 있음. 조상 패턴 추적이 필요하면 pattern 자체의 `parent_pattern_id` chain 을 따라간다.

D24 의 의의: 사용자의 thesis ("플랜을 짤 때 메인이 바로 하위 구조로 생성할 수도 있고, 각각 하위에게 전달할 수도 있고, 이 플랜 자체를 다른 협업 패턴으로 사전 논의한 다음에 짤 수도 있고") 을 entity 모델로 표현. 패턴 자체가 또 다른 패턴의 산출물이 될 수 있는 구조.

### D25. Pattern-scoped autonomy — 패턴 종류 별 게이트 위치 분리

AutonomyPolicy.scope_kind 에 `pattern_kind` 추가. workflow / graph / swarm / agents-as-tools 각각의 L0~L5 기본값을 독립 설정.

- **신규 기본값 (v0.5)**:
  - `workflow` → L5 (선행 step 검증이 강하므로 자동 진행 안전)
  - `graph` → L5 (consensus 게이트가 강함 — distinct agent_id ≥ 2)
  - `swarm` → L4 (peer 무계층의 수렴 보장이 약함 — sub-agent 자가 종료 신호 필요)
  - `agents-as-tools` → L4 (다른 agent 를 도구로 호출하는 비대칭 권한 — 외부 부수효과 가능성 높음)
  - `direct` → L3 강제 (D27 의 자동 demotion)
- **scope_kind='pattern_kind' 의 우선순위**: plan-level mode 와 충돌 시 더 엄격한 쪽 우선 (L3 > L4 > L5). 사용자가 plan = L5 로 설정해도 swarm 기본 L4 가 적용 — 단 명시적으로 plan 의 swarm policy 를 L5 로 override 시 그 plan 한정 적용.
- **circuit breaker (D18) 와의 관계**: circuit breaker 발동 시 모든 pattern_kind policy 도 L3 로 일괄 강등.

D25 의 의의: 패턴 별 game-theoretic 안전성 차이를 정책으로 반영. swarm 처럼 합의 보장이 약한 패턴은 사용자 게이트를 한 칸 더 둠 — autonomy 의 자율성이 패턴 종류 별로 차등 적용된다.

### D26. 4-pattern integrity gates — PreToolUse hook 의 패턴별 검증

D21 의 delegation gate 위에 **active pattern 의 kind 별 무결성 검증**을 추가한다. 패턴이 entity 로 존재하므로 (D22), 매 도구 호출 시 active pattern 을 조회해 패턴 규칙 위반을 차단 가능.

- **Workflow gate**: pattern.kind='workflow' 이면 (a) `steps_json` 의 길이 ≥ 2 강제 (shape validation — pending → active 전이 시 검증), (b) 선행 step 의 산출물 (Decision/Task evidence) 이 적재되지 않으면 후행 step 의 sub-agent execution 차단. 1-step 워크플로는 pending 단계에서 거부 — 가짜 패턴으로 D27 우회 시도 차단.
- **Graph gate** — sybil 차단 설계:
  - consensus 형성 시점까지 decision.apply (Decision.kind='consensus' 적재) 차단.
  - distinctness 단위 = **(AgentSpec.name, AgentSpec.stance) tuple**. 단순 `distinct agent_id` 가 아님 — 같은 `impl-coder` 인스턴스 2개는 agent_id 가 달라도 같은 system prompt / 컨텍스트 / 결론 → "독립된 판단" 보장 0. tuple distinctness 가 진짜 다양성 신호.
  - 신규 AgentSpec 컬럼: `stance` ∈ {proposer, devil_advocate, schema_guardian, performance_reviewer, security_reviewer, neutral} — 다양한 시각을 명시 분리. 1 AgentSpec.name 이 여러 stance 인스턴스를 가질 수 있고, consensus 게이트는 (name, stance) tuple ≥ 2 distinct 강제.
  - `graph.reviewers_json` (pattern row 의 매니페스트) — 패턴 생성 시점에 어떤 (name, stance) 가 참여하는지 미리 등재. 런타임 distinctness 와 매니페스트 둘 다 검증.
- **Swarm gate**:
  - spawn depth 검증 (parent_pattern_id chain depth ≤ AutonomyPolicy.pattern_depth_cap).
  - 자기 spawn (sub-agent X 가 다시 sub-agent X 를 spawn) 시 차단 — self-spawn 루프 방지.
  - `fan_out_json` ≥ 2 강제 (pending → active 전이 시 검증). 1 sub-agent swarm 은 swarm 이 아니라 direct.
- **Agents-as-Tools gate**:
  - 호출되는 agent 가 caller agent 의 `peer_registration_json` 에 등재돼야 함.
  - `peer_registration_json` 길이 ≥ 1 강제. 0 peer 는 패턴 위반.

D26 의 의의: D21 이 "메인 vs sub-agent" 의 차단이라면, D26 은 "**패턴 안에서의 행동 규칙**" 의 차단. **sybil 차단은 핵심 — agent_id 둘은 sender 다양성이지 판단 다양성이 아니다**. shape validation 은 1-step / 1-agent 가짜 패턴이 D27 우회 escape 로 작동하지 못하도록 보장.

### D27. Pattern shape & selection 게이트 — entity 생성 + 전이 시 진짜 패턴 강제

새 work entity 생성 시 `produced_via_pattern_id` NOT NULL 강제. 마이그레이션 시점의 legacy row 만 NULL 허용 — 신규 생성은 무조건 패턴 ID 필요.

- **생성 시점 (D27a)**: plan create / scenario add / task create / decision append / round start / requirement snapshot 6 entry point 에서 `produced_via_pattern_id` 부재 시 거부. 메인 세션이 명시 없이 entity 생성을 시도하면 daemon 이 자동으로 `kind='direct'` CollaborationPattern row 를 만들고 그 ID 부여. **`direct` 의 의미는 escape 가 아니라 명시적 1인극 마커** — 자동 L3 cap + 대시보드 빨간 배지 + activity log audit.
- **전이 시점 (D27b)**: CollaborationPattern.lifecycle `pending → active` 전이 시 D26 shape validation 강제 통과. shape 미통과 (workflow steps < 2, graph (name,stance) distinct < 2, swarm fan_out < 2, agents-as-tools peer = 0) 시 `pending` 에 머무름 — active 전이 거부. 즉 "shape 가짜 패턴" 은 active 가 되지 못해 D26 의 L4/L5 unlock 도 받지 못함.
- **escape 없음**: 가짜 패턴 (1-step workflow, 동일 specialist 2개 graph, 1-agent swarm) 으로 `direct` 의 L3 demotion 회피 불가. 진짜 패턴 (shape valid) 만 L4/L5 unlock 자격. 메인이 진짜 자율 진행을 원하면 진짜 협업 패턴 구성 필수.

D27 의 의의: D22~D26 의 entity / 정책 / gate 가 실제로 작동하려면 entity 생성 + 전이 시점 둘 다에서 게이트 필요. **생성 게이트는 NOT NULL 강제 + `direct` 마커, 전이 게이트는 shape validation** — 두 게이트의 결합으로 가짜 패턴 회피 경로가 닫힌다. ceremony 부담은 NOT NULL 의 자동 escape (`direct`) 가 흡수하되, 그 escape 의 비용 (L3 cap + 빨간 배지) 이 명시 부담된다.

### D28. Reversibility 1등 시민 — L5 의 진짜 병목 해소

L5 자율 (사용자 부재 자동 적용) 의 실제 병목은 합의 메커니즘이 아니라 **틀린 결정의 회복 비용**이다. Decision 이 잘못 적용됐을 때 자동 rollback 경로가 없으면 L5 는 위험 — 사용자가 자리를 비울 수 없다.

- **Decision 신규 컬럼**:
  - `reversal_plan` (JSON): inverse migration SQL / git revert SHA / fs snapshot ref / 외부 호출 compensating action. apply 직전 자동 생성 (impl-coder + schema-architect 협업) + decision-resolver 가 critique.
  - `blast_radius_score` (INTEGER, 0~10): kind 별 정적 점수. architecture=10, schema=8, naming-canonical=4, impl-internal=3, doc-only=1. AgentSpec 이 자체 수정 가능 (decision-kind 추가 시 자동 점수 부여 룰).
- **L5 unlock 의 신규 조건 (D14/D17 게이트 위에 추가)**:
  - (a) active pattern 이 D26/D27 shape valid
  - (b) `reversal_plan` 이 NOT NULL + 형식 valid (인수 기준 #19)
  - (c) `blast_radius_score` ≤ AutonomyPolicy.l5_threshold (default 5)
  세 조건 모두 충족 시에만 L5 자동 apply. 하나라도 미충족 시 L4 게이트 (timed auto-apply) 로 강제 강등.
- **rollback 실행 경로**:
  - 사용자가 대시보드의 "rollback this decision" 트리거 → daemon 이 `reversal_plan` 을 reversal-runner specialist 에 dispatch.
  - rollback 자체도 Decision (kind='consensus', reversal_of=<원 decision id>) 으로 적재 — append-only 보존 (D12).
  - rollback 실패 시 dissensus escalate (mode 무관 사용자 게이트, D20).

D28 의 의의: D22~D27 이 "결정이 어떻게 만들어졌는가" (process) 를 강화한다면, D28 은 "결정이 틀렸을 때 어떻게 되돌리는가" (reversibility) 를 강화. 사용자가 "도구를 두고 자리를 비워도 망치지 않는다" 라는 확신이 L5 의 실 가치 명제 — 이 확신은 reversibility 보장에서 온다.

### D29. Multi-session resource claims — daemon-centric multi-session 의 의사결정 일관성

daemon-centric 아키텍처가 storage 일관성은 SQLite ACID 로 보장하지만, **의사결정 일관성** (두 main session 이 같은 자원에 모순 변경) 은 별도 메커니즘 필요. Scenario 가 자원 claim 의 단위.

- **Scenario 신규 컬럼**:
  - `claimed_resources_json` (path glob 배열): 예 `["crates/db/migrations/*.sql", "plugin/agents/*.md"]`. 시나리오가 작업할 파일 / 디렉터리 / 모듈을 명시.
  - `claim_status` ∈ {none, requested, active, released}. scenario `confirmed` 진입 시 `requested` → daemon 이 다른 active claim 과 overlap 검사 → overlap 0이면 `active` 부여.
- **PreToolUse hook 의 신규 검증** (D26 4-gate 다음 단계):
  - 매 Edit/Write/MultiEdit 호출 시 path 추출 → daemon `/scenarios/active-claims` query → 호출 agent 의 active scenario 가 path 를 claim 하고 있어야 함.
  - 다른 scenario 의 claim 과 overlap (같은 path 를 두 scenario 가 동시 claim 시도) → 차단 + 사용자 prompt ("scenario A vs scenario B: merge or wait"). 사용자 결정 전까지 두 scenario 모두 `active` 진입 차단.
- **Plan-level advisory lock (옵션)**:
  - AutonomyPolicy 에 `plan_single_session_lock` (bool, default false). true 면 1 plan 은 1 session 만 active scenario 보유. 동시 작업 완전 차단 — 사용자가 명시 활성화.
  - default false 인 이유: multi-session 협업은 다수 사용자의 자연스러운 패턴. 강제 lock 은 ceremony.

D29 의 의의: "항상 세션이 1개이어야 하나요?" 라는 사용자 질문에 "아니오 — 단 자원 claim 으로 일관성 보장" 으로 답하는 entity 모델. daemon 이 의사결정 라우터로 작동 — 같은 plan 의 N session 이 같은 daemon 을 거치므로 race 검출 가능. 이 메커니즘 없이는 daemon-centric multi-session 은 storage 만 공유하는 N 개의 독립 1인극이 된다.

---

## 3. 모델

### 3.1 엔티티

```
Plan                            (produced_via_pattern_id → CollaborationPattern)
 ├─ Requirement[]               (snapshot, produced_via_pattern_id)
 ├─ Decision[]                  (append-only, ADR, kind ∈ {proposal,critique,consensus,dissensus};
 │                               reversal_plan + blast_radius_score + produced_via_pattern_id)
 ├─ Scenario[]                  (GWT, 1등 시민; produced_via_pattern_id;
 │   │                           claimed_resources_json + claim_status — D29 resource claim)
 │    ├─ tag[]                  (구 Unit 의 후신, string)
 │    └─ depends_on[]           (다른 Scenario.id, M4 scenario-as-contract DAG)
 ├─ Round[]                     (produced_via_pattern_id)
 │    ├─ R1, R2, ...
 │    └─ Task[]                 (runtime, LLM 생성, evidence chain; produced_via_pattern_id)
 ├─ AutonomyPolicy[]            (per scope/decision-kind/pattern_kind autonomy mode 저장)
 ├─ CollaborationPattern[]      (D22 7번째 1등 시민 — kind {workflow|graph|swarm|agents-as-tools|direct};
 │                               parent_pattern_id self-FK + depth; lifecycle pending → active → ...)
 ├─ AgentNote[]                 (M1 blackboard: agent 별 hypothesis/observation/intent journal)
 └─ AgentSpec[]                 (M5 self-organization: runtime 동적 specialist 정의;
                                 stance ∈ {proposer|devil_advocate|schema_guardian|
                                 performance_reviewer|security_reviewer|neutral} — D26 sybil-fix)
```

1등 시민 entity 는 **7종**: Plan / Requirement / Decision / Scenario / Round / AutonomyPolicy / CollaborationPattern (D2, D14, D22). AgentNote / AgentSpec / Scenario.depends_on / Decision.kind 는 multi-agent communication substrate (§5 Layer 2.5) 의 기재 형태 — 1등 시민이 아니지만 영구 entity 로 보존된다.

상태 전이:

- **Plan**: `draft → active → completed`. active 게이트 = 시나리오 ≥ 1, 각 시나리오 GWT 유효, 사람 명시 승인.
- **Round**: `planning → active → completed`. R(N) 이 active 인 동안 R(N+1) 시작 불가.
- **Task**: `todo → in_progress → done | cancelled | blocked`. `done` 전환 시 evidence 필수.
- **Scenario**: `draft → confirmed → (per-round) passing | failing | impacted | retired`. 별도 `claim_status`: `none → requested → active → released` (D29).
- **Requirement**: 상태 없음. 최신 스냅샷만 유효(SNAPSHOT-ONLY).
- **Decision**: 상태 없음. append-only. `kind` 는 합의/충돌 단계를 표기. `reversal_plan` 적재 후에만 L5 자동 apply unlock (D28).
- **AutonomyPolicy**: 상태 없음. 최신 스냅샷만 유효(SNAPSHOT-ONLY). circuit breaker 액션 (D18) 으로 즉시 모든 scope L3 강등 가능.
- **CollaborationPattern**: `pending → active → converged | dissensus | aborted` (D22). `pending → active` 전이 시 D26 shape validation 강제 통과.
- **AgentNote**: 상태 없음. append-only journal. 합의 이전의 사고 흐름.
- **AgentSpec**: `active → expired`. `expires_at` 도달 또는 명시적 폐기로 expired.

### 3.2 시나리오 스키마

```
Scenario {
  id: ULID
  short_code: e.g. SC-42
  plan_id: ref
  given: string         -- 자연어, LLM 정돈
  when:  string         -- 자연어, LLM 정돈
  then:  string         -- 자연어, LLM 정돈
  tags:  string[]       -- 구 Unit 격하분
  depends_on: ULID[]    -- 다른 Scenario.id, M4 scenario-as-contract DAG
  produced_by: string   -- M4 계약: 구현 책임 agent 식별자 (e.g. "impl-coder")
  verified_by: string   -- M4 계약: 검증 책임 agent 식별자 (e.g. "test-runner")
  produced_via_pattern_id: ULID    -- D23: 시나리오를 산출한 CollaborationPattern (NULL=legacy only)
  claimed_resources_json: string[] -- D29: path glob 배열 (예 ["crates/db/migrations/*.sql"])
  claim_status: none | requested | active | released  -- D29: 자원 claim 상태
  origin_round: R1 식별자
  status: draft | confirmed
  per_round_results: { round_id → passing | failing | impacted | retired, evidence_ref[] }
  created_at, updated_at
}
```

GWT 필드는 모두 필수, 비어 있을 수 없다. 길이 제한은 두지 않되 토큰 절약을 위해 atomic(단일 명제) 권장. `depends_on` 은 같은 plan 내 다른 Scenario.id 만 허용 — daemon 측 topological sort 로 cycle 검출 시 작성 거부 (M4 scenario-as-contract 의 DAG 무결성 강제).

**Resource claims (D29)**: `claimed_resources_json` 은 시나리오가 작업할 파일/디렉터리/모듈을 명시. scenario `confirmed` 진입 시 claim_status 가 `requested` 로 전이 → daemon 이 다른 active claim 과 overlap 검사 → overlap 0 이면 `active`. round 완료 또는 사용자 명시 release 시 `released`. PreToolUse hook 이 매 Edit/Write 호출의 path 를 daemon `/scenarios/active-claims` 에 query → 호출 agent 의 active scenario claim 범위 밖이면 차단. 다른 main session 이 같은 plan 의 다른 scenario 에서 동시 작업 가능하되 자원 충돌은 daemon 이 라우터로 차단.

### 3.3 Task 스키마

LLM 이 런타임에 생성. 사람이 만들지 않는다.

```
Task {
  id: ULID
  round_id: ref
  parent_scenario_ids: ref[]   -- 어떤 시나리오를 충족시키려는 작업인지
  parent_requirement_ids: ref[]
  description: string
  status: todo | in_progress | done | cancelled | blocked
  evidence: structured (시나리오별 합격/불합격 + 증거 ref)
  created_at, updated_at, evidence_at
}
```

### 3.4 Round 의미

- **R1** (신규 개발 모드): plan 의 모든 confirmed 시나리오에 대해 LLM 이 Task 를 생성·실행. 각 시나리오를 passing 으로 만든다.
- **R2+** (회귀 검증 흐름 모드): 이전 모든 R 에서 passing 이었던 시나리오 전부를 자동 재검증 + R(N) 에서 새로 추가/수정된 시나리오 검증. 두 흐름이 같은 엔진(D7).

Round 모드(D6):

- `strict-regression` (기본): 이전 모든 시나리오 재실행.
- `forward-only`: 이전 시나리오 중 명시적으로 retired 된 것 제외하고 재실행. 사람이 명시 선택해야 작동.

### 3.5 AgentNote 스키마 (M1 blackboard)

```
AgentNote {
  id: ULID
  scope_kind: plan | round | scenario | task
  scope_id: ref
  agent_name: string         -- "gwt-converter", "impl-coder" 등 specialist 식별자
  kind: hypothesis | observation | question | dissent | evidence | handoff
  body: string               -- markdown
  refs_json: { scenario_ids?, decision_ids?, file_paths? }
  to_agent: string | null    -- M2 peer hand-off 시 수신 agent 식별자
  receipt_acknowledged_at: integer | null  -- M2 receipt ack timestamp
  retired_at: integer | null -- M1 retirement timestamp (append-only, row 삭제 금지)
  retired_reason: string | null
  created_at
}
```

SQL DDL (시안):

```sql
CREATE TABLE agent_note (
  id                       TEXT PRIMARY KEY,
  scope_kind               TEXT NOT NULL CHECK (scope_kind IN ('plan','round','scenario','task')),
  scope_id                 TEXT NOT NULL,
  agent_name               TEXT NOT NULL,
  kind                     TEXT NOT NULL CHECK (kind IN ('hypothesis','observation','question','dissent','evidence','handoff')),
  body                     TEXT NOT NULL,
  refs_json                TEXT,
  to_agent                 TEXT,
  receipt_acknowledged_at  INTEGER,
  retired_at               INTEGER,
  retired_reason           TEXT,
  created_at               INTEGER NOT NULL
);
CREATE INDEX agent_note_scope_idx    ON agent_note (scope_kind, scope_id, created_at);
CREATE INDEX agent_note_agent_idx    ON agent_note (agent_name, created_at);
CREATE INDEX agent_note_handoff_idx  ON agent_note (to_agent, receipt_acknowledged_at) WHERE to_agent IS NOT NULL;
CREATE INDEX agent_note_active_idx   ON agent_note (scope_kind, scope_id, created_at) WHERE retired_at IS NULL;
```

agent 간 합의 이전의 사고 흐름 (M1 blackboard) 을 시간순 저장. Decision (합의된 의사결정) 과 분리. 다른 agent 는 비동기적으로 read 하여 자기 의사결정 입력으로 사용 (§5 Layer 2.5 M1).

### 3.6 Decision 스키마

```
Decision {
  id: ULID
  short_code: e.g. DEC-7
  plan_id: ref
  scope_kind: plan | scenario | requirement
  scope_id: ref
  kind: proposal | critique | consensus | dissensus
  title: string
  body: string                 -- markdown, ADR 본문
  proposers_json: string[]     -- agent_name 배열 (consensus 시 합의 agent)
  dissenters_json: { agent_name, reason }[]    -- dissensus 시 반대 agent + 이유
  refs_json: { scenario_ids?, requirement_ids?, autonomy_policy_id? }
  produced_via_pattern_id: ULID                -- D23: 결정을 산출한 CollaborationPattern
  reversal_plan: JSON | null                   -- D28: inverse migration / git revert SHA / fs snapshot / compensating action
  blast_radius_score: INTEGER (0..10)          -- D28: architecture=10, schema=8, naming-canonical=4, impl-internal=3, doc-only=1
  reversal_of: ULID | null                     -- D28: 이 Decision 이 다른 Decision 의 rollback 이면 원 Decision.id
  created_at
}
```

SQL DDL (시안):

```sql
CREATE TABLE decision (
  id                       TEXT PRIMARY KEY,
  short_code               TEXT NOT NULL UNIQUE,
  plan_id                  TEXT NOT NULL,
  scope_kind               TEXT NOT NULL CHECK (scope_kind IN ('plan','scenario','requirement')),
  scope_id                 TEXT NOT NULL,
  kind                     TEXT NOT NULL CHECK (kind IN ('proposal','critique','consensus','dissensus')),
  title                    TEXT NOT NULL,
  body                     TEXT NOT NULL,
  proposers_json           TEXT,
  dissenters_json          TEXT,
  refs_json                TEXT,
  produced_via_pattern_id  TEXT REFERENCES collaboration_pattern(id),
  reversal_plan            TEXT,
  blast_radius_score       INTEGER NOT NULL DEFAULT 5 CHECK (blast_radius_score BETWEEN 0 AND 10),
  reversal_of              TEXT REFERENCES decision(id),
  created_at               INTEGER NOT NULL
);
CREATE INDEX decision_plan_idx     ON decision (plan_id, created_at);
CREATE INDEX decision_kind_idx     ON decision (kind, created_at);
CREATE INDEX decision_pattern_idx  ON decision (produced_via_pattern_id);
CREATE INDEX decision_reversal_idx ON decision (reversal_of) WHERE reversal_of IS NOT NULL;
```

append-only ADR. `kind` 가 `proposal` → `critique` → (`consensus` | `dissensus`) 의 합의 단계를 표현 (D20). consensus = autonomy mode 에 따라 L4/L5 게이트 통과 가능. dissensus = mode 무관 사용자 escalation.

**Reversibility (D28)**: `reversal_plan` 은 apply 직전에 impl-coder + schema-architect 협업으로 자동 생성, decision-resolver 가 critique. 형식 valid 검증 = (a) JSON parse 성공, (b) `type` ∈ {migration_sql, git_revert, fs_snapshot, compensating_action} 중 하나, (c) 해당 type 별 필수 필드 (`migration_sql` → `sql` + `dependencies`; `git_revert` → `sha`; `fs_snapshot` → `snapshot_ref`; `compensating_action` → `action_spec`). `blast_radius_score` 는 kind 별 정적 룰: architecture=10, schema=8, naming-canonical=4, impl-internal=3, doc-only=1 (AgentSpec 으로 룰 확장 가능). L5 unlock 의 추가 조건 = (a) active pattern shape valid AND (b) reversal_plan NOT NULL + 형식 valid AND (c) blast_radius_score ≤ AutonomyPolicy.l5_threshold (default 5). 미충족 시 L4 timed gate 로 강등.

**Rollback 경로**: 사용자가 대시보드에서 "rollback this decision" 트리거 → daemon 이 `reversal_plan` 을 reversal-runner specialist 에 dispatch. rollback 자체도 새 Decision row (kind='consensus', reversal_of=원 decision id) 로 append. 원 row 는 수정/삭제하지 않음 (D12 SNAPSHOT-ONLY). rollback 실패 시 dissensus escalate (mode 무관 사용자 게이트).

### 3.7 AutonomyPolicy 스키마

```
AutonomyPolicy {
  id: ULID
  scope_kind: plan | decision_kind | pattern_kind | global       -- D25: pattern_kind 추가
  scope_id: ref | null         -- decision_kind / pattern_kind / global 인 경우 null
  decision_kind: string | null -- scope_kind='decision_kind' 일 때 "architecture" 등 식별자
  pattern_kind: string | null  -- scope_kind='pattern_kind' 일 때 "workflow"/"graph"/"swarm"/"agents-as-tools"
  mode: L3 | L4 | L5
  external_surface: bool       -- publish/deploy/external API 보유 plan 표식
  timeout_ms: integer | null   -- L4 시 자동 적용 대기 시간
  forced: bool                 -- D17 강제 (architecture/schema/naming-canonical) 표식
  l5_threshold: integer        -- D28: blast_radius_score 가 이 값 이하일 때만 L5 자동 apply (default 5)
  pattern_depth_cap: integer   -- D24: CollaborationPattern.depth 상한 (default 3)
  plan_single_session_lock: bool -- D29: true 면 1 plan 은 1 session 만 active scenario 보유 (default false)
  set_by: string               -- "system-default", "user:<name>", "circuit-breaker"
  created_at, updated_at
}
```

SQL DDL (시안):

```sql
CREATE TABLE autonomy_policy (
  id                        TEXT PRIMARY KEY,
  scope_kind                TEXT NOT NULL CHECK (scope_kind IN ('plan','decision_kind','pattern_kind','global')),
  scope_id                  TEXT,
  decision_kind             TEXT,
  pattern_kind              TEXT CHECK (pattern_kind IS NULL OR pattern_kind IN ('workflow','graph','swarm','agents-as-tools','direct')),
  mode                      TEXT NOT NULL CHECK (mode IN ('L3','L4','L5')),
  external_surface          INTEGER NOT NULL DEFAULT 0,
  timeout_ms                INTEGER,
  forced                    INTEGER NOT NULL DEFAULT 0,
  l5_threshold              INTEGER NOT NULL DEFAULT 5 CHECK (l5_threshold BETWEEN 0 AND 10),
  pattern_depth_cap         INTEGER NOT NULL DEFAULT 3 CHECK (pattern_depth_cap BETWEEN 1 AND 10),
  plan_single_session_lock  INTEGER NOT NULL DEFAULT 0,
  set_by                    TEXT NOT NULL,
  created_at                INTEGER NOT NULL,
  updated_at                INTEGER NOT NULL,
  UNIQUE (scope_kind, scope_id, decision_kind, pattern_kind)
);
CREATE INDEX autonomy_policy_scope_idx   ON autonomy_policy (scope_kind, scope_id);
CREATE INDEX autonomy_policy_pattern_idx ON autonomy_policy (pattern_kind) WHERE pattern_kind IS NOT NULL;
```

6번째 1등 시민 entity (D2, D14). 신규 plan default = L5, external_surface=true plan default = L4, decision_kind ∈ {architecture, schema, naming-canonical} 는 forced=true 로 L4 고정 (D17). Circuit breaker (D18) 발동 시 모든 row 의 mode 가 L3 로 일괄 update 되며 `set_by='circuit-breaker'` 로 표기.

**Pattern-scoped autonomy (D25)**: `pattern_kind` scope 의 신규 기본값 — workflow=L5, graph=L5, swarm=L4, agents-as-tools=L4, direct=L3 강제 (D27 자동 demotion). plan-level mode 와 pattern_kind mode 충돌 시 더 엄격한 쪽 우선 (L3 > L4 > L5). 사용자가 plan 의 특정 pattern_kind policy 를 명시 override 시 그 plan 한정 적용 — UNIQUE 인덱스가 plan_id + pattern_kind 조합으로 row 분리 보장.

**Reversibility threshold (D28)**: `l5_threshold` 가 L5 자동 apply 의 blast_radius 게이트. Decision.blast_radius_score ≤ l5_threshold 인 결정만 L5 자율 apply 자격. default 5 = architecture/schema 는 자동 L4 강등 (점수 10/8 > 5), naming/impl-internal/doc-only 는 L5 자격. 사용자가 plan 별로 threshold 를 0~10 사이 조정 가능.

**Pattern depth cap (D24)**: `pattern_depth_cap` 가 CollaborationPattern.depth (parent_pattern_id chain) 의 상한. default 3 — "전체 plan = workflow / 한 step = swarm / swarm 의 한 step = agents-as-tools" 까지. 초과 시 D26 swarm gate 가 차단.

**Multi-session lock (D29)**: `plan_single_session_lock=true` 면 1 plan 의 N 개 scenario 가 모두 1 session 에서만 active claim 가능. 다른 session 이 같은 plan 의 다른 scenario 를 active 로 전이 시도하면 daemon 이 거부. default false — multi-session 협업이 자연스럽다는 가정. 명시 활성화는 single-developer plan 등에 한정.

### 3.8 AgentSpec 스키마 (M5 self-organization)

```
AgentSpec {
  id: ULID
  name: string                 -- e.g. "impl-coder" (per-name UNIQUE 제거 — (name, stance) 가 새 distinctness)
  stance: string               -- D26: proposer | devil_advocate | schema_guardian | performance_reviewer | security_reviewer | neutral
  created_by: string           -- 생성을 trigger 한 agent_name
  origin_plan_id: ref
  system_prompt: string        -- 신규 agent 의 system instruction
  tool_allowlist_json: string[]
  decision_kinds_json: string[]    -- 이 agent 가 권한을 가지는 decision-kind 목록
  blast_radius_rules_json: string  -- D28: decision_kind → score 매핑 룰 (스펙별 score 확장)
  status: active | expired
  expires_at: timestamp | null     -- null = 영구
  created_at, updated_at
}
```

SQL DDL (시안):

```sql
CREATE TABLE agent_spec (
  id                       TEXT PRIMARY KEY,
  name                     TEXT NOT NULL,
  stance                   TEXT NOT NULL DEFAULT 'neutral'
                           CHECK (stance IN ('proposer','devil_advocate','schema_guardian',
                                             'performance_reviewer','security_reviewer','neutral')),
  created_by               TEXT NOT NULL,
  origin_plan_id           TEXT NOT NULL,
  system_prompt            TEXT NOT NULL,
  tool_allowlist_json      TEXT,
  decision_kinds_json      TEXT,
  blast_radius_rules_json  TEXT,
  status                   TEXT NOT NULL CHECK (status IN ('active','expired')),
  expires_at               INTEGER,
  created_at               INTEGER NOT NULL,
  updated_at               INTEGER NOT NULL,
  UNIQUE (name, stance)
);
CREATE INDEX agent_spec_status_idx ON agent_spec (status, expires_at);
CREATE INDEX agent_spec_stance_idx ON agent_spec (stance, status);
```

**Stance (D26 sybil-fix)**: distinctness 단위가 `name` 에서 **(name, stance) tuple** 로 격상. 같은 `impl-coder` 인스턴스 2개는 system prompt 가 동일해 "독립된 판단" 이 0 — sybil. tuple distinctness 가 진짜 다양성 신호. 1 AgentSpec.name 이 여러 stance 인스턴스 (`impl-coder/proposer`, `impl-coder/devil_advocate`) 를 가질 수 있고, Graph consensus 게이트 (D26) 는 (name, stance) ≥ 2 distinct 강제. stance 별로 system_prompt 가 달라야 — 예: `devil_advocate` 는 "이 제안의 실패 시나리오 3개를 먼저 적어라" 류 추가 지시. `neutral` 은 stance 미지정 호환 (legacy).

§2 의 8개 정적 specialist (gwt-converter / scenario-decomposer / impl-coder / test-runner / regression-runner / disruption-analyst / decision-resolver / schema-architect) 로 부족할 때 runtime 에 동적 생성 (M5 self-organization). 도메인 특화 분석이 필요한 시점에 결정-resolver 가 AgentSpec 을 작성하고 새 specialist 를 spawn 한다.

**D22~D29 specialist 확장 (v0.5)**: 3 신규 meta-specialist 가 추가된다.
- **pattern-orchestrator** (stance=proposer): work entity 생성 시 적절한 CollaborationPattern 을 제안. plan-level 의 pattern 선택 + nested pattern 트리 구성 + step manifest 작성.
- **pattern-critic** (stance=devil_advocate): pattern-orchestrator 의 제안에 critique. "1-step workflow 가짜 패턴 시도", "graph 에 동일 (name,stance) 2 인스턴스 등재 sybil" 등 D26 위반 조기 차단.
- **reversal-runner** (stance=neutral): D28 의 rollback 실행 specialist. Decision.reversal_plan 의 type 별 핸들러 (migration_sql / git_revert / fs_snapshot / compensating_action) 를 디스패치.

### 3.9 CollaborationPattern 스키마 (D22 7번째 1등 시민)

```
CollaborationPattern {
  id: ULID
  short_code: e.g. CP-12
  plan_id: ref
  kind: workflow | graph | swarm | agents-as-tools | direct
  applies_to: plan | requirement | scenario | task | decision | round  -- 어느 entity 종류에 적용되는 패턴인지
  scope_id: ref                             -- applies_to 의 구체 entity id
  parent_pattern_id: ULID | null            -- D24: self-FK, DAG 강제
  depth: integer                            -- 0=root, 자동 계산
  lifecycle: pending | active | converged | dissensus | aborted
  steps_json: JSON | null                   -- workflow.kind: ordered step list (D26 shape: len ≥ 2)
  reviewers_json: JSON | null               -- graph.kind: [(agent_name, stance)] manifest (D26 shape: distinct ≥ 2)
  fan_out_json: JSON | null                 -- swarm.kind: spawn 대상 agent list (D26 shape: len ≥ 2)
  peer_registration_json: JSON | null       -- agents-as-tools.kind: caller→callee 등록 (D26 shape: len ≥ 1)
  decided_at: timestamp | null              -- converged/dissensus/aborted 전이 시점
  decided_reason: string | null
  created_at, updated_at
}
```

SQL DDL (시안):

```sql
CREATE TABLE collaboration_pattern (
  id                      TEXT PRIMARY KEY,
  short_code              TEXT NOT NULL UNIQUE,
  plan_id                 TEXT NOT NULL,
  kind                    TEXT NOT NULL CHECK (kind IN ('workflow','graph','swarm','agents-as-tools','direct')),
  applies_to              TEXT NOT NULL CHECK (applies_to IN ('plan','requirement','scenario','task','decision','round')),
  scope_id                TEXT NOT NULL,
  parent_pattern_id       TEXT REFERENCES collaboration_pattern(id),
  depth                   INTEGER NOT NULL DEFAULT 0 CHECK (depth >= 0),
  lifecycle               TEXT NOT NULL CHECK (lifecycle IN ('pending','active','converged','dissensus','aborted')),
  steps_json              TEXT,
  reviewers_json          TEXT,
  fan_out_json            TEXT,
  peer_registration_json  TEXT,
  decided_at              INTEGER,
  decided_reason          TEXT,
  created_at              INTEGER NOT NULL,
  updated_at              INTEGER NOT NULL
);
CREATE INDEX collaboration_pattern_plan_idx       ON collaboration_pattern (plan_id, created_at);
CREATE INDEX collaboration_pattern_kind_idx       ON collaboration_pattern (kind, lifecycle);
CREATE INDEX collaboration_pattern_scope_idx      ON collaboration_pattern (applies_to, scope_id);
CREATE INDEX collaboration_pattern_parent_idx     ON collaboration_pattern (parent_pattern_id) WHERE parent_pattern_id IS NOT NULL;
CREATE INDEX collaboration_pattern_lifecycle_idx  ON collaboration_pattern (lifecycle, updated_at);
```

7번째 1등 시민 entity (D22). 6 work entity (plan/requirement/scenario/task/decision/round) 의 `produced_via_pattern_id` FK 가 이 테이블을 참조.

**Lifecycle 전이 게이트**:
- `pending → active`: kind 별 shape validation 통과 강제 (D27b). workflow: `steps_json` len ≥ 2; graph: `reviewers_json` 의 (name, stance) tuple distinct ≥ 2; swarm: `fan_out_json` len ≥ 2; agents-as-tools: `peer_registration_json` len ≥ 1; direct: shape 무관 통과 (자동 L3 cap 부여).
- `active → converged`: kind 별 진척 충족. workflow: 모든 step 의 evidence 적재; graph: consensus Decision 적재 (distinct (name,stance) ≥ 2 의 합의); swarm: 모든 fan_out agent 의 self-termination signal; agents-as-tools: caller 가 callee 결과 receipt ack.
- `active → dissensus`: graph/swarm/agents-as-tools 에서 합의 실패. mode 무관 Layer 0 사람 게이트 escalate (D20).
- `* → aborted`: 사용자 명시 종료 또는 circuit breaker (D18) 트리거.

**재귀 (D24)**: `parent_pattern_id` 가 패턴 트리 형성. cycle 차단 = 자식이 자기 조상을 parent 로 가질 수 없음 (daemon topological sort 검증). `depth` 는 자동 계산: parent NULL → 0, else parent.depth + 1. AutonomyPolicy.pattern_depth_cap 초과 시 패턴 생성 거부.

**`direct` 의 의미 (D23, D27)**: 메인 1인극 anti-pattern 의 명시 마커. D27a 의 자동 escape (사용자가 produced_via_pattern_id 명시 없이 entity 생성 시 daemon 이 `direct` row 자동 생성) — 단 비용 부담: AutonomyPolicy 자동 L3 cap, 대시보드 빨간 배지, activity log audit. shape 미통과 가짜 패턴 (1-step workflow) 의 escape 가 아니라 별개 marker.

---

## 4. 핵심 워크플로우

5 개 flow 와 multi-agent communication substrate (§5 Layer 2.5 의 M1~M5) · AWS 4 패턴 (D15) 간 매핑 매트릭스. 단순 task delegation (할당) 은 §4.2 자연어→GWT 변환에만 허용 — 나머지 모든 flow 는 agent 간 통신 (M1~M5) 이 first-class (D19).

| 위치 | flow | 적용 메커니즘 | 통신 vs 할당 | AWS 패턴 |
|---|---|---|---|---|
| §4.2 | 자연어 → GWT | M1 | 할당 (allowed exception — 입력 정규화) | Agents-as-Tools |
| §4.1 | R1 분해 | M3 + M5 | 통신 | Workflow + debate |
| §4.1 | R1 실행 | M1 + M2 | 통신 | Swarm |
| §4.4 | R2+ 회귀 | M4 + 병렬 | 통신 | Graph |
| §4.3 | Disruption | M3 | 통신 | Swarm + debate |

§4.2 만 단순 할당이 허용되는 이유: 자연어 → GWT 변환은 single-source 정규화 작업이라 agent 간 합의가 의미 없음. 그 외 모든 flow 는 통신 (M1~M5) 이 first-class.

### 4.1 한 Plan 의 일생

1. **사람** 이 Claude Code 세션에서 자유 자연어로 배경/요구사항 던짐.
2. **LLM** 이 `/plan new` 로 Plan 생성. 사람 입력을 정돈해 Requirement 로 저장(SNAPSHOT-ONLY).
3. **LLM** 이 `/scenario add` 를 반복 호출, GWT 시나리오들을 정돈해 적재. 사람은 자유 코멘트로 보완.
4. **LLM** 이 중요한 판단을 했으면 `/decide` 로 Decision 추가(append-only). proposal → critique → consensus / dissensus 단계 (Decision.kind) 가 multi-agent 합의로 형성 ← **M3 negotiation**.
5. **사람** 이 `/plan approve` 로 승인. 이 시점에 시나리오 ≥ 1 & GWT 유효성 검사 통과 필요(D8).
6. **LLM** 이 `/round start` 로 R1 생성. plan 의 confirmed 시나리오 전부를 대상으로 Task 를 분해 — 시나리오 분해는 specialist agent 들이 proposal 을 내고 서로 critique 한 뒤 consensus 로 수렴 ← **R1 분해: M3 (negotiation) + M5 (필요 시 self-organization), AWS Workflow + debate 패턴**.
7. **LLM** sub-agent 들이 시나리오별 Task 를 병렬 실행. AgentNote (M1 blackboard) 에 hypothesis / observation 을 적재해 다른 agent 가 비동기 read. 산출물은 receipt 와 함께 다음 agent 로 hand-off ← **R1 실행: M1 (blackboard) + M2 (peer hand-off), AWS Swarm 패턴**. 각 Task 완료 시 evidence 적재.
8. **사람** 이 R1 결과를 확인 / 보완 요청.
9. 다음 작업 시 `/round start` → R2. R1 의 모든 passing 시나리오 자동 재검증(D6 strict-regression 기본). 자세한 흐름은 §4.4.
10. 새 작업이 기존 시나리오에 disruption 을 일으키면 needs-review(D9) 게이트로 사람에게 묻는다 (§4.3). In-flight Task 있으면 pause(D10) (§4.5).

### 4.2 자연어 → GWT 변환

LLM 이 항상 변환 주체. **단순 할당 (delegation) 으로 처리되는 유일한 flow** — gwt-converter specialist 가 입력을 받아 GWT 시나리오를 단독 생산. 합의가 의미 없는 정규화 작업 (D19 의 예외). orchestrator 가 gwt-converter 를 호출하는 형태 ← **M1 (blackboard 에 변환 결과 적재) + AWS Agents-as-Tools 패턴**.

사람 입력 예:

> "로그인 페이지에 빈 칸 검사 좀 넣어줘. 아무것도 안 적고 누르면 빨간색 오류 뜨게."

LLM 이 적재하는 시나리오:

```
Given: 사용자가 로그인 페이지에 있고, 이메일·비밀번호 입력 칸이 비어 있다.
When:  사용자가 "로그인" 버튼을 누른다.
Then:  "이메일과 비밀번호를 입력하세요" 라는 오류 메시지가 빨간색으로 표시된다.
```

GWT 필드 중 하나라도 자유 형식으로 들어오면 거부(D5). 사람이 정형 형식을 외울 필요가 없으므로 변환 비용은 LLM 측이 부담.

### 4.3 Disruption 처리

새 시나리오/요구사항/결정이 기존 시나리오를 잠재적으로 무효화하는 경우. 영향 분석은 disruption-analyst 가 가설(hypothesis)을 AgentNote 에 적재하고, 다른 specialist (impl-coder / regression-runner) 가 critique 를 붙여 합의 또는 dissensus 로 수렴 ← **M3 (negotiation) + AWS Swarm + debate 패턴**. dissensus 발생 시 mode 무관 사용자 escalation (D20).

- **needs-review** (기본): 영향 받는 시나리오 목록을 사람에게 제시. 사람이 retire/edit/keep 결정.
- **auto** (옵션): LLM 이 자율 판정 + 처리안 제시. **단, 적용 전 사람 confirm 필수.** auto 모드도 사람 확인 없이 시나리오를 폐기/수정할 수 없다. autonomy mode 가 L5 인 plan 이라도 architecture / schema 영향 발견 시 D17 에 따라 L4 게이트로 강제 강등.

### 4.4 R2+ 회귀

R(N) (N ≥ 2) 시작 시 R1..R(N-1) 의 passing 시나리오 전부를 자동 재검증 (D6 strict-regression 기본). regression-runner 가 Scenario.depends_on DAG (§3.2, M4 scenario-as-contract) 를 topological sort 하고, 의존성 없는 시나리오를 병렬 group 으로 묶어 동시 실행 ← **M4 (scenario-as-contract) + 병렬 실행, AWS Graph 패턴**.

흐름:

1. `/round start` 가 R(N) 을 `planning` 으로 생성.
2. regression-runner 가 R1..R(N-1) passing 시나리오 + R(N) 신규 시나리오의 합집합을 대상으로 depends_on DAG 구성.
3. 위상 정렬 결과 root level 시나리오부터 병렬 실행. 각 시나리오의 Task 는 §4.1 step 7 과 동일한 M1+M2 substrate 위에서 진행.
4. 한 시나리오가 failing 으로 끝나면 그 시나리오에 의존한 후속 시나리오는 자동 `impacted` 표기 + sub-agent 가 원인 분석 노트를 AgentNote 에 적재.
5. R(N) 의 모든 시나리오가 종료되면 R(N) `completed`. R1..R(N-1) 의 시나리오 중 R(N) 결과로 status 가 바뀐 항목은 per_round_results 에 기록.

`forward-only` 모드 (D6 옵션) 는 retired 시나리오를 DAG 에서 제외하고 동일 흐름.

### 4.5 In-flight Task 처리

`/round start` 호출 시 진행 중 Task 가 있으면 :

- **pause** (기본): 진행 중 Task 동결. 새 Round 진입. 사람 결정 후 재개/취소.
- **abort**: 진행 중 Task 즉시 cancel + evidence 미수집.
- **continue-on-noimpact**: 새 Round 의 시나리오가 진행 중 Task 의 시나리오와 disjoint 인 경우에 한해 둘을 병렬 진행. LLM 이 disjoint 판정 — 판정 자체가 multi-agent 합의일 때 (impl-coder + regression-runner 양측 동의) consensus 인정 (D20).

---

## 5. 시스템 아키텍처

### 5.1 컴포넌트

```
[Claude Code / 다른 LLM 클라이언트]
        │
        │ stdio MCP
        ▼
[CLI 바이너리: cli + 내장 mcp 서브커맨드]
        │
        │ Unix socket / HTTP
        ▼
[Daemon: axum + rusqlite + sqlite-vec]
        │
        ▼
[Local SQLite DB (XDG paths)]
```

- **Cli + Daemon + MCP** 단일 Rust workspace 단일 저장소. cli 가 `mcp` 서브커맨드로 MCP stdio 서버를 노출.
- **Plugin shell** 동일 저장소에 포함. 2개 브랜치 유지: `main`(src) + `dist`(빌드된 바이너리 + 매니페스트).
- **Web 대시보드 / Desktop** 별도 저장소. 본 도구의 add-on. 핵심 흐름과 독립.

### 5.2 저장소 구조 (단일 Rust workspace)

```
<repo-root>/
  Cargo.toml              # workspace
  crates/
    cli/                  # 사용자 진입 바이너리
    daemon/               # 데몬 바이너리
    mcp/                  # MCP 서버 라이브러리 (cli 가 임베드)
    core/                 # 도메인 모델 + 저장소 추상화
    db/                   # rusqlite + sqlite-vec 어댑터
  plugin/                 # Claude Code 플러그인 shell
    .claude-plugin/
    .mcp.json
    hooks/
    skills/
    adapters/
  docs/
  Cargo.lock
```

Biome / Deno / rust-analyzer 의 monorepo 패턴 답습. src 와 dist 분리는 브랜치로.

### 5.3 데이터 위치 (XDG 준수, LM-8 invariant 계승)

| 영역 | 경로 |
|---|---|
| 데이터(SQLite) | `~/.local/share/<name>/` |
| 캐시(socket/pid/port) | `~/.cache/<name>/` |
| 설정 | `~/.config/<name>/` |
| 상태(로그) | `~/.local/state/<name>/` |

플러그인 디렉터리(`~/.claude/plugins/<name>-*/`) 아래에 사용자 데이터가 들어가서는 안 된다(LM-8 invariant). 런타임 가드 + `doctor` 진단 모두 강제. Clawket v3.0 의 invariant 그대로 계승.

### 5.4 Claude Code 통합

- **MCP 서버**: `<name> mcp` (stdio).
  - Read: `search_knowledge`, `search_scenarios`, `get_plan_context`, `get_recent_decisions`.
  - Write: `add_scenario`, `add_requirement`, `add_decision`, `update_task_evidence`, `start_round`.
- **슬래시 명령**: `/scenario`, `/round`, `/plan`, `/req`, `/decide` (D11).
- **Hooks**:
  - `SessionStart`: 활성 Plan/Round 컨텍스트 주입.
  - `UserPromptSubmit`: 활성 Task 컨텍스트 주입.
  - `PreToolUse` (Edit/Write/Bash/Agent): 활성 Task 없으면 변경 차단.
  - `PostToolUse` (Edit/Write): 변경을 Task 의 evidence 후보로 자동 기록.
  - `SubagentStart/Stop`: sub-agent 를 Task 에 바인딩 + 결과 요약 적재.
- **Skills**: `/scenario` 헬퍼 skill 1개(자연어 → GWT 가이드).
- **Agent frontmatter**: tier ↔ effort, scope ↔ memory 매핑.

### 5.5 에이전트 아키텍처 (6-Layer)

§5.1 의 물리 컴포넌트 위에 올라가는 **논리 에이전트 토폴로지**. Layer 0~4 + 2.5 의 6개 층으로 구성. 각 층은 직교 관심사: Layer 0 은 사람 게이트 위치, Layer 1 은 spawn / 모니터링, Layer 2 는 specialist agent 집합, Layer 2.5 는 agent 간 통신 substrate (M1~M5), Layer 3 은 자율 권한 정책, Layer 4 는 sub-agent system prompt 규약. 결정 권한은 Layer 1 이 아닌 Layer 2 의 specialist 간 합의에서 발생하며, Layer 0 이 그 합의가 사람 게이트를 거치는지 여부만 통제한다.

#### Layer 0 — Autonomy Mode (사람 게이트 위치 통제)

scope 별 slider 로 자율 수준을 설정. **mode 는 의사결정 권한이 아니라 합의 결과가 사람 게이트를 통과하는 방식만 통제** — agent 간 합의 자체는 mode 와 무관하게 항상 발생한다 (D14, D17).

| Mode | 정의 | 적용 시점 |
|---|---|---|
| **L3** (always ask) | 모든 합의에 대해 사람 confirm 필수 | 새 plan 의 신중 모드 default, external surface plan |
| **L4** (notify + timed auto-apply) | 합의 결과를 사람에게 통지 + N 초 (per scope) 안에 거부 없으면 자동 적용 | architecture / schema / naming-canonical scope 강제 default |
| **L5** (immediate apply + evidence only) | 합의 즉시 적용, evidence 만 사후 적재 | 신규 plan 의 적극 모드 default, 내부 surface plan |

scope 분류 (D14, D17):
- **architecture** (모듈/계층 변경) → L4 강제, L5 로 격하 불가
- **schema** (DB/메시지 스키마 변경) → L4 강제, L5 로 격하 불가
- **naming-canonical** (공개 API/슬래시 명령/엔티티 명칭) → L4 강제, L5 로 격하 불가
- **impl-internal** (내부 구현) → plan 기본값 (L3/L4/L5 자유)
- **doc-only** (문서 수정) → plan 기본값

**Dissensus escalation 규칙** (D20): specialist agent 간 합의가 형성되지 않은 dissensus 상태는 mode 무관 **항상 사람 게이트로 escalate**. L5 plan 에서도 dissensus 는 자동 적용 대상이 아니다. circuit breaker (Layer 3) 가 트리거된 경우도 동일 — mode 무관 사람 게이트.

#### Layer 1 — Orchestrator (얇은 spawn / 모니터링 전용)

orchestrator agent 의 책임 범위는 **의도적으로 얇게 유지** (D16). 의사결정 권한 없음.

- **허용된 책임**: (a) plan/round 시작 시 초기 specialist sub-agent spawn, (b) AgentNote (M1 blackboard) 폴링 + 정체 감지 시 추가 spawn / dispatch, (c) Task 결과 evidence 집계 + Round 진행 상황 모니터링, (d) circuit breaker 트리거 감지 시 escalate.
- **금지된 책임**: 시나리오 분해 판단, 구현 방식 선택, disruption 해결안 채택, Decision 본문 작성. 이들은 모두 Layer 2 specialist 간 합의에서 발생해야 한다.

orchestrator 는 sub-agent 의 출력을 종합 / 판단하는 위치가 아니다 — 출력은 AgentNote 에 적재되어 다른 peer specialist 가 비동기 read 한다. orchestrator 가 "최종 결정자" 가 되면 Layer 2 합의가 무력화되므로 (centralization 위험), spawn / 모니터링 외 책임은 명시적으로 금지한다 (D16).

#### Layer 1.5 — Delegation Enforcement (메인 execution 차단 게이트)

Layer 1 의 책임 분리 (allowed / forbidden) 를 **PreToolUse hook 에서 메커니컬하게 강제** 한다 (D21). Layer 1 의 텍스트 규약이 항상 자기 강제력을 갖도록 보장하는 invariant 층.

**Identity 신호 — Claude Code 공식 hook contract**:
- `hookInput.agent_id` 존재 → Agent 도구로 spawn 된 sub-agent (Layer 2 specialist)
- `hookInput.agent_id` 부재 → 메인 세션 (Layer 1 orchestrator)
- `hookInput.agent_type` → AgentSpec.name 과 정합 확인 (등록되지 않은 specialist 는 거부)

**도구 분류 — 메인 세션 기준**:

| 분류 | 도구 | 메인 처리 |
|---|---|---|
| **차단 (execution)** | `Edit`, `Write`, `NotebookEdit`, mutating `Bash` (화이트리스트 미통과) | 거부 → specialist 위임 |
| **허용 (read-only)** | `Read`, `Grep`, `Glob`, `WebSearch`, `WebFetch`, MCP read tools | 통과 |
| **허용 (read-only Bash)** | `git status`, `git log`, `git diff`, `cargo check`, `cargo clippy`, `pnpm typecheck`, `pnpm lint`, `ls`, `cat`, `head`, `tail`, `grep`, `find` (no `-delete`) | 화이트리스트 매칭 시 통과 |
| **허용 (orchestration)** | `Agent`, `TaskCreate`, `TaskUpdate`, `TaskList`, `SendMessage`, `ScheduleWakeup`, `Skill` | 통과 |
| **허용 (MCP write — Clawket / SDI 자체)** | `clawket task ...`, `sdi ...` 류 MCP tool 및 Bash 호출 | 통과 (메타 워크 관리는 차단 대상 아님) |

mutating Bash 판정은 화이트리스트 (read-only) + 블랙리스트 (`destructive-patterns.json` 의 카탈로그) 의 교집합. 둘 다에 안 잡히는 회색 영역은 기본 차단 → 사용자가 명시 등록.

**Sub-agent 측 처리**:
- `hookInput.agent_id` 가 있으면 위 차단을 적용하지 않는다. specialist 는 자기 영역에서 자유롭게 execution.
- 단 `hookInput.agent_type` 이 AgentSpec 에 미등록이면 `rogue-specialist` 코드로 거부 + activity log.

**Circuit breaker (Layer 3) 와의 관계**:
- Circuit breaker 트리거 시 → 메인에 한해 차단을 임시 해제 (사람이 직접 통제하는 비상 모드).
- 해제는 plan 단위로 잠금되지 않고 세션 단위. 재기동 시 기본값 (차단) 으로 복원.
- 모든 해제는 `audit=circuit-override` 로 활동 로그.

**Emergency bypass**:
- Primary surface: `sdi bypass arm --reason "<짧은 사유>" [--ttl <초>]` — daemon-친화 CLI verb 가 `~/.cache/sdi/bypass-once` 에 JSON 마커(`{reason, armed_at, expires_at, ttl_seconds}`) 를 쓴다. 한 마커가 변경성 PreToolUse 게이트 전체(D21 위임, 활성 태스크, D29 클레임 겹침) 를 다음 한 번의 도구 호출 동안 해제하고, hook 이 honor 직전 파일을 삭제 (자연스러운 one-shot). TTL 기본 60초, 만료 마커는 정리만 되고 게이트는 열지 않음. `sdi` 는 read-only Bash 화이트리스트에 있어 메인 세션이 직접 무장 가능 — substrate 가 D21 차단 안으로 다시 갇히는 self-deadlock 을 구조적으로 차단.
- 부속 verb: `sdi bypass status` (state ∈ {`armed`, `expired`, `absent`} + TTL 잔여 + reason), `sdi bypass disarm` (멱등).
- Startup-time fallback: `SDI_DELEGATION_BYPASS=1` env-var — Claude Code 를 해당 env 가 export 된 셸에서 새로 띄울 때만 작동. 인라인 `VAR=1 cmd` 프리픽스는 hook 에 닿지 않음. shell rc 에 export 한 사용자용 surface 로만 의미가 있음.
- 모든 surface 가 stderr 경고 출력 + 게이트별 audit 이벤트(`pre_tool_use_delegation_bypass`, `pre_tool_use_active_task_bypass`, `pre_tool_use_claim_bypass`, source ∈ {`marker`, `env`}) 적재.
- routine 사용은 protocol violation — auditor 가 호출 빈도 모니터링하여 임계치 초과 시 사용자 알림.

**책임 분리 원칙 (D13 + D21 의 결합 효과)**:
- 메인 = 계획 / 분해 / 위임 / 모니터링. "사고하고 분배" 역할.
- specialist = 코드 / 문서 / 테스트 / 분석 / 합의 형성. "실행" 역할.
- 두 역할의 도구 권한이 hook 으로 분리되어 있으므로 메인이 "잠깐만 직접 고치자" 가 구조적으로 불가능.

**왜 메커니컬 게이트인가** (D13 만으로 부족한 이유):
- 문서 규약은 컨텍스트 압축 / 망각 / 무의식적 우회로 깨진다 (`mechanical-overrides.md` §6 CONTEXT DECAY).
- hook 게이트는 매 도구 호출마다 발화하므로 잊을 수 없다.
- "단일 @main solo flow is anti-pattern" (D13) 이 안티패턴이라는 사실을 매번 사람이 기억해서 위임 패턴으로 강제하는 것은 비현실적. 런타임이 강제해야 D13 이 실제로 작동한다.

#### Layer 2 — Specialist Sub-agents (peer 관계, 계층 없음)

8개의 specialist agent 가 **peer 관계** 로 작동. orchestrator 의 하위가 아니다 — orchestrator 와 specialist 는 별개 layer, specialist 끼리는 동등 (D18).

| Agent | 역할 |
|---|---|
| **gwt-converter** | 자연어 입력을 GWT 시나리오로 정규화 (§4.2). 유일한 단순 할당 agent |
| **scenario-decomposer** | 시나리오를 Task 로 분해. proposal 을 내고 다른 specialist 의 critique 를 받음 |
| **impl-coder** | 시나리오의 Task 를 구현. 코드 변경을 AgentNote 에 hypothesis 형태로 기록 |
| **test-runner** | 시나리오의 GWT 를 실행 가능한 검증으로 변환 + 실행. evidence 적재 |
| **regression-runner** | R2+ 회귀 흐름 (§4.4) 의 DAG topological sort + 병렬 실행 주도 |
| **disruption-analyst** | 신규 시나리오 / Requirement / Decision 추가 시 영향 시나리오 분석 (§4.3) |
| **decision-resolver** | proposal → critique → consensus / dissensus 흐름을 Decision append-only 로 적재 (M3) |
| **schema-architect** | architecture / schema scope 변경 발견 시 L4 게이트 트리거 + critique 주도 |

peer 관계의 의미 (D18):
- 어떤 specialist 도 다른 specialist 의 출력을 거부 / 승인할 권한이 없다. 합의는 **여러 specialist 의 proposal + critique 누적** 으로 형성 (M3 negotiation).
- 계층이 없으므로 "관리자 agent" 가 없다 — sub-agent 가 막혔을 때 호소할 상위가 존재하지 않는다. 막힘은 AgentNote 에 dissensus 로 기록되고 Layer 0 의 사람 게이트로 escalate.
- specialist 추가 / 교체는 AgentSpec (§3.8) 등재로 처리. 새 specialist 가 합류해도 기존 peer 관계는 유지.

#### Layer 2.5 — Agent Communication Substrate (M1~M5)

Layer 2 specialist 끼리 **어떻게** 통신하는지를 정의하는 5개 메커니즘 (D19). 모든 flow (§4) 는 M1~M5 중 하나 이상을 사용한다 — 단순 할당 (delegation) 은 §4.2 의 단일 예외만 허용. 통신 substrate 가 first-class 인 이유: agent 간 합의 형성이 Layer 2 의 peer 관계 (D18) 를 작동시키는 유일한 수단이기 때문이다.

##### M1 — Blackboard (AgentNote 비동기 적재 / 폴링)

specialist agent 가 hypothesis / observation / question 을 **AgentNote 엔티티** (§3.5) 에 적재하고, 다른 specialist 가 비동기로 read.

- **저장 형태**: AgentNote row. `kind` ∈ {hypothesis, observation, question, dissent, evidence}. `scope` (plan/round/scenario/task) 으로 가시 범위 한정. body 는 markdown.
- **agent 의존성**: agent A 가 적재한 AgentNote 는 agent B 가 자유롭게 read 가능. write 권한은 적재한 본인만. read 는 scope 안의 모든 agent.
- **트리거**: agent 가 작업 도중 다른 agent 의 입력이 필요하다고 판단한 시점 (e.g., impl-coder 가 schema 변경 의심 시 → schema-architect 가 read), 또는 작업 종료 시 다른 agent 가 후속으로 사용할 evidence / observation 적재.
- **폐기 (retirement)**: AgentNote 는 **append-only**. 폐기 = `retired_at` timestamp 적재 + `retired_reason` 본문. row 삭제는 불가 (audit trail 보존). retired note 는 기본 read 대상에서 제외되지만 명시적 query 시 노출.

##### M2 — Peer Hand-off (수신 receipt 와 함께 다음 agent 로 전달)

한 specialist 가 작업을 마치고 그 결과물 + 다음 agent 가 이어받을 컨텍스트를 **수신 receipt** 와 함께 명시적으로 다음 agent 로 전달.

- **mechanism**: hand-off 는 AgentNote `kind=handoff` 로 적재 + `to_agent` 필드 명시. 수신측 agent 가 read 하고 `receipt_acknowledged_at` 적재 시 hand-off 완료.
- **receipt 의미**: 단순 read 가 아니라 "받았고 이어서 진행한다" 라는 명시 ack. ack 없이 N 분 (default 5분, AutonomyPolicy 에서 조정) 지나면 orchestrator (Layer 1) 가 정체 감지 + 추가 spawn 후보로 표시.
- **차이점 vs M1**: M1 (blackboard) 은 1:N 비동기 broadcast, M2 (hand-off) 는 1:1 명시 ack. 같은 AgentNote 테이블을 쓰지만 `to_agent` + `receipt_acknowledged_at` 필드 유무로 구분.
- **사용 예**: scenario-decomposer 가 Task 분해를 마치면 impl-coder 에게 hand-off. impl-coder 가 구현을 마치면 test-runner 에게 hand-off (R1 흐름 §4.1 step 7).

##### M3 — Negotiation (proposal → critique → consensus / dissensus)

Decision (§3.6) 적재가 항상 거치는 4단계 흐름. multi-agent 합의 형성의 1차 메커니즘.

- **단계 정의** (Decision.kind 로 분류):
  1. `proposal` — 한 specialist 가 결정안을 제시. body 에 근거 (file:line / 시나리오 id / 대안 비교) 포함 필수.
  2. `critique` — 다른 specialist 가 proposal 에 반론 / 보완 제시. critique 가 0건일 수 없다 — orchestrator 가 critique 발생을 강제 (Layer 1 의 monitoring 책임).
  3. `consensus` — 모든 critique 가 해결되어 합의 형성. consensus Decision 적재 시점이 결정 효력 발생 시점.
  4. `dissensus` — critique 가 해결되지 않은 채 합의 실패. Layer 0 의 mode 무관 사람 게이트로 escalate (D20).
- **append-only 보장**: M3 의 각 단계는 Decision 의 새 row 로 적재. 이전 단계 row 수정 금지 (D12 SNAPSHOT-ONLY 의 결정 본문판 — 본문이 아니라 단계 진행도 append-only).
- **decision-resolver agent 역할**: 4단계 흐름을 추적 + 누락 단계 (critique 0건 / consensus 도달 시점 불명확) 감지 + AgentNote 로 표시. resolver 자체는 결정자가 아니다 (peer 관계 유지).

##### M4 — Scenario-as-Contract (시나리오를 agent 간 계약으로 사용)

Scenario (§3.2) 가 단순 검증 단위가 아니라 **agent 간 인터페이스 계약**. agent A 가 구현한 결과가 agent B 의 시나리오를 깨면, 그 시나리오가 계약 위반의 자동 증거.

- **계약 표현**: 각 Scenario 는 `produced_by` (구현 책임 agent) + `verified_by` (검증 책임 agent) + `depends_on` (선행 시나리오 list, §3.2) 필드를 가짐. `depends_on` DAG 가 agent 간 의존성 그래프.
- **위반 감지**: regression-runner 가 R2+ (§4.4) 흐름에서 DAG topological sort 후 병렬 실행. 선행 시나리오가 fail 하면 그에 의존한 모든 후속 시나리오가 자동 `impacted` 표기 — 후속 agent 가 자기 작업을 시작하기 전 선행 시나리오 통과 여부를 read 하므로 깨진 계약이 즉시 노출.
- **차이점 vs 일반 테스트**: 일반 테스트는 코드의 정확성 검증. M4 의 시나리오는 **agent 간 합의의 인터페이스** — 다른 agent 가 자기 작업의 전제 조건으로 read 하는 산출물 명세. 따라서 시나리오 변경은 단순 테스트 수정이 아니라 D9 disruption 분석 대상.
- **AWS 패턴 매핑**: §4.4 의 Graph 패턴 (DAG 기반 병렬 실행) 의 기반 substrate.

##### M5 — Self-organization (동적 spawn / 역할 재배치)

처음에 spawn 된 specialist 구성이 작업 도중 부족 / 과잉으로 판명되면, AgentNote 의 정체 / 막힘 signal 을 기반으로 **동적 재구성**.

- **트리거 조건**:
  - 정체: 특정 AgentNote 가 N 분 (default 10분) 동안 receipt ack 없음 (M2 stagnation).
  - 막힘: dissent kind AgentNote 가 임계치 (default 3건) 누적 — orchestrator (Layer 1) 가 추가 specialist spawn 후보로 표시.
  - 과잉: 특정 specialist 의 AgentNote 가 N 시간 동안 0건 — 해당 specialist instance 회수.
- **재구성 권한**: orchestrator (Layer 1) 가 spawn / 회수 실행. **단, 신규 AgentSpec 등재는 아님** — 기존 AgentSpec (§3.8) 의 instance 수 조정만. 새 role 추가는 사람이 AgentSpec 을 등재해야 발생.
- **circuit breaker 연동**: self-organization 시도 횟수가 임계치 (default 5회 / 단위 시간) 초과 시 Layer 3 의 circuit breaker 가 트리거 → Layer 0 의 mode 무관 사람 게이트로 escalate. 무한 spawn 루프 방지.
- **AWS 패턴 매핑**: §4.1 R1 분해 흐름의 Workflow + debate 패턴, §4.3 Disruption 흐름의 Swarm + debate 패턴의 기반 substrate.

#### Layer 2.6 — Pattern Enforcement (4 패턴 무결성 게이트)

Layer 2.5 의 substrate 위에 **CollaborationPattern.kind 별 무결성 검증** 을 PreToolUse hook 에 추가 (D26). Layer 1.5 의 delegation gate 가 "메인 vs sub-agent" 차단이라면, Layer 2.6 은 "**패턴 안에서의 행동 규칙**" 차단.

**구조**: PreToolUse hook 이 sub-agent 의 tool 호출 시 → daemon `/patterns/active?scope_id=<scenario|task|...>` query → 반환된 active CollaborationPattern.kind 별 분기 처리.

**Workflow gate**:
- shape (pending → active): `steps_json` len ≥ 2 — 1-step workflow 차단 (가짜 패턴 escape 차단).
- 런타임: 선행 step 의 evidence (Decision/Task) 가 적재되지 않은 채 후행 step sub-agent 의 execution 호출 → 거부 ("step N requires step N-1 evidence first").
- 진척 추적: 매 evidence 적재가 daemon `/patterns/<id>/advance` 트리거 → step pointer 진행 + SSE 이벤트 emit.

**Graph gate** (sybil 차단 핵심):
- shape (pending → active): `reviewers_json` 의 (AgentSpec.name, stance) tuple distinct ≥ 2 — 같은 name 2 인스턴스도 stance 동일이면 차단 (sybil). 매니페스트 자체에 stance 명시 강제.
- 런타임: Decision.kind='consensus' 적재 시점에 `proposers_json` 의 (name, stance) tuple distinct ≥ 2 확인. 미달이면 거부 ("graph consensus requires ≥ 2 distinct (name, stance) tuples; got N").
- dissensus: `dissenters_json` 비어있지 않으면 mode 무관 Layer 0 escalate.

**Swarm gate**:
- shape (pending → active): `fan_out_json` len ≥ 2 — 1-agent swarm 은 swarm 이 아니라 direct.
- 런타임:
  - spawn depth: `parent_pattern_id` chain depth ≤ AutonomyPolicy.pattern_depth_cap (default 3). 초과 시 sub-pattern 생성 거부.
  - self-spawn 루프: sub-agent X 의 컨텍스트에서 다시 X 를 spawn 시도 시 거부 — 무한 spawn 차단.

**Agents-as-Tools gate**:
- shape (pending → active): `peer_registration_json` len ≥ 1.
- 런타임: 호출되는 agent 가 caller 의 `peer_registration_json` 에 등재되지 않으면 거부 ("agent X not registered as peer of caller Y").

**Direct marker (D27 자동 escape)**:
- 메인이 produced_via_pattern_id 명시 없이 work entity 생성 시 daemon 이 `kind='direct'` row 자동 생성 + ID 부여. shape 검증 없음 (pending 없이 즉시 active).
- 비용: AutonomyPolicy 자동 L3 cap (Decision.apply 시 항상 사람 게이트), 대시보드 빨간 배지 표기, activity log audit (`audit=direct-pattern-marker`). 안티패턴이 묻히지 않게 명시 표기.

**왜 hook 게이트인가**:
- 패턴 manifest (steps_json / reviewers_json / fan_out_json / peer_registration_json) 가 entity 로 영구 존재 (D22) 하므로, 매 tool 호출 시 active pattern 조회 가능.
- Layer 1.5 (D21) 가 메인 차단 — sub-agent 가 자유롭게 execution 가능. Layer 2.6 (D26) 가 그 sub-agent 가 active pattern 의 규칙을 따르도록 강제.
- 두 layer 의 결합: 메인은 pattern 선택만 (orchestrator 책임), sub-agent 는 그 pattern 안에서만 execution.

#### Layer 2.7 — Reversibility (D28 L5 자율의 회복 비용 게이트)

L5 자율 (사용자 부재 자동 적용) 의 진짜 병목은 합의 메커니즘이 아니라 **틀린 결정의 회복 비용**. Decision 이 잘못 적용됐을 때 자동 rollback 경로 없으면 L5 는 위험.

**reversal_plan 생성**:
- proposal → critique 단계에서 impl-coder + schema-architect 가 협업으로 `reversal_plan` JSON 작성:
  - migration SQL → `{type: "migration_sql", sql: "...", dependencies: [...]}` (역방향 ALTER/DROP).
  - 코드 변경 → `{type: "git_revert", sha: "<commit>"}`.
  - fs 변경 → `{type: "fs_snapshot", snapshot_ref: "<path>"}` (rsync/tar 기반).
  - 외부 호출 → `{type: "compensating_action", action_spec: {...}}` (e.g., webhook 보상 호출).
- decision-resolver 가 critique 단계에서 reversal_plan 의 형식 valid + 실행 가능성 검증. 검증 통과 후에만 consensus 단계 진입.

**blast_radius_score 산출**:
- AgentSpec.blast_radius_rules_json 의 룰 적용. default 룰: architecture=10, schema=8, naming-canonical=4, impl-internal=3, doc-only=1.
- AgentSpec 이 자체 룰 확장 가능 (e.g., schema-architect 가 `migration_with_data_loss=10`, `migration_additive=4` 로 세분화).
- 점수가 AutonomyPolicy.l5_threshold (default 5) 초과 시 L5 자동 apply 차단 → L4 timed gate 로 강등.

**rollback 실행**:
- 사용자가 대시보드의 "rollback this decision" 트리거 → daemon `/decisions/<id>/rollback` POST.
- daemon 이 reversal-runner specialist 에 dispatch — type 별 핸들러 (migration_sql / git_revert / fs_snapshot / compensating_action) 실행.
- rollback 자체도 새 Decision row (kind='consensus', reversal_of=원 decision id) 로 append. 원 row 는 수정/삭제하지 않음 (D12 SNAPSHOT-ONLY).
- rollback 실패 시 새 Decision (kind='dissensus') 적재 + Layer 0 사람 게이트 escalate.

**reversal-runner specialist (신규 v0.5)**:
- stance=neutral. tool_allowlist = {Bash, Edit, Write, Read}.
- decision_kinds = {*} — 모든 decision-kind 의 rollback 권한.
- system_prompt 핵심: "reversal_plan.type 별 핸들러 디스패치만 수행, 추가 critique 금지 (decision-resolver 가 사전 검증). 실패 시 dissensus Decision 적재 후 즉시 종료."

**왜 reversibility 가 1등 시민인가**:
- L5 의 가치 명제 = "사용자가 자리를 비워도 도구가 망치지 않는다" — 이 확신은 합의 메커니즘이 아니라 **롤백 보장** 에서 온다.
- D22~D27 의 패턴 강제는 "결정이 어떻게 만들어졌는가" (process), D28 은 "결정이 틀렸을 때 어떻게 되돌리는가" (reversibility). 둘 다 갖춰야 L5 안전.

#### Layer 2.8 — Resource Claims (D29 multi-session 의사결정 일관성)

daemon-centric multi-session 의 storage 일관성 (SQLite ACID) 위에 **의사결정 일관성** layer. 두 main session 이 같은 plan 의 다른 scenario 에서 동시 작업 시 같은 파일을 모순되게 변경하는 race 차단.

**Claim 단위 = Scenario**:
- Scenario.claimed_resources_json (path glob 배열) 이 작업 범위 명시. 예: `["crates/db/migrations/*.sql", "plugin/agents/*.md"]`.
- Scenario.claim_status: `none → requested → active → released`.
- 전이:
  - `none → requested`: scenario `confirmed` 진입 시 자동.
  - `requested → active`: daemon 이 다른 `active` claim 과 overlap 검사 → overlap 0 이면 grant. overlap 있으면 `requested` 에 머무름 + 사용자 prompt.
  - `active → released`: round 완료 또는 사용자 명시 release.

**PreToolUse hook 의 신규 검증** (Layer 1.5 D21, Layer 2.6 D26 다음 단계):
- 매 Edit/Write/MultiEdit 호출 시 hook 이 path 추출 → daemon `/scenarios/active-claims` query.
- 호출 agent 의 현재 active scenario 가 path 를 claim 하고 있어야 함. 미claim path 면 거부 ("file X not in active scenario's claimed_resources").
- 다른 active scenario 의 claim 과 overlap (path 가 다른 active claim 의 glob 에 매치) → 거부 + 사용자 prompt: "scenario A (session 1) vs scenario B (session 2): merge or wait".

**Overlap 검출**:
- daemon 측 glob matcher (예: `globset` crate). 두 glob 의 교집합이 비어있지 않으면 overlap.
- 보수적 판정: 매치 불확실 시 overlap 으로 간주 (false positive 가 race 보다 안전).

**Plan-level advisory lock (옵션)**:
- AutonomyPolicy.plan_single_session_lock=true → 1 plan 은 1 session 만 active claim 보유. 다른 session 이 같은 plan 의 다른 scenario 를 `requested → active` 전이 시도 시 거부.
- default false — multi-session 협업이 자연스럽다는 가정. 명시 활성화는 single-developer plan, 또는 high-conflict plan 에 한정.

**Daemon = 의사결정 라우터**:
- 같은 plan 의 N session 이 모두 같은 daemon 에 연결되므로 race 검출 가능. session 분리 (각자 별도 daemon) 시에는 race 차단 불가 — XDG paths invariant + 단일 daemon 가정 (LM-8 계승) 의 직접 귀결.
- claim ledger 가 storage 일관성을 의사결정 일관성으로 확장 — daemon 없는 multi-session 은 "독립된 N 개 1인극".

#### Layer 3 — Autonomy Policy (per decision-kind × per scope)

Layer 0 의 mode (L3/L4/L5) 는 plan 단위의 default. Layer 3 은 그 default 위에서 **decision-kind × scope 조합별 세분화 정책** 을 적용 — AutonomyPolicy 엔티티 (§3.7) 가 정본.

| Scope | 신규 plan default | 비고 |
|---|---|---|
| **architecture** | **L4 강제** | L5 로 격하 불가. consensus 즉시 적용은 금지 — 항상 N 초 timed gate 거침 |
| **schema** | **L4 강제** | L5 격하 불가. DB / 메시지 스키마 변경은 사람 인지 필수 |
| **naming-canonical** | **L4 강제** | L5 격하 불가. 슬래시 명령 / 엔티티 명칭 / 공개 API 명칭 |
| **impl-internal** | **L5** (적극 모드) / L3 (신중 모드) | plan 의 자율 모드에 따라 |
| **doc-only** | **L5** | 문서 수정은 즉시 적용 |

plan 종류별 자율 모드 default (Layer 0 mode 와 Layer 3 scope policy 의 곱):
- **신규 plan / 내부 surface** → 적극 모드 (impl-internal = L5, doc-only = L5, 강제 scope 는 L4).
- **external surface plan** (공개 API / 사용자 노출 명칭 영향) → 신중 모드 (impl-internal = L3, 강제 scope 는 L4 유지).

**Circuit breaker — 항상 활성**:
- M5 self-organization 의 spawn 시도가 임계치 초과 (default 5회 / 30분) → 트리거.
- M3 dissensus 가 동일 주제에 대해 N회 누적 (default 3회 / round) → 트리거.
- AgentNote 적재 속도가 평소 baseline 대비 K배 (default 10배) 초과 → 트리거 (agent 들이 무한 루프에 빠진 신호).
- 트리거 시 **mode 무관 사람 게이트로 즉시 escalate**. L5 plan 도 예외 없음. circuit breaker 는 plan 단위로 비활성화할 수 없다 — 안전 invariant.

AutonomyPolicy 본문 변경은 자체적으로 decision-kind = `policy-change` 로 분류되어 L4 게이트를 거쳐야 한다 (정책의 정책 — 메타 안전장치).

#### Layer 4 — Sub-agent System Prompts (규약)

specialist agent (Layer 2) 가 받는 system prompt 작성 규약. AgentSpec.system_prompt (§3.8) 의 본문 정책.

**금지 패턴**:
- **"모르면 사람에게 물어라" / "확실하지 않으면 ask" 류 지시 금지**. 이 패턴은 D18 의 peer 관계를 무력화 — sub-agent 가 합의 형성 (M3) 대신 사람에게 곧장 escalate 하게 만들어 L5 plan 의 의미가 사라진다. 대신 다음을 명시:
  - "모르면 AgentNote 에 question kind 로 적재하고 다른 specialist 가 응답할 때까지 다음 단계 진행 보류"
  - "다른 specialist 와 합의가 형성되지 않으면 dissensus 로 적재 — Layer 0 가 사람 게이트 escalate 처리"
- "최종 결정은 orchestrator 에게 의뢰" 류 지시 금지 (D16 위반 — Layer 1 은 결정 권한 없음).
- "사용자에게 직접 묻기" 류 지시 금지. 사람 게이트는 Layer 0 / Layer 3 의 escalation 경로로 일원화.

**필수 패턴**:
- proposal 적재 시 근거 (file:line / 시나리오 id / 대안 비교) 명시 강제.
- 다른 specialist 의 critique 를 read 한 뒤에만 자기 proposal 을 consensus 로 승격 가능.
- AgentNote 적재 시 scope (plan/round/scenario/task) 명시 강제.

AgentSpec.system_prompt 변경은 decision-kind = `agent-spec-change` 로 분류되어 L4 게이트 (Layer 3 의 naming-canonical 인접) 를 거친다.

---

## 6. 인수 기준 (Acceptance Criteria)

1. **GWT 강제**: `/scenario add` 가 G/W/T 중 하나라도 비어 있거나 정형성 검사를 통과하지 못하면 거부한다. 거부 메시지에 LLM 이 자가 보완할 수 있는 힌트 포함.
2. **Plan 승인 게이트**: 시나리오 0개인 Plan 은 `active` 전이 불가. Task 0개여도 승인 가능.
3. **자동 회귀 검증 흐름**: `/round start` 가 R2 이상이면, 이전 모든 R 의 passing 시나리오를 자동 큐에 적재한다. 빠진 시나리오가 있으면 round 진입 거부.
4. **Disruption needs-review**: 새 시나리오·요구사항·결정 추가 시 영향 시나리오 분석을 자동 수행. 영향 ≥ 1이면 사람 확인 게이트 통과 전까지 round 진입 보류.
5. **In-flight Task pause**: 진행 중 Task 가 있는 채 `/round start` 호출 시 기본 동작은 pause. 옵션은 명시 플래그(`--abort`, `--continue-on-noimpact`) 로만.
6. **Evidence 구조**: Task `done` 전환 시 evidence 는 시나리오 단위 합격/불합격 + 증거 참조의 구조화 데이터. 자유 문자열 거부.
7. **SNAPSHOT-ONLY Requirement**: 같은 Plan 의 Requirement 본문에 이전 버전 흔적이 남아 있으면 거부. 변경 이력은 Decision 으로 분리 적재.
8. **LM-8 path invariant**: 데몬 기동 시 데이터·캐시·설정·상태 경로가 `~/.claude/plugins/` 하위면 기동 거부. `doctor` 도 동일 검사.
9. **MCP read-only 분리**: 외부 LLM 클라이언트에 노출되는 read 도구는 scope=rag artifact만 반환. 다른 scope 는 절대 노출 안 됨.
10. **`/goal` 직교 공존**: 본 도구가 `/goal` 슬래시 명령을 가로채거나 비활성화하지 않는다. 같은 세션에서 `/goal` 단발성 호출과 본 도구 흐름이 공존 가능.
11. **M3 합의 4 단계 강제**: Decision 적재 시 `kind` ∈ {proposal, critique, consensus, dissensus}. consensus 적재 직전에 같은 plan/round 안에 critique ≥ 1건이 존재해야 한다 (critique 0건인 채 consensus 시도 시 거부). decision-resolver agent 가 누락된 단계를 감지하면 AgentNote 로 표시.
12. **Dissensus mode 무관 escalate**: Decision `kind=dissensus` 가 적재되면, plan 의 autonomy mode (L3/L4/L5) 와 무관하게 사람 게이트로 즉시 escalate. L5 plan 도 dissensus 는 자동 적용 대상이 아니다. circuit breaker (Layer 3) 가 트리거된 경우도 동일 — mode 우회 불가.
13. **Layer 1 결정 권한 금지**: orchestrator 가 시나리오 분해 / 구현 방식 선택 / disruption 해결안 채택 / Decision 본문 작성 중 하나라도 시도하면 거부. 이들은 Layer 2 specialist 간 합의 (M3) 에서만 발생해야 한다.
14. **architecture / schema / naming-canonical L4 강제**: 이 3개 scope 의 Decision 적용 시 mode 가 L5 로 설정되어 있어도 L4 게이트 (timed auto-apply) 로 강제 강등. L5 즉시 적용 경로로 우회 불가.
15. **Delegation gate (D21) 메커니컬 enforcement**: 메인 세션 (`hookInput.agent_id` 부재) 에서 `Edit` / `Write` / `NotebookEdit` 호출 시 PreToolUse hook 이 거부. mutating Bash (화이트리스트 미통과 + 부수효과 가능) 도 거부. Agent 도구로 spawn 된 sub-agent (`hookInput.agent_id` 존재) 는 통과. 미등록 `agent_type` 의 sub-agent execution 은 `rogue-specialist` 코드로 거부. 메인 차단 우회 경로는 두 surface: (a) `sdi bypass arm --reason "<짧은 사유>" [--ttl <초>]` — 실행 중 세션용 primary CLI verb, XDG-cache 마커, one-shot (TTL 기본 60초, 만료 마커는 정리 only / 게이트 미개방), 한 마커가 변경성 게이트 전체(D21 / 활성 태스크 / D29) 를 동시 해제 + 게이트별 audit 이벤트(`pre_tool_use_delegation_bypass`, `pre_tool_use_active_task_bypass`, `pre_tool_use_claim_bypass`) 적재; (b) `SDI_DELEGATION_BYPASS=1` env (startup-time only — Claude Code 를 해당 env export 셸에서 새로 띄울 때만 작동, 인라인 `VAR=1 cmd` 는 닿지 않음). 별도로 circuit breaker (Layer 3) 트리거 시 메인 차단이 세션 단위로 임시 해제되며 audit log 가 `audit=circuit-override` 로 적재된다. 모든 경로가 activity log 적재.
16. **Pattern provenance (D22, D23) NOT NULL**: 신규 work entity (plan/requirement/scenario/task/decision/round) 생성 시 `produced_via_pattern_id` 가 NOT NULL. 메인이 명시 ID 없이 생성 시도 시 daemon 이 자동으로 `kind='direct'` CollaborationPattern row 를 만들고 그 ID 부여 + AutonomyPolicy 자동 L3 cap + 활동 로그 `audit=direct-pattern-marker` 적재. 마이그레이션 시점 legacy row 만 NULL 허용.
17. **Pattern shape gate (D26, D27b)**: CollaborationPattern.lifecycle `pending → active` 전이 시 kind 별 shape validation 통과 강제. workflow: `steps_json` len ≥ 2; graph: `reviewers_json` 의 (AgentSpec.name, AgentSpec.stance) tuple distinct ≥ 2; swarm: `fan_out_json` len ≥ 2; agents-as-tools: `peer_registration_json` len ≥ 1. 미통과 시 pending 에 머무름 — active 전이 거부. `direct` 만 shape 검증 면제 (자동 L3 cap 부담).
18. **Graph consensus sybil 차단 (D26)**: Decision.kind='consensus' 적재 시 `proposers_json` 의 (AgentSpec.name, AgentSpec.stance) tuple distinct ≥ 2 강제. 동일 (name, stance) 가 2회 등재되면 거부. AgentSpec 의 stance ∈ {proposer, devil_advocate, schema_guardian, performance_reviewer, security_reviewer, neutral}. 같은 `impl-coder` 2 인스턴스 (둘 다 stance=proposer) 는 consensus 자격 0 — 진짜 다양성 없음.
19. **Reversal plan 형식 valid (D28)**: Decision.kind='consensus' 적재 직전에 `reversal_plan` 이 NOT NULL + 형식 valid 강제. valid 정의 = (a) JSON parse 성공, (b) `type` ∈ {migration_sql, git_revert, fs_snapshot, compensating_action}, (c) type 별 필수 필드 (migration_sql → sql + dependencies; git_revert → sha; fs_snapshot → snapshot_ref; compensating_action → action_spec) 모두 존재. 미충족 시 consensus 거부 — proposal/critique 단계로 복귀.
20. **L5 unlock 의 3 조건 (D28)**: Decision.apply 시 mode=L5 자격 = (a) active pattern shape valid (#17), (b) reversal_plan NOT NULL + 형식 valid (#19), (c) `blast_radius_score` ≤ AutonomyPolicy.l5_threshold (default 5). 하나라도 미충족 시 L4 timed gate 로 강등 (즉시 apply 안 함). L5 자율은 reversibility 가 보장된 결정에만 부여.
21. **Rollback append-only (D28)**: 대시보드의 "rollback this decision" 트리거 시 daemon 이 reversal-runner specialist 에 dispatch. 성공 시 새 Decision row (kind='consensus', `reversal_of`=원 decision id) 적재 — 원 row 수정/삭제 금지 (D12 SNAPSHOT-ONLY). 실패 시 새 Decision row (kind='dissensus') + Layer 0 사람 게이트 escalate.
22. **Multi-session resource claim (D29)**: 매 Edit/Write/MultiEdit 호출 시 PreToolUse hook 이 path 추출 → daemon `/scenarios/active-claims` query. 호출 agent 의 active scenario 가 path 를 claim (claimed_resources_json glob 매치) 하지 않으면 거부. 다른 active scenario 의 claim 과 overlap (glob 교집합 비어있지 않음) → 거부 + 사용자 prompt ("scenario A vs scenario B: merge or wait"). `plan_single_session_lock=true` plan 은 1 session 만 active claim 가능.

---

## 7. 비기능 요구사항

- **로컬 우선**: 모든 데이터는 로컬 SQLite. 외부 네트워크 의존 없음.
- **단일 머신 단일 사용자**: 멀티 사용자/멀티 머신 동기화는 본 PRD 범위 밖.
- **응답 지연**: 시나리오 CRUD 는 P99 100ms 이하(로컬 SQLite + 캐시).
- **벡터 검색**: sqlite-vec 사용. 인덱스 재계산은 백그라운드.
- **OS**: macOS / Linux 1차. Windows 는 추후.
- **재설치 안전**: 플러그인 재설치 시 사용자 데이터 보존(LM-8).
- **데몬 자동 기동**: CLI/훅 호출 시 자동 기동, 실패 시 stderr 상세 + `doctor` 진단 경로.

---

## 8. 비목표 (Out of scope, v1)

- 멀티 사용자 동기화 / 클라우드 백업.
- 사람 협업용 댓글·멘션 기능.
- LLM 무관(non-LLM) 사용자 흐름. 본 도구는 LLM 1차 사용자 가정.
- Cucumber 호환 Gherkin 컴파일.
- 외부 BDD 도구(Cucumber, SpecFlow 등) 와의 직접 통합.

---

## 9. 마이그레이션 (Clawket v3.0 → 본 도구)

별도 트랙. 본 PRD 의 v1 범위 밖이지만 다음 매핑이 1차 시안:

| Clawket v3 | 시나리오 엔진 |
|---|---|
| Plan | Plan (유지) |
| Unit | Scenario.tag (격하) |
| Cycle | Round (개명 + 의미 재정의) |
| Task | Task (runtime, 의미 재정의) |
| `type=decision` artifact | Decision (1등 시민 격상) |
| 자유 evidence string | 구조화 evidence (시나리오별 합격/불합격) |

GWT 시나리오는 신규 개념이므로 v3 데이터에 존재하지 않는다. 마이그레이션 도구는 v3 Task 의 description 을 LLM 보조로 GWT 시나리오 후보로 추출.

---

## 10. 열린 질문

1. **Forward-only round 의 trigger UX** — 자주 쓰이게 되면 plan-level default 로 둘 것인가?
2. **Evidence 의 외부 첨부 (스크린샷·HAR 등) 저장 위치** — 로컬 파일 / SQLite blob / 외부 참조.
3. **`/decide` 와 외부 ADR (예: Markdown 파일) 의 양방향 동기화** — 본 도구가 single source 인가, 외부 미러를 허용하는가.
4. **AgentSpec 등재 권한** — 사용자만 등재 가능한가, plan 내 합의로 신규 specialist 추가 가능한가? Layer 4 의 system prompt 규약 변경이 L4 게이트인데, 신규 AgentSpec 등재는 같은 게이트인가 별도인가.
5. **Circuit breaker 임계치 학습** — circuit breaker 트리거 임계치 (default 5회/30분 등) 가 plan 종류별로 다른 적정값이 있을 듯. 운영 데이터 누적 후 재조정 필요.
6. **AgentNote retention** — append-only 인 AgentNote 의 누적량 관리. retired note 의 물리 삭제 시점 / 압축 방식.
7. **외부 LLM agent 통합** — Claude 외 다른 LLM (Codex 등) 을 specialist 로 등재 시 AgentSpec 표준화 + 통신 substrate 호환성.

---

## 11. 릴리즈 로드맵

릴리즈 단위는 PRD 문서 갱신 (`v0.x` 의 .3 이하) 과 entity 코드 구현 (`v0.4+`) 으로 분리. 본 PRD 본문의 변경은 v0.3 까지 닫혀 있고, entity / API / daemon 구현은 v0.4+ 의 별도 plan 들로 진행한다.

| 단계 | 산출 | 상태 |
|---|---|---|
| **v0.1** | cli + daemon + sqlite 스키마 + `/scenario add` / `/plan new` / `/plan approve` 으로 self-host (dogfooding) | 완료 |
| **v0.2** | `/round start` + 자동 회귀 검증 흐름 기본 동작 | 완료 |
| **v0.3** | **PRD/문서 갱신만** — Multi-agent Collaboration Governance (D13~D20 + 6 entity + Layer 2.5 substrate + 5-flow matrix + §6 인수기준 #11~#14). | 완료 |
| **v0.4** | **entity 코드 구현 (M1~M5 substrate)** — AgentNote (M1), AutonomyPolicy (D14), AgentSpec (specialist 등재), Decision.kind 확장 (M3), Scenario.depends_on / produced_by / verified_by (M4) + D21 delegation gate (Layer 1.5). v0.3 PRD 가 ground truth. | 완료 |
| **v0.5** | **Pattern as First-Class Multi-Agent Enforcement** — D22~D29 + 7번째 entity CollaborationPattern + Layer 2.6 (Pattern Enforcement) + Layer 2.7 (Reversibility) + Layer 2.8 (Resource Claims) + 3 신규 meta-specialist (pattern-orchestrator/pattern-critic/reversal-runner) + 대시보드(plugin/web) 실시간 패턴 timeline + sdi-desktop tray badge + §6 인수기준 #16~#22. **차별점**: 시나리오 기반 (Clawket 와 동일) 위에 **실제 작업 패턴 = 4 멀티에이전트 협업 패턴의 메커니컬 강제**. multi-session daemon-centric. | 진행 중 (본 plan) |
| **v0.6** | Disruption needs-review (§4.3) + In-flight Task pause (§4.5) 의 multi-agent 흐름 구현 | 미시작 |
| **v1.0** | §6 인수 기준 #1~#22 전부 충족 + Clawket v3.0 → SDI 마이그레이션 도구 (§9) | 미시작 |

각 단계 사이 cross-version 호환성은 보장하지 않음 — v0.4+ entity 추가 시 schema migration 필요 (daemon 자동 적용). v0.3 plan 의 산출물 (PRD 본문) 은 v0.4+ 구현의 단일 진실 공급원으로 동결되며, 구현 단계에서 PRD 미스매치 발견 시 PRD 가 우선이고 구현이 변경된다.

---

*문서 작성: 2026-05-18. 선행 도구 Clawket v3.0 운영 약 1개월차에 본 후속 도구를 기획.*
