// SQLite 시스템오브레코드: 메시지 트리 영속 + FTS5 선-형태소화 색인/검색.

use rusqlite::Connection;

use crate::store::{StoredMessage, StoredSession};

// 스키마 버전 상수.
const CURRENT_SCHEMA_VERSION: u32 = 2;

// config 테이블 생성 SQL.
const CREATE_CONFIG: &str = "CREATE TABLE IF NOT EXISTS config (key TEXT PRIMARY KEY, value TEXT);";

// sessions 테이블 생성 SQL.
const CREATE_SESSIONS: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    id          TEXT PRIMARY KEY,
    head_id     INTEGER,
    created_at  TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
);
";

// messages 테이블 + 인덱스 생성 SQL.
const CREATE_MESSAGES: &str = "
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
";

// FTS5 가상 테이블 생성 SQL. content=선-형태소화 텍스트, session_id/msg_id는 UNINDEXED.
const CREATE_MESSAGES_FTS: &str = "
CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
    content,
    session_id UNINDEXED,
    msg_id     UNINDEXED,
    tokenize='unicode61'
);
";

// 메시지 벡터 저장 테이블. f32 LE BLOB, content_hash로 증분 색인 가드.
const CREATE_MESSAGE_VECTORS: &str = "
CREATE TABLE IF NOT EXISTS message_vectors (
    session_id   TEXT NOT NULL,
    msg_id       INTEGER NOT NULL,
    dim          INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    embedding    BLOB NOT NULL,
    PRIMARY KEY(session_id, msg_id)
);
";

/// SQLite 기반 메시지 트리 저장소.
pub struct SqliteStore {
    conn: Connection,
}

/// FTS 검색 결과 한 건.
pub struct SearchHit {
    pub session_id: String,
    pub msg_id: u64,
    pub speaker: String,
    pub content: String, // 원문(FTS의 형태소화본 아님)
    pub score: f64,      // bm25(낮을수록 관련 높음)
}

impl SqliteStore {
    /// 파일 기반 SQLite DB를 열고 WAL/foreign_keys 설정 + 마이그레이션을 적용한다.
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("sqlite: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| format!("sqlite: {e}"))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 인메모리 DB를 생성한다. 테스트 전용.
    pub fn open_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("sqlite: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("sqlite: {e}"))?;
        let db = Self { conn };
        db.migrate()?;
        Ok(db)
    }

    /// 스키마 마이그레이션을 실행한다. config 테이블 먼저, schema_version 없으면 v0->v1 일괄 적용.
    fn migrate(&self) -> Result<(), String> {
        // config 테이블 먼저 보장.
        self.conn
            .execute_batch(CREATE_CONFIG)
            .map_err(|e| format!("sqlite: {e}"))?;

        let version: u32 = self
            .conn
            .query_row(
                "SELECT value FROM config WHERE key = 'schema_version'",
                [],
                |row| {
                    let v: String = row.get(0)?;
                    Ok(v.parse::<u32>().unwrap_or(0))
                },
            )
            .unwrap_or(0);

        if version < CURRENT_SCHEMA_VERSION {
            // v0 -> v2: 전체 스키마 생성. IF NOT EXISTS라 기존 테이블 재실행 무해.
            self.conn
                .execute_batch(CREATE_SESSIONS)
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute_batch(CREATE_MESSAGES)
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute_batch(CREATE_MESSAGES_FTS)
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute_batch(CREATE_MESSAGE_VECTORS)
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO config(key, value) VALUES('schema_version', ?1)",
                    [CURRENT_SCHEMA_VERSION.to_string()],
                )
                .map_err(|e| format!("sqlite: {e}"))?;
        }

        Ok(())
    }

