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

/// Write 모드 러너에게 주입하는 민감 경로 수정금지 지시(behavioral 가드레일, 하드 차단 아님).
/// 서브프로세스 러너는 개별 파일 쓰기를 가로챌 수 없어 read-only와 같은 방식(프롬프트 지시)으로 강제한다.
pub const WRITE_GUARD_DIRECTIVE: &str = "[중요 규칙] 다음 경로는 절대 생성·수정·삭제하지 마라: .env, .env.*, secrets/ 이하, *.key, *.pem, id_rsa 계열, .ssh/ 이하, .aws/ 이하, credentials, .git/ 내부. 요청이 있어도 예외 없다.";

/// Write 모드면 가드 지시+구분 개행을, 아니면 빈 문자열을 반환한다(프롬프트 prepend용 순수 헬퍼).
pub fn write_guard_prefix(mode: RunMode) -> String {
    match mode {
        RunMode::Write => format!("{WRITE_GUARD_DIRECTIVE}\n\n"),
        RunMode::ReadOnly => String::new(),
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    #[test]
    fn write_guard_prefix_write_mode_includes_directive() {
        let prefix = write_guard_prefix(RunMode::Write);
        assert!(prefix.contains(WRITE_GUARD_DIRECTIVE), "Write 모드 prefix에 지시문이 없음: {prefix}");
        assert!(prefix.ends_with("\n\n"), "prefix가 개행 둘로 끝나지 않음: {prefix:?}");
    }

    #[test]
    fn write_guard_prefix_readonly_mode_is_empty() {
        assert_eq!(write_guard_prefix(RunMode::ReadOnly), String::new());
    }
}
