// 총괄(dispatcher) 결과 인박스: 내가 던진 task가 완료/실패하면 그 결과를 알린다(책임의 이전 = 결과 push).
// 브로커의 /dashboard/events SSE(무인증)를 구독해 fromAgent==dispatcher인 terminal 이벤트만 골라 stdout에
// 한 줄로 낸다. 총괄 세션이 이 프로세스를 Monitor로 감싸면 "던지고 자리 떠도 결과가 깨우는" 구조가 된다.

use std::collections::HashSet;

/// task 스냅샷에서 결과 텍스트를 뽑는다: completed=artifact 텍스트, 그 외=statusMessage 텍스트.
fn extract_result_text(task: &serde_json::Value) -> String {
    let from_artifact = task
        .get("artifacts")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str());
    let from_status = task
        .get("statusMessage")
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str());
    match from_artifact.or(from_status) {
        Some(t) => t.replace('\n', " ").chars().take(160).collect(),
        None => "(내용 없음)".to_string(),
    }
}

/// SSE data 한 줄(`{"event":..,"task":{..}}`)을 파싱해, dispatcher가 던진 terminal(completed/failed) task면
/// "RESULT ..." 한 줄을 만든다. 이미 본 task(seen)·비-terminal·다른 dispatcher는 None. dispatcher가 빈
/// 문자열이면 fromAgent 필터를 끈다(전체 완료 관측).
pub fn parse_result_line(data: &str, dispatcher: &str, seen: &mut HashSet<String>) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
    let task = v.get("task")?;
    let state = task.get("state")?.as_str()?;
    if state != "completed" && state != "failed" {
        return None;
    }
    let from = task.get("fromAgent").and_then(|x| x.as_str()).unwrap_or("");
    if !dispatcher.is_empty() && from != dispatcher {
        return None;
    }
    let id = task.get("id")?.as_str()?;
    if !seen.insert(id.to_string()) {
        return None; // 같은 task terminal은 한 번만
    }
    let to = task.get("toAgent").and_then(|x| x.as_str()).unwrap_or("?");
    let short: String = id.chars().take(8).collect();
    Some(format!("RESULT {short} {state} <- {to} :: {}", extract_result_text(task)))
}

/// 브로커 SSE를 구독해 dispatcher의 완료/실패 결과를 stdout으로 흘린다. 연결이 끊기면 종료(호출부가 재기동).
pub async fn run(core: &str, dispatcher: &str) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;
    let url = format!("{}/dashboard/events", core.trim_end_matches('/'));
    let resp = reqwest::Client::new()
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("watch-results: SSE 접속 실패({url}): {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("watch-results: SSE 상태 {}", resp.status()));
    }
    eprintln!("[watch-results] {url} 구독 시작 (dispatcher={dispatcher})");
    let mut seen: HashSet<String> = HashSet::new();
    let mut buf = String::new();
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|e| format!("watch-results: 스트림 오류: {e}"))?;
        buf.push_str(&String::from_utf8_lossy(&chunk));
        // 개행 단위로 완결된 라인만 처리(부분 청크는 buf에 남긴다).
        while let Some(pos) = buf.find('\n') {
            let line: String = buf.drain(..=pos).collect();
            let Some(data) = line.trim_end().strip_prefix("data: ") else {
                continue;
            };
            if let Some(out) = parse_result_line(data, dispatcher, &mut seen) {
                println!("{out}");
                let _ = std::io::stdout().flush();
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(state: &str, from: &str, to: &str, id: &str, artifact: Option<&str>) -> String {
        let art = match artifact {
            Some(t) => serde_json::json!([{ "parts": [{ "text": t }] }]),
            None => serde_json::json!([]),
        };
        serde_json::json!({
            "event": if state == "completed" { "completed" } else { "status" },
            "task": { "id": id, "state": state, "fromAgent": from, "toAgent": to, "artifacts": art }
        })
        .to_string()
    }

    #[test]
    fn completed_from_dispatcher_yields_result() {
        let mut seen = HashSet::new();
        let line = parse_result_line(&ev("completed", "dashboard", "mac-claude-sup", "abc12345xyz", Some("발견 6건")), "dashboard", &mut seen);
        let line = line.unwrap();
        assert!(line.contains("RESULT abc12345"));
        assert!(line.contains("completed"));
        assert!(line.contains("mac-claude-sup"));
        assert!(line.contains("발견 6건"));
    }

    #[test]
    fn failed_yields_result_with_status_text() {
        let mut seen = HashSet::new();
        let data = serde_json::json!({
            "event": "status",
            "task": { "id": "f00d", "state": "failed", "fromAgent": "dashboard", "toAgent": "mac-claude-sup",
                      "artifacts": [], "statusMessage": { "parts": [{ "text": "BLOCKED: discover 없음" }] } }
        }).to_string();
        let line = parse_result_line(&data, "dashboard", &mut seen).unwrap();
        assert!(line.contains("failed"));
        assert!(line.contains("BLOCKED"));
    }

    #[test]
    fn non_terminal_and_other_dispatcher_filtered() {
        let mut seen = HashSet::new();
        assert!(parse_result_line(&ev("working", "dashboard", "x", "1", None), "dashboard", &mut seen).is_none());
        assert!(parse_result_line(&ev("completed", "other", "x", "2", None), "dashboard", &mut seen).is_none());
    }

    #[test]
    fn same_task_terminal_reported_once() {
        let mut seen = HashSet::new();
        let e = ev("completed", "dashboard", "x", "dup1", Some("r"));
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_some());
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_none()); // 두 번째는 dedup
    }
}
