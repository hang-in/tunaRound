// 사람 주도 토론 오케스트레이터의 경계. 역할·프롬프트·라운드 구동.
pub mod roles;
pub mod prompt;

use std::collections::HashMap;

use crate::orchestrator::prompt::{build_round_prompt, PromptContext};
use crate::runner::{RunError, RunInput, RunMode, Runner};

/// 컨텍스트 전달 방식. Push(기본)=지금처럼 프롬프트에 직접 주입, Pull=포인터만 주고 에이전트가 도구로 당겨옴.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub enum ContextMode {
    #[default]
    Push,
    Pull,
}

/// pull(전사 당기기)이 실제로 검증된 좌석 판별. 현재는 claude뿐이다.
/// codex exec는 MCP 도구 호출을 승인 모델이 막아(헤드리스 "사용자 취소") read_transcript가
/// 안 되므로 pull 대상에서 빼고 push로 폴백한다. codex pull 활성화는 후속(승인 설정 조사).
pub fn is_mcp_capable(engine: &str) -> bool {
    matches!(engine, "claude")
}

/// 토론 한 자리. 엔진(어떤 러너로 구동) + 역할 + 추가 지시.
#[derive(Debug, Clone)]
pub struct Participant {
    pub engine: String,
    pub role: Option<String>,
    pub instruction: String,
}

impl Participant {
    /// 전사 표기용 라벨(역할 있으면 "engine/role", 없으면 engine).
    pub fn label(&self) -> String {
        match &self.role {
            Some(r) => format!("{}/{}", self.engine, r),
            None => self.engine.clone(),
        }
    }
}

/// 한 발언. speaker=Participant.label(), content=응답 본문.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: String,
    pub content: String,
}

/// 엔진 이름 → 러너 조회 경계. 오케스트레이터는 이 trait에만 의존한다.
pub trait RunnerRegistry {
    fn get(&self, engine: &str) -> Option<&dyn Runner>;
}

/// topic으로 관련 과거 맥락 슬라이스를 끌어오는 경계(RunnerRegistry와 동형, 비게이트).
pub trait ContextRetriever: Send + Sync {
    fn retrieve(&self, query: &str, limit: usize) -> Vec<Utterance>;
}

/// 세션 전사를 읽어 오는 추상(코어를 백엔드로 노출하는 오케스트레이션 primitive).
pub trait TranscriptReader: Send + Sync {
    /// session_id의 활성 경로(root->head) 발언. max_turns=Some(n)이면 마지막 n턴만.
    fn read_transcript(&self, session_id: &str, max_turns: Option<usize>) -> Vec<Utterance>;
}

/// 세션 전사 끝(head 자식)에 발언을 추가하는 경계(원격 post_turn·front=core 병합용, Plan 27).
/// 구현은 DB를 id 권위로 삼아 증분 추가하므로 REPL과 외부 writer가 충돌 없이 공존한다.
pub trait TranscriptWriter: Send + Sync {
    /// session_id에 발언을 추가하고 새 msg_id를 반환한다.
    fn append_turn(&self, session_id: &str, speaker: &str, content: &str) -> Result<u64, String>;
}

/// 발언의 유효성 상태를 기록하는 경계(사람이 /supersede·/reject로 지정, step 5 HITL).
/// 미배선(--db 없음)이면 REPL이 안내만 한다.
pub trait ValiditySink: Send + Sync {
    fn set_validity(
        &self,
        session_id: &str,
        msg_id: u64,
        valid_state: &str,
        superseded_by: Option<u64>,
    ) -> Result<(), String>;
}

/// 로스터 좌석 요약(get_roster MCP 노출용). 원격 참가자가 토론 좌석 구성을 발견한다.
#[derive(Debug, Clone, PartialEq)]
pub struct RosterSeat {
    pub engine: String,
    pub role: Option<String>,
}

