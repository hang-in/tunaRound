// SQLite FTS + 벡터 RRF 하이브리드로 관련 과거 맥락 슬라이스를 검색하는 ContextRetriever 구현.

#[cfg(feature = "sqlite")]
pub use sqlite_retriever::SqliteRetriever;

#[cfg(feature = "sqlite")]
pub use sqlite_transcript::{SqliteTranscriptReader, SqliteTranscriptWriter};

#[cfg(feature = "sqlite")]
mod sqlite_retriever {
    use std::collections::HashMap;

    use crate::orchestrator::Utterance;
    use crate::store::sqlite::SqliteStore;

    /// SqliteStore 읽기 연결 + 선-토크나이즈 closure + 선택적 Embedder를 묶은 맥락 검색기.
    /// rusqlite::Connection은 Send이지만 Sync가 아니므로 Mutex로 감싼다.
    pub struct SqliteRetriever {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
        embedder: Option<Box<dyn crate::store::embedding::Embedder>>,
    }

    impl SqliteRetriever {
        /// embedder=None이면 FTS 단독(기존 동작 불변). Some이면 FTS+벡터 RRF 하이브리드.
        pub fn new(
            store: SqliteStore,
            tok: Box<dyn Fn(&str) -> String + Send + Sync>,
            embedder: Option<Box<dyn crate::store::embedding::Embedder>>,
        ) -> Self {
            Self { store: std::sync::Mutex::new(store), tok, embedder }
        }
    }

    impl crate::orchestrator::ContextRetriever for SqliteRetriever {
        fn retrieve(&self, query: &str, limit: usize) -> Vec<Utterance> {
            if query.trim().is_empty() {
                return Vec::new();
            }

            let q = (self.tok)(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());

            // FTS 검색.
            let lex_hits = match store.search(&q, limit) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("[tunaRound] FTS 검색 실패: {e}");
                    Vec::new()
                }
            };

            // embedder 없으면 FTS 단독(기존 경로, 동작 불변).
            let Some(emb) = &self.embedder else {
                return lex_hits
                    .into_iter()
                    .map(|h| Utterance { speaker: h.speaker, content: h.content })
                    .collect();
            };

            // FTS 결과 키 리스트 + content_map 구축.
            let lex_keys: Vec<(String, u64)> =
                lex_hits.iter().map(|h| (h.session_id.clone(), h.msg_id)).collect();
            let mut content_map: HashMap<(String, u64), (String, String)> = lex_hits
                .into_iter()
                .map(|h| ((h.session_id, h.msg_id), (h.speaker, h.content)))
                .collect();

            // 쿼리 임베딩 시도(실패 시 FTS 단독 폴백).
            let qvec = match emb.embed(query) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[tunaRound] 쿼리 임베딩 실패(FTS 단독 폴백): {e}");
                    return content_map
                        .into_values()
                        .map(|(sp, ct)| Utterance { speaker: sp, content: ct })
                        .collect();
                }
            };

            // 벡터 검색.
            let vec_hits = match store.vector_search(&qvec, limit) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("[tunaRound] 벡터 검색 실패(FTS 단독 폴백): {e}");
                    return content_map
                        .into_values()
                        .map(|(sp, ct)| Utterance { speaker: sp, content: ct })
                        .collect();
                }
            };

            let vec_keys: Vec<(String, u64)> =
                vec_hits.iter().map(|(sid, mid, _)| (sid.clone(), *mid)).collect();

            // RRF 융합.
            let fused = crate::store::reciprocal_rank_fusion(&lex_keys, &vec_keys);

            // 상위 limit 키를 Utterance로 변환.
            let mut result = Vec::with_capacity(limit.min(fused.len()));
            for key in fused.into_iter().take(limit) {
                let utt = if let Some((sp, ct)) = content_map.remove(&key) {
                    // FTS 결과에 있으면 캐시 사용.
                    Some(Utterance { speaker: sp, content: ct })
                } else {
                    // 벡터-only 키: DB에서 원문 조회.
                    match store.get_message(&key.0, key.1) {
                        Ok(Some((sp, ct))) => Some(Utterance { speaker: sp, content: ct }),
                        Ok(None) => None,
                        Err(e) => {
                            eprintln!("[tunaRound] get_message 실패(스킵): {e}");
                            None
                        }
                    }
                };
                if let Some(u) = utt {
                    result.push(u);
                }
            }
            result
        }
    }
}

