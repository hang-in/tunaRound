---
title: "tunaRound v2 Plan 09: SQLite 시스템오브레코드 + FTS5 (선-형태소화 저장)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 북극성(능동 검색)의 토대. 메시지 트리를 SQLite로 영속하고 FTS5(unicode61)에 선-형태소화 텍스트를 색인해 한국어 키워드 검색을 가능케 한다("검색을"->"검색"). secall store/schema.rs + search/bm25.rs 패턴 답습. 이번 슬라이스는 격리 모듈(store/sqlite.rs) + 테스트만 - REPL/main의 기존 JSON 영속은 미접촉(다음 슬라이스에서 시스템오브레코드 전환 + RAG 주입). sqlite feature 게이트(기본 빌드 무영향). 스토어는 토크나이저 비의존(선-토크나이즈 텍스트 주입), morphology는 통합 테스트에서만 결합.
---

# tunaRound v2 Plan 09: SQLite 시스템오브레코드 + FTS5 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: test-driven-development. Steps use checkbox (`- [ ]`).
> 출처: `D:/privateProject/seCall/crates/secall-core/src/store/{schema.rs,db.rs}` + `search/bm25.rs` (정본). 결정: docs/design/v2-context-memory-direction_2026-06-30.md(저장소 계층화: SQLite=시스템오브레코드+FTS 백본). **재발명 말고 답습.** 아키텍처 재론 금지.

**Goal:** 토론 메시지 트리를 SQLite에 영속하고, FTS5에 **선-형태소화된 텍스트**를 색인해 한국어 키워드 검색의 토대를 깐다. Plan 08의 `tokenize_for_fts`가 비로소 쓰이는 자리("검색을" 색인 시 "검색"으로 저장 -> "검색" 쿼리가 매칭). 이것이 북극성 "전사 통째 재주입 -> 검색해 슬라이스만 주입(RAG)"의 첫 콘크리트 토대다.

**Architecture:** 신규 `src/store/sqlite.rs` 격리 모듈. secall `store/db.rs`의 open/migrate(WAL·foreign_keys·config schema_version) + `schema.rs`의 FTS5 패턴(content 색인 + session_id/msg_id UNINDEXED + tokenize='unicode61')을 tunaRound 메시지 트리(`StoredMessage{id,parent_id,speaker,content}` + `StoredSession{messages,head}`)에 적응. **스토어는 토크나이저 비의존** - 선-토크나이즈된 FTS 텍스트를 주입받아 저장/검색(morphology/폴백 선택은 호출부 책임). 이렇게 sqlite feature가 morphology와 독립. 에러는 프로젝트 관례대로 `Result<T, String>`(anyhow 미도입, Plan 08 방식).

**Tech Stack:** Rust 2024. 신규 의존성(optional, `sqlite` feature): `rusqlite = { version = "0.31", features = ["bundled"], optional = true }`(secall/tunaSalon 동일 검증된 선택, bundled=SQLite 소스 동봉 + FTS5 포함). 선행: v2 Plan 08 done(tokenize_for_fts).

> 규율: #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, TDD. 위임 Sonnet 서브에이전트, Opus 리뷰. 검증과 commit/push 분리.

---

## 범위

- **포함:** `sqlite` feature + optional `rusqlite`(bundled) + `src/store/sqlite.rs`(SqliteStore: open/open_memory/save_session/load_session/search + 스키마 + 마이그레이션) + `src/store/mod.rs` 모듈 선언. 토크나이저 비의존 코어 테스트 + `sqlite+morphology` 통합 테스트(선-형태소화 검색 실증).
- **비포함(다음 슬라이스):** REPL/main 영속의 SQLite 전환(현행 JSON 유지), Redis 스냅샷 경로 조정, 검색 주입(`build_round_prompt` RAG화), 벡터/하이브리드, 에이전트 검색 도구. 이 plan은 저장+FTS 색인+검색 쿼리까지의 격리 모듈만(어디서도 호출 안 함).
- **불변식:** 기본 `cargo test`(sqlite off) = 기존 66 그대로(SQLite 미컴파일). 기존 `store/mod.rs` JSON 경로 미접촉.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) optional `rusqlite`(bundled) + `[features] sqlite = ["dep:rusqlite"]`. |
| `src/store/mod.rs` | (수정) `#[cfg(feature = "sqlite")] pub mod sqlite;` 한 줄 추가. 기존 코드 미접촉. |
| `src/store/sqlite.rs` | (신규) 스키마 consts + SqliteStore(open/open_memory/save_session/load_session/search) + SearchHit + 테스트. 첫 줄 역할 주석. |