/// front=core 병합용: REPL이 코어 DB를 권위로 삼아 로드/추가하는 경계(Plan 27 옵션 B).
/// load_session은 외부 post_turn까지 포함한 최신 트리를, append_turn은 DB id 권위로 발언을 추가한다.
/// 이 경계가 연결되면 REPL은 매 라운드 DB를 adopt해 외부 쓰기와 충돌·클로버 없이 공존한다.
pub trait CoreSync: Send + Sync {
    fn load_session(&self, session_id: &str) -> Option<crate::store::StoredSession>;
    fn append_turn(&self, session_id: &str, speaker: &str, content: &str) -> Result<u64, String>;
}

/// HashMap 기반 기본 레지스트리. 테스트는 FakeRunner를 넣는다.
pub struct MapRegistry {
    runners: HashMap<String, Box<dyn Runner>>,
}

impl MapRegistry {
    pub fn new() -> Self {
        Self { runners: HashMap::new() }
    }
    pub fn insert(&mut self, engine: &str, runner: Box<dyn Runner>) {
        self.runners.insert(engine.to_string(), runner);
    }
}

impl Default for MapRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl RunnerRegistry for MapRegistry {
    fn get(&self, engine: &str) -> Option<&dyn Runner> {
        self.runners.get(engine).map(|b| b.as_ref())
    }
}

/// run_round에 넘기는 라운드 맥락 인자 묶음. 파라미터 폭발 방지용.
pub struct RoundInput<'a> {
    /// 이전 라운드들의 발언 슬라이스(호출자가 recent_turns 적용 후 제공).
    pub prior: &'a [Utterance],
    /// 검색으로 끌어온 과거 맥락 슬라이스(비면 무영향, 동작 불변).
    pub retrieved: &'a [Utterance],
    /// 드롭된 옛 턴의 압축 요약(비면 이월 섹션 없음, behavior-preserving).
    pub carried: &'a str,
    /// 컨텍스트 전달 모드. Pull이고 MCP 가능 좌석이면 포인터 프롬프트로 대체.
    pub ctx_mode: ContextMode,
    /// 활성 전사 전체 발언 수(포인터 힌트용).
    pub transcript_len: usize,
}

/// 한 라운드를 구동한다. 사람 주도이므로 topic = 사용자 메시지.
/// 각 자리를 순서대로 호출하되, 뒤 자리는 같은 라운드 앞 응답을 본다(순차-인지).
/// mode는 호출자가 지정(말하기=ReadOnly, 사람이 지목한 쓰기 턴=Write).
/// 반환값은 이번 라운드 발언 목록. 트리 append는 호출자(Session::append_round)가 담당.
pub fn run_round(
    participants: &[Participant],
    topic: &str,
    registry: &dyn RunnerRegistry,
    mode: RunMode,
    input: RoundInput<'_>,
) -> Result<Vec<Utterance>, RunError> {
    let mut same_round: Vec<Utterance> = Vec::new();

    for part in participants {
        // Pull 모드이고 MCP 도구 보유 좌석이면 포인터 프롬프트, 아니면 Push(기존 동일).
        let pull = input.ctx_mode == ContextMode::Pull && is_mcp_capable(&part.engine);
        let prompt = build_round_prompt(part, topic, PromptContext {
            prior: input.prior,
            same_round: &same_round,
            retrieved: input.retrieved,
            carried: input.carried,
            pull,
            transcript_len: input.transcript_len,
        });
        eprintln!("[ctx] seat={} mode={} prompt_chars={}", part.engine, if pull { "pull" } else { "push" }, prompt.chars().count());
        let runner = registry
            .get(&part.engine)
            .ok_or_else(|| RunError::Spawn(format!("엔진 러너 없음: {}", part.engine)))?;
        let run_input = RunInput {
            prompt,
            model: None,
            project_path: None,
            mode,
        };
        let out = runner.run(&run_input)?;
        same_round.push(Utterance { speaker: part.label(), content: out.content });
    }

    Ok(same_round)
}

#[cfg(test)]
mod tests {
    use super::is_mcp_capable;

    #[test]
    fn pull_capable_is_claude_only() {
        // pull은 claude만 검증됨. codex는 승인 블록으로 push 폴백, 그 외도 push.
        assert!(is_mcp_capable("claude"));
        assert!(!is_mcp_capable("codex"));
        assert!(!is_mcp_capable("ollama"));
        assert!(!is_mcp_capable("opencode"));
    }
}
