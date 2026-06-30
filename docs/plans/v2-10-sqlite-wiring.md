---
title: "tunaRound v2 Plan 10: SQLite 라이브 배선 (메시지 인덱싱, 검색 데이터 채우기)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: Plan 09의 격리 SqliteStore를 라이브 REPL에 배선해 검색 인덱스를 실제로 채운다. append_round(메시지 트리 진입점, 이미 Redis 미러 훅)에서 SQLite에도 미러 = 기존 SessionBus 패턴 그대로. MessageIndexer trait(비게이트) + SqliteIndexer impl(sqlite feature) + Session에 indexer: Option<Box<dyn MessageIndexer>> 필드 + main 배선(--db). 추가적(JSON save/load·Redis 미접촉), 검색 소비(RAG)는 다음 슬라이스. 토크나이저는 closure 주입(morphology=형태소, 아니면 폴백)이라 SqliteIndexer는 feature 직교.
---

# tunaRound v2 Plan 10: SQLite 라이브 배선 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: test-driven-development. Steps use checkbox (`- [ ]`).
> 결정: docs/design/v2-context-memory-direction_2026-06-30.md(SQLite=시스템오브레코드). Plan 09 done(SqliteStore). **기존 SessionBus 미러 패턴 답습**(append_round 훅, Option<Box<dyn ...>> 필드). 아키텍처 재론 금지.

**Goal:** Plan 09에서 만든 격리 `SqliteStore`를 라이브 토론 흐름에 연결해, 매 라운드의 메시지가 SQLite + FTS에 색인되게 한다. 그러면 `search()`가 라이브/과거 데이터를 실제로 검색할 수 있다(다음 RAG 슬라이스의 전제). 이 슬라이스는 **쓰기(색인) 경로만** - 검색 소비는 Plan 11.

**Architecture:** 기존 `bus: Option<Box<dyn SessionBus>>`(Plan 04~06)와 **동일 패턴**으로 `indexer: Option<Box<dyn MessageIndexer>>`를 Session에 추가. `append_round`(메시지 트리 진입점이자 이미 Redis 미러 호출 지점)에서 indexer가 있으면 현재 전체 트리를 SQLite에 persist(전량 교체 = Redis 스냅샷과 동일 의미론). **추가적**: JSON save_state/load_session·Redis 미러는 미접촉. SqliteIndexer는 토크나이저 closure(`Box<dyn Fn(&str)->String + Send + Sync>`)를 주입받아 feature 직교(morphology=형태소, 없으면 폴백). main이 feature에 맞는 closure를 만들어 배선.

**Tech Stack:** Rust 2024. 신규 의존성 없음(Plan 09의 rusqlite/sqlite feature 재사용). 선행: Plan 09 done.

> 규율: #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, TDD. 위임 Sonnet, Opus 리뷰. 검증/commit 분리.

---

## 범위

- **포함:** `MessageIndexer` trait(비게이트, store/mod.rs 또는 신규 store/indexer.rs) + `SqliteIndexer` impl(sqlite feature, SqliteStore + tokenize closure 래핑) + Session `indexer` 필드 + `append_round` 배선 + main.rs `--db <path>` 배선(sqlite feature 시) + `tokenize_fallback` un-gate(sqlite-without-morphology용).
- **비포함(다음 슬라이스):** 검색 소비/`/search` 명령, `build_round_prompt` RAG화(Plan 11), 벡터/하이브리드(Plan 12). JSON 영속 은퇴(별 정리). 기본 feature 플립(별 결정).
- **불변식:** 기본 `cargo test`/`cargo run`(sqlite off) = 동작 불변(indexer None, JSON+Redis 그대로). 기존 repl 테스트 전부 green 유지.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/search/mod.rs` | (수정) `tokenize_fallback`를 비게이트로 노출(morphology 밖에서도 사용). 기존 morphology 모듈은 `use`로 재참조. |
| `src/store/indexer.rs` | (신규) `MessageIndexer` trait(비게이트) + `#[cfg(feature="sqlite")] SqliteIndexer`. 첫 줄 역할 주석. |
| `src/store/mod.rs` | (수정) `pub mod indexer;` 선언. |
| `src/repl/mod.rs` | (수정) Session에 `indexer: Option<Box<dyn MessageIndexer>>` 필드 + 생성자 인자 + `append_round`에서 persist 호출. |
| `src/main.rs` | (수정) `--db <path>` 파싱 + `#[cfg(feature="sqlite")]` SqliteIndexer 생성·배선. |

---

### Task 1: MessageIndexer trait + SqliteIndexer + Session 배선

**Files:**
- Modify: `src/search/mod.rs`, `src/store/mod.rs`, `src/repl/mod.rs`
- Create: `src/store/indexer.rs`

