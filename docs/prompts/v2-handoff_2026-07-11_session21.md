# 세션21 핸드오프 (2026-07-11): v2-45 설계 확정 + P0·P1·P2 완주 + 대시보드 정체성 + /handoff 스킬

> 다음 세션 첫 행동: §5. **⚠ 다음 세션 모델 = Opus 4.8**(Fable 주간 사용량 소진). 정본 설계 = [v2-45](../design/v2-45-mesh-persistence-and-replay_2026-07-11.md) - **매 P 착수 전 §4 해당 항목 + §5 고정 계약 9건을 다시 읽을 것**(컴팩션·모델 교체를 넘는 무손실 앵커).

## 0. 한 줄 요약

v2-45 아크를 설계(병렬 조사 7 에이전트, gap-check 포함)부터 P0(직접 제어 제거)·P1(watch-results 재접속)·P2(피드 스냅샷+서버 재생 기반)까지 머지·라이브 배포·검증 완료. 대시보드=관제탑 정체성 확정, opencode·대시보드 고도화는 백로그 문서화, /handoff 전역 스킬을 win-codex-home A2A 위임으로 생성. 남은 것 = P3(진행 중이던 워크플로우, §4) → P4·P5 → P6a·b → P7·P8(세션 분할 계획 확정).

## 1. 완료 타임라인 (전부 CI green + 봇리뷰 전수 처리 후 머지, 라이브 배포·검증까지)

- **설계 정본**: [v2-45 mesh 영속·재생 아크](../design/v2-45-mesh-persistence-and-replay_2026-07-11.md). 병렬 조사(watch-results·feed·★영속·codex신호·기억화·Redis 6영역 + gap-check)가 잡은 함정들을 §5 고정 계약 9건으로 승격(envelope 매핑·since 포맷·스키마 v9/v10 선점·데이터 수명·sync_presence 최종형 등). 조사 원본 = 워크플로우 wf_0a846d3e-325 journal.
- **PR #57 = P0**: /dashboard/control+ControlForm 전체 제거(-305줄, 전용 SSRF 가드 포함·공유 CSRF는 유지). 라이브 확인 = control 라우트 소멸(401 폴백).
- **PR #58 = P1**: watch-results 재접속 루프(백오프 1→30s, InboxState 루프 밖 소유, 전 단절 경로 flush, 연속 20회 초과만 exit 1). 봇리뷰 반영 2건 = SeenSet 상한(FIFO 4096)·수립 시점 기준 순수 생존 측정(크래시루프 브로커 영구 재시도 방지). **라이브 재현 실증**: 배포 재기동 순간 구 바이너리 인박스가 정확히 이 결함으로 즉사 → 신판 재무장.
- **PR #59 = P2**: store 공용 질의 list_tasks_replay(+rowid 2차 키) + envelope 헬퍼 통일(state=completed만 "completed") + `/dashboard/events?replay=N`(전 상태, 상한 500)·`?since=TS&dispatcher=`(completed/failed만) subscribe-먼저 chain + Feed `?replay=50`+중복 가드. axum query 피처 대신 Uri 직접 파싱(Cargo.toml 불가침). 522 pass + temp 브로커 스모크 + **라이브 검증: 리로드해도 피드 유지, 무파라미터=무재생(watch-results 회귀 없음)**.
- **백로그 문서 2건**: [v2-47 대시보드 관제탑 고도화](../design/v2-47-dashboard-observatory-backlog_2026-07-11.md)(카드 상세·필터·헬스 패널·알림·검색 탭 - 전부 read-only 뷰) / [v2-48 opencode 배선](../design/v2-48-opencode-wiring_2026-07-11.md)(업스트림 정찰 확정: 워커=낮음·감독=codex보다 유리, TUI 포트는 --port/config로 고정 가능, 최대 리스크=SQLite 전환 직후 스키마 - 착수는 v2-45 뒤+냉각 후).
- **/handoff 전역 스킬**: win-codex-home에 A2A task(da1dabab)로 위임 → `~/.claude/commands/handoff.md` 생성 완료·검수 통과. **이번 마감부터 모든 프로젝트에서 /handoff 사용 가능**(규약 탐지→핸드오프 작성→포인터·체크리스트 갱신→커밋→/clear 안내).
- 부수: 어텐션 핑 5/5(감독 세션 전원 goal 경로 왕복 = 240분 유휴 리셋+수신 경로 전수 점검), P8 승격(마커 pid 생존, 하트비트 주입안 비채택 근거 기록).

## 2. 사용자 결정 (재론 금지)

