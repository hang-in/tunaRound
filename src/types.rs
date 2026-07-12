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
