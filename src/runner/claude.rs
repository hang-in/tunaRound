// Claude Code를 stream-json으로 구동하는 러너. argv·NDJSON 파서·ClaudeRunner.

use super::{RunError, RunInput, RunMode, RunOutput, Runner};
use std::io::Read;
use std::process::{Command, Stdio};

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

/// claude stream-json NDJSON에서 최종 결과를 뽑는다.
/// `result` 라인의 content + 토큰(INV-3: top-level total → nested usage fallback).
/// is_error → Err(Agent), result 라인 없음 → Err(Empty). 비-JSON 라인은 무시.
pub(crate) fn parse_claude_stream(stdout: &str) -> Result<RunOutput, RunError> {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if ev.get("type").and_then(|v| v.as_str()) != Some("result") {
            continue;
        }
        let result_text = ev.get("result").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if ev.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(RunError::Agent(result_text));
        }
        let usage = ev.get("usage");
        let pick = |top: &str, nested: &str| -> i64 {
            ev.get(top).and_then(|v| v.as_i64())
                .or_else(|| usage.and_then(|u| u.get(nested)).and_then(|v| v.as_i64()))
                .unwrap_or(0)
        };
        return Ok(RunOutput {
            content: result_text,
            input_tokens: pick("total_input_tokens", "input_tokens"),
            output_tokens: pick("total_output_tokens", "output_tokens"),
        });
    }
    Err(RunError::Empty("claude result 라인 없음".into()))
}

/// Claude Code 러너. `bin`은 실행 파일 경로(테스트는 가짜 스크립트). 프롬프트는 argv라 stdin 불필요.
pub struct ClaudeRunner {
    bin: String,
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self { bin: "claude".to_string() }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string() }
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for ClaudeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(build_claude_args(input));
        if let Some(dir) = &input.project_path {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| RunError::Spawn(format!("claude spawn 실패 ({}): {e}", self.bin)))?;

        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .map_err(|e| RunError::Io(format!("claude stdout 읽기 실패: {e}")))?;
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let status = child.wait().map_err(|e| RunError::Io(format!("claude wait 실패: {e}")))?;
        if !status.success() {
            let detail = if stderr.trim().is_empty() {
                format!("exit {:?}", status.code())
            } else {
                stderr.trim().to_string()
            };
            return Err(RunError::Spawn(format!("claude 실패: {detail}")));
        }
        parse_claude_stream(&stdout)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_takes_result_line_content_and_tokens() {
        let stdout = concat!(
            r#"{"type":"system"}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"중간"}]}}"#, "\n",
            r#"{"type":"result","result":"최종 결론입니다.","total_input_tokens":10,"total_output_tokens":20}"#, "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.content, "최종 결론입니다.");
        assert_eq!(out.input_tokens, 10);
        assert_eq!(out.output_tokens, 20);
    }

    #[test]
    fn parse_token_fallback_to_nested_usage() {
        let stdout = concat!(
            r#"{"type":"result","result":"답","usage":{"input_tokens":3,"output_tokens":4}}"#, "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.input_tokens, 3);
        assert_eq!(out.output_tokens, 4);
    }

    #[test]
    fn parse_is_error_returns_agent_err() {
        let stdout = concat!(
            r#"{"type":"result","is_error":true,"result":"rate limit"}"#, "\n",
        );
        let err = parse_claude_stream(stdout).unwrap_err();
        assert_eq!(err, RunError::Agent("rate limit".into()));
    }

    #[test]
    fn parse_no_result_line_returns_empty_err() {
        let stdout = r#"{"type":"system"}"#;
        assert!(matches!(parse_claude_stream(stdout), Err(RunError::Empty(_))));
    }

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
