// Codex exec --json argv·파싱·dedup 순수함수 + CodexRunner.

use super::exec::{run_with_watchdog, ExecSpec};
use super::{RunError, RunInput, RunMode, RunOutput, Runner};
use std::time::Duration;

/// Codex `exec --json` JSONL에서 (본문, 토큰)을 추출한다.
/// item.completed+agent_message → 본문(dedup), turn.completed → 토큰 누적,
/// 비-JSON 라인은 plain text fallback. 그 외 이벤트는 무시.
pub(crate) fn parse_codex_stream(stdout: &str) -> RunOutput {
    let mut texts: Vec<String> = Vec::new();
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            push_agent_text_dedup(&mut texts, line);
            continue;
        };
        match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "item.completed" => {
                if let Some(item) = event.get("item")
                    && item.get("type").and_then(|v| v.as_str()) == Some("agent_message")
                    && let Some(text) = item.get("text").and_then(|v| v.as_str())
                    && !text.is_empty()
                {
                    push_agent_text_dedup(&mut texts, text);
                }
            }
            "turn.completed" => {
                if let Some(usage) = event.get("usage") {
                    input_tokens += usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    output_tokens += usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    RunOutput {
        content: texts.join("\n\n").trim().to_string(),
        input_tokens,
        output_tokens,
    }
}

/// `codex exec` argv 조립. 모드에 따라 샌드박스 권한을 분리한다(쓰기 하드 분리).
/// 프롬프트는 stdin(`-`)으로 전달하므로 argv에 넣지 않는다.
/// mcp_args는 이미 조립된 `-c key=val` 쌍들(보통 4개). 빈 슬라이스면 기존과 동일.
/// Step 1 실측(2026-06-29): codex --full-auto 없음.
///   Write  → --sandbox workspace-write
///   ReadOnly → --sandbox read-only
fn build_codex_args(input: &RunInput, mcp_args: &[String]) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--color=never".into(),
    ];
    match input.mode {
        RunMode::Write => {
            args.push("--sandbox".into());
            args.push("workspace-write".into());
        }
        RunMode::ReadOnly => {
            args.push("--sandbox".into());
            args.push("read-only".into());
        }
    }
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args.push("-".into());
    args.extend_from_slice(mcp_args);
    args
}

/// Codex CLI 러너. `bin`은 실행 파일 경로(테스트는 가짜 스크립트 주입).
pub struct CodexRunner {
    bin: String,
    idle_timeout: Duration,
    /// 검색 MCP 서버에 넘길 DB 경로. Some이면 run() 시 -c mcp_servers 오버라이드를 조립해 전달한다.
    search_db: Option<String>,
}

impl CodexRunner {
    pub fn new() -> Self {
        Self { bin: "codex".to_string(), idle_timeout: Duration::from_secs(600), search_db: None }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string(), idle_timeout: Duration::from_secs(600), search_db: None }
    }
    /// 테스트/설정용 idle 타임아웃 주입.
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = d;
        self
    }
    /// 검색 MCP 서버 DB 경로 주입. Some이면 codex에 -c mcp_servers.tuna-search로 self-exe를 spawn하도록 배선한다.
    pub fn with_search_db(mut self, db: Option<String>) -> Self {
        self.search_db = db;
        self
    }
}

impl Default for CodexRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for CodexRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        // search_db가 Some이면 -c mcp_servers.tuna-search 오버라이드 쌍을 조립한다.
        // TOML 리터럴 문자열(작은따옴표)로 Windows 백슬래시 경로 안전.
        // 한계: exe/db 경로에 작은따옴표가 포함된 경우 TOML 파싱 오류 가능(일반 Windows 경로 OK).
        let mcp_args: Vec<String> = self.search_db.as_ref().map(|db| {
            let exe = std::env::current_exe()
                .ok()
                .and_then(|p| p.to_str().map(String::from))
                .unwrap_or_else(|| "tunaround".into());
            vec![
                "-c".into(),
                format!("mcp_servers.tuna-search.command='{exe}'"),
                "-c".into(),
                format!("mcp_servers.tuna-search.args=['--mcp-search','--db','{db}']"),
            ]
        }).unwrap_or_default();
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_codex_args(input, &mcp_args),
            cwd: input.project_path.clone(),
            stdin: Some(input.prompt.clone()),
            idle_timeout: self.idle_timeout,
            label: "codex".to_string(),
        };
        let stdout = run_with_watchdog(&spec)?;
        let out = parse_codex_stream(&stdout);
        if out.content.is_empty() {
            return Err(RunError::Empty("codex 응답 없음".into()));
        }
        Ok(out)
    }
}

