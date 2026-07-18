# A2A 작업 위임 사용법

이 문서는 tunaRound에서 다른 에이전트나 다른 머신에 작업을 맡기고 결과를 받는 방법을 설명합니다.

처음 사용하는 경우에는 **빠른 시작**만 따라 하면 됩니다. 태그 라우팅, 라이브 세션 수신, 장애 진단 같은 내용은 실제로 필요할 때 뒤 절을 참고하세요.

> 전체 구조를 먼저 보고 싶다면 [mesh 아키텍처](mesh-architecture.md), 설치와 피처 설정은 [온보딩 가이드](onboarding.md)를 참고하세요.

## 먼저 알아둘 세 가지

| 역할 | 하는 일 |
| --- | --- |
| 코어 | 작업을 저장하고 상태를 관리하는 브로커입니다. `tunaround serve`로 실행합니다. |
| 보내는 쪽 | 코어에 작업을 등록하고 결과를 확인합니다. 문서에서는 dispatcher라고 부릅니다. |
| 받는 쪽 | 자기 앞으로 온 작업을 가져가 처리합니다. 보통 `tunaround work` 워커입니다. |

기본 흐름은 단순합니다.

```text
작업 등록 → 워커가 발견 → 워커가 선점 → 러너 실행 → 결과 등록
```

작업 상태는 다음 순서로 바뀝니다.

```text
submitted → working → completed
                    └→ failed
```

사람이 작업 목표와 대상을 정하고, 발견·실행·완료 보고는 기계가 처리합니다. 사람 없이 계속 확장되는 자율 루프는 두지 않습니다.

---

# 1. 빠른 시작

아래 예시는 한 머신에서 코어와 Claude 워커를 띄운 뒤 작업 하나를 왕복시킵니다.

## 1.1 코어 실행

```bash
tunaround serve 0.0.0.0:8770 --token DEMO --db /tmp/broker.db
```

- `--token`은 브로커 접근용 bearer 토큰입니다.
- 실제 토큰은 레포나 셸 히스토리에 남기지 말고 환경변수나 로컬 설정에 둡니다.
- 다른 머신에서 접속할 때는 `127.0.0.1` 대신 코어 머신의 사설 IP를 씁니다.

## 1.2 워커 실행

다른 터미널에서 다음 명령을 실행합니다.

```bash
tunaround work \
  --core http://127.0.0.1:8770/mcp \
  --token DEMO \
  --agent local-worker \
  --runner claude
```

이 워커는 `local-worker` 앞으로 온 작업만 가져갑니다. 기본값은 읽기 전용입니다. 파일을 실제로 수정하게 하려면 `--write`를 명시해야 합니다.

## 1.3 작업 등록

```bash
curl -s \
  -H "Authorization: Bearer DEMO" \
  -H "Content-Type: application/json" \
  -X POST http://127.0.0.1:8770/a2a \
  -d '{
    "jsonrpc":"2.0",
    "id":"1",
    "method":"SendMessage",
    "params":{
      "message":{
        "messageId":"m1",
        "role":"user",
        "parts":[{"text":"이 함수의 시간복잡도를 한 줄로 설명해줘"}]
      },
      "fromAgent":"my-dispatcher",
      "toAgent":"local-worker"
    }
  }'
```

응답의 `result.id`가 작업 ID입니다.

## 1.4 결과 확인

```bash
curl -s \
  -H "Authorization: Bearer DEMO" \
  -H "Content-Type: application/json" \
  -X POST http://127.0.0.1:8770/a2a \
  -d '{
    "jsonrpc":"2.0",
    "id":"2",
    "method":"GetTask",
    "params":{"id":"<task_id>"}
  }'
```

완료된 작업은 `state`가 `completed`이고, 결과는 `artifacts`에 들어 있습니다.

---

# 2. 일반적인 사용

## 2.1 워커 종류 바꾸기

`tunaround work`는 실행할 러너만 바꾸면 Claude, Codex, OpenCode, 로컬 LLM을 같은 방식으로 사용할 수 있습니다.

