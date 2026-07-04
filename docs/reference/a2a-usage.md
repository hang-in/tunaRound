# A2A 작업 위임 사용법 (dispatcher · 코어 · 워커)

> tunaRound 코어를 표준 A2A(Agent2Agent) 작업 브로커로 쓰는 실전 가이드입니다. 파트너 에이전트에게 작업을 맡기고(dispatcher), 워커가 자율로 처리하며(worker), 진행을 실시간으로 지켜보는(SSE) 전체 흐름을 다룹니다.
>
> 설계 배경: [파트너 위임](../design/v2-a2a-partner-delegation_2026-07-02.md) · [스트리밍](../design/v2-a2a-streaming_2026-07-03.md) · [워커 데몬](../design/v2-a2a-worker-daemon_2026-07-03.md)

---

## 0. 세 역할

- **코어 (broker)**
  `serve`로 띄운 상주 프로세스. 작업 큐(SQLite `tasks` 테이블)와 A2A 서버(`/a2a`, `/.well-known/agent-card.json`)를 노출합니다. 작업의 상태 전이 권위는 코어 하나뿐입니다.
- **dispatcher**
  코어에 작업을 던지고 결과를 받는 쪽. `SendMessage`(단발) 또는 `SendStreamingMessage`(SSE 실시간)로 던지고 `GetTask`로 확인합니다.
- **worker**
  자기 앞으로 온 작업을 처리하는 쪽. `tunaround work` 데몬이 poll → claim → 러너 실행 → complete를 사람 개입 없이 돕니다.

코어 하나에 dispatcher·worker가 여럿 붙을 수 있고, 작업은 `to_agent`(받는 워커 id)로 라우팅됩니다.

### 네이밍/어드레싱 규약 (중요)

브로커는 본질적으로 **agent-id 라우팅 task 큐**입니다. `to_agent`는 큐의 subject이고, 그 id를 폴링하는 워커가 소비자입니다. 소비자가 없는 id로 던지면 작업이 조용히 영원히 `submitted`로 남습니다(세션9 실증: dispatcher id로 던져 폴러 없음). 이를 규약으로 없앱니다.

- **`to_agent`는 폴링하는 워커 id만 씁니다.** dispatcher id는 `from_agent` 전용이며 절대 `to_agent`가 되지 않습니다(던지는 쪽은 poll/claim을 하지 않으므로).
- **네이밍은 `{머신}-{역할|러너}`** 로, 이름만 봐도 워커인지·러너가 뭔지 드러나게 합니다.
  - 워커(소비자): `win-worker`·`mac-worker`(claude 기본), `mac-codex`·`mac-llm`(러너 명시).
  - dispatcher(생산자): `{머신}-dispatch` 또는 사람 이름(`win-opus`). 이건 던지기 전용입니다.
- **레인 종류 접미어**: 자동(헤드리스 데몬)=`-worker`류, 감독(대화형 세션이 poll)=`-claude`/`-codex`류.

> 코어는 미배달을 표시로 알립니다: `submitted`가 오래 claim 안 되면 `get_task`/`tasks`/`poll` 출력에 `⚠no-consumer?`가, `working`이 오래 멈춰 있으면 `⚠stuck?`가 붙습니다(§8). 자동 전이(requeue)는 두지 않았습니다(semi-a2a: 사람이 재던짐 결정).

> **표준 호환 범위 (정직하게):** 이 A2A는 A2A 프로토콜의 **구조를 차용**해 tunaRound 인스턴스끼리 위임하는 것이 목적입니다. JSON-RPC envelope·`GetTask`는 독립 A2A 클라이언트와 호환됨을 확인했으나(interop 스모크, context-notes 참조), (1) Agent Card가 인증 게이트 + 구식 단일-url 스키마라 표준 클라의 발견이 안 되고, (2) `SendMessage`가 브로커 라우팅 필드(`fromAgent`/`toAgent`)를 요구해 표준 클라가 task를 못 만듭니다. **임의의 제3자 표준 A2A 클라이언트와의 완전 호환은 비목표**이며, 필요해지면 표준↔브로커 번역 어댑터를 별도로 둡니다.

---

## 1. 코어 띄우기

```bash
tunaround serve 0.0.0.0:8770 --token <TOKEN> --db ~/.tunaround/broker.db
```

- `--token`은 bearer 인증. 레포에 커밋하지 말고 각자 환경/설정에만 둡니다.
- `serve`는 A2A 이벤트 버스(SSE 스트리밍용)를 자동으로 켭니다.
- 같은 LAN이면 `0.0.0.0:<port>` + 사설 IP로 접속, 아니면 Tailscale/SSH 터널을 씁니다. 방화벽 인바운드 포트를 열어야 합니다.
- 빌드: `cargo build --features "serve"`(스트리밍·A2A 포함). 워커 데몬까지 한 바이너리로 쓰려면 `--features "serve worker engines"`.