#[cfg(feature = "sqlite")]
mod sqlite_transcript {
    use crate::orchestrator::Utterance;
    use crate::store::sqlite::SqliteStore;

    /// 세션 전사 전체(또는 마지막 N턴)를 활성 경로(root->head)로 읽어 오는 구현.
    /// rusqlite Connection은 Send이지만 Sync가 아니므로 Mutex로 감싼다.
    pub struct SqliteTranscriptReader {
        store: std::sync::Mutex<SqliteStore>,
    }

    impl SqliteTranscriptReader {
        /// SqliteStore를 받아 새 전사 리더를 반환한다.
        pub fn new(store: SqliteStore) -> Self {
            Self { store: std::sync::Mutex::new(store) }
        }
    }

    impl crate::orchestrator::TranscriptReader for SqliteTranscriptReader {
        fn read_transcript(&self, session_id: &str, max_turns: Option<usize>) -> Vec<Utterance> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let Ok(Some(ss)) = store.load_session(session_id) else {
                return Vec::new();
            };
            let path = crate::store::path_to_root(&ss.messages, ss.head);
            match max_turns {
                Some(n) if path.len() > n => path[path.len() - n..].to_vec(),
                _ => path,
            }
        }
    }

    /// 세션 전사 끝에 발언을 증분 추가하는 쓰기 구현(post_turn·front=core 병합용, Plan 27).
    /// FTS 색인용 토크나이저 closure를 보유한다. Connection은 Send이나 Sync 아니라 Mutex로 감싼다.
    pub struct SqliteTranscriptWriter {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
    }

    impl SqliteTranscriptWriter {
        /// SqliteStore + 색인용 토크나이저 closure를 받아 새 writer를 반환한다.
        pub fn new(store: SqliteStore, tok: Box<dyn Fn(&str) -> String + Send + Sync>) -> Self {
            Self { store: std::sync::Mutex::new(store), tok }
        }
    }

    impl crate::orchestrator::TranscriptWriter for SqliteTranscriptWriter {
        fn append_turn(&self, session_id: &str, speaker: &str, content: &str) -> Result<u64, String> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.append_turn(session_id, speaker, content, |t| (self.tok)(t))
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

        // 별도 읽기 연결로 SqliteRetriever 생성 후 cross-session 검색(embedder=None -> FTS 단독).
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);

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

    #[test]
    fn retriever_hybrid_rrf_returns_results() {
        // MockEmbedder로 FTS+벡터 RRF 하이브리드 경로 검증.
        use crate::store::embedding::MockEmbedder;

        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_hybrid.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();

        let store_w = SqliteStore::open(p).unwrap();
        let ss = StoredSession {
            messages: vec![
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "claude".into(),
                    content: "검색 시스템 설계 논의".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "codex".into(),
                    content: "인덱스 전략 리뷰 결과".into(),
                },
            ],
            head: Some(2),
        };
        store_w.save_session("hybrid-s", &ss, |t| t.to_string()).unwrap();
        // 벡터 색인.
        let mock = MockEmbedder::new(64);
        store_w.index_vectors("hybrid-s", &ss, &mock).unwrap();
        drop(store_w);

        // 읽기 연결 + MockEmbedder로 하이브리드 retriever.
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(
            store_r,
            Box::new(|t: &str| t.to_string()),
            Some(Box::new(MockEmbedder::new(64))),
        );

        // RRF 경로 실행: 결과가 반환되어야 한다.
        let hits = retriever.retrieve("검색", 10);
        assert!(!hits.is_empty(), "하이브리드 검색이 결과를 반환해야 함: {:?}", hits);

        let _ = std::fs::remove_file(&path);
    }
}
