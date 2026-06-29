// Claude Code를 stream-json으로 구동하는 러너. argv·NDJSON 파서·ClaudeRunner.

use super::{RunInput, RunMode};

/// `claude -p` argv 조립. 프롬프트는 `-p <arg>`로 전달(stdin 아님).
/// 모드에 따라 도구 권한을 분리한다(쓰기 하드 분리). 실측 플래그는 Step 1 참조.
/// Step 1 실측(2026-06-29): claude --help 확인.
///   Write    → --dangerously-skip-permissions (모든 권한 우회)
///   ReadOnly → --disallowedTools Write,Edit,Bash (쓰기 도구 차단)
fn build_claude_args(input: &RunInput) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        input.prompt.clone(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    match input.mode {
        RunMode::Write => args.push("--dangerously-skip-permissions".into()),
        RunMode::ReadOnly => {
            args.push("--disallowedTools".into());
            args.push("Write,Edit,Bash".into());
        }
    }
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn args_have_stream_json_and_prompt() {
        let input = RunInput { prompt: "이 설계 어떤가요?".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
        let args = build_claude_args(&input);
        let joined = args.join(" ");
        assert!(joined.contains("-p 이 설계 어떤가요?"));
        assert!(joined.contains("--output-format stream-json"));
    }

    #[test]
    fn args_write_mode_skips_permissions() {
        let input = RunInput { prompt: "p".into(), model: Some("claude-x".into()), project_path: None, mode: RunMode::Write };
        let joined = build_claude_args(&input).join(" ");
        assert!(joined.contains("--dangerously-skip-permissions"));
        assert!(joined.contains("--model claude-x"));
    }
}
