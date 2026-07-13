// SQLite FTS + 벡터 RRF 하이브리드로 관련 과거 맥락 슬라이스를 검색하는 ContextRetriever 구현.

#[cfg(feature = "sqlite")]
pub use sqlite_retriever::SqliteRetriever;

#[cfg(feature = "sqlite")]
pub use sqlite_transcript::{
    SqliteAnnotationSink, SqliteCoreSync, SqliteTranscriptReader, SqliteTranscriptWriter,
    SqliteValiditySink,
};

#[cfg(feature = "sqlite")]
mod sqlite_retriever {
    use std::collections::HashMap;

    use crate::store::sqlite::SqliteStore;
    use crate::types::Utterance;

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
            Self {
                store: std::sync::Mutex::new(store),
                tok,
                embedder,
            }
        }
    }

    /// cross-session recency 임계(step 5c). 후보 집합 최신 대비 이보다 오래된 "다른 세션" 히트만 소폭 강등.
    const RECENCY_STALE_SECS: i64 = 7 * 86_400;

    /// "YYYY-MM-DD HH:MM:SS"를 단조 증가 정수(초 근사)로 파싱한다. 임계 비교 전용이라 절대 epoch
    /// 정확성은 불요하고 단조성만 필요(월=31일 근사 허용). 파싱 실패는 None.
    fn parse_ts_approx(s: &str) -> Option<i64> {
        let s = s.trim();
        let (date, time) = s.split_once(' ').unwrap_or((s, "00:00:00"));
        let mut dp = date.split('-');
        let y: i64 = dp.next()?.parse().ok()?;
        let mo: i64 = dp.next()?.parse().ok()?;
        let d: i64 = dp.next()?.parse().ok()?;
        let mut tp = time.split(':');
        let h: i64 = tp.next()?.parse().ok()?;
        let mi: i64 = tp.next()?.parse().ok()?;
        let se: i64 = tp.next().unwrap_or("0").parse().ok()?;
        Some((((y * 12 + mo) * 31 + d) * 24 + h) * 3600 + mi * 60 + se)
    }

    /// raw 질의를 앵커 매치용 토큰(영숫자 경계 분리·소문자·비어있지 않음)으로 만든다.
    /// 한글 음절은 is_alphanumeric()이 true라 "검색"은 한 토큰으로 유지된다. FTS 연산자 오염을
    /// 피하려 morphology tok 대신 raw query에서 직접 뽑는다.
    fn query_anchor_tokens(query: &str) -> Vec<String> {
        query
            .split(|c: char| !c.is_alphanumeric())
            .map(|t| t.to_lowercase())
            .filter(|t| !t.is_empty())
            .collect()
    }

    /// 앵커/질의 토큰 최소 길이(2자 미만은 무시). 짧고 흔한 토큰("이", "a")이 광범위 매치로
    /// 부스트를 오발화하는 것을 막는다.
    const MIN_ANCHOR_TOKEN_LEN: usize = 2;

    /// anchors 문자열 토큰과 질의 토큰이 **완전일치**(토큰 경계)하면 매치.
    /// 앵커 분리는 query_anchor_tokens와 동일하게 **비영숫자 경계**(`!c.is_alphanumeric()`)로 통일한다.
    /// 하이픈·언더스코어·슬래시가 든 앵커(예 `RAG-설계`, `mesh_scan`)도 질의 토큰과 매치되도록(gemini HIGH).
    /// 양쪽 모두 최소 길이 필터를 적용하고 부분일치(substring)는 쓰지 않는다(과매치 방지).
    /// anchors 비었거나 유효 질의 토큰 없으면 false. 대소문자 무시(양쪽 소문자화).
    fn anchor_matches(anchors: &str, query_tokens: &[String]) -> bool {
        let anchor_toks: Vec<String> = anchors
            .split(|c: char| !c.is_alphanumeric())
            .map(|t| t.to_lowercase())
            .filter(|t| t.chars().count() >= MIN_ANCHOR_TOKEN_LEN)
            .collect();
        if anchor_toks.is_empty() {
            return false;
        }
        query_tokens
            .iter()
            .filter(|q| q.chars().count() >= MIN_ANCHOR_TOKEN_LEN)
            .any(|q| anchor_toks.iter().any(|a| a == q))
    }

    /// rerank 1차 staged 항목: (penalty, anchor_rank, session_id, msg_id, item, ts_approx, abstraction).
    type RankStaged<T> = (u32, u8, String, u64, T, Option<i64>, Option<String>);
    /// rerank 2차 scored 항목: (penalty, anchor_rank, session_id, msg_id, item, abstraction).
    type RankScored<T> = (u32, u8, String, u64, T, Option<String>);

    /// penalty 기반 재랭크(안정 정렬로 같은 penalty 내 relevance 순서 보존).
    /// rejected 드롭 / superseded·stale +2 / 현재 세션 off-branch(버려진 분기) +1(step 5b) /
    /// 다른 세션의 낡은(후보 집합 최신 대비 임계 초과) 히트 +1(step 5c, recency 정책 A=보수).
    /// 유효성 미설정·active·unknown은 penalty 0. current_session=None이면 분기 페널티 없음.
    /// created_at NULL(마이그레이션 기존행)은 recency 판단 유보(강등 없음).
    /// 큐레이션 앵커 부스트(v2-51)는 penalty를 침범하지 않는 **2차 정렬 키**(매치=0/미매치=1)로만
    /// 작용해, 같은 penalty tier 안에서만 매치를 앞세운다(rejected 드롭·superseded 강등 불침해).
    /// 항목당 get_validity를 **1회만** 호출한다(핫패스 DB 왕복 절감). 반환 4번째 원소=큐레이션
    /// abstraction(공백 제거 후 Some). finish가 이걸 Utterance.abstraction에 실어 재조회를 없앤다.
    fn rerank<T>(
        store: &SqliteStore,
        items: Vec<(String, u64, T)>,
        current_session: Option<&str>,
        query_tokens: &[String],
    ) -> Vec<(String, u64, T, Option<String>)> {
        // 1차: rejected 드롭 + 유효성/분기 penalty + 앵커 매치 + abstraction + created_at + 후보 최신 타임스탬프.
        let mut staged: Vec<RankStaged<T>> = Vec::new();
        let mut max_ts: Option<i64> = None;
        for (sid, mid, v) in items {
            // 항목당 유일 조회(핫패스 DB 왕복 절감). Err는 무신호로 삼키지 않고 표면화한 뒤 이 후보를
            // 보수적으로 드롭한다(active로 폴백하면 DB 장애 순간 /reject로 기각된 발언이 재부상하므로,
            // coderabbit Major). 한 행의 일시 조회 실패로 그 행을 결과에서 빼는 편이 rejected 노출보다
            // 안전하고, 전체 검색은 나머지 후보로 계속된다. 여기는 penalty만 매기는 순수 함수라 Err
            // 전파는 하지 않는다.
            let meta = match store.get_validity(&sid, mid) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!(
                        "[tunaRound] get_validity 실패(후보 드롭): sid={sid} mid={mid} err={e}"
                    );
                    continue;
                }
            };
            let mut penalty = 0u32;
            match meta.as_ref().map(|m| m.valid_state.as_str()) {
                Some("rejected") => continue, // 드롭.
                Some("superseded") | Some("stale") => penalty += 2,
                _ => {} // active | unknown | None.
            }
            if current_session == Some(sid.as_str()) {
                // 현재 세션의 off-branch 히트(활성경로 콘텐츠는 repl이 이미 제외) = 버려진 분기.
                penalty += 1;
            }
            // 앵커 매치는 penalty를 넘지 않는 2차 키(매치=0 우선, 미매치=1). 유효성 강등을 넘어서지 못한다.
            let anchor_rank = match meta.as_ref().and_then(|m| m.anchors.as_deref()) {
                Some(a) if anchor_matches(a, query_tokens) => 0u8,
                _ => 1u8,
            };
            let abstraction = meta
                .and_then(|m| m.abstraction)
                .filter(|a| !a.trim().is_empty());
            let ts = store
                .get_created_at(&sid, mid)
                .ok()
                .flatten()
                .and_then(|s| parse_ts_approx(&s));
            if let Some(t) = ts {
                max_ts = Some(max_ts.map_or(t, |m| m.max(t)));
            }
            staged.push((penalty, anchor_rank, sid, mid, v, ts, abstraction));
        }
        // 2차: cross-session recency 강등(정책 A=보수). 다른 세션 && ts 존재 && 최신 대비 임계 초과 → +1.
        // 현재 세션·active·최신·created_at 미상은 불변(relevance/validity 우선 보존).
        let mut scored: Vec<RankScored<T>> = Vec::with_capacity(staged.len());
        for (mut penalty, anchor_rank, sid, mid, v, ts, abstraction) in staged {
            if current_session != Some(sid.as_str())
                && let (Some(t), Some(m)) = (ts, max_ts)
                && m - t > RECENCY_STALE_SECS
            {
                penalty += 1;
            }
            scored.push((penalty, anchor_rank, sid, mid, v, abstraction));
        }
        // 안정 정렬: penalty 1차, 앵커 매치 2차. 같은 (penalty, anchor_rank) 내 relevance 순서 보존.
        scored.sort_by_key(|(p, ar, _, _, _, _)| (*p, *ar));
        scored
            .into_iter()
            .map(|(_, _, sid, mid, v, abstraction)| (sid, mid, v, abstraction))
            .collect()
    }

    /// (session_id, msg_id, Utterance) 항목을 재랭크(유효성+분기+앵커) 후 큐레이션 abstraction을
    /// Utterance.abstraction 필드에 실어(content는 raw 유지) 세션 다양성 cap + limit으로 마무리한다.
    /// 표면화("[요약] 증류문+원문")는 여기가 아니라 렌더 경계에서만 일어난다(이중 주입 방지, v2-51).
    fn finish(
        store: &SqliteStore,
        cands: Vec<(String, u64, Utterance)>,
        limit: usize,
        current_session: Option<&str>,
        query_tokens: &[String],
    ) -> Vec<Utterance> {
        let reranked = rerank(store, cands, current_session, query_tokens);
        let items: Vec<(String, Utterance)> = reranked
            .into_iter()
            .map(|(sid, _mid, mut u, abstraction)| {
                u.abstraction = abstraction; // content는 raw 그대로(dedup 정상 작동).
                (sid, u)
            })
            .collect();
        crate::store::cap_per_session_backfill(items, MAX_PER_SESSION, limit)
    }

    impl SqliteRetriever {
        /// retrieve/retrieve_ctx 공용 구현. current_session=Some이면 분기 인지 디프리오리티.
        /// Ok(빈 벡터)=질의 빈 문자열·매칭 0건. Err=1차 FTS 경로의 DB 장애(R7, 코덱스 #9).
        /// embed/vector/get_message 실패는 FTS 결과로 정당하게 degrade하는 폴백이라 흡수 유지.
        fn retrieve_impl(
            &self,
            query: &str,
            limit: usize,
            current_session: Option<&str>,
        ) -> Result<Vec<Utterance>, String> {
            if query.trim().is_empty() {
                return Ok(Vec::new());
            }

            let q = (self.tok)(query);
            // 앵커 부스트용 질의 토큰(raw query 기반, FTS 연산자 오염 회피). 매 finish에 전달.
            let query_tokens = query_anchor_tokens(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());

            // 1차 FTS 검색(세션 다양성 cap을 위해 over-fetch). 실패=진짜 DB 장애 -> 전파(빈 결과로 은폐 금지).
            let lex_hits = store
                .search(&q, limit * OVERFETCH)
                .map_err(|e| format!("FTS 검색 실패: {e}"))?;

            // embedder 없으면 FTS 단독. 유효성 재랭크 + 세션 다양성 cap(단일 세션은 동작 불변).
            let Some(emb) = &self.embedder else {
                let cands: Vec<(String, u64, Utterance)> = lex_hits
                    .into_iter()
                    .map(|h| (h.session_id, h.msg_id, Utterance::new(h.speaker, h.content)))
                    .collect();
                return Ok(finish(&store, cands, limit, current_session, &query_tokens));
            };

            // FTS 결과 키 리스트 + content_map 구축.
            let lex_keys: Vec<(String, u64)> = lex_hits
                .iter()
                .map(|h| (h.session_id.clone(), h.msg_id))
                .collect();
            let mut content_map: HashMap<(String, u64), (String, String)> = lex_hits
                .into_iter()
                .map(|h| ((h.session_id, h.msg_id), (h.speaker, h.content)))
                .collect();

            // content_map에서 (sid, msg_id, Utterance) 후보를 만드는 폴백용 클로저.
            let cands_from_map =
                |m: HashMap<(String, u64), (String, String)>| -> Vec<(String, u64, Utterance)> {
                    m.into_iter()
                        .map(|((sid, mid), (sp, ct))| (sid, mid, Utterance::new(sp, ct)))
                        .collect()
                };

            // 쿼리 임베딩 시도(실패 시 FTS 단독 폴백 = 정당한 degrade, Err로 승격 안 함).
            let qvec = match emb.embed(query) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("[tunaRound] 쿼리 임베딩 실패(FTS 단독 폴백): {e}");
                    return Ok(finish(
                        &store,
                        cands_from_map(content_map),
                        limit,
                        current_session,
                        &query_tokens,
                    ));
                }
            };

            // 벡터 검색(세션 다양성 cap을 위해 over-fetch). 실패=FTS 단독 폴백(Err로 승격 안 함).
            let vec_hits = match store.vector_search(&qvec, limit * OVERFETCH) {
                Ok(hits) => hits,
                Err(e) => {
                    eprintln!("[tunaRound] 벡터 검색 실패(FTS 단독 폴백): {e}");
                    return Ok(finish(
                        &store,
                        cands_from_map(content_map),
                        limit,
                        current_session,
                        &query_tokens,
                    ));
                }
            };

            let vec_keys: Vec<(String, u64)> = vec_hits
                .iter()
                .map(|(sid, mid, _)| (sid.clone(), *mid))
                .collect();

            // RRF 융합 → (sid, msg_id, Utterance) 후보로 해석(벡터-only 키는 DB 조회).
            let fused = crate::store::reciprocal_rank_fusion(&lex_keys, &vec_keys);
            let mut cands: Vec<(String, u64, Utterance)> = Vec::with_capacity(fused.len());
            for key in fused {
                let utt = if let Some((sp, ct)) = content_map.remove(&key) {
                    Some(Utterance::new(sp, ct))
                } else {
                    match store.get_message(&key.0, key.1) {
                        Ok(Some((sp, ct))) => Some(Utterance::new(sp, ct)),
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
            Ok(finish(&store, cands, limit, current_session, &query_tokens))
        }
    }

    impl crate::orchestrator::ContextRetriever for SqliteRetriever {
        fn retrieve(&self, query: &str, limit: usize) -> Result<Vec<Utterance>, String> {
            self.retrieve_impl(query, limit, None)
        }
        fn retrieve_ctx(
            &self,
            query: &str,
            limit: usize,
            current_session: &str,
        ) -> Result<Vec<Utterance>, String> {
            self.retrieve_impl(query, limit, Some(current_session))
        }

        /// 리치 디버그: 토큰화 결과 + FTS bm25 점수 + 유효성 + 분기 + created_at/recency 표시(step 7·5c).
        fn debug_retrieve(&self, query: &str, limit: usize, current_session: &str) -> String {
            if query.trim().is_empty() {
                return "질의가 비어 있습니다.".to_string();
            }
            let q = (self.tok)(query);
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            let hits = store.search(&q, limit * OVERFETCH).unwrap_or_default();
            let hybrid = if self.embedder.is_some() {
                " (+벡터 하이브리드)"
            } else {
                ""
            };
            // recency: 후보 최신 created_at 대비 임계 초과한 다른 세션 히트를 표시(rerank와 동일 규칙, step 5c).
            let max_ts: Option<i64> = hits
                .iter()
                .filter_map(|h| store.get_created_at(&h.session_id, h.msg_id).ok().flatten())
                .filter_map(|s| parse_ts_approx(&s))
                .max();
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
                let branch = if current_session == h.session_id {
                    " cur-session"
                } else {
                    ""
                };
                let created = store.get_created_at(&h.session_id, h.msg_id).ok().flatten();
                let ts = created.as_deref().and_then(parse_ts_approx);
                let recency = match (ts, max_ts) {
                    (Some(t), Some(m))
                        if h.session_id != current_session && m - t > RECENCY_STALE_SECS =>
                    {
                        " recency↓"
                    }
                    _ => "",
                };
                let created_disp: String = created
                    .as_deref()
                    .map(|s| s.chars().take(10).collect())
                    .unwrap_or_else(|| "?".to_string());
                let snippet: String = h.content.chars().take(50).collect();
                out.push_str(&format!(
                    "  [#{} sid={} bm25={:.3} valid={}{} created={}{}] {}: {}\n",
                    h.msg_id,
                    h.session_id,
                    h.score,
                    state,
                    branch,
                    created_disp,
                    recency,
                    h.speaker,
                    snippet
                ));
            }
            out.push_str("(bm25: 낮을수록 관련 높음. valid=rejected는 제외·superseded/stale·cur-session off-branch는 강등. recency↓=다른 세션의 낡은 후보 강등.)");
            out
        }
    }
}

