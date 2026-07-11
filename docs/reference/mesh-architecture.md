# tunaRound 멀티 에이전트 mesh 개요

> 브로커(작업 큐 + 공유 전사 + A2A 서버) 위에서 맥·윈도우의 터미널 에이전트가 서로 task를 위임하고 자동 수신하는 구조의 개요입니다. 명령어·플래그 상세는 여기서 중복하지 않고 [a2a-usage](a2a-usage.md)로 링크합니다.
>
> 설계 배경: [브로커 거버넌스](../design/v2-broker-governance_2026-07-03.md) · [presence 스캐너 v2-44](../design/v2-44-presence-scanner-and-roles_2026-07-11.md) · [mesh 영속·재생 v2-45](../design/v2-45-mesh-persistence-and-replay_2026-07-11.md) · [파트너 위임](../design/v2-a2a-partner-delegation_2026-07-02.md) · [워커 데몬](../design/v2-a2a-worker-daemon_2026-07-03.md) · [codex-relay v2-46](../design/v2-46-codex-relay_2026-07-11.md)

---

## 1. mesh란

mesh는 **브로커 하나 + 터미널 에이전트 세션들 + 워커 노드들**이 맥·윈도우 여러 머신에 걸쳐 함께 도는 구조입니다. 브로커는 작업 큐(SQLite `tasks`)와 공유 전사, 그리고 A2A 서버(`/a2a`, `/.well-known/agent-card.json`, `/mcp`)를 노출하는 상주 프로세스입니다. 각 머신의 에이전트는 자기 앞으로 온 task를 스스로 발견해 처리합니다.

핵심 목적은 **no-shuttle**입니다. 사람이 머신 사이를 왔다갔다 하지 않고, 총괄 자리(TUI 한 곳)에서 목표(task)를 던지면 다른 머신의 에이전트가 자동으로 받아 처리하고 결과를 총괄에게 되돌립니다. 사람은 감시 루프를 한 번 세팅한 뒤 자리를 떠도 되고, 매 task를 손으로 나르는 것은 후퇴로 봅니다.

tunaRound는 팔 제품이 아니라 개인 악기입니다. 그래서 mesh도 규모 확장이나 완전 자율화가 아니라 개인이 2~3대의 머신을 얇게 잇는 수준으로만 설계되어 있습니다.

## 2. 구성 요소

각 요소는 한 프로세스이며, 대부분 Rust 데몬이라 유휴 시 토큰을 쓰지 않습니다.

- **브로커 (`serve`)**: 작업 큐 + 공유 전사 + A2A 서버를 노출하는 상주 프로세스. task 상태 전이의 권위는 브로커 하나뿐입니다.
- **세션 (라이브 TUI)**: 사람이 실제로 보는 claude·codex 대화 창. 총괄 자리이자 위임 대상 후보(관리자로 쓸 수 있는 자리)입니다.
- **워커 (`work`)**: 헤드리스 자율 데몬. `poll → claim → 러너 실행 → complete`를 사람 개입 없이 돌립니다(러너=claude/codex/opencode/http/a2a).
- **presence 스캐너 (`presence-scan`)**: 머신당 1개. 로컬 라이브 세션(claude jsonl + codex rollout)을 스캔해 브로커 로스터에 전집합을 일괄 동기화합니다(`report_presence`).
- **codex-relay (`codex-relay`)**: 머신당 1개. codex는 스스로 수신하는 메커니즘이 없어, 세션 앞 task를 대리 claim해 그 라이브 codex thread에 주입합니다.
- **watch-results (`watch-results`)**: 총괄의 결과 인박스. 자기가 던진 task의 완료/실패를 브로커 SSE로 받아 한 줄로 알립니다(던지고 자리를 떠도 결과가 깨웁니다).

수동 조작이 필요할 때는 `tunaround task poll|claim|get|complete|fail` CLI가 raw HTTP 없이 쓰는 저비용 경로입니다. 두 머신 환경 준비는 [dev-mac-windows](dev-mac-windows.md)를 참고하세요.

## 3. 역할 (총괄 · 관리자 · 실무자)

여러 Opus·에이전트가 **한 레포를 공유하며 동시에 일할 때** 충돌·중복을 막기 위한 협업 관례입니다(강제 메커니즘이 아니라 행동 규약). 상세는 [CLAUDE.md](../../CLAUDE.md)의 "총괄/관리자/실무자 협업 위계"에 있습니다.

- **총괄 (integrator)**: 사람이 앉은 통합자 자리. 방향을 정하고 `main` 머지를 독점하며 공유 파일(CLAUDE.md·README·Cargo.toml 등)을 소유합니다. 산출물을 PR로 받아 CI green 후 머지합니다.
- **관리자 (supervisor)**: 라이브 TUI 감독 세션. 진단·리뷰·repro·스펙 산출이 주 역할이고, 실제 코드 변경은 직접 하지 않고 총괄이나 실무자 worktree로 넘깁니다(코드가 여러 머신에 흩어지는 것 방지).
- **실무자 (worker)**: 헤드리스 워커. 1 task를 1 브랜치/worktree에서 처리하고, 무관한 파일은 손대지 않으며, 끝나면 `complete_task`(실패 시 `fail_task`)로 보고합니다.

핵심 규칙 다섯 가지입니다.

