# 세션22 핸드오프 (2026-07-12) - v2-47 대시보드 관제탑 고도화 #1~#5 완주

> 진입점: 이 파일 먼저 → 정본 [v2-47 백로그](../design/v2-47-dashboard-observatory-backlog_2026-07-11.md)(항목별 "완료" 주석 + 후속 경로) → 필요 시 [v2-45 설계](../design/v2-45-mesh-persistence-and-replay_2026-07-11.md).
> 이전 세션: [세션21](v2-handoff_2026-07-11_session21.md)(v2-45 P0~P8 완주 + v2-47 백로그 문서화).

## 한 줄 요약

세션21이 문서화한 **v2-47 대시보드 관제탑 고도화 5개 주 항목을 전부 완주**했다. 세 개의 소 PR(#67·#68·#69)로 나눠 각각 `구현 → 적대적 리뷰 → CI(clippy 3-OS·dashboard SPA·CodeRabbit) → 머지 → 배포(WMI 스폰) → 라이브 시각 검증(Chrome)` 파이프라인을 완주. **main = `9f2e238`**, 미커밋 없음, 열린 v2-47 브랜치 없음(머지 시 삭제).

## 완료 사항 (전부 머지·배포·라이브 검증)

| PR | 머지 SHA | 항목 | 라이브 검증 결과 |
|----|---------|------|----------------|
| **#67** | `cec4d8f`(merge), `201dacf` | #1 피드 카드 상세 펼침(요청·결과·실패 사유) + #2 필터 칩(상태·머신·러너·텍스트) | 실패 칩 → "2/58 tasks" 필터, 펼침에 요청/결과/실패 사유 표시 확인 |
| **#68** | `742a8e1`(merge), `2187374` | #3 브로커 헬스 패널(`GET /dashboard/health`) + #4 브라우저 알림(옵트인) | 헬스 실데이터(미배달 1 경고색·mac/win 스캐너 online), 알림 토글 렌더 확인 |
| **#69** | `9f2e238`(merge), `10c8519` | #5 위임 이력 검색(`GET /dashboard/search?q=`) | "진단" 입력 → a2a 스코프 결과(speaker `win-opus-boss`, 형태소 매칭) 렌더 확인 |

**배포**: 3회 모두 `target/release/tunaround.exe`(feature `"morphology mcp serve worker dashboard"`) 빌드 → **WMI 스폰**(`Invoke-CimMethod Win32_Process Create`)으로 `scripts/restart-win-mesh.ps1 -SourceBin` 실행 → `mesh.pids` PID 선별 종료 + rename-swap. 세션 수신 poll 무중단. 스테이블 바이너리 = `%LOCALAPPDATA%\tunaround\bin\tunaround.exe`. **최종 mesh 건강: mac·win 스캐너 online, 고착 0.**

## 적대적 리뷰가 잡은 실이슈 (전부 반영)

- **#67**: index-as-key(MAJOR) → 안정 키(`updatedAt-state`, SSE 중복 가드가 유일성 보장). 공백-only 텍스트 빈 블록 → `joinParts` trim 필터 + `DetailBlock` trim 가드.
- **#68 (CodeRabbit Minor)**: 헬스 핸들러가 `spawn_blocking`·내부 쿼리 실패를 `Health::default()`(전부 0)로 반환 = **고장 난 브로커를 "정상(0건)"으로 위장** → `Result`로 모아 **500으로 표면화**(프론트가 "조회 실패" 표시). "헬스는 실패를 정상으로 위장하지 않는다"가 원칙.
- **#69**: 검색이 전체 `messages/FTS`를 훑어 **비-a2a 세션버스 전사(post_turn)까지 무인증 대시보드로 노출** → 결과를 `speaker="a2a/*"`(P6a 색인 화자)로 스코프(위임 이력만, over-fetch 60→a2a 필터→20으로 희석 방지). React 키 경계 모호성 → `::` 구분자.

## 확정된 결정·설계 스코프 (재론 금지)

- **#3 브로커 uptime·WAL 크기는 후속으로 보류.** `SqliteStore`에 path 필드 + config get/set 접근자 추가가 필요해, 핵심 가치(열린 task 건강 집계 + 머신별 스캐너 도달성)만 **무상태-추가**로 구현했다. 착수 경로는 백로그 문서 #3 주석에 명시(uptime=serve 기동 시 config row 기록, WAL=`<db>-wal` stat).
- **#3 헬스 집계는 `classify_task_health`(enum) 단일 소스**로 수렴. `tasks()` MCP의 stuck?/no-consumer? 표시(`health_annotation`)와 **같은 임계**를 쓴다(`src/mcp/format.rs`). 중복 임계 금지.
- **#5 검색은 MCP `search_context`와 같은 retriever 재사용**(형태소+FTS). 별도 retriever-state 서브라우터를 axum `.merge()`(기존 store-state 핸들러 무영향, e2e 스모크로 404 회귀 방지). **배포 바이너리는 `semantic` 미포함 = retriever의 embedder(원격 Ollama) 없음 → 검색이 네트워크 비의존.** (semantic feature를 켜면 embedder가 붙어 Ollama에 물리니 주의.)
- **#5 UI = 탭 네비게이션 대신 자체 완결 섹션**(SearchPanel, 디바운스 400ms + AbortController). 관제탑 레이아웃 하단.
- **관제탑 원칙 불변**: v2-47 5건 전부 read-only 뷰 강화. 직접 제어 UX 비확장(세션21 사용자 확정).

## 교훈 (이 세션 관측)

- **DeepSource JS/Rust는 자문성**(main 브랜치 미보호 = 어떤 체크도 기술적 머지 게이트 아님). canonical 게이트 = **clippy 3-OS + dashboard SPA(frontend build+embed) + CodeRabbit**. DeepSource JS는 파일 기존 idiom(top-level `function` 선언·문자열 연결)을 따른 **신규 코드도 diff 라인이면 재귀속**해 fail시킴(기존 헬퍼는 grandfathered). Rust `String::new()`도 "default() 쓰라"로 플래그하나 clippy가 통과시키는 관용구. → 실질 이슈(index-key 등)만 고치고 idiom minor는 자문으로 문서화 후 머지(머지 후엔 기존 라인이 되어 재플래그 안 됨). 메모리 [[deepsource-python-fails-on-main]]에 추가 기록됨.
- **CodeRabbit 소요 편차 큼**(같은 세션에 1분~6분). pending 길어도 기다릴 것(canonical 게이트).
- 프론트 dist는 gitignore(`frontend/.gitignore`) → CI `dashboard SPA` 잡이 `npm run build`로 임베드 검증. src만 커밋.

## 미커밋·브랜치·백그라운드 상태

- **미커밋 변경: 없음**(핸드오프 커밋 제외). **현재 브랜치: main**(= origin/main = `9f2e238`).
- **열린 v2-47 브랜치: 없음**(#67·#68·#69 머지 시 `--delete-branch`).
- **백그라운드 작업: 없음**(세션 중 CI 폴 백그라운드 커맨드는 전부 완료).
- **스테일 브랜치 잔재**(이전 세션): 로컬/원격에 `feat/v2-45-p0~p8`, `feat/node-onboarding`, `feat/poll-watch`, `docs/session16-handoff` 등이 남아 있음. 이번 세션 산출 아님. 정리 필요하면 별도 위생 작업으로(거버넌스 규칙 #4).

## 다음 세션 첫 행동 (우선순위 순)

1. **`cargo test --features "morphology mcp serve worker dashboard"`로 상태 확인**(cargo는 Bash 툴로). 재부팅했으면 `pwsh -File scripts\restart-win-mesh.ps1`로 mesh 복구(nohup 데몬 재기동).
2. **방향 선택(사용자에게 물을 것)** - v2-47 주 항목은 끝났으므로 다음 아크는 열려 있음:
   - (a) **v2-47 낮은 우선순위 2건**(백로그 문서 하단): ★ 이동 이력·세션 등장/소멸 타임라인(P4의 `agent_human_input` 테이블 활용) / 원격 관전 모드 다듬기(관전 뱃지·모바일 반응형). "기록만" 수준, 급하지 않음.
   - (b) **#3 후속**(브로커 uptime·WAL): store 표면 변경(path 필드 + config 접근자) 후 헬스 패널 확장.
   - (c) **v2-48 opencode 배선**([백로그](../design/v2-48-opencode-wiring_2026-07-11.md), 정찰 확정 - 스키마 냉각 후 착수) 등 세션21이 남긴 백로그.
   - (d) 스테일 브랜치 정리(위생).
3. 규율: 비trivial 작업 전 plan + `checklist.md`·`context-notes.md`. 위임은 tunaLlama→A2A codex→Sonnet, 아키텍트·리뷰·검증=Opus. 커밋은 논리단위로 자유, **push는 사용자 승인**(이번 세션은 사용자가 "계획된건 다 하고" 전권 위임했었음 - 새 세션엔 재확인).

## 검증 커맨드 참고

- 헬스: `curl -s http://127.0.0.1:8770/dashboard/health`
- 검색: `curl -s "http://127.0.0.1:8770/dashboard/search?q=<질의>"`(percent-encoded)
- 대시보드: `http://127.0.0.1:8770/dashboard`(loopback=풀컨트롤, 원격=read-only 403 관전)
