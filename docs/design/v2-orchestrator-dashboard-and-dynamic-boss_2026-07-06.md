# 설계: 통합 총감독 대시보드 + 동적 총감독 (2026-07-06)

> 정본. 4자 감독 mesh(win-codex-sup/mac-claude-sup/mac-codex-sup + win-opus-boss 허브) 라이브 상태에서, 3자 A2A 티키타카(2026-07-06)로 도출한 설계를 종합한다. 구현 위임은 이번 실험에서 codex 감독(A2A)로 시도(평시 규율은 Sonnet 위임). 계획 = [v2-38](../plans/v2-38-orchestrator-dashboard.md).

## 0. 배경

세션에서 4자 감독 무-셔틀 mesh를 라이브로 세웠다(v2-37 codex 라이브 감독 + heartbeat PR #10 + Monitor 인박스 허브 자동수신). 그 위에서 두 요구가 드러났다.
- **가시성**: claude 감독은 세션 TUI라 과정이 보이는데, codex 감독은 headless app-server라 `codex --remote` 붙여야만 보인다(창이 감독마다 따로). 사람이 4자 전부의 과정을 **한 곳에서** 봐야 한다.
- **동적 총감독**: 사람이 있는 자리가 그때그때 dispatch 지점이 되는 게 실증됐다(맥으로 옮겨 dispatch → 윈 허브 자동수신). 총감독은 고정 자리가 아니라 역할이다.

## 1. 통합 총감독 대시보드 (PRIMARY, MVP 대상)

### 1.1 위치 = 브로커가 서빙하는 로컬 웹
- TUI 아님. 브로커는 이미 roster·task·**SSE 이벤트버스(v2 스트리밍 Phase2)**를 쥔 유일 프로세스라, 또 하나의 세션-바운드 TUI 창보다 **머신 독립 + 다중 뷰어 + 원격 브라우저 공유**가 되는 웹이 맞다.
- `tunaround serve`가 `/dashboard`를 **read-only HTML + SSE**로 노출(별 프로세스·설정 중복 없이 브로커 수명주기·연결 공유).

### 1.2 통합 원리 = task-level 공통 + 러너별 상세 오버레이
- **공통 레이어(러너 무관)**: task 상태 전이(submitted→working→artifact→completed)는 이미 SSE로 broadcast 중. 이게 통합 피드의 뼈대.
- **claude 감독 과정 = push(자발 emit), not pull(scrape)**: claude 하네스 내부를 긁지 말고, claude가 `post_turn`(이미 존재, Stage 3d)으로 자기 턴을 브로커에 emit. "claude 내부 가시성은 원리적으로 push다."
- **codex 감독 과정 = ws 자동 스트림**: codex app-server가 흘리는 `item/agentMessage/delta`·`item/commandExecution` 등을 브로커(또는 대시보드 백엔드)가 구독. 단 기본 화면은 구조화 lifecycle로 정규화하고, 원시 델타는 **선택한 감독 상세 패널의 보너스 오버레이**로만.
- **codex narrate 기본(적용됨)**: codex 주입 프롬프트를 "요청 요약 + 답변·결론 산문 emit 후 complete"로 바꿔, 결론이 tool 인자에 묻히지 않고 --remote·artifact·대시보드에 산문으로 뜨게 했다. codex 감독 레시피 기본값으로 굳힌다.

### 1.3 MVP 한 컷 (3요소)
`tunaround serve`가 `/dashboard` 서빙 = 브로커 SSE 구독하는 read-only HTML.
1. **roster**: 4자 감독 online/stale(heartbeat 기반).
2. **라이브 task 피드**: agent별 submitted→working→artifact→completed, artifact(결론) 표시.
3. **goal 제출 폼**: 사람이 목표를 던지는 단일 지점(to_agent 또는 to_selector).

제외(후속): 전체 TUI 미러링, 터미널 원격조작, claude 실시간 델타 오버레이, codex 원시 델타 패널.

### 1.4 왜 net-new가 적나
브로커에 **SSE 이벤트버스 + task 이벤트 + roster + heartbeat가 이미 있다**(스트리밍/레지스트리에서 구축). MVP에 필요한 신규 = `/dashboard` 라우트 + 정적 HTML(SSE 구독 JS) + goal 폼 처리(기존 SendMessage 재사용) + claude 감독의 post_turn emit 배선. 데이터 평면은 재사용.

## 2. 동적 총감독 (SECONDARY, 규약+얇은 배선)

- **총감독 = 역할, 자리 아님**: state(roster·task·transcript)가 broker.db에 외부화돼 있으니, 4자 중 누구든 **hydration ritual**로 현재 그림을 확보해 오케스트레이터가 된다. 사람이 있는 곳이 곧 dispatch 지점(no-shuttle 본질).
- **hydration ritual 최소셋**: `list_agents`(online) + open tasks(진행중) + recent artifacts(최근 결론).
- **동시 총감독 충돌 = 과대공포**: 브로커가 task 생성을 **직렬화**하므로 부패는 없다. 남는 건 "모순 지시" 혼란뿐 → **하드락 불필요**. 소프트 advisory 오케스트레이터-lease(누가 운전 중인지 표시)+대시보드 가시화면 충분. 이중 dispatch는 두 task로 보일 뿐 안 깨진다.
- **형태(tunaround)**: `tunaround hydrate`(또는 `boss`) 서브커맨드 = hydration ritual 실행 + 현재 그림 출력 + "hydrate하면 총감독" 프롬프트 규약. 소프트 lease는 브로커 필드 하나(가시성용).
- **드러난 갭(후속 외부화 필요)**:
  - roster가 인메모리라 브로커 재기동 시 소실(heartbeat 자가치유되나 즉시성 없음).
  - 크로스-mesh 오케스트레이션 로그가 1급이 아님(dispatch/결정 시퀀스가 개별 task 메시지에 흩어짐). tasks 테이블을 오케스트레이션 로그로 승격 또는 broker 이벤트로그 추가.
  - **허브가 task 큐를 "메시지 채널"로 쓰며 ack(claim/complete) 안 해서 win-opus-boss 앞 read-but-submitted task가 쌓임**. hydration의 open-tasks를 오염. → 허브가 complete로 ack하거나, 메시지 채널을 task 큐와 분리.

## 3. 감독 durability (관련, 대시보드/동적총감독의 전제)

- 감독 watcher/app-server를 **OS 서비스 매니저에 위임**: launchd(mac, `RunAtLoad`+`KeepAlive`)·Windows Service(win, SCM 복구). tunaRound가 프로세스 관리를 떠안지 말고 OS에(설계 원칙).
- startup 훅 = `register_agent` + Monitor/watcher re-arm + context rehydrate(read_transcript/핸드오프).
- **브로커는 heartbeat/lease 만료를 감지·표시·경보만**, 원격 머신 프로세스를 직접 재기동하지 않는다(장애·보안 경계). supervisor-of-supervisors는 OS 서비스와 겹쳐 불필요(부트스트랩만 한 층 위로).
- **runner 분업(범주 구분, 부분 수정됨)**: codex app-server=라이브 thread를 재기동 너머 유지=진짜 연속. claude=매 재기동 fresh 세션=**"재구성된 연속성"**(맥락이 in-session 아니라 브로커/핸드오프에 externalize돼 있을 때만 성립, read_transcript로 이미 가능). durable해야 할 것은 Claude 프로세스가 아니라 **브로커의 inbox·lease·checkpoint**다.

## 4. 구현 범위 (MVP = 대시보드)

- **T1** `tunaround serve`에 `/dashboard` GET 라우트 + 정적 read-only HTML(인라인 JS로 SSE 구독). 인증은 기존 토큰 스킴 따름(로컬).
- **T2** 대시보드 데이터: 기존 SSE 이벤트버스 구독(task 상태·artifact) + roster 조회(list_agents/heartbeat). 신규 이벤트 타입 최소.
- **T3** goal 제출 폼 → 기존 `SendMessage`(to_agent/to_selector)로 라우팅.
- **T4** claude 감독 post_turn emit 배선(claude 감독이 자기 턴을 브로커에 emit → 대시보드 피드 합류). 최소 형태.
- **비범위(후속 문서/PR)**: 동적 총감독 `tunaround hydrate` 서브커맨드·소프트 lease, durability OS 서비스화, codex 원시 델타 패널.

## 5. 열린 질문

- `/dashboard`가 구독할 SSE 이벤트 스키마(기존 TaskEvent로 충분한가, roster 변경 이벤트 필요한가).
- goal 폼의 라우팅 UX(to_agent 드롭다운 vs to_selector 태그 입력).
- claude 감독 post_turn emit의 트리거(매 턴 자동 vs 감독이 명시 호출) + 대시보드 표시 입도.
- 인증/노출(로컬 바인드 read-only라 저위험이나 goal 폼은 write이므로 토큰 게이트).

## 6. 검증 계획

1. `tunaround serve --dashboard`(또는 기본 serve) 기동 → 브라우저 `/dashboard`에서 4자 roster + 라이브 task 피드 렌더.
2. goal 폼으로 목표 제출 → task 생성 → 감독 자동 수신·처리 → 피드에 상태 전이·artifact 실시간 반영.
3. claude 감독 턴이 post_turn으로 피드에 뜨는지.
4. 3-OS CI(웹은 정적+SSE라 headless 테스트는 라우트 응답·SSE 프레임 단위테스트 위주).
