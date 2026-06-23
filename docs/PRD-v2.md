# SDI 2.0 — 결정주도 시나리오 엔진 (PRD)

> 대상 독자: 시니어 개발자. 본 문서는 **SDI 2.0 재설계 스펙**이다. 현행 v0.7 의 정본 PRD 는 [`PRD.md`](./PRD.md)(D1–D29) 이며, 본 문서가 정의하는 v2 가 구현되어 릴리즈되는 시점에 `PRD.md` 를 대체(supersede)한다. 그 전까지 `PRD.md` 는 **출시된 v0.x 의 진실**, 본 문서는 **목표 형상의 진실**이다.
>
> 본 문서는 SNAPSHOT-ONLY(D12) 를 따른다 — v2 설계를 단일 시점의 일관된 그림으로 기술한다. 단 §2.0 "Supersession map" 은 ADR 성격의 결정 추적이므로 "어떤 D 가 어떤 D 로 대체되는가"를 명시한다 (D12 의 Decision-artifact 예외).

---

## 0. 메타

- **상태**: Draft (구현 착수, `feature/scenario-engine-v2` 브랜치)
- **선행**: SDI v0.7.0 (현행 출시판). 본 v2 는 그 직계 재설계.
- **형상 결정**: develop → `feature/scenario-engine-v2` in-place (D38 / D-β). 인스턴스 분기 아님.
- **한 줄 정체성 (불변)**: 자연어 GWT 시나리오를 1등 시민으로 두는 Scenario-Driven Implementation 엔진. **v2 의 추가 명제**: 시나리오는 *완전한 제품 정의(oracle)* 위에서 *결정의 결정체*로 자라며, 엔진은 그 oracle 을 향해 *두 개의 수렴 루프*를 결정적으로 구동한다.

---

## 1. 문제 정의 — 현행 v0.7 진단

v0.7 은 **수동적 원장(ledger) + 권고 게이트**다. 코드 근거로 확정된 세 결함이 같은 뿌리를 공유한다.

### 1.1 라운드가 1회성 수동 단계다 (회귀 루프 부재)

- `round` 는 3상태 레코드(`Planning→Active→Completed`)다 — `crates/core/src/round.rs:11-15`.
- `round complete` 핸들러는 상태를 `Completed` 로 UPDATE 만 하고 끝, 다음 라운드 생성·연쇄 없음 — `crates/daemon/src/router/round.rs:210-230`.
- daemon 에 스케줄러/타이머/배치 루프가 없음 (`main.rs:28-98` 는 HTTP 수신 대기, `lifecycle.rs:22-38` 는 시그널 처리만). R2 생성은 100% 사람이 `sdi round create` 를 쳐야 함.
- carry-over 는 R2 가 *이미 수동 activate 된 시점에만* 일회성으로 일어남 — `router/round.rs:176-184`.

**결론**: 회귀 검증은 *데이터 모델 개념*으로만 존재하고 *돌아가는 오케스트레이터*로는 존재하지 않는다. 아무것도 루프를 돌리지 않으니 사람은 R1 에서 멈춘다.

### 1.2 시나리오가 형식만 보고 망라성은 판정하지 않는다 (oracle 부재)

- 검증은 `validate_gwt()` — given/when/then 3필드가 비었는지만 검사 — `crates/core/src/scenario.rs:126-138`.
- plan approve 게이트(D8)는 `confirmed ≥ 1` 하나뿐 — `crates/core/src/plan.rs:62-77`, `crates/db/src/repo/plan.rs:121-128`.
- 망라성/커버리지를 판정하는 개념이 코드·PRD(D1~D29) 어디에도 없음. "더 빠진 시나리오 없나?" 루프 없음.

**결론**: 엣지케이스·실패경로·경계조건이 전부 빠진 시나리오 1개로도 plan 이 승인된다. 재시도 루프가 향할 *기준점(oracle)*이 없다.

### 1.3 단계들을 엮는 결정적 척추가 없다 (설계상 의도)

- 모든 단계(impl-coder / test-runner / regression-runner / scenario-decomposer / pattern-orchestrator)는 *사람(메인 세션)이 다음 단계를 호출하는 1회성 단위*다.
- test-runner 가 `failing` 을 뱉어도 daemon 은 evidence 를 기록만 하고 재시도를 유발하지 않음 — `router/task.rs:196-217`.
- 훅(D13/D26 advisory)은 stderr 권고일 뿐 흐름을 구동하지 않음.
- **이것은 명시적 설계 결정이다** — D13/PRD §4 가 "Claude 가 매 협업 결정에 개입(수동)" 을 못박았다.

