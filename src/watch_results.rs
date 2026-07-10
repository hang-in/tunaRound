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
        // \r도 제거한다(터미널에서 \r은 커서를 줄 앞으로 보내 기존 출력을 덮어쓴다).
        Some(t) => t.replace('\r', "").replace('\n', " ").chars().take(160).collect(),
        None => "(내용 없음)".to_string(),
    }
}

/// SSE data 한 줄(`{"event":..,"task":{..}}`)을 파싱해, dispatcher가 던진 terminal(completed/failed) task면
/// ("RESULT ..." 한 줄, failed 여부)를 만든다. 이미 본 task(seen)·비-terminal·다른 dispatcher는 None.
/// dispatcher가 빈 문자열이면 fromAgent 필터를 끈다(전체 완료 관측). failed 여부는 --digest 분기용
/// (failed=즉시 알림 / completed=묶음 가능, v2-44 W5).
pub fn parse_result_line(data: &str, dispatcher: &str, seen: &mut HashSet<String>) -> Option<(String, bool)> {
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
    Some((format!("RESULT {short} {state} <- {to} :: {}", extract_result_text(task)), state == "failed"))
}

/// 브로커 SSE를 구독해 dispatcher의 완료/실패 결과를 stdout으로 흘린다. 스트림이 끊기면 Err로 종료해
/// 호출부(감시 도구)가 재기동하게 한다(exit 0이면 재기동 안 하는 감시자가 있어 Err가 안전).
/// digest_secs>0이면 completed는 그 구간 동안 묶어 한 번에 낸다(총괄 wake 절약, v2-44 W5).
/// failed는 digest와 무관하게 즉시 낸다(막힌 task는 총괄 판단이 급하다).
pub async fn run(core: &str, dispatcher: &str, digest_secs: u64) -> Result<(), String> {
    use futures_util::StreamExt;
    use std::io::Write;
    let url = format!("{}/dashboard/events", core.trim_end_matches('/'));
    // connect timeout만 둔다(SSE 바디는 무한정 열려 있어야 하므로 전체 요청 timeout은 두지 않는다).
    // TCP는 붙었는데 응답이 없는 상황(방화벽 drop)에서 send가 무한 대기하는 것을 막는다.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("watch-results: 클라이언트 구성 실패: {e}"))?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("watch-results: SSE 접속 실패({url}): {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("watch-results: SSE 상태 {}", resp.status()));
    }
    eprintln!("[watch-results] {url} 구독 시작 (dispatcher={dispatcher}, digest={digest_secs}s)");
    let mut seen: HashSet<String> = HashSet::new();
    // 버퍼는 Vec<u8>로 유지한다. 청크마다 UTF-8 변환하면 멀티바이트 문자(한글 등)가 청크 경계에서
    // 깨지므로(U+FFFD 영구 손실), 개행(\n=ASCII)으로 완결된 라인만 변환한다.
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    // digest 대기 중인 completed 라인들. 첫 항목이 들어온 시점 + digest_secs에 한 번에 낸다.
    let mut pending: Vec<String> = Vec::new();
    let mut flush_at: Option<tokio::time::Instant> = None;
    let flush = |pending: &mut Vec<String>| {
        for line in pending.drain(..) {
            println!("{line}");
        }
        let _ = std::io::stdout().flush();
    };
    loop {
        tokio::select! {
            // digest 마감: 묶인 completed를 한 번에 내보낸다(출력 burst 1회 = 총괄 wake 1회).
            _ = async { tokio::time::sleep_until(flush_at.unwrap()).await }, if flush_at.is_some() => {
                flush(&mut pending);
                flush_at = None;
            }
            chunk = stream.next() => {
                let Some(chunk) = chunk else {
                    flush(&mut pending); // 종료 전 잔여분을 잃지 않는다.
                    return Err("watch-results: SSE 스트림이 종료됨(재구독 필요)".to_string());
                };
                let chunk = chunk.map_err(|e| format!("watch-results: 스트림 오류: {e}"))?;
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                    // 라인은 \n에서 끝나므로 완결된 UTF-8(문자 중간에서 안 잘림) → lossy여도 손실 없음.
                    let line = String::from_utf8_lossy(&line_bytes);
                    let Some(data) = line.trim_end().strip_prefix("data: ") else {
                        continue;
                    };
                    let Some((out, is_failed)) = parse_result_line(data, dispatcher, &mut seen) else {
                        continue;
                    };
                    if digest_secs > 0 && !is_failed {
                        pending.push(out);
                        if flush_at.is_none() {
                            flush_at = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(digest_secs));
                        }
                    } else {
                        println!("{out}");
                        let _ = std::io::stdout().flush();
                    }
                }
            }
        }
    }
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
        let (line, is_failed) = line.unwrap();
        assert!(line.contains("RESULT abc12345"));
        assert!(line.contains("completed"));
        assert!(line.contains("mac-claude-sup"));
        assert!(line.contains("발견 6건"));
        assert!(!is_failed, "completed는 digest 묶음 대상");
    }

    #[test]
    fn failed_yields_result_with_status_text() {
        let mut seen = HashSet::new();
        let data = serde_json::json!({
            "event": "status",
            "task": { "id": "f00d", "state": "failed", "fromAgent": "dashboard", "toAgent": "mac-claude-sup",
                      "artifacts": [], "statusMessage": { "parts": [{ "text": "BLOCKED: discover 없음" }] } }
        }).to_string();
        let (line, is_failed) = parse_result_line(&data, "dashboard", &mut seen).unwrap();
        assert!(line.contains("failed"));
        assert!(line.contains("BLOCKED"));
        assert!(is_failed, "failed는 digest 무관 즉시 알림");
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
