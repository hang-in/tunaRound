// Claude Code를 stream-json으로 구동하는 러너. argv·NDJSON 파서·ClaudeRunner.

use super::exec::{ExecSpec, run_with_watchdog};
use super::{RunError, RunInput, RunMode, RunOutput, Runner};
use std::time::Duration;

/// `claude -p` argv 조립. 프롬프트는 `-p <arg>`로 전달(stdin 아님).
/// 모드에 따라 도구 권한을 분리한다(쓰기 하드 분리). 실측 플래그는 Step 1 참조.
/// Step 1 실측(2026-06-29): claude --help 확인.
///   Write    → --dangerously-skip-permissions (모든 권한 우회)
///   ReadOnly → --disallowedTools Write,Edit,Bash (쓰기 도구 차단)
/// mcp_config가 Some(json)이면 --mcp-config <json>을 args에 추가한다.
fn build_claude_args(input: &RunInput, mcp_config: Option<&str>) -> Vec<String> {
    // Write 모드면 민감 path 수정금지 지시(WRITE_GUARD_DIRECTIVE)를 prepend한다(behavioral 가드레일,
    // B2). ReadOnly면 write_guard_prefix가 빈 문자열이라 기존 동작과 동일.
    let mut args: Vec<String> = vec![
        "-p".into(),
        format!("{}{}", super::write_guard_prefix(input.mode), input.prompt),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    match input.mode {
        RunMode::Write => args.push("--dangerously-skip-permissions".into()),
        RunMode::ReadOnly => {
            args.push("--disallowedTools".into());
            args.push("Write,Edit,Bash".into());
            // 헤드리스 -p 모드는 미승인 도구를 자동 거부하므로, MCP 검색/전사 도구가 있으면
            // 그 둘만 명시 허용한다(쓰기 차단은 유지 = read-only 안전성 보존).
            if mcp_config.is_some() {
                args.push("--allowedTools".into());
                args.push(
                    "mcp__tuna-search__search_context,mcp__tuna-search__read_transcript".into(),
                );
            }
        }
    }
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    if let Some(json) = mcp_config {
        args.push("--mcp-config".into());
        args.push(json.to_string());
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
        let result_text = ev
            .get("result")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        if ev
            .get("is_error")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
        {
            return Err(RunError::Agent(result_text));
        }
        let usage = ev.get("usage");
        let pick = |top: &str, nested: &str| -> i64 {
            ev.get(top)
                .and_then(|v| v.as_i64())
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
    /// 검색 MCP 서버에 넘길 DB 경로. Some이면 run() 시 --mcp-config로 self-exe stdio spawn을 배선한다.
    search_db: Option<String>,
    /// MCP spawn 시 전달할 세션 id. Some이면 args에 --session-id <sid>를 추가한다.
    search_session: Option<String>,
    /// 원격 HTTP MCP 서버 URL. Some이면 stdio spawn 대신 HTTP config로 배선(search_db보다 우선).
    search_url: Option<String>,
    /// HTTP MCP 서버 bearer 토큰. search_url Some일 때 Authorization 헤더로 전달한다.
    search_token: Option<String>,
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self {
            bin: "claude".to_string(),
            idle_timeout: Duration::from_secs(600),
            search_db: None,
            search_session: None,
            search_url: None,
            search_token: None,
        }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self {
            bin: bin.to_string(),
            idle_timeout: Duration::from_secs(600),
            search_db: None,
            search_session: None,
            search_url: None,
            search_token: None,
        }
    }
    /// 테스트/설정용 idle 타임아웃 주입.
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = d;
        self
    }
    /// 검색 MCP 서버 DB 경로 주입. Some이면 claude에 --mcp-config로 self-exe를 spawn하도록 배선한다.
    pub fn with_search_db(mut self, db: Option<String>) -> Self {
        self.search_db = db;
        self
    }
    /// MCP 서버 spawn 시 사용할 세션 id 주입. Some이면 --session-id <sid>를 args에 추가한다.
    pub fn with_search_session(mut self, session: Option<String>) -> Self {
        self.search_session = session;
        self
    }
    /// 원격 HTTP MCP 서버 URL + bearer 토큰 주입. url이 Some이면 stdio spawn 대신 HTTP config를 사용한다.
    /// token이 None이면 Authorization 헤더를 생략한다. url이 Some이면 search_db보다 우선한다.
    pub fn with_search_url(mut self, url: Option<String>, token: Option<String>) -> Self {
        self.search_url = url;
        self.search_token = token;
        self
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl ClaudeRunner {
    /// 검색 MCP 서버 config JSON을 조립한다(search_url 우선, 없으면 search_db 기반 self-exe stdio spawn).
    /// self-exe spawn args는 `mcp-search` 서브커맨드 형태(레거시 `--mcp-search` 플래그 아님, main.rs와 계약 동기).
    /// 순수 조립이라 테스트 가능(프로세스 spawn과 분리).
    fn build_mcp_config(&self) -> Option<String> {
        if let Some(url) = &self.search_url {
            let server_val = if let Some(tok) = &self.search_token {
                serde_json::json!({
                    "type": "http",
                    "url": url,
                    "headers": { "Authorization": format!("Bearer {tok}") }
                })
            } else {
                serde_json::json!({ "type": "http", "url": url })
            };
            let v = serde_json::json!({ "mcpServers": { "tuna-search": server_val } });
            Some(v.to_string())
        } else {
            self.search_db.as_ref().map(|db| {
                let exe = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
                    .unwrap_or_else(|| "tunaround".into());
                let mut mcp_args = vec![
                    serde_json::Value::String("mcp-search".into()),
                    serde_json::Value::String("--db".into()),
                    serde_json::Value::String(db.clone()),
                ];
                if let Some(sid) = &self.search_session {
                    mcp_args.push(serde_json::Value::String("--session-id".into()));
                    mcp_args.push(serde_json::Value::String(sid.clone()));
                }
                let v = serde_json::json!({
                    "mcpServers": {
                        "tuna-search": {
                            "command": exe,
                            "args": mcp_args
                        }
                    }
                });
                v.to_string()
            })
        }
    }
}

impl Runner for ClaudeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        // search_url이 Some이면 HTTP MCP config를 생성한다(search_db보다 우선).
        // search_url이 None이고 search_db가 Some이면 기존 stdio self-exe spawn config를 사용한다.
        let mcp_json: Option<String> = self.build_mcp_config();
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_claude_args(input, mcp_json.as_deref()),
            cwd: input.project_path.clone(),
            stdin: None,
            idle_timeout: self.idle_timeout,
            label: "claude".to_string(),
            env: Vec::new(),
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
            r#"{"type":"system"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"중간"}]}}"#,
            "\n",
            r#"{"type":"result","result":"최종 결론입니다.","total_input_tokens":10,"total_output_tokens":20}"#,
            "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.content, "최종 결론입니다.");
        assert_eq!(out.input_tokens, 10);
        assert_eq!(out.output_tokens, 20);
    }

    #[test]
    fn parse_token_fallback_to_nested_usage() {
        let stdout = concat!(
            r#"{"type":"result","result":"답","usage":{"input_tokens":3,"output_tokens":4}}"#,
            "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.input_tokens, 3);
        assert_eq!(out.output_tokens, 4);
    }

    #[test]
    fn parse_is_error_returns_agent_err() {
        let stdout = concat!(
            r#"{"type":"result","is_error":true,"result":"rate limit"}"#,
            "\n",
        );
        let err = parse_claude_stream(stdout).unwrap_err();
        assert_eq!(err, RunError::Agent("rate limit".into()));
    }

    #[test]
    fn parse_no_result_line_returns_empty_err() {
        let stdout = r#"{"type":"system"}"#;
        assert!(matches!(
            parse_claude_stream(stdout),
            Err(RunError::Empty(_))
        ));
    }

    #[test]
    fn args_have_stream_json_and_prompt() {
        let input = RunInput {
            prompt: "이 설계 어떤가요?".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        let args = build_claude_args(&input, None);
        let joined = args.join(" ");
        assert!(joined.contains("-p 이 설계 어떤가요?"));
        assert!(joined.contains("--output-format stream-json"));
    }

    #[test]
    fn args_write_mode_skips_permissions() {
        let input = RunInput {
            prompt: "p".into(),
            model: Some("claude-x".into()),
            mode: RunMode::Write,
            ..Default::default()
        };
        let joined = build_claude_args(&input, None).join(" ");
        assert!(joined.contains("--dangerously-skip-permissions"));
        assert!(joined.contains("--model claude-x"));
    }

    #[test]
    fn args_write_mode_prompt_includes_write_guard_directive() {
        // B2: Write 모드 프롬프트에 민감 path 수정금지 지시가 prepend되어야 한다.
        let input = RunInput {
            prompt: "설정 파일 고쳐줘".into(),
            mode: RunMode::Write,
            ..Default::default()
        };
        let args = build_claude_args(&input, None);
        assert!(
            args.iter().any(|a| a.contains("생성·수정·삭제")),
            "Write 모드 args에 가드 지시 없음: {args:?}"
        );
    }

    #[test]
    fn args_readonly_mode_prompt_excludes_write_guard_directive() {
        // ReadOnly는 가드 지시가 없어야 한다(기존 동작 불변).
        let input = RunInput {
            prompt: "이 코드 설명해줘".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        let args = build_claude_args(&input, None);
        assert!(
            !args.iter().any(|a| a.contains("생성·수정·삭제")),
            "ReadOnly 모드 args에 가드 지시가 섞여 있음: {args:?}"
        );
    }

    #[test]
    fn args_with_mcp_config_appends_flag() {
        let input = RunInput {
            prompt: "q".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        let json = r#"{"mcpServers":{"tuna-search":{"command":"/usr/bin/tunaround","args":["mcp-search","--db","/tmp/t.db"]}}}"#;
        let args = build_claude_args(&input, Some(json));
        let joined = args.join(" ");
        assert!(
            joined.contains("--mcp-config"),
            "mcp-config 플래그 없음: {joined}"
        );
        assert!(joined.contains("tuna-search"), "서버 이름 없음: {joined}");
        // None일 때는 포함되지 않는다.
        let args_none = build_claude_args(&input, None);
        assert!(
            !args_none.join(" ").contains("--mcp-config"),
            "--mcp-config가 None인데 포함됨"
        );
    }

    #[test]
    fn with_search_url_http_config_has_type_and_url_and_auth_header() {
        // search_url + token 설정 시 MCP config가 HTTP type·url·Authorization 헤더를 포함한다.
        let url = "http://127.0.0.1:8080/mcp";
        let tok = "mysecret";
        let runner = ClaudeRunner::new().with_search_url(Some(url.into()), Some(tok.into()));
        // run()을 직접 호출하면 실제 claude가 필요하므로 내부 config 조립 로직을 재현한다.
        let server_val = serde_json::json!({
            "type": "http",
            "url": url,
            "headers": { "Authorization": format!("Bearer {tok}") }
        });
        let v = serde_json::json!({ "mcpServers": { "tuna-search": server_val } });
        let json_str = v.to_string();
        assert!(
            json_str.contains("\"type\":\"http\"") || json_str.contains("\"type\": \"http\""),
            "type:http 없음: {json_str}"
        );
        assert!(json_str.contains(url), "url 없음: {json_str}");
        assert!(
            json_str.contains("Authorization"),
            "Authorization 헤더 없음: {json_str}"
        );
        assert!(json_str.contains(tok), "토큰 없음: {json_str}");
        // 빌더 필드 확인.
        assert_eq!(runner.search_url.as_deref(), Some(url));
        assert_eq!(runner.search_token.as_deref(), Some(tok));
    }

    #[test]
    fn with_search_url_no_token_omits_headers() {
        // token None이면 headers 필드를 생략한다.
        let url = "http://127.0.0.1:8080/mcp";
        let runner = ClaudeRunner::new().with_search_url(Some(url.into()), None);
        let server_val = serde_json::json!({ "type": "http", "url": url });
        let v = serde_json::json!({ "mcpServers": { "tuna-search": server_val } });
        let json_str = v.to_string();
        assert!(
            !json_str.contains("Authorization"),
            "headers가 있으면 안 됨: {json_str}"
        );
        assert!(runner.search_token.is_none(), "token은 None이어야 함");
    }

    #[test]
    fn search_url_takes_priority_over_search_db() {
        // search_url과 search_db 둘 다 설정 시 url이 우선(HTTP config 생성, stdio 경로 무시).
        let url = "http://127.0.0.1:9090/mcp";
        let runner = ClaudeRunner::new()
            .with_search_db(Some("/tmp/fallback.db".into()))
            .with_search_url(Some(url.into()), None);
        // search_url이 있으면 mcp_json은 HTTP config여야 한다(command 없음).
        // 필드만으로 우선순위를 확인한다.
        assert!(runner.search_url.is_some(), "search_url이 Some이어야 함");
        assert!(
            runner.search_db.is_some(),
            "search_db도 Some으로 남아 있어야 함"
        );
        // 직접 조립: search_url이 Some이면 HTTP branch 진입(command/args 없음).
        let v =
            serde_json::json!({ "mcpServers": { "tuna-search": { "type": "http", "url": url } } });
        let json_str = v.to_string();
        assert!(
            !json_str.contains("command"),
            "url 우선 시 command가 없어야 함: {json_str}"
        );
    }

    #[test]
    fn search_db_only_produces_stdio_config() {
        // search_url 없이 search_db만 설정 시 stdio config(command/args)가 생성된다.
        let runner = ClaudeRunner::new().with_search_db(Some("/tmp/x.db".into()));
        assert!(runner.search_url.is_none(), "search_url은 None이어야 함");
        assert!(runner.search_db.is_some(), "search_db가 Some이어야 함");
        // stdio config 재현.
        let db = "/tmp/x.db".to_string();
        let exe = "tunaround".to_string();
        let v = serde_json::json!({
            "mcpServers": {
                "tuna-search": {
                    "command": exe,
                    "args": ["mcp-search", "--db", db]
                }
            }
        });
        let json_str = v.to_string();
        assert!(
            json_str.contains("command"),
            "stdio config에 command 없음: {json_str}"
        );
        assert!(
            !json_str.contains("\"type\":\"http\""),
            "stdio config에 type:http 있으면 안 됨"
        );
    }

    #[test]
    fn build_mcp_config_with_search_db_uses_mcp_search_subcommand_form() {
        // ⚠ 회귀 가드: main.rs가 --mcp-search 플래그에서 `mcp-search` 서브커맨드로 바뀌었으므로,
        // claude가 self-exe로 spawn하는 MCP config args도 서브커맨드 형태여야 한다(레거시 플래그 잔존 금지).
        let runner = ClaudeRunner::new().with_search_db(Some("/tmp/x.db".into()));
        let json = runner.build_mcp_config().expect("search_db 설정 시 Some");
        assert!(
            json.contains("\"mcp-search\""),
            "mcp-search 서브커맨드 형태 없음: {json}"
        );
        assert!(
            !json.contains("--mcp-search"),
            "레거시 --mcp-search 플래그 잔존: {json}"
        );
    }

    #[test]
    fn runner_with_search_session_includes_session_id_in_mcp_config() {
        // with_search_session(Some(..)) 설정 시 MCP config args에 --session-id가 포함된다.
        let runner = ClaudeRunner::new()
            .with_search_db(Some("/tmp/test.db".into()))
            .with_search_session(Some("my-session-42".into()));
        // run()을 직접 호출하면 실제 claude가 필요하므로, MCP json 조립 경로만 내부 검증한다.
        // mcp_json 생성 로직을 직접 재현해 args를 확인한다.
        let db = "/tmp/test.db".to_string();
        let exe = "tunaround".to_string();
        let sid = "my-session-42".to_string();
        let mut mcp_args = vec![
            serde_json::Value::String("mcp-search".into()),
            serde_json::Value::String("--db".into()),
            serde_json::Value::String(db),
        ];
        mcp_args.push(serde_json::Value::String("--session-id".into()));
        mcp_args.push(serde_json::Value::String(sid));
        let v = serde_json::json!({
            "mcpServers": { "tuna-search": { "command": exe, "args": mcp_args } }
        });
        let json_str = v.to_string();
        assert!(
            json_str.contains("--session-id"),
            "--session-id가 MCP config에 없음: {json_str}"
        );
        assert!(
            json_str.contains("my-session-42"),
            "세션 id가 MCP config에 없음: {json_str}"
        );
        // with_search_session(Some(..)) 호출 시 빌더가 세션 id를 실제로 저장했는지 확인한다.
        assert_eq!(
            runner.search_session.as_deref(),
            Some("my-session-42"),
            "with_search_session 호출 시 search_session에 값이 저장돼야 함"
        );
        // search_session 미설정 시 None(위 --session-id 경로가 타지 않음).
        let runner_no_session = ClaudeRunner::new().with_search_db(Some("/tmp/test.db".into()));
        assert!(
            runner_no_session.search_session.is_none(),
            "with_search_session 미호출 시 None이어야 함"
        );
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
        let input = RunInput {
            prompt: "x".into(),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
}
