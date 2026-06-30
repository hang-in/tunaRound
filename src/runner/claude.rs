// Claude Code를 stream-json으로 구동하는 러너. argv·NDJSON 파서·ClaudeRunner.

use super::exec::{run_with_watchdog, ExecSpec};
use super::{RunError, RunInput, RunMode, RunOutput, Runner};
use std::time::Duration;

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
    idle_timeout: Duration,
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self { bin: "claude".to_string(), idle_timeout: Duration::from_secs(600) }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string(), idle_timeout: Duration::from_secs(600) }
    }
    /// 테스트/설정용 idle 타임아웃 주입.
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = d;
        self
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for ClaudeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_claude_args(input),
            cwd: input.project_path.clone(),
            stdin: None,
            idle_timeout: self.idle_timeout,
            label: "claude".to_string(),
        };
        let stdout = run_with_watchdog(&spec)?;
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

    // 무출력으로 sleep하는 가짜 실행파일을 tmp에 만들어 경로를 돌려준다(OS별).
    #[cfg(unix)]
    fn fake_sleep_bin(name: &str) -> String {
        let p = std::env::temp_dir().join(format!("{name}.sh"));
        std::fs::write(&p, "#!/bin/sh\nexec sleep 5\n").unwrap();
        let _ = std::process::Command::new("chmod")
            .args(["+x", p.to_str().unwrap()])
            .status();
        p.to_str().unwrap().to_string()
    }
    #[cfg(windows)]
    fn fake_sleep_bin(name: &str) -> String {
        // .cmd는 Command가 cmd.exe로 래핑 실행한다(rustc>=1.77.2). ping으로 무출력 sleep.
        let p = std::env::temp_dir().join(format!("{name}.cmd"));
        std::fs::write(&p, "@ping -n 6 127.0.0.1 > nul\r\n").unwrap();
        p.to_str().unwrap().to_string()
    }

    #[test]
    fn runner_propagates_timeout_via_helper() {
        let bin = fake_sleep_bin("tuna_fake_sleep_claude");
        let r =
            ClaudeRunner::with_bin(&bin).with_idle_timeout(std::time::Duration::from_millis(150));
        let input = RunInput { prompt: "x".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
}