```bash
# Codex
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent codex-worker --runner codex

# OpenCode
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent opencode-worker --runner opencode

# OpenAI 호환 HTTP 서버 또는 Ollama
tunaround work --core http://<코어-IP>:8770/mcp --token <TOKEN> \
  --agent llm-worker --runner http \
  --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b
```

| 옵션 | 설명 |
| --- | --- |
| `--core <url>` | 코어의 `/mcp` 주소입니다. |
| `--token <token>` | 코어 접근 토큰입니다. |
| `--agent <id>` | 이 워커가 받을 작업의 대상 ID입니다. 생략하면 UUID를 자동 생성합니다. |
| `--runner <name>` | `claude`, `codex`, `opencode`, `http`, `a2a` 중 실행할 러너입니다. |
| `--project-path <path>` | 러너가 작업할 디렉터리입니다. |
| `--write` | 파일 변경을 허용합니다. 기본값은 읽기 전용입니다. |
| `--interval <seconds>` | 작업 확인 간격입니다. 기본값은 15초입니다. |
| `--once` | 한 번만 확인하고 종료합니다. 테스트할 때 사용합니다. |

러너는 해당 머신에 설치되고 로그인된 CLI를 사용합니다. `http` 러너는 지정한 LLM 서버를 사용합니다.

## 2.2 대화형 세션에서 작업 보내기

Claude Code나 Codex에 코어를 MCP 서버로 등록하면 raw HTTP 대신 `send_task`, `get_task`, `list_agents` 도구를 사용할 수 있습니다.

Claude Code 등록 예시:

```bash
claude mcp add --transport http tuna-core http://<코어-IP>:8770/mcp \
  --header "Authorization: Bearer <TOKEN>"
```

새 세션에서 다음 흐름으로 사용합니다.

```text
send_task from_agent=win-opus to_agent=mac-worker text="이 모듈의 오류 경로를 검토해줘"
get_task task_id=<task_id>
```

사람이 보는 대화형 세션에서는 MCP 도구 호출 승인이 필요할 수 있습니다.

## 2.3 진행 상황을 실시간으로 보기

`SendStreamingMessage`를 사용하면 작업 등록부터 완료까지 SSE로 상태를 받을 수 있습니다.

```bash
curl -N -s \
  -H "Authorization: Bearer <TOKEN>" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -X POST http://<코어-IP>:8770/a2a \
  -d '{
    "jsonrpc":"2.0",
    "id":"s1",
    "method":"SendStreamingMessage",
    "params":{
      "message":{
        "messageId":"m1",
        "role":"user",
        "parts":[{"text":"작업 지시"}]
      },
      "fromAgent":"my-dispatcher",
      "toAgent":"win-worker"
    }
  }'
```

일반적으로 다음 순서의 이벤트가 옵니다.

```text
submitted → working → artifact → completed
```

스트림이 끊겨도 작업 자체는 코어에서 계속됩니다. 이후 `GetTask`로 확인하거나 `SubscribeToTask`로 다시 구독할 수 있습니다.

## 2.4 프로젝트별로 워커 나누기

프로젝트별로 작업 디렉터리를 격리하려면 워커를 따로 띄우는 방식이 가장 단순합니다.

```bash
tunaround work --core ... --token ... \
  --agent project-a-worker --project-path /repos/A --runner claude

tunaround work --core ... --token ... \
  --agent project-b-worker --project-path /repos/B --runner codex
```

작업을 보낼 때 `to_agent`를 프로젝트에 맞는 워커로 지정합니다.

워커 하나에서 여러 프로젝트를 처리하려면 `context_id`와 `--context-map`을 사용할 수 있습니다.

```bash
tunaround work --core ... --token ... --agent shared-worker \
  --context-map "projA=/repos/A,projB=/repos/B" \
  --project-path /repos/default \
  --runner claude
```

작업의 `context_id`가 맵에 있으면 해당 디렉터리를 사용하고, 없으면 `--project-path`로 돌아갑니다.

---

# 3. 태그로 워커 찾기

