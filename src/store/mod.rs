// 전사 영속의 직렬화 형식과 변환. 트리-ready(id/parent), v1은 선형 체인.

#[cfg(feature = "sqlite")]
pub mod a2a;
#[cfg(feature = "sqlite")]
pub mod agents;
#[cfg(feature = "sqlite")]
#[cfg(feature = "sqlite")]
pub mod sqlite;

pub mod embedding;
pub mod indexer;
pub mod retriever;

/// RRF 상수: 랭킹 압축 계수(secall 답습, k=60).
#[cfg(feature = "sqlite")]
const RRF_K: f64 = 60.0;

/// 두 랭킹 리스트(키=(session_id, msg_id))를 RRF로 융합해 점수 내림차순 키 목록을 반환한다.
/// secall hybrid.rs::reciprocal_rank_fusion 답습(k=60, 1/(k+rank+1) 누적 후 내림차순).
#[cfg(feature = "sqlite")]
pub(crate) fn reciprocal_rank_fusion(
    lexical: &[(String, u64)],
    vector: &[(String, u64)],
) -> Vec<(String, u64)> {
    use std::collections::HashMap;

    let mut scores: HashMap<(String, u64), f64> = HashMap::new();

    for (rank, key) in lexical.iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
        *scores.entry((key.0.clone(), key.1)).or_insert(0.0) += rrf;
    }
    for (rank, key) in vector.iter().enumerate() {
        let rrf = 1.0 / (RRF_K + rank as f64 + 1.0);
        *scores.entry((key.0.clone(), key.1)).or_insert(0.0) += rrf;
    }

    // 점수 내림차순, 동점이면 키(session_id, msg_id) 오름차순으로 안정 정렬.
    let mut ranked: Vec<(String, u64)> = scores.keys().cloned().collect();
    ranked.sort_by(|a, b| {
        let sa = scores[a];
        let sb = scores[b];
        sb.partial_cmp(&sa)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
            .then_with(|| a.1.cmp(&b.1))
    });

    ranked
}

/// 검색 결과 세션 다양성: 순서를 보존하며 session_id별 최대 max_per_session개를 우선(primary)으로
/// 뽑고, limit에 못 미치면 나머지(overflow)로 backfill해 limit까지 채운다.
/// 다중 세션이면 다양하게, 단일 세션이면 그 세션으로 가득 채워 동작이 불변이다(under-fill 없음).
#[cfg(feature = "sqlite")]
pub(crate) fn cap_per_session_backfill<T>(
    items: Vec<(String, T)>,
    max_per_session: usize,
    limit: usize,
) -> Vec<T> {
    use std::collections::HashMap;
    let mut counts: HashMap<String, usize> = HashMap::new();
    let mut primary: Vec<T> = Vec::new();
    let mut overflow: Vec<T> = Vec::new();
    for (sid, item) in items {
        let c = counts.entry(sid).or_insert(0);
        if *c < max_per_session {
            *c += 1;
            primary.push(item);
        } else {
            overflow.push(item);
        }
    }
    primary.into_iter().chain(overflow).take(limit).collect()
}

#[cfg(all(test, feature = "sqlite"))]
mod rrf_tests {
    use super::reciprocal_rank_fusion;

    #[test]
    fn rrf_basic_top_keys_from_both_lists() {
        // 두 리스트에서 상위에 공통으로 나타나는 키가 최상위로 올라와야 한다(secall test_rrf_basic 적응).
        let lexical = vec![
            ("s1".to_string(), 1u64),
            ("s1".to_string(), 2u64),
            ("s2".to_string(), 1u64),
        ];
        let vector = vec![
            ("s1".to_string(), 1u64),
            ("s2".to_string(), 1u64),
            ("s1".to_string(), 2u64),
        ];
        let result = reciprocal_rank_fusion(&lexical, &vector);
        assert!(!result.is_empty());
        // ("s1", 1)은 두 리스트에서 모두 rank=0 -> 가장 높은 RRF 점수를 가져야 한다.
        assert_eq!(
            result[0],
            ("s1".to_string(), 1u64),
            "양쪽 상위 키가 최상위여야 함"
        );
    }

    use super::cap_per_session_backfill;

    #[test]
    fn cap_diverse_prefers_other_sessions_then_backfills() {
        // s1이 3개 연속, s2가 1개. max_per_session=2, limit=4 → primary=[s1,s1,s2], backfill=[s1].
        let items = vec![
            ("s1".to_string(), "a"),
            ("s1".to_string(), "b"),
            ("s1".to_string(), "c"),
            ("s2".to_string(), "d"),
        ];
        let out = cap_per_session_backfill(items, 2, 4);
        // 다양성 우선: s1 2개 + s2 1개 먼저, 그 뒤 s1 overflow 1개로 채움.
        assert_eq!(out, vec!["a", "b", "d", "c"]);
    }