- **대시보드 = 관제탑 충실.** 뷰(로스터·피드) + 목표 제출만. 직접 제어 UX 제거·비확장. 웹 goal의 human 신호 승격 비채택(★=TUI 자리 기준).
- **세션 분할 계획**: 이번 세션 P3까지 → P4·P5 → P6a·b → P7·P8 각 한 세션. 컨텍스트 70%쯤에서 사용자가 핸드오프 결정(자동 컴팩션은 손실 압축이라 신뢰하지 않음 - 사용자 실측).
- opencode 착수는 v2-45 완료 + 스키마 냉각 후(v2-48 §4).
- 240분 유휴 드롭 = 주기 하트비트 비채택, P8(0토큰 pid 확인)로.

## 3. 사고·교훈 (오진 방지, 다음 세션 필독)

- **mesh 전멸 사건 2회분 규명**: 진범 = P2 구현 에이전트가 스모크 정리로 실행한 `taskkill //F //IM tunaround.exe`(이름 전수 종료 = Job 경계 무관, 타 세션 poll까지 사망). 1차 오진(하네스 Job 정리)은 정정됨. **서브에이전트 스모크 지시에 "임시 브로커는 PID로만 종료" 명시 필수**(P3 프롬프트부터 적용 중). 메모리 `mesh-restart-needs-job-escape` 참조.
- **배포 재기동은 WMI 스폰으로**: `Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{CommandLine='pwsh -NoProfile -File D:\privateProject\tunaRound\scripts\restart-win-mesh.ps1 [-SourceBin ...]'}` - 하네스 태스크 수명과 분리(생존 검증됨). 하네스 background 태스크로 직접 돌리지 말 것.
- **DeepSource 스킵 계열 확장**: RS-W1079(String::new()=표준 관용구)·RS-W1007(2-arm match)·JS-0067(ES 모듈 스코프 오판). clippy -D warnings 클린이 이 레포의 lint 정본. C 백로그 .deepsource.toml 튜닝 때 억제 목록에 등재.
- **P 단계 파이프라인 관례**(그대로 반복하면 됨): 워크플로우(worktree 구현 + 적대 리뷰, 스펙은 설계 §4·§5 인용) → 통합자 diff 검토·후속 수정 → push·PR(이월 사항 본문 명시) → CI Monitor → 봇리뷰 전수(반영 or 사유 코멘트 스킵) → 머지 → npm build+cargo build+WMI 재기동 → 라이브 검증 → checklist 체크·커밋.
- tuna-broker MCP 도구가 세션에 미로드일 때 디스패치 = loopback `POST /dashboard/goal {text,targets:[uuid]}`(curl, python urllib은 행 실측). 종결 감시 = broker.db read-only 폴링 Monitor 패턴(이 세션에서 2회 실증).

## 4. P3 상태 (핸드오프 시점 = 워크플로우 진행 중)

- 브랜치 `feat/v2-45-p3-watch-results-replay` 생성됨(세션 종료 시점 기준 커밋 유무를 `git log main..feat/v2-45-p3-watch-results-replay`로 확인할 것).
- **커밋이 있으면**: 구현 결과·리뷰가 담긴 것 - diff 검토 후 §1의 파이프라인 관례대로 PR부터 진행.
- **커밋이 없으면(워크플로우가 세션 종료로 중단)**: 스크립트 파일이 세션 폴더에 영속되어 있으나 캐시 재개는 같은 세션 한정이라 재실행 필요. 스펙 정본 = 설계 §4 P3 + checklist P3 항목(**P2 리뷰 이월 3건 포함**: since 상한 500+잘림 시 chain 없이 종료=catch-up 연쇄 / since 'T' 정규화 / Feed 비연속 중복은 관찰 후 판단). 리뷰 계약 포인트 = 콜드스타트(상태 파일 없음)=현행 무파라미터 구독 그대로, 워터마크=서버 updatedAt만, 재접속마다 URL 재구성.

## 5. 다음 세션 첫 행동 (Opus 4.8)

1. `git pull` + 설계 [v2-45](../design/v2-45-mesh-persistence-and-replay_2026-07-11.md) §5 고정 계약 재확인. 수신은 훅이 자동 안내.
2. **P3 마무리**(§4의 분기대로) → 머지·배포(WMI)·라이브 검증까지. 여유 있으면 세션 계획의 다음 묶음(P4·P5)은 새 세션에서.
3. mesh가 죽어 있으면 `pwsh -File scripts\restart-win-mesh.ps1`(사용자 터미널) 또는 §3의 WMI 스폰. 절대 tunaround를 이름으로 전수 종료하지 말 것.
4. 세션 마감 = `/handoff` 스킬 사용 가능(이 세션이 만든 것).
