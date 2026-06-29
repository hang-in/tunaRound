// 에이전트 CLI를 한 턴 구동하고 결과를 돌려주는 러너 레이어의 경계.
pub mod claude;
pub mod codex;
pub(crate) mod exec;

/// 한 턴 입력. v1은 매 턴 전사를 prompt에 주입하므로 resume 토큰은 두지 않는다.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub prompt: String,
    pub model: Option<String>,
    pub project_path: Option<String>,
    pub mode: RunMode,
}

/// 말하기 턴(읽기 전용) vs 사람이 지목한 쓰기 턴. 쓰기 하드 분리.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
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
