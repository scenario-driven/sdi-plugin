<p align="center">
  <img src="./assets/sdi-logo.svg" alt="SDI — Scenario-Driven Implementation" width="440">
</p>

<p align="center">
  <a href="./README.md">English</a> · <strong>한국어</strong>
</p>

> 자연어 GWT 시나리오를 1등 시민으로 둔다. 여러 LLM 에이전트가 무엇을 만들지 제안·비평하고 합의에 도달한 뒤, 라운드를 거치며 구현·검증·자동 회귀까지 수행한다 — 사용자가 프롬프트에 계속 묶여 있지 않아도 통제할 수 있는, 스코프별 자율성 정책 아래에서.

---

## 이게 뭔가요?

SDI 는 TDD(1990년대) 와 BDD(2000년대) 의 LLM 시대 후속이다. 계보는 다음과 같다.

| | 명세 형식 | 검증자 | 누가 읽는가 |
|---|---|---|---|
| TDD | 테스트 코드 | 테스트 러너 | 사람 + 러너 |
| BDD | Gherkin DSL | 스텝 정의 + 러너 | 사람 (스텝 글루는 사람이 유지보수) |
| **SDI** | **자연어 Given/When/Then** | **LLM 에이전트** | **LLM 이 직접 — 컴파일 단계 없음** |

작업의 단위는 **시나리오**다. 플랜은 시나리오 집합을 확정한다. 전문가 에이전트들이 런타임 태스크를 분해하고, 구현을 제안하고, 서로를 비평한 뒤에야 자율성 정책으로 게이팅된 합의로 수렴한다. 다음 라운드는 이전 시나리오를 회귀 검증으로 자동 재생한다.

정체성 및 전체 명세: **[`docs/PRD.md`](./docs/PRD.md)** — 정본 PRD 는 이 저장소에 있으며, 결정 사항 D1–D29 는 §2 에 있다.

---

## 7대 1등 엔티티

| 엔티티 | 역할 |
|---|---|
| **Plan** | 승인된 의도. 승인 게이트 = 유효한 GWT 를 가진 시나리오 1개 이상 (태스크 수는 무관). |
| **Requirement** | SNAPSHOT-ONLY 자연어 요구. 최신 스냅샷만이 유일한 진실이며, 변경 이력은 Decision 에 산다. |
| **Decision** | `kind ∈ {proposal, critique, consensus, dissensus}` 를 갖는 append-only ADR. consensus 가 게이트 통과 형태다. `reversal_plan` + `blast_radius_score` 를 동반한다 (D28). |
| **Scenario** | 엄격한 Given/When/Then. `tags`, `depends_on` DAG, `produced_by`/`verified_by` 에이전트(M4 계약) 와 멀티세션 안전을 위한 `claimed_resources_json` + `claim_status` 를 갖는다 (D29). |
| **Round** | R1 은 신규 개발, R2+ 는 회귀. 기본 모드 `strict-regression` 은 이전에 통과한 모든 시나리오를 재생한다. |
| **AutonomyPolicy** | 스코프별(plan / decision_kind / pattern_kind / global) 모드 ∈ {L3, L4, L5} + `l5_threshold` + `pattern_depth_cap` + `plan_single_session_lock`. 사람 게이트의 위치를 결정한다. |
| **CollaborationPattern** *(D22, v0.5)* | Kind ∈ {workflow, graph, swarm, agents-as-tools, direct}, applies_to ∈ {plan, requirement, scenario, task, decision, round}. kind 별 형상(steps/reviewers/fan_out/peer_registration) 을 가진 영속 매니페스트. 모든 작업 엔티티가 `produced_via_pattern_id` 를 기록한다. |

멀티 에이전트 기반을 떠받치는 비-1등 영속 엔티티가 둘 있다: **AgentNote** (M1 블랙보드, append-only 저널) 와 **AgentSpec** (M5 런타임 전문가 등록, D26 시빌 방지를 위한 `stance ∈ {proposer, devil_advocate, schema_guardian, performance_reviewer, security_reviewer, neutral}` 포함).

---

## 멀티 에이전트 거버넌스 (D13–D29)

