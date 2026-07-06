# 설계: 유니버설 세션 버스 (임의 세션 → A2A 주소화·발견·제어) (2026-07-06)

> 정본. 지금은 "무장한(register+watcher) 감독"만 A2A 대상이다. 목표 = **시스템 전역(LAN 포함)의 실행 중인 Claude Code/codex 세션을 발견해 후보로 올리고, 총감독 대시보드에서 제어**. 예: 튜나라운드 세션에서 secall 세션에 A2A task를 던진다. 관련 [[core-purpose-no-shuttle]] · [[partner-orchestration-vision]]. 대시보드 UX는 [총감독 대시보드 UI 목업]에 "발견된 세션" 패널을 더한다.

## 0. 배경 / 요구

- 사용자 요구(2026-07-06): "시스템 전역에서 실행중인(LAN 감독 머신의 터미널 TUI 포함) 세션들을 찾아 후보로 올리고 대시보드에서 컨트롤". 구체 예: **지금 세션(tunaRound)에서 secall 세션에 A2A로 요청/지시**.
- 현재: 브로커 로스터 = `register_agent` + Monitor-watched `poll` 워처 + 브로커 MCP를 **수동 무장**한 감독만. 무장 안 한 세션은 발견도 제어도 안 됨.

## 1. 두 개의 서로 다른 난이도: 발견 vs 제어

이 둘을 분리해야 설계가 정직해진다.

### 1.1 발견 (discovery)
- **로컬(브로커 호스트)**: 프로세스/전사로 열거 가능. 실측(2026-07-06): `~/.claude/projects/<hash>/*.jsonl` 최근 mtime = 활동 세션, `codex.exe` 프로세스 = codex 세션. 이 머신의 seCall Claude Code 세션도 이렇게 발견됨.
- **원격 LAN**: 브로커는 **등록된 것만** 안다. 각 LAN 머신이 "발견 리포터"를 돌려야 미등록 ambient 세션도 집계된다. 원격 프로세스 직접 열거는 불가(채널이 브로커뿐).

### 1.2 제어 (control) - 러너별로 갈린다
- **claude 세션**: **외부 제어 소켓 없음.** 스스로 Monitor-watched poll 워처를 켜야(opt-in) 자동 수신. 발견돼도 무장 전엔 강제 제어 불가.
- **codex 세션**: `app-server --listen ws://`면 **turn/start로 외부 주입 가능**(v2-37에서 실증). 발견 즉시 구동 여지 큼.

> 결론: "발견=후보 올리기"는 되지만, "제어=대상이 채널(claude=워처 / codex=ws)을 열어둬야" 한다. 발견이 곧 제어권을 주지 않는다.

## 2. 아키텍처 (컴포넌트 4개)

### 2.1 자동 무장 (SessionStart 훅) - 제어의 열쇠
- Claude Code **SessionStart 훅**이 세션 시작 시 자동으로: (a) `register_agent`(uuid=세션 id, tags=project/machine/runner/user/host), (b) Monitor-watched `poll` 워처 기동(그 세션 하네스가 task 도착 시 깨어남), (c) 브로커 MCP 접근 확인.
- 이러면 **훅을 켠 머신의 모든 Claude Code 세션이 자동으로 A2A 주소를 얻는다.** LAN 각 머신이 같은 훅(공유 브로커 지정)을 돌리면 로스터 = 전 머신 라이브 세션.
- codex는 훅이 없으니 `app-server --listen` + `poll --on-task codex-inject` 레시피(v2-37)를 세션 시작 스크립트로.

### 2.2 발견 리포터 (per-machine) - 미무장 세션 가시화
- 무장 안 한(또는 훅 미적용) 세션도 후보로 보이게, 각 머신이 `tunaround discover`(또는 기존 `node` 확장) 데몬으로 로컬 세션을 열거해 브로커에 **candidate**로 보고(uuid·project·runner·age·armed=false).
- 브로커가 candidate를 집계 → 대시보드 "발견된 세션(후보)" 패널. armed=false는 **제어 불가, "연결/arm" 액션만** 노출.

