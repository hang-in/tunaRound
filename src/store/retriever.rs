// SQLite FTS + 벡터 RRF 하이브리드로 관련 과거 맥락 슬라이스를 검색하는 ContextRetriever 구현.

#[cfg(feature = "sqlite")]
pub use sqlite_retriever::SqliteRetriever;

#[cfg(feature = "sqlite")]
pub use sqlite_transcript::{
    SqliteCoreSync, SqliteTranscriptReader, SqliteTranscriptWriter, SqliteValiditySink,
};

#[cfg(feature = "sqlite")]
mod sqlite_retriever {
    use std::collections::HashMap;

    use crate::orchestrator::Utterance;
    use crate::store::sqlite::SqliteStore;

    /// 세션 다양성 cap: 한 세션이 결과를 독점하지 않도록 우선 뽑는 세션당 최대 개수.
    const MAX_PER_SESSION: usize = 2;
    /// 다양성 cap을 적용하려면 limit보다 넉넉히 후보를 모아야 한다(over-fetch 배수).
    const OVERFETCH: usize = 4;

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

    /// penalty 기반 재랭크(안정 정렬로 같은 penalty 내 relevance 순서 보존).
    /// rejected 드롭 / superseded·stale +2 / 현재 세션 off-branch(버려진 분기) +1(step 5b).
    /// 유효성 미설정·active·unknown은 penalty 0. current_session=None이면 분기 페널티 없음.
    fn rerank<T>(
        store: &SqliteStore,
        items: Vec<(String, u64, T)>,
        current_session: Option<&str>,
    ) -> Vec<(String, u64, T)> {
        let mut scored: Vec<(u32, String, u64, T)> = Vec::new();
        for (sid, mid, v) in items {
            let state = store.get_validity(&sid, mid).ok().flatten().map(|x| x.valid_state);
            let mut penalty = 0u32;
            match state.as_deref() {
                Some("rejected") => continue, // 드롭.
                Some("superseded") | Some("stale") => penalty += 2,
                _ => {} // active | unknown | None.
            }
            if current_session == Some(sid.as_str()) {
                // 현재 세션의 off-branch 히트(활성경로 콘텐츠는 repl이 이미 제외) = 버려진 분기.
                penalty += 1;
            }
            scored.push((penalty, sid, mid, v));
        }
        scored.sort_by_key(|(p, _, _, _)| *p); // 안정 정렬.
        scored.into_iter().map(|(_, sid, mid, v)| (sid, mid, v)).collect()
    }

    /// (session_id, Utterance) 항목을 재랭크(유효성+분기) 후 세션 다양성 cap + limit으로 마무리한다.
    fn finish(
        store: &SqliteStore,
        cands: Vec<(String, u64, Utterance)>,
        limit: usize,
        current_session: Option<&str>,
    ) -> Vec<Utterance> {
        let reranked = rerank(store, cands, current_session);
        let items: Vec<(String, Utterance)> = reranked.into_iter().map(|(sid, _, u)| (sid, u)).collect();
        crate::store::cap_per_session_backfill(items, MAX_PER_SESSION, limit)
    }

    impl SqliteRetriever {
        /// retrieve/retrieve_ctx 공용 구현. current_session=Some이면 분기 인지 디프리오리티.
        fn retrieve_impl(&self, query: &str, limit: usize, current_session: Option<&str>) -> Vec<Utterance> {
            if query.trim().is_empty() {
                return Vec::new();
            }

            let q = (self.tok)(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());

            // FTS 검색(세션 다양성 cap을 위해 over-fetch).
            let lex_hits = match store.search(&q, limit * OVERFETCH) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("[tunaRound] FTS 검색 실패: {e}");
                    Vec::new()
                }
            };

            // embedder 없으면 FTS 단독. 유효성 재랭크 + 세션 다양성 cap(단일 세션은 동작 불변).
            let Some(emb) = &self.embedder else {
                let cands: Vec<(String, u64, Utterance)> = lex_hits
                    .into_iter()
                    .map(|h| (h.session_id, h.msg_id, Utterance { speaker: h.speaker, content: h.content }))
                    .collect();
                return finish(&store, cands, limit, current_session);
            };

            // FTS 결과 키 리스트 + content_map 구축.
            let lex_keys: Vec<(String, u64)> =
                lex_hits.iter().map(|h| (h.session_id.clone(), h.msg_id)).collect();
            let mut content_map: HashMap<(String, u64), (String, String)> = lex_hits
                .into_iter()
                .map(|h| ((h.session_id, h.msg_id), (h.speaker, h.content)))
                .collect();

