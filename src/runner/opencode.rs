// opencode run --format json 엔진 러너(JSONL text/step_finish 파싱).

use super::exec::{run_with_watchdog, ExecSpec};
use super::{RunError, RunInput, RunOutput, Runner};
use std::time::Duration;

/// `opencode run --format json` JSONL에서 (본문, 토큰)을 추출한다.
/// type=="text" → part.text 누적, type=="step_finish" → part.tokens.input/output 누적.
/// 비-JSON 라인은 무시.
pub(crate) fn parse_opencode_stream(stdout: &str) -> RunOutput {
    let mut texts: Vec<String> = Vec::new();
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "text" => {
                if let Some(text) = event
                    .get("part")
                    .and_then(|p| p.get("text"))
                    .and_then(|v| v.as_str())
                    && !text.is_empty()
                {
                    texts.push(text.to_string());
                }
            }
            "step_finish" => {
                if let Some(tokens) = event.get("part").and_then(|p| p.get("tokens")) {
                    input_tokens += tokens.get("input").and_then(|v| v.as_i64()).unwrap_or(0);
                    output_tokens += tokens.get("output").and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    RunOutput {
        content: texts.join("").trim().to_string(),
        input_tokens,
        output_tokens,
    }
}

/// `opencode run` argv 조립.
/// 메시지는 positional arg로 전달(stdin 없음).
/// RunMode는 현재 무시 - opencode의 ReadOnly 강제 플래그 불명확(1차 모드무관).
pub(crate) fn build_opencode_args(input: &RunInput, model: Option<&str>) -> Vec<String> {
    let mut args: Vec<String> = vec!["run".into(), "--format".into(), "json".into()];
    if let Some(m) = model {
        args.push("--model".into());
        args.push(m.to_string());
    }
    args.push(input.prompt.clone());
    args
}

/// opencode CLI 러너. `bin`은 실행 파일(기본 "opencode"). 미설치 시 spawn 에러(graceful).
pub struct OpencodeRunner {
    bin: String,
    model: Option<String>,
    idle_timeout: Duration,
}

impl OpencodeRunner {
    pub fn new() -> Self {
        Self {
            bin: "opencode".to_string(),
            model: None,
            idle_timeout: Duration::from_secs(600),
        }
    }
    /// 테스트용 실행 파일 경로 주입.
    pub fn with_bin(bin: &str) -> Self {
        Self {
            bin: bin.to_string(),
            model: None,
            idle_timeout: Duration::from_secs(600),
        }
    }
    /// 모델 설정(provider/model 형식).
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }
    /// 테스트/설정용 idle 타임아웃 주입.
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = d;
        self
    }
}

impl Default for OpencodeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for OpencodeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_opencode_args(input, self.model.as_deref()),
            cwd: input.project_path.clone(),
            stdin: None,
            idle_timeout: self.idle_timeout,
            label: "opencode".to_string(),
            env: Vec::new(),
        };
        let stdout = run_with_watchdog(&spec)?;
        let out = parse_opencode_stream(&stdout);
        if out.content.is_empty() {
            return Err(RunError::Empty("opencode 응답 없음".into()));
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::{RunInput, RunMode};

    #[test]
    fn args_have_run_json_and_model() {
        let input = RunInput {
            prompt: "이 설계 어때?".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        let args = build_opencode_args(&input, Some("ollama-cloud/gemma3:4b"));
        let j = args.join(" ");
        assert!(j.contains("run") && j.contains("--format json"));
        assert!(j.contains("--model ollama-cloud/gemma3:4b"));
        assert!(args.last().map(|s| s.as_str()) == Some("이 설계 어때?"));
    }

    #[test]
    fn parse_extracts_text_and_tokens() {
        // 실측 JSONL 픽스처(3 이벤트).
        let out = concat!(
            r#"{"type":"step_start","part":{}}"#,
            "\n",
            r#"{"type":"text","part":{"type":"text","text":"안녕하세요."}}"#,
            "\n",
            r#"{"type":"step_finish","part":{"type":"step_finish","tokens":{"total":8694,"input":8690,"output":4}}}"#,
            "\n",
        );
        let r = parse_opencode_stream(out);
        assert_eq!(r.content, "안녕하세요.");
        assert_eq!(r.input_tokens, 8690);
        assert_eq!(r.output_tokens, 4);
    }
}
