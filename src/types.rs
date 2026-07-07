// 계층 경계를 넘나드는 공용 도메인 값 타입. store(영속)와 orchestrator(정책)가 모두 참조하되 서로를
// import하지 않도록, 어느 한쪽이 아니라 중립 위치에 둔다(store->orchestrator 역방향 결합 제거).

/// 한 발언. speaker=Participant.label(), content=응답 본문.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: String,
    pub content: String,
}
