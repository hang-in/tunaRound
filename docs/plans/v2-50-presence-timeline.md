# v2-50 진짜 presence 타임라인 (세션 등장·소멸 + human_input 이력)

> 관제탑(대시보드)은 지금 "현재 스냅샷"만 보여줍니다. 세션이 언제 나타났다 사라졌는지, ★(사람 입력)가
> 언제 어디로 움직였는지의 **이력**이 없습니다. 이 브랜치(`feat/presence-timeline`)는 presence의 edge
> (등장·소멸·사람입력)를 raw 이벤트로 영속하고 read-only 타임라인 패널로 노출합니다.

## 목표

- presence의 세 가지 edge를 순수 raw 이벤트로 기록합니다. appear(세션 등장), disappear(소멸,
  사유 stale·deregister), human_input(사람 입력 전진)의 세 종류입니다.
- 대시보드에 read-only 타임라인 패널을 붙여 "언제 무엇이 나타났다 사라졌나 + ★가 언제 움직였나"를
  보여줍니다.
- 관제탑 원칙을 유지합니다. 백엔드는 raw 이벤트만 기록하고 ★-도출(autoBoss) 로직은 기존 프론트
  `activity.ts`가 단일 소스로 유지합니다(백엔드에 ★ 판정을 넣지 않습니다).

## 비목표

- ★-도출·총감독 판정을 백엔드로 옮기지 않습니다(프론트 단일 소스 유지).
- 이벤트 편집·삭제 UI, 알림, 검색은 범위 밖입니다(read-only 뷰).
- 기존 스냅샷(roster·health·feed) 동작은 바꾸지 않습니다(additive).

## 설계 요약

- **저장**은 신규 테이블 `presence_events`(스키마 v11)를 씁니다. 순수 append-only이며 30일 보존 GC를
  둡니다. 기존 `agent_human_input`(v9)이 "최신 ★ 단일 값"만 유지하는 것과 달리, 이건 **이력 전체**를
  남깁니다.
- **기록 지점**은 edge만 남겨 스팸을 피합니다. 세부는 아래와 같습니다.
  - `sync_presence`는 보고 세션 중 직전 roster에 없던 uuid를 `appear`로, stale 제거된 각 uuid를
    `disappear`(stale)로 기록합니다. human_input_at이 실제로 전진할 때만 `human_input`을 남깁니다
    (매 heartbeat가 아닙니다).
  - `register_agent`도 roster 부재→존재 첫 진입이면 `appear`를 1회 남깁니다(disappear와 대칭).
  - `deregister_agent`는 성공 시 `disappear`(deregister)를 남깁니다.
  - `mark_human_input`은 값이 실제로 전진(now > 직전)할 때만 `human_input`을 남깁니다(claude
    human-ping 경로). 인메모리 ★ 갱신도 전진일 때만 하여 되감김을 막습니다.
- **best-effort** 원칙을 지킵니다. `log_presence_event`는 실패해도 로스터·통지를 막지 않습니다
  (`persist_human_input` 규약을 답습합니다).
- **조회**는 `list_presence_events(since, limit)`로 하며 `ORDER BY at DESC, id DESC`입니다. 상한
  캡으로 무인증 원격 관전자를 방어합니다.
- **엔드포인트**는 `GET /dashboard/presence-timeline?limit=&since=`이며 health 핸들러 패턴을
  따릅니다(spawn_blocking, serde_json, 실패는 500으로 표면화, 정상 0 위장 금지).
- **프론트**는 `PresenceTimeline.tsx`로 HealthPanel 폴 패턴을 씁니다. 시간 역순 리스트이며
  appear=녹색·disappear=회색·human_input=★로 표시하고 relativeTime을 재사용합니다.

## 단계 체크리스트

- [x] 1) 스키마 v11. `CURRENT_SCHEMA_VERSION` 10→11, `presence_events` 테이블·인덱스 CREATE
  (IF NOT EXISTS additive), migrate 배선, v10→v11 마이그레이션 테스트를 넣습니다.
- [x] 2) edge 로깅. `log_presence_event` best-effort 헬퍼, `PresenceEvent` 모델,
  `sync_presence`·`register_agent`·`deregister_agent`·`mark_human_input` 기록,
  `gc_presence_events`(30일), `list_presence_events`, 단위 테스트를 넣습니다.
- [x] 3) 엔드포인트. `dashboard_presence_timeline_handler`, 라우트 등록, e2e 테스트를 넣습니다.
- [x] 4) 프론트. api 타입·호출, `PresenceTimeline.tsx`, App 배치, index.css(반응형 편입)를 넣습니다.
- [x] 5) 검증. build, clippy -D warnings, test, `npm run build`를 모두 green으로 확인합니다.
- [x] 6) 자가 적대 리뷰. 마이그레이션 안전·스팸 방지·best-effort·fail-visible·read-only 원칙을
  점검합니다.

## 알려진 한계 / 후속

- **스캐너 flap이 disappear+appear 쌍 스팸과 ★ 회귀로 보입니다.** 스캐너가 일시적으로 부분보고를 하면
  살아있는 세션이 stale로 오제거되었다가 다음 주기에 다시 보고되어, 타임라인에 `disappear(stale)` 다음
  `appear` 쌍(그리고 ★가 잠깐 사라졌다 돌아오는 회귀)이 남습니다. **근본원인은 기존 스캐너 flap**이며
  (코어의 stale 제거·★ 삭제는 이 브랜치 이전부터의 동작입니다) 이 브랜치는 그 전이를 raw-edge로
  **가시화만** 합니다. raw-edge 정직성상 flap이 그대로 보이는 것은 의도된 성질입니다.
- **stale 제거 로직(핵심 mesh 동작)은 이 브랜치에서 건드리지 않습니다.** 실사용에서 flap 노이즈가 크면
  후속으로 debounce(연속 N주기 결측을 확인한 뒤에야 disappear 확정)를 검토합니다. **지금은
  미착수입니다**(raw 기록이 먼저이고, 노이즈 완화는 실측 후에 다룹니다).

## 검증 명령

- `cargo build --features "morphology mcp serve worker dashboard"`
- `cargo clippy --features "morphology mcp serve worker dashboard" -- -D warnings`
- `cargo test --features "morphology mcp serve worker dashboard"`
- `cd frontend && npm run build`