**진단(인과 3계층)**:
- *Immediate*: complete 가 연쇄 안 함 / GWT 만 검사.
- *Structural*: daemon 에 제어 루프 없음 / 완전성 게이트 없음.
- *Design*: "매 단계 수동" 이 SDI 의 핵심 테넷(D13/§4). **v2 가 깨려는 결정.**

### 1.4 v2 가 푸는 것

1. **oracle 신설** — "완전한 제품 정의" 를 세우고 강제하는 메커니즘.
2. **루프 신설** — 그 oracle 을 향해 명세·구현을 결정적으로 수렴시키는 두 루프.
3. **결정 추출** — oracle 의 빈칸(OPEN)을 SA 시험형 객관식 결정문제로 채우는 질문엔진(웹).

---

## 2. 재설계 결정 (D30–D38)

### 2.0 Supersession map (ADR 성격)

| v1 결정 | v2 처리 | 사유 |
|---|---|---|
| **D8** (plan approve = confirmed≥1) | **D34 로 대체** | 빈약한 게이트 → 전 계층 강제 완전성 게이트 |
| **PRD §4** (매 단계 수동 흐름) | **D30/D31 로 대체** | 수동 1회성 → 결정적 오케스트레이터가 두 루프 구동 |
| **D2** (7 엔티티) | **확장** | +Persona, +UserFlow, +DecisionQuestion/Option/Answer, +SSoT 노드 (D32/D33/D35) |
| **D13** (멀티에이전트 본체) | **재해석(유지)** | 본체 유지. 단 멀티에이전트 fan-out 을 *수동 호출*이 아니라 *루프가 구동*. "Claude 가 루프 안에 있다" 는 *결정적 드라이버*로 재정의 — 매 단계 수동 클릭커가 아니라. |
| **D5/D12** (GWT 강제 / snapshot) | **유지** | v2 의 DetailScenario 에도 그대로 적용 |
| **D6/D7** (round 모드 / 단일 엔진) | **유지·실현** | 모드는 유지하되 비로소 *자동으로 돈다* |
| **D21~D29** (위임/패턴/되돌림/claim) | **유지** | v2 엔진이 이 메커니즘을 *더 강하게* 실현 (척추가 패턴 fan-out 을 구동) |

### 2.1 신규 결정

**D30 — Engine over ledger (round = 동사).**
round 는 더 이상 수동 진행 레코드가 아니라 **결정적 오케스트레이터가 소유하는 실행 루프**다. 명세·구현 수렴 루프를 워크플로우 오케스트레이터(§5.1)가 구동한다. PRD §4 의 "매 단계 수동" 흐름을 대체한다. *근거*: §1.3 진단의 Design-level 원인 직접 대응. round 가 명사인 한 아무도 루프를 돌리지 않는다.

**D31 — 이중 수렴 루프.**
엔진은 두 루프를 갖는다. 각 루프는 검증 가능한 종료 조건(oracle)을 갖는다 — 기준 없는 루프는 헛돎이므로.
- **바깥 루프 — 명세 수렴**: L0→L1→L2 를 돌며 "(Persona×Capability) 미커버 / flow 단계 미커버 GWT / 미해소 OPEN / 미질문 결정지점" 이 0 이 될 때까지 loop-until-dry. 매 회 completeness-critic 이 "무엇이 빠졌나" 판정.
- **안쪽 루프 — 구현 수렴**: oracle(L2)에 대고 decompose→impl→test→verdict 를 돌리고, `failing/impacted` 면 bounded N회 재시도 또는 escalate, 전부 pass 면 round complete, 이전 round 대비 회귀 검출 시 **자동으로 다음 round open**.

**D32 — 완전성 oracle = SSoT 문서 그래프 (자체 네이티브). [D-α=B]**
"완전한 제품 정의" 를 SDI 네이티브 엔티티 그래프로 보유한다. ssot-studio 의 검증된 모델(노드별 4축 측면 + `OPEN` 마커 + dangling-edge 0 = 완전)을 흡수하되, **외부 ssot-studio 를 소비(A)하거나 세 도구를 통합(C)하지 않고 SDI 자체 모델(B)로** 둔다. *근거*: ① self-containment — 모든 SDI 프로젝트가 SSoT 레포 없이 동작해야 한다. ② D35 질문엔진이 ssot-studio 의 OPEN-채움 메커니즘의 *상위호환*이라 외부 의존이 불필요. ③ C(통합)는 Clawket/SDI/ssot 3도구 병합이라는 별도 전략 결정이므로 본 PRD 가 단독으로 못박지 않는다 (미래 옵션으로 열어둠).
완전성 두 축(ssot-studio `verify.mjs:74-145` 계승):
- **측면 완전성**: 노드 kind 별 필수 측면이 안 빔. 빈칸은 `OPEN` 으로 명시.
- **연결 완전성**: dangling edge = 0.

