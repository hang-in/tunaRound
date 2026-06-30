# Plan v2-26: Stage 3a-3 front=core 단일 프로세스

> (A) 코어-백엔드 Stage 3a 마무리. docs/design/v2-A2A-core-backend_2026-06-30.md, Plan 25(v2-25-stage3-resident-core.md) "3a 누가 코어인가: 프론트=코어(최소)".
> 3a-2(502e458)는 코어(`--serve-mcp`) + REPL(`--search-url`) **2프로세스**로 remote core e2e를 했다. 3a-3 = **1프로세스**(REPL이 자기 안에서 HTTP MCP 코어를 띄움).

## 목표

한 프로세스 = **프론트(REPL, 로컬 좌석 구동, 사람 운전) + 상주 HTTP MCP 코어**. 로컬 좌석은 in-process 코어에 HTTP로 접속(pull), 동시에 원격 프론트/에이전트도 같은 주소에 접속 가능. 2프로세스 e2e를 단일 진입점으로 합쳐 half-a2a 척추를 운영 가능한 형태로 닫는다.

## 무엇이 이미 되나(배선 완료)

- 러너 `with_search_url(url, token)` → claude HTTP config / codex `-c mcp_servers.url`(3a-2).
- `build_registry(roster, db, url, token)` 4-arg, main이 search_url/search_token 전달.
- HTTP MCP 서버 `start_http_mcp_server` / `serve_http_mcp_on_listener`(3a-1, bearer, serve feature).
- 러너 `run`은 **동기**(std::process subprocess), REPL 루프도 동기. → HTTP 서버를 rt 워커 스레드에 백그라운드 spawn하고 메인 스레드는 블로킹 REPL을 돌리면 충돌 없음.

## 설계 (린치핀 = main.rs 분기 추가, 순수 추가)

- **신규 플래그 `--core <addr>`**(serve feature). `--serve-mcp`(헤드리스 순수 서버)는 불변 유지, `--core`는 REPL+서빙.
- `--core <addr>` 동작:
  1. `--db <core.db>` 필수(없으면 에러+exit). `--token`은 선택(원격 접속 시 권장).
  2. serve-mcp 분기와 동일하게 retriever + transcript reader를 --db로 빌드.
  3. `rt.block_on(TcpListener::bind(addr))`로 **바인드를 동기 선행**(포트 경합 fail-fast) → `rt.spawn(serve_http_mcp_on_listener(...))` 백그라운드 서빙.
  4. 로컬 좌석 URL 유도: addr의 `0.0.0.0`/`[::]`를 loopback으로 치환 + `/mcp` 부착(`core_local_url`). search_url/search_token을 자동 설정(명시 `--search-url`이 이미 있으면 그쪽 우선, 경고).
  5. 그대로 일반 REPL 셋업 진입 → 로컬 좌석이 in-process 코어에 HTTP pull.
- **동시성**: rt = multi-thread(Runtime::new). 서버 future는 워커 스레드, 메인은 블로킹 REPL. REPL 자체 indexer가 core.db에 쓰고 HTTP reader가 core.db를 읽음 = WAL 동시 reader+writer(3a-2 2프로세스와 동일, 단 동일 프로세스 2커넥션).

## 분리(테스트 가능) 순수 함수

- `mcp::core_local_url(addr: &str) -> String`: `0.0.0.0:8771`→`http://127.0.0.1:8771/mcp`, `[::]:8771`→`http://127.0.0.1:8771/mcp`, `127.0.0.1:8771`→`http://127.0.0.1:8771/mcp`. 단위 테스트.

## 엣지

- `--core` without serve feature: 플래그는 항상 파싱(2인자 소비), serve 없으면 경고+exit.
- `--core` without `--db`: 에러+exit.
- bind 실패: 동기 선행 바인드라 즉시 에러+exit(REPL 진입 전).
- `--core` + 명시 `--search-url` 동시: 명시 URL 우선(경고). 보통은 `--core`만.

## 테스트

- 단위: `core_local_url` 유도(0.0.0.0/::/loopback/호스트). serve 기존 HTTP 테스트 유지.
- 라이브 e2e(Bash, 수동): 단일 `--core 127.0.0.1:8771 --db core.db --token T --pull-context`로 2~3턴, claude가 in-process 코어에서 read_transcript pull 확인. (선택) 별도 REPL이 같은 addr에 `--search-url`로 접속해 전사 읽기 = remote front 공존 확인.

## 범위

- main.rs: 플래그 파싱 + `--core` 분기 + 로컬 URL 자동 배선.
- mcp.rs: `core_local_url` 순수 함수 + 단위 테스트.
- 러너/오케스트레이터/로스터 무변경(배선 기존). 기본 동작 불변(`--core` 미지정 시).

## 3a 잔여(이후)

3d post_turn/get_roster(원격 쓰기 권위) · codex bearer-env(ExecSpec env) · 3c Tailscale(ops) · 3e 영속 에이전트(보류).
