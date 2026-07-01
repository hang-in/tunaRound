# Plan v2-29: retrieved 주입 길이 cap + session diversity cap (로드맵 step 3)

> docs/design/v2-temporal-validity-direction_2026-07-01.md step 3. 기반 안정화.

## 문제

- retrieved(검색) 주입은 개수(RETRIEVE_K=5)만 cap이고 **글자수 무제한** → 긴 발언 5개면 프롬프트 팽창(carried만 MAX_CARRY=1500 있음).
- **session diversity 없음**: 한 verbose 세션이 top-5 독점 가능.

## 핵심 뉘앙스

tunaRound 토론은 보통 **단일 세션**(한 토론). 세션당 무조건 cap하면 같은 세션 결과가 줄어 손해. → **다양성 우선 + backfill**: 다른 세션 결과가 있으면 다양하게 채우고, 없으면(단일 세션) 그 세션으로 limit까지 채운다. 단일 세션 동작 불변.

## 설계

### session diversity (SqliteRetriever)
- store.search/vector_search를 `limit * OVERFETCH`(4)로 **over-fetch**.
- `cap_per_session_backfill(candidates: Vec<(session_id, T)>, max_per_session=2, limit)`: 1차로 세션당 max_per_session(순서 보존)=primary, 나머지=overflow. `primary.chain(overflow).take(limit)`. 다양성 우선이되 부족하면 backfill로 limit 채움. 단일 세션이면 결과 동일.
- FTS 단독 경로와 RRF 경로 모두 적용(둘 다 session_id 보유).

### retrieved 길이 cap (repl)
- `MAX_RETRIEVED_CHARS`(2000). retrieve_for_from_path에서 dedup 후 content 글자수 누적, 예산 초과 발언은 드롭(통째, UTF-8 안전). carried의 MAX_CARRY 패턴 답습.

## 테스트

- cap_per_session_backfill 순수 단위: 다중 세션 다양성, 단일 세션 full-fill, backfill 동작.
- retriever: 단일 세션은 결과 수 불변(회귀 가드).
- repl: 긴 retrieved가 MAX_RETRIEVED_CHARS로 잘림(개수도 함께).

## 범위

store/mod.rs(cap 헬퍼), store/retriever.rs(over-fetch+cap), repl/mod.rs(길이 cap). eval 하네스는 store.search 직접 호출이라 무영향. 단일 세션 behavior-preserving.
