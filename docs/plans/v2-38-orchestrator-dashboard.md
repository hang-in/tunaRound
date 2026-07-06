# Plan v2-38: 통합 총감독 대시보드 MVP

> 설계 정본 = [v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06](../design/v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06.md). MVP = `tunaround serve`의 `/dashboard`. 이번 구현은 codex 감독(A2A 위임) 실험(평시 규율=Sonnet). Opus 스펙·리뷰·검증. GitHub Flow + 3-OS CI.

## 목표

`tunaround serve`가 read-only 웹 `/dashboard`를 서빙: 4자 감독 roster + 라이브 task 피드 + goal 제출 폼. 브로커의 기존 SSE 이벤트버스·roster·task 상태를 재사용(net-new 최소).

## Tasks

- [x] T1: `/dashboard` GET 라우트 + 정적 read-only HTML 스켈레톤(roster/task/goal 섹션 placeholder). **tunaLlama(kimi) 생성 → Opus 리뷰(bearer Ok-래핑 버그 걸러내고 기존 미들웨어 유지) → 적용.** auth 우회 배선(outer router에 merge)으로 브라우저 로드 가능. 라이브: GET /dashboard=200+HTML, POST /mcp=401(게이트 유지). clippy 클린.
- [ ] T2: SSE 배선 - 대시보드 JS가 브로커 SSE(기존 TaskEvent 스트림) 구독 → task 상태 전이·artifact를 피드에 실시간 렌더. roster는 list_agents/heartbeat 조회(주기 폴 또는 이벤트).
- [ ] T3: goal 제출 폼 → 기존 `SendMessage`(to_agent 선택 또는 to_selector) 호출로 task 생성.
- [ ] T4: claude 감독 post_turn emit 배선(최소) - claude 감독 턴이 브로커 피드에 합류. (범위 크면 별 PR로 분리 가능.)
- [ ] T5: 검증 - serve 기동 후 /dashboard 렌더 + goal 폼→감독 자동처리→피드 반영 라이브 스모크. 라우트/SSE 프레임 단위테스트. 3-OS CI green.

## 위임 규약 (codex 감독 A2A 실험)

- **워크트리 격리**: win-codex-sup·mac-codex-sup가 같은 repo 작업 시 충돌 방지. 각자 브랜치 + (필요시)worktree. 이 세션(win-opus-boss)이 쓰는 working copy와 겹치지 않게.
- **스코프 분할**: 한 감독에 한 task(예: T1+T2 웹/SSE = 한 감독, T3 goal폼 = 다른 감독). 결과는 브랜치 push + PR로 통합, Opus 리뷰.
- narrate 프롬프트라 과정이 --remote·대시보드에 보임.

## 비범위 (후속)

- 동적 총감독 `tunaround hydrate` 서브커맨드 + 소프트 lease(설계 §2).
- durability OS 서비스화(launchd/Windows Service, 설계 §3).
- codex 원시 델타 상세 패널, claude 실시간 델타.
