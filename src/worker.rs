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
        let msg_marker = " msg=";
        let state_pos = match after_bracket.find(state_marker) {
            Some(p) => p,
            None => continue,
        };
        let after_state = &after_bracket[state_pos + state_marker.len()..];
        let msg_pos = match after_state.find(msg_marker) {
            Some(p) => p,
            None => continue,
        };

        let id = block[1..1 + ID_LEN].to_string();
        let state = after_state[..msg_pos].to_string();
        let msg = after_state[msg_pos + msg_marker.len()..].to_string();

        tasks.push(ParsedTask { id, state, msg });
    }

    tasks
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
    mode: crate::runner::RunMode,
    interval_secs: u64,
    once: bool,
) -> Result<(), String> {
    loop {
        run_one_pass(client, &runner, agent, &model, &project_path, mode).await;

        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// 한 패스(poll -> submitted task들 순회 claim/run/complete)를 수행한다. 항상 정상 반환(에러는 로그만).
async fn run_one_pass(
    client: &McpHttpClient,
    runner: &Arc<dyn Runner + Send + Sync>,
    agent: &str,
    model: &Option<String>,
    project_path: &Option<String>,
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

        let input = RunInput {
            prompt: t.msg.clone(),
            model: model.clone(),
            project_path: project_path.clone(),
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
        let result = match rx.await {
            Ok(Ok(out)) => out.content,
            Ok(Err(e)) => {
                eprintln!("[work] task {} 러너 실패: {e:?}", t.id);
                format!("[runner 에러] {e:?}")
            }
            Err(_canceled) => {
                eprintln!("[work] task {} 러너 스레드 취소(결과 유실)", t.id);
                "[runner 스레드 취소]".to_string()
            }
        };

        match client.complete_task(&t.id, &result).await {
            Ok(_) => eprintln!("[work] task {} complete 완료", t.id),
            Err(e) => eprintln!("[work] task {} complete 실패: {e}", t.id),
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
}
