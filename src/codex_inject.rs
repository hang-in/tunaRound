// codex app-server 라이브 thread에 ws로 유저 턴을 주입하는 클라이언트(`tunaround codex-inject`).
//
// 순수부(thread 경로 계산·승인 판정·메시지별 다음 행동 결정)와 ws IO(접속·송수신 펌프)를 분리한다.
// 프로토콜 요청 빌더/파싱은 T1(src/codex_appserver.rs)을 그대로 재사용하고, 여기서는 그 결과를 어떻게
// 다룰지(자동 accept 전송/텍스트 출력/종료)만 결정한다. 설계 정본
// docs/design/v2-codex-live-supervisor-appserver_2026-07-05.md §4·§5.2·§6.

use std::path::Path;
use std::time::{Duration, Instant};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio_tungstenite::tungstenite::Message;

use crate::codex_appserver::{
    ApprovalPolicy, IncomingMessage, SandboxMode, build_approval_granted, build_elicitation_accept,
    build_initialize_request, build_thread_resume_request, build_thread_start_request,
    build_turn_start_request, classify_message, extract_final_agent_message, is_mcp_elicitation,
    is_turn_completed, parse_thread_id,
};
use crate::config::expand_home;

// ---------------------------------------------------------------------------
// 순수부
// ---------------------------------------------------------------------------

/// `--agent` 별 thread 영속 파일 경로(`~/.tunaround/codex-sup-<agent>.thread`, 설계 §5.1/§6).
pub fn thread_file_path(agent: &str) -> String {
    expand_home(&format!("~/.tunaround/codex-sup-{agent}.thread"))
}

/// `--approval` 문자열을 T1의 [`ApprovalPolicy`]로 파싱한다. 알 수 없는 값은 허용 목록을 담은 Err.
pub fn parse_approval_policy(s: &str) -> Result<ApprovalPolicy, String> {
    match s {
        "untrusted" => Ok(ApprovalPolicy::Untrusted),
        "on-failure" => Ok(ApprovalPolicy::OnFailure),
        "on-request" => Ok(ApprovalPolicy::OnRequest),
        "never" => Ok(ApprovalPolicy::Never),
        other => Err(format!(
            "알 수 없는 --approval 값 {other:?}(untrusted/on-failure/on-request/never 중 하나)"
        )),
    }
}

/// `--sandbox` 문자열을 T1의 [`SandboxMode`]로 파싱한다. 알 수 없는 값은 허용 목록을 담은 Err.
pub fn parse_sandbox_mode(s: &str) -> Result<SandboxMode, String> {
    match s {
        "read-only" => Ok(SandboxMode::ReadOnly),
        "workspace-write" => Ok(SandboxMode::WorkspaceWrite),
        "danger-full-access" => Ok(SandboxMode::DangerFullAccess),
        other => Err(format!(
            "알 수 없는 --sandbox 값 {other:?}(read-only/workspace-write/danger-full-access 중 하나)"
        )),
    }
}

/// 승인류 ServerRequest(설계 §5.2: execCommandApproval/applyPatchApproval/item/commandExecution·
/// fileChange·permissions/requestApproval)인지 판정한다. `mcpServer/elicitation/request`는
/// 별도(`is_mcp_elicitation`)로 처리하므로 여기 포함하지 않는다.
pub fn is_approval_method(method: &str) -> bool {
    matches!(
        method,
        "execCommandApproval"
            | "applyPatchApproval"
            | "item/commandExecution/requestApproval"
            | "item/fileChange/requestApproval"
            | "item/permissions/requestApproval"
    )
}

/// `item/agentMessage/delta` 알림에서 흘려보낼 텍스트를 뽑는다. 정확한 델타 필드명은 P0(stdio) 실측
/// 범위 밖이라 확정되지 않았다(설계 §7 미해소 잔여) - `delta`를 우선 보고, 없으면 `text`로 폴백한다.
/// 라이브 스모크(T5)에서 실제 필드명이 다르면 이 함수만 고치면 된다.
pub fn extract_agent_message_delta(method: &str, params: &Value) -> Option<String> {
    if method != "item/agentMessage/delta" {
        return None;
    }
    params
        .get("delta")
        .and_then(|v| v.as_str())
        .or_else(|| params.get("text").and_then(|v| v.as_str()))
        .map(String::from)
}

