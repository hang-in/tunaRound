<!-- v2-52 ⑤ store DTO↔도메인 경계 리팩토링의 착수 전 고정 계약(중립 타입·변환 경계·S0~S6 마이그레이션). -->

# v2-52 ⑤ Store DTO ↔ 도메인 경계: 착수 전 계약 (2026-07-12)

> 정본 배경 = [v2-52 리팩토링 백로그 §2](v2-52-refactoring-backlog_2026-07-12.md). 이 문서는 "착수 전 계약(공개 API·테스트) 고정" 요구를 만족하는 **고정 계약**이다. understand 페이즈(4렌즈 결합지도 + 계약 초안, 워크플로우) + Opus 대조검증 산출.

## 1. 목표와 비목표

- **목표**: `orchestrator`·`repl`·`store`가 `StoredSession`/`StoredMessage`를 직공유하는 결합을 끊는다. SQLite 스키마 형태(`msg_id`·`parent_id`·`head_id`)와 트리 append 상태머신이 REPL·prompt 로직에 새어드는 것을 중립 도메인 타입 뒤로 캡슐화한다.
- **비목표(과설계 회피, 북극성=연결조직만)**: `Validity`(message_validity 별도 테이블)·`SearchHit`·(session_id,msg_id) 랭킹 키는 이미 `retrieve`가 `Vec<Utterance>`만 반환해 외부 노출 0 → **이번 계약에서 제외**. `msg_id: u64` 스칼라는 raw 유지(newtype 승격이 CLI·append_turn·writer·sink로 번지기만 하고 트리-shape 누수가 아님).

## 2. 결합의 무게중심 (실측)

`src/repl/mod.rs`의 `Session`이다(4렌즈 일치, Opus 대조검증 완료):
- `Session { messages: Vec<StoredMessage>, head: Option<u64>, ... }`(repl:291-292) = SQLite 트리 스키마가 REPL 런타임 모델 그 자체.
- `append_round`(repl:568-586)가 `next_id`·`parent=head`·`head 전진`이라는 트리 append 상태머신 + `StoredMessage`/`StoredSession` 리터럴 조립을 store 밖에서 재구현.
- `/checkout`·`/branches`·`/supersede`·`/annotate`(repl:771-824)가 raw `msg_id` 스캔·head 재지정을 직접 수행.
- 경계 트레잇 `CoreSync::load_session -> StoredSession`(orchestrator:140)·`MessageIndexer::persist(&StoredSession)`(indexer)가 store DTO를 정책 계층 시그니처에 노출 = store→orchestrator 역결합.
- **대조군(이미 중립, 안 건드림)**: `TranscriptReader::read_transcript`·`ContextRetriever::retrieve`는 내부에서 트리를 소진하고 `Vec<Utterance>`만 냄 = 목표형 캡슐화의 기존 모범. `types.rs::Utterance`는 store↔orchestrator 역import를 피해 이미 중립 위치.

## 3. 중립 타입 (전부 `src/types.rs`, Utterance 옆, feature-neutral)

**핵심 원칙: serde derive 금지.** `StoredSession`/`StoredMessage`는 store에 **직렬화·행매핑 전용 DTO**로 잔존하고, 중립 타입은 serde를 갖지 않아 **와이어 포맷이 도메인에 새는 것이 구조적으로 불가**하다(이게 리팩토링의 요체). `Utterance`/`Validity`가 이미 serde 없는 선례.

```rust
type MessageId = u64;   // 별칭(도메인 어휘만; newtype 승격은 §6 결정=별칭 유지)

struct MessageNode { id: MessageId, parent: Option<MessageId>, speaker: String, content: String }
    // derive: Debug/Clone/PartialEq. StoredMessage와 1:1 shape이나 serde 없음.

struct BranchHead(Option<MessageId>);   // derive: Debug/Clone/Copy/PartialEq/Default
    // 활성 분기 끝. 메서드 tip(&self)->Option<MessageId>. cli_run observe의 head-변화(분기축소) 커서 리싱크를 명명.

struct ConversationSnapshot { nodes: Vec<MessageNode>, head: BranchHead }   // derive: Debug/Clone/PartialEq/Default
```