### 2.3 제어 채널 (러너별)
- claude candidate → "arm" = 그 세션에 무장 명령 주입 필요하나 외부 소켓이 없어 **사람이 그 세션에서 실행**(대시보드가 복붙 명령/프롬프트 생성) 또는 2.1 훅이 이미 처리.
- codex candidate(app-server 있음) → 대시보드에서 직접 turn/start 주입 경로 연결.
- armed agent(로스터) → 기존 `send_task`로 즉시 제어.

### 2.4 대시보드 UX (목업 확장)
- 로스터(armed, 제어 가능) + **신규 "발견된 세션" 패널**(candidate, armed=false, "연결" 액션).
- 목표 제출 대상 체크박스에 armed agent만(candidate는 arm 후 편입).
- candidate가 arm되면 로스터로 승격(라이브 애니메이션).

## 3. 안전 / 스코핑 (필수)

- **자동 무장은 opt-in.** 모든 세션을 조용히 브로커에 붙이면 사고. `TUNA_AUTOARM=1`(env) 또는 프로젝트 설정 + `available` 태그로 명시 동의한 세션만.
- **바쁜 세션 보호.** Monitor wake가 진행 중 턴을 끊으면 방해 → task는 **큐잉·비파괴 주입**(현재 대화 경계에서 픽업), `busy` 상태면 후보에서 제외 또는 대기.
- **write 게이트.** 목표 제출·arm은 토큰(또는 loopback-trust, 별도 결정). read(발견/피드)는 무인증 로컬.
- **토큰/사설 노출 금지.** candidate 보고에 토큰·LAN IP 평문 금지(브로커 로컬 상태만).
- **범위 격리.** project 태그로 프로젝트별 대상 격리(엉뚱한 프로젝트 세션에 오발 방지).

## 4. 단계 계획

- **S0 (지금도 됨, 코드 0)**: 대상 세션을 수동 무장(register + 브로커 MCP + poll 워처)하면 `send_task`로 세션↔세션 태스킹 성립. secall 세션으로 라이브 데모 가능(별도).
- **S1 자동 무장 훅**: SessionStart 훅 스크립트(opt-in env) - register + poll 워처 + 정리(SessionEnd). 한 머신 실증 → LAN 복제.
- **S2 발견 리포터**: `tunaround discover`(로컬 세션 열거 → 브로커 candidate 보고). 브로커 candidate 저장·조회 MCP/HTTP.
- **S3 대시보드 "발견된 세션" 패널** + arm 액션(목업 확장).
- **S4 codex 직접 제어**(app-server candidate turn/start 배선) + 안전 스코핑(busy/available/consent).
- **S5 검증**: tunaRound→secall 세션 왕복 + 크로스머신 발견/arm 라이브.

## 5. 열린 질문

- 자동 무장 훅의 정리(세션 종료 시 dereg)와 crash 잔재(stale roster) 처리 - heartbeat TTL이 자연 소멸시키나 즉시성?
- candidate 저장 위치(브로커 인메모리 vs 영속) + 원격 리포터 인증.
- "arm" UX: claude는 사람 개입 불가피(외부 소켓 없음) - 훅 전제로 갈지, 복붙 명령으로 갈지.
- busy 판정(세션이 턴 처리 중인지)을 브로커가 어떻게 아는가(하네스 신호 필요).
- 대시보드 목업의 "발견된 세션" 패널 위치(로스터 하단 vs 별 탭).

## 6. 비범위 (이 설계 밖)

- 임의 세션의 화면 미러링/원격 터미널 조작(관전은 codex --remote·SSE 피드로 충분).
- claude 하네스에 외부 제어 소켓 추가(업스트림 변경, 통제 불가).
