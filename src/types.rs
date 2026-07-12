// 계층 경계를 넘나드는 공용 도메인 값 타입. store(영속)와 orchestrator(정책)가 모두 참조하되 서로를
// import하지 않도록, 어느 한쪽이 아니라 중립 위치에 둔다(store->orchestrator 역방향 결합 제거).

/// 한 발언. speaker=Participant.label(), content=응답 본문(원문 raw).
/// abstraction=큐레이션 증류 요약(v2-51). Some이면 주입/표시 렌더 시점에 원문 앞에 표면화된다.
/// content는 항상 원문 raw를 유지한다(repl의 content 기반 중복제거가 정상 작동하도록). 표면화는
/// 렌더 경계(prompt::join_utterances·repl::render)에서만 일어나 이중 주입을 구조적으로 막는다.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Utterance {
    pub speaker: String,
    pub content: String,
    pub abstraction: Option<String>,
}

impl Utterance {
    /// abstraction 없는 발언(대다수 경로). `Utterance { speaker, content }` 리터럴 대체용 헬퍼.
    pub fn new(speaker: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            speaker: speaker.into(),
            content: content.into(),
            abstraction: None,
        }
    }
}

// v2-52 ⑤ store DTO ↔ 도메인 경계: 중립 도메인 타입.
// 계약 정본 = docs/design/v2-52-store-dto-contract_2026-07-12.md. 핵심 = **serde 없음**이라 와이어
// 포맷이 도메인에 새지 않는다(영속·직렬화는 store의 StoredSession/StoredMessage 전담). 트리 순회·append
// 상태머신을 ConversationSnapshot 메서드로 캡슐화해 SQLite 스키마 형태(msg_id/parent_id/head)가
// consumer(repl/prompt) 로직에 누수되는 결합을 끊는다.

/// 메시지 트리 노드 식별자(DB msg_id 권위). 스칼라라 newtype 미승격(§6-1): wrap이 CLI 파싱·append_turn
/// 반환·writer/sink로 번지기만 하고 트리-shape 누수 해결과 무관하다.
pub type MessageId = u64;

/// 대화 트리의 한 노드. store의 StoredMessage(serde 有)를 도메인으로 승격한 판(같은 shape, serde 無).
/// abstraction/valid_state는 별도 관심사(message_validity 테이블)라 담지 않는다(스코프 밖).
#[derive(Debug, Clone, PartialEq)]
pub struct MessageNode {
    pub id: MessageId,
    pub parent: Option<MessageId>,
    pub speaker: String,
    pub content: String,
}

/// 활성 분기 끝(head). None이면 빈 세션. cli_run observe의 head-변화(분기 축소) 커서 리싱크를 명명.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct BranchHead(pub Option<MessageId>);

impl BranchHead {
    /// 활성 분기 끝 id(없으면 None).
    pub fn tip(&self) -> Option<MessageId> {
        self.0
    }
}

/// 대화 트리 스냅샷 = 트리 구조를 가진 Utterance 저장소. 트리 순회·append 상태머신을 메서드로 흡수한다
/// (현재 (&[StoredMessage], head)로 분해돼 REPL이 호출하는 자유함수·리터럴 조립을 회수). serde 없음.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ConversationSnapshot {
    nodes: Vec<MessageNode>,
    head: BranchHead,
}

impl ConversationSnapshot {
    /// 빈 스냅샷.
    pub fn new() -> Self {
        Self::default()
    }

    /// 노드·head로 직접 구성한다(store From 변환·복원 경로 전용).
    pub fn from_parts(nodes: Vec<MessageNode>, head: BranchHead) -> Self {
        Self { nodes, head }
    }

    /// 활성 head 포인터.
    pub fn head(&self) -> BranchHead {
        self.head
    }

    /// 트리 전체 노드(분기 포함, 읽기 전용). store 변환·재조립 경로 전용.
    pub fn nodes(&self) -> &[MessageNode] {
        &self.nodes
    }

    /// head에서 parent를 따라 root까지 거슬러 올라간 활성 경로(root→head 순)를 전사로 반환한다.
    /// HashMap O(1) 조회 + HashSet 순환 가드(store::path_to_root 의미 보존).
    pub fn active_path(&self) -> Vec<Utterance> {
        use std::collections::{HashMap, HashSet};
        let by_id: HashMap<MessageId, &MessageNode> =
            self.nodes.iter().map(|n| (n.id, n)).collect();
        let mut chain: Vec<&MessageNode> = Vec::new();
        let mut seen: HashSet<MessageId> = HashSet::new();
        let mut cur = self.head.0;
        while let Some(id) = cur {
            if !seen.insert(id) {
                break; // 순환 가드
            }
            match by_id.get(&id) {
                Some(n) => {
                    chain.push(n);
                    cur = n.parent;
                }
                None => break,
            }
        }
        chain.reverse();
        chain
            .iter()
            .map(|n| Utterance::new(n.speaker.clone(), n.content.clone()))
            .collect()
    }

    /// 현재 head 자식으로 발언을 추가하고 새 id를 반환한다. id=max+1(빈=1), parent=현재 head, head 전진을
    /// 원자적으로 캡슐화(store::next_id + repl append 상태머신 흡수, DB id 권위 규칙 재현).
    pub fn append(&mut self, speaker: impl Into<String>, content: impl Into<String>) -> MessageId {
        let id = self
            .nodes
            .iter()
            .map(|n| n.id)
            .max()
            .map(|m| m + 1)
            .unwrap_or(1);
        self.nodes.push(MessageNode {
            id,
            parent: self.head.0,
            speaker: speaker.into(),
            content: content.into(),
        });
        self.head = BranchHead(Some(id));
        id
    }

