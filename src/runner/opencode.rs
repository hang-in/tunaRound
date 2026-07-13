// opencode run --format json 엔진 러너(JSONL text/step_finish 파싱).

use super::exec::{ExecSpec, run_with_watchdog};
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
/// 잔여(리뷰 #18): Windows npm 셰임(.cmd) + 개행 포함 프롬프트는 spawn 실패 가능. claude와 달리
/// stdin 지원 미검증이라 argv 유지. opencode stdin 지원 확인되면 claude와 동형으로 stdin 전환 검토.
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

    // 무출력으로 sleep하는 가짜 실행파일을 tmp에 만들어 경로를 돌려준다(OS별, 형제 러너 claude/codex와 동형).
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
        // watchdog idle 타임아웃 실경로 커버(형제 러너와 동형): 무출력 sleep 바이너리 → Timeout.
        let bin = fake_sleep_bin("tuna_fake_sleep_opencode");
        let r = OpencodeRunner::with_bin(&bin).with_idle_timeout(Duration::from_millis(150));
        let input = RunInput {
            prompt: "x".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
}
