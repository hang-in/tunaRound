# 설계: codex 라이브 감독 (app-server ws + turn/start 외부 주입)

> 정본. 2026-07-05 세션12 후속. codex 감독을 헤드리스 exec가 아니라 사람이 붙어 보는 **라이브 thread**로 두고, 외부(브로커 task 도착)에서 `turn/start`로 깨우는 구조. 구현은 Sonnet 위임, Opus 스펙·리뷰·검증. 계획 = [v2-37](../plans/v2-37-codex-live-supervisor.md).

## 1. 목적과 배경

- 감독<->감독 티키타카 스코프에서 각 감독은 **지속 맥락을 든 라이브 TUI**여야 한다. claude 감독은 세션 하네스 Monitor로 외부 wake가 되지만, codex는 그 메커니즘이 없다.
- 세션12에서 codex 감독을 `poll --on-task 'codex exec resume --last ...'`로 우회했으나, 그건 **별개 프로세스(워커 패턴)**라 라이브 TUI가 아니었고 스코프 위반이었다. 게다가 토큰 미전파로 codex가 raw HTTP로 자가구조하며 186k 토큰을 태웠다.
- 조사 결과 codex는 라이브 세션 외부 구동을 **네이티브로 지원**한다(아래 §2 실증). 따라서 codex 감독을 라이브 thread로 두고 외부에서 유저 턴을 주입하는 게 옳은 길이다.
- **과거 Stage 3e(codex app-server) 킬과의 구분**: 3e는 "크로스머신 헤드리스 위임"이 목적이었고 값이 해체돼 킬됐다(context-notes 2026-07-02). 본 설계는 목적이 다르다. **로컬 라이브 감독을 외부 이벤트로 wake(HITL 가시)** 이며, 크로스머신은 기존 브로커가 담당(§5.5). 3e 킬과 모순 아님.

## 2. 확정 사실 (2026-07-05 실측, codex-cli 0.142.5)

- `codex app-server --listen ws://127.0.0.1:PORT`가 **Windows에서 정상 기동**(`listening on ws://...`, readyz/healthz, "binds localhost only, use SSH port-forwarding for remote"). 관리형 `remote-control`/`app-server daemon`은 Unix 전용이나, **raw `app-server --listen ws://`는 크로스플랫폼**.
- `codex --remote ws://IP:PORT`로 사람이 그 라이브 thread에 TUI 접속(HITL 가시).
- app-server 프로토콜(JSON-RPC) 클라이언트 요청에 **`turn/start`** 존재(= 라이브 thread에 새 유저 턴 주입 = 외부 wake). 관련: `thread/start`, `thread/resume`, `thread/read`, `thread/list`, `thread/loaded/list`, `turn/steer`, `turn/interrupt`, `thread/inject_items`.
- 파라미터(스키마 `codex app-server generate-json-schema --out <dir>` 확인):
  - `initialize`: `{ capabilities?, clientInfo*: { name, version } }`.
  - `thread/start`: `{ approvalPolicy?, sandbox?, cwd?, model?, config?, baseInstructions?, developerInstructions?, ... }` -> 응답/`thread/started` 알림으로 `threadId` 획득.
  - `thread/resume`: `{ threadId*, approvalPolicy?, sandbox?, ... }`.
  - `turn/start`: `{ threadId*, input*: UserInput[], approvalPolicy?, sandboxPolicy?, model?, effort?, clientUserMessageId?, outputSchema? }`.
  - `UserInput`(텍스트): `{ type: "text", text: "<메시지>" }`.