**D33 — 시나리오 3계층 (Persona × UserFlow × DetailScenario).**
v1 의 단일 GWT Scenario 를 3계층으로 재정의한다.
- **Persona** (L0 노드): 누구(definition) · 무엇을 이루려 하나(purpose). 제품 정의 그래프의 일부.
- **UserFlow** (L1, 신규 1등 시민): 페르소나 1명 × 목적 1개의 **완성된 서비스 기준 전체 여정**. "persona.X 가 [목적]으로 [flow]를 할 때 서비스는 이렇게 동작한다." 이것이 명세 수렴 루프의 *기준*.
- **DetailScenario** (L2 = 기존 Scenario 재정의): GWT 검증 명제. 한 UserFlow 의 한 단계를 검증. `belongs_to_flow` FK 로 flow 에 앵커. D5(GWT 강제)·D29(claimed_resources) 계승.
Task 는 DetailScenario 에서 분해(D3 유지).

**D34 — 전 계층 강제 완전성 게이트 (D8 대체).**
각 계층 경계가 권고가 아니라 **하드 게이트**다.
1. L0 verify(OPEN 0 · dangling 0) ✅ → L1(UserFlow) 작성 허용.
2. L1 (Persona × Capability) 커버리지 100% ✅ → L2 분해 허용.
3. L2 (모든 flow 단계가 DetailScenario 로 커버 + OPEN 0) ✅ → **plan approve** (D8 의 `confirmed≥1` 대체).
4. L3 (전 DetailScenario pass + 회귀 0) ✅ → round 루프 종료.
*근거*: oracle 이 강제되지 않으면 재시도 루프가 향할 기준이 없다(§1.2).

**D35 — 결정-추출 질문엔진 (SA 시험형).**
oracle 의 빈칸(OPEN)을 *막연한 채움 요청*이 아니라 **맥락이 완전히 채워진 객관식 결정문제**로 승격한다. OPEN 은 사라지는 게 아니라 **confirm 게이트를 막는 Decision 요청**이 된다 (미지를 명시하되 해소 전엔 통과 불가).
- **질문 구조**: 맥락(상세 시나리오) + N개 보기 + 보기별 해설("왜 더 맞고 그른가") + LLM 권장안 + `+@` 주관식 옵션.
- **두 종류 (UI 가 구분 강제)**: §2a(ELIMINATION FIRST) 를 질문 생성 *전에* 돌린다.
  - **Type-Fact** (베스트프랙티스·아키텍처): 소거 후 1개 생존 → 질문이 아니라 *해설 붙은 자동결정*(LLM 권장, 투명성용 보기 표시).
  - **Type-Preference** (UX·도메인·비즈니스): 2개+ 생존 → 정답 없음, 보기는 트레이드오프 카드, 순수 사용자 결정.
- **메타 완전성**: 진짜 어려운 건 "답하기" 가 아니라 "다 물었는지" 다. 질문 생성 자체가 다양한 렌즈(정상/실패/경계/동시성/보안)로 loop-until-dry — completeness-critic 이 "더 물을 게 없다" 할 때까지. "미답 질문 0" 게이트는 "미질문 0" 이 보장될 때만 의미.
- **적응형 분기 트리**: 답에 따라 후속 질문이 갈린다(Q3=B → Q3.1/Q3.2 잠금해제).
- **provenance**: 각 생성 DetailScenario 는 어떤 질문-답에서 나왔는지 기록(D23 계승).

**D36 — 웹 SPA = 1급 작성 surface.**
`plugin/web/` 대시보드(현재 sdid 의 tower-http ServeDir 가 서빙)를 **읽기 대시보드에서 작성 surface 로 승격**. 두 답변 모드를 동등 지원:
- **일괄 모드**: 질문 1패스 생성 → 오프라인 응답 → 제출.
- **대화 모드**: 1문제씩 풀고 즉시 LLM 과 대화해 결론(정답 확인하듯).
daemon 에 `DecisionQuestion / Option / Answer` 엔티티 + 엔드포인트 추가.

