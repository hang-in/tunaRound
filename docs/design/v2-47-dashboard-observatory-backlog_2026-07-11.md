# v2-47: 대시보드 관제탑 고도화 백로그 (2026-07-11, 세션21 제안 문서화)

> 진행(2026-07-12, 세션22): **#1·#2 완료**(PR #67), **#3·#4 완료**(PR #68), **#5 완료**(PR #69). v2-47 주 항목 5건 완주. 남음 = 낮은 우선순위 2건(기록만).
> 상태 = 백로그(사용자 "문서화 해두자" 2026-07-11). 착수 시점 미정, 의존 항목은 v2-45 진행에 따름.
> 전제 원칙(세션21 사용자 확정) = **대시보드는 관제탑에 충실한다.** 뷰(로스터·피드) + 목표 제출(위임 티켓 발행)만. 직접 제어 UX는 v2-45 P0에서 제거했고 다시 늘리지 않는다. 아래 항목은 전부 read-only 뷰 강화라 이 원칙과 충돌하지 않는다.

## 단기 (v2-45 P2 머지 직후, 프론트 위주)

### 1. task 카드 상세 펼침 강화
- 문제: 피드 카드가 요약 수준. 결과 전문을 보려면 터미널에서 `get_task` 재조회(watch-results는 160자 절단).
- 해법: SSE task 스냅샷에 이미 실려 오는 요청 원문(message/history)·결과 artifacts 전문·실패 사유(statusMessage)를 카드 펼침에서 표시.
- 변경: 프론트 전용(Feed.tsx 카드 확장). 서버 무변경.

### 2. 피드 필터 칩
- 문제: P2의 `?replay=50` 도입 후 피드가 길어져 스캔 비용 증가.
- 해법: dispatcher·상태(완료/실패/진행)·머신·러너별 필터 칩 + 간단 텍스트 필터(클라이언트 사이드).
- 변경: 프론트 전용.

## 중기 (서버 소수정 포함)