- **D13 — 멀티 에이전트가 본체.** 단독 `@main` 실행은 안티패턴이다. 모든 흐름은 전문가 에이전트들이 소통한다고 가정한다.
- **D14 — AutonomyPolicy 는 1등 엔티티.** 스코프별 모드가 SQLite 에 영속되며 Decision 적용을 게이팅한다.
- **D15 — 내장 4대 패턴.** Workflow, Graph, Swarm, Agents-as-Tools 가 SDI 내부에 산다. 외부 A2A 프로토콜은 v1 범위 밖.
- **D16 — 기본값 = 정책에 따라 행동.** "매번 물어보기" 가 아니라 — 사용자는 개입 윈도우를 토글하지, 결정마다 프롬프트를 받지 않는다.
- **D17 — 모드 기본값.** 신규 플랜은 **L5** 기본; 외부 표면(publish/deploy/외부 API) 을 가진 플랜은 **L4** 기본; `decision_kind ∈ {architecture, schema, naming-canonical}` 은 플랜 모드와 무관하게 **L4** 로 강제된다.
- **D18 — 서킷 브레이커 상시 작동.** UI 액션 하나가 모든 정책 행을 즉시 L3 로 강등한다; 처리 중인 결정은 다음 게이트에서 적용된다.
- **D19 — 기반은 모드 독립으로 작동.** M1 블랙보드, M2 핸드오프, M3 협상, M4 시나리오-계약, M5 자기조직화는 어떤 모드에서도 계속 작동한다; 모드는 합의에 대한 사용자 게이트 위치만 정한다.
- **D20 — 합의가 게이트 단위.** 단일 에이전트 결정 = L3 최대. 멀티 에이전트 합의가 L4/L5 를 해제한다. dissensus 는 모드와 무관하게 항상 에스컬레이션된다.
- **D21 — 강제 위임 게이트.** 오케스트레이터(메인 세션) 는 실행 도구(`Edit`/`Write`/`NotebookEdit`/변경성 `Bash`) 호출이 금지된다. PreToolUse 훅이 `hookInput.agent_id` 부재를 감지해 호출을 차단한다 — 유일하게 정당한 경로는 `Agent` 로 스폰된 전문가 서브에이전트뿐이다. 이것이 D13 의 기계적 강제 면모다: 안티패턴이 문서상으로만이 아니라 구조적으로 불가능해진다.
- **D22 — 일곱 번째 엔티티로서의 CollaborationPattern.** AWS 의 4대 패턴(Workflow / Graph / Swarm / Agents-as-Tools) 이 라이프사이클(pending → active → converged | dissensus | aborted) 을 가진 영속 DB 행이 된다. `direct` 는 안티패턴 마커이지 탈출구가 아니다.
- **D23 — 패턴 출처는 NOT NULL.** 모든 신규 작업 엔티티가 `produced_via_pattern_id` 를 갖는다; 이를 누락한 메인 세션은 빨간 대시보드 배지 + L3 상한 + 감사 로그가 붙은 자동 `direct` 행을 받는다.
- **D24 — DAG 를 통한 패턴 재귀.** `parent_pattern_id` self-FK, depth ≤ `AutonomyPolicy.pattern_depth_cap` (기본 3). 패턴의 스텝이 서브패턴을 스폰할 수 있다; 사이클은 차단된다.
- **D25 — 패턴 스코프 자율성.** 기본값: workflow=L5, graph=L5, swarm=L4, agents-as-tools=L4, direct=L3. (plan 모드, pattern 모드) 중 가장 엄격한 쪽이 이긴다.
- **D26 — 시빌 방지를 갖춘 4대 패턴 무결성 게이트.** Graph 합의는 `서로 다른 (AgentSpec.name, AgentSpec.stance) 튜플 2개 이상` 을 요구한다 — 동일 stance 의 `impl-coder` 인스턴스 둘로는 더 이상 다양성을 위장할 수 없다. Workflow 는 순차 증거와 `steps ≥ 2` 가 필요하고; swarm 은 `fan_out ≥ 2` 에 더해 스폰 깊이·자기스폰 루프 차단이 필요하며; agents-as-tools 는 peer 등록과 `peer ≥ 1` 이 필요하다.
- **D27 — 패턴 형상 & 선택 게이트.** 형상 검증은 `pending → active` 전이에서 실행된다. 가짜 패턴(1-스텝 workflow, 단일 인스턴스 swarm, 레지스트리가 빈 agents-as-tools) 은 `direct` 의 L3 상한을 우회할 수 없다.
- **D28 — L5 에 대한 1등 제약으로서의 가역성.** Decision.reversal_plan (역방향 마이그레이션 / git revert SHA / 파일시스템 스냅샷 / 보상 액션) + Decision.blast_radius_score 가 L5 자동 적용을 게이팅한다: 형상 유효 AND reversal_plan 존재 AND blast_radius_score ≤ `AutonomyPolicy.l5_threshold` (기본 5). reversal-runner 전문가가 롤백을 append-only Decision 으로 실행한다.
- **D29 — 멀티세션 리소스 클레임.** Scenario.claimed_resources_json (경로 glob) + claim_status 가 데몬에 결정 라우터 역할을 부여한다: 세션 간 겹침은 PreToolUse 에서 merge-or-wait 프롬프트와 함께 차단된다. 충돌이 잦은 플랜을 위한 선택적 `plan_single_session_lock`.

