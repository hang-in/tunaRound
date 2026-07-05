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

### 어드레싱: UUID(라우팅) + 태그(발견)

브로커는 본질적으로 **agent-id 라우팅 task 큐**입니다. `to_agent`는 큐의 subject이고, 그 id를 폴링하는 워커가 소비자입니다. 소비자가 없는 id로 던지면 작업이 조용히 영원히 `submitted`로 남습니다(세션9 실증: dispatcher id로 던져 폴러 없음). tunaRound는 이를 두 층으로 해결합니다.

- **UUID = 라우팅 키.** 워커가 뜰 때 자가 발급(`--agent` 미지정 시 자동)하거나 사람이 읽기 쉬운 id(`win-worker`)를 직접 줍니다. task의 `to_agent`엔 항상 **구체 id 하나**가 남습니다.
- **태그 = 발견 키.** 워커는 `--tags "machine=win,runner=claude,role=worker"`로 자기를 광고하고, dispatcher는 그 태그로 **발송 시점에 대상을 발견**(`to_selector`)합니다. 발송자가 문자열을 손으로 맞출 필요가 없어 오타-불일치(no-consumer)가 구조적으로 줄어듭니다. 상세는 §9.

**태그 관례**(강제 아님, 표준키만 합의):

- `machine`(win/mac/linux) · `runner`(claude/codex/opencode/llm/a2a) · `role`(worker/supervised/dispatch) · `project`(레포명) · `mode`(read/write). 자유 키도 허용됩니다.
- 예전 네이밍 규약(`{머신}-{역할}`)은 이제 **태그로 흡수**됩니다. `win-worker`라는 이름 대신 `machine=win,role=worker` 태그로 같은 정보를 라우팅 가능한 형태로 담습니다. 사람이 읽는 id는 `display_name`으로 따로 둘 수 있습니다.

여전히 유효한 규칙:

- **`to_agent`/셀렉터 대상은 폴링하는 워커만.** dispatcher id는 `from_agent` 전용이며 절대 대상이 되지 않습니다(던지는 쪽은 poll/claim을 하지 않으므로). 태그로도 dispatcher를 `role=worker`로 광고하지 않습니다.

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
| `--agent <id>` | 이 워커의 id. 이 id를 `to_agent`로 가진 작업만 집습니다. **생략 시 자가 uuid 생성**(로그에 출력). |
| `--tags <k=v,..>` | 로스터 발견용 태그. dispatcher가 `to_selector`로 이 워커를 찾습니다(§9). 예: `machine=win,runner=claude,role=worker`. |
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

---

## 9. 에이전트 레지스트리 (등록 · 발견 · 셀렉터 라우팅)

`to_agent`로 문자열을 손으로 맞추는 대신, **워커가 태그로 자기를 광고하고 dispatcher가 태그로 발견**합니다. 로스터는 코어 인메모리라 코어를 재기동하면 비고, 워커가 heartbeat 실패를 감지해 자동 재등록합니다(영속 아님, 재등록으로 복원).

### 9a. 워커 자동 등록

`tunaround work`에 `--tags`를 주면 뜰 때 로스터에 자기 등록하고, 매 폴마다 heartbeat로 online을 유지합니다.

```bash
tunaround work \
  --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --tags "machine=win,runner=claude,role=worker,project=tunaround" \
  --runner claude
# --agent 미지정 -> 자가 uuid 생성(로그: "[work] --agent 미지정 -> 자가 uuid 생성: <uuid>")
# 등록됨(로그: "[work] 로스터 등록: 등록됨: uuid=<uuid> tags=4개")
```

`--agent`로 사람이 읽는 id(`win-worker`)를 직접 줘도 됩니다(그 id가 uuid 자리에 들어갈 뿐).

### 9b. dispatcher가 online 워커 발견 (MCP `list_agents`)

```
list_agents                       # online 전부
list_agents selector="runner=claude"   # 태그로 필터(부분집합 매칭)
# -> [<uuid>] <display> tags: machine=win, runner=claude, role=worker (heartbeat=...)
```

`heartbeat`가 90초(AGENT_TTL_SECS) 넘게 끊긴 워커는 목록에서 빠집니다.

### 9c. 태그로 작업 던지기 (`to_selector`)

`send_task`(MCP) 또는 `SendMessage`(`/a2a`)에 `to_agent` 대신 `to_selector`를 줍니다. 코어가 **발송 시점에 매칭 online uuid로 해석**합니다.

- MCP: `send_task from_agent=win-opus to_selector="runner=claude,project=tunaround" text="..."`
- `/a2a`: `SendMessage` params에 `"toSelector":"runner=claude"` (그리고 `toAgent`는 생략).

해석 결과:

| 매칭 수 | 코어 동작 |
| --- | --- |
| 1개 | 그 uuid로 task 생성(정상 라우팅). task의 `to_agent`엔 구체 uuid가 남습니다. |
| 0개 | **no-consumer 안내**(task 생성 안 함). "list_agents로 확인하세요". |
| 2개+ | **후보 목록 반환**(task 생성 안 함). dispatcher가 `to_agent`로 하나를 골라 재요청(HITL). |

