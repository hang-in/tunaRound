---
title: "tunaRound v2 Plan 14: 에이전트 능동 검색 도구 (MCP search_context, secall rmcp 답습)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 에이전트(claude/codex)가 토론 중 스스로 검색을 호출하게 한다(설계의 "능동 검색"). secall이 rmcp MCP 서버라 답습 포팅. tunaRound가 stdio MCP 서버(`--mcp-search --db`)를 띄워 search_context(query) 툴 하나를 노출 - 이미 만든 SqliteRetriever(FTS+벡터 RRF 하이브리드)를 그대로 감싼다. 러너가 claude/codex에 mcp-config를 넘겨 에이전트가 자율 호출. mcp feature(rmcp/schemars). Task 1=서버+stdio 모드(rmcp Windows 빌드 게이트), Task 2=러너 mcp-config 배선, Task 3=라이브 스모크. 라이브(에이전트의 실제 툴 사용)는 실 claude/codex 필요(토큰 소모).
---

# tunaRound v2 Plan 14: 에이전트 능동 검색 도구(MCP) Implementation Plan

> **For agentic workers:** TDD. **cargo는 Bash 툴로.**
> 출처: `D:/privateProject/seCall/crates/secall-core/src/mcp/{server,tools}.rs`(정본, rmcp 1.3.0). 결정: 설계 v2-context-memory-direction(에이전트 검색 도구 = A2A MCP 통로). **이미 만든 SqliteRetriever 재사용.** 아키텍처 재론 금지.

**Goal:** 사람이 /search를 치는 대신(Plan 12), **에이전트가 토론 턴 중 스스로** 관련 맥락을 검색하게 한다. tunaRound가 MCP 검색 서버를 제공하고, 러너가 claude/codex에 그 서버를 물려 에이전트가 `search_context(query)`를 자율 호출한다.

**Architecture:** secall rmcp 답습. tunaRound가 **stdio MCP 서버 모드**(`tunaround --mcp-search --db <path>`)를 갖는다. 서버는 단일 툴 `search_context(query, limit)`를 노출하고, 내부는 **이미 만든 `SqliteRetriever`**(Plan 11/13, FTS+벡터 RRF 하이브리드)를 그대로 호출 -> Vec<Utterance> -> MCP Content. 러너(Plan 02/03)가 claude `--mcp-config`(codex 대응)로 이 서버를 spawn하도록 배선. 에이전트가 도구를 쓸지는 비결정적(라이브 검증 영역).

**Tech Stack:** Rust 2024. 신규(optional): `rmcp = "1.3.0"`(server/macros/schemars/transport-io) + `schemars`. `mcp = ["sqlite","dep:rmcp","dep:schemars"]`(하이브리드 원하면 semantic도). rmcp는 async(tokio, 기존 보유). 선행: Plan 9~13 done.

> 규율 #5/#6, TDD, 위임 Sonnet + Opus 리뷰. **rmcp Windows 빌드가 Task 1 게이트**(rusqlite처럼 실패 시 멈추고 보고 -> 대안=경량 hand-roll JSON-RPC 검토).

---

## 범위

- **포함:** `mcp` feature + rmcp/schemars + `src/mcp.rs`(TunaSearchServer, search_context 툴, SqliteRetriever 래핑) + `main --mcp-search --db` stdio serve 모드 + 러너 mcp-config 배선(Task 2) + 라이브 스모크(Task 3).
- **비포함:** 다중 툴(recall/get/expand) - search_context 하나로 시작. REST 전송 · 그래프/wiki 툴 · A2A 자율 핸드오프.
- **불변식:** mcp feature off = 서버/배선 미컴파일, 기존 동작·테스트 불변. `--mcp-search` 안 주면 기존 REPL 그대로.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) optional rmcp/schemars + `[features] mcp`. |
| `src/mcp.rs` | (신규) `#[cfg(feature="mcp")]` TunaSearchServer(rmcp ServerHandler) + search_context 툴 + Params. 첫 줄 역할 주석. |
| `src/lib.rs` | (수정) `#[cfg(feature="mcp")] pub mod mcp;`. |
| `src/main.rs` | (수정) `--mcp-search` 인자 -> stdio serve 모드(REPL 대신). |
| `src/runner/{claude,codex}.rs` | (Task 2) mcp-config 인자 추가(검색 서버 spawn 설정). |

---

### Task 1: rmcp MCP 검색 서버 + stdio 모드 (빌드 게이트)

**Files:** Modify `Cargo.toml`, `src/lib.rs`, `src/main.rs`; Create `src/mcp.rs`