#[cfg(feature = "sqlite")]
mod sqlite_transcript {
    use crate::store::sqlite::SqliteStore;
    use crate::types::Utterance;

    /// 세션 전사 전체(또는 마지막 N턴)를 활성 경로(root->head)로 읽어 오는 구현.
    /// rusqlite Connection은 Send이지만 Sync가 아니므로 Mutex로 감싼다.
    pub struct SqliteTranscriptReader {
        store: std::sync::Mutex<SqliteStore>,
    }

    impl SqliteTranscriptReader {
        /// SqliteStore를 받아 새 전사 리더를 반환한다.
        pub fn new(store: SqliteStore) -> Self {
            Self {
                store: std::sync::Mutex::new(store),
            }
        }
    }

    impl crate::orchestrator::TranscriptReader for SqliteTranscriptReader {
        fn read_transcript(
            &self,
            session_id: &str,
            max_turns: Option<usize>,
        ) -> Result<Vec<Utterance>, String> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            // 세션 없음(Ok(None))=정상 빈 결과. load_session Err=진짜 DB 장애 -> 전파(R7).
            let ss = match store.load_session(session_id) {
                Ok(Some(ss)) => ss,
                Ok(None) => return Ok(Vec::new()),
                Err(e) => return Err(format!("세션 로드 실패: {e}")),
            };
            let path = crate::types::ConversationSnapshot::from(ss).active_path();
            Ok(match max_turns {
                Some(n) if path.len() > n => path[path.len() - n..].to_vec(),
                _ => path,
            })
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
            Self {
                store: std::sync::Mutex::new(store),
                tok,
            }
        }
    }

    impl crate::orchestrator::TranscriptWriter for SqliteTranscriptWriter {
        fn append_turn(
            &self,
            session_id: &str,
            speaker: &str,
            content: &str,
        ) -> Result<u64, String> {
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
            Self {
                store: std::sync::Mutex::new(store),
                tok,
            }
        }
    }

    impl crate::orchestrator::CoreSync for SqliteCoreSync {
        fn load_session(&self, session_id: &str) -> Option<crate::types::ConversationSnapshot> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            // 세션 없음(Ok(None))=정상 스킵. DB 에러(Err)는 TranscriptReader::read_transcript(R7)와
            // 같은 원칙으로 무신호로 삼키지 않고 eprintln으로 표면화한 뒤 None을 반환한다(시그니처는
            // Option 고정이라 전파 불가 - adopt_from_core가 None을 "세션 없음"과 동일하게 스킵하므로,
            // 그러지 않으면 DB 장애가 무신호로 stale 트리를 쓰게 된다).
            match store.load_session(session_id) {
                Ok(ss) => ss.map(Into::into),
                Err(e) => {
                    eprintln!(
                        "[tunaRound] load_session 실패(스킵, adopt 건너뜀): session_id={session_id} err={e}"
                    );
                    None
                }
            }
        }
        fn append_turn(
            &self,
            session_id: &str,
            speaker: &str,
            content: &str,
        ) -> Result<u64, String> {
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
            Self {
                store: std::sync::Mutex::new(store),
            }
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

    /// 큐레이션 지정 sink 구현(/annotate → message_validity의 abstraction/anchors 쓰기).
    pub struct SqliteAnnotationSink {
        store: std::sync::Mutex<SqliteStore>,
    }

    impl SqliteAnnotationSink {
        pub fn new(store: SqliteStore) -> Self {
            Self {
                store: std::sync::Mutex::new(store),
            }
        }
    }

    impl crate::orchestrator::AnnotationSink for SqliteAnnotationSink {
        fn set_annotation(
            &self,
            session_id: &str,
            msg_id: u64,
            abstraction: Option<&str>,
            anchors: Option<&str>,
        ) -> Result<(), String> {
            let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
            store.set_annotation(session_id, msg_id, abstraction, anchors)
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
        store_w
            .save_session("session-a", &ss_a, |t| t.to_string())
            .unwrap();
        drop(store_w);

        // 별도 읽기 연결로 SqliteRetriever 생성 후 cross-session 검색(embedder=None -> FTS 단독).
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);

        // "session-a"의 발언을 다른 연결에서 retrieve할 수 있어야 한다.
        let hits = retriever.retrieve("검색", 10).unwrap();
        assert!(!hits.is_empty(), "cross-session 검색이 결과를 반환해야 함");
        assert!(
            hits.iter()
                .any(|u| u.content.contains("검색") || u.speaker.contains("claude")),
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
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "a".into(),
                    content: "검색 활성".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "b".into(),
                    content: "검색 대체됨".into(),
                },
                StoredMessage {
                    id: 3,
                    parent_id: Some(2),
                    speaker: "c".into(),
                    content: "검색 기각됨".into(),
                },
            ],
            head: Some(3),
        };
        store_w.save_session("s", &ss, |t| t.to_string()).unwrap();
        store_w.set_validity("s", 2, "superseded", None).unwrap();
        store_w.set_validity("s", 3, "rejected", None).unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("검색", 10).unwrap();
        let contents: Vec<&str> = hits.iter().map(|u| u.content.as_str()).collect();
        assert!(
            !contents.iter().any(|c| c.contains("기각")),
            "rejected는 제외: {contents:?}"
        );
        let pos_active = contents.iter().position(|c| c.contains("활성"));
        let pos_super = contents.iter().position(|c| c.contains("대체"));
        assert!(
            pos_active.is_some() && pos_super.is_some(),
            "active·superseded 모두 존재: {contents:?}"
        );
        assert!(
            pos_active < pos_super,
            "active가 superseded보다 앞: {contents:?}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retrieve_demotes_stale_cross_session() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_recency.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        // 두 세션, 각 1발언, 같은 질의어 "검색" 포함(동일 relevance). "old"를 먼저 저장(기본 순서상 앞).
        store_w
            .save_session(
                "old",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색 오래된".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w
            .save_session(
                "new",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "b".into(),
                        content: "검색 최신".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        // 8일 간격(임계 7일 초과) → 낡은 세션 히트가 강등돼야 함.
        store_w
            .set_created_at("old", 1, "2026-01-01 00:00:00")
            .unwrap();
        store_w
            .set_created_at("new", 1, "2026-01-09 00:00:00")
            .unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("검색", 10).unwrap();
        let contents: Vec<&str> = hits.iter().map(|u| u.content.as_str()).collect();
        let pos_new = contents.iter().position(|c| c.contains("최신"));
        let pos_old = contents.iter().position(|c| c.contains("오래된"));
        assert!(
            pos_new.is_some() && pos_old.is_some(),
            "두 발언 모두 존재: {contents:?}"
        );
        assert!(
            pos_new < pos_old,
            "최신 세션이 낡은 세션보다 앞(recency 강등): {contents:?}"
        );

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
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색 랭킹".into(),
                    }],
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
        assert!(out.contains("created="), "created_at 표시: {out}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn debug_retrieve_marks_stale_cross_session_recency() {
        use crate::orchestrator::ContextRetriever;
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_debug_recency.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        // 두 세션, 같은 질의어. old를 8일 과거로 aging(임계 7일 초과).
        for (sid, body) in [("old", "검색 오래된"), ("new", "검색 최신")] {
            store_w
                .save_session(
                    sid,
                    &StoredSession {
                        messages: vec![StoredMessage {
                            id: 1,
                            parent_id: None,
                            speaker: "a".into(),
                            content: body.into(),
                        }],
                        head: Some(1),
                    },
                    |t| t.to_string(),
                )
                .unwrap();
        }
        store_w
            .set_created_at("old", 1, "2026-01-01 00:00:00")
            .unwrap();
        store_w
            .set_created_at("new", 1, "2026-01-09 00:00:00")
            .unwrap();
        drop(store_w);
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        // current_session은 제3자("none")라 old·new 모두 다른 세션 → old만 recency 강등 표시돼야.
        let out = retriever.debug_retrieve("검색", 10, "none");
        assert!(
            out.contains("recency↓"),
            "낡은 다른세션 후보에 recency 표시: {out}"
        );
        assert!(
            out.contains("created=2026-01-01"),
            "created_at 날짜 표시: {out}"
        );
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
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색 현재세션".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w
            .save_session(
                "oth",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "b".into(),
                        content: "검색 다른세션".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);

        // 현재 세션="cur" → cur의 off-branch 히트가 타 세션(oth)보다 뒤로 강등.
        let hits = retriever.retrieve_ctx("검색", 10, "cur").unwrap();
        let contents: Vec<&str> = hits.iter().map(|u| u.content.as_str()).collect();
        let pos_other = contents.iter().position(|c| c.contains("다른세션"));
        let pos_cur = contents.iter().position(|c| c.contains("현재세션"));
        assert!(
            pos_other.is_some() && pos_cur.is_some(),
            "둘 다 존재: {contents:?}"
        );
        assert!(
            pos_other < pos_cur,
            "다른 세션이 현재세션 off-branch보다 앞: {contents:?}"
        );

        // 컨텍스트 없는 retrieve는 분기 페널티 없음(둘 다 반환).
        assert_eq!(retriever.retrieve("검색", 10).unwrap().len(), 2);

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retrieve_carries_abstraction_and_keeps_raw_content() {
        // 큐레이션(v2-51): abstraction은 별도 필드로 실려 오고 content는 **원문 raw 그대로**여야 한다
        // (repl의 content 기반 중복제거가 정상 작동해 이중 주입을 막는다). 표면화는 렌더 경계 담당.
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_abstraction.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "s",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "원문 검색 구현 논의".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        // 사람이 남긴 증류 요약(abstraction). anchors는 None으로 두어 캐리만 검증.
        store_w
            .set_annotation("s", 1, Some("핵심 결정: 하이브리드 검색 채택"), None)
            .unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("검색", 10).unwrap();
        assert_eq!(hits.len(), 1, "1건 반환: {hits:?}");
        // content는 원문 raw 그대로(표면화되지 않음).
        assert_eq!(
            hits[0].content, "원문 검색 구현 논의",
            "content가 원문 raw가 아님: {:?}",
            hits[0].content
        );
        // abstraction은 별도 필드로 실려 온다.
        assert_eq!(
            hits[0].abstraction.as_deref(),
            Some("핵심 결정: 하이브리드 검색 채택"),
            "abstraction 캐리 안 됨"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retrieve_boosts_anchor_match_within_penalty_tier() {
        // 큐레이션(v2-51): 같은 penalty tier(둘 다 active·cross-session)에서 쿼리와 매치되는 anchors를
        // 가진 발언이 순위를 올린다. 앵커 없는 baseline은 FTS relevance 순(dense="a" 먼저).
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_anchor_boost.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();

        // "a"=짧고 밀도 높은 매치(FTS 상위), "b"=길어 상대적으로 하위. 둘 다 유효성 미설정 → penalty 0 동률.
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "a",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w
            .save_session(
                "b",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "b".into(),
                        content: "검색 시스템 상세 설계 배경 기록".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        // baseline(앵커 없음): FTS relevance로 "a"(밀도 높음)가 앞선다.
        let store_b = SqliteStore::open(p).unwrap();
        let base = SqliteRetriever::new(store_b, Box::new(|t: &str| t.to_string()), None);
        let base_hits = base.retrieve("검색", 10).unwrap();
        let base_first = base_hits.first().map(|u| u.speaker.clone());
        assert_eq!(
            base_first.as_deref(),
            Some("a"),
            "baseline은 dense 'a'가 먼저: {base_hits:?}"
        );

        // "b"에 쿼리("검색")와 매치되는 anchors 부여 → 같은 penalty tier에서 'b'가 앞서야 한다.
        let store_ann = SqliteStore::open(p).unwrap();
        store_ann
            .set_annotation("b", 1, None, Some("검색,아키텍처"))
            .unwrap();
        drop(store_ann);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("검색", 10).unwrap();
        assert_eq!(hits.len(), 2, "두 발언 모두 반환: {hits:?}");
        assert_eq!(
            hits[0].speaker, "b",
            "앵커 매치 발언이 부스트로 먼저: {hits:?}"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn retrieve_boosts_hyphenated_anchor_match() {
        // gemini HIGH 회귀 방지: 하이픈·언더스코어가 든 앵커(예 "RAG-설계")도 앵커 분리가 비영숫자 경계라
        // 질의 토큰("설계")과 완전일치해 부스트된다(콤마·공백 분리였다면 "rag-설계"로 남아 매치 실패).
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_anchor_hyphen.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();

        // "a"=밀도 높은 매치(baseline 상위), "b"=길어 하위. 둘 다 penalty 0 동률.
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "a",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "설계".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        store_w
            .save_session(
                "b",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "b".into(),
                        content: "설계 시스템 상세 배경 기록".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        // "b"에 하이픈·언더스코어 포함 앵커 부여. 질의 토큰 "설계"가 "RAG-설계" 안 토큰과 완전일치해야 한다.
        store_w
            .set_annotation("b", 1, None, Some("RAG-설계,mesh_scan"))
            .unwrap();
        drop(store_w);

        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|t: &str| t.to_string()), None);
        let hits = retriever.retrieve("설계", 10).unwrap();
        assert_eq!(hits.len(), 2, "두 발언 모두 반환: {hits:?}");
        assert_eq!(
            hits[0].speaker, "b",
            "하이픈 포함 앵커도 완전일치 부스트로 먼저: {hits:?}"
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
        store_w
            .save_session("hybrid-s", &ss, |t| t.to_string())
            .unwrap();
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
        let hits = retriever.retrieve("검색", 10).unwrap();
        assert!(
            !hits.is_empty(),
            "하이브리드 검색이 결과를 반환해야 함: {:?}",
            hits
        );

        let _ = std::fs::remove_file(&path);
    }

    /// (R7) 1차 FTS(store.search) 실패는 빈 벡터로 은폐하지 않고 Err로 전파해야 한다.
    /// 토크나이저가 FTS5 문법 오류(닫히지 않은 따옴표)를 내보내 store.search를 실제로 실패시킨다.
    #[test]
    fn retrieve_propagates_fts_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_fts_err.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "s",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색 내용".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        // 토크나이저가 항상 닫히지 않은 따옴표(FTS5 문법 오류)를 반환 -> store.search가 Err.
        let store_r = SqliteStore::open(p).unwrap();
        let retriever = SqliteRetriever::new(store_r, Box::new(|_: &str| "\"".to_string()), None);
        let res = retriever.retrieve("검색", 10);
        assert!(
            res.is_err(),
            "FTS 검색 실패는 Err로 전파돼야 함(빈 벡터 은폐 금지): {res:?}"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// (R7) "매칭 0건"(정상, Ok(빈 벡터))과 "DB 오류"(Err)를 명확히 구분한다.
    #[test]
    fn retrieve_distinguishes_empty_from_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_retriever_empty_vs_err.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "s",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "검색 내용".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        // 정상 토크나이저: 매칭 없는 질의 -> Ok(빈 벡터). 빈 질의도 Ok(빈 벡터).
        let store_ok = SqliteStore::open(p).unwrap();
        let ok = SqliteRetriever::new(store_ok, Box::new(|t: &str| t.to_string()), None);
        let no_match = ok.retrieve("존재하지않는질의어zzzqqq", 10);
        assert!(
            matches!(no_match, Ok(ref v) if v.is_empty()),
            "매칭 0건은 Ok(빈 벡터): {no_match:?}"
        );
        let blank = ok.retrieve("   ", 10);
        assert!(
            matches!(blank, Ok(ref v) if v.is_empty()),
            "빈 질의는 Ok(빈 벡터): {blank:?}"
        );

        // 오류 토크나이저: 같은 DB라도 store.search 실패 -> Err(빈 결과와 구분됨).
        let store_err = SqliteStore::open(p).unwrap();
        let err = SqliteRetriever::new(store_err, Box::new(|_: &str| "\"".to_string()), None);
        assert!(
            err.retrieve("검색", 10).is_err(),
            "DB 오류는 Err(매칭 0건과 구분)"
        );

        let _ = std::fs::remove_file(&path);
    }

    /// (R7) read_transcript: 세션 없음 -> Ok(빈 벡터), load_session DB 오류 -> Err.
    #[test]
    fn read_transcript_distinguishes_missing_session_from_error() {
        use super::SqliteTranscriptReader;
        use crate::orchestrator::TranscriptReader;

        let dir = std::env::temp_dir();
        let path = dir.join("tuna_transcript_missing_vs_err.db");
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w
            .save_session(
                "s",
                &StoredSession {
                    messages: vec![StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "a".into(),
                        content: "전사 발언".into(),
                    }],
                    head: Some(1),
                },
                |t| t.to_string(),
            )
            .unwrap();
        drop(store_w);

        // 세션 없음 -> Ok(빈 벡터)(정상), 존재 세션 -> Ok(발언).
        let store_r = SqliteStore::open(p).unwrap();
        let reader = SqliteTranscriptReader::new(store_r);
        let missing = reader.read_transcript("nonexistent-session", None);
        assert!(
            matches!(missing, Ok(ref v) if v.is_empty()),
            "세션 없음은 Ok(빈 벡터): {missing:?}"
        );
        let present = reader.read_transcript("s", None).unwrap();
        assert_eq!(present.len(), 1, "존재 세션은 발언 반환: {present:?}");

        // DB 오류 유도: 별도 연결로 messages 테이블 드롭 -> load_session의 messages 조회 실패 -> Err.
        let raw = rusqlite::Connection::open(p).unwrap();
        raw.execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE messages;")
            .unwrap();
        drop(raw);
        let err = reader.read_transcript("s", None);
        assert!(
            err.is_err(),
            "load_session DB 오류는 Err로 전파(세션 없음과 구분): {err:?}"
        );

        let _ = std::fs::remove_file(&path);
    }
}
