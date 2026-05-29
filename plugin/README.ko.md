# SDI — Claude Code 플러그인 셸

[English](./README.md) · **한국어**

이 디렉터리는 SDI(Scenario-Driven Implementation) 의 **Claude Code 플러그인 표면**이다. SDI 본체(`crates/` 워크스페이스: `cli` + `daemon` + `mcp` + `core` + `db`) 와 동일한 저장소의 일부다. 플러그인은 별도 패키지가 아니라 — 이 저장소의 여러 얼굴 중 하나다.

정본 명세: [`../docs/PRD.md`](../docs/PRD.md) (결정 사항 D1–D29).

## 구성

| 경로 | 역할 |
|---|---|
| `.claude-plugin/plugin.json` | 플러그인 매니페스트. `commands/`, `agents/`, `skills/`, 마켓플레이스 메타데이터를 선언. |
| `.mcp.json` | MCP 서버 등록. `sdi mcp` (CLI 의 stdio MCP 서브명령) 를 스폰. |
| `hooks/hooks.json` | `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `SubagentStart`, `SubagentStop` 에 대한 훅 라우팅. |
| `adapters/claude/*.cjs` | `shared/sdi-hooks.cjs` 에 위임하는 얇은 Claude Code 전용 래퍼. |
| `adapters/shared/sdi-hooks.cjs` | 설치 로직 + 훅 본문의 단일 진실 공급원 (멱등 `ensureInstalled`, 데몬 스폰, 활성 태스크 / 위임 / 패턴 / 클레임 가드). |
| `commands/*.md` | 슬래시 명령어 (D11 + v0.5): `/plan`, `/req`, `/scenario`, `/round`, `/decide`, `/consensus`, `/autonomy`, `/agent-note`, `/pattern`, `/sdi-status`. |
| `agents/*.md` | 전문가 서브에이전트 정의 (아래 참조). |
| `skills/{sdi-overview,sdi-scenario,sdi-round,sdi-evidence}/SKILL.md` | 오리엔테이션, GWT 변환, 라운드 라이프사이클, 증거 기록을 다루는 4개의 태스크 스코프 스킬. |
| `scripts/setup.cjs` | `ensureInstalled` 로의 수동 / CI 진입 (`SessionStart` 와 동일 코드 경로). |
| `bin/`, `daemon/bin/` | 번들된 `sdi` 및 `sdid` 바이너리의 설치 대상 (릴리스 번들 레이아웃 사용 시 `ensureInstalled` 가 채움). |

## 전문가 에이전트

플러그인은 `agents/` 아래에 11개의 전문가 에이전트를 제공한다. 멀티 에이전트 협업이 SDI 의 본체이므로(D13), 오케스트레이터는 결코 실행하지 않는다 — 위임한다.

| 에이전트 | 역할 |
|---|---|
| `scenario-decomposer` | 플랜의 의도를 GWT 시나리오로 분해. |
| `gwt-converter` | 자유 형식 요구를 엄격한 Given / When / Then 형태로 변환 (D5). |
| `impl-coder` | 시나리오를 구현; 4대 협업 패턴 중 하나 아래에서 동작. |
| `test-runner` | 검증을 실행하고 증거를 방출. |
| `regression-runner` | R2+ 라운드에서 이전에 통과한 시나리오를 재생 (D7). |
| `disruption-analyst` | 시나리오 / 요구사항 / 결정이 바뀔 때 교란을 분류 (D9). |
| `schema-architect` | 스키마 형태의 결정을 소유 (D17 에 따라 L4 강제). |
| `decision-resolver` | Decision 행에 대한 consensus / dissensus 해결을 진행. |
| `pattern-orchestrator` *(v0.5)* | CollaborationPattern 을 선택·활성화; 형상 게이트를 강제 (D26 / D27). |
| `pattern-critic` *(v0.5)* | D26 graph-consensus 시빌 방지에 필요한 두 번째 구별되는 `(AgentSpec.name, AgentSpec.stance)` 튜플을 제공. |
| `reversal-runner` *(v0.5)* | D28 에 따라 롤백을 append-only Decision (`kind=consensus, reversal_of=<id>`) 으로 실행. |

## 훅과 게이트

Claude Code 위에 얹힌 훅 체인:

| 훅 | 동작 |
|---|---|
| `SessionStart` | `ensureInstalled` (멱등) 호출, `sdid` 스폰, 대시보드 컨텍스트 주입. |
| `UserPromptSubmit` | 활성 시나리오 컨텍스트를 해석하고 주입. |
| `PreToolUse` | 순서대로 4개 게이트: 활성 태스크 / **위임 (D21)** / **패턴 형상 (D26 권고)** / **리소스 클레임 (D29)**. `Edit`, `Write`, `MultiEdit`, `Bash`, `NotebookEdit`, `Agent`, `Task`, `TeamCreate`, `SendMessage` 에 매칭. |
| `PostToolUse` | 활성 시나리오 / 태스크에 파일 변경을 기록; `Edit`, `Write`, `MultiEdit`, `NotebookEdit` 에 매칭. |
| `SubagentStart` / `SubagentStop` | 서브에이전트 실행을 활성 시나리오에 바인딩; 종료 시 결과 요약을 영속화. |

D21 위임 게이트: 오케스트레이터(메인 세션) 는 실행 도구(`Edit` / `Write` / `MultiEdit` / `NotebookEdit` / 변경성 `Bash`) 호출이 차단된다. `Agent` 로 스폰된 전문가만이 게이트를 만족시키는 `hookInput.agent_id` 를 갖는다.

D26 패턴 무결성 (권고): `Agent` 또는 `Task` 디스패치가 멀티 에이전트 의도 토큰(`specialist team`, `parallel`, `swarm`, `graph review`, `fan-out`, `agents-as-tools`, `multi-agent`) 이나 `pattern_id` 를 동반하면, 훅이 `/patterns/active` 를 조회한다. 행이 없으면 비차단 경고가 발생하고; 데몬이 자율성을 L3 로 제한하는 `direct` 행을 자동 생성한다.

D29 멀티세션 클레임: `Edit` / `Write` / `NotebookEdit` 에 대해 훅이 `/scenarios/active-claims` 를 조회한다. 세션 간 겹침은 코드 2 와 구조화된 `{ block: 'sdi_claim_overlap', target_path, my_scenario, holders, hint }` 페이로드로 종료된다. 데몬 도달 불가 → 진행 (오프라인 데몬이 결코 에디터를 잠그지 않도록).

비상 우회: `SDI_HOOK_V05_DISABLE=1` 은 사용할 때마다 감사 로그가 남는 단일 호출 탈출구다. 일상적 사용은 프로토콜 위반이다.

활성 시나리오는 데몬이 `AgentRun ↔ Scenario` 엣지를 갖기 전까지 현재 `SDI_ACTIVE_SCENARIO` 환경 변수를 통해 흐른다.

## 설치 게이트

`adapters/shared/sdi-hooks.cjs::ensureInstalled` 가 **단일** 설치 진입점이다. `SessionStart` 가 이를 호출하고; `scripts/setup.cjs` 는 수동 / CI 흐름을 위해 이에 위임한다. 게이트는 멱등이다: 두 바이너리가 모두 해석되고, 스킬 파일이 확인되며, 데몬의 `/health` 가 응답하면 즉시 반환한다. SDI 는 하나의 워크스페이스로 배포되므로 cli/daemon 버전은 `Cargo.toml [workspace.package].version` 이 관장한다 — 컴포넌트별 매니페스트는 없다.

## LM-8 불변식

사용자 데이터는 XDG 경로(`~/.local/share/sdi/`, `~/.cache/sdi/`, `~/.config/sdi/`, `~/.local/state/sdi/`) 아래로 해석된다 — **결코** `~/.claude/plugins/` 아래가 아니다. 데몬이 시작 시 이를 강제하고; `sdi doctor` 가 겹침을 치명적 오류로 보고한다. 플러그인 게이트는 `pluginRoot` 아래에 바이너리 / 번들만 쓰며, 사용자 상태는 절대 쓰지 않는다.

## 관련 표면

- [`web/`](./web/) — 대시보드 SPA (Vite/React 19/Tailwind 4) 가 이 동일한 저장소에 산다; `sdid` 가 그 `dist/` 를 `/` 로 서빙하고 HTTP API + `/events` SSE 로 공급한다.
- [`sdi-desktop`](https://github.com/scenario-driven/sdi-desktop) — 별도 org 저장소. `plugin/web/dist` 를 번들하고 `sdid` 를 사이드카로 스폰하는 Tauri 2 셸.
- [`sdi-docs`](https://github.com/scenario-driven/sdi-docs) — 별도 org 저장소. 저장소의 `docs/PRD.md` 를 미러링하는 Astro/Starlight 랜딩 + 이중언어(ko / en) 가이드 사이트.

전체 정체성 진술과 D1–D29 불변식은 저장소 루트 [`README.md`](../README.md) 와 [`CLAUDE.md`](../CLAUDE.md) 를 참조.
