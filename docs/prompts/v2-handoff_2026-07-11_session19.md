# 세션19 핸드오프 (2026-07-11): v2-44 완주 + 유령·깜빡임 근절 + 수신 자동화 + codex "보이는 세션" 전환

> 다음 세션 첫 행동: §7. 정본 타겟 모델은 [v2-43](../design/v2-43-target-model_2026-07-08.md) + [v2-44](../design/v2-44-presence-scanner-and-roles_2026-07-11.md)(구현 완료), 여기에 §2의 사용자 재확정이 얹힌다. 라이브 값 = backend-private 세션19 블록.

## 0. 한 줄 요약

v2-44 전 구간(T1~T4.5) 구현·라이브 전환 완료(PR #47~#51 머지, #52 대기). 유령 세션·로스터 깜빡임을 근본 원인 3개(마커 부재·tasklist 부하 에러·수신 poll 무태그 재등록)까지 파서 근절, 수신을 완전 자동화(사용자가 A2A를 언급할 필요 0), codex 주입을 "보이는 세션"으로 전환(mac 실증). 승인 게이트 폐지 등 거버넌스 개정. 다음 = T4.7(codex-relay).

## 1. PR 타임라인 (전부 CI green + 봇리뷰 반영 후 머지)

- **#47** v2-44 T1+T2: presence 스캐너(claude jsonl+codex rollout, 프로세스 게이트)·report_presence·supervised→infra alias·`tunaround task` CLI(W3)·watch-results --digest(W5)·훅 다이어트(W1·W2: 무장 로직 전삭제, 안내 5줄 1회)·codex 래퍼 삭제. **W1 근본 실측** = 훅 3중 등록(전역 python+python3 별도 엔트리+프로젝트 settings).
- **#48** T4: 머신 헤더 인프라 도트(presence·codex주입) + infra 카드 분리 + 스캐너 자기등록({machine}-presence-scan=머신 도달성) + 접속 재시도·MCP 재접속 + mcp_client 타임아웃(connect 10s/요청 60s).
- **#49** 유령 근절 + 수신 자동화: 세션 마커(.ctx=owner claude PID, autoarm+ping 자가치유, resume 갱신·unknown sentinel·args basename 매칭)로 pid 죽은 세션 즉시 제외. **수신 자동 가동** = SessionStart 지시문 + 기존 세션은 ping이 .rx 1회 주입 → 세션이 스스로 Monitor(poll) 가동.
- **#50** T4.5: main.rs 2,552→1,155줄(cli.rs 695 / cli_daemons.rs 370 / cli_node.rs 388, 동작 불변. 잔여=chat/core/serve 세션 배선 ~560줄).
- **#51** 깜빡임 근절 2종: ① tasklist 부하 에러 가드(exit 0인 채 "timeout expired"만 출력 - 실측 5회 중 3회 → pid<20이면 스냅샷 실패 간주=필터 스킵) ② `poll --no-register`(수신 전용 - 무태그 재등록이 스캐너 항목 덮어 '기타' 유령 생성하던 것 제거).
- **#52**(머지 확인 필요): #51 가드를 windows 한정으로(gemini high: 경량 unix 컨테이너는 프로세스<20 정상).
- 부수: DB 정리(WAL 3.2MB 체크포인트, 스테일 task cancel, 고아 db 4개 삭제), DeepSource 스타일 오탐(Empty new·부분경로)은 계속 스킵(.deepsource.toml 백로그).

## 2. 정본 재확정 (사용자, 2026-07-11 - 이후 세션은 이 문장이 기준)

1. **터미널 TUI로 열린 세션 = 전부 감독 세션.** 사용자가 어느 세션에서 말하든 서로 task를 주고받을 수 있어야 한다.
2. **sup·헤드리스 = 라이브 task 피드 외 비노출.** 사용자가 명시적으로 헤드리스를 지목하지 않는 한 감독 세션들이 알아서 위임·처리한다.
3. **감독 세션발 task 승인 게이트 폐지.** "감독 세션은 사용자가 시켜서 보내는 것" - 수신자는 민감 건 포함 전부 자율 수행+보고, 비가역·파괴 작업만 결과에 명시. (메모리 갱신 + 맥 전파 완료. a2a-usage §12 정본 반영은 T5.)
4. **codex 주입 = 로스터에 보이는 그 세션의 thread.** sup 사설 글루 thread 폐지(mac은 .thread 교체로 이미 전환·실증, 옵션 B). "sup이라는 정체성 자체가 불필요, 익명 배달 데몬(codex-relay)만 남는다" → T4.7.
5. **Redis 서서히 폐기**(tunaSalon에선 유용했으나 여기선 SSE·SQLite가 자리 대체).

## 3. 라이브 상태 (win, 재부팅 시 재기동 필요)

- 스택(안정 경로 `%LOCALAPPDATA%\tunaround\bin`, 풀피처 dashboard 빌드=#51 포함): serve 8770 / presence-scan(machine=win) / win-codex-sup poll(role=infra,purpose=codex-inject, 핸들러=`~/.tunaround/codex-sup-handle.cmd` **CRLF 필수**) / 총괄 세션 Monitor=watch-results --digest 60. PID는 backend-private 세션19.
- codex app-server ws://127.0.0.1:8790 (재부팅 후 미기동이 win-codex 무응답의 원인이었음 - codex.exe 직접 Start-Process로 기동, npm 래퍼는 Start-Process 불가).
- 훅(전역 ~/.claude/hooks, v2-44 최종판): autoarm=안내+pid 마커+수신 지시(1회) / ping=human-ping+마커 자가치유+.rx 지시 1회 / disarm=deregister+.ctx/.rx 정리. 프로젝트 settings에는 훅 등록 없음(전역만).
- mac: 동등 배포 완료(3개 task를 맥이 자율 수행: T3 스캐너, #49 배포, thread 교체). mac 함정 = 실행 중 바이너리 in-place cp 시 코드서명 SIGKILL → 원자 재배포(cp .new→codesign→mv).
- 로스터 = 실세션과 일치(win 세션 3~4 + mac 3 + 인프라 4: 양 머신 스캐너·codex-sup). 브로커 재기동 직후 ~15초는 로스터 공백 창(정상).

## 4. 미해결 / T4.7 입력

- **T4.7 = codex-relay 재설계**(checklist 항목): sup 정체성·주소 폐기 → 익명 relay가 자기 머신 runner=codex 세션들 앞 task를 대리 폴링·claim해 **그 세션 threadId로 주입**. GoalForm codex 세션 카드 = 유효 대상 복귀(그 머신 relay online 조건). 관전 전제 = codex를 `resume <threadId> --remote ws://127.0.0.1:8790`로 여는 규약(mac 충족·win 온보딩 문서화).
- **win 재현 미해결**: 사용자 TUI가 --remote로 attach된 thread(019f4d64)에 codex-inject 주입 시 **응답 대기 타임아웃**(task 3bc23088, canceled. mac은 같은 구성으로 성공). v2-37 §7 미해결(멀티클라이언트 브로드캐스트/승인 라우팅)과 동일 계열 - T4.7에서 원인 규명 필요. app-server 로그엔 에러 없음.
- 시스템 부하 시 tasklist/taskkill 자체가 timeout으로 실패(실측) - 배포 스크립트는 Stop-Process(네이티브)가 신뢰적.

## 5. 백로그 (checklist v2-44 섹션이 정본)

T5 정리(alias·report_candidates 제거, a2a-usage §9·§10·§12 갱신, infra 태그 규약 명문화) / v2-45(mesh 기억화=task→messages/FTS 색인+retention, watch-results 재구독 재생, Redis 폐기) / 마커 생존 유지 확장(3중 가드: claude 이름 검증·pid당 최신 1세션·창 폴백 - 유휴 4시간 드롭 해소) / W4 codex thread 로테이션(codex-inject에 기능 미구현) / DeepSource .deepsource.toml 튜닝 / main.rs 잔여 세션 배선 분할.

## 6. 오늘 교훈

- **성능 문제는 측정이 먼저**: 깜빡임 = "tasklist가 부하 시 exit 0으로 에러 문자열만 출력" - 5회 반복 측정이 한 방에 잡음. 추측 패치 안 함.
- 봇리뷰 실질 가치 재확인: resume 마커 버그(산 세션 오삭제)·comm 매칭(node 래퍼)·자가치유 무한반복·타임아웃 부재·가드 unix 오적용 전부 봇이 잡음. 단 **머지 직전 마지막 리뷰 확인**을 한 번 더(이번에 gemini high 1건을 머지 후 발견 → #52 후속).
- cmd 핸들러 CRLF(기지 함정 재발) / cargo 빌드는 실행 중 exe에 잠김(스모크용 임시 브로커도 잠금 유발).

## 7. 다음 세션 첫 행동

1. `git pull` + **PR #52 머지 확인**(CI green이면 머지 - gemini high 반영분).
2. **T4.7 착수**: 설계 문서(codex-relay) 먼저(규율 #7) → 구현. §4의 win 타임아웃 원인 규명 포함.
3. 새 세션은 자동으로 수신 지시를 받으니 별도 세팅 불요. 총괄로 쓸 세션이면 watch-results Monitor만 확인.
4. 라이브 값 = backend-private 세션19 블록.
