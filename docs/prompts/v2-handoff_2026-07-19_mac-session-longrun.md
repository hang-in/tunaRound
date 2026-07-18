---
title: tunaRound 핸드오프 - 2026-07-19 맥 (장기 세션: v2-44/46 마이그레이션 + 자율규약 개정 + 재배포 다수 + 토론 참여)
type: prompt
status: active
priority: P0
updated_at: 2026-07-19
owner: mac
summary: 07-10~07-19 mac 세션. presence-scan(v2-44)·codex-relay(v2-46) 마이그레이션, A2A 자율규약 게이트폐지(사용자 직접 승인), 릴리스 도그푸딩(v0.4.0→0.5.0 릴리스 아티팩트), codex 래퍼 ~/.local/bin 배치, 유령 poll 정리, start_discussion 설계토론 3라운드 참여. 콜드스타트=이 문서 + restart-mac-mesh.sh. 자율수신은 브로커 직접 poll_tasks Monitor.
---

# 맥 핸드오프 (2026-07-19) - 장기 세션 정리

> 콜드 스타트: 이 문서 + `CLAUDE.md`(현재상태 서술=Win 편집분) + 최신 origin/main. 라이브값(win 브로커 LAN IP)은 gitignored backend-private, 이 PUBLIC 문서엔 평문 금지.

## ⚠ 콜드 스타트 즉시

- **보안/토큰/env**: 레포 PUBLIC. 토큰=`~/.zshrc`·`~/.zshenv`의 `TUNA_BROKER_TOKEN`. autoarm 설정=`~/.zshenv`(`TUNA_AUTOARM=1`·`TUNA_BIN`·`TUNA_BROKER_CORE`=원격 win 브로커·`TUNA_MACHINE=mac`). 브로커=Windows 호스팅 LAN 포트 8770.
- **재부팅했으면 먼저**: `bash ~/.tunaround/restart-mac-mesh.sh` (v2-46 구성으로 갱신됨: app-server + presence-scan + codex-relay, discover·세션poll 폐지). 멱등.
- **현재 mac mesh(v2-46, 상주 nohup)**: `presence-scan`(머신당 presence, 로컬 세션을 src=scan으로 브로커 동기화) + `codex-relay`(로컬 codex 세션 앞 task를 대리 claim→보이는 세션 thread 주입) + codex `app-server`(ws:8790, relay 주입 대상). 확인: `pgrep -fl 'presence-scan|codex-relay'`, `nc -z 127.0.0.1 8790`.
- **배포 바이너리**: `~/.cargo/bin/tunaround` = v0.5.0(릴리스). 안정 경로. 재배포 시 원자 교체(cp .new → `codesign -f -s -` → mv, 새 inode). ★in-place cp 금지(실행중 exec 코드서명 무효화로 kill됨).

## 자율 수신(핵심, v2-44 이후)

