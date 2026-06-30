// 전사 영속의 직렬화 형식과 변환. 트리-ready(id/parent), v1은 선형 체인.

#[cfg(feature = "sqlite")]
pub mod sqlite;

use serde::{Deserialize, Serialize};

use crate::orchestrator::Utterance;

/// 영속 메시지. 트리-ready: parent_id로 체인/분기 표현(v1은 선형, parent=직전).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub speaker: String,
    pub content: String,
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
pub fn path_to_root(messages: &[StoredMessage], head: Option<u64>) -> Vec<Utterance> {
    let mut chain: Vec<&StoredMessage> = Vec::new();
    let mut cur = head;
    while let Some(id) = cur {
        match messages.iter().find(|m| m.id == id) {
            Some(m) => {
                chain.push(m);
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
    use crate::orchestrator::Utterance;

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
