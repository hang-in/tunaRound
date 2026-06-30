# tunaRound 시간성·유효성 방향 (정본, 2026-07-01)

> 사용자(동구) + 아키텍트 리뷰 수렴 확정. 외부 memory 프레임워크(Zep/Graphiti·Mem0·Letta·Cognee) 검토 후 정정한 방향.
> 관련: docs/design/v2-context-memory-direction_2026-06-30.md(검색/맥락 북극성), Plan 27(3d 쓰기 권위).

## 한 줄

Graphiti를 따라가는 게 아니라, **Graphiti가 말하는 시간성 문제를 SQLite 컬럼 + 랭킹 가중치로 번역**한다. 인프라(graph DB·managed service)는 안 가고 개념만 흡수. tunaRound는 로컬-first·SQLite-light·사람-주도를 지킨다.

## 진단

- provenance는 이미 있음: `session_id`·`msg_id`·`parent_id`·`speaker`·branch/path. "이 발언이 어디서 왔는가"는 추적 가능.
- 빠진 것은 **"이 발언이 지금도 유효한가"** = valid_from/until·superseded_by. → 다음 단계는 provenance graph가 아니라 **validity metadata**.
- 참고 1순위 = **Memora**(원문/abstraction/cue anchor 분리). 인프라 안 바꾸고 SQLite 컬럼으로 바로 흡수 가능.

## 가져올 아이디어 (딱 둘)

### 1. 시간성·유효성 (Zep/Graphiti의 *개념*만)

질문: "이 기억은 언제부터 유효했고, 언제 폐기됐고, 무엇이 대체했는가?" SQLite로는 이 정도면 충분.

### 2. Memora식 원문/요약/앵커 분리

- content = 원문 발언
- abstraction = 결정 요약
- anchors = 검색 단서(모듈·기능·에러·설계쟁점)
- valid_state = 현재 유효성

## 최소 스키마 확장 (graph DB 없이)

`messages`에 추가(schema v2→v3 ALTER 마이그레이션):

```
abstraction          TEXT          -- 결정/발언 요약
anchors              TEXT          -- JSON array 또는 newline-separated 토큰
valid_state          TEXT          -- active | superseded | rejected | stale | unknown
superseded_by_msg_id INTEGER       -- 대체한 발언
valid_from_msg_id    INTEGER       -- (선택) 유효 시작
valid_until_msg_id   INTEGER       -- (선택) 유효 종료
embedding_model_id   TEXT          -- 임베딩 무효화 키(아래 #2)
embedding_dim        INTEGER
```

## 검색 랭킹 가중 (temporal KG 없이 "현재 유효한 기억" 근사)

```
base = FTS/BM25 또는 RRF score
boost: +현재 active branch  +현재 session  +recency  +valid_state=active  +anchors match
penalty: -superseded  -rejected  -abandoned branch  -stale  -타세션(명시 요청 없으면)
```

## 확정 작업 순서

1. **3d(post_turn 쓰기 권위) 마무리** — Plan 27, 옵션 B(front=core 병합). 진행 중.
2. **embedding 무효화 키에 model_id/dim/provider 포함** — 현재 `content_hash`가 content만 해싱 → 모델 교체 시 stale 벡터 조용히 skip(실버그). 기반 안정화.
3. **retrieved 주입 길이 cap + session diversity cap** — 현재 retrieved 글자수·세션 독점 무제한. 기반 안정화.
4. **valid_state/superseded_by/abstraction/anchors 컬럼 추가** — 시간성·유효성 흡수 시작.
5. **branch/session/recency/valid_state-aware 검색 랭킹** — "현재 유효한 기억 우선".
6. **실코퍼스 regression set** — 현재 합성 40발언/21질의는 대표성 약함.
7. **/search --debug · search_context explain** — 질의→토큰화→히트 score.
8. **reindex/lint 명령** — 모델교체·스키마변경 후 복구.

1~3 = 기반 안정화, 4~5 = 시간성·유효성 흡수, 6~8 = 검증·운용.

## Graphiti를 1순위로 보면 안 되는 이유

외부 graph DB/managed service로 유도됨(SQLite-light 충돌). 지금 문제는 graph traversal이 아니라 **검색 오염**(active branch·stale vector·outdated decision = 더 싼 결함). 연구/제품 memory framework와 터미널 설계토론 도구는 도메인이 다름. 필요한 건 "더 똑똑한 메모리"가 아니라 "검색 결과가 현재 작업에 유효한지 구분하는 최소 메타데이터".

## 기대 효과

"한국어 검색을 의식한 RAG 레이어" → "SQLite-light 장기 설계 기억 레이어"로 한 단계 상승.
