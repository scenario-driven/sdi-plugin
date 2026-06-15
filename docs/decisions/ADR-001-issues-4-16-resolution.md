# ADR-001 — GitHub 이슈 #4~#16 해결 방향 (13건)

Status: accepted · Date: 2026-06-15

근거는 프로젝트 결정(CLAUDE.md D-table) · 코드 file:line · 공식 docs · 1차 학술 출처로 검증. 사용자와 1건씩 합의.

## 사용자 결정 (AskUserQuestion)

| # | 이슈 | 결정 | 핵심 근거 |
|---|---|---|---|
| D1 | #11 미등록 agent 권한 | **작업 가능 + 자율 L3 상한**. 게이트 3-root(`.claude/agents` + `~/.claude/agents` + `plugin/agents`) 읽기, 첫 작업 시 `agent_specs`에 정의 스냅샷(이름+메타+prompt sha256)+origin. 중앙 위치=`~/.claude/agents` | D26 sybil 방지(distinct (name,stance)≥2)는 등록을 *구조적으로* 강제 → 작업을 막지 않아도 고자율(L4/L5)만 등록 요구. Claude Code는 파일에서만 spawn(공식 docs) → DB=거버넌스/provenance, 파일=spawn. `~/.claude/agents`는 LM-8 안전(plugins 바깥)+전역 spawn |
| D2 | #4 메인 sdi 면제 | **서브커맨드 분할** — plan/scenario/round/decide=메인 허용, task=에이전트 위임 | D3 "Task is a runtime artifact; humans do not author tasks directly". plan/scenario는 D5/D8상 spec 저작=조정 영역. scenario-decomposer specialist가 task 분해 담당 |
| D3 | #4c gh 경계 | **read-only 서브커맨드만 화이트리스트** (auth status, repo/issue/pr/run/release/search의 list\|view\|status\|diff\|checks, `gh api`는 GET만) | least privilege + fail-safe defaults (Saltzer & Schroeder 1975) — default-deny |
| D4 | #8 retire | **가역 상태 토글** (draft·confirmed 모두, un-retire 복원, 과거 verdict 보존, 미래 라운드/회귀/needs-verification 제외) | SCN id 보존(이슈 핵심) + D12 append-only |
| D5 | #15 next/brief | **풀 채택** — `sdi next` + `sdi task brief` + 라운드 검증 베이스라인 저장 | "브리핑 작성(강한 모델)"과 "브리핑 따르기(약한 모델)" 분리 = 자율 런 티어 레버. 데몬이 상태의 결정적 중재자 |
| D6 | #16 provisional | **상태 신설 안 함 — `accepted` + `supersede_when` 컬럼**. 잠정 집합 = `supersede_when IS NOT NULL`인 accepted | single source of truth(가변성은 supersession 체인이 이미 표현) + D28 가역성 1급 + economy of mechanism. ADR 정전(Nygard 2011)에도 provisional 상태 없음 — supersession이 그 역할 |

## 명확 항목 (소거 후 1개 수렴)

- **#9** active-task: `SDI_ACTIVE_TASK` env(세션 내 충족 불가) → 데몬 **lease/run 상태**로 판정. 에러 메시지 실존 명령(`sdi task start`)으로.
- **#10**: `2>/dev/null` 등 /dev/null 리다이렉트 허용 (체인·다중인자는 0.4.2 `f0e0adb2`에서 이미 해소).
- **#5**: round mode CLI/SKILL을 데몬 enum(`strict-regression`|`forward-only`)에 정렬 + `additive`를 `forward-only` alias로 수용.
- **#6** tier: SKILL의 task create 문법을 실제(positional)로 정정 + 시나리오 `tags` 활용 (tasks.tier 신설 안 함 — D3·Clawket 차별점). short_code: cancelled 점유 유지 + 409 메시지에 명시.
- **#7**: round activate 응답에 `scenarios_needing_verification` 추가 (SKILL.md:164 계약).
- **#12**: task complete evidence의 `scenario_id`가 `parent_scenario_ids`에 속하는지 대조, 불일치(유령 ID) 시 거부.
- **#13**: complete 트랜잭션 원자화(`pool.rs:38 tx`) + short_code→SCN ULID resolve(플랜 스코프).
- **#14**: bypass 마커를 (세션,에이전트)/lease 토큰으로 스코프, 전역 단일 파일(`~/.cache/sdi/bypass-once`) 폐기.

## 순서 (사용자 결정)

게이트 슬라이스(#4·#9·#10·#11·#14) + 데이터 무결성(#12·#13) **먼저** → Agent Registry 풀 에픽은 다음.

## 별도 에픽 (다음): Agent Registry & Management

DB-authoritative(`agent_specs` SSoT, `.md`는 `~/.claude/agents`로 생성된 뷰 — 프로젝트 "markdown is generated view" 원칙) + CLI(`sdi agent register/list/disable/delete`) + 스킬(대화형: file-save vs name-only) + web 관리 페이지. 발견된 구조적 한계: 레지스트리가 `.claude/agents` 미참조, `_agentRegistryCache` 영구, `agent_specs` INSERT 호출처 0(스냅샷 미구현).

## 1차 출처

- Saltzer & Schroeder (1975), *The Protection of Information in Computer Systems* — https://web.mit.edu/Saltzer/www/publications/protection/Basic.html
- Nygard (2011), *Documenting Architecture Decisions* — https://cognitect.com/blog/2011/11/15/documenting-architecture-decisions
- Claude Code subagents (공식) — https://code.claude.com/docs/en/sub-agents