**D37 — 구독 LLM 통신 = llm-bridge 사이드카 (ACP + SDK). [D-γ]**
sdid(Rust)는 Node provider 를 직접 호출할 수 없다. agent-devtools 가 검증한 두 provider 를 그대로 재사용한다 — `createAcpProvider()`(Claude Code CLI 를 child 로 spawn, stdio JSON-RPC) + `createSdkProvider()`(`@anthropic-ai/claude-agent-sdk` in-process) + SSE transport 를 **Node `llm-bridge` 사이드카**로. sdid 는 `/v1/agent/stream` 을 프록시만. 둘 다 `~/.claude` OAuth 재사용 → API 비용 0 (§5a 정렬).
- 자연 정렬(강제 아님, 클라이언트가 per-request 선택): 일괄 ↔ SDK(stateless), 대화 ↔ ACP(stateful 세션 풀).
- *근거*: 단일 진실 — 사용자 본인의 동작하는 도구 재사용. "두 방식 동등 지원" 이 거의 공짜.
- 3번째 채널(Claude Code 세션이 열려 있으면 그 세션 자체가 구독 LLM)은 인프라-0 베이스라인으로 공존.
- 비용 정직: Node 런타임 의존 추가. 단 플러그인은 이미 Node 훅(`sdi-hooks.cjs`)을 돌리고 Claude Code 자체가 Node 위 → 새 toolchain 강요 아님.

**D38 — 형상 = develop 피처브랜치 in-place. [D-β]**
인스턴스 분기(§3 instance-forking) 대신 `develop` → `feature/scenario-engine-v2`. *근거*: 도그푸딩 일시중단·pre-1.0 단계라 단일 진실 유지가 정당. §1b(브랜치 전략 agnostic)에 따라 사용자가 선택한 흐름을 따름.

---

## 3. 모델

### 3.1 엔티티 (v1 7종 + v2 추가)

v1 7 엔티티 유지(Plan / Requirement / Decision / DetailScenario(←Scenario) / Round / AutonomyPolicy / CollaborationPattern). v2 추가:

| 엔티티 | 계층 | 역할 |
|---|---|---|
| **SsotNode** | L0 | 제품 정의 그래프 노드. kind ∈ {Persona, Capability, Domain, Concept, Invariant, Decision-ref, …}. 4축 측면 + OPEN 마커. |
| **SsotEdge** | L0 | 노드 간 관계 (servesPersona, relatesTo, …). dangling 0 강제. |
| **Persona** | L0 | SsotNode 의 특수형(1등 노출). definition · purpose. |
| **UserFlow** | L1 | 페르소나×목적의 완성-서비스 여정. flow steps 보유. |
| **DecisionQuestion** | 횡단 | OPEN 을 승격한 결정문제. type ∈ {fact, preference}. 적응형 트리(parent_question_id). |
| **Option** | 횡단 | DecisionQuestion 의 보기. rationale(해설) + is_llm_recommended. |
| **Answer** | 횡단 | 사용자가 고른 Option 또는 `+@` free-text. → 결정적으로 시나리오/노드 생성 + OPEN close. |

### 3.2 SsotNode 스키마 (요지)

```
SsotNode {
  id, project_id, kind, title,
  facets_json,        // 4축: business(purpose,value) / domain(definition) / system / governance(owner,lifecycle,confidence)
  open_markers_json,  // [{ id, field, description, question_id? }]  ← OPEN = 차단형 결정요청
  produced_via_pattern_id,   // D23 provenance
  created_at, updated_at
}
```
완전성: `open_markers` 중 미해소가 있으면 측면 미완. 측면/연결 완전성은 daemon 의 결정적 verify 가 판정(ssot-studio verify.mjs 계승, Rust 포팅).

### 3.3 UserFlow 스키마 (요지)

```
UserFlow {
  id, project_id, persona_id (FK), purpose,
  steps_json,         // 완성 서비스 기준 전체 여정 단계
  covers_capabilities_json,  // 이 flow 가 커버하는 Capability id 배열
  status,             // draft → confirmed
  produced_via_pattern_id
}
```
L1 완전성 = 모든 (Persona × Capability) 가 ≥1 UserFlow 로 커버.

### 3.4 DetailScenario (= v1 Scenario 재정의)