    /// 세션을 저장(upsert)한다. 기존 메시지/FTS를 전량 교체하고 fts_tok로 선-형태소화한다.
    pub fn save_session<F: Fn(&str) -> String>(
        &self,
        session_id: &str,
        ss: &StoredSession,
        fts_tok: F,
    ) -> Result<(), String> {
        // head는 Option<u64> -> NULL(None) 또는 정수.
        let head_val: Option<i64> = ss.head.map(|h| h as i64);

        // 트랜잭션 시작.
        self.conn
            .execute_batch("BEGIN;")
            .map_err(|e| format!("sqlite: {e}"))?;

        let result = (|| -> Result<(), String> {
            // (1) sessions upsert.
            self.conn
                .execute(
                    "INSERT INTO sessions(id, head_id) VALUES(?1, ?2) \
                     ON CONFLICT(id) DO UPDATE SET head_id=excluded.head_id, updated_at=datetime('now')",
                    rusqlite::params![session_id, head_val],
                )
                .map_err(|e| format!("sqlite: {e}"))?;

            // (2) 기존 메시지/FTS 전량 삭제.
            self.conn
                .execute("DELETE FROM messages WHERE session_id=?1", [session_id])
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute("DELETE FROM messages_fts WHERE session_id=?1", [session_id])
                .map_err(|e| format!("sqlite: {e}"))?;

            // (3) 각 메시지 삽입.
            for msg in &ss.messages {
                let msg_id = msg.id as i64;
                let parent_id: Option<i64> = msg.parent_id.map(|p| p as i64);

                self.conn
                    .execute(
                        "INSERT INTO messages(session_id, msg_id, parent_id, speaker, content) \
                         VALUES(?1, ?2, ?3, ?4, ?5)",
                        rusqlite::params![session_id, msg_id, parent_id, msg.speaker, msg.content],
                    )
                    .map_err(|e| format!("sqlite: {e}"))?;

                let fts_content = fts_tok(&msg.content);
                self.conn
                    .execute(
                        "INSERT INTO messages_fts(content, session_id, msg_id) VALUES(?1, ?2, ?3)",
                        rusqlite::params![fts_content, session_id, msg_id],
                    )
                    .map_err(|e| format!("sqlite: {e}"))?;
            }

            Ok(())
        })();

        if result.is_ok() {
            self.conn
                .execute_batch("COMMIT;")
                .map_err(|e| format!("sqlite: {e}"))?;
        } else {
            let _ = self.conn.execute_batch("ROLLBACK;");
        }

        result
    }

    /// 세션을 로드한다. 없으면 Ok(None)을 반환한다.
    pub fn load_session(&self, session_id: &str) -> Result<Option<StoredSession>, String> {
        // sessions 테이블에서 head_id 조회. 행 없음(세션 없음)과 실제 DB 에러를 구분한다.
        let head_raw: Option<i64> = match self.conn.query_row(
            "SELECT head_id FROM sessions WHERE id=?1",
            [session_id],
            |row| row.get(0),
        ) {
            Ok(v) => v,                                            // 세션 있음(head_id NULL이면 None).
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None), // 세션 없음.
            Err(e) => return Err(format!("sqlite: {e}")),         // 실제 DB 에러는 전파.
        };

        let head: Option<u64> = head_raw.map(|h| h as u64);

        // messages 테이블에서 ORDER BY msg_id로 복원.
        let mut stmt = self
            .conn
            .prepare(
                "SELECT msg_id, parent_id, speaker, content \
                 FROM messages WHERE session_id=?1 ORDER BY msg_id",
            )
            .map_err(|e| format!("sqlite: {e}"))?;

        let messages: Vec<StoredMessage> = stmt
            .query_map([session_id], |row| {
                let msg_id: i64 = row.get(0)?;
                let parent_id: Option<i64> = row.get(1)?;
                let speaker: String = row.get(2)?;
                let content: String = row.get(3)?;
                Ok(StoredMessage {
                    id: msg_id as u64,
                    parent_id: parent_id.map(|p| p as u64),
                    speaker,
                    content,
                })
            })
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;