    #[test]
    fn cap_single_session_fills_without_underfill() {
        // 단일 세션 5개, max_per_session=2, limit=5 → backfill로 5개 모두(동작 불변).
        let items: Vec<(String, i32)> = (0..5).map(|i| ("s1".to_string(), i)).collect();
        let out = cap_per_session_backfill(items, 2, 5);
        assert_eq!(
            out,
            vec![0, 1, 2, 3, 4],
            "단일 세션은 under-fill 없이 가득 채워야 함"
        );
    }

    #[test]
    fn rrf_empty_lists_return_empty() {
        assert!(reciprocal_rank_fusion(&[], &[]).is_empty());
    }

    #[test]
    fn rrf_single_list_preserves_order() {
        let lexical = vec![
            ("a".to_string(), 1u64),
            ("b".to_string(), 2u64),
            ("c".to_string(), 3u64),
        ];
        let result = reciprocal_rank_fusion(&lexical, &[]);
        // 단일 리스트면 rank 0이 최고 점수 -> 원래 순서 유지.
        assert_eq!(result[0], ("a".to_string(), 1u64));
    }
}

use serde::{Deserialize, Serialize};

use crate::types::Utterance;

/// 영속 메시지. 트리-ready: parent_id로 체인/분기 표현(v1은 선형, parent=직전).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub speaker: String,
    pub content: String,
}

/// 발언에 레이어링되는 유효성·요약 메타(원문 StoredMessage와 분리, Memora식).
/// message_validity 테이블에 저장. 없으면 기본 active로 간주한다(step 5 랭킹에서 사용).
#[cfg(feature = "sqlite")]
#[derive(Debug, Clone, PartialEq)]
pub struct Validity {
    /// active | superseded | rejected | stale | unknown.
    pub valid_state: String,
    /// 이 발언을 대체한 발언 id(superseded일 때).
    pub superseded_by: Option<u64>,
    /// 결정 요약(Memora primary abstraction). 생성 파이프라인은 후속.
    pub abstraction: Option<String>,
    /// 검색 단서(모듈·에러·쟁점). 생성 파이프라인은 후속.
    pub anchors: Option<String>,
}

/// 전사를 stored로. id는 1부터, parent는 직전 id(첫 메시지는 None).
pub fn to_stored(transcript: &[Utterance]) -> Vec<StoredMessage> {
    let mut out = Vec::with_capacity(transcript.len());
    let mut prev: Option<u64> = None;
    for (i, u) in transcript.iter().enumerate() {
        let id = (i as u64) + 1;
        out.push(StoredMessage {
            id,
            parent_id: prev,
            speaker: u.speaker.clone(),
            content: u.content.clone(),
        });
        prev = Some(id);
    }
    out
}

/// stored를 전사로(메타 버리고 speaker/content만). v1 선형 가정.
pub fn from_stored(messages: &[StoredMessage]) -> Vec<Utterance> {
    messages
        .iter()
        .map(|m| Utterance::new(m.speaker.clone(), m.content.clone()))
        .collect()
}