- 진행/완료 신호(ServerNotification): `turn/started`, `item/agentMessage/delta`, `item/mcpToolCall/progress`, `item/completed`, **`turn/completed`**, `thread/started` 등.
- 승인(ServerRequest, 서버가 클라에 물음): `item/commandExecution/requestApproval`, `item/fileChange/requestApproval`, `mcpServer/elicitation/request`, `item/permissions/requestApproval`, `execCommandApproval`, `applyPatchApproval`, `item/tool/requestUserInput`.
- app-server는 `~/.codex/config.toml`을 읽으므로 `[mcp_servers.tuna-broker]`가 그대로 로드된다. 단 **`TUNA_BROKER_TOKEN`이 app-server 프로세스 env에 있어야** 브로커 MCP가 401 없이 로드된다(세션12 186k 낭비의 근본 원인 = 토큰 미전파).

## 3. 아키텍처

codex 감독 머신(윈 또는 맥) 한 대에서 세 구성요소가 상주한다.

```
[브로커(원격/로컬)] --broker task--> [poll watcher] --turn/start(ws)--> [codex app-server(라이브 thread)]
                                                                              ^
                                                                     codex --remote (사람 HITL 관전)
```

1. **codex app-server (라이브 thread 호스트)**: `TUNA_BROKER_TOKEN=<tok> codex app-server --listen ws://127.0.0.1:PORT`. 상주. tuna-broker MCP 로드됨.
2. **사람 TUI(선택, HITL)**: `codex --remote ws://127.0.0.1:PORT`로 붙어 대화를 관전/개입. 안 붙어 있어도 감독은 동작(무인 티키타카는 §5.2 승인정책에 의존).
3. **poll watcher + inject 글루**: 기존 `tunaround poll --agent <id> --on-task '<inject cmd>'`. task 도착 시 `<inject cmd>`가 ws로 app-server에 `turn/start`를 쏜다. 명령만 `codex exec resume`에서 **신규 `tunaround codex-inject`**로 교체.

## 4. 주입 시퀀스 (codex-inject 1회 실행 = task 1건)

1. ws 접속 `ws://127.0.0.1:PORT`.
2. `initialize { clientInfo: { name: "tunaround-inject", version } }`.
3. thread 확보(§5.1): 최초엔 `thread/start`(threadId 파일에 영속), 이후엔 그 threadId로 `thread/resume`.
4. `turn/start { threadId, input: [{ type:"text", text: "<브로커 task 처리 지시 + task 메시지>" }], approvalPolicy: <정책> }`.
5. 알림 스트림 수신: `item/agentMessage/delta`(응답 텍스트) 흘려보고, **`turn/completed`** 오면 종료. 승인 ServerRequest는 §5.2 정책대로 처리.
6. codex는 이 턴 안에서 tuna-broker MCP 도구(`claim_task` -> 처리 -> `complete_task`)를 호출한다(글루가 claim/complete하는 게 아니라 codex가 in-thread로). 사람이 붙어 있으면 그 과정을 라이브로 본다.

## 5. 핵심 설계 결정

### 5.1 thread 소유 = 글루(결정론)
- 글루가 `thread/start`로 thread를 만들고 `threadId`를 파일(예: `~/.tunaround/codex-sup-<agent>.thread`)에 영속. 이후 task마다 같은 threadId로 `thread/resume` -> `turn/start` = 맥락 누적(티키타카).
- 사람은 `codex --remote`로 붙어 `thread/list`에서 그 thread를 고른다. **열린 질문(§7)**: `--remote` TUI가 특정 threadId를 선택/자동부착하는 UX는 라이브 확인 필요.
- 대안(글루가 아니라 사람이 thread 소유 -> 글루가 `thread/loaded/list`로 발견)은 비결정적이라 비채택.

