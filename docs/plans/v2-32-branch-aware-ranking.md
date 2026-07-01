# Plan v2-32: 분기/세션 인지 검색 랭킹 (로드맵 step 5b)

> step 5에서 분리. 아키텍트 리뷰 약점3(검색이 분기 비인지 → 버려진 분기 주입).

## 설계

repl은 현재 활성경로 콘텐츠를 retrieved에서 이미 dedup 제외한다. 따라서 **현재 세션에 남은 검색 히트 ≈ off-branch(checkout으로 버려진 분기)**. 이를 디프리오리티한다.

- ContextRetriever에 `retrieve_ctx(query, limit, current_session)` **default 메서드**(기본은 retrieve 위임) 추가 → 다른 impl/호출부 ripple 없음. SqliteRetriever만 override.
- rerank를 penalty 기반 안정 정렬로 통합: rejected 드롭 / superseded·stale +2 / 현재 세션(off-branch) +1. 같은 penalty 내 relevance 순서 보존(안정 정렬).
- repl retrieve_for_from_path가 retrieve_ctx(topic, K, &session_id) 호출. MCP search_context는 retrieve(cross-session, 컨텍스트 없음) 유지.

## recency 후속

메시지 타임스탬프 컬럼이 없어 cross-session recency 불가. msg_id는 세션별이라 비교 불가. → messages에 created_at 추가 후 별도(step 5c). 지금은 branch/session만.

## 테스트

- retrieve_ctx: 현재 세션 off-branch 히트가 타 세션 히트보다 뒤로. 컨텍스트 없는 retrieve는 불변.

## 범위

orchestrator(retrieve_ctx default), store/retriever.rs(retrieve_impl+penalty rerank), repl(retrieve_ctx 호출). 컨텍스트 미전달 시 동작 불변.