1. **비trivial 변경은 브랜치.** `main` 직접 편집 금지, `main`은 총괄 머지 전용(CI green 전제).
2. **한 브랜치 한 에이전트.** 병렬 워커는 worktree로 물리 격리합니다(같은 브랜치 동시 편집 금지).
3. **작업 선점 = A2A task.** 총괄이 배정하면 claim으로 "누가 뭐 하는지"가 직렬화되어 중복 작업이 사라집니다.
4. **미커밋을 들고 있지 않기.** 코드 변경은 곧 브랜치 커밋. 스테일 브랜치는 정리합니다.
5. **동기화 위생.** 세션 시작·push 전 `git pull --rebase`, 작은 커밋 자주, 공유 파일은 총괄 경유.

## 4. A2A task 수명주기

task는 A2A 상태 머신을 따릅니다.

```text
submitted ──claim──▶ working ──complete──▶ completed   (종료)
                        │      ──fail─────▶ failed      (종료)
                        │      ──cancel───▶ canceled    (종료)
                        └── input_required (추가 입력 대기, 열린 상태)
```

- **열린 상태**: `submitted` · `working` · `input_required` (dispatcher가 아직 결과를 기다림).
- **종료 상태**: `completed` · `failed` · `canceled`.

기본 흐름은 **dispatch → claim → run → complete**입니다. 던지는 쪽(dispatcher)이 `SendMessage`(또는 SSE `SendStreamingMessage`)로 task를 만들고, 워커나 세션이 `poll → claim`으로 선점해 러너로 실행한 뒤 `complete_task`로 결과 artifact를 붙입니다. task는 받는 에이전트 id(`to_agent`)로 라우팅되며, 태그 셀렉터(`to_selector`)로 발송 시점에 대상을 발견할 수도 있습니다.

**워커 사망 시 lease 기반 requeue**가 있습니다. claim 시 30분 lease가 걸리고, 워커가 죽어 lease가 만료되면 poll 경로의 지연 sweep(`expire_stale_claims`)이 그 task를 `submitted`로 되돌립니다. 재시도 횟수(attempt)에 상한이 있어 무한 requeue 대신 상한 초과분은 `failed`로 격리합니다.

**미배달·고착 신호**는 상태를 바꾸지 않고 표시로만 알립니다. 오래 claim 안 된 `submitted`에는 `⚠no-consumer?`(폴러 없음), 오래 갱신 없는 `working`에는 `⚠stuck?`가 붙습니다. 사람이 보고 취소하거나 다시 던집니다.

자율 수준은 **semi-A2A (HITL)** 입니다. 사람은 목표(task)를 발행할 뿐이고, 발견·실행·완료·통지는 기계끼리 처리합니다. 사람 없이 무한히 도는 자율 루프는 두지 않았습니다.

## 5. 영속과 재생 (v2-45)

브로커가 재기동하거나 뷰를 리로드해도 그림이 유지되도록, mesh의 상태는 SQLite에 영속됩니다.

- **task 장부 영속**: task와 전사가 SQLite에 남습니다. 재생의 단일 진실은 `tasks` 테이블입니다(휘발성 이벤트 버스가 아님).
- **피드·인박스 재생**: 대시보드 피드는 `/dashboard/events` SSE에 선행 스냅샷 프레임을 실어 리로드 후에도 최근 task를 다시 그립니다. `watch-results`는 끊기면 재접속하고, 워터마크 이후의 완료/실패를 재생받아 다운 중 놓친 결과도 뒤늦게 도달합니다.
- **mesh 기억화**: 종결된 task의 요청문과 결과가 `a2a:<task_id>` 네임스페이스로 색인되어, 과거 위임 이력을 `search_context`로 검색할 수 있습니다(전용 네임스페이스라 일반 전사 색인과 섞이지 않습니다). 오래된 종결 task는 artifact와 실패 사유는 남긴 채 본문만 슬림화됩니다.

## 6. A2A 표준 호환 (정직하게)

tunaRound의 A2A는 A2A 프로토콜의 **구조를 차용**해 tunaRound 인스턴스끼리 위임하는 것이 목적입니다. 방향에 따라 호환 수준이 다릅니다. **Outbound**(우리가 표준 A2A로 외부 에이전트에 던지기)는 진짜 표준입니다. `--runner a2a`가 외부 agent-card를 발견해 표준 `SendMessage`로 위임하고 `GetTask`로 결과를 회수하며, 독립 표준 서버 상대 왕복을 실증했습니다. 반면 **inbound**(제3자 표준 클라이언트가 우리에게 던지기)는 비목표입니다. 브로커가 라우팅 확장 필드(`fromAgent`/`toAgent`)를 요구하고 Agent Card가 인증 게이트를 두기 때문입니다. 필요해지면 표준과 브로커 사이 번역 어댑터를 별도로 둡니다.

명령어와 전체 흐름(코어 기동, 워커·relay·스캐너 세팅, dispatch·SSE·셀렉터 라우팅, 위임 행동 규약)은 [a2a-usage](a2a-usage.md)에 있습니다. 주소·토큰·호스트는 이 문서 어디서도 실값을 쓰지 않으며(`<코어-IP>`·`<토큰>`·`127.0.0.1`·`0.0.0.0` 플레이스홀더), mesh는 같은 LAN이면 사설 IP로, 아니면 터널로 잇습니다.
