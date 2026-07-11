// 메시지 트리를 검색 인덱스(SQLite/FTS)에 미러링하는 인덱서 추상화.
use crate::store::StoredSession;

/// 메시지 트리를 검색 인덱스에 반영하는 추상화(append 후 fire-and-forget 미러 패턴).
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

    /// SqliteStore + 선-토크나이즈 closure + 선택적 Embedder를 묶은 인덱서.
    /// rusqlite::Connection은 Send이지만 Sync가 아니므로 Mutex로 감싸 Sync를 충족한다.
    pub struct SqliteIndexer {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
        embedder: Option<Box<dyn crate::store::embedding::Embedder>>,
    }
    impl SqliteIndexer {
        /// embedder=None이면 FTS 단독(기존 동작 불변). Some이면 FTS 색인 후 벡터도 색인(best-effort).
        pub fn new(
            store: SqliteStore,
            tok: Box<dyn Fn(&str) -> String + Send + Sync>,
            embedder: Option<Box<dyn crate::store::embedding::Embedder>>,
        ) -> Self {
            Self { store: std::sync::Mutex::new(store), tok, embedder }
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
            // 벡터 색인: embedder 있으면 best-effort(실패해도 토론 흐름 불중단).
            if let Some(emb) = &self.embedder
                && let Err(e) = store.index_vectors(session_id, ss, emb.as_ref())
            {
                eprintln!("[tunaRound] 벡터 색인 실패(best-effort): {e}");
            }
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    use crate::store::sqlite::SqliteStore;
    use crate::store::{StoredMessage, StoredSession};

    #[test]
    fn indexer_persists_and_is_searchable() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_idx_rt.db");
        let _ = std::fs::remove_file(&path); // 깨끗한 시작.
        let p = path.to_str().unwrap();
        let store = SqliteStore::open(p).unwrap();
        let idx = SqliteIndexer::new(store, Box::new(|t: &str| t.to_string()), None);
        let ss = StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "claude".into(),
                content: "검색 시스템".into(),
            }],
            head: Some(1),
        };
        idx.persist("s1", &ss);
        // 같은 파일 DB를 다시 열어 색인 확인.
        let reopened = SqliteStore::open(p).unwrap();
        let hits = reopened.search("검색", 10).unwrap();
        assert!(hits.iter().any(|h| h.session_id == "s1" && h.msg_id == 1));
        let _ = std::fs::remove_file(&path);
    }
}
