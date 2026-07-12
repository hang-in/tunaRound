# 세션27 핸드오프 (2026-07-13) - 이슈 #88 시간창 게이트 정본화(머지) + 대시보드 스피너·러너아이콘(머지·배포) + 스피너 버그 #94 발견

> 진입점: 이 파일 먼저 → 다음 = **① fable 5로 프로젝트 리뷰 먼저 → ② 스피너 버그(이슈 #94) 패치.** 이전: [세션26](v2-handoff_2026-07-12_session26.md).

## 한 줄 요약

이슈 #88(codex presence 유령 오라우팅)을 **라이브 실측으로 대안 #1·#2를 기각**하고 사람활동 **시간창 게이트를 정본화**(적대 검증 GO) → **PR #92 머지 → #88 CLOSED**. 사용자 아이디어로 **대시보드 "동작 중(working) 스피너 + 로스터 러너 아이콘"**을 붙여 **PR #93 머지 → WMI 스폰 재배포 v0.4.0 라이브 검증**. **티키타카 테스트**(mac-claude·win-codex 실제 A2A 왕복) 성공. 라이브 검증 중 **스피너 오탐/누락 버그 발견 → 이슈 #94 등록**(수정은 다음 세션). main=`84efe21`. 미커밋 없음.

## 완료 사항 (전부 머지·CI green·봇 리뷰 처리)

| PR | 트랙 | 핵심 | 머지 커밋 |
|----|------|------|------|
| #92 | 이슈 #88 | codex presence 시간창 게이트(`apply_codex_human_input_gate`) 정본화. 적대 검증 3렌즈 GO(blocker/major 0). **#88 CLOSED.** | `e746b13`(게이트=`ba0fba8`) |
| #93 | 대시보드 | 동작 스피너(`busy` 필드=working task의 to_agent) + 러너 아이콘(RunnerIcon 복원). 봇 리뷰 3건 반영. | `84efe21`(=`ea67897`+`f65735f`) |

### 이슈 #88 = 시간창 게이트 정본화 (재론 금지)
- **라이브 실측(codex-cli 0.144.1)으로 대안 기각**: **#1(session_meta PID)** = codex가 rollout에 PID 미기록 → 불가. **#2(app-server loaded/list canonical)** = `thread/list` status·`thread/loaded/list`는 **인스턴스별**(throwaway 8799=전부 notLoaded)이고 **죽은 thread도 `thread/resume` 성공→`idle`/loaded로 오염**(relay 주입이 유령을 loaded로 만듦=악화). 사람 TUI는 VS Code 자체 app-server(PID 44852, ws 도달 불가)에 삶. **깨끗한 per-thread 생존 신호가 도달 범위 밖.**
- **적대 검증 3렌즈 = GO**: "#2 viable" 반증 실패로 결정 재확증. 시간창은 임시방편이 아니라 **원리적 상한**. **수용된 잔여**: 방금 닫은 세션은 window 동안 잔존(`codex_gate_fresh_churn_ghost_lingers` 테스트가 명시). 게이트는 유령 수명 상한(240→window) + relay 자기유지 차단.
- **세션27 강화**(minor 2): 256KB tail 밖으로 밀린 라이브 장기세션의 human_input over-drop 방지=재스캔 None이어도 캐시 이전값 유지(단조). lib 597 pass.
- 상세: 실측·결정 = `context-notes.md` 세션27 §, 계약 = 게이트 doc 주석(`src/presence_scan.rs::apply_codex_human_input_gate`).

### 티키타카 테스트 (실제 A2A 왕복 = 생존핑 아님)
- mac-claude-home(크로스머신 claude): "2+2=4 — mesh 왕복 정상" / win-codex-home(codex relay 경로): "3+4=7, DESKTOP-BB1EP5U의 codex". 실제 처리·자기식별 확인.
- 관찰: **수신 알림이 긴 task 본문을 프리뷰에서 truncate**(mac 세션 언급) → 별개 작은 개선거리.

### 배포·라이브 검증
- 릴리스 빌드(`morphology mcp serve worker dashboard`, semantic 제외) → **WMI 스폰**으로 `restart-win-mesh.ps1 -SourceBin`(하네스 job 탈출=데몬 생존, rename-swap·mesh.pids 선별종료=세션 poll 생존). 새 broker PID 12320, mesh 9/9 online.
- 대시보드 v0.4.0 라이브: 러너 아이콘·busy 스피너 렌더 확인(단, 아래 #94 버그).

## 진행 중 / 다음 세션 착수 (우선순위 순)

### 1. ⚠ fable 5로 프로젝트 리뷰 먼저 (사용자 지정)
- 다음 세션은 **스피너 패치 전에 fable 5(claude-fable-5)로 프로젝트 리뷰를 먼저** 수행할 예정(사용자 계획).

### 2. 스피너 버그 수정 (이슈 #94)
- **증상**: mac-codex-home이 아무것도 안 하는데 스피너 계속 돎(FP) / win-codex·mac-claude가 열일했는데 조용(FN).
- **근본원인(실측 확정)**: 스피너 `busy` = `/dashboard/roster`의 `busy` = **열린 `state=working` task의 to_agent 집합**(`src/mcp/server.rs` dashboard_roster_handler). 이게 "지금 일하는 중"의 나쁜 프록시:
  - **FP = stuck working task**: claim 후 미완료로 `working`에 갇힌 task가 lease 만료·requeue 전까지 to_agent를 영구 busy로. 실측 `t-978e`(win-codex→mac-codex, claimed_by=mac-codex, runner=codex) **433s(7분+) working, lease 미만료**.
  - **FN = 폴링 랙**: roster 5초 폴(App.tsx). 빠른 task(tiki-taka)는 working 창이 5초보다 짧아 관측 안 됨.
  - to_agent↔로스터 uuid 매칭 자체는 정상(실측). uuid 불일치 아님.
- **수정 방향(다음 세션 검토)**: 유력=스피너를 **라이브 SSE task 이벤트 스트림**에서 도출(status=working 추가/completed·failed·canceled 제거, Feed가 이미 `/dashboard/events` 구독)+**stale 타임아웃**(stuck FP 해소). 대안=backend busy를 fresh-lease/최근 claim으로 제한+폴 간격 축소(FP만 완화).
- 상세: [이슈 #94](https://github.com/hang-in/tunaRound/issues/94).

### 3. 남은 후속 (급하지 않음)
- CHANGELOG [Unreleased] 갱신(#88 게이트·스피너·아이콘) → v0.5.0 릴리즈(도그푸딩 후·승인, 대시보드-릴리스-포함 P0).
- 수신 알림 긴 task 본문 truncate 개선(위 티키타카 관찰).
- 부수: `t-978e`가 7분+ working 갇힌 것 자체(codex relay claim 후 미완료 / lease requeue 동작) 점검 여지.

## 확정 결정·교훈 (재론 금지)

- **#88 시간창 게이트 = 정본**: 라이브 실측으로 #1·#2 원리적 불가 확증. per-thread 생존엔 아키텍처 전환(--remote attach 모델)이 필요하나 v2-46 "독립 보이는 세션" 방향과 상충이라 비채택. [[deepsource-python-fails-on-main]] 계열: **DeepSource JS는 out-of-diff 재귀속 자문성**(PR #93 red였으나 main=success, 내 diff 위 인라인 0건 → 머지 후 소멸, canonical=clippy·dashboard·CodeRabbit).
- **스피너 신호 교훈**: `state=working`은 "claim됐고 미종결"이지 "지금 활동 중"이 아니다. stuck task=FP, 폴 랙=FN. 실시간 신호(SSE)+staleness가 필요.
- **배포=WMI 스폰**([[mesh-restart-needs-job-escape]]) + `restart-win-mesh.ps1 -SourceBin`. mesh.pids 선별종료로 세션 Monitor 생존. 릴리스 dashboard 빌드는 dist/ 임베드라 **프론트 `npm run build` 먼저**.
- **적대 검증이 표준 오라클**: #88에서 "#2 viable" 반증을 3렌즈로 시도→실패로 정본화 정당성 확증.

## 미커밋·브랜치·백그라운드

- **미커밋: 없음**(이 핸드오프 커밋 제외). main=`84efe21`(#88 게이트 + 스피너 통합).
- **열린 브랜치**: origin=main만(#92·#93 머지분 삭제 완료).
- **백그라운드**: WMI mesh 데몬 상주(broker 8770=PID 12320·app-server 8790·presence-scan·codex-relay·watch-results, **v0.4.0 새 바이너리**). 이 세션 A2A 수신 Monitor는 재시작 시 SessionStart 훅 재무장. **라이브에 stuck task `t-978e`(mac-codex working 7분+) 잔존** = mac-codex 스피너 오탐 원인(#94, lease 만료 시 requeue 예정).

## 검증 커맨드 참고

- 상태: `cargo test --features "morphology semantic mcp serve worker"`(597 lib). 게이트: `cargo fmt --all -- --check` + `cargo clippy --features "..." --all-targets -D warnings` + no-default·worker단독·dashboard 빌드.
- 대시보드: http://127.0.0.1:8770/dashboard. 로스터 busy: `curl -s .../dashboard/roster`. mesh 헬스: `.../dashboard/health`. 재부팅 복구=`pwsh -File scripts\restart-win-mesh.ps1`.
- 스피너 버그 재현: 로스터에서 stuck working task(`t-978e`류)가 to_agent를 영구 busy로 만드는지 = `broker.db` `SELECT ... FROM tasks WHERE state='working'`(read-only).
