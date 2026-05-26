# sdi-cli

[English](./README.md) · **한국어**

`sdi` 명령줄 바이너리 — 사용자와 LLM 에이전트가 다루는 진입점. `@scenario-driven/sdi-plugin` Rust 워크스페이스의 일부다.

## 무엇인가

`sdi-cli` 는 하나의 바이너리 `sdi` 와, 통합 테스트가 서브프로세스를 스폰하지 않고 clap 앱·데몬 라이프사이클 헬퍼·doctor 검사·HTTP 클라이언트를 직접 구동할 수 있도록 하는 얇은 `sdi_cli` 라이브러리를 빌드한다.

CLI 는 SQLite 를 직접 건드리지 않는다. `sdid` 데몬의 HTTP 표면(`reqwest`) 으로 대화하고 결과를 렌더링한다. 모든 상태는 데몬이 소유한다.

## 서브명령

| 그룹 | 명령어 |
|---|---|
| 1등 엔티티 | `plan`, `req`, `scenario`, `round`, `decide`, `consensus`, `autonomy`, `agent-note`, `pattern` |
| 런타임 | `task`, `run`, `project` |
| 집계 & 운영 | `aggregate` (dashboard / summary / board / wiki / timeline), `usage`, `knowledge`, `comment`, `question`, `impexp`, `ops`, `doctor` |
| MCP | `sdi mcp` — stdio MCP 서버를 호스팅 ([`sdi-mcp`](../mcp/) 에 위임). 플러그인의 `.mcp.json` 이 바로 이것을 호출한다. |

## 워크스페이스에서의 위치

```
sdi-cli (이것) ──HTTP──▶ sdi-daemon (sdid) ──▶ sdi-db (SQLite)
    └── sdi-mcp 임베드 (stdio MCP 서버, `sdi mcp`)
```

`sdi-core` (도메인 타입), `sdi-db` (공유 타입용), `sdi-mcp` (MCP 서브명령) 에 의존한다.

## 빌드 & 검증

```sh
cargo build -p sdi-cli      # target/debug/sdi 생성
cargo check -p sdi-cli
```

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md). 저장소 개요: [`../../README.md`](../../README.md).
