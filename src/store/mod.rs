// 전사 영속의 직렬화 형식과 변환. 트리-ready(id/parent), v1은 선형 체인.

#[cfg(feature = "sqlite")]
pub mod a2a;
#[cfg(feature = "sqlite")]
pub mod agents;
#[cfg(feature = "sqlite")]
pub mod candidates;
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
        assert_eq!(result[0], ("s1".to_string(), 1u64), "양쪽 상위 키가 최상위여야 함");
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
        assert_eq!(out, vec![0, 1, 2, 3, 4], "단일 세션은 under-fill 없이 가득 채워야 함");
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
        .map(|m| Utterance { speaker: m.speaker.clone(), content: m.content.clone() })
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

/// head에서 parent_id를 따라 root까지 거슬러 올라간 경로(루트->head 순)를 전사로 반환.
/// HashMap O(1) 조회 + HashSet 순환 가드로 무한루프를 방지한다.
pub fn path_to_root(messages: &[StoredMessage], head: Option<u64>) -> Vec<Utterance> {
    use std::collections::{HashMap, HashSet};
    let by_id: HashMap<u64, &StoredMessage> = messages.iter().map(|m| (m.id, m)).collect();
    let mut chain: Vec<&StoredMessage> = Vec::new();
    let mut seen: HashSet<u64> = HashSet::new();
    let mut cur = head;
    while let Some(id) = cur {
        if !seen.insert(id) {
            break; // 순환 가드
        }
        match by_id.get(&id) {
            Some(m) => {
                chain.push(*m);
                cur = m.parent_id;
            }
            None => break,
        }
    }
    chain.reverse();
    chain.iter().map(|m| Utterance { speaker: m.speaker.clone(), content: m.content.clone() }).collect()
}

/// 다음 메시지 id(max+1, 비어 있으면 1).
pub fn next_id(messages: &[StoredMessage]) -> u64 {
    messages.iter().map(|m| m.id).max().map(|m| m + 1).unwrap_or(1)
}

/// 트리 요약 줄(id, parent, speaker, 본문 일부). /branches 표시용.
pub fn tree_summary(messages: &[StoredMessage], head: Option<u64>) -> String {
    if messages.is_empty() {
        return "(빈 트리)".to_string();
    }
    let mut out = String::new();
    for m in messages {
        let marker = if Some(m.id) == head { "*" } else { " " };
        let parent = m.parent_id.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        let snippet: String = m.content.chars().take(30).collect();
        out.push_str(&format!("{marker} #{} (<-{parent}) {}: {}\n", m.id, m.speaker, snippet));
    }
    out
}

/// StoredSession을 JSON으로 저장.
pub fn save_session(s: &StoredSession, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// StoredSession 로드. 레거시 bare-array(head 없음)이면 head=마지막 id로 폴백.
pub fn load_session(path: &str) -> std::io::Result<StoredSession> {
    let s = std::fs::read_to_string(path)?;
    if let Ok(ss) = serde_json::from_str::<StoredSession>(&s) {
        return Ok(ss);
    }
    // 레거시 v1: bare [StoredMessage]
    let messages: Vec<StoredMessage> = serde_json::from_str(&s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let head = messages.iter().map(|m| m.id).max();
    Ok(StoredSession { messages, head })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::Utterance;

    fn utts() -> Vec<Utterance> {
        vec![
            Utterance { speaker: "claude/proposer".into(), content: "제안".into() },
            Utterance { speaker: "codex/reviewer".into(), content: "리뷰".into() },
        ]
    }

    #[test]
    fn path_to_root_walks_parents() {
        // 트리: 1 -> 2 -> 3, 그리고 2 -> 4 (분기)
        let msgs = vec![
            StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
            StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
            StoredMessage { id: 3, parent_id: Some(2), speaker: "c".into(), content: "3".into() },
            StoredMessage { id: 4, parent_id: Some(2), speaker: "d".into(), content: "4".into() },
        ];
        let path = path_to_root(&msgs, Some(3));
        assert_eq!(path.iter().map(|u| u.content.clone()).collect::<Vec<_>>(), vec!["1","2","3"]);
        let branch = path_to_root(&msgs, Some(4));
        assert_eq!(branch.iter().map(|u| u.content.clone()).collect::<Vec<_>>(), vec!["1","2","4"]);
        assert!(path_to_root(&msgs, None).is_empty());
    }

    #[test]
    fn path_to_root_cycle_guard_terminates() {
        // 순환: id=1.parent=Some(2), id=2.parent=Some(1). head=Some(1).
        // 무한루프 없이 유계 반환(2개 이하).
        let msgs = vec![
            StoredMessage { id: 1, parent_id: Some(2), speaker: "a".into(), content: "1".into() },
            StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
        ];
        let path = path_to_root(&msgs, Some(1));
        // 순환이므로 결과 길이가 유한해야 함(2개 이하).
        assert!(path.len() <= 2, "순환에서 유한 반환이어야 함: len={}", path.len());
    }

    #[test]
    fn next_id_is_max_plus_one() {
        assert_eq!(next_id(&[]), 1);
        let msgs = vec![StoredMessage { id: 5, parent_id: None, speaker: "a".into(), content: "x".into() }];
        assert_eq!(next_id(&msgs), 6);
    }

    #[test]
    fn session_roundtrip_preserves_tree_and_head() {
        let ss = StoredSession {
            messages: vec![
                StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
                StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
            ],
            head: Some(2),
        };
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_session_rt.json");
        save_session(&ss, path.to_str().unwrap()).unwrap();
        let back = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(back.messages, ss.messages);
        assert_eq!(back.head, Some(2));
    }

    #[test]
    fn load_session_falls_back_to_legacy_bare_array() {
        // 레거시 v1: bare [StoredMessage] (head 없음) -> head = 마지막 id
        let legacy = vec![
            StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
            StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_legacy.json");
        save(&legacy, path.to_str().unwrap()).unwrap(); // 기존 bare-array 저장
        let ss = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(ss.messages.len(), 2);
        assert_eq!(ss.head, Some(2)); // 마지막 id
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