### 3. 브로커 헬스 패널 (완료, PR #68)
- 문제: mesh 건강 상태(미배달·고착 task, 머신 도달성)를 보려면 MCP `tasks()` 호출이나 터미널 필요.
- 해법: `tasks()`가 이미 계산하는 no-consumer/stuck 주석 + 열린 task 수 + 스캐너 heartbeat 나이(머신 도달성) + WAL 크기·브로커 기동 시각을 한 패널로. read-only GET 1개(기존 roster 핸들러 패턴).
- 가치: "mesh가 지금 건강한가" 한눈 파악 = 관제탑의 본질 기능.
- **구현(PR #68)**: `GET /dashboard/health` = 열린 task 수 + no-consumer/stuck 집계(`classify_task_health` 단일 소스, `tasks()`와 동일 임계) + 머신별 스캐너 도달성.
- **후속 구현(세션23)**: **WAL 크기·브로커 uptime 추가.** store 표면 변경 = `SqliteStore.db_path` 필드 + `get_config`/`set_config`(기존 config 테이블 재사용, 마이그레이션 불요) + `wal_bytes()`. uptime = `serve_http_mcp_on_listener`(serve/core/node 단일 깔때기) 기동 시 `broker_started_at` config row 기록(매 기동 덮어씀) → 헬스 핸들러가 `age_secs(now, started)`로 계산. WAL = `<db>-wal` stat(부재=체크포인트됨=0, 실 IO 오류만 500). uptime·WAL은 **임계 없는 raw 게이지**(task-health 아님). fail-visible 유지(조회 오류는 500 표면화).

### 4. 브라우저 알림 (옵트인) (완료, PR #68)
- 해법: task 완료/실패 SSE 수신 시 Notification API 데스크톱 알림(토글, 기본 off).
- 성격: 제어가 아니라 관제탑이 소리를 내는 것. 총괄이 대시보드를 띄워두는 사용 패턴과 정합.
- 변경: 프론트 전용.
- **구현(PR #68)**: 헤더 토글(권한 승인 시에만 켜짐·localStorage 기억). 이 세션에서 non-terminal 로 관측했던 task의 완료/실패 전이일 때만 발화 = `?replay=50` 과거 스냅샷·EventSource 재접속 re-send 무음(적대적 리뷰로 전 시퀀스 확증). `tag=id`로 같은 task 겹침.

## P6a(mesh 기억화) 이후

### 5. 위임 이력 검색 탭 (완료, PR #69)
- 전제: v2-45 P6a가 task 결과를 messages/FTS로 색인(`a2a:*` 네임스페이스).
- 해법: `/dashboard/search?q=` read-only 엔드포인트(retriever 재사용) + 검색 탭. "지난주 mac에 맡긴 진단이 뭐였지"를 웹에서.
- 가치: mesh 기억화의 가치가 사용자에게 보이는 지점 = P6a의 완성.
- **구현(PR #69)**: `GET /dashboard/search?q=`(별도 retriever-state 서브라우터를 merge - 기존 store-state 핸들러 무영향). MCP search_context와 **같은 retriever**(형태소+FTS) 재사용. 배포 바이너리는 semantic 미포함이라 **embedder(원격 Ollama) 네트워크 비의존**. 프론트 `SearchPanel`(디바운스 400ms, speaker=`a2a/<agent>` 표시). 실패는 500으로 표면화(결과 없음 위장 안 함). 탭 네비게이션 대신 자체 완결 섹션.

## 낮은 우선순위

### ★ 이동 이력·세션 등장/소멸 타임라인 (모디스트 버전 완료 / 진짜 타임라인 defer)

- **가정 정정(세션23)**: "P4의 agent_human_input 테이블이 생기면 ★ 이력 데이터는 공짜"는 **틀렸다.** agent_human_input은 `(uuid PK, at)` = uuid당 최신 1행(단조 UPSERT)이고 세션 소멸(stale/deregister) 시 **DELETE** + 7일 GC라 과거를 능동적으로 버린다. 스키마 어디에도 세션 등장/소멸·★ 이동 event-log가 없다. 진짜 타임라인은 **새 append-only presence_events 테이블(스키마 v11) + 3~4곳 edge-detect 로깅 + endpoint + 패널**이 필요하고, **백필 불가(도입 시점부터 빈 상태로 시작)**이며, v9 주석의 "로스터 이력 비영속(유령 카드 방지, 설계 §2 비스코프)" 결정을 **뒤집는다** → 제품 결정 없이 단독 강행 부적절. **defer**(사용자가 영속 presence 감사를 명시적으로 원하고 "빈 상태 시작 + 스코프 반전"을 수용할 때).
- **모디스트 버전(완료, 세션23)**: 기존 데이터(roster의 human_input_at + last_heartbeat)로 각 세션에 "★ 마지막 N분 전"을 표시(비-총괄 행) = 이력 테이블 없이 "★가 거쳐간 자리"를 한눈에. 순수 프론트(Roster.tsx). 진짜 "무엇이 언제 일어났나"는 이미 tasks 테이블(created_at/updated_at/state)이 피드(`?replay=50`)·검색(P6a)으로 노출한다.

### 원격 관전 모드 다듬기 (완료, 세션23)

- **뱃지 노출 강화 + 모바일 반응형.** 원격=read-only 403 태세는 불변. 관전 뱃지는 이미 있었고(Header `remoteViewer`), **eye 아이콘 + 안내 title 추가**로 강화. index.css에 폭 기반 `@media (max-width:640px)` 신설: 좁은 화면에서 로스터·피드 단일 컬럼 스택(min-width:0/flex-basis:100%) + 패딩 축소로 body 가로 오버플로 제거.
- **옵션(비착수)**: `GET /dashboard/whoami {loopback}` 권위 신호. 현재 프론트는 hostname 휴리스틱(remoteViewer=!loopback host)으로 판정 - 대개 맞으나 SSH 터널·dual-stack에서 서버 게이트(to_canonical().is_loopback())와 어긋날 수 있다. "노출 강화"보다 "판정 정확화"라 원항목 스코프 밖 = 별도 소항목.

## 권고 순서

1·2(P2 직후 프론트 소 PR) → 3·4 → 5(P6a에 묶음). 낮은 우선순위 2건은 필요가 생길 때.
