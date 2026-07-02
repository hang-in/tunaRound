# A2A 자율 워커 데몬 (worker auto-poll) 설계 - 세션8

> 2026-07-03 세션8. 크로스머신 SSE 스트리밍 스모크 뒤, 남은 "사람 트리거 릴레이"(worker-discovery 폴링)를 제거하는 마지막 조각. 워커가 코어를 auto-poll -> claim -> 러너로 실행 -> complete를 사람 개입 없이 돈다. **(a) 워커 auto-poll = (b) 이기종 파트너**: 데몬이 어느 Runner/model로 task를 실행하냐가 곧 파트너 종류(Codex-on-Ollama 등). 정본 [파트너 위임](v2-a2a-partner-delegation_2026-07-02.md) + [스트리밍](v2-a2a-streaming_2026-07-03.md).

## 0. 최종 목표

**`tunaround work`(신규 서브커맨드) = 헤드리스 자율 워커 데몬.** 지정한 agent id 앞으로 온 A2A task를 코어에서 스스로 발견(poll)->착수(claim)->지정 Runner(claude/codex/opencode/http)로 실행->완료(complete)한다. 사람은 dispatcher 쪽에서 목표를 1회 발행할 뿐, 워커 쪽 "이제 처리해" 릴레이가 사라진다.

이로써 마찰 #3의 두 절반이 다 닫힌다: **dispatcher-notify(SSE 스트리밍으로 해소, 완료) + worker-discovery(이 데몬으로 해소).** 그리고 데몬의 `--runner`/`--model`이 **이기종 파트너 확장(b)의 실제 배선**이다(신규 어댑터 없이 기존 Runner 재사용).

### 비목표

- **debate AutoLoop(Stage 4)가 아니다.** 이건 토론 제어 평면 자율화가 아니라 "위임 task 워커"의 자율 실행이다. dispatcher(사람)가 목표를 발행하는 구조는 유지.
- 다중 워커 스케줄링·우선순위·재시도 정책은 최소만(후속).

## 1. 재사용할 기존 조각 (정찰 완료)

- `Runner` trait(`src/runner/mod.rs`): `fn run(&self, &RunInput) -> Result<RunOutput, RunError>`. `RunInput{prompt, model, project_path, mode, pull}`, `RunOutput{content, in/out tokens}`, `RunMode{ReadOnly(기본), Write}`. 러너 = claude/codex/opencode/http(engines).
- 워커 inbox 툴 = MCP `poll_tasks`/`claim_task`/`complete_task`(`src/mcp.rs`). 원격 워커는 코어 `/mcp`를 HTTP로 호출해야 함.
- MCP HTTP 클라이언트 패턴이 **`mcp.rs` 테스트 코드에만** 존재(initialize->mcp-session-id 캡처->initialized->tools/call, SSE 응답 파싱). 프로덕션용으로 추출 필요.
- 서브커맨드 = clap `Commands`(Chat/Core/Serve/Join/McpSearch/Reindex). `Work` 추가.

## 2. 설계

### 2.1 `work` 서브커맨드 (WorkArgs)
- `--core <url>`(코어 `/mcp` 베이스, 예 http://192.0.2.10:8770), `--token <T>`(bearer, env로도 - `--search-token-env` 관례 답습).
- `--agent <id>`(이 워커의 to_agent, 예 win-worker), `--runner <claude|codex|opencode|http>`(기본 claude), `--model <m>`(옵션), `--project-path <p>`(옵션).
- `--interval <secs>`(기본 15), `--once`(1패스 후 종료, 테스트·수동용), `--write`(기본 ReadOnly=behavioral read-only 유지).

### 2.2 루프
```
loop:
  tasks = mcp_client.poll_tasks(agent)          # 열린(submitted/working/input_required) 목록
  for t in tasks where state==submitted:
    mcp_client.claim_task(t.id)                 # -> working (SSE 구독자에게 working emit)
    input = RunInput{ prompt: t.message_text, model, project_path, mode }
    out = runner.run(&input)                    # 헤드리스 에이전트 1턴 실행
    mcp_client.complete_task(t.id, out.content) # -> completed+artifact (SSE emit)
  if once: break
  sleep(interval)
```
- claim/complete가 코어 store 버스를 통해 **dispatcher의 SSE로 실시간 흐른다**(스트리밍 Phase 2와 자동 결합). 즉 dispatcher는 SendStreamingMessage로 던지고, 워커 데몬이 자율 처리하며, 그 진행이 SSE로 dispatcher에 실시간 도착 = 사람 릴레이 0.
- 러너 실패(RunError)면 그 task를 complete에 에러 요약을 넣거나(향후 fail 상태 전이) 로그 후 스킵. 최소판 = 에러를 result에 담아 complete(가시성 우선). fail 상태 전이는 후속.

### 2.3 MCP 클라이언트(W1)
`src/mcp.rs` 테스트의 핸드셰이크/tools-call 로직을 프로덕션 모듈(예 `src/mcp_client.rs`, 역할 주석)로 추출: `McpHttpClient::connect(url, token)`(initialize->session-id->initialized) + `call_tool(name, args) -> Result<String, _>`(SSE `data:` 파싱). poll/claim/complete는 이 위의 얇은 래퍼. reqwest(기존 의존, blocking or async - 러너가 sync라 blocking 편이 단순).

## 3. 태스크 분해
- **W1**: 프로덕션 MCP HTTP 클라이언트(핸드셰이크 + call_tool + SSE 파싱) 추출·일반화. 단위테스트(기존 serve 테스트 하네스로 왕복).
- **W2**: poll/claim/complete 래퍼 + `work` 루프(poll->claim->runner.run->complete, --once). 루프 로직은 Runner·client를 trait 주입해 fake로 단위테스트.
- **W3**: `Work` 서브커맨드(WorkArgs) + main.rs 배선 + 러너 선택(claude/codex/opencode/http factory).
- **W4**: 로컬 라이브 데모(코어 + `tunaround work --agent win-worker --once` + dispatcher가 SendStreamingMessage로 던지면 데몬이 자율 claim/run/complete, SSE로 완료 관찰 = 사람 트리거 0). + (b) 이기종: `--runner codex --model <ollama>`로 Codex-on-Ollama 워커 스모크.

## 4. 스코프·안전 경계
- 데몬은 **opt-in**(사용자가 `tunaround work` 실행). read-only 기본(behavioral, [[readonly-soft-enforcement-ok]]). `--write`는 명시적.
- dispatcher-side 사람이 목표 발행하는 구조 유지(semi-a2a HITL). 워커 자율은 "발견+실행"에 한정.
- 러너는 그 머신의 claude/codex 로그인·설치에 의존(기존 전제).

## 5. 열린 결정
1. MCP client 동기(blocking reqwest) vs 비동기: 러너가 sync라 **blocking 추천**(work 커맨드는 자체 루프, tokio 불필요할 수도). 단 기존 serve/reqwest는 async - 재사용 위해 async+block_on일 수도. W1에서 확정.
2. 러너 실행 실패 시 task 상태: 최소=complete에 에러 담기 vs 신규 `fail` 전이(update_task_state Failed). 최소로 시작, fail 전이는 후속.
3. task 본문->prompt 매핑: message.parts의 text 이어붙임(현 dogfood 관례). 리치 Part(data/url)는 후속.