고정된 `to_agent` 문자열을 직접 관리하기 어려우면 워커가 태그를 광고하게 할 수 있습니다.

```bash
tunaround work \
  --core http://<코어-IP>:8770/mcp \
  --token <TOKEN> \
  --tags "machine=win,runner=claude,role=worker,project=tunaround" \
  --runner claude
```

권장 태그는 다음과 같습니다.

| 키 | 예시 | 의미 |
| --- | --- | --- |
| `machine` | `win`, `mac`, `linux` | 워커가 실행 중인 머신 |
| `runner` | `claude`, `codex`, `http` | 실제 실행 러너 |
| `role` | `worker`, `session`, `infra` | 에이전트의 역할 |
| `project` | `tunaround` | 담당 프로젝트 |
| `mode` | `read`, `write` | 허용 작업 범위 |

MCP의 `list_agents`로 현재 온라인 대상을 찾을 수 있습니다.

```text
list_agents
list_agents selector="runner=claude,project=tunaround"
```

작업을 보낼 때 `to_agent` 대신 `to_selector`를 지정할 수도 있습니다.

```text
send_task from_agent=win-opus \
  to_selector="runner=claude,project=tunaround" \
  text="현재 변경분을 검토해줘"
```

매칭 결과는 다음처럼 처리됩니다.

| 매칭 수 | 동작 |
| --- | --- |
| 1개 | 해당 워커로 작업을 보냅니다. |
| 0개 | 작업을 만들지 않고 대상이 없다고 알립니다. |
| 2개 이상 | 후보를 보여주고 사람이 하나를 고르게 합니다. |

자동 부하분산은 하지 않습니다. 대상 선택 권한은 보내는 쪽에 남깁니다.

---

# 4. 라이브 세션을 mesh에 연결하기

헤드리스 워커뿐 아니라 사람이 보고 있는 Claude Code·Codex 세션도 로스터에 올리고 작업 대상으로 사용할 수 있습니다.

## 4.1 presence 스캐너

머신마다 스캐너 하나를 띄우면 로컬의 Claude Code와 Codex 세션을 찾아 코어 로스터에 동기화합니다.

```bash
export TUNA_BROKER_CORE=http://<코어-IP>:8770/mcp
export TUNA_BROKER_TOKEN=<TOKEN>

tunaround presence-scan --machine win
```

스캐너는 세션 존재 여부를 보고할 뿐, 그 세션이 자동으로 작업을 받게 하지는 않습니다. **로스터에 보이는 것과 작업을 수신하는 것은 별개입니다.**

Claude 세션은 Monitor 기반 폴링을 연결해야 하며, MCP가 없는 세션은 `tunaround task poll|claim|get|complete|fail` CLI를 사용할 수 있습니다.

## 4.2 Codex 라이브 세션 수신

Codex 라이브 세션은 자체 수신 루프가 없으므로 머신마다 `codex-relay` 하나가 필요합니다.

```bash
# 1. Codex app-server 실행
TUNA_BROKER_TOKEN=<TOKEN> \
  codex app-server --listen ws://127.0.0.1:<PORT>

# 2. 기존 세션을 app-server thread로 열기
codex resume <threadId> --remote ws://127.0.0.1:<PORT>

# 3. relay 실행
tunaround codex-relay --ws ws://127.0.0.1:<PORT>
```

작업이 Codex 세션 앞으로 오면 relay가 대신 선점해 해당 thread에 주입합니다. Codex는 같은 턴 안에서 작업을 처리하고 `complete_task` 또는 `fail_task`로 결과를 보고합니다.

새 thread가 생기는 것을 피하려면 기존 세션을 반드시 `resume`으로 여세요.

---

# 5. 운영과 장애 진단

## 5.1 작업이 계속 `submitted`에 머무를 때

다음 항목을 확인합니다.

1. `to_agent`가 실제 워커 ID와 같은가
2. 워커 프로세스가 실행 중인가
3. 워커의 `--core` 주소가 `/mcp`로 끝나는가
4. 코어와 워커가 같은 토큰을 쓰는가
5. 태그 셀렉터를 썼다면 온라인 대상이 정확히 하나인가

