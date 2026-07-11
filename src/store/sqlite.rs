// SQLite 시스템오브레코드: 메시지 트리 영속 + FTS5 선-형태소화 색인/검색.

use std::cell::RefCell;
use std::collections::HashMap;

use rusqlite::Connection;

use crate::store::a2a::TaskEvent;
use crate::store::agents::AgentEntry;

// 스키마 버전 상수. v3: message_vectors.model_id. v4: message_validity(유효성 메타).
// v5: messages.created_at. v6: tasks(A2A task 위임, docs/design/v2-a2a-partner-delegation_2026-07-02.md).
// v7: tasks.claimed_at/lease_expires_at/claimed_by/attempt_count(claim-후-워커사망 자동 requeue,
// lease 기반. DB 내부용 컬럼이라 Task wire 구조체(store/a2a.rs)에는 노출하지 않는다).
// v8: tasks.runner(어떤 러너가 claim했는지 트레이스). Task wire 구조체에도 노출(runner 표시용).
const CURRENT_SCHEMA_VERSION: u32 = 8;

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
    created_at  TEXT,
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

// 메시지 벡터 저장 테이블. f32 LE BLOB, (content_hash, model_id)로 증분 색인 가드.
// model_id=임베딩 모델 정체성. 모델 교체 시 같은 내용이라도 재임베딩(stale 벡터 방지).
const CREATE_MESSAGE_VECTORS: &str = "
CREATE TABLE IF NOT EXISTS message_vectors (
    session_id   TEXT NOT NULL,
    msg_id       INTEGER NOT NULL,
    dim          INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    model_id     TEXT,
    embedding    BLOB NOT NULL,
    PRIMARY KEY(session_id, msg_id)
);
";

// 유효성 메타 테이블. 원문(messages)과 분리해 레이어링(valid_state/superseded/abstraction/anchors).
const CREATE_MESSAGE_VALIDITY: &str = "
CREATE TABLE IF NOT EXISTS message_validity (
    session_id           TEXT NOT NULL,
    msg_id               INTEGER NOT NULL,
    valid_state          TEXT NOT NULL DEFAULT 'active',
    superseded_by_msg_id INTEGER,
    abstraction          TEXT,
    anchors              TEXT,
    updated_at           TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(session_id, msg_id)
);
";