v1 Scenario 스키마 유지 + `belongs_to_flow_id (FK)` + `covers_flow_step` 추가. D5(GWT 강제)·D29(claimed_resources)·confirm 상태 계승. L2 완전성 = 모든 flow step 이 ≥1 DetailScenario 로 커버 + OPEN 0.

### 3.5 DecisionQuestion / Option / Answer 스키마 (요지)

```
DecisionQuestion {
  id, project_id, scope_ref,   // 어떤 노드/flow/OPEN 을 채우는가
  type,                        // 'fact' | 'preference'
  context_md,                  // SA 시험 stem (상세 시나리오 맥락)
  parent_question_id?,         // 적응형 분기 트리
  status                       // open → answered | auto_decided
}
Option {
  id, question_id, label, body_md,
  rationale_md,                // 왜 더 맞고 그른가 (해설)
  is_llm_recommended           // LLM 권장 마커
}
Answer {
  id, question_id, chosen_option_id?, free_text?,   // +@ 주관식
  answered_by, answered_at,
  generated_refs_json          // 이 답이 생성한 노드/flow/scenario ids (provenance)
}
```

### 3.6 Round 의미 (재정의, D30)

Round 는 *구현 수렴 루프의 1회전*이다. R1 = 신규 구현(D7), R2+ = 회귀 포함 검증. **차이**: v2 에서 R→R+1 전이는 엔진이 회귀 검출 시 *자동으로* 연다. carry-over(D6 strict-regression) 유지. 종료 조건 = 전 DetailScenario pass + 회귀 0 (D34-4).

---

## 4. 핵심 워크플로우 (두 루프)

### 4.1 바깥 루프 — 명세 수렴 (oracle 빌드)

```
while (명세 미완):
  1. L0: SsotNode 그래프에서 OPEN/dangling 스캔 → 다양한 렌즈로 결정지점 발굴
  2. 각 결정지점 → §2a 소거 → 1생존이면 auto_decided(해설), 2+생존이면 DecisionQuestion(type=preference)
  3. 웹(D36)에 질문 제시 → 사용자 답변(일괄/대화) → Answer → 결정적 생성(노드/flow/scenario) + OPEN close
  4. completeness-critic: "빠진 Persona×Capability / flow step / 미질문 결정지점 있나?"
  5. 없으면 loop 종료(oracle 완성), 있으면 계속
종료 시: L0 verify ✅, L1 커버리지 100% ✅, L2 커버리지 100%+OPEN 0 ✅ → plan approve 가능(D34)
```

### 4.2 안쪽 루프 — 구현 수렴 (round 회귀)

```
round = open(plan)
loop:
  1. pattern-orchestrator(D13/D22) 가 needs-verification 집합을 fan-out 설계
  2. decompose → impl-coder(병렬) → test-runner → verdict
  3. for verdict in failing|impacted:
       retry impl (bounded N) | escalate(decision-resolver/disruption-analyst)
  4. 전 DetailScenario pass?  →  round complete
  5. 이전 round 대비 회귀 검출?  →  round = open(next)  (자동, D30)
  6. 회귀 0 + 전 pass  →  루프 종료(구현 수렴)
```

### 4.3 질문엔진 사이클 (D35 상세)

1. 빈칸/모호 지점 수집(L0/L1/L2).
2. §2a 소거 → fact(1생존)/preference(2+생존) 분류.
3. preference 만 DecisionQuestion 생성: stem(맥락) + Option[] (label+rationale+권장) + `+@`.
4. 웹 제시. 대화 모드면 ACP 세션으로 보기별 토론(정답 확인하듯).
5. Answer → 결정적 컴파일(노드/flow/scenario row + OPEN close + 후속 질문 잠금해제).
6. completeness-critic 통과까지 2~5 반복.

---

## 5. 시스템 아키텍처

### 5.1 척추 — 워크플로우 오케스트레이터 (신설)

두 루프(§4)를 소유하는 **결정적 오케스트레이터**. 위치는 daemon 이 아니라 **오케스트레이션 레이어**다 — daemon 엔 LLM 이 없어 비결정 단계(시나리오 발굴·구현·판정)를 못 돌린다. 오케스트레이터는:
- 비결정 단계 → LLM 서브에이전트(스킬/Agent) 호출.
- 결정 단계 → daemon CLI/MCP(상태 전이·verify·carry-over·complete) 호출.
- 실패 1급 처리 + bounded retry + loop-until-dry.
에어스튜디오 파이프라인 커맨드 형상이며, Claude Code `Workflow`(pipeline/parallel/loop-until-dry/N-vote)에 매핑된다. **daemon 은 원장(SoT)으로 남는다.**