오랫동안 선점되지 않은 작업에는 `⚠no-consumer?` 표시가 붙습니다. 대개 대상 ID가 틀렸거나 워커가 떠 있지 않은 경우입니다.

## 5.2 작업이 `working`에서 멈출 때

오랫동안 갱신되지 않는 작업에는 `⚠stuck?` 표시가 붙습니다. 워커 프로세스가 죽었거나 러너가 응답하지 않는 경우를 먼저 확인합니다.

현재 구현은 claim lease 만료 후 작업을 다시 `submitted`로 돌릴 수 있으며, 재시도 횟수가 상한을 넘으면 `failed`로 격리합니다. 같은 작업이 반복 실패한다면 자동 재시도보다 원인을 먼저 확인해야 합니다.

## 5.3 어디서 상태를 볼 수 있나

| 도구 | 용도 |
| --- | --- |
| `get_task(task_id)` | 특정 작업 상태와 결과 확인 |
| `tasks()` | 코어 전체의 열린 작업 확인 |
| `poll_tasks(agent)` | 특정 워커 앞으로 온 작업 확인 |
| 웹 대시보드 | 로스터, 작업 피드, 완료·실패 상태 확인 |
| `watch-results` | 자신이 보낸 작업의 완료·실패 알림 수신 |

## 5.4 쓰기 워커의 작업 디렉터리

`--write` 워커는 tunaRound 노드 자체가 실행 중인 클론과 같은 디렉터리에서 작업하지 않는 것이 안전합니다. 워커가 자기 실행 파일이나 실행 중인 작업 트리를 바꾸면 스스로 중단될 수 있습니다.

프로젝트별 별도 클론이나 worktree를 사용하세요.

## 5.5 토큰과 네트워크

- 코어 토큰은 명령줄보다 환경변수나 `~/.tunaround/config`에 둡니다.
- 토큰을 바꾸면 이미 실행 중인 데몬을 재기동해야 합니다.
- 같은 LAN에서는 사설 IP를 사용합니다.
- 외부 네트워크에서는 Tailscale이나 SSH 터널처럼 접근 범위를 제한하는 연결을 사용합니다.
- 코어 포트를 공용 인터넷에 그대로 노출하지 않습니다.

---

# 6. 외부 A2A 에이전트에 위임하기

`tunaround work --runner a2a`를 사용하면 외부 표준 A2A 에이전트를 tunaRound 워커처럼 연결할 수 있습니다.

```bash
tunaround work \
  --core http://<코어-IP>:8770/mcp \
  --token <TOKEN> \
  --agent bridge-worker \
  --runner a2a \
  --a2a-card http://some-agent.example/ \
  --a2a-token <외부-에이전트-토큰>
```

이 경로는 외부 Agent Card를 발견하고 표준 `SendMessage`로 작업을 넘긴 뒤 `GetTask`로 결과를 회수합니다.

## 호환 범위

| 방향 | 상태 |
| --- | --- |
| tunaRound → 외부 표준 A2A 에이전트 | 지원 |
| 외부 표준 A2A 클라이언트 → tunaRound 브로커 | 비목표 |

브로커 내부에는 `fromAgent`, `toAgent` 같은 라우팅 확장이 있고 Agent Card에도 인증 게이트가 있으므로, tunaRound 브로커 자체를 완전한 범용 A2A 서버로 보기는 어렵습니다. 필요할 경우 표준과 브로커 사이에 별도 번역 계층을 두는 방향입니다.

---

# 7. 작업 위임 규칙

A2A 배관이 있다고 해서 작업이 자동으로 잘 나뉘는 것은 아닙니다. 여러 에이전트가 같은 레포에서 일할 때는 다음 규칙을 지키는 편이 안전합니다.

1. 비단순 변경은 브랜치에서 처리합니다.
2. 한 브랜치는 한 에이전트만 편집합니다.
3. 병렬 작업은 worktree나 별도 클론으로 물리적으로 분리합니다.
4. 작업 범위와 완료 조건을 task 본문에 적습니다.
5. 공유 파일과 `main` 머지는 총괄 한 곳에서 관리합니다.
6. 실패한 워커는 이유를 `fail_task`에 남깁니다.
7. 완료 결과에는 변경 내용, 검증 결과, 남은 문제를 포함합니다.

