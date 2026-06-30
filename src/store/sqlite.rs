// SQLite 시스템오브레코드: 메시지 트리 영속 + FTS5 선-형태소화 색인/검색.

use rusqlite::Connection;

use crate::store::{StoredMessage, StoredSession};

// 스키마 버전 상수.
const CURRENT_SCHEMA_VERSION: u32 = 1;

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
            // v0 -> v1: 전체 스키마 생성.
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
        // sessions 테이블에서 head_id 조회.
        let row: Option<Option<i64>> = self
            .conn
            .query_row(
                "SELECT head_id FROM sessions WHERE id=?1",
                [session_id],
                |row| row.get(0),
            )
            .ok();

        // 세션이 없으면 None.
        let head_raw = match row {
            None => return Ok(None),
            Some(v) => v,
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
}
