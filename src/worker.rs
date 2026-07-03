// A2A 워커 데몬: poll_tasks 텍스트 파싱 + poll->claim->runner.run->complete 루프.

use std::sync::Arc;
use std::time::Duration;

use crate::mcp_client::McpHttpClient;
use crate::runner::{RunInput, Runner};

/// task id의 고정 길이(SqliteStore::new_task_id = lower(hex(randomblob(16))) = 32 hex chars).
const ID_LEN: usize = 32;

/// poll_tasks 텍스트 한 블록에서 뽑아낸 필드(from_agent는 워커 루프에 불필요해 생략).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTask {
    pub id: String,
    pub state: String,
    /// A2A context_id(프로젝트별 라우팅 키). poll에 `ctx=-`이거나 없으면 None.
    pub context_id: Option<String>,
    pub msg: String,
}

fn is_hex32(s: &str) -> bool {
    s.len() == ID_LEN && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// 텍스트에서 블록 헤더(`[<32hex>] from=...`)가 시작하는 바이트 오프셋을 모두 찾는다.
/// `format_open_tasks`(src/mcp.rs)는 블록을 `"\n\n"`로 join하므로, 헤더는 문자열 맨 앞이거나
/// 직전 두 글자가 `"\n\n"`일 때만 유효하다고 본다(메시지 본문 안의 우연한 개행과 구분).
fn find_header_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'[' {
            continue;
        }
        let at_start = i == 0;
        let after_blank_line = i >= 2 && &text[i - 2..i] == "\n\n";
        if !(at_start || after_blank_line) {
            continue;
        }
        let rest = &text[i + 1..];
        if rest.len() < ID_LEN + "] from=".len() {
            continue;
        }
        if !is_hex32(&rest[..ID_LEN]) {
            continue;
        }
        if !rest[ID_LEN..].starts_with("] from=") {
            continue;
        }
        starts.push(i);
    }
    starts
}

/// poll_tasks(agent) 응답 텍스트를 파싱해 각 task 블록을 구조체로 반환한다.
/// 빈 목록 안내 문구(`"... 앞 열린 task 없음"`)를 포함하면 빈 Vec을 반환한다.
pub fn parse_open_tasks(poll_text: &str) -> Vec<ParsedTask> {
    if poll_text.contains("앞 열린 task 없음") {
        return Vec::new();
    }

    let starts = find_header_starts(poll_text);
    let mut tasks = Vec::with_capacity(starts.len());

    for (idx, &start) in starts.iter().enumerate() {
        // 다음 블록 헤더 직전의 "\n\n" 구분자는 이 블록의 msg에서 제외한다.
        let block_end = starts.get(idx + 1).map(|&next| next - 2).unwrap_or(poll_text.len());
        let block = &poll_text[start..block_end];

        // block = "[<32hex id>] from=<from_agent> state=<state> msg=<msg...>"
        let after_bracket = match block[1 + ID_LEN..].strip_prefix("] from=") {
            Some(s) => s,
            None => continue,
        };
        let state_marker = " state=";
        let ctx_marker = " ctx=";
        let msg_marker = " msg=";
        let state_pos = match after_bracket.find(state_marker) {
            Some(p) => p,
            None => continue,
        };
        let after_state = &after_bracket[state_pos + state_marker.len()..];
        // msg를 앵커로 삼는다(항상 있음). state와 msg 사이의 " ctx=<id>"는 선택적으로 처리해
        // 구 포맷(ctx 없음)과도 호환한다.
        let msg_pos = match after_state.find(msg_marker) {
            Some(p) => p,
            None => continue,
        };
        let between = &after_state[..msg_pos]; // "submitted ctx=projA" 또는 "submitted"
        let msg = after_state[msg_pos + msg_marker.len()..].to_string();
        let (state, context_id) = match between.find(ctx_marker) {
            Some(cp) => {
                let state = between[..cp].to_string();
                let ctx_raw = &between[cp + ctx_marker.len()..];
                let context_id = if ctx_raw == "-" { None } else { Some(ctx_raw.to_string()) };
                (state, context_id)
            }
            None => (between.to_string(), None),
        };

        let id = block[1..1 + ID_LEN].to_string();
        tasks.push(ParsedTask { id, state, context_id, msg });
    }

    tasks
}

