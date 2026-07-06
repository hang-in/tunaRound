# 핸드오프: 4자 감독 mesh 라이브 + 통합 대시보드 (2026-07-06)

> WIN 핸드오프. **라이브 접속값(브로커 IP·토큰·PID·포트·핸들러 경로)은 gitignored `docs/reference/backend-private.md` 하단 "2026-07-06 라이브 상태"를 먼저 읽어라.** 레포 PUBLIC이라 여기엔 평문 금지. 다음 세션 = **1) roster 복구 → 2) 대시보드 T2/T3** 순서(사용자 확정).

## 이 세션에 한 것 (요약)

1. **v2-37 codex 라이브 감독 머지(PR #9)**: `codex app-server --listen ws://` + `tunaround codex-inject`(ws turn/start 주입) = 헤드리스 exec 우회 완전 대체. 감독=라이브 thread, 사람은 `codex --remote`로 관전. 코드리뷰(Opus 3앵글+gemini+CodeRabbit) 반영.
2. **heartbeat 머지(PR #10)**: `poll`에 register/heartbeat + `--tags` → 감독이 watcher 도는 동안 상시 online(TTL 90초 stale 해소). first_pass 리뷰 반영.
3. **4자 감독 무-셔틀 mesh 라이브**: win-codex-sup(codex/win) / mac-claude-sup(claude/mac) / mac-codex-sup(codex/mac) + **win-opus-boss 허브(이 세션류)**. 던지면 자동 수신·처리, 사람 릴레이 0. 허브도 **Monitor 인박스 감시로 자동 수신**(양방향). 3자·4자 A2A 티키타카 실증.
4. **동적 총감독 실증**: 사람 자리가 곧 dispatch 지점(맥으로 옮겨 dispatch→윈 허브 자동수신). 총감독=역할(state가 broker.db 외부화).
5. **codex narrate 수정**: 주입 프롬프트를 "요청 요약+답변 산문 emit 후 complete"로 → --remote·artifact·대시보드에 결론이 산문으로 보임(claude처럼). 핸들러(`target/codex-sup-handle.cmd`) 반영. 맥도 전파.
6. **구현 위임 규율 개정**: ① tunaLlama(kimi-k2.7-code:cloud) → ② A2A codex → ③ Sonnet. 아키텍트=Opus. (CLAUDE.md는 `feat/orchestrator-dashboard` 브랜치 b1f788d에, PR 머지 시 main 반영.)
7. **설계+MVP 착수**: 통합 총감독 대시보드 + 동적 총감독 설계 정본([v2-orchestrator-dashboard-and-dynamic-boss](../design/v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06.md)) + 계획 [v2-38](../plans/v2-38-orchestrator-dashboard.md). **T1 완료**(`/dashboard` 라우트+스켈레톤 HTML, tunaLlama 생성→Opus 리뷰→적용, 라이브 200 검증). 브랜치 `feat/orchestrator-dashboard`(main rebase됨).
8. **hydration ritual 실증**: broker.db 읽기+MCP로 5d26c7a6(mac 판정)·open tasks·roster 확인. **두 갭 empirical 발견**: (a) roster in-memory 소실(브로커 재기동+heartbeat 없던 바이너리), (b) 허브 unacked 메시지 4건이 open-tasks 오염 → **4건 MCP ack로 정리**.

## 다음: 1) roster 복구 → 2) 대시보드 T2/T3

### 1) roster 복구 (먼저)
main에 heartbeat 들어갔다. 지금 win-codex-sup watcher는 옛 바이너리(--tags 없음)라 로스터가 비어 **to_selector 실명** 상태.
- `feat/orchestrator-dashboard`(main rebase됨 = heartbeat+대시보드T1) 체크아웃 → 브로커·watcher 내리고 `cargo build --features "morphology mcp serve worker"` → 브로커 detached 재기동 → **win-codex-sup watcher를 `--tags "machine=win,runner=codex,role=supervised,project=tunaround"` 붙여 재기동**(핸들러=`target/codex-sup-handle.cmd`). heartbeat로 로스터 상시 유지 → `to_selector` 복구.
- **맥**: `git pull origin main`(heartbeat) + 재빌드 후 mac-claude-sup·mac-codex-sup watcher를 `--tags`로 재기동하라고 A2A 요청(mac-claude-sup 앞 to_agent). 그러면 4자 다 상시 online.
- 검증: `to_selector="role=supervised"`로 4자 후보 반환되나(90초 넘겨).

### 2) 대시보드 T2/T3 (그다음, Plan v2-38)
- **T2**: `/dashboard`가 브로커 SSE(기존 TaskEvent 스트림) 구독 → task 상태·artifact 실시간 렌더 + roster 표시. **T3**: goal 폼 → 기존 SendMessage. **T4**: claude 감독 post_turn emit(피드 합류). 구현 위임 **1순위 tunaLlama**(kimi), Opus 리뷰·검증. `feat/orchestrator-dashboard`에서 이어감 → PR.
- 설계 §1.3 MVP 3요소(roster/task피드/goal폼), §5 열린질문(SSE 스키마·라우팅UX·post_turn 트리거·auth).

## 첫 행동

1. `docs/reference/backend-private.md` "2026-07-06 라이브 상태" 읽어 브로커/토큰/포트/watcher PID/핸들러 확보. **⚠ 이 세션 종료로 죽는 것**: 허브 Monitor 인박스(bcjw13rcw)·session-bound watcher류. detached(브로커·app-server·win watcher)는 생존 가능하나 재부팅엔 죽음.
2. **1) roster 복구부터**(위). 그다음 2) 대시보드 T2.
3. 규율: 구현 위임 ①tunaLlama ②A2A codex ③Sonnet, Opus 리뷰·검증. GitHub Flow + 3-OS CI + 봇 리뷰 반영 후 머지. cargo는 Bash 툴. 레포 PUBLIC=평문 토큰/LAN IP 금지(A2A 메시지·backend-private만). 굵직한 결정 재론 금지(설계 정본 따름).

## 진행 중 브랜치/PR
- main `d7deae3` = #9(codex 감독) + #10(heartbeat) 머지됨.
- `feat/orchestrator-dashboard`(force-push, main rebase) = 대시보드 T1 + 위임규율 + 설계·계획. T2~ 이어서 → PR.