`ConversationSnapshot` 메서드(현재 (&[StoredMessage],head)로 분해돼 REPL이 호출하는 자유함수·상태머신을 회수):
- `active_path(&self) -> Vec<Utterance>` : `path_to_root` 흡수(root→head walk + 순환가드). **안정 오라클 의미 보존.**
- `append(&mut self, speaker, content) -> MessageId` : `next_id`(max+1·빈=1) + parent=head + head 전진을 원자 캡슐화(repl:568-576 회수).
- `checkout(&mut self, id) -> bool` / `contains(&self, id) -> bool` : /checkout·/supersede 회수.
- `tree_summary(&self) -> String` : /branches 회수.
- `head(&self) -> BranchHead` / `transcript_len(&self)`(=active_path().len()) / `node_count(&self)`(mcp `.messages.len()` 대체) / `is_empty` / `new()`.
- Utterance와의 관계: snapshot = "트리 구조를 가진 Utterance 저장소", Utterance = 그 평탄화 뷰 원소.

## 4. 변환 경계

store 계층에 `From` impl로 격리(두 타입 모두 crate 로컬이라 orphan rule은 impl을 crate 어디든 허용 = 모듈 무관. store/mod.rs에 두는 것은 변환을 store에만 가두는 계층 경계 선택):
- `impl From<StoredSession> for ConversationSnapshot` + `impl From<&ConversationSnapshot> for StoredSession`. types.rs 중립 타입은 StoredSession을 import하지 않음(중립 유지).
- **저수준 SQLite 매핑은 시그니처 불변**: `SqliteStore::{load_session/save_session/append_turn/index_vectors}`는 계속 `StoredSession`/`u64` 생산·소비 → 그 강한 오라클(`session_roundtrip_preserves_tree_and_head`·`append_turn_chains`·orphan 정리)이 손 안 대고 green = 영속 불변의 연속 증명.
- 변환은 그 한 겹 위(트레잇 래퍼)에서만: `SqliteCoreSync::load_session = store.load_session().map(Into::into)`, `SqliteIndexer::persist(&ConversationSnapshot)`는 내부에서 `StoredSession::from(snap)` 후 save. 공개 `store::load_session(path)`는 StoredSession deserialize(레거시 bare-array 폴백) 후 `.into()`.
- **레거시 하위호환·serde 와이어는 전량 StoredSession 쪽에만**. `load_session_falls_back_to_legacy_bare_array`가 이 경계 안전망.

## 5. 고정 공개 API 계약

**바뀌는 시그니처(계약)**:
1. `CoreSync::load_session(&self,&str) -> Option<ConversationSnapshot>`(기존 StoredSession). `append_turn -> Result<u64,String>` **불변**.
2. `MessageIndexer::persist(&self,&str,&ConversationSnapshot)`(기존 &StoredSession).
3. 공개 fn `store::load_session -> io::Result<ConversationSnapshot>`·`store::save_session(&ConversationSnapshot,..)`(레거시 폴백 내부 보존).
4. `repl::Session`: 필드 → `snapshot: ConversationSnapshot`(사적). `seed_from(ConversationSnapshot)`, `to_stored()->ConversationSnapshot`(→ `snapshot()`으로 개명), `save_state`는 내부 From. `transcript_len`/`message_count`는 snapshot 위임(공개 시그니처 불변).

**불변(안전망 축, 절대 안 바뀜)**: `read_transcript -> Vec<Utterance>`, `retrieve/retrieve_ctx -> Vec<Utterance>`, `append_turn -> u64`, `ValiditySink/AnnotationSink`(msg_id:u64), store 내부 `SqliteStore::*`(StoredSession/u64 유지).

## 6. Open questions - 결정 (Opus, 최소설계 편향)