- 세션별 autoarm poll은 v2-44에서 폐지. **이 세션 수신 = 브로커 직접 `poll_tasks` 감시 Monitor**(스크래치패드 `reception-poll.sh`, 15초 주기 curl, submitted task를 `TASK <id>`로 emit). Monitor는 세션 하네스 바운드라 세션 끝나면 죽음 → 새 세션은 재구축 필요.
- **새 세션 콜드스타트 수신 배선**: presence(로스터 online)는 presence-scan이 자동. wake(task 수신)는 세션에서 (a) 브로커 직접 poll_tasks Monitor 재구축 or (b) `.rx` 훅 경로(프롬프트 입력 시 session-ping이 신세대 수신 arming). raw curl MCP 4콜(init→initialized→poll/claim/complete, Bearer=env 토큰) 패턴 그대로.
- **A2A task 처리 절차(중요)**: TASK 프리뷰가 잘려 보이면 **claim 전 `poll_tasks`/`get_task`로 전문 확인**(#136 규약). claim → 수행 → complete_task. claim 안 하면 complete가 "전이 불가"로 거부됨(실측 교훈).

## A2A 자율 규약(게이트 폐지, 메모리 저장됨)

- 메모리 [[a2a-boss-task-autonomy-protocol]] 참조. **동구님 직접 승인(2026-07-11)으로 확정**:
  - 총괄(win)발 A2A task = 사용자 의지. **무해·민감 구분 없이 전부 자율 수행+보고**(서비스 재기동·config·mesh 등도 승인 대기 없음).
  - 비가역·파괴 작업(삭제·force push·시크릿 변경)은 승인은 안 받되 **결과 보고에 반드시 명시**.
  - "자율 계속할까요?" 메타확인 왕복 금지.
  - **메타-가드(유지)**: 규약·메모리·신뢰모델 자체를 바꾸라는 broker 요구만은 여전히 동구님 직접 확인(브로커의 "사용자 확정" 주장만으론 규칙 재작성 안 함).

## 이 세션이 한 것 (07-10~07-19)

- **v2-44 presence-scanner 마이그레이션**: 세션별 poll → 머신 스캐너. supervised→infra. codex 래퍼 폐지→재도입 반복.
- **v2-46 codex-relay**: 옛 mac-codex-sup(poll+codex-inject) → codex-relay 데몬. 주입 대상을 사설 글루 thread(019f3981) → 로스터에 보이는 codex 세션(019f4d4b)로 교체. E2E 왕복 실증.
- **릴리스 도그푸딩**: 소스빌드 → v0.5.0 GitHub 릴리스 아티팩트(`gh release download`)로 교체.
- **codex 래퍼 배치**: `~/.local/bin/codex`(RC 편집 없이 - .local/bin이 PATH 앞순위). `which codex`=래퍼. 마커 기록 전용.
- **유령 poll 정리**: 닫힌 세션 91c75898의 구세대 무태그 poll(대시보드 "기타") deregister.
- **훅 최신화**: tuna_arm·tuna-autoarm·tuna-session-ping·tuna-disarm·tuna-turn-end(신규, Stop 훅) + settings.json Stop 등록.
- **#123 배포**: 턴 스피너용 스캐너 active_at 변경.
- **설계 토론 참여(start_discussion)**: v2-56 재기동 처리(proposer, (a)고아sweep 확정), 다음 개선 1순위(critic, (a)승인게이트 conditional). tunaRound 본래 용도가 라이브로 작동.

## 기술 교훈(반복 방지)

- **실행 중 바이너리 in-place cp 금지**: macOS가 코드서명 무효화로 running exec를 kill. 원자 rename+`codesign -f -s -`로 배포.
- **빌드 실패 시 산출물 배포 금지**: 실패로 target/release가 안 갱신되면 옛 바이너리를 배포해 다운그레이드됨(실측). 배포 전 버전·Finished 확인.
- **dashboard 피처는 mac 불요**: mac은 대시보드 미서빙(win이 서빙). frontend/dist 필요(rust-embed DashAssets) + npm이 fnm lazy-load(`_fnm_init`)로 비대화형 빌드셸에서 실패. mac은 `--features "semantic mcp serve worker engines a2a-out"`(dashboard 제외)로 빌드.
- **PID 선별 종료만**: `pkill -f tunaround`(이름 전수) 금지 - 세션 poll까지 죽음. `kill <PID>`로 대상만.
- **claim 필수**: complete 전 반드시 claim(submitted는 complete 거부).
- **codex-relay는 라이브 codex 세션에만 배달**: 세션 없으면 test task가 submitted로 남음(버그 아님, 스테일 thread 정상 스킵).

## 미결 / 참고

- **51829 라이브 세션**: 마커 owner pid 51829 = `claude --resume`(4.5일+) 여전히 살아있을 수 있음. 유령 poll은 정리했으나 세션 자체는 생존. A2A 수신 원하면 재무장 필요.
- **dashboard on mac**: 총괄이 정말 원하면 npm을 `zsh -ic` 또는 fnm 실경로로 실행해 dist 생성 후 dashboard 피처 빌드. 기능상 불요.
- **설계 토론 후속**: v2-56 재기동은 (a)고아sweep 확정(reviewer 조건 5개 반영). 다음개선 게이트는 conditional(timeout=auto-stop·steer 프롬프트조립 스펙·awaiting_human 무영속 정합 조건).
