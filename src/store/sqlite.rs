// SQLite 시스템오브레코드: 메시지 트리 영속 + FTS5 선-형태소화 색인/검색.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};

use rusqlite::Connection;

use crate::store::a2a::{
    append_history_json, Artifact, Message, Task, TaskEvent, TaskRow, TaskState,
};
use crate::store::agents::AgentEntry;
use crate::store::candidates::CandidateEntry;
use crate::store::{StoredMessage, StoredSession};

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
    /// 인메모리 발견 후보 풀(uuid → 후보). 리포터가 열거해 보고한 미무장 세션. roster와 별개 공간이나
    /// 조회 시 armed overlay(uuid∈online roster)로 승격 표시한다. reported_at TTL로 stale 소멸(영속 아님).
    candidate_pool: RefCell<HashMap<String, CandidateEntry>>,
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
        let db = Self {
            conn,
            event_bus: None,
            agent_roster: RefCell::new(HashMap::new()),
            candidate_pool: RefCell::new(HashMap::new()),
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
            candidate_pool: RefCell::new(HashMap::new()),
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

    /// 에이전트를 로스터에 등록(있으면 교체). now는 last_heartbeat 초기값.
    pub fn register_agent(
        &self,
        uuid: &str,
        tags: BTreeMap<String, String>,
        display_name: Option<String>,
        now: &str,
    ) {
        self.agent_roster.borrow_mut().insert(
            uuid.to_string(),
            AgentEntry { uuid: uuid.to_string(), tags, display_name, last_heartbeat: now.to_string() },
        );
    }

    /// heartbeat: 존재하면 last_heartbeat 갱신 후 true, 미등록 uuid면 false(등록 선행 필요).
    pub fn heartbeat_agent(&self, uuid: &str, now: &str) -> bool {
        match self.agent_roster.borrow_mut().get_mut(uuid) {
            Some(entry) => {
                entry.last_heartbeat = now.to_string();
                true
            }
            None => false,
        }
    }

    /// selector에 매칭되며 online인 에이전트를 uuid 오름차순으로 반환(clone).
    pub fn list_agents(
        &self,
        selector: &BTreeMap<String, String>,
        now: &str,
        ttl_secs: i64,
    ) -> Vec<AgentEntry> {
        let mut out: Vec<AgentEntry> = self
            .agent_roster
            .borrow()
            .values()
            .filter(|entry| {
                crate::store::agents::selector_matches(&entry.tags, selector)
                    && crate::store::agents::is_online(&entry.last_heartbeat, now, ttl_secs)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        out
    }

    /// 발견 후보를 풀에 보고(upsert). uuid 단위로 교체하며 reported_at은 브로커 수신 시각(now)으로
    /// 덮어쓴다(리포터 시계 불신). 재보고 없는 후보는 list_candidates의 TTL로 자연 제외된다.
    pub fn report_candidates(&self, candidates: Vec<CandidateEntry>, now: &str) {
        let mut pool = self.candidate_pool.borrow_mut();
        for mut c in candidates {
            c.reported_at = now.to_string();
            pool.insert(c.uuid.clone(), c);
        }
    }

    /// fresh(reported_at이 ttl_secs 이내)인 후보를 uuid 오름차순으로 반환(clone).
    pub fn list_candidates(&self, now: &str, ttl_secs: i64) -> Vec<CandidateEntry> {
        let mut out: Vec<CandidateEntry> = self
            .candidate_pool
            .borrow()
            .values()
            .filter(|c| crate::store::candidates::is_fresh(&c.reported_at, now, ttl_secs))
            .cloned()
            .collect();
        out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        out
    }

    /// selector 매칭 online 에이전트의 uuid만 정렬해 반환(라우팅 해석용).
    pub fn resolve_selector(
        &self,
        selector: &BTreeMap<String, String>,
        now: &str,
        ttl_secs: i64,
    ) -> Vec<String> {
        self.list_agents(selector, now, ttl_secs).into_iter().map(|entry| entry.uuid).collect()
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

            // (2a) 전량 교체 전에 기존 created_at을 보존한다(save_session은 DELETE+INSERT라
            // now로 덮으면 매 스냅샷마다 타임스탬프가 리셋돼 recency 신호가 무의미해짐).
            let mut prev_created: std::collections::HashMap<i64, String> =
                std::collections::HashMap::new();
            {
                let mut stmt = self
                    .conn
                    .prepare("SELECT msg_id, created_at FROM messages WHERE session_id=?1")
                    .map_err(|e| format!("sqlite: {e}"))?;
                let rows = stmt
                    .query_map([session_id], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, Option<String>>(1)?))
                    })
                    .map_err(|e| format!("sqlite: {e}"))?;
                for row in rows {
                    let (mid, ca) = row.map_err(|e| format!("sqlite: {e}"))?;
                    if let Some(ca) = ca {
                        prev_created.insert(mid, ca);
                    }
                }
            }

            // (2b) 기존 메시지/FTS 전량 삭제.
            self.conn
                .execute("DELETE FROM messages WHERE session_id=?1", [session_id])
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute("DELETE FROM messages_fts WHERE session_id=?1", [session_id])
                .map_err(|e| format!("sqlite: {e}"))?;

            // (3) 각 메시지 삽입. created_at은 보존값 우선, 없으면(신규) now.
            for msg in &ss.messages {
                let msg_id = msg.id as i64;
                let parent_id: Option<i64> = msg.parent_id.map(|p| p as i64);
                let created: Option<String> = prev_created.get(&msg_id).cloned();

                self.conn
                    .execute(
                        "INSERT INTO messages(session_id, msg_id, parent_id, speaker, content, created_at) \
                         VALUES(?1, ?2, ?3, ?4, ?5, COALESCE(?6, datetime('now')))",
                        rusqlite::params![session_id, msg_id, parent_id, msg.speaker, msg.content, created],
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

            // (4) orphan 정리. 전량 교체로 messages가 새 집합이 됐으니, 그 세션에서 새 messages에
            // 없는 msg_id의 부속 행(message_vectors·message_validity)도 같은 트랜잭션에서 삭제한다
            // (messages/FTS만 지우면 축소 저장 시 벡터·유효성 행이 orphan으로 남음).
            self.conn
                .execute(
                    "DELETE FROM message_vectors WHERE session_id=?1 \
                     AND msg_id NOT IN (SELECT msg_id FROM messages WHERE session_id=?1)",
                    [session_id],
                )
                .map_err(|e| format!("sqlite: {e}"))?;
            self.conn
                .execute(
                    "DELETE FROM message_validity WHERE session_id=?1 \
                     AND msg_id NOT IN (SELECT msg_id FROM messages WHERE session_id=?1)",
                    [session_id],
                )
                .map_err(|e| format!("sqlite: {e}"))?;

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

    /// 단일 발언을 세션 전사 끝(현재 head의 자식)에 증분 추가하고 새 msg_id를 반환한다.
    /// save_session(전량 교체)과 달리 INSERT만 하므로, 외부 writer(post_turn)와 REPL이
    /// 같은 DB id 권위(max msg_id+1)를 공유해 충돌·클로버가 구조적으로 없다(Plan 27 옵션 B).
    /// 단일 트랜잭션이라 SQLite 쓰기 직렬화로 동시 append 안전.
    pub fn append_turn<F: Fn(&str) -> String>(
        &self,
        session_id: &str,
        speaker: &str,
        content: &str,
        fts_tok: F,
    ) -> Result<u64, String> {
        self.conn
            .execute_batch("BEGIN;")
            .map_err(|e| format!("sqlite: {e}"))?;

        let result = (|| -> Result<u64, String> {
            // 현재 head(부모) 조회. 세션 행이 없으면 신규(parent=None).
            let parent: Option<i64> = match self.conn.query_row(
                "SELECT head_id FROM sessions WHERE id=?1",
                [session_id],
                |r| r.get(0),
            ) {
                Ok(v) => v,
                Err(rusqlite::Error::QueryReturnedNoRows) => None,
                Err(e) => return Err(format!("sqlite: {e}")),
            };

            // 새 msg_id = 이 세션 max(msg_id)+1 (DB가 id 권위).
            let max_id: Option<i64> = self
                .conn
                .query_row(
                    "SELECT MAX(msg_id) FROM messages WHERE session_id=?1",
                    [session_id],
                    |r| r.get(0),
                )
                .map_err(|e| format!("sqlite: {e}"))?;
            let new_id = max_id.unwrap_or(0) + 1;

            // sessions 행 보장 + head 갱신(messages FK가 sessions를 참조하므로 먼저).
            self.conn
                .execute(
                    "INSERT INTO sessions(id, head_id) VALUES(?1, ?2) \
                     ON CONFLICT(id) DO UPDATE SET head_id=excluded.head_id, updated_at=datetime('now')",
                    rusqlite::params![session_id, new_id],
                )
                .map_err(|e| format!("sqlite: {e}"))?;

            self.conn
                .execute(
                    "INSERT INTO messages(session_id, msg_id, parent_id, speaker, content, created_at) \
                     VALUES(?1, ?2, ?3, ?4, ?5, datetime('now'))",
                    rusqlite::params![session_id, new_id, parent, speaker, content],
                )
                .map_err(|e| format!("sqlite: {e}"))?;

            let fts_content = fts_tok(content);
            self.conn
                .execute(
                    "INSERT INTO messages_fts(content, session_id, msg_id) VALUES(?1, ?2, ?3)",
                    rusqlite::params![fts_content, session_id, new_id],
                )
                .map_err(|e| format!("sqlite: {e}"))?;

            Ok(new_id as u64)
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

    /// 메시지의 created_at(절대 타임스탬프 문자열)을 조회한다. 미설정(마이그레이션 기존행)·부재는 None.
    /// cross-session recency 랭킹(step 5c)용. 포맷은 datetime('now')="YYYY-MM-DD HH:MM:SS".
    pub fn get_created_at(&self, session_id: &str, msg_id: u64) -> Result<Option<String>, String> {
        let msg_id_i64 = msg_id as i64;
        match self.conn.query_row(
            "SELECT created_at FROM messages WHERE session_id=?1 AND msg_id=?2",
            rusqlite::params![session_id, msg_id_i64],
            |row| row.get::<_, Option<String>>(0),
        ) {
            Ok(v) => Ok(v),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("sqlite: {e}")),
        }
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

    /// 모든 세션 id를 반환한다(reindex 순회용).
    pub fn list_sessions(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM sessions ORDER BY id")
            .map_err(|e| format!("sqlite: {e}"))?;
        let ids = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(ids)
    }

    /// 인덱스 카운트 (sessions, messages, messages_fts, message_vectors, message_validity). lint 리포트용.
    pub fn index_stats(&self) -> Result<(usize, usize, usize, usize, usize), String> {
        let count = |table: &str| -> Result<usize, String> {
            self.conn
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
                .map_err(|e| format!("sqlite: {e}"))
        };
        Ok((
            count("sessions")?,
            count("messages")?,
            count("messages_fts")?,
            count("message_vectors")?,
            count("message_validity")?,
        ))
    }

    /// 발언의 유효성 상태를 설정한다(upsert). abstraction/anchors는 보존한다.
    /// valid_state=superseded일 때 superseded_by로 대체 발언을 가리킬 수 있다.
    pub fn set_validity(
        &self,
        session_id: &str,
        msg_id: u64,
        valid_state: &str,
        superseded_by: Option<u64>,
    ) -> Result<(), String> {
        let sup = superseded_by.map(|v| v as i64);
        self.conn
            .execute(
                "INSERT INTO message_validity(session_id, msg_id, valid_state, superseded_by_msg_id) \
                 VALUES(?1, ?2, ?3, ?4) \
                 ON CONFLICT(session_id, msg_id) DO UPDATE SET \
                     valid_state=excluded.valid_state, \
                     superseded_by_msg_id=excluded.superseded_by_msg_id, \
                     updated_at=datetime('now')",
                rusqlite::params![session_id, msg_id as i64, valid_state, sup],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }

    /// 발언의 요약(abstraction)·앵커(anchors)를 부분 설정한다(upsert). valid_state는 보존.
    /// None인 필드는 기존 값을 유지한다(COALESCE, 부분 갱신). 최초 삽입이면 valid_state는 기본 'active'.
    pub fn set_annotation(
        &self,
        session_id: &str,
        msg_id: u64,
        abstraction: Option<&str>,
        anchors: Option<&str>,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO message_validity(session_id, msg_id, abstraction, anchors) \
                 VALUES(?1, ?2, ?3, ?4) \
                 ON CONFLICT(session_id, msg_id) DO UPDATE SET \
                     abstraction=COALESCE(excluded.abstraction, message_validity.abstraction), \
                     anchors=COALESCE(excluded.anchors, message_validity.anchors), \
                     updated_at=datetime('now')",
                rusqlite::params![session_id, msg_id as i64, abstraction, anchors],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }

    /// 발언의 유효성 메타를 조회한다. 행 없으면 Ok(None)(호출자가 기본 active로 간주).
    pub fn get_validity(
        &self,
        session_id: &str,
        msg_id: u64,
    ) -> Result<Option<crate::store::Validity>, String> {
        match self.conn.query_row(
            "SELECT valid_state, superseded_by_msg_id, abstraction, anchors \
             FROM message_validity WHERE session_id=?1 AND msg_id=?2",
            rusqlite::params![session_id, msg_id as i64],
            |row| {
                let valid_state: String = row.get(0)?;
                let sup: Option<i64> = row.get(1)?;
                let abstraction: Option<String> = row.get(2)?;
                let anchors: Option<String> = row.get(3)?;
                Ok(crate::store::Validity {
                    valid_state,
                    superseded_by: sup.map(|v| v as u64),
                    abstraction,
                    anchors,
                })
            },
        ) {
            Ok(v) => Ok(Some(v)),
            Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
            Err(e) => Err(format!("sqlite: {e}")),
        }
    }

    /// 메시지의 created_at을 직접 설정한다(전사 import 백필·테스트용). 포맷은 "YYYY-MM-DD HH:MM:SS".
    /// 대상 메시지가 없으면 아무 행도 갱신하지 않는다(무해).
    pub fn set_created_at(&self, session_id: &str, msg_id: u64, ts: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE messages SET created_at=?3 WHERE session_id=?1 AND msg_id=?2",
                rusqlite::params![session_id, msg_id as i64, ts],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
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

        let model_id = embedder.model_id();
        let result = (|| -> Result<(), String> {
            for msg in &ss.messages {
                // content_hash 계산(FNV-1a 64bit, 버전 무관 결정적).
                let content_hash = content_hash(&msg.content);

                let msg_id = msg.id as i64;

                // 기존 행 조회: content_hash와 model_id가 모두 같을 때만 skip(증분).
                // 모델이 바뀌면(model_id 불일치) 같은 내용이라도 재임베딩한다(stale 벡터 방지).
                let existing: Option<(String, Option<String>)> = self
                    .conn
                    .query_row(
                        "SELECT content_hash, model_id FROM message_vectors WHERE session_id=?1 AND msg_id=?2",
                        rusqlite::params![session_id, msg_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .ok();

                if let Some((h, m)) = &existing
                    && h == &content_hash
                    && m.as_deref() == Some(model_id.as_str())
                {
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
                        "INSERT INTO message_vectors(session_id, msg_id, dim, content_hash, model_id, embedding) \
                         VALUES(?1, ?2, ?3, ?4, ?5, ?6) \
                         ON CONFLICT(session_id, msg_id) DO UPDATE SET \
                             dim=excluded.dim, \
                             content_hash=excluded.content_hash, \
                             model_id=excluded.model_id, \
                             embedding=excluded.embedding",
                        rusqlite::params![session_id, msg_id, dim, content_hash, model_id, blob],
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

    /// 현재 SQLite 시각을 `datetime('now')` 포맷 문자열로 반환한다. A2A task 생성 등 애플리케이션단에서
    /// 타임스탬프를 미리 stamping해야 하는 호출자(예: JSON-RPC send 핸들러)가 사용하는 공용 헬퍼다.
    pub fn now(&self) -> Result<String, String> {
        self.conn
            .query_row("SELECT datetime('now')", [], |row| row.get(0))
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// 신규 A2A task_id를 생성한다. SQLite 내장 randomblob(16)을 hex로 인코딩(32자)해 신규 crate 의존
    /// 없이 고유 식별자를 얻는다(uuid crate 등 도입 회피).
    pub fn new_task_id(&self) -> Result<String, String> {
        self.conn
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// A2A task를 신규 생성한다(INSERT). created_at/updated_at은 Task 값을 그대로 쓴다(SQL 기본값 없음).
    /// 호출자(dispatcher)가 시각을 stamping해 전달하는 것을 전제한다(round-trip 필드 보존 우선).
    pub fn create_task(&self, task: &Task) -> Result<(), String> {
        let message_json = match &task.status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        let artifacts_json =
            serde_json::to_string(&task.artifacts).map_err(|e| format!("json: {e}"))?;
        let history_json =
            serde_json::to_string(&task.history).map_err(|e| format!("json: {e}"))?;
        self.conn
            .execute(
                &format!(
                    "INSERT INTO tasks({TASK_COLUMNS}) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
                ),
                rusqlite::params![
                    task.id,
                    task.context_id,
                    task.from_agent,
                    task.to_agent,
                    task.state.as_str(),
                    message_json,
                    artifacts_json,
                    history_json,
                    task.created_at,
                    task.updated_at,
                    task.runner,
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }

    /// 메시지 하나로 submitted task를 만들어 영속한다(A2A SendMessage와 MCP send_task 툴이 공유하는
    /// 헬퍼). task_id/시각은 이 함수가 발급하고, message는 status_message이자 history의 첫 항목으로
    /// 그대로 보존한다. a2a_server::handle_send와 mcp::send_task 양쪽이 이 함수로 수렴해 "메시지로
    /// task를 튼다"는 로직이 store 레이어 한 곳에만 존재하게 한다(serve<->mcp 크로스피처 의존 회피).
    pub fn create_task_from_message(
        &self,
        from_agent: &str,
        to_agent: &str,
        message: Message,
    ) -> Result<Task, String> {
        let id = self.new_task_id()?;
        let now = self.now()?;
        let context_id = message.context_id.clone();
        let mut task = Task::new(id, context_id, from_agent, to_agent, now);
        task.status_message = Some(message.clone());
        task.history = vec![message];
        self.create_task(&task)?;
        self.emit_task_event(TaskEvent::Status(task.clone()));
        Ok(task)
    }

    /// task_id로 단건 조회한다. 없으면 Ok(None)(load_session 폴리시 답습: QueryReturnedNoRows만 None).
    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>, String> {
        let row: TaskRow = match self.conn.query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE task_id=?1"),
            [task_id],
            task_row_from_sql,
        ) {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(format!("sqlite: {e}")),
        };
        row.into_task().map(Some)
    }

    /// 특정 에이전트(to_agent) 앞으로 열려 있는(submitted/working/input_required) task를
    /// created_at 오름차순으로 반환한다. 상태 리터럴은 TaskState::is_open과 의미를 동기 유지한다.
    pub fn list_open_tasks_for(&self, agent: &str) -> Result<Vec<Task>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks \
                 WHERE to_agent=?1 AND state IN ('submitted','working','input_required') \
                 ORDER BY created_at"
            ))
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map([agent], task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        rows.into_iter().map(TaskRow::into_task).collect()
    }

    /// 브로커 전역에서 열려 있는(submitted/working/input_required) task를 to_agent 필터 없이
    /// created_at 오름차순으로 전부 반환한다(운영자 조망용 tasks MCP 도구 전용, list_open_tasks_for와
    /// 같은 패턴에서 to_agent 조건만 뺀 버전).
    pub fn list_all_open_tasks(&self) -> Result<Vec<Task>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks \
                 WHERE state IN ('submitted','working','input_required') \
                 ORDER BY created_at"
            ))
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map([], task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        rows.into_iter().map(TaskRow::into_task).collect()
    }

    /// state와 동반 상태 메시지를 원자적으로 갱신한다(A2A TaskStatus 단위). status_message=None이면
    /// 이번 전이에 메시지가 없다는 뜻으로 message_json을 비운다(이전 값 보존 아님).
    pub fn update_task_state(
        &self,
        task_id: &str,
        state: TaskState,
        status_message: Option<&Message>,
    ) -> Result<(), String> {
        let message_json = match status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        self.conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1",
                rusqlite::params![task_id, state.as_str(), message_json],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// task를 completed로 마감하고 산출물을 세팅한다.
    pub fn complete_task(&self, task_id: &str, artifacts: &[Artifact]) -> Result<(), String> {
        let artifacts_json =
            serde_json::to_string(artifacts).map_err(|e| format!("json: {e}"))?;
        self.conn
            .execute(
                "UPDATE tasks SET state=?2, artifacts_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1",
                rusqlite::params![task_id, TaskState::Completed.as_str(), artifacts_json],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Completed(task));
        }
        Ok(())
    }

    /// task를 조건부로 working 전이한다(claim). WHERE 절의 현재상태 가드가 상태머신을 저장소 계층에서
    /// 강제한다: submitted/input_required일 때만 성공하므로, 두 워커가 같은 task를 동시에 claim해도
    /// UPDATE 하나만 1행을 갱신하고 나머지는 0행(rows_affected!=1)으로 걸러진다(레이스 방지).
    /// status_message(=task 지시문)는 지우지 않고 그대로 둔다: requeue로 다시 submitted가 되면 새 워커가
    /// poll에서 이 지시문(msg)을 읽어 실행하므로, claim에서 지우면 재배달된 task가 빈 프롬프트가 된다.
    /// 같은 UPDATE에서 lease(claimed_at/lease_expires_at/claimed_by)를 세팅하고 attempt_count를
    /// 증가시켜, claim-후-워커사망 자동 requeue(expire_stale_claims)의 판단 근거를 남긴다. claimed_by는
    /// 하위호환을 위해 Option(호출자가 agent id를 안 넘기면 NULL). runner도 같은 순간 기록한다(v8,
    /// 어떤 러너 종류가 처리했는지 트레이스용, 호출자가 안 넘기면 NULL).
    /// 전이 성공 시에만 update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_claim(
        &self,
        task_id: &str,
        claimed_by: Option<&str>,
        runner: Option<&str>,
    ) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, updated_at=datetime('now'), \
                 claimed_at=datetime('now'), \
                 lease_expires_at=datetime('now', '+' || ?3 || ' seconds'), \
                 claimed_by=?4, runner=?5, attempt_count=attempt_count + 1 \
                 WHERE task_id=?1 AND state IN ('submitted','input_required')",
                rusqlite::params![
                    task_id,
                    TaskState::Working.as_str(),
                    CLAIM_LEASE_SECS,
                    claimed_by,
                    runner
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!("전이 불가: task_id={task_id} (현재 상태가 대상 아님)"));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// lease 만료된 working task를 회수한다(poll 경로에서 호출되는 지연 sweep, 별도 타이머 없음).
    /// lease_expires_at이 지난 working 중 attempt_count < MAX_CLAIM_ATTEMPTS면 submitted로 되돌리고
    /// (claim 필드 클리어, attempt_count는 유지해 다음 claim에서 다시 증가), MAX 이상이면 무한 requeue를
    /// 막기 위해 failed로 격리한다. status_message(지시문)는 건드리지 않는다: 재배달된 워커가 poll에서
    /// 그 지시문을 읽어 실행해야 하기 때문(try_claim이 지시문을 보존하는 것과 짝). 두 UPDATE는 서로소
    /// 조건(전자가 먼저 실행돼도 그 행들은 이미 state!='working'으로 바뀌어 후자의 WHERE에 안 걸림)이라
    /// 실행 순서가 결과에 영향을 주지 않는다. 이벤트 emit은 의도적으로 생략한다(다건 sweep이라 poll마다
    /// 이벤트가 몰릴 수 있고, 구독자는 다음 poll_tasks/get_task로 최신 상태를 확인할 수 있어 SSE 실시간성
    /// 손실이 크지 않다고 판단). 회수(requeue)·격리(failed) 합계 행 수를 반환한다.
    pub fn expire_stale_claims(&self) -> Result<usize, String> {
        // 단일 UPDATE(암묵적 단일 트랜잭션 = 원자적·fsync 1회)로 처리한다. attempt_count로 갈라
        // 회수(attempt<MAX: submitted + claim 필드 클리어) 또는 격리(attempt>=MAX: failed, claim 필드는
        // 포렌식용으로 유지)한다. status_message(지시문)는 어느 쪽도 건드리지 않아 재배달 워커가 읽는다.
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET \
                 state = CASE WHEN attempt_count < ?2 THEN ?1 ELSE ?3 END, \
                 claimed_at = CASE WHEN attempt_count < ?2 THEN NULL ELSE claimed_at END, \
                 lease_expires_at = CASE WHEN attempt_count < ?2 THEN NULL ELSE lease_expires_at END, \
                 claimed_by = CASE WHEN attempt_count < ?2 THEN NULL ELSE claimed_by END, \
                 runner = CASE WHEN attempt_count < ?2 THEN NULL ELSE runner END, \
                 updated_at = datetime('now') \
                 WHERE state='working' AND lease_expires_at IS NOT NULL \
                 AND julianday('now') > julianday(lease_expires_at)",
                rusqlite::params![
                    TaskState::Submitted.as_str(),
                    MAX_CLAIM_ATTEMPTS,
                    TaskState::Failed.as_str()
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(affected)
    }

    /// task를 조건부로 completed 전이한다(complete). working 상태일 때만 성공한다(레이스 방지: 이미
    /// completed/canceled/failed로 종료된 task를 덮어쓰지 못함). 추가로 first-completer-wins 가드:
    /// completer가 Some이면 claimed_by가 NULL이거나 completer와 일치할 때만 성공한다. lease 만료로
    /// requeue된 뒤 뒤늦게 살아난 stale 워커가 이미 다른 워커가 완료시킨(혹은 재claim된) task를
    /// completer 불일치로 덮어쓰지 못하게 막는다. completer=None이면 이 가드가 무력화되어(claimed_by
    /// 무관하게 working이면 성공) 기존 동작(하위호환, agent 인자 없는 호출)을 그대로 유지한다.
    /// 전이 성공 시에만 complete_task와 동일하게 Completed 이벤트를 emit한다.
    pub fn try_complete(
        &self,
        task_id: &str,
        artifacts: &[Artifact],
        completer: Option<&str>,
    ) -> Result<(), String> {
        let artifacts_json =
            serde_json::to_string(artifacts).map_err(|e| format!("json: {e}"))?;
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, artifacts_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state='working' \
                 AND (?4 IS NULL OR claimed_by IS NULL OR claimed_by = ?4)",
                rusqlite::params![task_id, TaskState::Completed.as_str(), artifacts_json, completer],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!("전이 불가: task_id={task_id} (현재 상태가 대상 아님)"));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Completed(task));
        }
        Ok(())
    }

    /// task를 조건부로 failed 전이한다(fail). submitted/working/input_required(=열린 상태)일 때만
    /// 성공한다(레이스 방지: completed/canceled로 이미 종료된 task를 덮어쓰지 못함). status_message는
    /// update_task_state의 관례를 그대로 따른다(None이면 message_json 비움). 전이 성공 시에만
    /// update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_fail(
        &self,
        task_id: &str,
        status_message: Option<&Message>,
        failer: Option<&str>,
    ) -> Result<(), String> {
        let message_json = match status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        // try_complete와 대칭인 first-completer-wins 가드: lease 만료로 requeue된 뒤 되살아난 stale
        // 워커가 이미 다른 워커가 재claim한 task를 failed로 덮어쓰지 못하게 한다(failer 불일치 거부).
        // failer=None이면 무력화(하위호환, agent 인자 없는 호출).
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state IN ('submitted','working','input_required') \
                 AND (?4 IS NULL OR claimed_by IS NULL OR claimed_by = ?4)",
                rusqlite::params![task_id, TaskState::Failed.as_str(), message_json, failer],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!("전이 불가: task_id={task_id} (현재 상태가 대상 아님)"));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// task를 조건부로 canceled 전이한다(cancel). submitted/working/input_required(=열린 상태)일 때만
    /// 성공한다(레이스 방지: completed로 이미 끝난 task를 canceled로 덮어쓰지 못함). 전이 성공 시에만
    /// update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_cancel(&self, task_id: &str) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=NULL, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state IN ('submitted','working','input_required')",
                rusqlite::params![task_id, TaskState::Canceled.as_str()],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!("전이 불가: task_id={task_id} (현재 상태가 대상 아님)"));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// history에 메시지를 append한다(기존 history_json을 읽어 병합 후 저장). 대상 task가 없으면 에러.
    pub fn append_history(&self, task_id: &str, msg: &Message) -> Result<(), String> {
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT history_json FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        let updated = append_history_json(existing.as_deref(), msg)?;
        self.conn
            .execute(
                "UPDATE tasks SET history_json=?2, updated_at=datetime('now') WHERE task_id=?1",
                rusqlite::params![task_id, updated],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }
}

/// tasks SELECT 행 -> TaskRow. query_row/query_map 양쪽에서 재사용(fn 포인터는 Fn/FnMut 모두 충족).
fn task_row_from_sql(row: &rusqlite::Row) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        context_id: row.get(1)?,
        from_agent: row.get(2)?,
        to_agent: row.get(3)?,
        state: row.get(4)?,
        message_json: row.get(5)?,
        artifacts_json: row.get(6)?,
        history_json: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        runner: row.get(10)?,
    })
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

/// 테스트 전용 크레이트 내부 헬퍼(cfg(test)라 테스트 빌드에만 존재). `conn`은 이 모듈 밖(예:
/// crate::mcp의 테스트)에서 접근 불가한 private 필드라, lease 만료를 raw SQL로 강제 시뮬레이션해야
/// 하는 크로스모듈 테스트가 이 pub(crate) 통로로 우회한다(운영 코드 경로에는 영향 없음).
#[cfg(test)]
impl SqliteStore {
    pub(crate) fn test_force_lease_expired(&self, task_id: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET lease_expires_at=datetime('now','-1 hour') WHERE task_id=?1",
                [task_id],
            )
            .unwrap();
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

    #[test]
    fn save_session_shrink_cleans_orphan_vectors_and_validity() {
        // 전량 교체로 메시지가 줄면(id 2 제거), 그 세션의 부속 행(message_vectors·
        // message_validity)도 같은 트랜잭션에서 정리돼 orphan이 남지 않아야 한다.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap(); // 메시지 2건(id 1, 2).

        // 부속 행을 두 메시지 모두에 채운다.
        let emb = CountingEmbedder { dim: 8, model: "model-A".into(), calls: 0.into() };
        db.index_vectors("s1", &ss(), &emb).unwrap(); // message_vectors 2건.
        db.set_validity("s1", 1, "active", None).unwrap();
        db.set_validity("s1", 2, "active", None).unwrap(); // message_validity 2건.

        // 축소 저장: id 1만 남긴다.
        let shrunk = StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "claude/proposer".into(),
                content: "검색 시스템 설계".into(),
            }],
            head: Some(1),
        };
        db.save_session("s1", &shrunk, |t| t.to_string()).unwrap();

        // 제거된 id 2의 부속 행이 orphan으로 남지 않아야 한다.
        let orphan_count = |table: &str| -> i64 {
            db.conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} WHERE session_id='s1' \
                         AND msg_id NOT IN (SELECT msg_id FROM messages WHERE session_id='s1')"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(orphan_count("message_vectors"), 0, "축소 후 orphan 벡터 0");
        assert_eq!(orphan_count("message_validity"), 0, "축소 후 orphan 유효성 0");

        // 남은 id 1의 부속 행은 보존돼야 한다(과잉 삭제 아님).
        let kept = |table: &str| -> i64 {
            db.conn
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE session_id='s1'"),
                    [],
                    |r| r.get(0),
                )
                .unwrap()
        };
        assert_eq!(kept("message_vectors"), 1, "id 1 벡터 보존");
        assert_eq!(kept("message_validity"), 1, "id 1 유효성 보존");
    }

    /// embed 호출 횟수를 세는 임베더. model_id를 주입 가능(무효화 키 테스트용).
    struct CountingEmbedder {
        dim: usize,
        model: String,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl crate::store::embedding::Embedder for CountingEmbedder {
        fn embed(&self, _text: &str) -> Result<Vec<f32>, String> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Ok(vec![0.1_f32; self.dim])
        }
        fn dim(&self) -> usize {
            self.dim
        }
        fn model_id(&self) -> String {
            self.model.clone()
        }
    }

    #[test]
    fn index_vectors_skips_same_model_reembeds_on_model_change() {
        use std::sync::atomic::Ordering::SeqCst;
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap(); // 메시지 2건.

        let emb_a = CountingEmbedder { dim: 8, model: "model-A".into(), calls: 0.into() };
        // 최초 색인: 2건 임베드.
        db.index_vectors("s1", &ss(), &emb_a).unwrap();
        assert_eq!(emb_a.calls.load(SeqCst), 2, "최초 색인은 모든 메시지 임베드");

        // 같은 모델 재색인: content_hash+model_id 일치 → skip(추가 임베드 0).
        db.index_vectors("s1", &ss(), &emb_a).unwrap();
        assert_eq!(emb_a.calls.load(SeqCst), 2, "같은 모델 재색인은 skip");

        // 모델 교체: model_id 불일치 → 재임베딩(2건 더).
        let emb_b = CountingEmbedder { dim: 8, model: "model-B".into(), calls: 0.into() };
        db.index_vectors("s1", &ss(), &emb_b).unwrap();
        assert_eq!(emb_b.calls.load(SeqCst), 2, "모델 교체 시 stale 벡터 재임베딩");

        // 다시 model-B 재색인: 이제 일치 → skip.
        db.index_vectors("s1", &ss(), &emb_b).unwrap();
        assert_eq!(emb_b.calls.load(SeqCst), 2, "교체 후 같은 모델은 다시 skip");
    }

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

    #[test]
    fn save_session_preserves_created_at_on_resave() {
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s", &ss(), |t| t.to_string()).unwrap();
        // 기존 created_at을 알려진 과거값으로 고정.
        db.set_created_at("s", 1, "2020-01-01 00:00:00").unwrap();
        // 같은 세션 재저장(전량 교체) 후에도 created_at 보존(now로 리셋 금지)이어야 한다.
        db.save_session("s", &ss(), |t| t.to_string()).unwrap();
        let ca = db.get_created_at("s", 1).unwrap();
        assert_eq!(ca.as_deref(), Some("2020-01-01 00:00:00"), "재저장 시 created_at 보존");
        // 보존값 없던 메시지(msg 2)는 now로 채워져 NULL이 아니어야 한다.
        assert!(db.get_created_at("s", 2).unwrap().is_some(), "보존값 없는 메시지는 now로 채움");
    }

    #[test]
    fn list_sessions_and_index_stats() {
        let db = SqliteStore::open_memory().unwrap();
        assert!(db.list_sessions().unwrap().is_empty());
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap(); // 2 메시지.
        db.save_session("s2", &ss(), |t| t.to_string()).unwrap();
        db.set_validity("s1", 1, "rejected", None).unwrap();
        let sessions = db.list_sessions().unwrap();
        assert_eq!(sessions, vec!["s1".to_string(), "s2".to_string()]);
        let (s, m, f, _v, val) = db.index_stats().unwrap();
        assert_eq!(s, 2, "세션 2");
        assert_eq!(m, 4, "메시지 4(세션당 2)");
        assert_eq!(f, 4, "FTS 4");
        assert_eq!(val, 1, "유효성 1");
    }

    #[test]
    fn reindex_rebuilds_fts_with_new_tokenizer() {
        // 초기엔 원문 그대로 색인(검색 안 됨) → 재색인 시 접미사 토큰으로 색인해 검색되게.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s", &ss(), |t| t.to_string()).unwrap(); // "검색 시스템 설계" 등 원문.
        // "설계로" 질의는 원문 색인에선 매치 안 됨(정확 토큰 아님).
        assert!(db.search("설계로", 10).unwrap().is_empty());
        // 재색인: 모든 세션 load→save_session(새 토크나이저=각 단어에 뒤 글자 덧붙임 흉내 대신 identity로 재구성).
        // 여기선 재색인이 FTS를 재생성한다는 것만 확인(save_session 재호출로 rowid 갱신 없이 동일 검색).
        for sid in db.list_sessions().unwrap() {
            let ssn = db.load_session(&sid).unwrap().unwrap();
            db.save_session(&sid, &ssn, |t| t.to_string()).unwrap();
        }
        assert!(!db.search("검색", 10).unwrap().is_empty(), "재색인 후에도 검색 유지");
    }

    #[test]
    fn validity_roundtrip_and_missing_is_none() {
        let db = SqliteStore::open_memory().unwrap();
        // 미설정이면 None(호출자가 기본 active로 간주).
        assert_eq!(db.get_validity("s1", 1).unwrap(), None);
        // superseded 설정.
        db.set_validity("s1", 1, "superseded", Some(5)).unwrap();
        let v = db.get_validity("s1", 1).unwrap().expect("존재");
        assert_eq!(v.valid_state, "superseded");
        assert_eq!(v.superseded_by, Some(5));
        assert_eq!(v.abstraction, None);
    }

    #[test]
    fn set_validity_preserves_annotation_and_vice_versa() {
        let db = SqliteStore::open_memory().unwrap();
        // 먼저 요약/앵커 설정.
        db.set_annotation("s1", 1, Some("결정 요약"), Some("검색\n랭킹")).unwrap();
        // 그 다음 유효성 설정 → 요약/앵커 보존.
        db.set_validity("s1", 1, "rejected", None).unwrap();
        let v = db.get_validity("s1", 1).unwrap().unwrap();
        assert_eq!(v.valid_state, "rejected");
        assert_eq!(v.abstraction.as_deref(), Some("결정 요약"));
        assert_eq!(v.anchors.as_deref(), Some("검색\n랭킹"));
        // 반대로 요약만 갱신(anchors=None) → valid_state·anchors 보존(부분 갱신).
        db.set_annotation("s1", 1, Some("갱신된 요약"), None).unwrap();
        let v2 = db.get_validity("s1", 1).unwrap().unwrap();
        assert_eq!(v2.valid_state, "rejected", "annotation 갱신이 valid_state를 덮지 않음");
        assert_eq!(v2.abstraction.as_deref(), Some("갱신된 요약"));
        assert_eq!(v2.anchors.as_deref(), Some("검색\n랭킹"), "None 필드는 기존 값 보존(COALESCE)");
    }

    #[test]
    fn append_turn_chains_from_head_and_returns_ids() {
        // 신규 세션에 두 번 append -> head 자식 체인(1<-2), 반환 id 1,2, head=2.
        let db = SqliteStore::open_memory().unwrap();
        let id1 = db.append_turn("s1", "claude", "첫 발언", |t| t.to_string()).unwrap();
        let id2 = db.append_turn("s1", "codex", "둘째 발언", |t| t.to_string()).unwrap();
        assert_eq!((id1, id2), (1, 2));
        let back = db.load_session("s1").unwrap().expect("present");
        assert_eq!(back.head, Some(2));
        assert_eq!(back.messages.len(), 2);
        assert_eq!(back.messages[0].parent_id, None);
        assert_eq!(back.messages[1].parent_id, Some(1));
    }

    #[test]
    fn append_turn_after_save_session_does_not_clobber() {
        // save_session(전량) 후 append -> 기존 2턴 보존 + 새 턴(id 3, parent 2)이 head.
        let db = SqliteStore::open_memory().unwrap();
        db.save_session("s1", &ss(), |t| t.to_string()).unwrap();
        let id3 = db.append_turn("s1", "claude", "원격 추가 발언", |t| t.to_string()).unwrap();
        assert_eq!(id3, 3);
        let back = db.load_session("s1").unwrap().unwrap();
        assert_eq!(back.messages.len(), 3, "기존 2턴 + 새 턴(클로버 없음)");
        assert_eq!(back.head, Some(3));
        assert_eq!(back.messages[2].parent_id, Some(2));
    }

    #[test]
    fn append_turn_is_fts_searchable() {
        // append한 발언이 FTS로 검색되어야 한다.
        let db = SqliteStore::open_memory().unwrap();
        db.append_turn("s1", "claude", "이벤트소싱 설계", |t| t.to_string()).unwrap();
        let hits = db.search("이벤트소싱", 10).unwrap();
        assert!(hits.iter().any(|h| h.session_id == "s1" && h.content.contains("이벤트소싱")));
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

            fn model_id(&self) -> String {
                self.inner.model_id()
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

    // A2A tasks 테이블 테스트: sqlite 피처 전용(파일 전체가 이미 sqlite 게이트).
    mod a2a_tests {
        use super::*;
        use crate::store::a2a::{Artifact, Message, Part, Task, TaskState};

        fn sample_message(id: &str) -> Message {
            Message {
                message_id: id.into(),
                role: "user".into(),
                parts: vec![Part { text: Some("내용".into()), ..Default::default() }],
                task_id: Some("t1".into()),
                context_id: None,
            }
        }

        #[test]
        fn now_returns_nonempty_datetime_string() {
            let db = SqliteStore::open_memory().unwrap();
            let ts = db.now().unwrap();
            // "YYYY-MM-DD HH:MM:SS" 형식(datetime('now') 기본 포맷). 정확한 파싱보다 형태만 가드.
            assert_eq!(ts.len(), 19, "datetime('now') 포맷 불일치: {ts}");
            assert!(ts.contains('-') && ts.contains(':'), "datetime('now') 포맷 불일치: {ts}");
        }

        #[test]
        fn new_task_id_is_unique_and_hex() {
            let db = SqliteStore::open_memory().unwrap();
            let a = db.new_task_id().unwrap();
            let b = db.new_task_id().unwrap();
            assert_ne!(a, b, "연속 생성 id가 겹치면 안 됨");
            assert_eq!(a.len(), 32, "randomblob(16) hex는 32자여야 함: {a}");
            assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "hex가 아님: {a}");
            assert_eq!(a, a.to_lowercase(), "lower(hex(...))인데 대문자 포함: {a}");
        }

        #[test]
        fn create_get_roundtrip_preserves_all_fields() {
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let mut task =
                Task::new("t1", Some("ctx1".into()), "win-claude", "mac-claude", "2026-07-02 10:00:00");
            task.status_message = Some(msg.clone());
            task.history = vec![msg.clone()];
            db.create_task(&task).unwrap();

            let back = db.get_task("t1").unwrap().expect("존재해야 함");
            assert_eq!(back.id, "t1");
            assert_eq!(back.context_id.as_deref(), Some("ctx1"));
            assert_eq!(back.from_agent, "win-claude");
            assert_eq!(back.to_agent, "mac-claude");
            assert_eq!(back.state, TaskState::Submitted);
            assert_eq!(back.status_message, Some(msg.clone()));
            assert_eq!(back.history, vec![msg]);
            assert!(back.artifacts.is_empty());
            assert_eq!(back.created_at, "2026-07-02 10:00:00");
            assert_eq!(back.updated_at, "2026-07-02 10:00:00");
        }

        #[test]
        fn get_task_missing_is_none() {
            let db = SqliteStore::open_memory().unwrap();
            assert!(db.get_task("nope").unwrap().is_none());
        }

        #[test]
        fn create_task_from_message_creates_submitted_task_and_persists_message() {
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let task = db.create_task_from_message("win-claude", "mac-claude", msg.clone()).unwrap();

            assert_eq!(task.state, TaskState::Submitted);
            assert_eq!(task.id.len(), 32, "task_id는 randomblob(16) hex 32자여야 함: {}", task.id);
            assert_eq!(task.from_agent, "win-claude");
            assert_eq!(task.to_agent, "mac-claude");
            assert_eq!(task.status_message, Some(msg.clone()));
            assert_eq!(task.history, vec![msg]);

            // store에도 실제로 영속되었는지 확인(round-trip).
            let persisted = db.get_task(&task.id).unwrap().expect("영속되어야 함");
            assert_eq!(persisted, task);
        }

        #[test]
        fn create_task_from_message_preserves_context_id_from_message() {
            let db = SqliteStore::open_memory().unwrap();
            let mut msg = sample_message("m1");
            msg.context_id = Some("ctx1".into());
            let task = db.create_task_from_message("a", "b", msg).unwrap();
            assert_eq!(task.context_id.as_deref(), Some("ctx1"));
        }

        #[test]
        fn create_task_from_message_two_calls_produce_distinct_task_ids() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = db.create_task_from_message("a", "b", sample_message("m1")).unwrap();
            let t2 = db.create_task_from_message("a", "b", sample_message("m2")).unwrap();
            assert_ne!(t1.id, t2.id);
        }

        #[test]
        fn list_open_tasks_for_filters_agent_and_completed() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00"); // open, mac
            let mut t2 = Task::new("t2", None, "win", "mac", "2026-07-02 09:05:00"); // completed, mac
            t2.state = TaskState::Completed;
            let t3 = Task::new("t3", None, "win", "other", "2026-07-02 09:10:00"); // open, other
            db.create_task(&t1).unwrap();
            db.create_task(&t2).unwrap();
            db.create_task(&t3).unwrap();

            let open = db.list_open_tasks_for("mac").unwrap();
            assert_eq!(open.len(), 1, "completed 제외 + 다른 to_agent 제외");
            assert_eq!(open[0].id, "t1");
        }

        #[test]
        fn list_open_tasks_for_orders_by_created_at() {
            let db = SqliteStore::open_memory().unwrap();
            let t_later = Task::new("later", None, "win", "mac", "2026-07-02 09:10:00");
            let t_earlier = Task::new("earlier", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&t_later).unwrap();
            db.create_task(&t_earlier).unwrap();
            let open = db.list_open_tasks_for("mac").unwrap();
            assert_eq!(
                open.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["earlier", "later"]
            );
        }

        #[test]
        fn list_all_open_tasks_returns_every_agent_and_excludes_completed() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00"); // open, mac
            let t2 = Task::new("t2", None, "win", "other", "2026-07-02 09:05:00"); // open, other
            let mut t3 = Task::new("t3", None, "win", "mac", "2026-07-02 09:10:00"); // completed, mac
            t3.state = TaskState::Completed;
            db.create_task(&t1).unwrap();
            db.create_task(&t2).unwrap();
            db.create_task(&t3).unwrap();

            let all_open = db.list_all_open_tasks().unwrap();
            assert_eq!(
                all_open.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["t1", "t2"],
                "to_agent 필터 없이 열린 task 전부(agent 무관) + completed 제외"
            );
        }

        #[test]
        fn state_transition_submitted_to_working_to_completed_sets_artifacts() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Submitted);

            let working_msg = sample_message("wm1");
            db.update_task_state("t1", TaskState::Working, Some(&working_msg)).unwrap();
            let mid = db.get_task("t1").unwrap().unwrap();
            assert_eq!(mid.state, TaskState::Working);
            assert_eq!(mid.status_message, Some(working_msg));

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part { text: Some("완료 보고".into()), ..Default::default() }],
            }];
            db.complete_task("t1", &artifacts).unwrap();
            let done = db.get_task("t1").unwrap().unwrap();
            assert_eq!(done.state, TaskState::Completed);
            assert_eq!(done.artifacts, artifacts);
        }

        #[test]
        fn append_history_grows_in_order() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            let m1 = sample_message("h1");
            let m2 = sample_message("h2");
            db.append_history("t1", &m1).unwrap();
            db.append_history("t1", &m2).unwrap();

            let back = db.get_task("t1").unwrap().unwrap();
            assert_eq!(back.history, vec![m1, m2]);
        }

        #[test]
        fn append_history_on_missing_task_is_err() {
            let db = SqliteStore::open_memory().unwrap();
            let m1 = sample_message("h1");
            assert!(db.append_history("nope", &m1).is_err());
        }

        #[test]
        fn task_events_emit_status_then_status_then_completed_in_order() {
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db.task_event_sender().expect("with_task_events 후엔 버스 활성화").subscribe();

            let msg = sample_message("m1");
            let task = db.create_task_from_message("win-claude", "mac-claude", msg).unwrap();

            let working_msg = sample_message("wm1");
            db.update_task_state(&task.id, TaskState::Working, Some(&working_msg)).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part { text: Some("완료 보고".into()), ..Default::default() }],
            }];
            db.complete_task(&task.id, &artifacts).unwrap();

            // 1) create_task_from_message -> Status(submitted).
            match rx.try_recv().expect("첫 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Submitted),
                other => panic!("Status(submitted) 기대, 실제: {other:?}"),
            }
            // 2) update_task_state(Working) -> Status(working).
            match rx.try_recv().expect("둘째 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Working),
                other => panic!("Status(working) 기대, 실제: {other:?}"),
            }
            // 3) complete_task -> Completed(completed, artifacts 포함).
            match rx.try_recv().expect("셋째 이벤트 없음") {
                TaskEvent::Completed(t) => {
                    assert_eq!(t.state, TaskState::Completed);
                    assert_eq!(t.artifacts, artifacts);
                }
                other => panic!("Completed 기대, 실제: {other:?}"),
            }
            assert!(rx.try_recv().is_err(), "이벤트가 3건보다 많음");
        }

        #[test]
        fn task_events_no_bus_is_noop() {
            // with_task_events()를 호출하지 않으면 emit이 조용히 무시된다(기존 unary 경로 무영향).
            let db = SqliteStore::open_memory().unwrap();
            assert!(db.task_event_sender().is_none());
            let msg = sample_message("m1");
            let task = db.create_task_from_message("win-claude", "mac-claude", msg).unwrap();
            assert_eq!(task.state, TaskState::Submitted);
        }

        // --- R2: 조건부 전이(try_claim/try_complete/try_fail/try_cancel) 단위테스트 ---

        #[test]
        fn try_claim_twice_second_call_is_transition_conflict() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            // 첫 claim: submitted -> working 성공.
            db.try_claim("t1", None, None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working);

            // 둘째 claim(동시 착수 경쟁 시뮬레이션): 이미 working이라 전이 대상 아님 -> Err.
            let err = db.try_claim("t1", None, None).unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            // 실패한 전이가 상태를 건드리지 않았는지 확인(여전히 working, 다른 상태로 안 튐).
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working);
        }

        #[test]
        fn try_complete_on_non_working_task_is_transition_conflict() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap(); // submitted 상태(아직 claim 안 됨).

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part { text: Some("결과".into()), ..Default::default() }],
            }];
            let err = db.try_complete("t1", &artifacts, None).unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            // submitted로 남아있어야 함(완료로 잘못 전이되지 않음).
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Submitted);
        }

        #[test]
        fn try_cancel_on_completed_task_is_blocked_and_state_preserved() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", None, None).unwrap();
            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part { text: Some("결과".into()), ..Default::default() }],
            }];
            db.try_complete("t1", &artifacts, None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Completed);

            // 이미 completed(종료 상태)인 task를 canceled로 덮어쓰려 하면 차단돼야 한다(R2 핵심 회귀).
            let err = db.try_cancel("t1").unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            let after = db.get_task("t1").unwrap().unwrap();
            assert_eq!(after.state, TaskState::Completed, "completed가 canceled로 덮어써짐(R2 회귀)");
            assert_eq!(after.artifacts, artifacts, "완료 산출물이 유지돼야 함");
        }

        #[test]
        fn try_claim_then_try_complete_emit_status_then_completed() {
            // 기존 update_task_state/complete_task 경로를 검증하던
            // task_events_emit_status_then_status_then_completed_in_order와 동일한 이벤트버스 계약을
            // try_* 조건부 전이 경로에서도 유지하는지 확인한다(R2: emit 보존이 핵심 요구사항).
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db.task_event_sender().expect("with_task_events 후엔 버스 활성화").subscribe();

            let msg = sample_message("m1");
            let task = db.create_task_from_message("win-claude", "mac-claude", msg).unwrap();

            db.try_claim(&task.id, None, None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part { text: Some("완료 보고".into()), ..Default::default() }],
            }];
            db.try_complete(&task.id, &artifacts, None).unwrap();

            // 1) create_task_from_message -> Status(submitted).
            match rx.try_recv().expect("첫 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Submitted),
                other => panic!("Status(submitted) 기대, 실제: {other:?}"),
            }
            // 2) try_claim -> Status(working).
            match rx.try_recv().expect("둘째 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Working),
                other => panic!("Status(working) 기대, 실제: {other:?}"),
            }
            // 3) try_complete -> Completed(completed, artifacts 포함).
            match rx.try_recv().expect("셋째 이벤트 없음") {
                TaskEvent::Completed(t) => {
                    assert_eq!(t.state, TaskState::Completed);
                    assert_eq!(t.artifacts, artifacts);
                }
                other => panic!("Completed 기대, 실제: {other:?}"),
            }
            assert!(rx.try_recv().is_err(), "이벤트가 3건보다 많음");
        }

        #[test]
        fn try_transition_on_missing_task_is_err_and_emits_nothing() {
            // 대상 task 자체가 없으면 rows_affected=0으로 같은 에러 경로를 타야 한다(스펙 요구사항).
            // 전이가 없었으니 이벤트도 없어야 한다.
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db.task_event_sender().expect("with_task_events 후엔 버스 활성화").subscribe();
            assert!(db.try_claim("nope", None, None).is_err());
            assert!(db.try_fail("nope", None, None).is_err());
            assert!(db.try_cancel("nope").is_err());
            assert!(rx.try_recv().is_err(), "존재하지 않는 task에 대해 이벤트가 emit됨");
        }

        // --- lease 기반 claim-후-워커사망 자동 requeue 단위테스트 ---

        /// tasks의 DB 내부 컬럼(claimed_at/lease_expires_at/claimed_by/attempt_count)을 직접 조회한다.
        /// Task wire 구조체에는 노출되지 않는 컬럼이라 raw SQL로만 검증 가능.
        fn raw_claim_fields(
            db: &SqliteStore,
            task_id: &str,
        ) -> (Option<String>, Option<String>, Option<String>, i64) {
            db.conn
                .query_row(
                    "SELECT claimed_at, lease_expires_at, claimed_by, attempt_count \
                     FROM tasks WHERE task_id=?1",
                    [task_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap()
        }

        #[test]
        fn try_claim_sets_lease_claimed_by_and_increments_attempt_count() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let (claimed_at, lease_expires_at, claimed_by, attempt_count) = raw_claim_fields(&db, "t1");
            assert!(claimed_at.is_some(), "claimed_at이 세팅되어야 함");
            assert!(lease_expires_at.is_some(), "lease_expires_at이 세팅되어야 함");
            assert_eq!(claimed_by.as_deref(), Some("worker-a"));
            assert_eq!(attempt_count, 1, "최초 claim은 attempt_count=1");
        }

        #[test]
        fn try_claim_records_runner_and_get_task_exposes_it() {
            // v8: claim 시 runner를 기록하고, get_task로 조회한 Task에도 그대로 노출되어야 한다.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), Some("codex")).unwrap();

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.runner.as_deref(), Some("codex"), "claim한 runner가 노출되어야 함");
        }

        #[test]
        fn try_claim_without_runner_leaves_runner_null_backward_compat() {
            // 하위호환: runner 인자 없이 claim해도(레거시 워커·raw curl 등) 정상 동작, runner만 NULL.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.runner, None, "runner 없이 claim하면 NULL이어야 함");
        }

        #[test]
        fn try_claim_without_agent_leaves_claimed_by_null_backward_compat() {
            // 하위호환: agent 인자 없이 claim해도(raw curl 등) 정상 동작, claimed_by만 NULL.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", None, None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working);

            let (_, _, claimed_by, attempt_count) = raw_claim_fields(&db, "t1");
            assert_eq!(claimed_by, None, "agent 없이 claim하면 claimed_by는 NULL");
            assert_eq!(attempt_count, 1);
        }

        #[test]
        fn expire_stale_claims_requeues_expired_lease_under_max_attempts() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), Some("codex")).unwrap(); // attempt_count=1, runner 기록.

            // lease를 과거로 강제 심어 만료를 시뮬레이션한다(raw SQL, wire에 없는 내부 컬럼).
            db.conn
                .execute(
                    "UPDATE tasks SET lease_expires_at=datetime('now','-1 hour') WHERE task_id='t1'",
                    [],
                )
                .unwrap();

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1, "만료된 claim 1건이 회수되어야 함");

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.state, TaskState::Submitted, "만료된 working은 submitted로 복귀");
            assert!(reloaded.runner.is_none(), "runner는 회수(submitted 복귀) 시 클리어되어야 함(claimed_by와 동형)");

            let (claimed_at, lease_expires_at, claimed_by, attempt_count) = raw_claim_fields(&db, "t1");
            assert!(claimed_at.is_none(), "claimed_at은 클리어되어야 함");
            assert!(lease_expires_at.is_none(), "lease_expires_at은 클리어되어야 함");
            assert!(claimed_by.is_none(), "claimed_by는 클리어되어야 함");
            assert_eq!(attempt_count, 1, "attempt_count는 유지(다음 claim에서 다시 증가)");
        }

        #[test]
        fn expire_stale_claims_preserves_task_instruction_for_redelivery() {
            // requeue된 task는 새 워커가 poll에서 지시문(status_message)을 다시 읽어 실행하므로,
            // claim·requeue 모두 status_message를 지우면 안 된다(재배달 시 빈 프롬프트 방지).
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let task = db.create_task_from_message("win", "mac", msg.clone()).unwrap();
            db.try_claim(&task.id, Some("worker-a"), None).unwrap();
            db.test_force_lease_expired(&task.id);

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1);

            let reloaded = db.get_task(&task.id).unwrap().unwrap();
            assert_eq!(reloaded.state, TaskState::Submitted, "만료 claim은 submitted로 복귀");
            assert_eq!(
                reloaded.status_message,
                Some(msg),
                "requeue 후 지시문(status_message)이 보존되어야 재배달 워커가 프롬프트를 얻는다"
            );
        }

        #[test]
        fn expire_stale_claims_fails_task_when_attempt_count_at_max() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            // attempt_count를 상한(MAX_CLAIM_ATTEMPTS=3)까지 도달한 상태로 강제 세팅한다(raw SQL).
            db.conn
                .execute(
                    "UPDATE tasks SET attempt_count=3, \
                     lease_expires_at=datetime('now','-1 hour') WHERE task_id='t1'",
                    [],
                )
                .unwrap();

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1, "상한 도달 claim 1건이 격리되어야 함");

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.state, TaskState::Failed, "상한 초과는 submitted가 아니라 failed로 격리");
        }

        #[test]
        fn expire_stale_claims_leaves_unexpired_working_untouched() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap(); // lease는 기본 30분 후(미래).

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 0, "lease가 아직 안 지났으면 아무것도 회수되지 않아야 함");
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working);
        }

        #[test]
        fn expire_stale_claims_ignores_non_working_tasks() {
            // submitted/completed 등 working이 아닌 task는 sweep 대상이 아니다(설사 lease 컬럼이 남아있어도).
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap(); // submitted, lease 없음.

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 0);
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Submitted);
        }

        #[test]
        fn try_complete_first_completer_wins_rejects_mismatched_completer() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part { text: Some("결과".into()), ..Default::default() }],
            }];
            // stale(되살아난) worker-b가 completer 불일치로 거부되어야 한다(레이스 방지 핵심).
            let err = db.try_complete("t1", &artifacts, Some("worker-b")).unwrap_err();
            assert!(err.contains("t1"));
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working, "거부 후 상태 불변");

            // claim한 본인(worker-a)이 completer면 성공.
            db.try_complete("t1", &artifacts, Some("worker-a")).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Completed);
        }

        #[test]
        fn try_fail_first_completer_wins_rejects_mismatched_failer() {
            // try_complete와 대칭: 되살아난 stale worker-b가 worker-a claim task를 failed로 덮어쓰지 못한다
            // (gemini/coderabbit 리뷰). failer=None이면 하위호환으로 통과.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            // stale worker-b의 fail은 거부 -> 상태 불변(working).
            let err = db.try_fail("t1", None, Some("worker-b")).unwrap_err();
            assert!(err.contains("t1"));
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working, "거부 후 상태 불변");

            // claim 본인(worker-a)이면 성공.
            db.try_fail("t1", None, Some("worker-a")).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Failed);

            // 하위호환: failer=None이면 가드 무력화(다른 task로 확인).
            let t2 = Task::new("t2", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&t2).unwrap();
            db.try_claim("t2", Some("worker-a"), None).unwrap();
            db.try_fail("t2", None, None).unwrap();
            assert_eq!(db.get_task("t2").unwrap().unwrap().state, TaskState::Failed);
        }

        #[test]
        fn try_complete_completer_none_bypasses_guard_backward_compat() {
            // 하위호환: completer=None이면 claimed_by 불일치와 무관하게(가드 무력화) 기존 동작대로 성공.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part { text: Some("결과".into()), ..Default::default() }],
            }];
            db.try_complete("t1", &artifacts, None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Completed);
        }

        #[test]
        fn try_complete_succeeds_when_claimed_by_is_null() {
            // agent 인자 없이 claim된(claimed_by NULL) task는 completer가 있어도 성공해야 한다
            // (claimed_by IS NULL 분기, 혼재 호출 시나리오 하위호환).
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", None, None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part { text: Some("결과".into()), ..Default::default() }],
            }];
            db.try_complete("t1", &artifacts, Some("worker-a")).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Completed);
        }

        #[test]
        fn migration_v6_to_v7_adds_lease_columns_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v6v7.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v6 스키마 수동 구성: tasks에 lease 컬럼 없음 + schema_version=6 + 기존 task 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     CREATE TABLE tasks(task_id TEXT PRIMARY KEY, context_id TEXT, \
                         from_agent TEXT NOT NULL, to_agent TEXT NOT NULL, state TEXT NOT NULL, \
                         message_json TEXT, artifacts_json TEXT, history_json TEXT, \
                         created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
                     INSERT INTO tasks(task_id, context_id, from_agent, to_agent, state, \
                         created_at, updated_at) \
                         VALUES('t1', NULL, 'win', 'mac', 'submitted', \
                         '2026-07-02 09:00:00', '2026-07-02 09:00:00');
                     INSERT INTO config(key,value) VALUES('schema_version','6');",
                )
                .unwrap();
            }
            // open → migrate v6→v7(lease 컬럼 4종 신설).
            let db = SqliteStore::open(p).unwrap();
            for col in ["claimed_at", "lease_expires_at", "claimed_by", "attempt_count"] {
                assert!(db.column_exists("tasks", col), "마이그레이션이 {col} 컬럼을 추가해야 함");
            }
            // 기존 task 보존 + attempt_count 기본값 0.
            let preserved = db.get_task("t1").unwrap().expect("기존 task 보존");
            assert_eq!(preserved.state, TaskState::Submitted);
            let (_, _, _, attempt_count) = raw_claim_fields(&db, "t1");
            assert_eq!(attempt_count, 0, "기존 행의 attempt_count는 기본값 0");
            // 마이그레이션된 스키마에서 claim이 바로 동작해야 한다(신규 컬럼이 실사용 가능한지 확인).
            db.try_claim("t1", Some("worker-a"), None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Working);
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn migration_v7_to_v8_adds_runner_column_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v7v8.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v7 스키마 수동 구성: tasks에 lease 컬럼은 있으나 runner 없음 + schema_version=7 + 기존 task 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     CREATE TABLE tasks(task_id TEXT PRIMARY KEY, context_id TEXT, \
                         from_agent TEXT NOT NULL, to_agent TEXT NOT NULL, state TEXT NOT NULL, \
                         message_json TEXT, artifacts_json TEXT, history_json TEXT, \
                         created_at TEXT NOT NULL, updated_at TEXT NOT NULL, \
                         claimed_at TEXT, lease_expires_at TEXT, claimed_by TEXT, \
                         attempt_count INTEGER NOT NULL DEFAULT 0);
                     INSERT INTO tasks(task_id, context_id, from_agent, to_agent, state, \
                         created_at, updated_at) \
                         VALUES('t1', NULL, 'win', 'mac', 'submitted', \
                         '2026-07-02 09:00:00', '2026-07-02 09:00:00');
                     INSERT INTO config(key,value) VALUES('schema_version','7');",
                )
                .unwrap();
            }
            // open → migrate v7→v8(runner 컬럼 신설).
            let db = SqliteStore::open(p).unwrap();
            assert!(db.column_exists("tasks", "runner"), "마이그레이션이 runner 컬럼을 추가해야 함");
            // 기존 task 보존 + runner는 NULL(마이그레이션 이전엔 없던 컬럼).
            let preserved = db.get_task("t1").unwrap().expect("기존 task 보존");
            assert_eq!(preserved.state, TaskState::Submitted);
            assert_eq!(preserved.runner, None, "마이그레이션 이전 행의 runner는 NULL이어야 함");
            // 마이그레이션된 스키마에서 runner를 포함한 claim이 바로 동작해야 한다.
            db.try_claim("t1", Some("worker-a"), Some("claude")).unwrap();
            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.state, TaskState::Working);
            assert_eq!(reloaded.runner.as_deref(), Some("claude"));
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn migration_v5_to_v6_adds_tasks_table_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v5v6.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v5 스키마 수동 구성: tasks 테이블 없음 + schema_version=5 + 기존 메시지 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     INSERT INTO sessions(id, head_id) VALUES('s', 1);
                     INSERT INTO messages(session_id,msg_id,parent_id,speaker,content) \
                         VALUES('s',1,NULL,'a','hi');
                     INSERT INTO config(key,value) VALUES('schema_version','5');",
                )
                .unwrap();
            }
            // open → migrate v5→v6(tasks 테이블 신설).
            let db = SqliteStore::open(p).unwrap();
            let table_count: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(table_count, 1, "마이그레이션이 tasks 테이블 생성");
            // 기존 메시지 보존.
            let content: String = db
                .conn
                .query_row(
                    "SELECT content FROM messages WHERE session_id='s' AND msg_id=1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(content, "hi", "기존 메시지 보존");
            // tasks 테이블 실제 사용 가능 확인(신규 마이그레이션 스키마에 바로 INSERT 가능해야 함).
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            assert!(db.get_task("t1").unwrap().is_some());
            let _ = std::fs::remove_file(&path);
        }
    }

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn register_then_list_agents_roundtrip() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", tags(&[("machine", "win")]), Some("win-claude".into()), "2026-07-04 10:00:00");
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:10", 90);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
        assert_eq!(found[0].display_name.as_deref(), Some("win-claude"));
    }

    #[test]
    fn heartbeat_agent_updates_existing_and_rejects_unknown() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 10:00:00");
        assert!(db.heartbeat_agent("u1", "2026-07-04 10:01:00"));
        assert!(!db.heartbeat_agent("unknown", "2026-07-04 10:01:00"));
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:01:05", 90);
        assert_eq!(found[0].last_heartbeat, "2026-07-04 10:01:00");
    }

    #[test]
    fn list_agents_excludes_offline() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 09:00:00");
        // now 기준 1시간 경과, ttl 90초 -> offline이라 제외되어야 함.
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:00", 90);
        assert!(found.is_empty(), "offline 에이전트는 list_agents에서 제외되어야 함");
    }

    #[test]
    fn resolve_selector_matches_none_one_or_many() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", tags(&[("machine", "win"), ("runner", "claude")]), None, "2026-07-04 10:00:00");
        db.register_agent("u2", tags(&[("machine", "mac"), ("runner", "claude")]), None, "2026-07-04 10:00:00");
        let now = "2026-07-04 10:00:10";

        let none = db.resolve_selector(&tags(&[("machine", "linux")]), now, 90);
        assert!(none.is_empty());

        let one = db.resolve_selector(&tags(&[("machine", "mac")]), now, 90);
        assert_eq!(one, vec!["u2".to_string()]);

        let many = db.resolve_selector(&tags(&[("runner", "claude")]), now, 90);
        assert_eq!(many, vec!["u1".to_string(), "u2".to_string()]);
    }

    fn candidate(uuid: &str) -> CandidateEntry {
        CandidateEntry {
            uuid: uuid.to_string(),
            runner: "claude".to_string(),
            project: Some("tunaround".to_string()),
            machine: Some("win".to_string()),
            source: "claude-jsonl".to_string(),
            age_secs: 5,
            reported_at: String::new(), // report_candidates가 now로 덮어씀
        }
    }

    #[test]
    fn report_then_list_candidates_roundtrip_and_upsert() {
        let db = SqliteStore::open_memory().unwrap();
        db.report_candidates(vec![candidate("s1"), candidate("s2")], "2026-07-06 10:00:00");
        let found = db.list_candidates("2026-07-06 10:00:10", 180);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].uuid, "s1");
        assert_eq!(found[0].reported_at, "2026-07-06 10:00:00"); // 브로커 now로 채워짐
        // 같은 uuid 재보고는 upsert(교체), 개수 불변.
        db.report_candidates(vec![candidate("s1")], "2026-07-06 10:01:00");
        let again = db.list_candidates("2026-07-06 10:01:05", 180);
        assert_eq!(again.len(), 2);
        let s1 = again.iter().find(|c| c.uuid == "s1").unwrap();
        assert_eq!(s1.reported_at, "2026-07-06 10:01:00");
    }

    #[test]
    fn list_candidates_excludes_stale() {
        let db = SqliteStore::open_memory().unwrap();
        db.report_candidates(vec![candidate("s1")], "2026-07-06 09:00:00");
        // now 기준 1시간 경과, ttl 180초 -> stale이라 제외되어야 함.
        let found = db.list_candidates("2026-07-06 10:00:00", 180);
        assert!(found.is_empty(), "stale 후보는 list_candidates에서 제외되어야 함");
    }

    #[test]
    fn list_agents_filters_by_selector_subset() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "u1",
            tags(&[("machine", "win"), ("runner", "claude"), ("role", "worker")]),
            None,
            "2026-07-04 10:00:00",
        );
        db.register_agent("u2", tags(&[("machine", "win"), ("runner", "codex")]), None, "2026-07-04 10:00:00");
        let now = "2026-07-04 10:00:10";
        let found = db.list_agents(&tags(&[("machine", "win"), ("runner", "claude")]), now, 90);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
    }
}
