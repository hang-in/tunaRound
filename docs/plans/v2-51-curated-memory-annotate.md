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

### abstraction 표면화 = 렌더 경계에서만(retriever는 raw content 유지)

**핵심 결정(적대 리뷰 반영)**: 표면화를 retriever `finish`에서 content로 하면 이중 주입 실회귀가 난다. repl의 active-path 중복제거는 `a.content == u.content`(내용 완전일치)에 의존하는데, 변형된 content가 원문과 안 맞아 현재-세션 active 발언에 annotation을 달면 active 경로와 검색 맥락 양쪽에 이중 주입된다. 그래서 **`Utterance`에 `abstraction: Option<String>` 필드를 추가**하고, retriever는 **content를 원문 raw 그대로** 두고 abstraction만 필드에 실어 보낸다(dedup 정상 작동). **표면화("[요약] 증류문 + 원문")는 렌더 경계**(`prompt::join_utterances` 주입 시점·`repl::render` 표시 시점)에서만 일어난다. abstraction 미설정이면 완전 불변(additive).

### anchor 부스트 = penalty tier 내 2차 정렬 키(유효성 강등 불침해)

`rerank`는 penalty(낮을수록 상위) 안정 정렬이다. anchor 부스트를 penalty에 직접 감산하면 superseded(+2)·off-branch(+1)·recency(+1) 강등을 넘어설 위험이 있다(자가 리뷰 지적). 그래서 **penalty를 1차, anchor_rank(매치=0/미매치=1)를 2차 키**로 삼아 `sort_by_key((penalty, anchor_rank))` 한다. 이러면 유효성/분기/recency 순서는 절대 불변이고, **같은 penalty tier 안에서만** anchor 매치가 앞선다(rejected 드롭·superseded 강등 무손상 보장). 쿼리 토큰과 anchors 모두 **비영숫자 경계**(`!c.is_alphanumeric()`)로 분리·소문자화한다(양쪽 토크나이저 통일 - 하이픈·언더스코어가 든 앵커 `RAG-설계`도 `설계` 질의와 매치, gemini HIGH). 매치는 **토큰 완전일치**(2자 미만·부분일치 제외)라 짧고 흔한 토큰의 과매치를 막는다(적대 리뷰 반영). abstraction·anchors 조회는 rerank가 항목당 `get_validity`를 **1회만** 호출해 결과에 실어 나른다(핫패스 DB 왕복 중복 제거).

### 봇 리뷰(PR #78) 반영 3건

- **anchor 토크나이저 불일치(gemini HIGH)**: `anchor_matches`의 앵커 분리를 `query_anchor_tokens`와 동일한 비영숫자 경계로 통일했다(위 문단). 하이픈·슬래시·언더스코어가 든 앵커가 질의 토큰과 완전일치하게 된다.
- **abstraction 길이 캡(CodeRabbit)**: `join_utterances`에서 원문은 `MAX_ANSWER_LEN`으로 캡되는데 abstraction은 무제한이라 "발언당 길이 상한"이 깨졌다. abstraction도 `chars().take(MAX_ANSWER_LEN)`으로 캡한 뒤 얹는다. `repl::render`는 원문 자체를 캡하지 않는 터미널 표시 경로라 길이 불변식이 없어 캡 미적용(비대칭 회피).
- **플래그 값 파싱(CodeRabbit)**: `--abstraction`/`--anchors`의 값 자리에 오는 다음 토큰이 `--`로 시작하면(다음 플래그) 값 없음으로 보고 삼키지 않는다(예 `--abstraction --anchors "x"`는 anchors만 설정).

## 변경 범위

- `src/types.rs`: `Utterance.abstraction: Option<String>` 필드 + `Utterance::new` 헬퍼(리터럴 대체).
- `src/orchestrator/mod.rs`: `AnnotationSink` trait 신설.
- `src/orchestrator/prompt.rs`: `join_utterances`가 abstraction을 주입 렌더 시점에 표면화.
- `src/store/retriever.rs`: `SqliteAnnotationSink` + 재export / `rerank`가 query_tokens·anchor_rank 2차 키 + abstraction 캐리(단일 get_validity) / `finish`가 abstraction을 Utterance 필드로(content raw 유지) / anchor 완전일치 헬퍼 / 단위 테스트 2건(raw+abstraction 캐리·부스트).
- `src/repl/mod.rs`: `Command::Annotate` + 파싱(따옴표 존중) + `annotation_sink` 필드/빌더 + `mark_annotation` + `render` 표면화 + 도움말 + 파싱/이중주입 방지 테스트.
- `src/main.rs`: `--db` 시 annotation_sink 배선(validity_sink 옆).

불변: 스키마(v10)·마이그레이션·`StoredMessage`·직렬화. abstraction 미설정 시 기존 검색 동작 완전 불변.

## 검증

- `cargo build/clippy/test --features "morphology mcp serve"` green(신규 테스트 포함, 기존 retrieve_* 유지).
- `cargo build --features "semantic morphology mcp serve"` 컴파일 확인.