`to_agent`와 `to_selector`는 배타입니다(둘 다 주면 에러). **레거시 경로 불변**: `to_agent`에 문자열/uuid를 직접 주면 예전처럼 그 id로 exact-match 라우팅합니다(레지스트리 우회).

> 다중 매칭을 코어가 자동 배정(부하분산)하지 않고 dispatcher에게 되돌리는 건 semi-a2a(사람이 대상 결정)에 맞춘 기본값입니다. 자동 배정은 후속(YAGNI).

---

## 10. codex 라이브 감독 (app-server)

지금까지의 워커(§2·§3)는 **헤드리스**(fresh runner가 claim→처리→complete하고 끝)입니다. 반면 **감독**은 사람이 관전할 수 있는 **라이브 세션**이 맥락을 누적하며 브로커 작업을 받아 처리해야 합니다. claude 감독은 세션 하네스(Monitor)로 외부에서 깨울 수 있지만, codex는 그 메커니즘이 없어 별도 경로가 필요합니다.

### 개념

codex 감독을 헤드리스 `exec`가 아니라 **`codex app-server`가 띄운 라이브 thread**로 둡니다. 브로커에 작업이 도착하면 `poll --on-task`가 그 thread에 ws로 `turn/start`를 주입해 깨우고, codex는 그 턴 안에서 tuna-broker MCP 도구(`claim_task`→처리→`complete_task`)를 직접 호출합니다. 사람은 `codex --remote`로 그 라이브 thread에 붙어 대화를 관전(HITL)할 수 있으며, 붙어 있지 않아도 감독은 동작합니다.

- **워커와의 구분**: 워커(`tunaround work`)는 맥락 없는 헤드리스 프로세스입니다. 감독은 맥락을 누적하는 라이브 thread + 사람 관전(선택)입니다. 같은 `poll` 골격을 쓰되 `--on-task` 대상만 다릅니다(워커=`work`류, 감독=`codex-inject`).
- 정본 설계: [docs/design/v2-codex-live-supervisor-appserver_2026-07-05.md](../design/v2-codex-live-supervisor-appserver_2026-07-05.md).

### 세팅 절차

```bash
# 1) app-server 기동 (감독 머신에서 상주). 토큰 env 필수 - 없으면 tuna-broker MCP가
#    로드 안 돼 codex가 raw HTTP로 자가구조하며 토큰을 낭비합니다(설계 §5.3).
TUNA_BROKER_TOKEN=<TOKEN> codex app-server --listen ws://127.0.0.1:<PORT>

# 2) (선택, HITL 관전) 사람이 라이브 thread에 붙어 대화를 지켜봄
codex --remote ws://127.0.0.1:<PORT>

# 3) 감독 등록: codex(또는 app-server thread)가 register_agent로 로스터에 광고
#    (uuid=<agent-id>, tags="machine=<win|mac>,runner=codex,role=supervised,project=tunaround")

# 4) 감시 + 주입: 브로커 작업 도착 시 codex-inject가 ws로 turn/start를 쏨
tunaround poll --core <core-url> --token <TOKEN> --agent <agent-id> \
  --on-task 'tunaround codex-inject --ws ws://127.0.0.1:<PORT> --agent <agent-id> \
             --text "브로커 task {id}를 claim_task로 처리하고 complete_task로 보고하라"'
```

### 동작

1. 브로커에 작업이 도착 → `poll`이 감지 → `--on-task`로 `codex-inject`를 실행.
2. `codex-inject`가 ws로 접속해 (영속된 threadId가 있으면 `thread/resume`, 없으면 `thread/start` 후) `turn/start`를 주입.
3. 라이브 thread(codex)가 그 턴 안에서 tuna-broker MCP의 `claim_task`→처리→`complete_task`를 native 호출.
4. 사람이 `codex --remote`로 붙어 있으면 이 과정이 실시간으로 보입니다. 붙어 있지 않아도 완료됩니다.

사람 릴레이는 0입니다 - task 도착부터 완료 보고까지 사람 개입 없이 기계가 돕니다(감독 스코프는 no-shuttle).

### exec-resume과의 구분

이전(세션12)에는 `poll --on-task 'codex exec resume --last ...'`로 codex 감독을 우회했으나, 이건 **별개 프로세스(워커 패턴)**라 라이브 TUI가 아니었고 맥락이 매번 새로 열렸습니다(게다가 토큰 미전파로 186k 토큰 낭비). `exec resume`은 워커(헤드리스) 스코프에 남고, **감독 스코프는 app-server 경로**로 대체됩니다.

### 승인

`approvalPolicy: never`로 기동해도 MCP 도구 호출은 `mcpServer/elicitation/request`(ServerRequest)로 승인을 요청합니다 - approvalPolicy만으론 MCP 호출이 무프롬프트가 안 됩니다. `codex-inject`가 이 요청에 자동으로 `{action:"accept"}`를 응답해야 도구가 진행되고 턴이 완료됩니다(설계 §5.2). 감독의 행위 범위가 tuna-broker MCP 호출뿐이라 자동 accept는 안전합니다.

### 플랫폼

관리형 `remote-control`/`app-server daemon`은 Unix 전용이지만, raw `codex app-server --listen ws://`는 **크로스플랫폼**(Windows 포함)으로 확인됐습니다. Windows 감독 머신도 이 경로를 그대로 씁니다.
