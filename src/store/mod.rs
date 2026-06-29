// 전사 영속의 직렬화 형식과 변환. 트리-ready(id/parent), v1은 선형 체인.

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