중요한 구분은 **배정과 위임이 다르다**는 점입니다.

- 배정: 실행만 넘기고 사람이 계속 상태를 감시합니다.
- 위임: 실행과 결과 정리까지 넘기고, 사람은 완료·실패 보고만 받습니다.

`tunaRound`가 목표로 하는 기본 운영은 두 번째입니다. 작업을 보낸 사람이 계속 폴링하거나 수동으로 결과를 옮겨야 한다면 mesh의 이점이 줄어듭니다.

---

# 8. mesh 토론

여러 에이전트의 의견을 라운드로 주고받는 설계 토론을 mesh 위에서 돌립니다(v2-56). 브로커가 라운드 오케스트레이션(순차-인지·역할 주입·종합)을 대신하므로, 총괄 세션은 시작만 하고 결과를 인박스로 받습니다.

## 8.1 시작·중단

총괄 세션에서 tuna-broker MCP 도구를 호출합니다.

```
start_discussion(
  topic="...토론 주제...",
  seats=[
    {"agent": "<uuid>", "role": "proposer"},
    {"agent": "<uuid>", "role": "reviewer"},
  ],
  rounds=3,
)
```

- 좌석은 2~6석, 로스터 online 에이전트만. 배열 순서 = 라운드 내 발언 순서(뒤 좌석이 앞 좌석 답을 봅니다).
- **라이브 세션 좌석은 `live: true`를 명시해야 합니다**(그 세션의 컨텍스트를 소모하므로 실수 방지 게이트). 헤드리스 워커 lane 좌석을 기본으로 권장합니다.
- rounds(기본 3, 최대 10) 소진 후 첫 좌석이 synthesizer 역할로 합의/이견/미결을 종합합니다.
- 역할은 proposer/reviewer(critic)/verifier(judge)/synthesizer(lead)만 행동 지시문이 주입되고, `instruction`으로 좌석별 자유 지시를 덧붙일 수 있습니다.
- `stop_discussion(discussion_id)`은 이후 라운드 발행만 중단합니다. 이미 실행 중인 좌석 러너는 끝까지 돌 수 있으나 늦은 완료는 무시됩니다.
- 동시 토론은 1건입니다(MVP).
- `gate=true`(옵트인)면 각 라운드 완료 후 사람 승인까지 대기합니다(§8.5).

## 8.2 결과 받기·읽기

- 라운드 발언은 `from_agent=debate:<id>`인 일반 task로 발행되므로, `tunaround watch-results --core <브로커> --dispatcher debate:<id>`를 Monitor로 띄우면 라운드마다 RESULT 줄이 오고 마지막 완료가 종합입니다.
- 전사는 `debate:<id>` 세션에 영속됩니다: `read_transcript(session_id="debate:<id>")`로 전문을, `search_context`로 과거 토론을 검색합니다.
- 토론은 비동기 작업입니다(좌석당 수 분, 좌석 타임아웃 600초). 던져놓고 다른 일을 하다 인박스로 받는 흐름이 정상입니다.
- 브로커가 재기동되면 진행 중이던 토론은 실패 처리됩니다(열린 라운드 task가 기동 시 "broker restart" 사유의 failed로 전이 = 인박스 통지). 재발의하면 됩니다.
- 알려진 통지 공백 두 가지: ① 첫 RESULT가 오기 전(watch-results 워터마크가 아직 없는 창)에 브로커가 재기동되면 그 failed 통지는 재접속 재생 대상이 아니라 유실될 수 있습니다. ② 라운드 task가 종결된 직후 다음 발행 전의 수 초 창에서 재기동되면 열린 task가 없어 실패 전이 자체가 없습니다. 인박스가 예상보다 오래 조용하면 `read_transcript(session_id="debate:<id>")`로 전사를 직접 확인하세요.

