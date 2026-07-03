# A2A 작업 위임 사용법 (dispatcher · 코어 · 워커)

> tunaRound 코어를 표준 A2A(Agent2Agent) 작업 브로커로 쓰는 실전 가이드. 파트너 에이전트에게 작업을 맡기고(dispatcher), 워커가 자율로 처리(worker)하며, 진행을 실시간으로 지켜보는(SSE) 전체 흐름. 설계 배경은 [파트너 위임](../design/v2-a2a-partner-delegation_2026-07-02.md) · [스트리밍](../design/v2-a2a-streaming_2026-07-03.md) · [워커 데몬](../design/v2-a2a-worker-daemon_2026-07-03.md).

## 0. 세 역할

- **코어(broker)**: `serve`로 띄운 상주 프로세스. 작업 큐(SQLite `tasks` 테이블)와 A2A 서버(`/a2a`, `/.well-known/agent-card.json`)를 노출한다. 작업의 상태 전이 권위는 코어 하나뿐이다.
- **dispatcher**: 코어에 작업을 던지고 결과를 받는 쪽. `SendMessage`(단발) 또는 `SendStreamingMessage`(SSE 실시간)로 던지고 `GetTask`로 확인한다.
- **worker**: 자기 앞으로 온 작업을 처리하는 쪽. `tunaround work` 데몬이 poll -> claim -> 러너 실행 -> complete를 사람 개입 없이 돈다.

코어 하나에 dispatcher·worker가 여럿 붙을 수 있고, 작업은 `to_agent`(받는 워커 id)로 라우팅된다.

## 1. 코어 띄우기

```bash
tunaround serve 0.0.0.0:8770 --token <TOKEN> --db ~/.tunaround/broker.db
```
- `--token`은 bearer 인증. 레포에 커밋하지 말고 각자 환경/설정에만 둔다.
- `serve`는 A2A 이벤트 버스(SSE 스트리밍용)를 자동으로 켠다.
- 같은 LAN이면 `0.0.0.0:<port>` + 사설 IP로 접속, 아니면 Tailscale/SSH 터널. 방화벽 인바운드 포트를 열어야 한다.
- 빌드: `cargo build --features "serve"`(스트리밍·A2A 포함). 워커 데몬까지 한 바이너리로 쓰려면 `--features "serve worker engines"`.

Agent Card로 코어가 뭘 지원하는지 확인(선택):
```bash
curl -s -H "Authorization: Bearer <TOKEN>" http://<코어-IP>:8770/.well-known/agent-card.json
# -> capabilities: {"streaming": true, "pushNotifications": false}
```

## 2. 워커 데몬 띄우기

워커는 코어의 `/mcp` 엔드포인트에 붙어 `poll_tasks`/`claim_task`/`complete_task`를 돈다.

```bash
# win-worker 앞 작업을 Claude로 자율 처리 (15초 간격 상시 폴링)
tunaround work \
  --core http://<코어-IP>:8770/mcp \
  --token <TOKEN> \
  --agent win-worker \
  --runner claude
```

주요 옵션(`tunaround work --help`):
- `--core <url>`: 코어의 **`/mcp`** URL(끝에 `/mcp` 포함).
- `--token <T>`: 코어 bearer 토큰.
- `--agent <id>`: 이 워커의 id. 이 id를 `to_agent`로 가진 작업만 집는다.
- `--runner <claude|codex|opencode|http>`: 작업을 실행할 러너(기본 `claude`).
- `--model <m>`: 러너 모델(선택).
- `--project-path <p>`: 러너가 작업할 디렉터리(선택). 프로젝트별 격리에 사용.
- `--interval <secs>`: 폴링 간격(기본 15). `--once`: 한 번만 폴링하고 종료(테스트·수동용).
- `--write`: 러너를 쓰기 모드로(기본은 읽기 전용 = behavioral read-only).
- `--http-base-url <url>`: `--runner http`일 때 LLM 엔드포인트(예: `http://127.0.0.1:11434`, 끝에 `/v1` 붙이지 않음).

## 3. 이기종 파트너 (러너만 바꾸면 됨)

같은 데몬을 어떤 러너로 띄우느냐가 파트너 종류다.

```bash
# Codex 워커
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent codex-worker --runner codex

# 로컬 LLM(Ollama, OpenAI 호환) 워커
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent llm-worker --runner http \
  --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b
```
- `--runner http`는 `engines` 피처가 필요하다(`cargo build --features "serve worker engines"`).
- Ollama는 bearer 인증을 무시하므로 지금은 `--token`이 LLM 키로도 쓰인다. 인증이 필요한 LLM 엔드포인트를 붙일 때는 키 분리가 필요하다(후속 `--http-api-key`).
- 로컬 모델이 콜드 상태면 첫 응답이 수십 초 걸릴 수 있다(모델 로드).

## 4. 작업 던지기 (dispatcher)

### 4a. 단발: SendMessage + GetTask