    /// 발언 존재 판정.
    pub fn contains(&self, id: MessageId) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    /// head를 id로 이동한다(존재할 때만). 이동 성공 시 true, 없는 id면 false(head 불변).
    pub fn checkout(&mut self, id: MessageId) -> bool {
        if self.contains(id) {
            self.head = BranchHead(Some(id));
            true
        } else {
            false
        }
    }

    /// 트리 요약 줄(/branches 표시용). store::tree_summary 포맷과 바이트 등가.
    pub fn tree_summary(&self) -> String {
        if self.nodes.is_empty() {
            return "(빈 트리)".to_string();
        }
        let mut out = String::new();
        for n in &self.nodes {
            let marker = if Some(n.id) == self.head.0 { "*" } else { " " };
            let parent = n
                .parent
                .map(|p| p.to_string())
                .unwrap_or_else(|| "-".into());
            let snippet: String = n.content.chars().take(30).collect();
            out.push_str(&format!(
                "{marker} #{} (<-{parent}) {}: {}\n",
                n.id, n.speaker, snippet
            ));
        }
        out
    }

    /// 활성 경로 길이(선형 사용 시 기존 transcript.len()과 동일).
    pub fn transcript_len(&self) -> usize {
        self.active_path().len()
    }

    /// 트리 전체 메시지 수(분기 포함).
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// 빈 트리 여부.
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: u64, parent: Option<u64>, content: &str) -> MessageNode {
        MessageNode {
            id,
            parent,
            speaker: format!("s{id}"),
            content: content.into(),
        }
    }

    #[test]
    fn active_path_walks_parents_like_path_to_root() {
        // store::path_to_root_walks_parents 미러: 1->2->3 + 2->4(분기).
        let snap = ConversationSnapshot::from_parts(
            vec![
                node(1, None, "1"),
                node(2, Some(1), "2"),
                node(3, Some(2), "3"),
                node(4, Some(2), "4"),
            ],
            BranchHead(Some(3)),
        );
        assert_eq!(
            snap.active_path()
                .iter()
                .map(|u| u.content.clone())
                .collect::<Vec<_>>(),
            vec!["1", "2", "3"]
        );
        // head=4면 분기 경로.
        let branch = ConversationSnapshot::from_parts(snap.nodes().to_vec(), BranchHead(Some(4)));
        assert_eq!(
            branch
                .active_path()
                .iter()
                .map(|u| u.content.clone())
                .collect::<Vec<_>>(),
            vec!["1", "2", "4"]
        );
        // head=None이면 빈 경로.
        assert!(ConversationSnapshot::new().active_path().is_empty());
    }

    #[test]
    fn active_path_cycle_guard_terminates() {
        // 순환(1.parent=2, 2.parent=1), head=1. HashSet 가드로 무한루프 없이 유계 반환(2개 이하).
        let snap = ConversationSnapshot::from_parts(
            vec![node(1, Some(2), "1"), node(2, Some(1), "2")],
            BranchHead(Some(1)),
        );
        assert!(snap.active_path().len() <= 2);
    }

    #[test]
    fn append_reproduces_next_id_parent_and_head_advance() {
        // store::next_id(max+1, 빈=1) + parent=head + head 전진 규칙 재현.
        let mut snap = ConversationSnapshot::new();
        assert!(snap.is_empty());
        let a = snap.append("claude", "제안");
        assert_eq!(a, 1); // 빈 → 1
        assert_eq!(snap.head().tip(), Some(1));
        let b = snap.append("codex", "리뷰");
        assert_eq!(b, 2); // max+1
        assert_eq!(snap.head().tip(), Some(2));
        // parent 체인: 2의 parent=1.
        assert_eq!(snap.nodes()[1].parent, Some(1));
        assert_eq!(snap.node_count(), 2);
        // active_path = 선형 2개.
        assert_eq!(snap.transcript_len(), 2);
    }

    #[test]
    fn checkout_moves_head_only_when_present() {
        let mut snap = ConversationSnapshot::new();
        snap.append("a", "1"); // id 1
        snap.append("b", "2"); // id 2, head=2
        // checkout 1 → head 이동(분기 준비).
        assert!(snap.checkout(1));
        assert_eq!(snap.head().tip(), Some(1));
        // 없는 id → false, head 불변.
        assert!(!snap.checkout(99));
        assert_eq!(snap.head().tip(), Some(1));
        // checkout 후 append = 분기(2의 sibling, parent=1).
        let c = snap.append("a", "3");
        assert_eq!(c, 3);
        assert_eq!(snap.nodes()[2].parent, Some(1));
        assert_eq!(snap.node_count(), 3); // 트리 3개
        assert_eq!(snap.transcript_len(), 2); // 활성 경로 1->3
    }

    #[test]
    fn tree_summary_matches_store_format() {
        // store::tree_summary_characterization_format와 동일 포맷(바이트 등가).
        let snap = ConversationSnapshot::from_parts(
            vec![
                MessageNode {
                    id: 1,
                    parent: None,
                    speaker: "a".into(),
                    content: "첫줄".into(),
                },
                MessageNode {
                    id: 2,
                    parent: Some(1),
                    speaker: "b".into(),
                    content: "둘째".into(),
                },
            ],
            BranchHead(Some(2)),
        );
        assert_eq!(
            snap.tree_summary(),
            "  #1 (<--) a: 첫줄\n* #2 (<-1) b: 둘째\n"
        );
        assert_eq!(ConversationSnapshot::new().tree_summary(), "(빈 트리)");
    }
}