## 8.3 좌석이 받는 프리앰블 규약

토론 라운드 task 본문은 `[토론 규약]` 프리앰블로 시작합니다. 이 발신자 클래스(`debate:<id>`)는 사용자가 start_discussion으로 발의한 것이므로 **총괄발 task와 동일한 자율 수행 대상**입니다(메타 확인 불필요). 라이브 세션 좌석은 발언을 작성해 `complete_task`로 마감하고, 헤드리스 워커 좌석은 출력 전체가 그대로 발언으로 기록됩니다(워커 데몬이 마감).

예외가 하나 있습니다: **게이트 다이제스트**(`gate=true` 토론이 라운드 사이에 인박스로 보내는 `[게이트]` RESULT)는 자율 수행 대상이 아닙니다. 받은 세션은 이를 사용자에게 보고하고 지시를 기다려야 합니다(§8.5 - 승인 주체는 사람).

## 8.4 브로커 없이 수동으로 (운영 레시피)

driver 없이도 총괄 세션이 기존 프리미티브만으로 토론을 재연할 수 있습니다(v2-56 Phase 0에서 실증). 라운드마다 `send_task`로 "역할 지시 + 이전 발언들 + 주제"를 좌석에 보내고 `get_task`로 발언을 회수해 다음 좌석 프롬프트에 넣는 식입니다. 구조는 같지만 총괄 컨텍스트를 소모하므로, 브로커가 있으면 start_discussion을 쓰는 편이 낫습니다.

## 8.5 라운드 간 사람 승인 게이트 (옵트인, 이슈 #131)

`start_discussion(..., gate=true)`로 켭니다. 게이트가 켜지면 각 라운드 완료 시(마지막 라운드 뒤 = 종합 발행 직전 포함) driver가 멈추고 다음이 일어납니다.

1. **다이제스트**가 인박스(RESULT 줄)로 옵니다: 라운드 발언 요약 + 해제 명령 안내.
2. **대기 표식 task**가 열린 채 유지됩니다. 게이트 대기 중 브로커가 재기동되면 이 표식이 "broker restart" 사유의 failed로 마감되어 인박스에 통지됩니다(대기 중이던 토론은 소멸 = 재발의 필요).
3. `continue_discussion(discussion_id, steer?, conclude?)`로 진행합니다.
   - `steer`: 조향 지시. 전사에 `debate/user` 화자·`[사용자 조향 지시]` 프리픽스로 남고 다음 라운드 프롬프트에 주입됩니다.
   - `conclude=true`: 남은 라운드를 건너뛰고 synthesizer 종합으로 직행합니다(사람의 "충분" 판단).
4. 대기 중에도 `stop_discussion`이 즉시 듣습니다.

**승인 주체는 사람입니다.** 다이제스트를 인박스로 받는 세션은 이를 사용자에게 보고하고, 사용자 지시가 있을 때만 continue/stop을 호출해야 합니다(자율 진행 금지 - §8.3 자율 수행 규약의 명시적 예외). 게이트 대기에는 타임아웃이 없고(사람이 수 시간 뒤 복귀해도 됩니다), 대기 중인 토론이 동시 1건 슬롯을 점유하므로 그동안 새 토론 시작은 거부됩니다(거부 문구가 대기 상태와 해제 명령을 안내합니다).

---

# 9. 관련 문서

| 문서 | 내용 |
| --- | --- |
| [온보딩 가이드](onboarding.md) | 설치, 피처, 설정 파일, 토큰 관리 |
| [mesh 아키텍처](mesh-architecture.md) | 코어·세션·워커·스캐너·relay의 관계 |
| [맥·윈도우 운영](dev-mac-windows.md) | 두 머신을 실제로 연결하는 방법 |
| [소스 빌드](../development/source-run.md) | 필요한 피처를 포함해 직접 빌드하는 방법 |

세부 JSON-RPC 스키마와 구현 배경은 `docs/design/`의 A2A 관련 설계 문서에 남겨 두며, 일반 사용자는 이 문서만으로 기본 작업 위임을 시작할 수 있습니다.