작업 생성:
```bash
curl -s -H "Authorization: Bearer <TOKEN>" -H "Content-Type: application/json" \
  -X POST http://<코어-IP>:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"1","method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"user",
                   "parts":[{"text":"이 함수의 시간복잡도를 한 줄로 설명해줘"}]},
        "fromAgent":"my-dispatcher","toAgent":"win-worker"}}'
# -> result.id = 생성된 task_id (32 hex)
```
결과 확인(워커가 처리하면 completed + artifact):
```bash
curl -s -H "Authorization: Bearer <TOKEN>" -H "Content-Type: application/json" \
  -X POST http://<코어-IP>:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"1","method":"GetTask","params":{"id":"<task_id>"}}'
# -> result.state = "completed", result.artifacts[0].parts[0].text = 워커의 결과
```

### 4b. 실시간: SendStreamingMessage (SSE)

던지는 즉시 SSE 스트림이 열리고, 작업 생명주기가 프레임으로 흘러온다. `curl -N`(버퍼 끄기) 필수.
```bash
curl -N -s -H "Authorization: Bearer <TOKEN>" -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -X POST http://<코어-IP>:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"s1","method":"SendStreamingMessage","params":{
        "message":{"messageId":"m1","role":"user","parts":[{"text":"작업 지시..."}]},
        "fromAgent":"my-dispatcher","toAgent":"win-worker"}}'
```
받게 되는 프레임(각 `data:` 줄은 JSON-RPC 응답, `result`는 StreamResponse):
```
data: {... "result":{"task":{... "state":"submitted" ...}}}          # 초기 스냅샷
:                                                                     # keep-alive 하트비트
data: {... "result":{"statusUpdate":{"status":{"state":"working"},"final":false,...}}}
data: {... "result":{"artifactUpdate":{"artifact":{...결과...},"lastChunk":true,...}}}
data: {... "result":{"statusUpdate":{"status":{"state":"completed"},"final":true,...}}}   # 여기서 종료
```
- `final:true` 프레임 뒤 스트림이 닫힌다.
- 워커가 느리면(예: 로컬 LLM 콜드, Codex) `curl --max-time`을 넉넉히 준다. 스트림이 끊겨도 작업은 코어에서 계속되며 `GetTask`나 `SubscribeToTask`로 이어 확인할 수 있다.
- **재구독**: 이미 진행 중인 작업에 다시 붙으려면 `SubscribeToTask`(params `{"id":"<task_id>"}`)를 쓴다. 현재 스냅샷을 먼저 받고 이후 이벤트를 이어받는다.

### 4c. 대화형 에이전트가 dispatcher일 때 (MCP 도구)

Claude Code·Codex 같은 대화형 세션이 코어를 MCP 서버로 등록하면, `send_task`/`get_task` MCP 도구로 던지고 확인할 수 있다(사람이 도구 호출을 승인). raw HTTP를 쓰기 싫을 때의 경로다. 등록:
```bash
claude mcp add --transport http tuna-core http://<코어-IP>:8770/mcp \
  --header "Authorization: Bearer <TOKEN>"
```
등록 후 새 세션에서 `send_task from_agent=... to_agent=win-worker text="..."` -> `get_task task_id=...`.

## 5. 프로젝트별 라우팅

작업 큐는 코어 db 하나에 평평하게 있고 `to_agent`로만 갈린다. 프로젝트를 격리하려면 **프로젝트마다 워커 데몬을 따로** 띄우고 agent id와 작업 디렉터리를 분리한다.
```bash
# 프로젝트 A 워커
tunaround work --core ... --token ... --agent projA-worker --project-path /repos/A --runner claude
# 프로젝트 B 워커
tunaround work --core ... --token ... --agent projB-worker --project-path /repos/B --runner codex
```
dispatcher는 `to_agent`를 `projA-worker` 또는 `projB-worker`로 지정해 라우팅한다. (작업의 `context_id`로 한 데몬이 여러 프로젝트를 자동 배분하는 방식은 후속.)

## 6. 자율 수준 · 안전

- **semi-a2a(HITL)**: 사람이 목표(작업)를 발행할 뿐, 발견·실행·완료·통지는 기계끼리 처리한다. 사람 없이 무한히 도는 자동 토론 루프는 두지 않았다.
- 워커 데몬은 **opt-in**(직접 `tunaround work`를 띄워야 동작)이고 러너는 **기본 읽기 전용**이다. 파일을 실제로 고치게 하려면 `--write`.
- 러너는 그 머신에 설치·로그인된 claude/codex/opencode에 의존한다. `--runner http`는 지정한 LLM 엔드포인트에만 의존한다.

## 7. 빠른 로컬 왕복(한 머신에서 검증)

```bash
# 1) 코어
tunaround serve 0.0.0.0:8770 --token DEMO --db /tmp/broker.db &

# 2) 작업 던지기(win-worker 앞), task_id 기록
curl -s -H "Authorization: Bearer DEMO" -H "Content-Type: application/json" -X POST http://127.0.0.1:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"1","method":"SendMessage","params":{"message":{"messageId":"m1","role":"user","parts":[{"text":"broadcast 채널을 한 줄로 설명"}]},"fromAgent":"disp","toAgent":"win-worker"}}'

# 3) 워커 한 번 돌리기(자율 발견->실행->완료)
tunaround work --once --agent win-worker --runner claude --core http://127.0.0.1:8770/mcp --token DEMO

# 4) 결과 확인
curl -s -H "Authorization: Bearer DEMO" -H "Content-Type: application/json" -X POST http://127.0.0.1:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"1","method":"GetTask","params":{"id":"<task_id>"}}'
```