/// task의 context_id를 `--context-map`에서 찾아 실행할 project-path를 정한다(순수 함수).
/// 매핑에 있으면 그 경로, 없거나 context_id가 없으면 기본 project-path로 폴백한다.
pub fn resolve_project_path(
    context_id: Option<&str>,
    context_map: &std::collections::HashMap<String, String>,
    default_path: Option<&str>,
) -> Option<String> {
    context_id
        .and_then(|c| context_map.get(c))
        .cloned()
        .or_else(|| default_path.map(|s| s.to_string()))
}

/// `--context-map` 문자열("k=v,k=v")을 context_id->project-path 맵으로 파싱한다(순수 함수).
/// 형식 오류(= 없음)·빈 key·빈 value·중복 key는 조용히 버리지 않고 Err로 거부한다. 오타 항목이
/// 조용히 사라져 기본 project-path로 폴백되면 --write 시 엉뚱한 레포를 고칠 수 있어서다. 완전히 빈
/// 항목(후행 콤마 등)만 무해하게 건너뛴다.
pub fn parse_context_map(spec: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    for entry in spec.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (k, v) = entry
            .split_once('=')
            .ok_or_else(|| format!("--context-map 항목이 'key=value' 형식이 아닙니다: {entry:?}"))?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || v.is_empty() {
            return Err(format!("--context-map 항목의 key 또는 value가 비어있습니다: {entry:?}"));
        }
        if let Some(prev) = map.insert(k.to_string(), v.to_string()) {
            return Err(format!("--context-map에 중복 key '{k}'가 있습니다(이전 값 {prev:?})"));
        }
    }
    Ok(map)
}