        Ok(Some(StoredSession { messages, head }))
    }

    /// 단건 메시지를 조회한다. 없으면 Ok(None). 벡터-only 키의 원문 해석용.
    pub fn get_message(
        &self,
        session_id: &str,
        msg_id: u64,
    ) -> Result<Option<(String, String)>, String> {
        let msg_id_i64 = msg_id as i64;
        let row = match self.conn.query_row(
            "SELECT speaker, content FROM messages WHERE session_id=?1 AND msg_id=?2",
            rusqlite::params![session_id, msg_id_i64],
            |row| Ok((row.get(0)?, row.get(1)?)),
        ) {
            Ok(r) => Some(r),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => return Err(format!("sqlite: {e}")),
        };
        Ok(row)
    }

    /// 선-형태소화된 FTS 쿼리로 메시지를 검색한다. 빈 쿼리는 빈 결과를 반환한다.
    pub fn search(&self, fts_query: &str, limit: usize) -> Result<Vec<SearchHit>, String> {
        if fts_query.trim().is_empty() {
            return Ok(vec![]);
        }

        let mut stmt = self
            .conn
            .prepare(
                "SELECT f.session_id, f.msg_id, m.speaker, m.content, bm25(messages_fts) AS score \
                 FROM messages_fts f \
                 JOIN messages m ON m.session_id = f.session_id AND m.msg_id = f.msg_id \
                 WHERE messages_fts MATCH ?1 \
                 ORDER BY score \
                 LIMIT ?2",
            )
            .map_err(|e| format!("sqlite: {e}"))?;

        let limit_i64 = limit as i64;
        let hits: Vec<SearchHit> = stmt
            .query_map(rusqlite::params![fts_query, limit_i64], |row| {
                let session_id: String = row.get(0)?;
                let msg_id: i64 = row.get(1)?;
                let speaker: String = row.get(2)?;
                let content: String = row.get(3)?;
                let score: f64 = row.get(4)?;
                Ok(SearchHit {
                    session_id,
                    msg_id: msg_id as u64,
                    speaker,
                    content,
                    score,
                })
            })
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;

        Ok(hits)
    }

    /// 세션 메시지를 벡터 색인한다. content_hash가 동일하면 skip(증분). sqlite 피처 전용.
    #[cfg(feature = "sqlite")]
    pub fn index_vectors(
        &self,
        session_id: &str,
        ss: &StoredSession,
        embedder: &dyn crate::store::embedding::Embedder,
    ) -> Result<(), String> {
        self.conn
            .execute_batch("BEGIN;")
            .map_err(|e| format!("sqlite: {e}"))?;

        let result = (|| -> Result<(), String> {
            for msg in &ss.messages {
                // content_hash 계산(FNV-1a 64bit, 버전 무관 결정적).
                let content_hash = content_hash(&msg.content);

                let msg_id = msg.id as i64;

                // 기존 행 조회: 같은 hash면 skip(증분).
                let existing: Option<String> = self
                    .conn
                    .query_row(
                        "SELECT content_hash FROM message_vectors WHERE session_id=?1 AND msg_id=?2",
                        rusqlite::params![session_id, msg_id],
                        |row| row.get(0),
                    )
                    .ok();

                if existing.as_deref() == Some(&content_hash) {
                    continue;
                }

                // 임베딩 생성.
                let vec = embedder.embed(&msg.content)?;
                let dim = vec.len() as i64;

                // f32 LE BLOB 직렬화.
                let blob: Vec<u8> = vec.iter().flat_map(|f| f.to_le_bytes()).collect();

                // upsert.
                self.conn
                    .execute(
                        "INSERT INTO message_vectors(session_id, msg_id, dim, content_hash, embedding) \
                         VALUES(?1, ?2, ?3, ?4, ?5) \
                         ON CONFLICT(session_id, msg_id) DO UPDATE SET \
                             dim=excluded.dim, \
                             content_hash=excluded.content_hash, \
                             embedding=excluded.embedding",
                        rusqlite::params![session_id, msg_id, dim, content_hash, blob],
                    )
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            Ok(())
        })();

        if result.is_ok() {
            self.conn
                .execute_batch("COMMIT;")
                .map_err(|e| format!("sqlite: {e}"))?;
        } else {
            let _ = self.conn.execute_batch("ROLLBACK;");
        }

        result
    }

    /// message_vectors 전체를 brute-force cosine으로 검색해 top-K를 반환한다. sqlite 피처 전용.
    #[cfg(feature = "sqlite")]
    pub fn vector_search(
        &self,
        query_vec: &[f32],
        limit: usize,
    ) -> Result<Vec<(String, u64, f64)>, String> {
        if query_vec.is_empty() {
            return Ok(vec![]);
        }

        let mut stmt = self
            .conn
            .prepare("SELECT session_id, msg_id, dim, embedding FROM message_vectors")
            .map_err(|e| format!("sqlite: {e}"))?;

        let mut scored: Vec<(String, u64, f64)> = stmt
            .query_map([], |row| {
                let session_id: String = row.get(0)?;
                let msg_id: i64 = row.get(1)?;
                let dim: i64 = row.get(2)?;
                let blob: Vec<u8> = row.get(3)?;
                Ok((session_id, msg_id as u64, dim as usize, blob))
            })
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?
            .into_iter()
            .filter_map(|(sid, mid, dim, blob)| {
                // BLOB -> Vec<f32>(LE 역직렬화).
                if blob.len() != dim * 4 {
                    return None;
                }
                let vec: Vec<f32> = blob
                    .chunks_exact(4)
                    .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                    .collect();

                // cosine 유사도.
                let score = cosine_similarity(query_vec, &vec);
                Some((sid, mid, score))
            })
            .collect();

        // 내림차순 정렬 후 top-K.
        scored.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}

/// FNV-1a 64bit. Rust 버전 무관 결정적이라 content_hash 안정(임베딩 재색인 방지).
fn content_hash(s: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// cosine 유사도. norm 0이면 0 반환.
#[cfg(feature = "sqlite")]
fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.len() != b.len() {
        return 0.0;
    }
    let dot: f64 = a.iter().zip(b.iter()).map(|(x, y)| (*x as f64) * (*y as f64)).sum();
    let norm_a: f64 = a.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    let norm_b: f64 = b.iter().map(|x| (*x as f64) * (*x as f64)).sum::<f64>().sqrt();
    if norm_a < 1e-9 || norm_b < 1e-9 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::StoredMessage;

    fn ss() -> StoredSession {
        StoredSession {
            messages: vec![
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "claude/proposer".into(),
                    content: "검색 시스템 설계".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "codex/reviewer".into(),
                    content: "인덱스 전략 리뷰".into(),
                },
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

    #[test]
    fn search_matches_indexed_token() {
        // 선-토크나이즈된 텍스트("검색 시스템")를 저장 -> "검색" 쿼리가 매칭.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        let hits = db.search("검색", 10).unwrap();
        assert!(hits.iter().any(|h| h.msg_id == 1));
        assert!(hits.iter().all(|h| !h.content.is_empty())); // 원문 복원.
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
    fn get_message_returns_some_for_existing_and_none_for_missing() {
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("gm", &ss(), |t| t.to_string()).unwrap();
        // 존재하는 msg_id=1 -> Some.
        let found = db.get_message("gm", 1).expect("DB 에러 없어야 함");
        assert!(found.is_some(), "msg_id=1은 Some이어야 함");
        let (spk, ct) = found.unwrap();
        assert_eq!(spk, "claude/proposer");
        assert_eq!(ct, "검색 시스템 설계");
        // 없는 msg_id=999 -> Ok(None), 에러 아님.
        let missing = db.get_message("gm", 999).expect("DB 에러 없어야 함");
        assert!(missing.is_none(), "없는 msg_id는 None이어야 함");
        // 없는 세션 -> Ok(None).
        let no_session = db.get_message("no-such-session", 1).expect("DB 에러 없어야 함");
        assert!(no_session.is_none(), "없는 세션은 None이어야 함");
    }

    #[test]
    fn content_hash_is_deterministic_and_unique() {
        // 같은 입력 -> 같은 해시(결정성).
        let h1 = content_hash("hello world");
        let h2 = content_hash("hello world");
        assert_eq!(h1, h2, "같은 입력은 같은 해시여야 함");
        // 다른 입력 -> 다른 해시.
        let h3 = content_hash("hello world!");
        assert_ne!(h1, h3, "다른 입력은 다른 해시여야 함");
        // 빈 문자열도 결정적.
        let he = content_hash("");
        assert_eq!(he, content_hash(""), "빈 문자열 결정성");
        // 16자리 hex 포맷.
        assert_eq!(h1.len(), 16, "FNV-1a 64bit는 16자리 hex");
    }

    #[test]
    fn search_returns_correct_msg_id_across_sessions() {
        // rowid != msg_id 상황(secall test_turn_index_not_rowid 적응): 두 세션 색인 후 msg_id 정확성.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("a", &ss(), |t| t.to_string()).unwrap();
        let other = StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "x".into(),
                content: "검색 색인".into(),
            }],
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
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "a".into(),
                content: "검색을 어떻게 설계할까".into(),
            }],
            head: Some(1),
        };
        db.save_session("m", &s, |t| tok.tokenize_for_fts(t)).unwrap();
        // 쿼리도 동일 형태소화.
        let q = tok.tokenize_for_fts("검색");
        let hits = db.search(&q, 10).unwrap();
        assert!(!hits.is_empty(), "형태소 색인이 '검색을'을 '검색'으로 잡아야 함");
    }

    // 벡터 검색 테스트: sqlite 피처 전용.
    #[cfg(feature = "sqlite")]
    mod vector_tests {
        use super::*;
        use crate::store::embedding::{Embedder, MockEmbedder};
        use std::sync::atomic::{AtomicUsize, Ordering};

        /// embed 호출 횟수를 카운트하는 MockEmbedder 래퍼.
        struct CountingMock {
            inner: MockEmbedder,
            calls: AtomicUsize,
        }

        impl CountingMock {
            fn new(dim: usize) -> Self {
                Self { inner: MockEmbedder::new(dim), calls: AtomicUsize::new(0) }
            }

            fn call_count(&self) -> usize {
                self.calls.load(Ordering::SeqCst)
            }
        }

        impl Embedder for CountingMock {
            fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.inner.embed(text)
            }

            fn dim(&self) -> usize {
                self.inner.dim()
            }
        }

        #[test]
        fn vector_search_finds_same_content() {
            // 두 메시지 색인 후, 첫 메시지 content로 쿼리 -> 첫 메시지가 top.
            let db = SqliteStore::open_memory().unwrap();
            let mock = MockEmbedder::new(64);
            let session = StoredSession {
                messages: vec![
                    StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "목표 텍스트".into(),
                    },
                    StoredMessage {
                        id: 2,
                        parent_id: Some(1),
                        speaker: "b".into(),
                        content: "다른 내용의 메시지".into(),
                    },
                ],
                head: Some(2),
            };
            db.save_session("vs1", &session, |t| t.to_string()).unwrap();
            db.index_vectors("vs1", &session, &mock).unwrap();

            // 같은 텍스트로 쿼리 벡터 생성(MockEmbedder는 결정적이므로 cosine=1).
            let query_vec = mock.embed("목표 텍스트").unwrap();
            let results = db.vector_search(&query_vec, 10).unwrap();

            assert!(!results.is_empty(), "벡터 검색 결과가 있어야 함");
            let top = &results[0];
            assert_eq!(top.0, "vs1");
            assert_eq!(top.1, 1, "같은 텍스트를 가진 msg_id=1이 top이어야 함");
            // cosine 유사도는 1.0에 근사해야 함.
            assert!(top.2 > 0.99, "cosine 유사도가 1.0에 근사해야 함: {}", top.2);
        }

        #[test]
        fn index_vectors_is_incremental() {
            // 같은 세션 두 번 색인 시 두 번째는 embed 호출 수 = 0(content_hash 동일이라 skip).
            let db = SqliteStore::open_memory().unwrap();
            let counter = CountingMock::new(64);
            let session = StoredSession {
                messages: vec![StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "a".into(),
                    content: "증분 테스트 메시지".into(),
                }],
                head: Some(1),
            };
            db.save_session("inc1", &session, |t| t.to_string()).unwrap();

            // 첫 번째 색인: embed 1회 호출.
            db.index_vectors("inc1", &session, &counter).unwrap();
            assert_eq!(counter.call_count(), 1, "첫 번째 색인에서 embed 1회 호출");

            // 두 번째 색인: 동일 content_hash이므로 skip -> embed 0회 추가 호출.
            db.index_vectors("inc1", &session, &counter).unwrap();
            assert_eq!(counter.call_count(), 1, "두 번째 색인에서 embed 추가 호출 없어야 함");
        }
    }
}