/// stored 메시지를 JSON 파일로 저장.
pub fn save(messages: &[StoredMessage], path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// JSON 파일에서 stored 메시지를 로드.
pub fn load(path: &str) -> std::io::Result<Vec<StoredMessage>> {
    let s = std::fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// 세션 저장 단위: 메시지 트리 + 현재 head(활성 분기 끝).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub messages: Vec<StoredMessage>,
    pub head: Option<u64>,
}

// v2-52 ⑤: 영속 DTO(StoredSession, serde 有)↔ 중립 도메인(ConversationSnapshot, serde 無) 변환 경계.
// 이 경계가 store 계층에만 있어 SQLite 스키마·와이어 포맷이 consumer로 새지 않는다. 저수준 SQLite 매핑
// (SqliteStore::*)은 계속 StoredSession을 생산·소비하고(오라클 불변), 트레잇 래퍼가 여기서 변환한다.
impl From<StoredSession> for crate::types::ConversationSnapshot {
    fn from(ss: StoredSession) -> Self {
        let nodes = ss
            .messages
            .into_iter()
            .map(|m| crate::types::MessageNode {
                id: m.id,
                parent: m.parent_id,
                speaker: m.speaker,
                content: m.content,
            })
            .collect();
        crate::types::ConversationSnapshot::from_parts(nodes, crate::types::BranchHead(ss.head))
    }
}

impl From<&crate::types::ConversationSnapshot> for StoredSession {
    fn from(snap: &crate::types::ConversationSnapshot) -> Self {
        let messages = snap
            .nodes()
            .iter()
            .map(|n| StoredMessage {
                id: n.id,
                parent_id: n.parent,
                speaker: n.speaker.clone(),
                content: n.content.clone(),
            })
            .collect();
        StoredSession {
            messages,
            head: snap.head().tip(),
        }
    }
}

// v2-52 ⑤ S6: path_to_root·next_id·tree_summary 자유함수는 ConversationSnapshot 메서드
// (active_path·append·tree_summary)로 흡수·삭제됨. 트리 순회·id 채번은 이제 도메인 타입에만 산다.

/// ConversationSnapshot을 JSON으로 저장한다. 와이어 포맷은 StoredSession(serde)이라 하위호환 불변이다
/// (중립 타입은 serde 없음 → 직렬화 책임이 store 경계에만, v2-52 ⑤).
pub fn save_session(snap: &crate::types::ConversationSnapshot, path: &str) -> std::io::Result<()> {
    let ss = StoredSession::from(snap);
    let json = serde_json::to_string_pretty(&ss)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// ConversationSnapshot을 로드한다. 와이어=StoredSession 역직렬화(레거시 bare-array면 head=마지막 id
/// 폴백) 후 중립 타입으로 변환. 하위호환 로직은 이 StoredSession 경계에만 존재한다.
pub fn load_session(path: &str) -> std::io::Result<crate::types::ConversationSnapshot> {
    let s = std::fs::read_to_string(path)?;
    let ss = if let Ok(ss) = serde_json::from_str::<StoredSession>(&s) {
        ss
    } else {
        // 레거시 v1: bare [StoredMessage]
        let messages: Vec<StoredMessage> = serde_json::from_str(&s)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        let head = messages.iter().map(|m| m.id).max();
        StoredSession { messages, head }
    };
    Ok(ss.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Utterance;

    fn utts() -> Vec<Utterance> {
        vec![
            Utterance {
                speaker: "claude/proposer".into(),
                content: "제안".into(),
                abstraction: None,
            },
            Utterance {
                speaker: "codex/reviewer".into(),
                content: "리뷰".into(),
                abstraction: None,
            },
        ]
    }

    #[test]
    fn stored_session_snapshot_roundtrip_and_equivalence() {
        // v2-52 ⑤: From 변환이 트리·head를 보존하고(round-trip) active_path가 순회를 보존하는지.
        use crate::types::ConversationSnapshot;
        let ss = StoredSession {
            messages: vec![
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "a".into(),
                    content: "1".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "b".into(),
                    content: "2".into(),
                },
                StoredMessage {
                    id: 3,
                    parent_id: Some(1),
                    speaker: "c".into(),
                    content: "3".into(),
                },
            ],
            head: Some(3),
        };
        let snap: ConversationSnapshot = ss.clone().into();
        // 라운드트립 = 원본 보존.
        let back: StoredSession = (&snap).into();
        assert_eq!(back, ss);
        // From 변환 후 active_path가 트리 순회를 보존(head=3, parent=1 → [1, 3]).
        assert_eq!(
            snap.active_path()
                .iter()
                .map(|u| u.content.clone())
                .collect::<Vec<_>>(),
            vec!["1", "3"]
        );
    }

    #[test]
    fn session_roundtrip_preserves_tree_and_head() {
        let ss = StoredSession {
            messages: vec![
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "a".into(),
                    content: "1".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "b".into(),
                    content: "2".into(),
                },
            ],
            head: Some(2),
        };
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_session_rt.json");
        // save/load는 ConversationSnapshot 경계이나 와이어 포맷=StoredSession이라 라운드트립 등가.
        let snap: crate::types::ConversationSnapshot = ss.clone().into();
        save_session(&snap, path.to_str().unwrap()).unwrap();
        let back = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(StoredSession::from(&back), ss);
        assert_eq!(back.head().tip(), Some(2));
    }

    #[test]
    fn load_session_falls_back_to_legacy_bare_array() {
        // 레거시 v1: bare [StoredMessage] (head 없음) -> head = 마지막 id
        let legacy = vec![
            StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "a".into(),
                content: "1".into(),
            },
            StoredMessage {
                id: 2,
                parent_id: Some(1),
                speaker: "b".into(),
                content: "2".into(),
            },
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_legacy.json");
        save(&legacy, path.to_str().unwrap()).unwrap(); // 기존 bare-array 저장
        let snap = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(snap.node_count(), 2);
        assert_eq!(snap.head().tip(), Some(2)); // 마지막 id로 폴백
    }

    #[test]
    fn to_stored_assigns_linear_ids_and_parents() {
        let s = to_stored(&utts());
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, 1);
        assert_eq!(s[0].parent_id, None);
        assert_eq!(s[1].id, 2);
        assert_eq!(s[1].parent_id, Some(1)); // 트리-ready: 직전이 parent
        assert_eq!(s[1].speaker, "codex/reviewer");
    }

    #[test]
    fn roundtrip_stored_to_transcript() {
        let original = utts();
        let back = from_stored(&to_stored(&original));
        assert_eq!(back, original);
    }
}
