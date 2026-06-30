// SQLite FTS로 관련 과거 맥락 슬라이스를 검색하는 ContextRetriever 구현.

#[cfg(feature = "sqlite")]
pub use sqlite_retriever::SqliteRetriever;

#[cfg(feature = "sqlite")]
mod sqlite_retriever {
    use crate::orchestrator::Utterance;
    use crate::store::sqlite::SqliteStore;

    /// SqliteStore 읽기 연결 + 선-토크나이즈 closure를 묶은 맥락 검색기.
    /// rusqlite::Connection은 Send이지만 Sync가 아니므로 Mutex로 감싼다.
    pub struct SqliteRetriever {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
    }

    impl SqliteRetriever {
        pub fn new(store: SqliteStore, tok: Box<dyn Fn(&str) -> String + Send + Sync>) -> Self {
            Self { store: std::sync::Mutex::new(store), tok }
        }
    }

    impl crate::orchestrator::ContextRetriever for SqliteRetriever {
        fn retrieve(&self, query: &str, limit: usize) -> Vec<Utterance> {
            let q = (self.tok)(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            match store.search(&q, limit) {
                Ok(hits) => hits
                    .into_iter()
                    .map(|h| Utterance { speaker: h.speaker, content: h.content })
                    .collect(),
                Err(e) => {
                    eprintln!("[tunaRound] 검색 실패: {e}");
                    Vec::new()
                }
            }
        }
    }
}

#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::SqliteRetriever;
    use crate::orchestrator::ContextRetriever;
    use crate::store::sqlite::SqliteStore;
    use crate::store::{StoredMessage, StoredSession};

    #[test]
    fn retriever_finds_cross_session_content() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_cross.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();

        // 과거 세션 "session-a" 색인.
        let store_w = SqliteStore::open(p).unwrap();
        let ss_a = StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "claude/proposer".into(),
                content: "검색 시스템 설계".into(),
            }],
            head: Some(1),
        };
        store_w.save_session("session-a", &ss_a, |t| t.to_string()).unwrap();
        drop(store_w);

        // 별도 읽기 연결로 SqliteRetriever 생성 후 cross-session 검색.
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()));

        // "session-a"의 발언을 다른 연결에서 retrieve할 수 있어야 한다.
        let hits = retriever.retrieve("검색", 10);
        assert!(!hits.is_empty(), "cross-session 검색이 결과를 반환해야 함");
        assert!(
            hits.iter().any(|u| u.content.contains("검색") || u.speaker.contains("claude")),
            "검색 결과 내용 불일치: {:?}",
            hits.iter().map(|u| u.content.as_str()).collect::<Vec<_>>()
        );

        let _ = std::fs::remove_file(&path);
    }
}