/// 들어온 메시지에 대해 injector가 취할 다음 행동(순수 판정, 설계 §4 5단계·§5.2).
#[derive(Debug, Clone, PartialEq)]
pub enum InjectAction {
    /// 이 값을 그대로 ws에 되쏜다(elicitation accept 또는 승인 granted 응답).
    RespondWith(Value),
    /// stdout으로 흘려보낼 텍스트(Monitor 관측용).
    PrintText(String),
    /// 응답하지 않고 stderr로만 남긴다(알 수 없는 ServerRequest, 설계 §5.2).
    LogOnly(String),
    /// 우리 threadId의 turn/completed - 루프 종료(성공).
    Complete,
    /// 무시(다른 thread의 turn/completed, 우리가 보낸 요청의 응답, 그 외 알림).
    Ignore,
}

/// [`IncomingMessage`] + 우리 threadId로부터 다음 행동을 결정한다(순수 함수, ws IO 없음).
pub fn decide_action(msg: &IncomingMessage, our_thread_id: &str) -> InjectAction {
    match msg {
        IncomingMessage::ServerRequest { id, method, .. } => {
            if let Some(req_id) = is_mcp_elicitation(method, id) {
                InjectAction::RespondWith(build_elicitation_accept(&req_id))
            } else if is_approval_method(method) {
                InjectAction::RespondWith(build_approval_granted(id, method))
            } else {
                InjectAction::LogOnly(format!(
                    "알 수 없는 ServerRequest method={method:?}(id={id:?}) - 무응답(설계 §5.2)"
                ))
            }
        }
        IncomingMessage::Notification { method, params } => {
            if let Some(tc) = is_turn_completed(method, params) {
                if tc.thread_id == our_thread_id {
                    InjectAction::Complete
                } else {
                    InjectAction::Ignore
                }
            } else if let Some(text) = extract_final_agent_message(method, params) {
                InjectAction::PrintText(text)
            } else if let Some(delta) = extract_agent_message_delta(method, params) {
                InjectAction::PrintText(delta)
            } else {
                InjectAction::Ignore
            }
        }
        // 우리가 보낸 요청(initialize/thread.start/thread.resume/turn.start)의 응답은 expect_response가
        // 별도로 소비하므로, 일반 펌프 단계에서는 무시한다.
        IncomingMessage::Response { .. } => InjectAction::Ignore,
    }
}

/// thread 영속 파일을 읽어 기존 threadId를 반환한다(없거나 빈 파일이면 None).
fn read_persisted_thread_id(path: &str) -> Option<String> {
    std::fs::read_to_string(path).ok().map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

/// thread 영속 파일에 threadId를 기록한다(상위 디렉터리 없으면 생성).
fn persist_thread_id(path: &str, thread_id: &str) -> Result<(), String> {
    let p = Path::new(path);
    if let Some(parent) = p.parent()
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("codex-inject: {} 디렉터리 생성 실패: {e}", parent.display()))?;
    }
    std::fs::write(p, thread_id).map_err(|e| format!("codex-inject: {path} 쓰기 실패: {e}"))
}

// ---------------------------------------------------------------------------
// ws IO (라이브 전용, 단위테스트 대상 아님 - T5에서 실측)
// ---------------------------------------------------------------------------

type WsStreamInner = tokio_tungstenite::WebSocketStream<
    tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
>;
type WsSink = futures_util::stream::SplitSink<WsStreamInner, Message>;
type WsRead = futures_util::stream::SplitStream<WsStreamInner>;

/// JSON 값 하나를 텍스트 프레임 하나로 전송한다(ws는 프레임 1개=JSON-RPC 객체 1개, 설계 §6 가정).
async fn send_json(sink: &mut WsSink, v: &Value) -> Result<(), String> {
    sink.send(Message::Text(v.to_string()))
        .await
        .map_err(|e| format!("codex-inject: ws 전송 실패: {e}"))
}

/// 다음 텍스트 프레임 하나를 JSON으로 파싱해 반환한다. ping/pong/binary는 건너뛴다.
/// `deadline`을 넘기거나 연결이 끊기면 Err.
async fn recv_json(stream: &mut WsRead, deadline: Instant) -> Result<Value, String> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err("codex-inject: 응답 대기 타임아웃".to_string());
        }
        let next = tokio::time::timeout(remaining, stream.next())
            .await
            .map_err(|_| "codex-inject: 응답 대기 타임아웃".to_string())?
            .ok_or_else(|| "codex-inject: ws 연결이 종료됨".to_string())?
            .map_err(|e| format!("codex-inject: ws 수신 오류: {e}"))?;
        match next {
            Message::Text(t) => {
                return serde_json::from_str(&t)
                    .map_err(|e| format!("codex-inject: JSON 파싱 실패: {e}(원문: {t})"));
            }
            Message::Close(_) => return Err("codex-inject: 서버가 ws 연결을 닫음".to_string()),
            _ => continue,
        }
    }
}