> 선제 설계: 격리 모듈(기존 JSON 영속 미접촉). feature 게이트로 기본 빌드 무영향. 토크나이저 비의존(선-토크나이즈 텍스트 주입)이라 morphology와 직교.

## 스키마 (secall 적응)

```sql
-- 세션: 트리 head 포함. 타임스탬프는 SQLite datetime('now')로(chrono 미도입).
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    head_id     INTEGER,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);

-- 메시지 트리: msg_id=세션 내 id(StoredMessage.id), parent_id=트리 부모.
CREATE TABLE IF NOT EXISTS messages (
    rowid       INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id  TEXT NOT NULL REFERENCES sessions(id),
    msg_id      INTEGER NOT NULL,
    parent_id   INTEGER,
    speaker     TEXT NOT NULL,
    content     TEXT NOT NULL,
    UNIQUE(session_id, msg_id)
);
CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id);

-- FTS5: content=선-형태소화 텍스트. session_id/msg_id는 UNINDEXED(역참조용).
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    session_id UNINDEXED,
    msg_id     UNINDEXED,
    tokenize='unicode61'
);

CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT);
```

## API 스케치

```rust
pub struct SqliteStore { conn: rusqlite::Connection }

pub struct SearchHit {
    pub session_id: String,
    pub msg_id: u64,
    pub speaker: String,
    pub content: String, // 원문(FTS의 형태소화본 아님)
    pub score: f64,      // bm25 (낮을수록 관련 높음)
}

impl SqliteStore {
    pub fn open(path: &str) -> Result<Self, String>;        // WAL + foreign_keys + migrate
    pub fn open_memory() -> Result<Self, String>;           // 테스트용
    // 세션 upsert + 메시지/FTS 전량 교체(트랜잭션). fts_tok로 선-형태소화.
    pub fn save_session<F: Fn(&str) -> String>(&self, session_id: &str, ss: &StoredSession, fts_tok: F) -> Result<(), String>;
    pub fn load_session(&self, session_id: &str) -> Result<Option<StoredSession>, String>;
    // 선-형태소화된 쿼리로 FTS MATCH + bm25. 빈 쿼리는 빈 결과.
    pub fn search(&self, fts_query: &str, limit: usize) -> Result<Vec<SearchHit>, String>;
}
```

---

### Task 1: 의존성 + 스키마 + open/migrate + 세션 저장/로드 라운드트립

**Files:**
- Modify: `Cargo.toml`, `src/store/mod.rs`
- Create: `src/store/sqlite.rs`

- [ ] **Step 1: Cargo 의존성 + feature**
```toml
[dependencies]
# ... 기존 ...
rusqlite = { version = "0.31", features = ["bundled"], optional = true }

[features]
morphology = ["dep:kiwi-rs", "dep:lindera"]
sqlite = ["dep:rusqlite"]
```
  - `src/store/mod.rs`에 `#[cfg(feature = "sqlite")] pub mod sqlite;` 추가(기존 내용 미접촉).

- [ ] **Step 2: `cargo build --features sqlite`로 rusqlite(bundled) 컴파일 확인.** bundled는 SQLite C 소스 컴파일이라 첫 빌드 느림(수십 초~분). 깨지면 멈추고 보고(MSVC 툴체인 등 Windows 빌드 이슈 가능성).

