# 세션18 핸드오프 (2026-07-10~11): 신뢰성 대청소 + 크로스머신 총감독 + 로스터 리디자인

> 다음 세션 첫 행동: 이 문서 → §7(다음 행동) 순으로. 정본 타겟 모델은 여전히 [v2-43](../design/v2-43-target-model_2026-07-08.md)이고, 이 세션에서 **v2-44(presence 재설계) 제안이 나와 사용자 승인 대기 중**(§6).

## 0. 한 줄 요약

PR #36~45 열 개 머지 + #46(로스터 리디자인) 열림. v2-43 배선 4/4 완료, 재검증 매트릭스 전 구간(win 4 leg + mac 2 leg + 크로스머신 총감독) 실측 통과, 로스터 오염 3층 근절, codex "회귀" 오판을 소스 대조로 정정. 운영 발견 3건(안정 바이너리·rust-embed 런타임 dist·MCP 연결 조건)과 v2-44 제안까지.

## 1. PR 타임라인 (전부 CI green + 봇리뷰 반영 후 머지)

- **#36** deregister + claude pull 수신 배선 + orphan 리퍼 (+빈 pmap 가드 등 봇 6건)
- **#37** 워커 섹션(v2-43 §5-4, role=worker 분리·작업 중만 표시)
- **#38** codex 자동무장 래퍼(scripts/codex PATH shim) + v2-37 §11 회귀 기록 + gitignore
- **#39** codex "회귀" **정정**: 회귀 아님 - plain `codex --remote`는 원래 새 thread. **관전 = `codex resume <threadId> --remote ws://...`**(소스 대조 + win 실측 완료). a2a-usage §10도 정정
- **#40** presence 신뢰성: 유령 poll 부활 차단(kill 확인 후에만 pidfile 삭제) + 총감독 첫핑 404 재시도 + work env 토큰 폴백 + home 프로젝트 정규화
- **#41** 크로스머신 총감독: human-ping·deregister 원격+Bearer 허용(v2-43 비범위 해제). **실측: 맥 입력 → ★ 맥 이동 → win 입력 → 복귀**
- **#42** 무장 경합 제거(세션별 락 + autoarm을 ensure_armed로 일원화) + temp cwd 노이즈 무장 제외
- **#43** detached 기동 CREATE_NO_WINDOW(콘솔 창 튀어나옴 제거) + codex 래퍼 role=session 통일
- **#44** 총감독 핑을 사람 프롬프트에만: 하네스가 Monitor wake에도 UserPromptSubmit을 발화 → task 수신만 한 세션이 ★ 탈취(실측). `<task-notification>`/`[SYSTEM NOTIFICATION` prefix면 핑 생략
- **#45** scripts/codex exec bit(맥이 발견한 레포 버그: 100644라 POSIX which가 래퍼 건너뜀)
- **#46 (열림)** 로스터·목표제출 리디자인: 머신 그룹·1줄 행·브랜드 아이콘(lobe-icons)·선택 칩+프리셋. 목업=https://claude.ai/code/artifact/8b93801d-dea9-42e8-90a8-0cda2dd2ed3d. 2커밋째(23377af) CI 재검 중이었음 - **머지 전 CI+봇리뷰 확인**

## 2. 재검증 매트릭스 (전 구간 실측, 새 presence 모델)

① codex 관전(`resume --remote`, M1 히스토리+M2 라이브) ② claude 감독 pull(goal→poll→Monitor wake→claim→complete) ③ 헤드리스 워커(work --once, runner 기록) ④ codex 풀루프(goal→watcher→inject→native MCP)+**watch-results가 총괄 깨움** ⑤ mac 크로스머신(훅 배포 task 자율 수행) ⑥ 크로스머신 총감독(★ 이동). **v2-43 워크플로우 = 측정된 사실.**

## 3. 로스터 오염 3층 (전부 해결)

