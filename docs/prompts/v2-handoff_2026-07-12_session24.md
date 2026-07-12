# 세션24 핸드오프 (2026-07-12) - v2-48 재대조·기능2·품질게이트·리팩토링백로그·대시보드 전면 재편·도그푸딩

> 진입점: 이 파일 먼저 → 다음 목표 = **v0.5.0 릴리즈(도그푸딩 후)** + **v2-52 리팩토링 백로그**. 이전 세션: [세션23](v2-handoff_2026-07-12_session23.md).

## 한 줄 요약

세션23이 남긴 "v2-48 opencode 감독 레인"으로 시작했으나 재대조 결과 defer 유지 → 사용자 지시로 **presence 타임라인 + 큐레이션 기억 두 기능 병렬 구현 → 품질게이트 → 대시보드 전면 재편**으로 확장. **PR #76~#82 7개 머지 + WMI 도그푸딩 배포(broker.db v10→v11) + 라이브 검증(라이트/다크) + 생존확인 5/5**. main=`d6774ac`, 미커밋 없음, 열린 PR/브랜치 없음(스테일 `session17`만 잔여). **v0.5.0 릴리즈는 도그푸딩 후로 유예**(사용자 결정).

## 완료 사항 (전부 머지·CI green·적대+봇 리뷰 반영)

| PR | 머지 | 항목 |
|----|------|------|
| #76 | `652addd` | **v2-48 감독 레인 재대조**: 3축 병렬 정찰 → still_churning, defer 유지. §0 배너에 트립와이어 기록 |
| #77 | `fb9b209` | **v2-50 presence 타임라인**: 스키마 **v11** `presence_events`(append-only) + edge 로깅 + `/dashboard/presence-timeline` + 대시보드 패널 |
| #78 | `55b2746` | **v2-51 큐레이션 기억**: `/annotate` + AnnotationSink + retriever abstraction 표면화·anchor 부스트. **스키마 불변** |
| #79 | (docs) | CHANGELOG `[Unreleased]`에 presence 타임라인·큐레이션 기억 |
| #80 | `d47968a` | **v2-52 리팩토링 백로그 문서**(Codex 전수조사 기반) |
| #81 | `4fa4128` | **품질 게이트**: clippy `--all-targets` 3건 + exec.rs Windows 이식성(`#[cfg(not(windows))]`) |
| #82 | `d6774ac` | **v2-53 대시보드 전면 재편** |

## 각 기능 상세