- [ ] **Step 1: `tokenize_fallback` 비게이트화** — `src/search/mod.rs`에서 `pub fn tokenize_fallback`를 morphology 밖(항상 컴파일)으로 옮기고, morphology의 `tokenizer.rs`는 `use super::tokenize_fallback;`로 참조(중복 정의 제거). 기존 morphology 테스트 불변 확인.

- [ ] **Step 2: 실패 테스트 먼저(`src/repl/mod.rs` tests)** — 기존 `FakeBus` 패턴을 답습한 `FakeIndexer`:
```rust
    #[derive(Default)]
    struct IdxCalls { persists: usize, last_session: String, last_len: usize }
    struct FakeIndexer(Rc<RefCell<IdxCalls>>);
    impl crate::store::indexer::MessageIndexer for FakeIndexer {
        fn persist(&self, session_id: &str, ss: &StoredSession) {
            let mut c = self.0.borrow_mut();
            c.persists += 1; c.last_session = session_id.to_string(); c.last_len = ss.messages.len();
        }
    }

    #[test]
    fn round_persists_to_indexer_when_present() {
        let calls = Rc::new(RefCell::new(IdxCalls::default()));
        let mut reg = MapRegistry::new();
        reg.insert("claude", Box::new(FakeRunner { reply: "제안".into() }));
        let participants = vec![Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() }];
        let mut s = Session::new_with_indexer(participants, Box::new(reg), "sess-i".into(), None, Some(Box::new(FakeIndexer(Rc::clone(&calls)))));
        let _ = s.step(Command::Message("주제".into()));
        let c = calls.borrow();
        assert_eq!(c.persists, 1);
        assert_eq!(c.last_session, "sess-i");
        assert_eq!(c.last_len, 1); // 1자리 1발언
    }

    #[test]
    fn no_indexer_means_normal_behavior() {
        let mut s = session_with_two_seats(); // indexer 없음
        let _ = s.step(Command::Message("주제".into()));
        assert_eq!(s.transcript_len(), 2); // 기존 동작 불변
    }
```

- [ ] **Step 3: 구현**
  - `src/store/indexer.rs`(신규, 첫 줄 `// 메시지 트리를 검색 인덱스(SQLite/FTS)에 미러링하는 인덱서 추상화.`):
```rust
use crate::store::StoredSession;

/// 메시지 트리를 검색 인덱스에 반영하는 추상화(SessionBus 미러 패턴과 동형).
pub trait MessageIndexer: Send + Sync {
    /// 현재 전체 트리를 인덱스에 persist한다(전량 교체 의미론).
    fn persist(&self, session_id: &str, ss: &StoredSession);
}

#[cfg(feature = "sqlite")]
pub use sqlite_indexer::SqliteIndexer;

#[cfg(feature = "sqlite")]
mod sqlite_indexer {
    use super::*;
    use crate::store::sqlite::SqliteStore;

    /// SqliteStore + 선-토크나이즈 closure를 묶은 인덱서.
    pub struct SqliteIndexer {
        store: SqliteStore,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
    }
    impl SqliteIndexer {
        pub fn new(store: SqliteStore, tok: Box<dyn Fn(&str) -> String + Send + Sync>) -> Self {
            Self { store, tok }
        }
    }
    impl MessageIndexer for SqliteIndexer {
        fn persist(&self, session_id: &str, ss: &StoredSession) {
            // best-effort: 색인 실패는 토론 흐름을 막지 않는다(eprintln 경고).
            if let Err(e) = self.store.save_session(session_id, ss, |t| (self.tok)(t)) {
                eprintln!("[tunaRound] SQLite 색인 실패: {e}");
            }
        }
    }
}
```
  - `src/store/mod.rs`: `pub mod indexer;` 추가.
  - `src/repl/mod.rs`: Session에 `indexer: Option<Box<dyn crate::store::indexer::MessageIndexer>>` 필드. 기존 생성자(`new`, `new_with_bus`)는 `indexer: None`으로 유지(behavior-preserving). 신규 `new_with_indexer(participants, registry, session_id, bus, indexer)` 추가. `append_round` 끝(bus 미러 다음)에 `if let Some(idx) = &self.indexer { idx.persist(&self.session_id, &StoredSession { messages: self.messages.clone(), head: self.head }); }`.

- [ ] **Step 4: 검증 + 커밋**
  - `cargo test`(기본) — 기존 repl 테스트 + 신규 2개 PASS(sqlite off라 SqliteIndexer 미컴파일, trait/FakeIndexer는 컴파일됨). `cargo test --features sqlite` PASS. build/clippy(기본 + sqlite) 경고 0.
  - `git commit` (push 금지): `feat(store): MessageIndexer 추상화 + SqliteIndexer + Session 배선`.

---

### Task 2: main.rs --db 배선 + 라이브 persist 통합 테스트

**Files:**
- Modify: `src/main.rs`
- Create: `tests/sqlite_wiring.rs`(integration, `#[cfg(feature="sqlite")]`)