1. **유령 부활**: disarm/리퍼가 kill 실패에도 pidfile 삭제 → poll이 heartbeat "미등록" 응답에 자가 재등록(#40)
2. **무장 경합**: SessionStart(autoarm)↔첫 프롬프트(ping) 동시 무장 → 중복 poll → pidfile 없는 poll = 리퍼 사각지대(#42, 락+일원화)
3. **노이즈**: %TEMP% 자동화 headless까지 무장(#42, temp 제외) / **★ 오염**: 자동 이벤트 wake의 핑(#44)

## 4. 운영 발견 (재발 방지 핵심)

- **안정 바이너리 분리**: 런타임은 `%LOCALAPPDATA%\tunaround\bin\tunaround.exe`(config TUNA_BIN이 가리킴), target/debug는 빌드 전용. **재빌드가 mesh를 안 죽인다**(이전엔 mass-kill → 열일 중 세션(luckyCAD)이 재무장 기회 없이 소멸 2회).
- **rust-embed는 debug에서 dist를 런타임 로드**: 프론트 반영 = `npm run build`만. "반영용 브로커 재빌드" 불필요(release만 진짜 임베드).
- **tuna-broker MCP native는 세션 시작 시 브로커 생존 필요**: 죽어 있으면 그 세션은 영영 raw HTTP(curl) 신세. 세션 재시작으로 붙음(이 세션 실측).
- 기동 순서: **브로커 먼저 → app-server**(늦으면 MCP 로드 실패).

## 5. 거버넌스 실증 + 규약

- 맥이 민감 task(셸 프로필·토큰파일·mesh 기동)를 **운영자 미승인 fail로 보고**(로컬 운영자 > 원격 총괄) / 무해 task 6건은 자율 수행. 실패 경로도 watch-results가 총괄을 깨움.
- **총괄발 task 자율 규약**(양 머신 메모리 저장됨): 총괄 task=사용자 의지, 메타 확인 왕복 금지, 무해=자율+보고, 민감=로컬 운영자 게이트. 정본 반영 후보: a2a-usage §12.

## 6. v2-44 제안 (사용자 승인 대기)

"세션이 자꾸 로스터에서 사라진다"의 구조 답: **presence와 수신의 분리**. presence = 머신당 스캐너 데몬 1개(프로세스 테이블 + ~/.claude/projects jsonl 활동 스캔 → 머신 로스터 일괄 보고, 세션 존재=ground truth, poll·훅·래퍼·PATH 비의존, 유령·소멸 원천 차단, codex 래퍼 불요). 수신 = 세션이 원할 때만 자기 poll+Monitor(현행). boss = human-ping 유지. discover.rs 스캔 코드 용도 변경. 리치 TUI도 같은 roster API. **승인 시 설계 문서(v2-44)부터**(규율 #7).

## 7. 다음 세션 첫 행동

1. `git checkout main && git pull`. **PR #46** CI+봇리뷰 확인 후 머지(브랜치 feat/roster-redesign, 리디자인 확인은 대시보드 새로고침으로 - 이미 라이브).
2. **v2-44 승인 여부 확인** → 승인이면 설계 문서 작성부터.
3. mac codex 확인: 맥 **새 터미널**에서 codex 실행 → 로스터에 뜨는지(래퍼 PATH는 배포됨).
4. 백로그(급하지 않음): 맥 격차(감독 mesh 재기동·config-first, 운영자 보류) / `tunaround task claim/complete` CLI(수신 raw-curl 마찰… 단 native MCP 연결 조건 발견으로 우선순위 재평가) / "무장됨≠수신중" 로스터 표시 / .deepsource.toml(부분경로·complexity·ESM 오탐) / v2-37 §11에 win 실측 확인 한 줄.
5. 라이브 값 = backend-private 세션18 블록(안정 bin 경로·PID 포함).

## 8. 교훈 (이 세션에서 배운 것)

- **성급한 회귀 결론 금지**: 성공/실패 커맨드 대조 + 오픈소스 태그 diff 먼저(메모리 저장됨).
- **유지보수가 곧 장애 원인**: "모든 프로세스 kill" 류 정리가 산 세션을 떨어뜨림 → 안정 바이너리 분리로 구조 해결.
- **하네스 이벤트 의미론 주의**: UserPromptSubmit ≠ 항상 사람(자동 wake 포함) - 훅이 신호 의미를 검증해야 함.
