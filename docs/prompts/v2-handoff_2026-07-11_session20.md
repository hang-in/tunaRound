# 세션20 핸드오프 (2026-07-11): 재부팅 복구 1줄 + v2-46 codex-relay 완주 + T5 정리 + 다음=B(v2-45)·C

> 다음 세션 첫 행동: §5. 라이브 값 = backend-private 세션20 블록. 정본 설계 = [v2-46 codex-relay](../design/v2-46-codex-relay_2026-07-11.md).

## 0. 한 줄 요약

재부팅 대비(restart-win-mesh.ps1)와 v2-46 codex-relay(sup 정체성 폐기, 보이는 세션 thread 직주입)를 설계→구현→양 머신 라이브 이관까지 완주하고, T5 정리(alias·candidates/discover 제거)와 운영 견고화 A(세션 poll 생존·task get 본문·피드 이름)까지 닫았다. 남은 것 = B(v2-45 mesh 기억화 + watch-results 재구독·재생) · C(소규모 잔여).

## 1. PR 타임라인 (전부 CI green + 봇 리뷰 반영/근거 스킵 후 머지)

- **#53** 윈도우 mesh 재기동 스크립트(restart-win-mesh.ps1, 재부팅 복구 1줄) + presence 스냅샷 10초 데드라인(이 머신 tasklist가 부하 시 출력 0 무한 행 실측 - 스캔 루프 전체가 멈춰 win 세션 전멸하던 근본 원인) + 훅 Monitor 커맨드 경로 정규화(백슬래시가 Git Bash에서 exit 127).
- **#54** v2-46 codex-relay: 머신당 배달 데몬(로컬 codex 세션 열거=스캐너 SoR 공유 → 대리 claim → in-process 주입 → 실패=fail_task) + codex-inject `--thread` 직지정 + GoalForm codex 세션 카드 복귀(relay online 조건)·infra 대상 제외 + **유령 B석 tombstone**(SessionEnd가 .ctx 삭제 대신 "dead" 기록, 스캐너 Dead=스냅샷 무관 즉시 제외, ping 자가치유가 산 세션 복원). 라이브 E2E: win 56dd249b(403)·mac 2a09fab7. **mac 이관 = A2A task로 mac이 자율 수행**(eb9b2e2d).
- **#55** T5 정리: supervised→infra alias 제거(유예 종료) + candidates/discover 계열 전삭제(presence 스캐너가 대체) + a2a-usage §10 재작성(relay)·§0 infra 태그 규약 명문화 + README 현행화. mac 반영 = task ff31b212(자율 완료).
- **#56**(머지됨): 운영 견고화 A = restart 스크립트 **mesh.pids 선별 종료 + rename-swap**(세션 수신 poll이 재기동을 통과해 생존 - 2차 실행 실측) + `task get` 열린 task 본문 표시([요청], claim 후 재조회 마찰 제거) + 피드 uuid→로스터 이름 표시.

## 2. 오늘 실측·교훈 (다음 세션이 오진하지 않게)

- **재기동 후 타 세션 증상은 재기동 이력부터 대조**: luckyCAD "수십 분 후 exit 127" x3 = 내 재기동 3회(전수 종료가 세션 poll을 죽임). 코어 로그는 결백. → #56이 근본 해소. 메모리 `restart-script-kills-session-polls`.
- **세션19 "재배포 불필요" 오판 정정**: cargo test/clippy는 bin을 안 만든다 - 배포 전 `cargo build` 필수(tombstone 필터 빠진 채 배포했다 재배포한 실측).
- **mac은 dashboard 피처 빌드 불가가 정상**: rust-embed가 frontend/dist(gitignored)를 요구 - npm build를 한 win/CI만 됨. mac 데몬은 `--features "morphology mcp serve worker"`.
- **win 멀티클라이언트 attach 타임아웃(세션19 §4) = 해소 관찰**: 사용자 TUI가 보는 thread(019f4d64)에 주입 → 턴·complete·답이 사용자 화면에 표시되며 완주. 재현 시도 불요.
- **tunaCTX(claude-vault) 삭제됨**: "vec-daemon down" 경고는 정상 상태(사용자 의도 삭제). 복구 제안 금지. 메모리 `tunactx-removed-intentionally`.
- DeepSource 잔여 fail 2종은 알려진 스킵 계열: JS(모듈 함수를 전역 오염으로 오판·복잡도 6) / Rust(fn main 복잡도 146 = pre-existing, C-6에서 해소).

## 3. 다음 = B (v2-45 아크, 설계부터. 사용자 조사 지시 2건 포함)

1. **watch-results 재접속·재생**(실측 근거 2건): ① 브로커 재기동 시 SSE 단절로 watch-results가 종료(재접속 없음 - #56 검증 중 exit 1 실측) ② 인박스 다운 중 완료된 task 통지 유실(mac 완료 보고를 2회 수동 조회로 회수한 실측). 재생은 브로커 DB(SQLite)로 충분.
2. **대시보드 피드 초기 스냅샷**(사용자 지적, 코드 확정): /dashboard/events가 라이브 버스 구독만이라 리로드 시 피드 전멸. 접속 시 최근 task 스냅샷 선행(REST 또는 SSE 선행 프레임) 후 라이브 이어붙이기 - 1의 재생과 같은 메커니즘.
3. **총괄(★) 결함 2개**(사용자 지적, 코드 확정): (a) human_input_at이 인메모리 전용 → 브로커 재기동마다 ★ 증발(사용자가 claude 세션에 입력해야 복귀) → SQLite 영속 필요. (b) codex 세션 입력은 human-ping이 없어 ★를 못 옮김("TUI=전부 감독" 정본과 갭) → codex 입력 신호 승격 방식 결정(예: 스캐너가 rollout의 사람 턴 mtime을 human 신호로).
4. **mesh 기억화**: task 종결 시 결과를 messages/FTS로 색인(search_context로 위임 이력 검색) + 종결 task retention(색인 후 슬림화).
5. **Redis 완전 opt-out**(사용자 확정): observe 스냅샷도 SQLite로 흡수. 3(a)의 로스터/human_input_at 영속과 같은 방향.

## 4. 다음 = C (소규모 잔여, 우선순위 낮음)

- **마커 생존 유지 확장**(별도 PR, 3중 가드 필수): pid 살아있음+이름 검증 / 같은 pid 다중 마커는 mtime 최신만 / 마커 없음=창 폴백. 유휴-열림 세션 240분 드롭 해소.
- W4 codex thread 로테이션(codex-inject에 미구현).
- main.rs 잔여 세션 배선 분할(~560줄, DeepSource 복잡도 146 해소 겸).
- .deepsource.toml 튜닝(반복 오탐 억제) / R9 poll 견고화(옵션).

## 5. 다음 세션 첫 행동

1. `git pull`(#56까지 전부 머지 = d195657). 라이브 win 스택 = #56 빌드로 배포 완료(재배포 불요).
2. **B 착수 = 설계 문서 먼저**(규율 #7): v2-45(watch-results 재접속·재생 + mesh 기억화 + Redis opt-out). 재접속은 소수정이라 설계에서 분리해 먼저 PR 가능.
3. mesh가 죽어 있으면 `pwsh -File scripts\restart-win-mesh.ps1`(재부팅 복구 1줄. -SourceBin은 새 빌드 배포 시에만). 세션 수신은 훅이 자동 안내.
4. 로스터·피드 정상 여부는 대시보드에서 확인(피드가 이름으로 표시되는 건 #56 머지+npm build 반영 후).