- [ ] **Step 1: main.rs `--db <path>` 파싱** — 기존 인자 루프(`--roster`/`--observe`/`--session`)에 `--db` 추가(`db_path: Option<String>`).

- [ ] **Step 2: SqliteIndexer 생성·배선(`#[cfg(feature="sqlite")]`)**
  - sqlite feature 시 `--db` 있으면: `SqliteStore::open(&db_path)` + 토크나이즈 closure 생성:
    - `#[cfg(feature="morphology")]`: `let tok = create_tokenizer("kiwi")?; Box::new(move |t| tok.tokenize_for_fts(t))` (실패 시 폴백은 create_tokenizer 내부 처리).
    - `#[cfg(not(feature="morphology"))]`: `Box::new(|t| crate::search::tokenize_fallback(t).join(" "))` — 단 main은 bin이라 `tunaround::search::tokenize_fallback`.
  - `Session::new_with_indexer(...)`로 bus + indexer 동시 배선. (기존 resume/redis 분기 모두 indexer를 함께 넘기도록 조정. indexer는 Option이라 sqlite off/`--db` 없으면 None.)
  - sqlite feature 아니면 기존 `new_with_bus` 경로 그대로(불변).
  - **주의:** Session 생성 분기가 3곳(resume/redis/신규)이라 indexer를 일관되게 전달. 복잡하면 indexer를 먼저 만들어 변수로 잡고 각 분기에 전달.

- [ ] **Step 3: 통합 테스트(`tests/sqlite_wiring.rs`, `#[cfg(feature="sqlite")]`)** — 라이브 경로 대신 라이브러리 API로 검증(바이너리 stdin 구동은 무거움):
```rust
// SqliteIndexer를 단 Session이 라운드마다 SQLite에 persist하고, 이후 search로 잡히는지 검증.
#![cfg(feature = "sqlite")]
use tunaround::store::sqlite::SqliteStore;
// FakeRunner를 쓸 수 없으면(테스트 전용), repl의 공개 API로 구성 가능한 범위에서 검증.
```
  - 검증 핵심: SqliteStore::open_memory는 공유가 안 되므로, 파일 DB 경로로 open한 SqliteStore를 indexer에 넣고 Session 라운드 후 **같은 경로를 다시 open**해 `search`/`load_session`으로 색인 확인. (또는 indexer가 보유한 store를 직접 조회할 getter를 두지 않고, 파일 경로 왕복으로 검증.) 구현 세부는 Sonnet이 repl 공개 API와 맞춰 결정하되, "라운드 -> SQLite 색인 -> search 매칭"을 1개 이상 통합 테스트로 실증.

- [ ] **Step 4: 검증 + 커밋**
  - `cargo test`(기본) 불변. `cargo test --features sqlite` + `cargo test --features "sqlite morphology"` PASS. build/clippy 3조합(기본/sqlite/sqlite+morphology) 경고 0.
  - `cargo run --features sqlite -- --db /tmp/x.db` 스모크(EOF 즉시 종료라도 open/배선 패닉 없음) 1회.
  - `git commit` (push 금지): `feat(main): --db로 SqliteIndexer 배선 + 라이브 색인 통합 테스트`.

---

## Self-Review (작성자 체크)

- **패턴 답습:** SessionBus 미러(Option 필드 + append_round 훅)와 동형 -> 학습비용 0, 격리 유지.
- **추가적/불변:** JSON·Redis 미접촉. sqlite off면 indexer None = 기존 동작 그대로(기본 테스트 불변).
- **feature 직교:** SqliteIndexer는 tokenize closure 주입이라 morphology와 직교. main만 feature별 closure 선택. tokenize_fallback un-gate로 sqlite-only도 동작.
- **best-effort:** 색인 실패가 토론을 막지 않게 eprintln 경고(데이터 흐름 우선).
- **범위:** 쓰기(색인)만. 검색 소비는 Plan 11. JSON 은퇴/feature 플립은 명시적 비포함.

## 위험 / 한계 (문서화된 후속)

- **전량 재persist:** 매 라운드 전체 트리를 save_session(전량 교체) -> 큰 세션엔 비효율. 증분 색인은 후속(시스템오브레코드 본격화 시).
- **세 생성 분기:** main의 resume/redis/신규 분기에 indexer 전달이 번잡 -> Sonnet이 indexer를 선-생성해 일관 전달. 회귀 주의(기존 resume/observe 동작 불변 확인).
- **DB 경로 동시성:** 여러 프로세스가 같은 `--db`를 열면 WAL이 다중 reader 허용하나 writer 경합 가능(busy_timeout=5s 완화). 멀티세션 동시 쓰기 본격화는 후속.
- **검색 미소비:** 색인만 되고 아직 프롬프트에 안 쓰임 -> 다음 Plan 11(RAG)에서 build_round_prompt가 search 소비.