전체 L3/L4/L5 의미론, 스코프 매트릭스, 서킷 브레이커 트리거, 위임 게이트 도구 분류, 패턴 무결성 규칙은 **[`docs/PRD.md`](./docs/PRD.md)** §3.7, §3.9, §5 Layer 0 / 1.5 / 2.6 / 2.7 / 2.8 / 3 에 상세히 기술되어 있다.

---

## 슬래시 명령어

| 명령어 | 소유자 | 용도 |
|---|---|---|
| `/scenario` | 이 플러그인 | 시나리오 생성 / 목록 / 폐기 (엄격한 GWT). |
| `/round` | 이 플러그인 | 회귀 자동 재생과 함께 R1 또는 R2+ 시작. |
| `/plan` | 이 플러그인 | 플랜 생성, Requirement 관리, 승인 게이트. |
| `/req` | 이 플러그인 | 요구사항 스냅샷 (SNAPSHOT-ONLY). |
| `/decide` | 이 플러그인 | `kind` (proposal → critique → consensus / dissensus) 와 함께 Decision 추가. `reversal_plan` + `blast_radius_score` 동반 (D28). |
| `/consensus` | 이 플러그인 | 멀티 에이전트 합의 라운드 — 제안 / 비평 / 수렴 — 를 진행. 활성 CollaborationPattern 의 형상으로 게이팅됨 (D20, D26). |
| `/autonomy` | 이 플러그인 | 스코프별 AutonomyPolicy 조회 / 변경; 서킷 브레이커 노출. `pattern_kind`, `l5_threshold`, `pattern_depth_cap`, `plan_single_session_lock` 포함. |
| `/agent-note` | 이 플러그인 | AgentNote (M1 블랙보드) 추가 — hypothesis / observation / question / handoff / dissent / evidence. |
| `/pattern` *(D22, v0.5)* | 이 플러그인 | CollaborationPattern 생성 / 목록 / 진행. workflow / graph / swarm / agents-as-tools 매니페스트용 서브명령. |
| `/sdi-status` | 이 플러그인 | 데몬의 해석된 상태 스냅샷 — 활성 플랜, 시나리오, 자율성 모드, 활성 패턴, 클레임 원장. |
| `/goal` | Claude Code 내장 | 직교적. SDI 는 가로채지 않는다. |

---

## 저장소 구조

이것은 **본체가 Rust 워크스페이스인 Claude Code 플러그인**이다. 플러그인 셸, cli, 데몬은 모두 같은 저장소의 서로 다른 표면이다.