            // content_map에서 (sid, msg_id, Utterance) 후보를 만드는 폴백용 클로저.
            let cands_from_map = |m: HashMap<(String, u64), (String, String)>| -> Vec<(String, u64, Utterance)> {
                m.into_iter()
                    .map(|((sid, mid), (sp, ct))| (sid, mid, Utterance { speaker: sp, content: ct }))
                    .collect()
            };

            // 쿼리 임베딩 시도(실패 시 FTS 단독 폴백).
            let qvec = match emb.embed(query) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[tunaRound] 쿼리 임베딩 실패(FTS 단독 폴백): {e}");
                    return finish(&store, cands_from_map(content_map), limit, current_session);
                }
            };

            // 벡터 검색(세션 다양성 cap을 위해 over-fetch).
            let vec_hits = match store.vector_search(&qvec, limit * OVERFETCH) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("[tunaRound] 벡터 검색 실패(FTS 단독 폴백): {e}");
                    return finish(&store, cands_from_map(content_map), limit, current_session);
                }
            };

            let vec_keys: Vec<(String, u64)> =
                vec_hits.iter().map(|(sid, mid, _)| (sid.clone(), *mid)).collect();

            // RRF 융합 → (sid, msg_id, Utterance) 후보로 해석(벡터-only 키는 DB 조회).
            let fused = crate::store::reciprocal_rank_fusion(&lex_keys, &vec_keys);
            let mut cands: Vec<(String, u64, Utterance)> = Vec::with_capacity(fused.len());
            for key in fused {
                let utt = if let Some((sp, ct)) = content_map.remove(&key) {
                    Some(Utterance { speaker: sp, content: ct })
                } else {
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
                    cands.push((key.0, key.1, u));
                }
            }
            finish(&store, cands, limit, current_session)
        }
    }

    impl crate::orchestrator::ContextRetriever for SqliteRetriever {
        fn retrieve(&self, query: &str, limit: usize) -> Vec<Utterance> {
            self.retrieve_impl(query, limit, None)
        }
        fn retrieve_ctx(&self, query: &str, limit: usize, current_session: &str) -> Vec<Utterance> {
            self.retrieve_impl(query, limit, Some(current_session))
        }

        /// 리치 디버그: 토큰화 결과 + FTS bm25 점수 + 유효성 + 분기 표시(step 7).
        fn debug_retrieve(&self, query: &str, limit: usize, current_session: &str) -> String {
            if query.trim().is_empty() {
                return "질의가 비어 있습니다.".to_string();
            }
            let q = (self.tok)(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let hits = store.search(&q, limit * OVERFETCH).unwrap_or_default();
            let hybrid = if self.embedder.is_some() { " (+벡터 하이브리드)" } else { "" };
            let mut out = format!(
                "질의: {query}\n토큰화(FTS{hybrid}): {q}\n후보({}건, 상위 {} 표시):\n",
                hits.len(),
                limit.min(hits.len())
            );
            for h in hits.iter().take(limit) {
                let state = store
                    .get_validity(&h.session_id, h.msg_id)
                    .ok()
                    .flatten()
                    .map(|v| v.valid_state)
                    .unwrap_or_else(|| "active".to_string());
                let branch = if current_session == h.session_id { " cur-session" } else { "" };
                let snippet: String = h.content.chars().take(50).collect();
                out.push_str(&format!(
                    "  [#{} sid={} bm25={:.3} valid={}{}] {}: {}\n",
                    h.msg_id, h.session_id, h.score, state, branch, h.speaker, snippet
                ));
            }
            out.push_str("(bm25: 낮을수록 관련 높음. valid=rejected는 제외·superseded/stale·cur-session off-branch는 강등.)");
            out
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

    /// front=core 병합용 CoreSync 구현: REPL이 코어 DB를 권위로 로드/추가한다(Plan 27 옵션 B).
    /// load_session으로 외부 post_turn까지 포함한 트리를 읽고, append_turn으로 DB id 권위 추가.
    pub struct SqliteCoreSync {
        store: std::sync::Mutex<SqliteStore>,
        tok: Box<dyn Fn(&str) -> String + Send + Sync>,
    }

    impl SqliteCoreSync {
        /// SqliteStore + 색인용 토크나이저 closure를 받아 새 core-sync를 반환한다.
        pub fn new(store: SqliteStore, tok: Box<dyn Fn(&str) -> String + Send + Sync>) -> Self {
            Self { store: std::sync::Mutex::new(store), tok }
        }
    }

    impl crate::orchestrator::CoreSync for SqliteCoreSync {
        fn load_session(&self, session_id: &str) -> Option<crate::store::StoredSession> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.load_session(session_id).ok().flatten()
        }
        fn append_turn(&self, session_id: &str, speaker: &str, content: &str) -> Result<u64, String> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.append_turn(session_id, speaker, content, |t| (self.tok)(t))
        }
    }

    /// 유효성 지정 sink 구현(/supersede·/reject → message_validity 쓰기).
    pub struct SqliteValiditySink {
        store: std::sync::Mutex<SqliteStore>,
    }

    impl SqliteValiditySink {
        pub fn new(store: SqliteStore) -> Self {
            Self { store: std::sync::Mutex::new(store) }
        }
    }

    impl crate::orchestrator::ValiditySink for SqliteValiditySink {
        fn set_validity(
            &self,
            session_id: &str,
            msg_id: u64,
            valid_state: &str,
            superseded_by: Option<u64>,
        ) -> Result<(), String> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.set_validity(session_id, msg_id, valid_state, superseded_by)
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
    fn retrieve_excludes_rejected_and_demotes_superseded() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_validity.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        // 세 발언 모두 "검색" 포함(같은 세션).
        let ss = StoredSession {
            messages: vec![
                StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "검색 활성".into() },
                StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "검색 대체됨".into() },
                StoredMessage { id: 3, parent_id: Some(2), speaker: "c".into(), content: "검색 기각됨".into() },
            ],
            head: Some(3),
        };
        store_w.save_session("s", &ss, |t| t.to_string()).unwrap();
        store_w.set_validity("s", 2, "superseded", None).unwrap();
        store_w.set_validity("s", 3, "rejected", None).unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("검색", 10);
        let contents: Vec<&str> = hits.iter().map(|u| u.content.as_str()).collect();
        assert!(!contents.iter().any(|c| c.contains("기각")), "rejected는 제외: {contents:?}");
        let pos_active = contents.iter().position(|c| c.contains("활성"));
        let pos_super = contents.iter().position(|c| c.contains("대체"));
        assert!(pos_active.is_some() && pos_super.is_some(), "active·superseded 모두 존재: {contents:?}");
        assert!(pos_active < pos_super, "active가 superseded보다 앞: {contents:?}");

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn debug_retrieve_shows_tokenization_score_and_validity() {
        use crate::orchestrator::ContextRetriever;
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_debug.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "s",
                &StoredSession {
                    messages: vec![StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "검색 랭킹".into() }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w.set_validity("s", 1, "superseded", None).unwrap();
        drop(store_w);
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let out = retriever.debug_retrieve("검색", 10, "s");
        assert!(out.contains("토큰화"), "토큰화 라인: {out}");
        assert!(out.contains("bm25="), "bm25 점수: {out}");
        assert!(out.contains("valid=superseded"), "유효성 표시: {out}");
        assert!(out.contains("cur-session"), "현재세션 표시: {out}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retrieve_ctx_demotes_current_session_offbranch() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_branch.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "cur",
                &StoredSession {
                    messages: vec![StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "검색 현재세션".into() }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w
            .save_session(
                "oth",
                &StoredSession {
                    messages: vec![StoredMessage { id: 1, parent_id: None, speaker: "b".into(), content: "검색 다른세션".into() }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);

        // 현재 세션="cur" → cur의 off-branch 히트가 타 세션(oth)보다 뒤로 강등.
        let hits = retriever.retrieve_ctx("검색", 10, "cur");
        let contents: Vec<&str> = hits.iter().map(|u| u.content.as_str()).collect();
        let pos_other = contents.iter().position(|c| c.contains("다른세션"));
        let pos_cur = contents.iter().position(|c| c.contains("현재세션"));
        assert!(pos_other.is_some() && pos_cur.is_some(), "둘 다 존재: {contents:?}");
        assert!(pos_other < pos_cur, "다른 세션이 현재세션 off-branch보다 앞: {contents:?}");

        // 컨텍스트 없는 retrieve는 분기 페널티 없음(둘 다 반환).
        assert_eq!(retriever.retrieve("검색", 10).len(), 2);

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
