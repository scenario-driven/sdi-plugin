# sdi-mcp

[English](./README.md) · **한국어**

SDI MCP 서버 — LLM 클라이언트를 위한 stdio JSON-RPC 2.0 표면. `@scenario-driven/sdi-plugin` Rust 워크스페이스의 일부다.

## 무엇인가

`sdi-mcp` 는 바이너리가 아니라 라이브러리다. 단일 진입점 `run_stdio` 가 `sdi mcp` 서브명령에 의해 호출된다 (`main` 이 아니라 함수로 유지하여, 통합 테스트가 서브프로세스 없이 구동할 수 있다). 전송은 stdin/stdout 위의 개행 구분 JSON 이다 (MCP stdio 변형 — `Content-Length` 프레이밍 없음). 플러그인의 `.mcp.json` 이 `sdi mcp` 를 MCP 서버로 연결한다.

## 도구 표면 (PRD §5.4)

| 종류 | 도구 |
|---|---|
| **read** | `search_knowledge`, `search_scenarios`, `get_plan_context`, `get_recent_decisions` — RAG 전용. 결과는 반드시 `scope=rag` 여야 하며; LLM 은 `reference` / `archive` 아티팩트를 결코 보지 않는다 (Clawket 에서 계승한 LM 불변식). |
| **write** | `add_scenario`, `add_requirement`, `add_decision`, `update_task_evidence`, `start_round` — 데몬의 HTTP 라우트로 곧장 매핑되는 중재된 변경. |

## 워크스페이스에서의 위치

```
LLM 클라이언트 ──stdio JSON-RPC──▶ sdi mcp (sdi-mcp, 이것) ──HTTP──▶ sdi-daemon ──▶ sdi-db
```

쓰기는 결코 데몬을 우회하지 않는다 — `sdi-mcp` 는 CLI 가 쓰는 것과 동일한 HTTP 라우트를 호출하므로(`reqwest`), 데몬의 게이트가 일관되게 적용된다.

## 빌드 & 검증

```sh
cargo build -p sdi-mcp
cargo check -p sdi-mcp
```

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md). 저장소 개요: [`../../README.md`](../../README.md).