- **presence 타임라인(#77)**: 세션 등장/소멸/human_input을 append-only 영속(스키마 v11, `CREATE TABLE IF NOT EXISTS` additive, v9 선례). edge 로깅=`sync_presence`(appear/disappear)·`deregister_agent`·`register_agent`(대칭)·`mark_human_input`(전진 게이트). ★-도출은 프론트 activity.ts 단일소스 유지(백엔드 raw 로깅만). GC 30일. **적대+봇 리뷰 실이슈**: appear/disappear 비대칭·`mark_human_input` human_input_at 되감김(CodeRabbit MAJOR, advanced 게이트 안으로)·폴 레이스(AbortController).
- **큐레이션 기억(#78)**: 이미 존재하나 죽어있던 `message_validity.abstraction/anchors`(v4) 활성화. raw cross-session RAG는 이미 완성(v2-11·P6a)이라 순수 델타=증류 결정 기억만. **적대+봇 실이슈**: 이중주입 회귀(retriever가 content 변형→repl dedup 깨짐 → Utterance.abstraction 별도필드+렌더경계 표면화로 구조적 수정)·토크나이저 불일치(`!is_alphanumeric` 통일)·MAX_ANSWER_LEN 캡·플래그 파싱.
- **품질게이트(#81)**: canonical CI가 `--all-targets`·`fmt --check` 미실행이라 놓친 기존 이슈. exec.rs 테스트가 `sh` 하드코딩→OS 인지형(Unix sh / Windows cmd). **fmt 전역 드리프트 1008 hunk는 v2-52로 defer**(mac 조율).
- **대시보드 재편(#82, v2-53)**: 목업(확정) → React. **3층 IArch**: 헤더(tunaRound v{ver}·omnisearch 위임검색·목표제출 모달·테마토글·알림·연결·시계) / [사이드바 로스터 | 본문(서버소스 요약타일·전폭 피드+드롭다운 필터 50→200·presence 로그)] / 푸터(헬스 좌·스캐너 우, 타이틀 제거). lucide-react 도입, 브레싱+하트비트 그린닷, 테마토글(OS+수동·localStorage·pre-paint 스크립트). 백엔드=health에 `version`·`task_counts{working,completed,failed}`(StatTiles 서버소스화, `count_by_state`). SearchPanel·HealthPanel·WorkerSection 삭제(흡수). **적대 리뷰 MAJOR**=StatTiles fail-visible 회귀(healthOk 미전달→옛 값 라이브 위장, `ok` prop+stale 표시로 수정) + MINOR/NIT(FOUC·모바일 푸터·드롭다운 z·모달 포커스트랩) 반영.

## 배포·검증

- **WMI 도그푸딩 배포 2회**(기능 배치 후, 대시보드 재편 후). `restart-win-mesh.ps1 -SourceBin`을 **WMI 스폰**(하네스 job escape)으로 띄움. mesh.pids 선별 종료로 세션 poll 보존.
- **broker.db v10→v11 마이그레이션** 라이브 확인(presence_events 채워짐). health `version:"0.4.0"`·`task_counts{working:0,completed:122,failed:3}` 신규 필드 확인.
- **Chrome 라이브 검증**(http://127.0.0.1:8770/dashboard): 라이트/다크 양쪽 새 레이아웃 정상 렌더(사이드바·서버소스 타일·드롭다운 필터·presence 로그·푸터·테마토글·하트비트 닷).
- **v0.4.0 릴리즈 확인**(세션23 유예분): CI run 29171411699 green(전 잡), 릴리스 페이지 아티팩트 + brew `hang-in/tap` 0.4.0 발행.
- **생존확인 5/5**: mac-codex-home·win-codex-home·mac-claude-home·mac-claude-tunaRound·win-claude-luckyCAD 전원 claim→complete(무응답 0). `/dashboard/goal` loopback으로 대상별 1 task 발행(send_task MCP가 이 세션 도구목록에 없어 우회).

## 확정 결정·교훈 (재론 금지)

- **버전 = v0.5.0**(semver: 신규기능 2 + 스키마 v11 = minor). **릴리즈는 도그푸딩 후**(사용자 결정): presence 타임라인·/annotate·대시보드를 며칠 써보고 안정 확인 후 태그.
- **리팩토링 스코프**: P0 품질게이트만 이번 처리. 구조 P1-P2(main.rs·mcp.rs·tasks.rs god파일 분리, task 문자열→JSON, store DTO, fmt 전역+CI)는 **v2-52 백로그로 defer**(세션16식 전용 세션, mac 조율). 정본 [v2-52](../design/v2-52-refactoring-backlog_2026-07-12.md).
- **대시보드 = 관제탑 3층 IArch.** 로스터는 사이드바(피드와 높이 경쟁 해소). 요약숫자는 서버소스(리로드 안정). 필터=검색+드롭다운. 목표제출=헤더 모달. 위임검색=헤더 omnisearch.
- **"피드 리로드 리셋"은 결함 아님**: replay=200으로 복원됨. 착시=클라 파생 요약숫자였고 서버소스화로 해소. (조사 확정.)
- **v2-48 감독 레인 defer 유지**: 업스트림 스토리지 미성숙. 재검토 트립와이어 = #34922(V2 스키마 GA) 종료 + 07-10/11 세션가시성 클러스터(#36222·#36064·#36464·#36178) 해소 + 릴리즈 캐던스 주단위+ 둔화. 정본 [v2-48 §0 배너](../design/v2-48-opencode-wiring_2026-07-11.md).
- **메모리 정정**: 사용자 호칭 "사장님"·tunaRound "악기" 비유 disavow → 호칭 없는 존댓말. 실제 방향=팔지 않되 OSS 공개해 필요한 사람이 씀([[how-to-address-user]]·[[tunaround-north-star]] 갱신).

## 다음 세션 첫 행동 (우선순위 순)

1. **v0.5.0 릴리즈** (도그푸딩 안정 확인 후): `cargo release minor` → v0.5.0 태그 → cargo-dist(4타깃+brew) → 맥 알림. **push·태그는 승인 후.** CHANGELOG `[Unreleased]`→`[0.5.0]` 확정.
2. **v2-52 리팩토링 백로그** (전용 세션들, 한 번에 하나): fmt 전역+CI 게이트(mac 조율) → god파일 분리(main.rs·mcp.rs·store/sqlite/tasks.rs) → task 문자열→JSON → store DTO. 정본 [v2-52](../design/v2-52-refactoring-backlog_2026-07-12.md).
3. **v2-48 감독 레인** = 트립와이어 충족 전 미착수(위 결정 참조). 착수 시 §1 표 재대조.
4. (선택) 스테일 브랜치 삭제: `git push origin --delete feat/session17-roster-supervision`.
5. 규율: 비trivial 전 plan + checklist·context-notes. 위임 tunaLlama→A2A codex→Sonnet, 아키텍트·리뷰=Opus. 커밋 자유, push·릴리즈는 승인.

## 미커밋·브랜치·백그라운드

- **미커밋: 없음.** main=`d6774ac`. 열린 PR: 없음. 열린 피처 브랜치: 없음. 미머지 잔여 = `origin/feat/session17-roster-supervision`(삭제 권고).
- **백그라운드**: WMI mesh 데몬 상주(broker 8770·app-server 8790·presence-scan·codex-relay·watch-results, 정상). 이 세션의 A2A 수신 Monitor는 재시작 시 새 세션이 SessionStart 훅으로 재무장.
- **배포 상태**: broker 0.4.0 바이너리(대시보드 재편·presence·annotate·품질게이트 전부 포함)가 라이브. broker.db v11.

## 검증 커맨드 참고

- 상태: `cargo test --features "morphology mcp serve worker dashboard"`(577 lib). CI clippy = `--all-targets` 포함해야 테스트 코드 idiom도 잡힘(v2-52로 CI 강화 예정).
- 대시보드: http://127.0.0.1:8770/dashboard. mesh 재부팅 복구 = `pwsh -File scripts\restart-win-mesh.ps1`. 배포 = 같은 스크립트 `-SourceBin`을 WMI 스폰.