/// ServerRequest/Notification 한 건에 대해 [`decide_action`]대로 실제 IO(응답 전송/텍스트 출력/로그)를
/// 수행하고, 판정 결과를 그대로 돌려준다(호출자가 `Complete`를 감지할 수 있게).
async fn handle_incoming(
    sink: &mut WsSink,
    incoming: &IncomingMessage,
    our_thread_id: &str,
) -> Result<InjectAction, String> {
    let action = decide_action(incoming, our_thread_id);
    match &action {
        InjectAction::RespondWith(v) => send_json(sink, v).await?,
        InjectAction::PrintText(t) => {
            println!("{t}");
            use std::io::Write;
            let _ = std::io::stdout().flush();
        }
        InjectAction::LogOnly(s) => eprintln!("[codex-inject] {s}"),
        InjectAction::Complete | InjectAction::Ignore => {}
    }
    Ok(action)
}

/// `expect_id`에 대한 Response가 올 때까지 대기한다. 그 사이 도착하는 ServerRequest/Notification은
/// [`handle_incoming`]으로 정상 처리(승인 자동응답 등)하고 계속 기다린다.
async fn expect_response(
    sink: &mut WsSink,
    stream: &mut WsRead,
    expect_id: u64,
    deadline: Instant,
) -> Result<Value, String> {
    loop {
        let msg = recv_json(stream, deadline).await?;
        let Some(incoming) = classify_message(&msg) else { continue };
        if let IncomingMessage::Response { id, result, error } = &incoming {
            if *id == json!(expect_id) {
                if let Some(err) = error {
                    return Err(format!("codex-inject: 서버 에러 응답(id={expect_id}): {err}"));
                }
                return Ok(json!({ "result": result.clone().unwrap_or(Value::Null) }));
            }
            // 우리 관심 밖의 id에 대한 응답 - 무시하고 계속 대기.
            continue;
        }
        handle_incoming(sink, &incoming, "").await?;
    }
}