Agent Card로 코어가 뭘 지원하는지 확인(선택):

```bash
curl -s -H "Authorization: Bearer <TOKEN>" http://<코어-IP>:8770/.well-known/agent-card.json
# -> capabilities: {"streaming": true, "pushNotifications": false}
# -> buildFeatures: ["sqlite","mcp","serve","worker", ...]  # 이 코어가 컴파일된 피처(능력 발견용)
```

`buildFeatures`로 코어가 무슨 러너/기능을 할 수 있는지 알 수 있습니다(`engines`=http 러너, `a2a-out`=외부 표준 A2A 위임, `worker`=워커 클라이언트). dispatcher·doctor가 라우팅·진단에 참고합니다.

---

## 2. 워커 데몬 띄우기

워커는 코어의 `/mcp` 엔드포인트에 붙어 `poll_tasks` / `claim_task` / `complete_task`를 돕니다.

```bash
# win-worker 앞 작업을 Claude로 자율 처리 (15초 간격 상시 폴링)
tunaround work \
  --core http://<코어-IP>:8770/mcp \
  --token <TOKEN> \
  --agent win-worker \
  --runner claude
```

주요 옵션 (`tunaround work --help`):

| 옵션 | 설명 |
| --- | --- |
| `--core <url>` | 코어의 **`/mcp`** URL (끝에 `/mcp` 포함). |
| `--token <T>` | 코어 bearer 토큰. |
| `--agent <id>` | 이 워커의 id. 이 id를 `to_agent`로 가진 작업만 집습니다. |
| `--runner <claude\|codex\|opencode\|http>` | 작업을 실행할 러너 (기본 `claude`). |
| `--model <m>` | 러너 모델 (선택). |
| `--project-path <p>` | 러너가 작업할 디렉터리 (선택). 프로젝트별 격리에 사용. |
| `--interval <secs>` | 폴링 간격 (기본 15). |
| `--once` | 한 번만 폴링하고 종료 (테스트·수동용). |
| `--write` | 러너를 쓰기 모드로 (기본은 읽기 전용 = behavioral read-only). |
| `--http-base-url <url>` | `--runner http`일 때 LLM 엔드포인트 (예: `http://127.0.0.1:11434`, 끝에 `/v1` 붙이지 않음). |

---

## 3. 이기종 파트너 (러너만 바꾸면 됨)

같은 데몬을 어떤 러너로 띄우느냐가 파트너 종류입니다.

```bash
# Codex 워커
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent codex-worker --runner codex

# 로컬 LLM(Ollama, OpenAI 호환) 워커
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent llm-worker --runner http \
  --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b
```

- `--runner http`는 `engines` 피처가 필요합니다(`cargo build --features "serve worker engines"`).
- Ollama는 bearer 인증을 무시하므로 지금은 `--token`이 LLM 키로도 쓰입니다. 인증이 필요한 LLM 엔드포인트를 붙일 때는 키 분리가 필요합니다(후속 `--http-api-key`).
- 로컬 모델이 콜드 상태면 첫 응답이 수십 초 걸릴 수 있습니다(모델 로드).

```bash
# 외부 표준 A2A 에이전트를 워커로 (outbound: 우리가 표준 A2A로 그 에이전트에 위임)
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent bridge-worker --runner a2a \
  --a2a-card http://some-agent.example/ --a2a-token <외부 에이전트 토큰>
```

- `--runner a2a`는 `a2a-out` 피처가 필요합니다(`cargo build --features "serve worker a2a-out"`). a2a-client로 `--a2a-card` URL의 agent-card를 발견해 **표준 A2A `SendMessage`로 위임**하고, 완료까지 `GetTask`를 폴링해 결과 artifact를 받아옵니다. 외부 표준 A2A 에이전트가 그대로 워커가 됩니다(독립 표준 서버 상대 왕복 실증). 반대 방향(제3자가 우리한테 던지기)은 비목표입니다(§0 호환 범위 참조).

---

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

던지는 즉시 SSE 스트림이 열리고, 작업 생명주기가 프레임으로 흘러옵니다. `curl -N`(버퍼 끄기) 필수.

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

- `final:true` 프레임 뒤 스트림이 닫힙니다.
- 워커가 느리면(예: 로컬 LLM 콜드, Codex) `curl --max-time`을 넉넉히 줍니다. 스트림이 끊겨도 작업은 코어에서 계속되며 `GetTask`나 `SubscribeToTask`로 이어 확인할 수 있습니다.
- **재구독**: 이미 진행 중인 작업에 다시 붙으려면 `SubscribeToTask`(params `{"id":"<task_id>"}`)를 씁니다. 현재 스냅샷을 먼저 받고 이후 이벤트를 이어받습니다.

### 4c. 대화형 에이전트가 dispatcher일 때 (MCP 도구)

