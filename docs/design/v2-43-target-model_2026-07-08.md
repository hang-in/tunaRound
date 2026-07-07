// tunaRound A2A 콕핏의 정본 타겟 모델(단순화). 이후 세션은 이걸 기준으로 배선만.
# v2-43 정본 타겟 모델 (단순)

> 2026-07-08 세션17. v2-40~42를 라이브 도그푸딩하며 대시보드 정책이 왔다갔다 했고, 사용자와 재센터링해 **가장 단순한 정본**으로 확정. 관련 문서로 [v2-42](v2-42-heartbeat-presence-boss_2026-07-08.md) · [watch-results PR #19] · [semi-a2a 파트너 위임](v2-a2a-partner-delegation_2026-07-02.md)을 참고한다.

## 0. 핵심 문장

**사람은 총괄(아키텍트) 한 세션에 앉아 던지고 결과를 브리핑받는다. 나머지 세션은 각자 heartbeat 달고, 던지면 자율로 받아 처리해 총괄에 보고한다.**

## 1. A2A 워크플로우 (이미 다 만든 프리미티브)

```text
사람 ⟷ 총괄(아키텍트 세션)
        │  ① send_task / goal 폼으로 던짐(개별·크로스프로젝트)
        ▼
   감독/워커 세션 ── ② Monitor(poll)로 자율 수신·처리 → complete_task
        │
        └─ ③ watch-results(총괄 인박스 SSE)가 결과로 총괄을 깨움(PR #19 "책임의 이전")
        ▼
   총괄 ── ④ 사람에게 결과 브리핑 + 다음 논의(아키텍트)
```

- ① `send_task`(MCP) / `/dashboard/goal`(대시보드 폼)
- ② `Monitor(poll --agent <id>)` = 0토큰 대기(파킹), task 오면 깨어나 claim/처리. claude 감독 패턴(실증). codex는 app-server+`codex-inject`. 헤드리스는 `work` 데몬.
- ③ `watch-results --dispatcher <총괄>` = 던진 task의 완료/실패를 SSE로 받아 총괄 세션을 깨움.
- ④ 총괄은 아키텍트로 사람과 대화.

**받는 자리(감독/워커)는 자율이라 사람이 그 UX를 안 본다** → Monitor "바쁨" UX는 논점 아님. 사람은 총괄에만 앉음.

## 2. 대시보드 = mesh의 뷰(콕핏), 메커니즘 아님

**순수 heartbeat=presence:**
- **관리자 로스터 = online(heartbeat) 세션 전부.** heartbeat 있음 → 뜸. 닫힘(exit) → heartbeat 끊김 → offline → 사라짐(TTL 딜레이 OK).
- **총감독 ★ = human_input_at 최신 online 세션**(사람이 지금 입력하는 세션). 자동, 수동토글 없음.
- **정렬**: 총괄 → 현재 사용 머신 → 원격.
- **발견/유휴 섹션·discover·활동(jsonl age) 모델 = 제거**(전부 autoarm heartbeat라 불필요). v2-40 discovery / v2-41 활동모델은 이 정본에서 비채택.
- **워커(헤드리스)**: 별 섹션(작업 중인 것만). 후속.

## 3. presence 배선

- **claude 세션**: autoarm 훅(SessionStart) + session-ping 훅(UserPromptSubmit) → detached `poll`(register+heartbeat). env 필요(TUNA_AUTOARM=1·TUNA_BIN·TOKEN·CORE·MACHINE).
- **codex 세션**: claude 훅이 안 잡음. codex용 poll 등록 별도 배선 필요(win-codex-sup처럼).
- **닫힘**: SessionEnd 훅(disarm)이 poll 종료 → heartbeat 끊김 → offline → 로스터에서 사라짐.

## 4. 수용 기준 (사용자, 2026-07-08)

- 작업(전 세션 재시작) 후 **발견/유휴 없음**.
- **관리자 로스터 = win·mac 모든 머신의 열린 TUI 세션 전부**(claude + codex-server). 없으면 재시작(exit→다시)하면 올라옴.
- **exit → 로스터에서 사라짐**(딜레이 OK).

## 5. 남은 배선 (새 설계 아님, 기존 연결)

1. **env → 설정파일**(최우선): 훅이 env 대신 `~/.tunaround/config` 읽기 → 터미널 launch 신선도 무관(env 두 번 물린 근본). #3 신뢰성.
2. **수신 배선**: autoarm 훅 additionalContext에 "받으려면 `Monitor(poll --agent <id>)`" 안내 → 감독 패턴 일반화. 총괄은 watch-results 운용.
3. **codex arming**: codex-server 세션도 poll 등록되게(claude autoarm과 별개).
4. **워커 섹션**: role=worker 헤드리스만 별 섹션에 "작업 중" 표시.

## 6. 비범위

- 크로스머신 boss-ping 토큰 인증(현재 loopback = win boss만). 사용자 win 구동이라 급하지 않음.
- 모든 세션 강제 Monitor(사람 앉은 총괄은 clean chat, watch-results로 결과 수신).