/// `tunaround codex-inject` 본체: ws 접속 -> initialize -> thread 확보(resume|start) -> turn/start ->
/// turn/completed까지 알림 펌프. 성공 시 Ok(()), 타임아웃·프로토콜 에러는 Err(설계 §6 종료코드 계약은
/// main.rs가 이 Result를 process::exit로 변환).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    ws_url: &str,
    agent: &str,
    text: &str,
    approval: ApprovalPolicy,
    sandbox: SandboxMode,
    timeout_secs: u64,
    force_new: bool,
) -> Result<(), String> {
    let (ws_stream, _resp) = tokio_tungstenite::connect_async(ws_url)
        .await
        .map_err(|e| format!("codex-inject: ws 접속 실패({ws_url}): {e}"))?;
    let (mut sink, mut stream) = ws_stream.split();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    let mut next_id: u64 = 0;
    macro_rules! alloc_id {
        () => {{
            next_id += 1;
            next_id
        }};
    }

    // 1. initialize
    let init_id = alloc_id!();
    send_json(&mut sink, &build_initialize_request(init_id, "tunaround-inject", env!("CARGO_PKG_VERSION")))
        .await?;
    expect_response(&mut sink, &mut stream, init_id, deadline).await?;

    // 2. thread 확보(설계 §5.1: 글루가 thread를 소유, 영속 파일로 결정론 유지).
    let thread_path = thread_file_path(agent);
    let existing = if force_new { None } else { read_persisted_thread_id(&thread_path) };
    let cwd = std::env::current_dir().ok().map(|p| p.to_string_lossy().to_string());

    let thread_id = if let Some(tid) = existing {
        let resume_id = alloc_id!();
        send_json(&mut sink, &build_thread_resume_request(resume_id, &tid, Some(approval), Some(sandbox)))
            .await?;
        let resp = expect_response(&mut sink, &mut stream, resume_id, deadline).await?;
        parse_thread_id(&resp).unwrap_or(tid)
    } else {
        let start_id = alloc_id!();
        send_json(
            &mut sink,
            &build_thread_start_request(start_id, Some(approval), Some(sandbox), cwd.as_deref()),
        )
        .await?;
        let resp = expect_response(&mut sink, &mut stream, start_id, deadline).await?;
        let tid = parse_thread_id(&resp)
            .ok_or_else(|| "codex-inject: thread/start 응답에서 thread id 파싱 실패".to_string())?;
        persist_thread_id(&thread_path, &tid)?;
        tid
    };
    eprintln!("[codex-inject] thread={thread_id}로 turn/start 주입");

    // 3. turn/start
    let turn_start_id = alloc_id!();
    send_json(&mut sink, &build_turn_start_request(turn_start_id, &thread_id, text, Some(approval))).await?;

    // 4. turn/completed까지 알림 펌프(승인은 handle_incoming이 자동응답).
    loop {
        let msg = recv_json(&mut stream, deadline).await?;
        let Some(incoming) = classify_message(&msg) else { continue };
        if handle_incoming(&mut sink, &incoming, &thread_id).await? == InjectAction::Complete {
            eprintln!("[codex-inject] turn/completed 수신, 종료");
            return Ok(());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_appserver::TurnCompleted;

    // -- thread_file_path ----------------------------------------------------

    #[test]
    fn thread_file_path_uses_agent_scoped_filename() {
        let path = thread_file_path("win-codex-sup");
        assert!(
            path.ends_with(".tunaround/codex-sup-win-codex-sup.thread"),
            "예상 접미와 다름: {path}"
        );
    }

    // -- 정책 파싱 -------------------------------------------------------------

    #[test]
    fn parse_approval_policy_accepts_all_variants() {
        assert_eq!(parse_approval_policy("untrusted"), Ok(ApprovalPolicy::Untrusted));
        assert_eq!(parse_approval_policy("on-failure"), Ok(ApprovalPolicy::OnFailure));
        assert_eq!(parse_approval_policy("on-request"), Ok(ApprovalPolicy::OnRequest));
        assert_eq!(parse_approval_policy("never"), Ok(ApprovalPolicy::Never));
    }

    #[test]
    fn parse_approval_policy_rejects_unknown() {
        assert!(parse_approval_policy("yolo").is_err());
    }

    #[test]
    fn parse_sandbox_mode_accepts_all_variants() {
        assert_eq!(parse_sandbox_mode("read-only"), Ok(SandboxMode::ReadOnly));
        assert_eq!(parse_sandbox_mode("workspace-write"), Ok(SandboxMode::WorkspaceWrite));
        assert_eq!(parse_sandbox_mode("danger-full-access"), Ok(SandboxMode::DangerFullAccess));
    }

    #[test]
    fn parse_sandbox_mode_rejects_unknown() {
        assert!(parse_sandbox_mode("full-yolo").is_err());
    }

    // -- is_approval_method ----------------------------------------------------

    #[test]
    fn is_approval_method_matches_known_methods() {
        assert!(is_approval_method("execCommandApproval"));
        assert!(is_approval_method("applyPatchApproval"));
        assert!(is_approval_method("item/commandExecution/requestApproval"));
        assert!(is_approval_method("item/fileChange/requestApproval"));
        assert!(is_approval_method("item/permissions/requestApproval"));
    }

    #[test]
    fn is_approval_method_rejects_elicitation_and_unknown() {
        // elicitation은 별도 경로(is_mcp_elicitation)로 처리하므로 승인류가 아니다.
        assert!(!is_approval_method("mcpServer/elicitation/request"));
        assert!(!is_approval_method("turn/completed"));
        assert!(!is_approval_method("some/unknown/method"));
    }

    // -- extract_agent_message_delta --------------------------------------------

    #[test]
    fn extract_agent_message_delta_prefers_delta_field() {
        let params = json!({ "delta": "안녕", "text": "무시됨" });
        assert_eq!(
            extract_agent_message_delta("item/agentMessage/delta", &params).as_deref(),
            Some("안녕")
        );
    }

    #[test]
    fn extract_agent_message_delta_falls_back_to_text_field() {
        let params = json!({ "text": "폴백 텍스트" });
        assert_eq!(
            extract_agent_message_delta("item/agentMessage/delta", &params).as_deref(),
            Some("폴백 텍스트")
        );
    }

    #[test]
    fn extract_agent_message_delta_none_for_other_methods() {
        let params = json!({ "delta": "안녕" });
        assert_eq!(extract_agent_message_delta("item/completed", &params), None);
    }

    // -- decide_action -----------------------------------------------------------

    #[test]
    fn decide_action_elicitation_responds_with_accept() {
        let msg = IncomingMessage::ServerRequest {
            id: json!(42),
            method: "mcpServer/elicitation/request".to_string(),
            params: json!({}),
        };
        assert_eq!(
            decide_action(&msg, "tid-1"),
            InjectAction::RespondWith(build_elicitation_accept(&json!(42)))
        );
    }

    #[test]
    fn decide_action_approval_method_responds_with_granted() {
        let msg = IncomingMessage::ServerRequest {
            id: json!(7),
            method: "execCommandApproval".to_string(),
            params: json!({}),
        };
        assert_eq!(
            decide_action(&msg, "tid-1"),
            InjectAction::RespondWith(build_approval_granted(&json!(7), "execCommandApproval"))
        );
    }

    #[test]
    fn decide_action_unknown_server_request_logs_only() {
        let msg = IncomingMessage::ServerRequest {
            id: json!(9),
            method: "item/tool/requestUserInput".to_string(),
            params: json!({}),
        };
        match decide_action(&msg, "tid-1") {
            InjectAction::LogOnly(s) => assert!(s.contains("item/tool/requestUserInput")),
            other => panic!("LogOnly를 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn decide_action_turn_completed_matching_thread_completes() {
        let msg = IncomingMessage::Notification {
            method: "turn/completed".to_string(),
            params: json!({ "threadId": "tid-1", "turnId": "turn-9" }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Complete);
    }

    #[test]
    fn decide_action_turn_completed_other_thread_ignored() {
        let msg = IncomingMessage::Notification {
            method: "turn/completed".to_string(),
            params: json!({ "threadId": "tid-OTHER", "turnId": "turn-9" }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
        // is_turn_completed 자체 파싱도 재확인(회귀 방지 겸용).
        assert_eq!(
            is_turn_completed("turn/completed", &json!({"threadId":"tid-OTHER","turnId":"turn-9"})),
            Some(TurnCompleted { thread_id: "tid-OTHER".to_string(), turn_id: "turn-9".to_string() })
        );
    }

    #[test]
    fn decide_action_final_agent_message_prints_text() {
        let msg = IncomingMessage::Notification {
            method: "item/completed".to_string(),
            params: json!({
                "item": { "type": "agentMessage", "id": "m1", "text": "완료 답변", "phase": "final_answer" }
            }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::PrintText("완료 답변".to_string()));
    }

    #[test]
    fn decide_action_agent_message_delta_prints_text() {
        let msg = IncomingMessage::Notification {
            method: "item/agentMessage/delta".to_string(),
            params: json!({ "delta": "진행중..." }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::PrintText("진행중...".to_string()));
    }

    #[test]
    fn decide_action_other_notification_ignored() {
        let msg = IncomingMessage::Notification {
            method: "turn/started".to_string(),
            params: json!({ "threadId": "tid-1", "turnId": "turn-9" }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
    }

    #[test]
    fn decide_action_response_is_ignored() {
        let msg = IncomingMessage::Response { id: json!(1), result: Some(json!({})), error: None };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
    }

    // -- thread 영속 파일 IO ------------------------------------------------------

    #[test]
    fn persist_and_read_thread_id_roundtrip() {
        let dir = std::env::temp_dir().join(format!("tunaround-codex-inject-test-{}", std::process::id()));
        let path = dir.join("nested").join("agent.thread");
        let path_str = path.to_string_lossy().to_string();

        assert_eq!(read_persisted_thread_id(&path_str), None, "파일이 없으면 None");

        persist_thread_id(&path_str, "019f2f6d-thread-id").expect("중첩 디렉터리도 생성돼야 함");
        assert_eq!(read_persisted_thread_id(&path_str).as_deref(), Some("019f2f6d-thread-id"));

        // 정리(테스트 오염 방지, 실패해도 무해).
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_persisted_thread_id_treats_blank_file_as_none() {
        let dir = std::env::temp_dir().join(format!("tunaround-codex-inject-blank-{}", std::process::id()));
        let path = dir.join("agent.thread");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "   \n").unwrap();
        assert_eq!(read_persisted_thread_id(&path.to_string_lossy()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
