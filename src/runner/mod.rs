// 에이전트 CLI를 한 턴 구동하고 결과를 돌려주는 러너 레이어의 경계.
#[cfg(feature = "a2a-out")]
pub mod a2a;
pub mod claude;
pub mod codex;
pub(crate) mod exec;
#[cfg(feature = "engines")]
pub mod http;
pub mod opencode;

/// 한 턴 입력. v1은 매 턴 전사를 prompt에 주입하므로 resume 토큰은 두지 않는다.
#[derive(Debug, Clone, Default)]
pub struct RunInput {
    pub prompt: String,
    pub model: Option<String>,
    pub project_path: Option<String>,
    pub mode: RunMode,
    /// 이 턴이 pull 모드(포인터 프롬프트 + 에이전트가 MCP로 전사 당김)인지.
    /// codex는 pull+ReadOnly일 때 샌드박스를 풀어 MCP 승인을 통과시키므로 러너가 알아야 한다.
    pub pull: bool,
}

/// 말하기 턴(읽기 전용) vs 사람이 지목한 쓰기 턴. 쓰기 하드 분리.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RunMode {
    #[default]
    ReadOnly,
    Write,
}

/// 한 턴 출력.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutput {
    pub content: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Spawn(String),
    Io(String),
    Empty(String),
    Agent(String),
    Timeout(String),
}

/// 엔진 경계. 오케스트레이터는 concrete 엔진이 아니라 이 trait에 의존한다.
pub trait Runner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError>;
}
