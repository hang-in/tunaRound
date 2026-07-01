# Plan v2-31: 유효성 인지 검색 랭킹 + 지정 커맨드 (로드맵 step 5)

> step 4(message_validity)의 페이오프. "현재 유효한 기억 우선, 폐기된 것 배제".

## 범위 (step 5 = valid_state 축)

valid_state가 헤드라인. recency/current-session/active-branch 가중은 retriever에 컨텍스트 전달(트레잇 변경)이 필요해 **step 5b로 분리**. 여기선 valid_state 랭킹 + 사람 지정 커맨드.

## 랭킹 (SqliteRetriever)

후보(over-fetch된 FTS·RRF 순서 목록)에 유효성 재랭크 적용:
- **rejected → 드롭**(폐기된 결정은 주입 안 함).
- **superseded/stale → 디프리오리티**(순서 유지하되 active 뒤로 강등).
- **active/unknown/없음 → 유지**(기본).
파이프라인: 후보 → 유효성 조회 → rejected 드롭 + active-우선/demoted-후순 파티션 → 세션 다양성 cap(step 3) → limit.

## 커맨드 (사람이 유효성 지정, HITL)

- `/supersede <id> [<by_id>]` → set_validity(sid, id, "superseded", by_id).
- `/reject <id>` → set_validity(sid, id, "rejected", None).
- 배선: `ValiditySink` 트레잇(set_validity) + SqliteValiditySink + Session.validity_sink(Option) + main이 --db로 배선. 미배선(--db 없음)이면 안내.

## 테스트

- 재랭크 순수/통합: rejected 드롭, superseded 강등, active 유지.
- 커맨드 파싱 + step이 sink 호출(fake sink).

## 범위

store/retriever.rs(재랭크), orchestrator(ValiditySink), store/retriever.rs(SqliteValiditySink), repl(커맨드+sink), main(배선). 유효성 미설정 시 동작 불변(전부 active).

## step 5b (후속)

retrieve에 current_session/active_path 컨텍스트 전달 → recency·current-session·active-branch 가중. abstraction/anchors 생성·활용.