- [ ] **Step 3: 실패 테스트 먼저(`src/store/sqlite.rs`의 `mod tests`)** - 파일 첫 줄 `// SQLite 시스템오브레코드: 메시지 트리 영속 + FTS5 선-형태소화 색인/검색.`
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredMessage;

    fn ss() -> StoredSession {
        StoredSession {
            messages: vec![
                StoredMessage { id: 1, parent_id: None, speaker: "claude/proposer".into(), content: "검색 시스템 설계".into() },
                StoredMessage { id: 2, parent_id: Some(1), speaker: "codex/reviewer".into(), content: "인덱스 전략 리뷰".into() },
            ],
            head: Some(2),
        }
    }

    #[test]
    fn session_roundtrip_preserves_tree_and_head() {
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        let back = db.load_session("s1").unwrap().expect("present");
        assert_eq!(back.messages, ss().messages);
        assert_eq!(back.head, Some(2));
    }

    #[test]
    fn load_missing_session_is_none() {
        let db = SqliteStore::open_memory().unwrap();
        assert!(db.load_session("nope").unwrap().is_none());
    }

    #[test]
    fn save_is_idempotent_upsert() {
        // 같은 세션 두 번 저장 -> 메시지 중복 없이 최신 상태.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        let back = db.load_session("s1").unwrap().unwrap();
        assert_eq!(back.messages.len(), 2);
    }
}
```

- [ ] **Step 4: 구현(`src/store/sqlite.rs`)** - secall `db.rs` open/migrate 답습:
  - 스키마 consts(위 "스키마" 절 그대로). `CURRENT_SCHEMA_VERSION: u32 = 1`.
  - `open`: `Connection::open(path)` + `execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;")` + `migrate()`. `open_memory`: `open_in_memory()` + foreign_keys + migrate.
  - `migrate`: config 테이블 먼저, schema_version 읽어 없으면 전체 CREATE(IF NOT EXISTS) 실행 후 version 기록. (단순 버전: v0->v1 일괄 생성.)
  - `save_session`: 트랜잭션. (1) `INSERT INTO sessions(id, head_id) VALUES(?,?) ON CONFLICT(id) DO UPDATE SET head_id=excluded.head_id, updated_at=datetime('now')`. (2) `DELETE FROM messages WHERE session_id=?` + `DELETE FROM messages_fts WHERE session_id=?`. (3) 각 메시지: `INSERT INTO messages(session_id,msg_id,parent_id,speaker,content)` + `INSERT INTO messages_fts(content,session_id,msg_id) VALUES(fts_tok(content), ?, ?)`. head는 `Option<u64>`->NULL/정수.
  - `load_session`: `SELECT head_id FROM sessions WHERE id=?`(없으면 Ok(None)). `SELECT msg_id,parent_id,speaker,content FROM messages WHERE session_id=? ORDER BY msg_id`로 `Vec<StoredMessage>` 복원. `Ok(Some(StoredSession{messages, head}))`.
  - 모든 `rusqlite::Error`는 `.map_err(|e| format!("sqlite: {e}"))`로 String화. **anyhow 쓰지 마라.**

- [ ] **Step 5: 검증 + 커밋**
  - `cargo test --features sqlite` -> Task 1 테스트 PASS. `cargo test`(기본) -> 66 그대로(sqlite off). `cargo build`/`clippy --all-targets`(기본 + `--features sqlite`) 경고 0.
  - `git add Cargo.toml Cargo.lock src/store/mod.rs src/store/sqlite.rs && git commit -m "feat(store): SQLite 시스템오브레코드 - 메시지 트리 저장/로드 (sqlite feature)"` (push 금지).

---

### Task 2: FTS5 색인 + 검색 (선-형태소화 실증)

**Files:**
- Modify: `src/store/sqlite.rs`

- [ ] **Step 1: 실패 테스트 먼저** - 토크나이저 비의존 코어 + morphology 통합:
```rust
    #[test]
    fn search_matches_indexed_token() {
        // 선-토크나이즈된 텍스트("검색 시스템")를 저장 -> "검색" 쿼리가 매칭.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        let hits = db.search("검색", 10).unwrap();
        assert!(hits.iter().any(|h| h.msg_id == 1));
        assert!(hits.iter().all(|h| !h.content.is_empty())); // 원문 복원
    }

    #[test]
    fn search_empty_query_is_empty() {
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        assert!(db.search("", 10).unwrap().is_empty());
    }

    #[test]
    fn search_no_match_is_empty() {
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        assert!(db.search("존재하지않는단어xyz", 10).unwrap().is_empty());
    }

    #[test]
    fn search_returns_correct_msg_id_across_sessions() {
        // rowid != msg_id 상황(secall test_turn_index_not_rowid 적응): 두 세션 색인 후 msg_id 정확성.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("a", &ss(), |t| t.to_string()).unwrap();
        let other = StoredSession {
            messages: vec![StoredMessage { id: 1, parent_id: None, speaker: "x".into(), content: "검색 색인".into() }],
            head: Some(1),
        };
        db.save_session("b", &other, |t| t.to_string()).unwrap();
        let hits = db.search("검색", 10).unwrap();
        assert!(hits.iter().any(|h| h.session_id == "b" && h.msg_id == 1));
    }

    // 선-형태소화 핵심: "검색을"(조사 포함)이 형태소 색인되어 "검색" 쿼리에 잡힌다.
    #[cfg(feature = "morphology")]
    #[test]
    fn morpheme_indexing_matches_inflected_form() {
        use crate::search::tokenizer::create_tokenizer;
        let tok = create_tokenizer("kiwi").expect("kiwi or lindera");
        let db = SqliteStore::open_memory().unwrap();
        let s = StoredSession {
            messages: vec![StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "검색을 어떻게 설계할까".into() }],
            head: Some(1),
        };
        db.save_session("m", &s, |t| tok.tokenize_for_fts(t)).unwrap();
        // 쿼리도 동일 형태소화.
        let q = tok.tokenize_for_fts("검색");
        let hits = db.search(&q, 10).unwrap();
        assert!(!hits.is_empty(), "형태소 색인이 '검색을'을 '검색'으로 잡아야 함");
    }
```