Claude Code·Codex 같은 대화형 세션이 코어를 MCP 서버로 등록하면, `send_task` / `get_task` MCP 도구로 던지고 확인할 수 있습니다(사람이 도구 호출을 승인). raw HTTP를 쓰기 싫을 때의 경로입니다.

등록:

```bash
claude mcp add --transport http tuna-core http://<코어-IP>:8770/mcp \
  --header "Authorization: Bearer <TOKEN>"
```

등록 후 새 세션에서 `send_task from_agent=... to_agent=win-worker text="..."` → `get_task task_id=...`.

---

## 5. 프로젝트별 라우팅

작업 큐는 코어 db 하나에 평평하게 있고 `to_agent`로만 갈립니다. 프로젝트를 격리하려면 **프로젝트마다 워커 데몬을 따로** 띄우고 agent id와 작업 디렉터리를 분리합니다.

```bash
# 프로젝트 A 워커
tunaround work --core ... --token ... --agent projA-worker --project-path /repos/A --runner claude
# 프로젝트 B 워커
tunaround work --core ... --token ... --agent projB-worker --project-path /repos/B --runner codex
```

dispatcher는 `to_agent`를 `projA-worker` 또는 `projB-worker`로 지정해 라우팅합니다.

**데몬 하나로 여러 프로젝트 배분 (`--context-map`):** 작업의 `context_id`를 프로젝트 키로 삼아, 워커 하나가 각 작업을 맞는 디렉터리에서 실행합니다.

```bash
tunaround work --core ... --token ... --agent shared-worker \
  --context-map "projA=/repos/A,projB=/repos/B" --project-path /repos/default --runner claude
```

dispatcher는 작업을 던질 때 `context_id`를 넣습니다(`SendMessage` params의 `message.contextId`, 또는 MCP `send_task`의 `context_id`). 워커는 그 값을 `--context-map`에서 찾아 project-path를 정하고, 매핑에 없으면 `--project-path`로 폴백합니다. 코어의 `poll_tasks` 출력에 `ctx=<context_id>`가 포함되어 워커가 라우팅에 씁니다.

---

## 6. 자율 수준 · 안전

- **semi-a2a (HITL):** 사람이 목표(작업)를 발행할 뿐, 발견·실행·완료·통지는 기계끼리 처리합니다. 사람 없이 무한히 도는 자동 토론 루프는 두지 않았습니다.
- 워커 데몬은 **opt-in**(직접 `tunaround work`를 띄워야 동작)이고 러너는 **기본 읽기 전용**입니다. 파일을 실제로 고치게 하려면 `--write`.
- 러너는 그 머신에 설치·로그인된 claude/codex/opencode에 의존합니다. `--runner http`는 지정한 LLM 엔드포인트에만 의존합니다.

---

## 7. 빠른 로컬 왕복 (한 머신에서 검증)

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

---

## 8. 미배달·고착 감지 (거버넌스)

코어는 작업이 조용히 썩는 두 상황을 **표시로** 알립니다(자동 전이는 하지 않습니다 - semi-a2a: 사람이 재던짐 결정). A2A 스펙에 `expired` 같은 상태가 없어 상태를 바꾸는 대신 신호만 붙입니다.

- **`⚠no-consumer?(N분)`**: `submitted`인데 오래(기본 5분 초과) claim이 안 된 작업. 폴링하는 워커가 없다는 뜻(잘못된 `to_agent`로 던졌거나 워커가 안 떠 있음). 세션9의 "dispatcher id로 던져 폴러 없음" 실패가 이 신호로 보입니다.
- **`⚠stuck?(N분)`**: `working`인데 오래(기본 15분 초과) 갱신이 없는 작업. claim한 워커가 죽었을 가능성(프로세스 사망·세션 만료·self-disruption).

어디서 보이나:

| 도구/명령 | 대상 | no-consumer/stuck 표시 |
| --- | --- | --- |
| `get_task(task_id)` (MCP) | dispatcher가 자기 작업 확인 | 붙음 |
| `tasks()` (MCP, 신규) | 브로커 운영자가 **전역** 열린 작업 조망(to_agent 무관) | 붙음 |
| `poll_tasks(agent)` (MCP) | 워커가 자기 앞 작업 확인 | 붙음(워커 데몬은 자동 무시하고 claim) |

폴러가 없는 작업은 아무도 `poll_tasks`를 안 하므로, **`tasks()`가 그런 no-consumer 작업까지 한눈에** 보여줍니다. 신호를 보면 사람이 취소(`CancelTask`)하거나 올바른 `to_agent`로 다시 던집니다.

> 자동 재큐(claim TTL requeue)·하트비트는 두지 않았습니다(개인 2~3머신엔 과함, 후속). self-disruption(워커가 자기 클론을 갈아엎어 stuck)은 §2의 `--write` + 별도 작업 디렉터리로 애초에 막습니다: write 워커의 작업 디렉터리가 노드 실행 클론과 겹치면 코어가 거부합니다(별도 클론/워크트리 필요).