### 5.2 llm-bridge 사이드카 (D37)

```
Browser(SPA) ──SSE──▶ sdid(/v1/agent/stream proxy) ──▶ llm-bridge(Node)
                                                          ├─ createAcpProvider()  → spawn `claude` (stdio JSON-RPC)
                                                          └─ createSdkProvider()  → @anthropic-ai/claude-agent-sdk
                                                          둘 다 ~/.claude OAuth (구독)
```
provider 는 클라이언트가 per-request 선택. agent-devtools `packages/core/src/providers/{acp,sdk}.ts` + `widget-core/.../sse-transport.ts` 재사용.

### 5.3 웹 작성 surface (D36)

`plugin/web/` (Vite/React 19/Tailwind 4) 를 작성 surface 로 확장. 질문 카드(보기+해설+권장+`+@`), 일괄/대화 토글, oracle 완전성 대시보드(OPEN/dangling/커버리지 진행률), round 루프 라이브 뷰.

### 5.4 daemon 역할 (원장 + 결정적 verify)

- 신 엔티티 CRUD + 엔드포인트(SsotNode/Edge, UserFlow, DecisionQuestion/Option/Answer).
- 결정적 verify(측면/연결 완전성, ssot-studio verify.mjs → Rust 포팅).
- 강제 게이트(D34)를 상태 전이 API 에 박음(plan approve = L2 완전성 통과).
- XDG 불변(LM-8) 유지.

---

## 6. 인수 기준 (Acceptance)

1. SsotNode 그래프에 OPEN 또는 dangling 이 있으면 UserFlow 작성이 차단된다(D34-1).
2. (Persona×Capability) 미커버가 있으면 L2 분해가 차단된다(D34-2).
3. flow step 미커버 또는 OPEN 잔존 시 plan approve 가 거부된다(D34-3, D8 대체).
4. round 가 회귀 검출 시 다음 round 가 **자동으로** 열린다(D30).
5. 명세 수렴 루프가 completeness-critic 통과까지 자동 반복하고, 미질문 결정지점 0 을 보장한다(D31/D35).
6. 질문은 fact/preference 로 분류되고, fact(1생존)는 질문이 아니라 자동결정+해설로 처리된다(D35).
7. 웹에서 일괄·대화 두 모드 모두로 답변할 수 있고, 답변이 결정적으로 시나리오/노드를 생성한다(D36).
8. 웹↔LLM 통신이 ACP·SDK 두 방식 모두로 동작하며 구독(OAuth)을 쓴다(D37, API 비용 0).
9. 생성 시나리오는 answer-provenance 를 갖는다(D35/D23).

---

## 7. 비목표 (v2 범위 외)

- Mac App Store 등록 (사용자 명시 "논외").
- Clawket/SDI/ssot-studio 3도구 통합(C) — 미래 전략 옵션으로 열어두되 본 v2 범위 아님(D32).
- 외부 A2A 프로토콜(D15 유지 — v1 비목표 계승).

---

## 8. 단계 로드맵 (정상 완성도 — 일정 압축 분할 아님)

각 Phase 는 제품 완성도상 독립적으로 의미 있는 단위다(단순 일정 분할 금지, MVP 압축 금지).

- **Phase 0 — 결정체화**: 본 PRD. ✔(진행 중)
- **Phase 1 — oracle 데이터 모델**: SsotNode/Edge·UserFlow·DetailScenario 재정의 + 결정적 verify(Rust 포팅) + 강제 게이트(D34) 의 daemon 측 enforcement. (crates/db migration + crates/core + crates/daemon/router)
- **Phase 2 — 질문엔진 백엔드**: DecisionQuestion/Option/Answer 엔티티 + 소거-기반 질문 생성 + answer→결정적 컴파일 + completeness-critic.
- **Phase 3 — llm-bridge 사이드카**: agent-devtools provider 재사용 + sdid 프록시(/v1/agent/stream).
- **Phase 4 — 웹 작성 surface**: 질문 카드 UI + 일괄/대화 모드 + oracle/round 라이브 대시보드.
- **Phase 5 — 척추(오케스트레이터)**: 두 루프 워크플로우 구동 + bounded retry + loop-until-dry + round 자동 advance.

> Phase 간 게이트: 5+ 파일 multi-file refactor 단위에서만 명시 승인(`mechanical-overrides §2`). Class B 일반 작업은 elimination-first 로 진행.
