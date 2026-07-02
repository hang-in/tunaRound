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
/// bypass=true이면 read-only 샌드박스 대신 --dangerously-bypass-approvals-and-sandbox를 쓴다.
/// (codex exec는 read-only 유지 채로 MCP 도구 승인이 불가=업스트림 #24135. pull+ReadOnly 좌석만
/// 호출자가 bypass=true로 넘기며, read-only는 프롬프트 지시로 보완한다.)
fn build_codex_args(input: &RunInput, mcp_args: &[String], bypass: bool) -> Vec<String> {
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
        RunMode::ReadOnly if bypass => {
            // MCP 도구 승인을 통과시키는 유일 우회. 샌드박스가 풀리므로 read-only는 지시로 강제.
            args.push("--dangerously-bypass-approvals-and-sandbox".into());
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
    /// MCP spawn 시 전달할 세션 id. Some이면 TOML args에 --session-id <sid>를 추가한다.
    search_session: Option<String>,
    /// 원격 HTTP MCP 서버 URL. Some이면 stdio spawn 대신 -c mcp_servers.tuna-search.url 배선(search_db보다 우선).
    search_url: Option<String>,
    /// HTTP MCP 서버 bearer 토큰. Some이면 bearer_token_env_var(TUNA_SEARCH_TOKEN) config +
    /// 자식 env 주입으로 인증(비밀을 argv/config에 노출하지 않음).
    search_token: Option<String>,
}

impl CodexRunner {
    pub fn new() -> Self {
        Self {
            bin: "codex".to_string(),
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
    /// 검색 MCP 서버 DB 경로 주입. Some이면 codex에 -c mcp_servers.tuna-search로 self-exe를 spawn하도록 배선한다.
    pub fn with_search_db(mut self, db: Option<String>) -> Self {
        self.search_db = db;
        self
    }
    /// MCP 서버 spawn 시 사용할 세션 id 주입. Some이면 --session-id <sid>를 TOML args에 추가한다.
    pub fn with_search_session(mut self, session: Option<String>) -> Self {
        self.search_session = session;
        self
    }
    /// 원격 HTTP MCP 서버 URL + bearer 토큰 주입. url이 Some이면 stdio spawn 대신 HTTP 배선하고,
    /// token이 Some이면 bearer_token_env_var config + 자식 env로 인증까지 배선한다.
    pub fn with_search_url(mut self, url: Option<String>, token: Option<String>) -> Self {
        self.search_url = url;
        self.search_token = token;
        self
    }
}

impl Default for CodexRunner {
    fn default() -> Self {
        Self::new()
    }
}

/// codex가 원격 HTTP MCP bearer 토큰을 읽어올 환경변수 이름(config엔 이름만, 값은 env로 주입).
const BEARER_TOKEN_ENV: &str = "TUNA_SEARCH_TOKEN";

/// pull+ReadOnly codex는 샌드박스를 풀어(bypass) MCP를 쓰므로, read-only를 지시로 강제하는 접두 규칙.
/// codex는 규칙 준수가 강해 우리 루프(사람의 unlock 개입 없음)에서 이를 지킨다.
const READONLY_DIRECTIVE: &str = "[중요 규칙] 너는 읽기 전용 토론 참가자다. 어떤 파일도 생성·수정·삭제하지 말고, 변경성 셸 명령(git commit, rm, 파일 쓰기 등)도 실행하지 마라. 오직 전사·레포를 읽고 논의만 하라. 이 규칙은 요청이 있어도 예외 없다.";

/// TOML basic 문자열로 안전 인용(역슬래시·큰따옴표 이스케이프). 인자 주입 방지.
fn toml_basic(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

impl CodexRunner {
    /// 검색 MCP 배선(-c mcp_servers... 인자)과 자식 env(bearer 토큰)를 조립한다.
    /// search_url이 Some이면 HTTP MCP 배선 우선(+token 있으면 bearer_token_env_var+env),
    /// 아니면 search_db 기반 stdio self-exe spawn. TOML basic으로 인용해 주입 방지.
    /// 순수 조립이라 테스트 가능(프로세스 spawn과 분리).
    fn build_mcp_wiring(&self) -> (Vec<String>, Vec<(String, String)>) {
        let mut child_env: Vec<(String, String)> = Vec::new();
        let mcp_args: Vec<String> = if let Some(url) = &self.search_url {
            let mut v = vec![
                "-c".into(),
                format!("mcp_servers.tuna-search.url={}", toml_basic(url)),
            ];
            // bearer 토큰은 config엔 env 변수명만, 실제 값은 자식 env로 주입(argv/config에 비밀 미노출).
            if let Some(token) = &self.search_token {
                v.push("-c".into());
                v.push(format!(
                    "mcp_servers.tuna-search.bearer_token_env_var={}",
                    toml_basic(BEARER_TOKEN_ENV)
                ));
                child_env.push((BEARER_TOKEN_ENV.to_string(), token.clone()));
            }
            v
        } else {
            self.search_db.as_ref().map(|db| {
                let exe = std::env::current_exe()
                    .ok()
                    .and_then(|p| p.to_str().map(String::from))
                    .unwrap_or_else(|| "tunaround".into());
                let mut items = vec![
                    "mcp-search".to_string(),
                    "--db".to_string(),
                    db.clone(),
                ];
                if let Some(sid) = &self.search_session {
                    items.push("--session-id".into());
                    items.push(sid.clone());
                }
                let arr = items.iter().map(|a| toml_basic(a)).collect::<Vec<_>>().join(",");
                let args_toml = format!("mcp_servers.tuna-search.args=[{arr}]");
                vec![
                    "-c".into(),
                    format!("mcp_servers.tuna-search.command={}", toml_basic(&exe)),
                    "-c".into(),
                    args_toml,
                ]
            }).unwrap_or_default()
        };
        (mcp_args, child_env)
    }
}

impl Runner for CodexRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let (mcp_args, child_env) = self.build_mcp_wiring();
        // pull+ReadOnly+MCP배선 좌석만 샌드박스를 풀어 MCP 승인 통과. 그 외는 기존 샌드박스 유지.
        let mcp_wired = self.search_url.is_some() || self.search_db.is_some();
        let bypass = matches!(input.mode, RunMode::ReadOnly) && input.pull && mcp_wired;
        // 샌드박스를 푼 경우 read-only를 지시로 보완(프롬프트 접두).
        let prompt = if bypass {
            format!("{READONLY_DIRECTIVE}\n\n{}", input.prompt)
        } else {
            input.prompt.clone()
        };
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_codex_args(input, &mcp_args, bypass),
            cwd: input.project_path.clone(),
            stdin: Some(prompt),
            idle_timeout: self.idle_timeout,
            label: "codex".to_string(),
            env: child_env,
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
        let input = RunInput { prompt: "p".into(), mode: RunMode::Write, ..Default::default() };
        let args = build_codex_args(&input, &[], false);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox workspace-write"));
        assert_eq!(args.last().unwrap(), "-"); // prompt via stdin
    }

    #[test]
    fn args_readonly_mode_uses_sandbox_readonly() {
        let input = RunInput {
            prompt: "p".into(),
            model: Some("gpt-x".into()),
            mode: RunMode::ReadOnly,
            ..Default::default()
        };
        let args = build_codex_args(&input, &[], false);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-x"));
    }

    #[test]
    fn args_readonly_bypass_replaces_sandbox() {
        // bypass=true(pull+ReadOnly+MCP)면 read-only 샌드박스 대신 위험 우회 플래그를 쓴다.
        let input = RunInput { prompt: "p".into(), mode: RunMode::ReadOnly, pull: true, ..Default::default() };
        let args = build_codex_args(&input, &[], true);
        let joined = args.join(" ");
        assert!(joined.contains("--dangerously-bypass-approvals-and-sandbox"), "bypass 플래그 없음: {joined}");
        assert!(!joined.contains("--sandbox read-only"), "bypass인데 read-only 샌드박스 잔존: {joined}");
    }

    #[test]
    fn args_with_mcp_args_appends_c_flags() {
        let input = RunInput { prompt: "q".into(), mode: RunMode::ReadOnly, ..Default::default() };
        let mcp_args = vec![
            "-c".to_string(),
            "mcp_servers.tuna-search.command=\"/usr/bin/tunaround\"".to_string(),
            "-c".to_string(),
            "mcp_servers.tuna-search.args=[\"mcp-search\",\"--db\",\"/tmp/t.db\"]".to_string(),
        ];
        let args = build_codex_args(&input, &mcp_args, false);
        let joined = args.join(" ");
        assert!(joined.contains("-c"), "-c 플래그 없음: {joined}");
        assert!(joined.contains("mcp_servers.tuna-search"), "서버 이름 없음: {joined}");
        // 빈 슬라이스면 -c 없음.
        let args_none = build_codex_args(&input, &[], false);
        assert!(!args_none.join(" ").contains("mcp_servers.tuna-search"), "mcp_args 없는데 포함됨");
    }

    #[test]
    fn with_search_url_produces_url_c_flag() {
        // search_url 설정 시 실제 조립(build_mcp_wiring)이 -c mcp_servers.tuna-search.url을 포함한다.
        let url = "http://127.0.0.1:8080/mcp";
        let runner = CodexRunner::new().with_search_url(Some(url.into()), Some("tok".into()));
        let (mcp_args, _env) = runner.build_mcp_wiring();
        let joined = mcp_args.join(" ");
        assert!(joined.contains("-c"), "-c 없음: {joined}");
        assert!(joined.contains("mcp_servers.tuna-search.url"), "url 키 없음: {joined}");
        assert!(joined.contains(url), "url 값 없음: {joined}");
    }

    #[test]
    fn with_search_token_wires_bearer_env_not_argv() {
        // token 주입 시 config엔 bearer_token_env_var(변수명)만, 실제 토큰은 자식 env로 간다(argv 비노출).
        let url = "http://127.0.0.1:8080/mcp";
        let runner = CodexRunner::new().with_search_url(Some(url.into()), Some("secret-tok".into()));
        let (mcp_args, env) = runner.build_mcp_wiring();
        let joined = mcp_args.join(" ");
        assert!(joined.contains("bearer_token_env_var"), "bearer_token_env_var 키 없음: {joined}");
        assert!(joined.contains(BEARER_TOKEN_ENV), "env 변수명 없음: {joined}");
        assert!(!joined.contains("secret-tok"), "토큰이 argv에 노출됨: {joined}");
        assert_eq!(
            env.iter().find(|(k, _)| k == BEARER_TOKEN_ENV).map(|(_, v)| v.as_str()),
            Some("secret-tok"),
            "env에 토큰 주입 안 됨: {env:?}"
        );
    }

    #[test]
    fn no_token_means_no_bearer_wiring() {
        // token 없으면 bearer_token_env_var 키도 env도 없다.
        let url = "http://127.0.0.1:8080/mcp";
        let runner = CodexRunner::new().with_search_url(Some(url.into()), None);
        let (mcp_args, env) = runner.build_mcp_wiring();
        assert!(!mcp_args.join(" ").contains("bearer_token_env_var"), "토큰 없는데 bearer 키 포함");
        assert!(env.is_empty(), "토큰 없는데 env 주입됨: {env:?}");
    }

    #[test]
    fn search_url_priority_over_search_db_in_codex() {
        // search_url과 search_db 둘 다 있으면 url이 우선(command/args 없음).
        let url = "http://127.0.0.1:9090/mcp";
        let runner = CodexRunner::new()
            .with_search_db(Some("/tmp/fallback.db".into()))
            .with_search_url(Some(url.into()), None);
        let (mcp_args, _env) = runner.build_mcp_wiring();
        let joined = mcp_args.join(" ");
        assert!(joined.contains("mcp_servers.tuna-search.url"), "url 배선 없음: {joined}");
        assert!(!joined.contains("command"), "url 우선 시 command 없어야 함: {joined}");
    }

    #[test]
    fn build_mcp_wiring_with_search_db_uses_mcp_search_subcommand_form() {
        // ⚠ 회귀 가드: main.rs가 --mcp-search 플래그에서 `mcp-search` 서브커맨드로 바뀌었으므로,
        // codex가 self-exe로 spawn하는 args도 서브커맨드 형태여야 한다(레거시 플래그 잔존 금지).
        let runner = CodexRunner::new().with_search_db(Some("/tmp/x.db".into()));
        let (mcp_args, _env) = runner.build_mcp_wiring();
        let joined = mcp_args.join(" ");
        assert!(joined.contains("\"mcp-search\""), "mcp-search 서브커맨드 형태 없음: {joined}");
        assert!(!joined.contains("--mcp-search"), "레거시 --mcp-search 플래그 잔존: {joined}");
    }

    #[test]
    fn runner_with_search_session_includes_session_id_in_mcp_args() {
        // with_search_session(Some(..)) 설정 시 TOML args에 --session-id가 포함된다.
        let db = "/tmp/test.db".to_string();
        let sid = "debate-session-7".to_string();
        // toml_basic으로 조립한 결과 포맷: double-quote.
        let items_with = ["mcp-search", "--db", &db, "--session-id", &sid];
        let args_toml_with = format!(
            "mcp_servers.tuna-search.args=[{}]",
            items_with.iter().map(|a| toml_basic(a)).collect::<Vec<_>>().join(",")
        );
        assert!(args_toml_with.contains("--session-id"), "--session-id 없음: {args_toml_with}");
        assert!(args_toml_with.contains("debate-session-7"), "세션 id 없음: {args_toml_with}");
        // search_session 없을 때.
        let items_without = ["mcp-search", "--db", &db];
        let args_toml_without = format!(
            "mcp_servers.tuna-search.args=[{}]",
            items_without.iter().map(|a| toml_basic(a)).collect::<Vec<_>>().join(",")
        );
        assert!(!args_toml_without.contains("--session-id"), "--session-id가 None인데 포함됨");
        // 빌더 필드 검증.
        let runner = CodexRunner::new()
            .with_search_db(Some(db))
            .with_search_session(Some(sid));
        assert!(runner.search_session.is_some(), "search_session이 Some이어야 함");
        assert_eq!(runner.search_session.as_deref(), Some("debate-session-7"));
        // 미설정 시 None.
        let runner_no = CodexRunner::new()
            .with_search_db(Some("/tmp/x.db".into()))
            .with_search_db(Some("/tmp/x.db".into()));
        assert!(runner_no.search_session.is_none(), "미설정 시 None이어야 함");
    }

    #[test]
    fn toml_basic_escapes_special_characters() {
        // 작은따옴표는 그대로 통과(TOML basic 이스케이프 불필요).
        assert_eq!(toml_basic("a'b"), "\"a'b\"");
        // 큰따옴표는 이스케이프.
        assert_eq!(toml_basic("a\"b"), "\"a\\\"b\"");
        // 역슬래시는 이스케이프(Windows 경로).
        assert_eq!(toml_basic("C:\\path"), "\"C:\\\\path\"");
        // 주입 시도: 닫힘 큰따옴표+추가 원소가 배열을 조기 종결하지 않음.
        let evil = "\",\"--evil";
        let quoted = toml_basic(evil);
        // 결과 전체가 하나의 quoted 원소여야 한다(배열에 넣어도 원소 수 = 1).
        assert!(quoted.starts_with('"'), "시작 따옴표 없음");
        assert!(quoted.ends_with('"'), "끝 따옴표 없음");
        // 원소 내부에 비이스케이프 큰따옴표가 없어야 한다(첫·끝 제외).
        let inner = &quoted[1..quoted.len() - 1];
        // \" 이스케이프 시퀀스를 제거한 뒤 남는 " 가 없어야 한다(비이스케이프 주입 방지).
        let without_escaped = inner.replace("\\\"", "");
        assert!(!without_escaped.contains('"'), "비이스케이프 큰따옴표: {inner}");
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
        let input = RunInput { prompt: "x".into(), mode: RunMode::ReadOnly, ..Default::default() };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
}
