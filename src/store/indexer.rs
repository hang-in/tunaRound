// 메시지 트리를 검색 인덱스(SQLite/FTS)에 미러링하는 인덱서 추상화.
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
    /// rusqlite::Connection은 Send이지만 Sync가 아니므로 Mutex로 감싸 Sync를 충족한다.
    pub struct SqliteIndexer {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
    }
    impl SqliteIndexer {
        pub fn new(store: SqliteStore, tok: Box<dyn Fn(&str) -> String + Send + Sync>) -> Self {
            Self { store: std::sync::Mutex::new(store), tok }
        }
    }
    impl MessageIndexer for SqliteIndexer {
        fn persist(&self, session_id: &str, ss: &StoredSession) {
            // best-effort: 색인 실패는 토론 흐름을 막지 않는다(eprintln 경고).
            let store = match self.store.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            if let Err(e) = store.save_session(session_id, ss, |t| (self.tok)(t)) {
                eprintln!("[tunaRound] SQLite 색인 실패: {e}");
            }
        }
    }
}
