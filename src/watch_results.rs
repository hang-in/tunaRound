// 총괄(dispatcher) 결과 인박스: 내가 던진 task가 완료/실패하면 그 결과를 알린다(책임의 이전 = 결과 push).
// 브로커의 /dashboard/events SSE(무인증)를 구독해 fromAgent==dispatcher인 terminal 이벤트만 골라 stdout에
// 한 줄로 낸다. 총괄 세션이 이 프로세스를 Monitor로 감싸면 "던지고 자리 떠도 결과가 깨우는" 구조가 된다.

use std::collections::{HashSet, VecDeque};

/// terminal dedup 집합의 상한. 라이브 버스는 terminal 이벤트를 task당 한 번만 흘리므로 dedup은
/// 방어 장치다 - 최근 창만 유지하면 충분하고, 상한 없이는 장기 상주 시 무한 성장한다(리뷰 지적).
/// 주간 task 약 100건 실측 대비 수개월분 여유.
const SEEN_CAP: usize = 4096;

/// 상한이 있는 terminal dedup 집합: 초과 시 가장 오래 기억한 id부터 잊는다(FIFO).
struct SeenSet {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenSet {
    fn new() -> Self {
        Self { set: HashSet::new(), order: VecDeque::new() }
    }

    /// 새 id면 기억하고 true, 이미 본 id면 false. 상한 초과분은 오래된 것부터 방출한다.
    fn insert(&mut self, id: &str) -> bool {
        if !self.set.insert(id.to_string()) {
            return false;
        }
        self.order.push_back(id.to_string());
        while self.order.len() > SEEN_CAP {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }
}

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
fn parse_result_line(data: &str, dispatcher: &str, seen: &mut SeenSet) -> Option<(String, bool)> {
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
    if !seen.insert(id) {
        return None; // 같은 task terminal은 한 번만
    }
    let to = task.get("toAgent").and_then(|x| x.as_str()).unwrap_or("?");
    let short: String = id.chars().take(8).collect();
    Some((format!("RESULT {short} {state} <- {to} :: {}", extract_result_text(task)), state == "failed"))
}

/// 재접속을 포기하기 전까지 허용하는 연속 실패 횟수. 초과 시 run()이 Err를 반환해 호출부가
/// exit(1)하게 한다(주소 오타 같은 영구 실패를 조용히 삼키지 않고 Monitor가 죽음을 알게 하는 정책).
const MAX_CONSECUTIVE_FAILURES: u32 = 20;

/// 재접속 지수 백오프 대기 시간(초): 연속 실패 1회=1s, 이후 2배씩(2→4→8→16), 상한 30s.
fn backoff_secs(consecutive_failures: u32) -> u64 {
    // 2^5=32는 상한 30을 넘으므로 지수를 5에서 멈추고 min으로 자른다(0회는 방어적으로 1s).
    let exp = consecutive_failures.saturating_sub(1).min(5);
    (1u64 << exp).min(30)
}

/// 실패 연쇄 리셋에 필요한 접속 최소 생존 시간(초). 2xx 수립만으로 리셋하면 "수립 직후 즉시
/// 닫히는" 브로커(크래시루프, 200 후 빈 바디를 주는 오설정 엔드포인트)가 카운터를 영원히
/// 리셋해 포기(exit 1)가 불가능해지므로, 이 시간 이상 살았던 접속만 건강했던 것으로 본다.
const MIN_HEALTHY_SECS: u64 = 30;

/// 실패 연쇄 리셋 판정: 2xx 스트림 수립 후(None=수립 실패) 최소 생존 시간을 넘긴 접속만 "건강했다".
/// 생존 시간은 수립 시점부터 잰다(접속 수립에 쓴 핸드셰이크 시간을 생존으로 오산하지 않게, 리뷰 반영).
fn connection_was_healthy(lived_after_connect: Option<std::time::Duration>) -> bool {
    lived_after_connect.is_some_and(|lived| lived >= std::time::Duration::from_secs(MIN_HEALTHY_SECS))
}

/// 재접속을 넘어 유지되는 인박스 상태(재접속 루프 바깥 소유): terminal dedup(seen)·digest 묶음(pending)·
/// flush 예정 시각. 접속이 끊겨도 "이미 알린 task"와 "아직 못 알린 묶음"을 잃지 않는다.
struct InboxState {
    seen: SeenSet,
    pending: Vec<String>,
    flush_at: Option<tokio::time::Instant>,
}

/// digest로 묶인 completed 라인들을 한 번에 stdout으로 내보낸다(출력 burst 1회 = 총괄 wake 1회).
fn flush_pending(pending: &mut Vec<String>) {
    use std::io::Write;
    if pending.is_empty() {
        return;
    }
    for line in pending.drain(..) {
        println!("{line}");
    }
    let _ = std::io::stdout().flush();
}

/// SSE 접속 1회분: 접속해 끊길 때까지 이벤트를 처리하고, 단절 사유를 돌려준다(정상 종료 없음).
/// 2xx 스트림 수립에 성공하면 *connected_at=수립 시점(호출부가 순수 생존 시간으로 실패 카운터
/// 리셋을 판정할 근거). state(seen·pending·flush_at)는 호출부(재접속 루프) 소유라 재접속을 넘어 유지된다.
async fn run_once(
    client: &reqwest::Client,
    url: &str,
    dispatcher: &str,
    digest_secs: u64,
    state: &mut InboxState,
    connected_at: &mut Option<tokio::time::Instant>,
) -> String {
    use futures_util::StreamExt;
    use std::io::Write;
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return format!("SSE 접속 실패({url}): {e}"),
    };
    if !resp.status().is_success() {
        return format!("SSE 상태 {}", resp.status());
    }
    *connected_at = Some(tokio::time::Instant::now());
    eprintln!("[watch-results] {url} 구독 시작 (dispatcher={dispatcher}, digest={digest_secs}s)");
    // 버퍼는 Vec<u8>로 유지한다. 청크마다 UTF-8 변환하면 멀티바이트 문자(한글 등)가 청크 경계에서
    // 깨지므로(U+FFFD 영구 손실), 개행(\n=ASCII)으로 완결된 라인만 변환한다.
    // 접속마다 새로 시작한다(끊긴 접속의 반쪽 라인을 새 스트림에 이어 붙이면 오염).
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    loop {
        tokio::select! {
            // digest 마감: 묶인 completed를 한 번에 내보낸다(출력 burst 1회 = 총괄 wake 1회).
            _ = async { tokio::time::sleep_until(state.flush_at.unwrap()).await }, if state.flush_at.is_some() => {
                flush_pending(&mut state.pending);
                state.flush_at = None;
            }
            chunk = stream.next() => {
                let Some(chunk) = chunk else {
                    return "SSE 스트림이 종료됨".to_string();
                };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => return format!("스트림 오류: {e}"),
                };
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                    // 라인은 \n에서 끝나므로 완결된 UTF-8(문자 중간에서 안 잘림) → lossy여도 손실 없음.
                    let line = String::from_utf8_lossy(&line_bytes);
                    let Some(data) = line.trim_end().strip_prefix("data: ") else {
                        continue;
                    };
                    let Some((out, is_failed)) = parse_result_line(data, dispatcher, &mut state.seen) else {
                        continue;
                    };
                    if digest_secs > 0 && !is_failed {
                        state.pending.push(out);
                        if state.flush_at.is_none() {
                            state.flush_at = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(digest_secs));
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

/// 브로커 SSE를 구독해 dispatcher의 완료/실패 결과를 stdout으로 흘린다. 단절(접속 실패·비2xx·
/// 스트림 종료·청크 오류) 시 pending을 flush한 뒤 지수 백오프(1s→30s 상한)로 재접속한다
/// (브로커 재기동을 넘어 생존, v2-45 P1). 연속 MAX_CONSECUTIVE_FAILURES회 초과 실패 시에만
/// Err로 종료해 호출부가 exit(1)하게 한다(주소 오타 같은 영구 실패는 Monitor가 죽음으로 알게).
/// digest_secs>0이면 completed는 그 구간 동안 묶어 한 번에 낸다(총괄 wake 절약, v2-44 W5).
/// failed는 digest와 무관하게 즉시 낸다(막힌 task는 총괄 판단이 급하다).
pub async fn run(core: &str, dispatcher: &str, digest_secs: u64) -> Result<(), String> {
    let url = format!("{}/dashboard/events", core.trim_end_matches('/'));
    // connect timeout만 둔다(SSE 바디는 무한정 열려 있어야 하므로 전체 요청 timeout은 두지 않는다).
    // TCP는 붙었는데 응답이 없는 상황(방화벽 drop)에서 send가 무한 대기하는 것을 막는다.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("watch-results: 클라이언트 구성 실패: {e}"))?;
    // seen(dedup)·digest pending은 재접속을 넘어 유지한다(루프 바깥 소유, v2-45 P1 고정 계약).
    let mut state = InboxState { seen: SeenSet::new(), pending: Vec::new(), flush_at: None };
    let mut consecutive_failures: u32 = 0;
    loop {
        let mut connected_at: Option<tokio::time::Instant> = None;
        let reason = run_once(&client, &url, dispatcher, digest_secs, &mut state, &mut connected_at).await;
        // 모든 단절 경로(접속 실패·비2xx·스트림 종료·청크 오류)에서 pending을 먼저 flush한다
        // (digest 묶음 유실 방지). flush했으니 예정 시각도 지운다.
        flush_pending(&mut state.pending);
        state.flush_at = None;
        if connection_was_healthy(connected_at.map(|at| at.elapsed())) {
            consecutive_failures = 0; // 건강했던 접속(수립+최소 생존) 이후의 단절 = 새 실패 연쇄
        }
        consecutive_failures += 1;
        if consecutive_failures > MAX_CONSECUTIVE_FAILURES {
            return Err(format!(
                "watch-results: 연속 {consecutive_failures}회 접속 실패, 재접속 포기(마지막 사유: {reason})"
            ));
        }
        let wait = backoff_secs(consecutive_failures);
        // 재접속 시도·사유는 stderr에만 기록한다(stdout은 RESULT 라인 계약 전용).
        eprintln!(
            "[watch-results] 단절: {reason} → {wait}s 후 재접속 (연속 실패 {consecutive_failures}/{MAX_CONSECUTIVE_FAILURES})"
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
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
        let mut seen = SeenSet::new();
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
        let mut seen = SeenSet::new();
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
        let mut seen = SeenSet::new();
        assert!(parse_result_line(&ev("working", "dashboard", "x", "1", None), "dashboard", &mut seen).is_none());
        assert!(parse_result_line(&ev("completed", "other", "x", "2", None), "dashboard", &mut seen).is_none());
    }

    #[test]
    fn same_task_terminal_reported_once() {
        let mut seen = SeenSet::new();
        let e = ev("completed", "dashboard", "x", "dup1", Some("r"));
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_some());
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_none()); // 두 번째는 dedup
    }

    #[test]
    fn backoff_grows_exponentially_to_cap() {
        // 계약(v2-45 P1): 1s → 2 → 4 → 8 → 16 → 30 상한, 이후 30 유지.
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(4), 8);
        assert_eq!(backoff_secs(5), 16);
        assert_eq!(backoff_secs(6), 30);
        assert_eq!(backoff_secs(7), 30);
        assert_eq!(backoff_secs(u32::MAX), 30); // 오버플로 없이 상한 유지
    }

    #[test]
    fn backoff_zero_failures_is_defensive_min() {
        // 0회는 호출부에서 오지 않지만(항상 실패 후 호출) 방어적으로 최소값 1s.
        assert_eq!(backoff_secs(0), 1);
    }

    #[test]
    fn healthy_connection_needs_establishment_and_min_lifetime() {
        use std::time::Duration;
        // 수립 실패(None)는 생존 시간 개념 자체가 없다 = 실패 연쇄 유지.
        assert!(!connection_was_healthy(None));
        // 수립했어도 즉시 닫히면(크래시루프 브로커) 건강 아님 = 카운터가 계속 쌓여 포기 가능.
        assert!(!connection_was_healthy(Some(Duration::from_secs(1))));
        assert!(!connection_was_healthy(Some(Duration::from_secs(MIN_HEALTHY_SECS - 1))));
        // 수립 시점부터 잰 순수 생존이 최소치를 넘긴 접속만 리셋 근거.
        assert!(connection_was_healthy(Some(Duration::from_secs(MIN_HEALTHY_SECS))));
        assert!(connection_was_healthy(Some(Duration::from_secs(3600))));
    }

    #[test]
    fn seen_set_dedups_and_evicts_oldest_beyond_cap() {
        let mut seen = SeenSet::new();
        assert!(seen.insert("a"));
        assert!(!seen.insert("a"), "같은 id는 dedup");
        // 상한을 넘기면 가장 오래된 id부터 잊는다(무한 성장 방지, 리뷰 반영).
        for i in 0..SEEN_CAP {
            seen.insert(&format!("id-{i}"));
        }
        assert!(seen.set.len() <= SEEN_CAP, "상한 유지");
        assert!(seen.insert("a"), "방출된 가장 오래된 id는 다시 새 것으로 취급");
        assert!(!seen.insert(&format!("id-{}", SEEN_CAP - 1)), "최근 id는 여전히 dedup");
    }

    #[test]
    fn flush_pending_drains_all_lines() {
        let mut pending = vec!["a".to_string(), "b".to_string()];
        flush_pending(&mut pending);
        assert!(pending.is_empty(), "flush 후 pending은 비어야 한다");
        flush_pending(&mut pending); // 빈 상태 재호출도 안전(no-op)
        assert!(pending.is_empty());
    }
}