### 5.2 승인 = injector가 MCP elicitation을 자동 accept (P0 확정)
- **P0 실측**: `approvalPolicy: "never"`여도 MCP 도구 호출은 `mcpServer/elicitation/request`(ServerRequest, id 있음)로 승인을 요청한다. params = `{ threadId, turnId, serverName:"tuna-broker", mode:"form", _meta:{ codex_approval_kind:"mcp_tool_call", ... } }`. 즉 approvalPolicy만으론 MCP 호출이 무프롬프트가 안 된다.
- **결정**: injector가 이 ServerRequest에 `{ jsonrpc, id, result:{ action:"accept" } }`로 **자동 응답**해야 도구가 진행되고 턴이 완료된다(P0에서 accept -> `list_agents` 실제 호출 -> `turn/completed` 확인). 감독의 행위는 tuna-broker MCP 호출뿐이라 자동 accept가 안전하다(메모리 [[readonly-soft-enforcement-ok]]).
- 다른 ServerRequest(execCommandApproval, applyPatchApproval, item/*/requestApproval 등)도 injector가 정책대로 응답(감독 레인 기본=자동 승인 최소셋, 그 외 로그). 사람이 `--remote`로 붙어 있어도 injector가 응답하므로 승인 병목 없음.

### 5.3 토큰 전파 = 필수
- `codex app-server`는 반드시 `TUNA_BROKER_TOKEN=<tok>`을 env로 받고 떠야 한다. 안 그러면 tuna-broker MCP 미로드 -> codex가 raw HTTP로 자가구조(세션12 186k 낭비 재현). 문서·기동 스크립트에 못박는다.

### 5.4 워커와의 역할 분리
- **워커** = 헤드리스 `work`/exec(fresh runner, claim->run->complete). 맥락·사람 관전 없음. 기존 유지.
- **감독** = 본 설계(라이브 app-server thread + inject). 맥락 누적 + HITL 가시.
- `poll` watcher 골격은 공유하되 `--on-task` 대상만 다르다(워커=`work`류, 감독=`codex-inject`).

### 5.5 크로스머신 = 브로커가 담당(ws 포워딩 불요)
- app-server ws는 **감독 머신 로컬**. poll watcher도 그 머신에서 돌며 (원격일 수 있는) 브로커를 폴링해 로컬 app-server에 주입한다. 즉 크로스머신 채널은 이미 브로커다. ws 포트포워딩은 **다른 머신에서 `--remote`로 관전할 때만** 필요(SSH). 맥 총감독 -> 윈 codex 감독은 브로커 경유라 ws 노출 불필요.

## 6. 구현 범위

- **신규 서브커맨드 `tunaround codex-inject`**(worker 피처): ws 클라이언트. CLI 계약(확정, 모든 T가 이 인터페이스에 정렬):
  - `--ws <url>` (필수, 예 `ws://127.0.0.1:8790`) / `--agent <id>` (필수, thread 영속 키) / `--text <msg>` (필수, 주입할 유저 턴) / `--approval <policy>` (기본 `never`) / `--sandbox <mode>` (기본 `workspace-write`) / `--timeout <secs>` (기본 300) / `--new` (영속 threadId 무시하고 새 thread).
  - 동작: ws 접속 -> `initialize` -> thread 확보(`--agent`별 영속 파일 `~/.tunaround/codex-sup-<agent>.thread` 있으면 `thread/resume`, 없거나 `--new`면 `thread/start` 후 `result.thread.id`를 그 파일에 기록) -> `turn/start`(text) -> 알림 루프: `mcpServer/elicitation/request`·승인 ServerRequest는 T1 헬퍼(`build_elicitation_accept`/`build_approval_granted`)로 자동응답, `item/agentMessage/*`는 stdout(Monitor 관측), 우리 threadId의 `turn/completed`에서 종료.
  - 종료코드: 성공(turn/completed) 0 / 타임아웃·프로토콜 에러 비-0.
  - 의존성: `tokio-tungstenite`(worker 피처 게이트). ws 자체는 무인증(localhost 바인드). 토큰은 codex-inject가 아니라 **app-server 프로세스 env**에 필요(§5.3).
  - on-task 배선: `poll --on-task 'tunaround codex-inject --ws ws://127.0.0.1:PORT --agent <id> --text "브로커 task {id}를 claim_task로 처리하고 complete_task로 보고하라"'`.
- **기동 헬퍼(선택)**: `codex app-server`를 토큰 env로 상주시키고 threadId를 seed하는 노드/스크립트. node.toml 감독 레인 안내 문구에 이 경로 반영.
- **문서**: a2a-usage에 codex 감독(app-server) 레시피 추가. dev-mac-windows에 SSH 관전 노트.
- **신규 소스 파일 첫 줄 = 역할 한국어 주석**(규율 #6). 예: `// codex app-server에 ws로 유저 턴을 주입하는 클라이언트`.

## 7. 열린 질문 -> P0(2026-07-05 stdio 실측)로 대부분 해소

P0 = `codex app-server --listen stdio://`를 파이프로 구동해 initialize->thread/start->turn/start->turn/completed 왕복 성립(파이썬 드라이버). ws 대신 stdio로 프로토콜만 확정(실제 injector는 ws, 프로토콜 동일).

- **확정된 프로토콜 사실**:
  - thread id 경로 = **`result.thread.id`**(`result.threadId` 아님). thread rollout이 `~/.codex/sessions/.../rollout-*.jsonl`로 저장 -> `thread/resume` 재개 가능(질문 4 해소).
  - `initialize`(id 응답) -> `thread/start`{approvalPolicy,"sandbox":"workspace-write",cwd} -> `turn/start`{threadId, approvalPolicy, input:[{type:"text",text}]}.
  - 완료 = `turn/completed` 알림(params에 threadId/turnId). 최종답 = `item/completed`의 item.type=="agentMessage", phase=="final_answer".
  - **승인**: MCP 도구 호출은 approvalPolicy=never여도 `mcpServer/elicitation/request`로 옴 -> injector가 `{result:{action:"accept"}}` 응답 필수(질문 2·3 해소, §5.2).
  - approvalPolicy enum = untrusted/on-failure/on-request/never. sandbox = read-only/workspace-write/danger-full-access. sandboxPolicy(turn) = readOnly/workspaceWrite/dangerFullAccess/externalSandbox.
  - **native MCP 확증**: accept 후 codex가 tuna-broker `list_agents` 실제 호출, 정답 반환, **raw HTTP 폴백 0**(토큰 env 전제).
- **남은 라이브 확인(ws 단계, T2·T5)**:
  1. `codex --remote ws://`가 글루-소유 threadId를 선택/부착하는 UX(관전).
  2. ws에서 다중 접속 시 알림 브로드캐스트/ServerRequest 라우팅(글루가 자기 turnId 필터, 붙은 TUI와 응답 경합 여부).
  3. app-server 재기동 후 `thread/resume`로 rollout 복구 실동작.

## 8. 비범위

- 관리형 `remote-control`/daemon(Unix 전용) 경로. Windows raw ws로 통일.
- claude 감독(별건, Monitor+poll로 이미 깔끔).
- 워커 경로 변경(현행 유지).
- push 알림/webhook, 다중 thread 동시 감독(YAGNI).

## 9. 검증 계획

1. 단위: codex-inject의 JSON-RPC 프레이밍·알림 파싱 순수부 테스트(가짜 ws 서버 또는 프레임 픽스처).
2. 라이브 스모크(로컬): app-server 기동(토큰 env) -> codex-inject로 "1+1?" turn/start -> `turn/completed`+agentMessage 수신, **raw HTTP 폴백 0**(codex가 native tuna-broker MCP로 claim/complete하는지 broker.db로 교차검증).
3. 티키타카: 총감독(별도 claude TUI, tuna-broker MCP)이 send_task 2~3회 -> 매번 codex 감독 라이브 thread가 맥락 유지하며 응답 -> broker.db로 completed 확인.
4. HITL: `codex --remote`로 붙어 라이브 대화가 보이는지.
5. 3-OS CI(codex 미설치 환경은 라이브 스모크 스킵, 순수부만).
