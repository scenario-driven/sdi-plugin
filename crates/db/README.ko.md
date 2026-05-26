# sdi-db

[English](./README.md) · **한국어**

SDI 저장소 어댑터 — SQLite 스키마, 커넥션 풀, 엔티티별 리포지토리. `@scenario-driven/sdi-plugin` Rust 워크스페이스의 일부다.

## 무엇인가

`sdi-db` 는 온디스크 스키마와 CRUD 를 소유한다. `r2d2` 커넥션 풀 뒤의 `rusqlite` 를 사용하며, 키워드 검색에는 FTS5 를 쓴다; 벡터 검색은 보류 상태다 (PRD §5.2). `rusqlite` / 풀 오류를 `sdi-core` 의 `DomainError` 로 매핑하여, 호출자가 `rusqlite` 타입에 의존하지 않도록 한다.

런타임에 이 크레이트를 링크하는 것은 데몬뿐이다. 하위 크레이트(cli, mcp) 는 데몬의 HTTP/소켓 표면을 통해 상태에 도달하며, 결코 SQLite 에 직접 접근하지 않는다.

## 표면

| 모듈 | 역할 |
|---|---|
| `paths` | XDG 경로 해석 + LM-8 불변식 (`Paths`, `ENV_ALLOW_OVERLAP`, `ENV_HOME_OVERRIDE`). |
| `pool` | `open_pool`, `tx`, `Pool`, `PooledConn`. |
| `schema` | `ensure_schema` — 시작 시 멱등 마이그레이션. |
| `repo/*` | 엔티티별 리포지토리 하나씩 (plan / scenario / decision / round / pattern / autonomy_policy / agent_note / agent_spec / task / event / project / …). |

## LM-8 불변식

사용자 데이터는 XDG 경로(`~/.local/share/sdi/`, `~/.cache/sdi/`, `~/.config/sdi/`, `~/.local/state/sdi/`) 아래로 해석되며 **결코** `~/.claude/plugins/` 아래가 아니다. `Paths` 가 이를 강제하고; 겹침은 `sdi doctor` 가 표면화하는 치명적 오류다.

## 워크스페이스에서의 위치

```
sdi-daemon ──▶ sdi-db (이것) ──▶ SQLite 파일 (XDG data 디렉터리)
                  └── 도메인 타입을 위해 sdi-core 에 의존
```

## 빌드 & 검증

```sh
cargo build -p sdi-db
cargo check -p sdi-db
```

정본 명세: [`../../docs/PRD.md`](../../docs/PRD.md). 저장소 개요: [`../../README.md`](../../README.md).