/// Codex는 한 턴에 agent_message를 여러 번 emit한다(reasoning 후 재방출).
/// 정확 중복은 skip, prefix 확장이면 교체, 긴(>=40) 직전이 포함되면 교체, 그 외 append.
fn push_agent_text_dedup(texts: &mut Vec<String>, incoming: &str) {
    let trimmed = incoming.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(last) = texts.last() {
        let last_tr = last.trim().to_string();
        if last_tr == trimmed {
            return;
        }
        if trimmed.starts_with(&last_tr) && trimmed.len() > last_tr.len() {
            *texts.last_mut().unwrap() = incoming.to_string();
            return;
        }
        if last_tr.len() >= 40 && trimmed.contains(&last_tr) {
            *texts.last_mut().unwrap() = incoming.to_string();
            return;
        }
    }
    texts.push(incoming.to_string());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_skips_exact_duplicate() {
        let mut v = vec!["hello".to_string()];
        push_agent_text_dedup(&mut v, "hello");
        assert_eq!(v, vec!["hello"]);
    }

    #[test]
    fn dedup_replaces_when_incoming_extends_prefix() {
        let mut v = vec!["hello".to_string()];
        push_agent_text_dedup(&mut v, "hello world");
        assert_eq!(v, vec!["hello world"]);
    }

    #[test]
    fn dedup_replaces_when_long_last_is_contained() {
        let long = "x".repeat(40);
        let mut v = vec![long.clone()];
        push_agent_text_dedup(&mut v, &format!("prefix {long}"));
        assert_eq!(v, vec![format!("prefix {long}")]);
    }

    #[test]
    fn dedup_appends_distinct() {
        let mut v = vec!["a".to_string()];
        push_agent_text_dedup(&mut v, "b");
        assert_eq!(v, vec!["a", "b"]);
    }

    #[test]
    fn parse_extracts_agent_message_and_tokens() {
        let stdout = concat!(
            r#"{"type":"thread.started"}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"설계 의견입니다."}}"#, "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":34}}"#, "\n",
        );
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "설계 의견입니다.");
        assert_eq!(out.input_tokens, 12);
        assert_eq!(out.output_tokens, 34);
    }

    #[test]
    fn parse_falls_back_on_non_json_line() {
        let stdout = "그냥 텍스트 한 줄\n";
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "그냥 텍스트 한 줄");
    }

    #[test]
    fn parse_dedups_repeated_agent_message() {
        let stdout = concat!(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"답"}}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"답 확장"}}"#, "\n",
        );
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "답 확장");
    }

    #[test]
    fn args_write_mode_uses_workspace_write() {
        let input = RunInput {
            prompt: "p".into(),
            model: None,
            project_path: None,
            mode: RunMode::Write,
        };
        let args = build_codex_args(&input, &[]);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox workspace-write"));
        assert_eq!(args.last().unwrap(), "-"); // prompt via stdin
    }

    #[test]
    fn args_readonly_mode_uses_sandbox_readonly() {
        let input = RunInput {
            prompt: "p".into(),
            model: Some("gpt-x".into()),
            project_path: None,
            mode: RunMode::ReadOnly,
        };
        let args = build_codex_args(&input, &[]);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-x"));
    }

    #[test]
    fn args_with_mcp_args_appends_c_flags() {
        let input = RunInput {
            prompt: "q".into(),
            model: None,
            project_path: None,
            mode: RunMode::ReadOnly,
        };
        let mcp_args = vec![
            "-c".to_string(),
            "mcp_servers.tuna-search.command='/usr/bin/tunaround'".to_string(),
            "-c".to_string(),
            "mcp_servers.tuna-search.args=['--mcp-search','--db','/tmp/t.db']".to_string(),
        ];
        let args = build_codex_args(&input, &mcp_args);
        let joined = args.join(" ");
        assert!(joined.contains("-c"), "-c 플래그 없음: {joined}");
        assert!(joined.contains("mcp_servers.tuna-search"), "서버 이름 없음: {joined}");
        // 빈 슬라이스면 -c 없음.
        let args_none = build_codex_args(&input, &[]);
        assert!(!args_none.join(" ").contains("mcp_servers.tuna-search"), "mcp_args 없는데 포함됨");
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
        let bin = fake_sleep_bin("tuna_fake_sleep_codex");
        let r =
            CodexRunner::with_bin(&bin).with_idle_timeout(std::time::Duration::from_millis(150));
        let input = RunInput { prompt: "x".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
}