- [ ] **Step 2: `search` 구현**
  - 빈 쿼리(`fts_query.trim().is_empty()`) -> `Ok(vec![])`.
  - 쿼리:
```sql
SELECT f.session_id, f.msg_id, m.speaker, m.content, bm25(messages_fts) AS score
FROM messages_fts f
JOIN messages m ON m.session_id = f.session_id AND m.msg_id = f.msg_id
WHERE messages_fts MATCH ?1
ORDER BY score
LIMIT ?2
```
  - `?1`=fts_query, `?2`=limit. row -> SearchHit(msg_id는 i64->u64). bm25는 낮을수록 관련 높음 -> ORDER BY score ASC(기본). (정규화는 하이브리드 슬라이스로 미룸.)
  - **주의(리스크):** FTS5 MATCH는 일부 토큰(구두점 등)을 연산자로 해석할 수 있음. 폴백/형태소 토크나이저가 구두점을 제거하므로 현 입력은 안전하나, 방어적으로 토큰을 큰따옴표 감싸는 것은 후속 검토(이 슬라이스는 secall처럼 직접 전달).

- [ ] **Step 3: 검증 + 커밋**
  - `cargo test --features sqlite`(코어 검색 테스트) + `cargo test --features "sqlite morphology"`(형태소 통합 테스트 포함) PASS. `cargo test`(기본) 66 그대로. `cargo build`/`clippy --all-targets`를 `--features sqlite` / `--features "sqlite morphology"` 양쪽 경고 0.
  - `git add src/store/sqlite.rs && git commit -m "feat(store): FTS5 선-형태소화 색인 + bm25 검색"` (push 금지).

---

## Self-Review (작성자 체크)

- **결정 준수:** secall 정본 답습(schema FTS5 unicode61 + 선-형태소화 + UNINDEXED 역참조). 저장소 계층화 설계대로 SQLite=시스템오브레코드+FTS. 재발명 안 함.
- **범위 규율:** 격리 모듈만(REPL/main JSON 미접촉). 시스템오브레코드 전환·RAG·벡터는 명시적 비포함(다음 슬라이스). 사용자 확정(격리 우선 + sqlite feature).
- **직교성:** sqlite feature는 morphology와 독립(스토어 토크나이저 비의존, 선-토크나이즈 주입). 기본 빌드 무영향(66 불변).
- **의존성 적응:** rusqlite만(anyhow/chrono 미도입). 타임스탬프는 SQLite datetime('now'). 에러 String화.
- **리스크 관리:** rusqlite bundled 컴파일을 Task 1 Step 2에서 먼저 검증(Windows MSVC 빌드). FTS5 MATCH 연산자 해석은 리스크로 명시.

## 위험 / 한계 (문서화된 후속)

- **rusqlite bundled 빌드:** SQLite C 소스 컴파일 -> Windows는 MSVC(`cl.exe`) 필요. 실패 시 멈추고 보고(빌드 환경 이슈, 코드 아님).
- **FTS5 MATCH 연산자:** 토큰에 FTS5 특수문자가 섞이면 쿼리 깨질 수 있음. 현 토크나이저가 구두점 제거라 안전하나, 방어적 큰따옴표 래핑은 후속.
- **save_session 전량 교체:** 현재 세션 저장 시 메시지/FTS를 delete+reinsert(작은 토론엔 충분). 증분 색인은 시스템오브레코드 전환 슬라이스에서.
- **미배선:** 스토어는 아직 REPL/main에서 호출 안 함(격리). 다음 슬라이스에서 (1) 영속을 SQLite로 전환 + (2) `build_round_prompt`를 검색 슬라이스 주입(RAG)으로.
- **벡터/하이브리드:** 의미 검색은 별 슬라이스(원격 Ollama bge-m3 dim 1024). 이 plan은 어휘(FTS) 단독.
