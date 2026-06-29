// Codex exec --json argv·파싱·dedup 순수함수 + CodexRunner.

use super::{RunInput, RunMode, RunOutput};

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
                if let Some(item) = event.get("item") {
                    if item.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                push_agent_text_dedup(&mut texts, text);
                            }
                        }
                    }
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
/// Step 1 실측(2026-06-29): codex --full-auto 없음.
///   Write  → --sandbox workspace-write
///   ReadOnly → --sandbox read-only
fn build_codex_args(input: &RunInput) -> Vec<String> {
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
    args
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

    // NOTE: Step 1 실측 결과 --full-auto 없음.
    // Write mode → --sandbox workspace-write (실제 codex 플래그로 변경)
    #[test]
    fn args_write_mode_uses_full_auto() {
        let input = RunInput {
            prompt: "p".into(),
            model: None,
            project_path: None,
            mode: RunMode::Write,
        };
        let args = build_codex_args(&input);
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
        let args = build_codex_args(&input);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-x"));
    }
}