1. **MessageId = 별칭**(newtype 아님). newtype는 CLI 파싱·append_turn 반환·writer·sink로 리플이 번지는 스칼라 plumbing이라 트리-shape 누수 해결과 무관. → 별칭 확정.
2. **BranchHead = 유지**(minimal Copy newtype, `tip()`만). 설계 doc 명명 타입 정합 + cli_run observe 등가비교 명명. 비용 1필드 Copy 구조체로 미미.
3. **`Session::to_stored` → `snapshot()` 개명**(의미 정확, cli_run seed 호출부 소폭 diff).
4. **자유함수**: **path_to_root/next_id/tree_summary만 S6에서 삭제**(ConversationSnapshot 메서드가 대체, 유닛테스트를 메서드 대상으로 재작성). **to_stored/from_stored/save/load는 유지**(구현 시 확정): 이들은 트리-shape 누수가 아니라 **v1 bare-array 파일 포맷**(Utterance↔StoredMessage + 바 배열 JSON)이고, `store_roundtrip` 통합테스트가 하위호환을 검증하는 데 쓴다. 삭제 대상은 consumer 누수를 만들던 트리 순회/채번 함수 3개로 국한.

## 7. 마이그레이션 (S0~S6, 각 단계 = 컴파일 + 전체 테스트 green, 한 번에 하나)

- **S0 안전망 선보강**: `tree_summary`/`Command::Branches`는 오라클 전무 → 특성화(characterization) 테스트로 현행 /branches 출력·/checkout 동작 먼저 핀. STRONG 오라클(`path_to_root_walks_parents`·`session_roundtrip` 양쪽·`load_session_falls_back`·`append_turn_chains`·`retrieve_carries_abstraction`·`core_sync_adopts`) green 확인 후 시작.
- **S1 순수 추가**: types.rs 중립 타입 + 메서드, store/mod.rs From 2개. 새 유닛테스트(active_path/append/checkout/next_id 규칙 = 기존 강한 오라클 미러). 아무도 아직 안 씀 → green 불변.
- **S2 REPL 내부만 전환**: Session 필드 → `snapshot`. append/active_path/checkout/branches/contains를 메서드로. seed_from/to_stored는 아직 StoredSession 받/주며 경계 From → 공개 트레잇·REPL 테스트 시그니처 불변 통과. **최대 누수를 여기서 내부 격리.**
- **S3 CoreSync 뒤집기**: `load_session -> Option<ConversationSnapshot>`. SqliteCoreSync + FakeCoreSync + adopt_from_core 갱신.
- **S4 Indexer 뒤집기**: `persist(&ConversationSnapshot)`. REPL의 StoredSession 리터럴 조립 제거(`&self.snapshot` 전달).
- **S5 공개 fn + seed_from/snapshot을 중립으로**: main 재개·cli_run observe(snapshot.active_path()/head()) 갱신. store_roundtrip 재작성.
- **S6 자유함수 내부화**: path_to_root/next_id/tree_summary 삭제(마지막 호출처 retriever·cli_run을 `ConversationSnapshot::from(ss).active_path()`로 전환), 유닛테스트를 snapshot 메서드 대상으로 재작성(같은 assert). to_stored/from_stored/save/load(v1 포맷)·StoredSession/StoredMessage + serde + SQLite roundtrip은 영속 DTO/하위호환으로 잔존.

전 구간 Utterance 경계 오라클(retrieve 6종·read_transcript·prompt 13종)은 시그니처·assert 불변 = 랭킹/검색/프롬프트 동작 불변의 연속 증명.

## 8. 위험

- Blast radius: 거의 전부 repl/mod.rs(Session) + 그 테스트(FakeCoreSync·seed_from fixture). 대부분 '입력 구성 코드의 기계적 교체'(리터럴→중립 생성자)이지 assert 변화 아님. store 내부·retriever 안 건드림.
- 함정1(오라클 공백): tree_summary/Branches 무테스트 → **S0 특성화 테스트 선행 필수**(안 하면 /branches 포맷 회귀가 조용히 통과).
- 함정2(포맷 드리프트): 중립 타입에 serde 실수 부착 시 두 직렬화 경로. 가드 = serde 금지 + `load_session_falls_back` green.
- 등가성: `append`가 next_id·parent·head 전진을 정확 재현, checkout=contains-후-이동, core-sync 공존(save 후 append 클로버 없음). S1 유닛테스트를 기존 STRONG 오라클에서 미러해 방어.