// A2A task 위임 테이블. message_json/artifacts_json/history_json은 crate::store::a2a 타입의 직렬화본.
// created_at/updated_at은 SQL 기본값 없이 애플리케이션(create_task)이 명시적으로 채운다.
// claimed_at/lease_expires_at/claimed_by/attempt_count(v7)는 claim-후-워커사망 자동 requeue용 DB 내부
// 컬럼이다(Task wire 구조체에는 노출하지 않음). fresh DB는 여기서 바로 만들고, 기존(v6) DB는 migrate()의
// ALTER TABLE로 보강한다.
const CREATE_TASKS: &str = "
CREATE TABLE IF NOT EXISTS tasks (
    task_id           TEXT PRIMARY KEY,
    context_id        TEXT,
    from_agent        TEXT NOT NULL,
    to_agent          TEXT NOT NULL,
    state             TEXT NOT NULL,
    message_json      TEXT,
    artifacts_json    TEXT,
    history_json      TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    claimed_at        TEXT,
    lease_expires_at  TEXT,
    claimed_by        TEXT,
    runner            TEXT,
    attempt_count     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_tasks_to_agent_state ON tasks(to_agent, state);
";

// tasks 컬럼 목록(고정 순서). SELECT/INSERT 양쪽에서 재사용해 컬럼 순서 불일치를 방지한다.
// runner(v8)는 맨 끝에 추가: claimed_at/lease_expires_at/claimed_by/attempt_count(v7, DB 내부용)는
// 이 목록에 없으므로, 기존 컬럼 순서를 그대로 두고 새 컬럼만 끝에 붙이면 인덱스가 안전하게 늘어난다.
const TASK_COLUMNS: &str = "task_id, context_id, from_agent, to_agent, state, \
     message_json, artifacts_json, history_json, created_at, updated_at, runner";

/// claim lease 기본 유효시간(초). 에이전트 실행이 길 수 있어 넉넉히 잡는다(죽은 워커 감지용이지
/// task 실행시간 상한이 아니다).
const CLAIM_LEASE_SECS: i64 = 30 * 60;
/// requeue 시도 상한. lease 만료로 회수될 때마다 attempt_count가 늘고, 이 값 이상이면 무한 requeue를
/// 막기 위해 failed로 격리한다.
const MAX_CLAIM_ATTEMPTS: i64 = 3;

/// SQLite 기반 메시지 트리 저장소.
pub struct SqliteStore {
    conn: Connection,
    /// A2A task 상태변이 broadcast 버스. None이면(스트리밍 미사용 구성) emit은 no-op(§2.1).
    event_bus: Option<tokio::sync::broadcast::Sender<TaskEvent>>,
    /// 인메모리 에이전트 로스터(uuid → 항목). 영속 아님(브로커 재기동 시 워커 재등록으로 복원).
    /// 내부 가변성(RefCell): 모든 접근이 바깥 Mutex로 직렬화되므로 &self 메서드로 갱신 가능하다.
    agent_roster: RefCell<HashMap<String, AgentEntry>>,
}

/// FTS 검색 결과 한 건.
pub struct SearchHit {
    pub session_id: String,
    pub msg_id: u64,
    pub speaker: String,
    pub content: String, // 원문(FTS의 형태소화본 아님)
    pub score: f64,      // bm25(낮을수록 관련 높음)
}

mod messages;
mod registry;
mod tasks;

impl SqliteStore {
    /// 파일 기반 SQLite DB를 열고 WAL/foreign_keys 설정 + 마이그레이션을 적용한다.
    pub fn open(path: &str) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("sqlite: {e}"))?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| format!("sqlite: {e}"))?;
        let db = Self {
            conn,
            event_bus: None,
            agent_roster: RefCell::new(HashMap::new()),
        };
        db.migrate()?;
        Ok(db)
    }

    /// 인메모리 DB를 생성한다. 테스트 전용.
    pub fn open_memory() -> Result<Self, String> {
        let conn = Connection::open_in_memory().map_err(|e| format!("sqlite: {e}"))?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")
            .map_err(|e| format!("sqlite: {e}"))?;
        let db = Self {
            conn,
            event_bus: None,
            agent_roster: RefCell::new(HashMap::new()),
        };
        db.migrate()?;
        Ok(db)
    }

    /// broadcast 채널 용량. 구독자가 이 이상 뒤처지면 오래된 이벤트부터 유실(SSE는 최신 상태만 중요하므로 허용).
    const TASK_EVENT_CAP: usize = 256;

    /// task 이벤트 broadcast 채널을 활성화한다(빌더). 이후 `task_event_sender()`로 구독 가능해진다.
    /// 초기 Receiver는 즉시 drop해도 된다(broadcast::Sender는 live receiver 없이도 send 가능).
    pub fn with_task_events(mut self) -> Self {
        let (tx, _rx) = tokio::sync::broadcast::channel(Self::TASK_EVENT_CAP);
        self.event_bus = Some(tx);
        self
    }

    /// 구독자가 `.subscribe()`할 수 있도록 broadcast Sender를 clone해 반환한다. 버스 미활성화 시 None.
    pub fn task_event_sender(&self) -> Option<tokio::sync::broadcast::Sender<TaskEvent>> {
        self.event_bus.clone()
    }

    /// task 이벤트를 버스에 publish한다. 버스가 없거나 수신자가 없으면 조용히 무시한다(no-op).
    fn emit_task_event(&self, ev: TaskEvent) {
        if let Some(tx) = &self.event_bus {
            let _ = tx.send(ev);
        }
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
            // v4: 유효성 메타 테이블(새 TABLE이라 IF NOT EXISTS로 fresh·기존 모두 처리).
            self.conn
                .execute_batch(CREATE_MESSAGE_VALIDITY)
                .map_err(|e| format!("sqlite: {e}"))?;
            // v6: A2A task 위임 테이블(새 TABLE이라 IF NOT EXISTS로 fresh·기존 모두 처리).
            self.conn
                .execute_batch(CREATE_TASKS)
                .map_err(|e| format!("sqlite: {e}"))?;
            // v3: 기존(v2) DB의 message_vectors엔 model_id가 없으므로 ADD COLUMN으로 보강한다.
            // fresh DB는 CREATE에 이미 있어 column_exists가 true → ALTER 생략. 기존 행은 NULL이라
            // 다음 색인 때 model_id 불일치로 재임베딩(자동 복구).
            if !self.column_exists("message_vectors", "model_id") {
                self.conn
                    .execute("ALTER TABLE message_vectors ADD COLUMN model_id TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            // v5: messages.created_at(cross-session recency 랭킹용). ALTER는 비상수 default 불가라
            // nullable로 추가하고 값은 INSERT에서 명시(datetime('now')). 기존 행은 NULL(=recency 판단 유보).
            if !self.column_exists("messages", "created_at") {
                self.conn
                    .execute("ALTER TABLE messages ADD COLUMN created_at TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            // v7: 기존(v6) DB의 tasks엔 lease 컬럼들이 없으므로 ADD COLUMN으로 보강한다. fresh DB는
            // CREATE_TASKS에 이미 있어 column_exists가 true → ALTER 생략.
            if !self.column_exists("tasks", "claimed_at") {
                self.conn
                    .execute("ALTER TABLE tasks ADD COLUMN claimed_at TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            if !self.column_exists("tasks", "lease_expires_at") {
                self.conn
                    .execute("ALTER TABLE tasks ADD COLUMN lease_expires_at TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            if !self.column_exists("tasks", "claimed_by") {
                self.conn
                    .execute("ALTER TABLE tasks ADD COLUMN claimed_by TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            if !self.column_exists("tasks", "attempt_count") {
                self.conn
                    .execute(
                        "ALTER TABLE tasks ADD COLUMN attempt_count INTEGER NOT NULL DEFAULT 0",
                        [],
                    )
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            // v8: 어떤 러너가 처리했는지 트레이스(claim 시 기록). 기존 행은 NULL.
            if !self.column_exists("tasks", "runner") {
                self.conn.execute("ALTER TABLE tasks ADD COLUMN runner TEXT", [])
                    .map_err(|e| format!("sqlite: {e}"))?;
            }
            self.conn
                .execute(
                    "INSERT OR REPLACE INTO config(key, value) VALUES('schema_version', ?1)",
                    [CURRENT_SCHEMA_VERSION.to_string()],
                )
                .map_err(|e| format!("sqlite: {e}"))?;
        }

        Ok(())
    }

    /// 테이블에 특정 컬럼이 존재하는지 PRAGMA table_info로 확인한다(마이그레이션 가드).
    fn column_exists(&self, table: &str, column: &str) -> bool {
        let Ok(mut stmt) = self.conn.prepare(&format!("PRAGMA table_info({table})")) else {
            return false;
        };
        let Ok(rows) = stmt.query_map([], |row| row.get::<_, String>(1)) else {
            return false;
        };
        rows.flatten().any(|name| name == column)
    }

    /// 현재 SQLite 시각을 `datetime('now')` 포맷 문자열로 반환한다. A2A task 생성 등 애플리케이션단에서
    /// 타임스탬프를 미리 stamping해야 하는 호출자(예: JSON-RPC send 핸들러)가 사용하는 공용 헬퍼다.
    pub fn now(&self) -> Result<String, String> {
        self.conn
            .query_row("SELECT datetime('now')", [], |row| row.get(0))
            .map_err(|e| format!("sqlite: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fresh_db_has_model_id_column() {
        let db = SqliteStore::open_memory().unwrap();
        assert!(db.column_exists("message_vectors", "model_id"), "v3 스키마에 model_id 컬럼 존재");
    }

    #[test]
    fn migration_v2_to_v3_adds_model_id_and_preserves_rows() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_mig_v2v3.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        // v2 스키마 수동 구성: message_vectors에 model_id 없음 + schema_version=2 + 기존 행 1건.
        {
            let conn = rusqlite::Connection::open(p).unwrap();
            conn.execute_batch(
                "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE message_vectors(session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, \
                     dim INTEGER NOT NULL, content_hash TEXT NOT NULL, embedding BLOB NOT NULL, \
                     PRIMARY KEY(session_id, msg_id));
                 INSERT INTO message_vectors(session_id,msg_id,dim,content_hash,embedding) \
                     VALUES('s',1,8,'h',x'00');
                 INSERT INTO config(key,value) VALUES('schema_version','2');",
            )
            .unwrap();
        }
        // open → migrate v2→v3(ALTER로 model_id 추가).
        let db = SqliteStore::open(p).unwrap();
        assert!(db.column_exists("message_vectors", "model_id"), "마이그레이션이 model_id 추가");
        let cnt: i64 = db
            .conn
            .query_row("SELECT COUNT(*) FROM message_vectors WHERE session_id='s'", [], |r| r.get(0))
            .unwrap();
        assert_eq!(cnt, 1, "기존 벡터 행 보존");
        let mid: Option<String> = db
            .conn
            .query_row(
                "SELECT model_id FROM message_vectors WHERE session_id='s' AND msg_id=1",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(mid, None, "기존 행 model_id는 NULL(다음 색인 때 재임베딩 트리거)");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn migration_v4_to_v5_adds_created_at_nullable() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_mig_v4v5.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        // v4 스키마 수동 구성: messages에 created_at 없음 + schema_version=4 + 기존 행 1건.
        {
            let conn = rusqlite::Connection::open(p).unwrap();
            conn.execute_batch(
                "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                     created_at TEXT, updated_at TEXT);
                 CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                     session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                     speaker TEXT NOT NULL, content TEXT NOT NULL, UNIQUE(session_id, msg_id));
                 INSERT INTO sessions(id, head_id) VALUES('s', 1);
                 INSERT INTO messages(session_id,msg_id,parent_id,speaker,content) \
                     VALUES('s',1,NULL,'a','hi');
                 INSERT INTO config(key,value) VALUES('schema_version','4');",
            )
            .unwrap();
        }
        // open → migrate v4→v5(ALTER로 created_at 추가).
        let db = SqliteStore::open(p).unwrap();
        assert!(db.column_exists("messages", "created_at"), "마이그레이션이 created_at 추가");
        let ca: Option<String> = db.get_created_at("s", 1).unwrap();
        assert_eq!(ca, None, "기존 행 created_at은 NULL(recency 판단 유보)");
        let _ = std::fs::remove_file(&path);
    }
}
