# sdi-daemon

[English](./README.md) · **한국어**

`sdid` 백그라운드 데몬 — SQLite 를 건드리는 유일한 프로세스. `@scenario-driven/sdi-plugin` Rust 워크스페이스의 일부다.

## 무엇인가

`sdi-daemon` 은 하나의 바이너리 `sdid` 와 `sdi_daemon` 라이브러리를 빌드한다. 단일 `tokio` 런타임 위에서 `axum` HTTP API 와 SSE 이벤트 버스를 돌리며, 모든 상태를 [`sdi-db`](../db/) 를 통해 보유한다. CLI, MCP 서버, 대시보드 SPA 는 모두 이 표면의 클라이언트다 — 어느 것도 데이터베이스를 직접 열지 않는다.

## 표면

| 모듈 | 역할 |
|---|---|
| `state` | 공유 `AppState` — db 풀 + 이벤트 브로드캐스터 + 해석된 경로. |
| `router` | axum 라우터 조립, 엔티티별 서브모듈 하나씩 (plan / scenario / decision / round / pattern / autonomy / agent_note / task / project / aggregate / …). |
| `events` | tokio broadcast 채널 + `/events` SSE 핸들러. |
| `error` | `DomainError` → JSON HTTP 응답 매핑. |
| `lifecycle` | pid / port / socket 파일 관리. |

## 워크스페이스에서의 위치

```
sdi-cli / sdi-mcp / 대시보드 SPA ──HTTP + SSE──▶ sdi-daemon (이것) ──▶ sdi-db ──▶ SQLite
```

데몬이 유일한 writer 이므로, 자율성 게이트·합의 규칙·패턴 형상 검증·멀티세션 클레임 라우팅이 런타임에 강제되는 곳이 바로 여기다. 또한 `plugin/web/dist/` 를 tower-http `ServeDir` 로 서빙한다.

## 빌드 & 검증

```sh
cargo build -p sdi-daemon   # target/debug/sdid 생성
cargo check -p sdi-daemon
```

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md). 저장소 개요: [`../../README.md`](../../README.md).
