# Plan v2-51: 큐레이션 기억 배선 (abstraction/anchors 활성화, Option A)

> 배경: cross-session raw RAG(FTS+벡터+유효성 랭킹)는 이미 완성돼 있다. `message_validity` 테이블의 `abstraction`·`anchors` 컬럼(스키마 v4)도 이미 존재하나 생성/소비 파이프라인이 없어 테스트에서만 쓰인다. 이 작업은 순수 델타 = 그 컬럼을 배선해 사람이 큐레이션(증류 요약·앵커)을 남기고 검색이 그걸 표면화·부스트하게 한다.

## 목표

1. 사람이 REPL에서 발언에 큐레이션을 남긴다: `/annotate <id> --abstraction "..." --anchors "k1,k2"`.
2. 검색(retrieve/retrieve_ctx)이 abstraction을 주입 텍스트로 표면화한다(원문 보존).
3. 검색 재랭크가 쿼리 토큰과 anchors가 매치되면 순위를 부스트한다.
4. 스키마는 절대 불변(이미 있는 컬럼 배선만, `CURRENT_SCHEMA_VERSION`=10 유지, ALTER 없음).

## 설계 판단

### 신규 `AnnotationSink` trait (ValiditySink 확장 아님)

유효성(supersede/reject)과 큐레이션(abstraction/anchors)은 **직교한 관심사**다. store가 이미 `set_validity`/`set_annotation`을 분리해 서로 보존하고(테스트 `set_validity_preserves_annotation_and_vice_versa`가 증명), `ValiditySink`의 시그니처(`valid_state`, `superseded_by`)에 큐레이션 파라미터를 얹으면 trait이 지저분해진다. `AnnotationSink`를 ValiditySink와 동형으로 신설해 REPL 배선을 대칭·명료하게 유지한다(worktree 격리라 additive 신설의 blast radius 최소).

- `orchestrator::AnnotationSink { fn set_annotation(sid, msg_id, abstraction: Option<&str>, anchors: Option<&str>) -> Result<(), String>; }`
- `store::retriever::SqliteAnnotationSink`(store.set_annotation 위임), 재export 추가.
- REPL: `annotation_sink` 필드 + `with_annotation_sink` 빌더 + `mark_annotation` 처리(mark_validity 대칭). `--db` 없으면(sink None) 안내만.

### abstraction 표면화 = 원문 앞에 증류 요약 얹기(대체 아님)

`finish`에서 재랭크된 각 히트에 abstraction이 있으면 `content = "[요약] {abstraction}\n{원문}"`으로 표면화한다. 원문을 지우지 않아(정보 손실 없음·provenance 유지) 기존 검색 동작을 훼손하지 않고, 주입 프롬프트는 사람이 증류한 결정을 앞세운다. abstraction 미설정이면 content 불변 = 기존 테스트 전량 그대로 통과(additive).

### anchor 부스트 = penalty tier 내 2차 정렬 키(유효성 강등 불침해)

`rerank`는 penalty(낮을수록 상위) 안정 정렬이다. anchor 부스트를 penalty에 직접 감산하면 superseded(+2)·off-branch(+1)·recency(+1) 강등을 넘어설 위험이 있다(자가 리뷰 지적). 그래서 **penalty를 1차, anchor_rank(매치=0/미매치=1)를 2차 키**로 삼아 `sort_by_key((penalty, anchor_rank))` 한다. 이러면 유효성/분기/recency 순서는 절대 불변이고, **같은 penalty tier 안에서만** anchor 매치가 앞선다(rejected 드롭·superseded 강등 무손상 보장). 쿼리 토큰은 raw query를 영숫자 경계로 분리·소문자화해 만들고, anchors는 콤마·공백으로 분리해 토큰 상호 포함 매치한다.

## 변경 범위

- `src/orchestrator/mod.rs`: `AnnotationSink` trait 신설.
- `src/store/retriever.rs`: `SqliteAnnotationSink` + 재export / `rerank`에 query_tokens·anchor_rank 2차 키 / `finish`에 abstraction 표면화 / anchor 매치 헬퍼 / 단위 테스트 2건(표면화·부스트).
- `src/repl/mod.rs`: `Command::Annotate` + 파싱(따옴표 존중) + `annotation_sink` 필드/빌더 + `mark_annotation` + 도움말 + 파싱 테스트.
- `src/main.rs`: `--db` 시 annotation_sink 배선(validity_sink 옆).

불변: 스키마(v10)·마이그레이션·`StoredMessage`·직렬화. abstraction 미설정 시 기존 검색 동작 완전 불변.

## 검증

- `cargo build/clippy/test --features "morphology mcp serve"` green(신규 테스트 포함, 기존 retrieve_* 유지).
- `cargo build --features "semantic morphology mcp serve"` 컴파일 확인.
