# v2-50 진짜 presence 타임라인 (세션 등장·소멸 + human_input 이력)

> 관제탑(대시보드)은 지금 "현재 스냅샷"만 보여준다. 세션이 언제 나타났다 사라졌는지, ★(사람 입력)가
> 언제 어디로 움직였는지의 **이력**이 없다. 이 브랜치(`feat/presence-timeline`)는 presence의 edge
> (등장/소멸/사람입력)를 raw 이벤트로 영속하고 read-only 타임라인 패널로 노출한다.

## 목표

- presence의 세 가지 edge를 순수 raw 이벤트로 기록한다: `appear`(세션 등장) · `disappear`(소멸,
  사유=stale|deregister) · `human_input`(사람 입력 전진).
- 대시보드에 read-only 타임라인 패널을 붙여 "언제 무엇이 나타났다 사라졌나 + ★가 언제 움직였나"를 본다.
- 관제탑 원칙 유지: 백엔드는 raw 이벤트만 기록하고 ★-도출(autoBoss) 로직은 기존 프론트
  `activity.ts`가 단일 소스로 유지한다(백엔드에 ★ 판정 넣지 않음).

## 비목표

- ★-도출/총감독 판정을 백엔드로 옮기지 않는다(프론트 단일 소스 유지).
- 이벤트 편집·삭제 UI, 알림, 검색은 범위 밖(read-only 뷰).
- 기존 스냅샷(roster/health/feed) 동작 변경 없음(additive).

## 설계 요약

- **저장**: 신규 테이블 `presence_events`(스키마 v11). 순수 append-only, 30일 보존 GC.
  기존 `agent_human_input`(v9)이 "최신 ★ 단일 값"만 유지하는 것과 달리, 이건 **이력 전체**를 남긴다.
- **기록 지점**(edge만, 스팸 방지):
  - `sync_presence`: 보고 세션 중 직전 roster에 없던 uuid = `appear`. stale 제거 각 uuid = `disappear`(stale).
    human_input_at이 실제로 전진할 때만 `human_input`(매 heartbeat 아님).
  - `deregister_agent`: 성공 시 `disappear`(deregister).
  - `mark_human_input`: 값이 실제로 전진(now > 직전)할 때만 `human_input`(claude human-ping 경로).
- **best-effort**: `log_presence_event`는 실패해도 로스터/통지를 막지 않는다(`persist_human_input` 규약 답습).
- **조회**: `list_presence_events(since, limit)` = `ORDER BY at DESC, id DESC`. 상한 캡으로 무인증
  원격 관전자 방어.
- **엔드포인트**: `GET /dashboard/presence-timeline?limit=&since=` = health 핸들러 패턴(spawn_blocking,
  serde_json, 실패는 500으로 표면화, 정상 0 위장 금지).
- **프론트**: `PresenceTimeline.tsx` = HealthPanel 폴 패턴, 시간 역순 리스트, appear=녹색·disappear=회색·
  human_input=★, relativeTime 재사용.

## 단계 체크리스트

- [ ] 1) 스키마 v11: `CURRENT_SCHEMA_VERSION` 10→11 + `presence_events` 테이블/인덱스 CREATE
  (IF NOT EXISTS additive) + migrate 배선 + v10→v11 마이그레이션 테스트.
- [ ] 2) edge 로깅: `log_presence_event` best-effort 헬퍼 + `PresenceEvent` 모델 +
  `sync_presence`/`deregister_agent`/`mark_human_input` 기록 + `gc_presence_events`(30일) +
  `list_presence_events` + 단위 테스트(appear/disappear/human_input/list/GC).
- [ ] 3) 엔드포인트: `dashboard_presence_timeline_handler` + 라우트 등록 + e2e 테스트.
- [ ] 4) 프론트: api 타입/호출 + `PresenceTimeline.tsx` + App 배치 + index.css(반응형 편입).
- [ ] 5) 검증: build / clippy -D warnings / test / `npm run build` 전부 green.
- [ ] 6) 자가 적대 리뷰(마이그레이션 안전·스팸 방지·best-effort·fail-visible·read-only 원칙).

## 알려진 한계 / 후속

- **스캐너 flap = disappear+appear 쌍 스팸 + ★ 회귀 가시화.** 스캐너가 일시적 부분보고를 하면
  살아있는 세션이 stale로 오제거되었다가 다음 주기에 다시 보고되어, 타임라인에 `disappear(stale)` →
  `appear` 쌍(+ ★가 잠깐 사라졌다 돌아오는 회귀)이 남는다. **근본원인은 기존 스캐너 flap**(코어의
  stale 제거·★ 삭제는 이 브랜치 이전부터의 동작)이고, 이 브랜치는 그 전이를 raw-edge로 **가시화만**
  한다. raw-edge 정직성상 flap이 그대로 보이는 것은 의도된 성질이다.
- **stale 제거 로직(핵심 mesh 동작)은 이 브랜치에서 건드리지 않는다.** 실사용에서 flap 노이즈가 크면
  후속으로 debounce(연속 N주기 결측을 확인한 뒤에야 disappear 확정)를 검토한다. **지금은 미착수**
  (raw 기록이 먼저, 노이즈 완화는 실측 후).

## 검증 명령

- `cargo build --features "morphology mcp serve worker dashboard"`
- `cargo clippy --features "morphology mcp serve worker dashboard" -- -D warnings`
- `cargo test --features "morphology mcp serve worker dashboard"`
- `cd frontend && npm run build`