- [ ] **Step 1: deps + feature** — `rmcp = { version = "1.3.0", features = ["server","macros","schemars","transport-io"], optional = true }` + `schemars = { version = "0.8", optional = true }`(secall 버전 맞춤 확인) + `mcp = ["sqlite","dep:rmcp","dep:schemars"]`. **`cargo build --features mcp`로 rmcp Windows 컴파일 확인 - 실패 시 즉시 멈추고 에러 보고**(대안 검토 필요).
- [ ] **Step 2: `src/mcp.rs`(secall server.rs 답습, 단순화)** — 첫 줄 역할 주석.
  - `SearchParams { query: String, limit: Option<usize> }` (`#[derive(Deserialize, JsonSchema)]`).
  - `TunaSearchServer { tool_router, retriever: Arc<dyn ContextRetriever + Send + Sync> }`(또는 SqliteRetriever 보유). `#[tool_router]`/`#[tool] async fn search_context(&self, Parameters(p): Parameters<SearchParams>) -> Result<CallToolResult, McpError>`: `let hits = self.retriever.retrieve(&p.query, p.limit.unwrap_or(10).min(50));` -> hits를 텍스트/JSON Content로(speaker+content). `#[tool_handler] impl ServerHandler`(get_info: instructions = "토론 맥락 검색. search_context(query)로 과거/다른 분기 관련 발언을 찾는다.").
  - **주의:** ContextRetriever는 현재 `Send`만 보장? trait에 `Send+Sync` 필요할 수 있음(rmcp 서버는 Clone+Send+Sync). SqliteRetriever는 Mutex라 Sync OK. trait bound 조정 필요 시 `ContextRetriever: Send + Sync` 추가(기존 영향 확인).
- [ ] **Step 3: main `--mcp-search` 모드** — 인자 파싱에 `--mcp-search`(bool) 추가. 켜지면 REPL 대신: `--db`로 SqliteStore(읽기)+tok+embedder -> SqliteRetriever -> TunaSearchServer -> rmcp stdio serve(`serve((stdin(), stdout())).await`), 기존 tokio rt로 block_on. (observe 모드처럼 early-return.)
- [ ] **Step 4: 테스트** — rmcp 인-프로세스 클라이언트가 무거우면, 최소 `TunaSearchServer`의 검색 위임을 직접 검증(FakeRetriever 주입 -> search_context 호출 -> 결과 Content에 내용 포함). 빌드 게이트(`--features mcp`) 통과가 핵심.
- [ ] **Step 5: 검증 + 커밋** — `cargo build --features mcp` OK + `cargo test --features mcp` PASS + 기본 `cargo test` 불변 + clippy(mcp) 0. 스모크: `tunaround --mcp-search --db x.db`가 stdio 대기(즉시 EOF로 종료 확인). 커밋 `feat(mcp): rmcp search_context 서버 + --mcp-search stdio 모드`.

---

### Task 2: 러너 mcp-config 배선

**Files:** Modify `src/runner/{claude,codex}.rs`, `src/main.rs`(러너에 db 경로 전달)

- [ ] claude: `build_claude_args`에 검색 서버 활성 시 `--mcp-config <json>`(또는 임시 파일) 추가 - 서버 정의 = `{"mcpServers":{"tuna-search":{"command":"<self-exe>","args":["--mcp-search","--db","<path>"]}}}`. self-exe = `std::env::current_exe()`. (claude CLI mcp-config 포맷 실측 확인.)
- [ ] codex: codex의 MCP 설정 메커니즘(`-c mcp_servers...` 또는 config) 실측 확인 후 대응. 불확실하면 claude만 먼저, codex는 후속 표시.
- [ ] 러너에 옵션(`with_search_db(path)` 등) - 켜졌을 때만 mcp 인자. 기본 동작 불변(인자 미추가).
- [ ] 테스트: args 빌더가 mcp-config 포함/미포함을 옵션대로(순수 테스트). 커밋 `feat(runner): 검색 MCP 서버를 claude/codex에 배선`.

---

### Task 3: 라이브 스모크 + 문서

- [ ] 실 claude/codex로 `--db` REPL 한 라운드, 에이전트가 search_context를 호출하는지 관찰(stream-json/로그). 호출되면 능동 검색 실증. (토큰 소모, 사용자 환경.)
- [ ] 안 불리면 프롬프트/도구 설명 조정 또는 instructions 보강. 결과를 plan에 기록.
- [ ] /help·README에 능동 검색 옵션 문서화.

---

## Self-Review (작성자 체크)
- **답습:** secall rmcp 서버 정본 포팅(재발명 안 함). 단일 툴로 최소 시작.
- **재사용:** search_context = 기존 SqliteRetriever 하이브리드 호출 = 새 검색 로직 0.
- **불변:** mcp feature off / --mcp-search 미지정 = 기존 동작·테스트 불변.
- **리스크 격리:** rmcp 빌드를 Task 1 게이트로. 실패 시 경량 대안 검토.
- **범위:** search_context 하나. 다중 툴·A2A 자율은 후속.

## 위험 / 한계 (후속)
- **rmcp 빌드/크기:** 무거운 async 의존(tokio 기반, 기존 보유). Windows 빌드 미검증 -> Task 1 Step 1 게이트.
- **비결정 사용:** 에이전트가 도구를 실제로 부를지는 모델 판단(라이브에서만 검증). instructions/프롬프트로 유도.
- **codex MCP:** claude와 설정 포맷 다름 - 실측 필요. 불확실 시 claude 우선.
- **Send+Sync:** ContextRetriever bound 조정이 기존 코드에 영향 가능 -> 확인.
- **stdio 충돌:** MCP 서버는 stdio 전용 프로세스(REPL 아님). 러너가 별 프로세스로 spawn하므로 REPL stdio와 무관.