```
sdi-plugin/
├── Cargo.toml               # 워크스페이스 루트 (resolver = 2)
├── crates/
│   ├── cli/                 # `sdi` 바이너리 — 사용자/LLM 진입점. `sdi mcp` 서브명령을 호스팅.
│   ├── daemon/              # `sdid` 바이너리 — 백그라운드 데몬 (HTTP + 유닉스 소켓).
│   ├── mcp/                 # stdio MCP 서버 라이브러리, cli 에 임베드됨.
│   ├── core/                # 도메인 모델: Plan / Requirement / Decision / Scenario / Round / AutonomyPolicy / CollaborationPattern + AgentNote / AgentSpec.
│   └── db/                  # SQLite 저장소 어댑터 (rusqlite + r2d2; FTS5 키워드 검색, 벡터 검색은 보류).
├── plugin/                  # Claude Code 플러그인 셸
│   ├── .claude-plugin/plugin.json
│   ├── .mcp.json
│   ├── hooks/hooks.json
│   ├── web/                 # 대시보드 SPA (Vite/React 19/Tailwind 4); `sdid` 가 dist/ 를 서빙.
│   └── README.md
├── assets/                  # 로고 + 브랜드 SVG
├── docs/
│   ├── PRD.md               # 정본 제품 명세 (D1–D29)
│   ├── ARCHITECTURE.md      # 이 저장소의 아키텍처 + 멀티 에이전트 레이어
│   └── …
├── README.md                # 영문 README
├── CLAUDE.md                # 기여자 / 에이전트용 AI 컨텍스트
├── LICENSE                  # MIT
└── .gitignore
```

대시보드 SPA 는 이 저장소의 [`plugin/web/`](./plugin/web/) 에 있으며 `sdid` 가 tower-http `ServeDir` 로 직접 서빙한다. 자율성 패널, 결정 타임라인, 에이전트 노트 블랙보드, 패턴 뷰는 모두 데몬의 HTTP API + `/events` SSE 로부터 렌더링된다.

이 저장소와 함께하는 별도 org 저장소가 둘 있다:

- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — Tauri 2 셸. `plugin/web/dist` 를 번들하고 `sdid` 를 사이드카로 스폰한다. 해석된 자율성 모드 + 활성 패턴 배지를 창 제목과 트레이에 미러링하고, 서킷 브레이커를 전역 단축키(Cmd+Shift+L / Ctrl+Shift+L) 로 노출한다.
- [`sdi-docs`](https://github.com/scenario-driven/sdi-docs) — Astro/Starlight 랜딩 + 이중언어(ko / en) 가이드 사이트. 이 저장소의 `docs/PRD.md` 를 미러링하는 프레젠테이션 레이어.

---

## 설치

미리 빌드된 `sdi` + `sdid` 바이너리(macOS + Linux × x86_64 + aarch64) 가 Claude Code 플러그인 마켓플레이스를 통해 배포된다 — Rust 툴체인이 필요 없다.

```text
/plugin marketplace add scenario-driven/sdi-plugin
/plugin install sdi@scenario-driven-sdi-plugin
```

플러그인 셸은 [`plugin/`](./plugin/) 아래에 있다; 마켓플레이스는 이를 `dist` 브랜치에서 가져온다 (바이너리는 각 [GitHub Release](https://github.com/scenario-driven/sdi-plugin/releases) 에 첨부됨).

---

## 소스에서 빌드

```sh
cargo build
```

두 바이너리를 빌드한다: `sdi` (cli) 와 `sdid` (daemon). 대시보드 SPA 를 다시 빌드하려면:

```sh
pnpm --dir plugin/web install
pnpm --dir plugin/web build
```

---

## 선행 작업

SDI 는 **Clawket v3.0** (약 1개월 운영) 의 직계 후속이다. Clawket 은 로컬 SQLite + 데몬 + MCP 아키텍처로 LLM 이 장기 실행 작업 상태를 운반할 수 있음을 입증했으나, 태스크 중심의 Jira 계보 모델은 LLM 주도 검증, 자동 회귀, 멀티 에이전트 거버넌스를 가능케 하지 못했다. SDI 는 시나리오를 중심에 다시 두고 그 격차를 메우기 위한 멀티 에이전트 기반을 더한다.

마이그레이션 매핑은 [`docs/PRD.md`](./docs/PRD.md) §9 에 있다. SDI 는 Clawket 의 버전 업이 아니라, 새 org 의 새 도구다.

---

## 라이선스

MIT. `LICENSE` 참조.