/// 워커 한 패스: poll -> (submitted만) claim -> runner.run -> complete.
/// `once=true`면 한 패스 후 반환, 아니면 `interval_secs` 간격으로 무한 루프한다.
/// poll/claim/complete 실패는 eprintln 로그 후 그 task만 건너뛰고 루프는 죽지 않는다.
/// 인자 8개는 work 서브커맨드 옵션을 그대로 투영한 것이라(WorkArgs 필드 1:1), 별도 struct로
/// 묶기보다 이 시그니처를 유지한다(설계문서 §2.2 계약).
#[allow(clippy::too_many_arguments)]
pub async fn run_worker_loop(
    client: &McpHttpClient,
    runner: Arc<dyn Runner + Send + Sync>,
    agent: &str,
    model: Option<String>,
    project_path: Option<String>,
    context_map: std::collections::HashMap<String, String>,
    mode: crate::runner::RunMode,
    interval_secs: u64,
    once: bool,
) -> Result<(), String> {
    loop {
        run_one_pass(client, &runner, agent, &model, &project_path, &context_map, mode).await;

        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// 감시 전용 루프: agent 앞 새 submitted task만 stdout에 한 줄씩 알리고 claim은 하지 않는다.
/// Claude Code 세션이 이 커맨드를 Monitor로 감싸면, task 도착이 이벤트로 세션을 깨워 스스로
/// claim/처리하게 할 수 있다(감독 레인을 유휴 0토큰으로 운용). 이미 알린 id는 HashSet으로 디듑한다
/// (task는 claim 전까지 submitted로 남아 매 폴마다 재등장하므로 중복 알림을 막는다).
pub async fn run_poll_loop(
    client: &McpHttpClient,
    agent: &str,
    interval_secs: u64,
    once: bool,
) -> Result<(), String> {
    use std::io::Write;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    loop {
        match client.poll_tasks(agent).await {
            Ok(text) => {
                for t in parse_open_tasks(&text).into_iter().filter(|t| t.state == "submitted") {
                    if seen.insert(t.id.clone()) {
                        // Monitor 이벤트 = stdout 한 줄. 파이프는 블록 버퍼라 flush로 즉시 전달한다.
                        let preview: String =
                            t.msg.chars().take(80).collect::<String>().replace('\n', " ");
                        println!("TASK {} :: {preview}", t.id);
                        let _ = std::io::stdout().flush();
                    }
                }
            }
            // 폴 실패는 이벤트가 아니라 stderr로(Monitor 이벤트 오염 방지). 루프는 죽지 않는다.
            Err(e) => eprintln!("[poll] poll_tasks 실패: {e}"),
        }
        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// 한 패스(poll -> submitted task들 순회 claim/run/complete)를 수행한다. 항상 정상 반환(에러는 로그만).
#[allow(clippy::too_many_arguments)]
async fn run_one_pass(
    client: &McpHttpClient,
    runner: &Arc<dyn Runner + Send + Sync>,
    agent: &str,
    model: &Option<String>,
    project_path: &Option<String>,
    context_map: &std::collections::HashMap<String, String>,
    mode: crate::runner::RunMode,
) {
    let poll_text = match client.poll_tasks(agent).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[work] poll_tasks 실패: {e}");
            return;
        }
    };

    let tasks = parse_open_tasks(&poll_text);
    for t in tasks.iter().filter(|t| t.state == "submitted") {
        eprintln!("[work] task {} claim 시도", t.id);
        if let Err(e) = client.claim_task(&t.id).await {
            eprintln!("[work] task {} claim 실패: {e}", t.id);
            continue;
        }

        // 프로젝트 라우팅: task의 context_id가 --context-map에 있으면 그 project-path로 실행하고,
        // 없으면 기본 --project-path로 폴백한다. 데몬 하나가 여러 프로젝트를 배분할 수 있다.
        let resolved_project =
            resolve_project_path(t.context_id.as_deref(), context_map, project_path.as_deref());
        if let Some(cid) = t.context_id.as_deref()
            && let Some(p) = context_map.get(cid)
        {
            eprintln!("[work] task {} context={cid} -> project-path {p}", t.id);
        }
        let input = RunInput {
            prompt: t.msg.clone(),
            model: model.clone(),
            project_path: resolved_project,
            mode,
            pull: false,
        };
        // 러너는 sync이고 일부(OpenAiChatRunner)는 내부에서 reqwest::blocking을 쓴다. tokio의
        // spawn_blocking 스레드는 Handle::current()가 살아 있어 reqwest::blocking이 "런타임 안에서
        // blocking 불가"로 거부한다. 그래서 런타임 핸들이 전혀 없는 순수 std 스레드에서 러너를 돌린다
        // (subprocess 러너 claude/codex도 std 스레드에서 정상 동작).
        let runner2 = Arc::clone(runner);
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(runner2.run(&input));
        });
        // 성공 -> complete_task(결과 artifact, state=completed). 실패 -> fail_task(사유, state=failed).
        // 실패를 completed로 위장하지 않아 dispatcher가 성패를 구분하고 재시도를 판단할 수 있다.
        match rx.await {
            Ok(Ok(out)) => match client.complete_task(&t.id, &out.content).await {
                Ok(_) => eprintln!("[work] task {} complete 완료", t.id),
                Err(e) => eprintln!("[work] task {} complete 실패: {e}", t.id),
            },
            Ok(Err(e)) => {
                eprintln!("[work] task {} 러너 실패: {e:?}", t.id);
                let reason = format!("러너 실행 실패: {e:?}");
                if let Err(fe) = client.fail_task(&t.id, &reason).await {
                    eprintln!("[work] task {} fail 처리 실패: {fe}", t.id);
                }
            }
            Err(_canceled) => {
                eprintln!("[work] task {} 러너 스레드 취소(결과 유실)", t.id);
                if let Err(fe) = client.fail_task(&t.id, "러너 스레드 취소(결과 유실)").await {
                    eprintln!("[work] task {} fail 처리 실패: {fe}", t.id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_open_tasks_empty_message_returns_empty_vec() {
        let tasks = parse_open_tasks("mac-claude 앞 열린 task 없음");
        assert!(tasks.is_empty());
    }

    #[test]
    fn parse_open_tasks_single_task() {
        let id = "a".repeat(32);
        let text = format!("[{id}] from=win-claude state=submitted msg=리뷰 부탁");
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].id, id);
        assert_eq!(tasks[0].state, "submitted");
        assert_eq!(tasks[0].msg, "리뷰 부탁");
    }

    #[test]
    fn parse_open_tasks_multiple_tasks_joined_by_blank_line() {
        let id1 = "1".repeat(32);
        let id2 = "2".repeat(32);
        let text = format!(
            "[{id1}] from=win-claude state=submitted msg=첫 task\n\n[{id2}] from=win-claude state=working msg=둘째 task"
        );
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 2);
        assert_eq!(tasks[0].id, id1);
        assert_eq!(tasks[0].msg, "첫 task");
        assert_eq!(tasks[1].id, id2);
        assert_eq!(tasks[1].state, "working");
        assert_eq!(tasks[1].msg, "둘째 task");
    }

    #[test]
    fn parse_open_tasks_msg_with_embedded_newlines() {
        let id = "3".repeat(32);
        let text = format!("[{id}] from=win-claude state=submitted msg=1번\n2번\n\n3번(빈 줄 포함)");
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].msg, "1번\n2번\n\n3번(빈 줄 포함)");
    }

    #[test]
    fn parse_open_tasks_submitted_and_working_mixed() {
        let id1 = "4".repeat(32);
        let id2 = "5".repeat(32);
        let id3 = "6".repeat(32);
        let text = format!(
            "[{id1}] from=a state=submitted msg=하나\n\n[{id2}] from=a state=working msg=둘\n\n[{id3}] from=a state=submitted msg=셋"
        );
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 3);
        let submitted: Vec<_> = tasks.iter().filter(|t| t.state == "submitted").collect();
        assert_eq!(submitted.len(), 2);
        assert_eq!(submitted[0].id, id1);
        assert_eq!(submitted[1].id, id3);
    }

    #[test]
    fn parse_open_tasks_extracts_context_id() {
        let id = "7".repeat(32);
        let text = format!("[{id}] from=disp state=submitted ctx=projA msg=작업 지시");
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].context_id.as_deref(), Some("projA"));
        assert_eq!(tasks[0].state, "submitted");
        assert_eq!(tasks[0].msg, "작업 지시");
    }

    #[test]
    fn parse_open_tasks_ctx_dash_is_none() {
        let id = "8".repeat(32);
        let text = format!("[{id}] from=disp state=submitted ctx=- msg=작업");
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].context_id, None);
        assert_eq!(tasks[0].msg, "작업");
    }

    #[test]
    fn parse_open_tasks_no_ctx_marker_is_backward_compatible() {
        // 구 포맷(ctx= 없음)도 context_id=None으로 그대로 파싱된다.
        let id = "9".repeat(32);
        let text = format!("[{id}] from=disp state=submitted msg=구포맷");
        let tasks = parse_open_tasks(&text);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].context_id, None);
        assert_eq!(tasks[0].msg, "구포맷");
    }

    #[test]
    fn resolve_project_path_uses_map_then_falls_back() {
        let mut map = std::collections::HashMap::new();
        map.insert("projA".to_string(), "/repos/A".to_string());
        // 매핑에 있으면 그 경로.
        assert_eq!(resolve_project_path(Some("projA"), &map, Some("/default")), Some("/repos/A".to_string()));
        // context_id가 매핑에 없으면 기본값.
        assert_eq!(resolve_project_path(Some("projX"), &map, Some("/default")), Some("/default".to_string()));
        // context_id 자체가 없으면 기본값.
        assert_eq!(resolve_project_path(None, &map, Some("/default")), Some("/default".to_string()));
        // 매핑도 기본값도 없으면 None.
        assert_eq!(resolve_project_path(Some("projX"), &map, None), None);
    }

    #[test]
    fn parse_context_map_valid_entries() {
        let m = parse_context_map("projA=/repos/A, projB=/repos/B").unwrap();
        assert_eq!(m.get("projA").map(String::as_str), Some("/repos/A"));
        assert_eq!(m.get("projB").map(String::as_str), Some("/repos/B"));
        assert_eq!(m.len(), 2);
    }

    #[test]
    fn parse_context_map_trailing_comma_ok() {
        // 완전히 빈 항목(후행 콤마)만 무해하게 스킵한다.
        let m = parse_context_map("projA=/repos/A,").unwrap();
        assert_eq!(m.len(), 1);
    }

    #[test]
    fn parse_context_map_rejects_malformed_entry() {
        // '=' 없는 오타 항목은 조용히 버리지 않고 거부한다(기본 레포 오폴백 방지).
        assert!(parse_context_map("projA=/repos/A,badentry").is_err());
    }

    #[test]
    fn parse_context_map_rejects_empty_key_or_value() {
        assert!(parse_context_map("=/repos/A").is_err());
        assert!(parse_context_map("projA=").is_err());
    }

    #[test]
    fn parse_context_map_rejects_duplicate_key() {
        assert!(parse_context_map("projA=/x,projA=/y").is_err());
    }
}
