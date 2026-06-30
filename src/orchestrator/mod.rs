// 사람 주도 토론 오케스트레이터의 경계. 역할·프롬프트·라운드 구동.
pub mod roles;
pub mod prompt;

use std::collections::HashMap;

use crate::orchestrator::prompt::build_round_prompt;
use crate::runner::{RunError, RunInput, RunMode, Runner};

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

/// 한 라운드를 구동한다. 사람 주도이므로 topic = 사용자 메시지.
/// 각 자리를 순서대로 호출하되, 뒤 자리는 같은 라운드 앞 응답을 본다(순차-인지).
/// transcript는 이전 라운드들이며, 이번 라운드 응답이 끝에 append된다.
/// mode는 호출자가 지정(말하기=ReadOnly, 사람이 지목한 쓰기 턴=Write).
/// retrieved는 검색으로 끌어온 과거 맥락 슬라이스(비면 무영향, 동작 불변).
/// carried는 드롭된 옛 턴의 압축 요약(비면 이월 섹션 없음, behavior-preserving).
pub fn run_round(
    participants: &[Participant],
    transcript: &mut Vec<Utterance>,
    topic: &str,
    registry: &dyn RunnerRegistry,
    mode: RunMode,
    retrieved: &[Utterance],
    carried: &str,
) -> Result<Vec<Utterance>, RunError> {
    let prior: Vec<Utterance> = transcript.clone();
    let mut same_round: Vec<Utterance> = Vec::new();

    for part in participants {
        let prompt = build_round_prompt(part, topic, &prior, &same_round, retrieved, carried);
        let runner = registry
            .get(&part.engine)
            .ok_or_else(|| RunError::Spawn(format!("엔진 러너 없음: {}", part.engine)))?;
        let input = RunInput {
            prompt,
            model: None,
            project_path: None,
            mode,
        };
        let out = runner.run(&input)?;
        same_round.push(Utterance { speaker: part.label(), content: out.content });
    }

    transcript.extend(same_round.iter().cloned());
    Ok(same_round)
}
