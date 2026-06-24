# plugin/web — SDI 대시보드 SPA

[English](./README.md) · **한국어**

`@scenario-driven/sdi-plugin` Claude Code 플러그인에 번들되는 대시보드 SPA. `sdid` HTTP API + `/events` SSE 의 단방향 소비자 — 데몬이 소유한 모든 1등 엔티티와 PRD D14–D29 에 명세된 멀티 에이전트 거버넌스 표면을 노출한다.

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md).

## 무엇을 보여주는가

| 뷰 | 용도 |
|---|---|
| **SummaryView** | 상위 수준 대시보드: 활성 플랜, 자율성 모드, 활성 라운드, 최근 결정. |
| **BoardView** | 시나리오 + 런타임 태스크에 대한 칸반 (todo → in_progress → done / cancelled). |
| **BacklogView** | 플랜별로 묶인 시나리오 백로그, `tags` 와 `depends_on` DAG 힌트 포함. |
| **TimelineView** | Append-only Decision 피드 — `proposal → critique → consensus / dissensus` (M3, D20). `reversal_plan` + `blast_radius_score` 칩 동반 (D28). |
| **WikiView** | 플랜별 / 시나리오별 롱폼 컨텍스트. |
| **PatternsView** *(v0.5)* | kind 배지, 라이프사이클, depth, fan-out, `direct`-패턴 안티마커를 가진 활성 CollaborationPattern 행 (D22 / D23 / D27). |
| **AutonomyPanel** | 스코프별 `AutonomyPolicy` 행 (L3 / L4 / L5 칩, `external_surface` / `forced` 플래그, `l5_threshold`, `pattern_depth_cap`, `plan_single_session_lock`). **서킷 브레이커** 액션이 한 번의 클릭으로 모든 행을 L3 로 강등 (D18). |
| **AgentNotesPanel** | SSE 위의 실시간 M1 블랙보드 / M2 핸드오프 인박스 — observation / hypothesis / question / handoff / dissent / evidence 추가; 감사 추적을 잃지 않고 (소프트) 폐기. |

모든 것은 데몬의 해석된 상태로부터 렌더링된다 — 클라이언트 측 자율성 결정 없음, 백채널 없음. 멀티 에이전트 다양성은 `AgentSpec.stance` (`proposer / devil_advocate / schema_guardian / performance_reviewer / security_reviewer / neutral`) 를 통해 렌더링되어 D26 시빌 방지가 끝에서 끝까지 보인다.

## 스택

Vite 6 · React 19 · Tailwind 4 · TypeScript 5.7 · react-router-dom 7 · @dnd-kit.

## 어떻게 서빙되는가

`sdid` 가 `plugin/web/dist/` 를 데몬의 HTTP 리스너(tower-http `ServeDir`) 로 서빙한다. 플러그인의 `session-start` 훅이 첫 설치 시 빌드된 번들을 확인하고 사용자에게 옵트인을 요청한다; 거절하면 대시보드는 비활성 상태로 유지되며 CLI / MCP 표면에는 아무 영향이 없다. 이 트리가 스냅샷 임포트된 업스트림 커밋은 `SNAPSHOT.json` 을 참조.

## 개발

```sh
pnpm install
pnpm dev          # vite 개발 서버 (vite.config.ts 의 기본 포트)
pnpm build        # → ./dist (sdid 가 소비)
pnpm typecheck
pnpm lint
```

`pnpm dev` 는 `sdid` 가 도달 가능하다고 가정한다 — 두 번째 터미널에서 `sdi daemon start` 를 실행하거나 Claude Code session-start 훅이 스폰하도록 둔다.

## 라이선스

MIT.
