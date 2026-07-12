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

/// agent 값을 파일명 안전 문자로 정규화한다. 정상 id(win-codex-sup 등 영숫자+`-`+`_`)는 불변이고,
/// `/`·`..` 같은 경로 문자는 `_`로 치환해 `~/.tunaround` 네임스페이스 밖으로 새거나 임의 하위 디렉터리를
/// 만들지 못하게 한다(경로 탈출 방지, 리뷰 지적).
fn safe_agent_filename(agent: &str) -> String {
    let safe: String = agent
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "default".to_string()
    } else {
        safe
    }
}

/// `--agent` 별 thread 영속 파일 경로(`~/.tunaround/codex-sup-<agent>.thread`, 설계 §5.1/§6).
pub fn thread_file_path(agent: &str) -> String {
    let agent = safe_agent_filename(agent);
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
    /// 실패 신호(에러 응답 또는 우리 thread의 error 알림) - 루프를 즉시 Err로 끝낸다.
    /// 이게 없으면 turn/start 거부·턴 오류가 조용히 무시되어 --timeout 전부를 블록한 뒤 원인 없는
    /// 타임아웃만 남는다(리뷰 findings).
    Fail(String),
    /// 무시(다른 thread의 알림, 우리가 보낸 요청의 정상 응답 등).
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
            // error 알림(`{error, threadId, turnId, willRetry}`) = 턴 실패 신호. 우리 thread거나
            // thread 불명이면 즉시 실패로 surface해 --timeout 블록 없이 원인을 남긴다(리뷰 findings).
            if method == "error" {
                let ours = params
                    .get("threadId")
                    .and_then(|v| v.as_str())
                    .is_none_or(|t| t == our_thread_id);
                if ours {
                    let e = params
                        .get("error")
                        .map(std::string::ToString::to_string)
                        .unwrap_or_default();
                    let retry = params
                        .get("willRetry")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false);
                    return InjectAction::Fail(format!(
                        "codex error 알림: {e} (willRetry={retry})"
                    ));
                }
                return InjectAction::Ignore;
            }
            if let Some(tc) = is_turn_completed(method, params) {
                if tc.thread_id == our_thread_id {
                    InjectAction::Complete
                } else {
                    InjectAction::Ignore
                }
            } else if params.get("threadId").and_then(|v| v.as_str()) == Some(our_thread_id) {
                // 우리 thread의 최종 답변만 stdout으로. threadId로 거르지 않으면 같은 ws에 다른 thread가
                // 붙어 있을 때(사람 --remote 관전 등) 그쪽 답변이 우리 task 결과로 오염될 수 있다(findings).
                if let Some(text) = extract_final_agent_message(method, params) {
                    InjectAction::PrintText(text)
                } else {
                    InjectAction::Ignore
                }
            } else {
                InjectAction::Ignore
            }
        }
        // 우리가 보낸 요청의 응답: 정상(error 없음)은 expect_response가 소비하거나 무시하면 되지만,
        // 에러 응답(turn/start 거부 등)은 fire-and-forget이라 여기서만 잡히므로 즉시 실패로 surface한다.
        IncomingMessage::Response { error, .. } => match error {
            Some(e) => InjectAction::Fail(format!("codex 에러 응답: {e}")),
            None => InjectAction::Ignore,
        },
    }
}

/// thread 영속 파일을 읽어 기존 threadId를 반환한다(없거나 빈 파일이면 None).
fn read_persisted_thread_id(path: &str) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
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

type WsStreamInner =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;
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
        InjectAction::Fail(e) => eprintln!("[codex-inject] 실패 신호: {e}"),
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
        let Some(incoming) = classify_message(&msg) else {
            continue;
        };
        if let IncomingMessage::Response { id, result, error } = &incoming {
            // 에러 응답은 id 일치 여부와 무관하게 즉시 실패로 surface한다(id:null 에러 응답이나 프록시가
            // id 타입을 바꿔 에코한 경우도 swallow되지 않게, 리뷰 findings).
            if let Some(err) = error {
                return Err(format!("codex-inject: 서버 에러 응답(id={id:?}): {err}"));
            }
            if *id == json!(expect_id) {
                return Ok(json!({ "result": result.clone().unwrap_or(Value::Null) }));
            }
            // 우리 관심 밖의 id에 대한 정상 응답 - 무시하고 계속 대기.
            continue;
        }
        // 핸드셰이크 중 error 알림 등 실패 신호가 오면 즉시 Err로 끝낸다.
        if let InjectAction::Fail(e) = handle_incoming(sink, &incoming, "").await? {
            return Err(format!("codex-inject: {e}"));
        }
    }
}

/// thread/start를 보내 새 thread를 만들고 threadId를 영속화해 반환한다. else 분기와 resume 실패 시
/// 자가치유가 공유한다(중복 제거). `next_id`는 호출자의 id 카운터를 그대로 증가시킨다.
#[allow(clippy::too_many_arguments)]
async fn start_thread(
    sink: &mut WsSink,
    stream: &mut WsRead,
    next_id: &mut u64,
    approval: ApprovalPolicy,
    sandbox: SandboxMode,
    cwd: Option<&str>,
    deadline: Instant,
    thread_path: &str,
) -> Result<String, String> {
    *next_id += 1;
    let start_id = *next_id;
    send_json(
        sink,
        &build_thread_start_request(start_id, Some(approval), Some(sandbox), cwd),
    )
    .await?;
    let resp = expect_response(sink, stream, start_id, deadline).await?;
    let tid = parse_thread_id(&resp)
        .ok_or_else(|| "codex-inject: thread/start 응답에서 thread id 파싱 실패".to_string())?;
    persist_thread_id(thread_path, &tid)?;
    Ok(tid)
}

/// `tunaround codex-inject` 본체: ws 접속 -> initialize -> thread 확보(resume|start) -> turn/start ->
/// turn/completed까지 알림 펌프. 성공 시 Ok(()), 타임아웃·프로토콜 에러는 Err(설계 §6 종료코드 계약은
/// main.rs가 이 Result를 process::exit로 변환).
///
/// `thread`가 Some이면 직지정 모드(v2-46): 영속 파일을 읽지도 쓰지도 않고 그 threadId를 resume만 한다.
/// resume 실패 시 새 thread 자가치유 없이 Err - 엉뚱한 thread에 답이 생기는 것을 막고, 호출자(codex-relay)가
/// fail_task로 전환한다. None이면 기존 `--agent` 파일 모드(불변).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    ws_url: &str,
    agent: &str,
    thread: Option<&str>,
    text: &str,
    approval: ApprovalPolicy,
    sandbox: SandboxMode,
    timeout_secs: u64,
    force_new: bool,
) -> Result<String, String> {
    // deadline을 먼저 잡고 connect_async도 같은 예산으로 감싼다(연결 단계가 무한정 대기하지 않게, 리뷰 지적).
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let (ws_stream, _resp) = tokio::time::timeout(
        Duration::from_secs(timeout_secs),
        tokio_tungstenite::connect_async(ws_url),
    )
    .await
    .map_err(|_| format!("codex-inject: ws 접속 타임아웃({timeout_secs}s, {ws_url})"))?
    .map_err(|e| format!("codex-inject: ws 접속 실패({ws_url}): {e}"))?;
    let (mut sink, mut stream) = ws_stream.split();

    let mut next_id: u64 = 0;
    macro_rules! alloc_id {
        () => {{
            next_id += 1;
            next_id
        }};
    }

    // 1. initialize
    let init_id = alloc_id!();
    send_json(
        &mut sink,
        &build_initialize_request(init_id, "tunaround-inject", env!("CARGO_PKG_VERSION")),
    )
    .await?;
    expect_response(&mut sink, &mut stream, init_id, deadline).await?;

    // 2. thread 확보. 직지정 모드(v2-46)는 지정 threadId resume만(영속 파일·자가치유 없음),
    //    파일 모드는 기존 그대로(설계 §5.1: 글루가 thread를 소유, 영속 파일로 결정론 유지).
    if let Some(tid) = thread {
        let resume_id = alloc_id!();
        send_json(
            &mut sink,
            &build_thread_resume_request(resume_id, tid, Some(approval), Some(sandbox)),
        )
        .await?;
        let resp = expect_response(&mut sink, &mut stream, resume_id, deadline)
            .await
            .map_err(|e| format!("codex-inject: --thread {tid} resume 실패(자가치유 없음): {e}"))?;
        let thread_id = parse_thread_id(&resp).unwrap_or_else(|| tid.to_string());
        return pump_turn(
            &mut sink,
            &mut stream,
            &mut next_id,
            &thread_id,
            text,
            approval,
            deadline,
        )
        .await;
    }
    let thread_path = thread_file_path(agent);
    let existing = if force_new {
        None
    } else {
        read_persisted_thread_id(&thread_path)
    };
    let cwd = std::env::current_dir()
        .ok()
        .map(|p| p.to_string_lossy().to_string());

    let thread_id = if let Some(tid) = existing {
        let resume_id = alloc_id!();
        send_json(
            &mut sink,
            &build_thread_resume_request(resume_id, &tid, Some(approval), Some(sandbox)),
        )
        .await?;
        match expect_response(&mut sink, &mut stream, resume_id, deadline).await {
            Ok(resp) => parse_thread_id(&resp).unwrap_or_else(|| {
                eprintln!(
                    "[codex-inject] thread/resume 응답에서 thread id 파싱 실패, 기존 {tid} 재사용"
                );
                tid.clone()
            }),
            // 서버 재기동·thread 만료로 죽은 threadId면 resume이 에러다. 수동 개입(.thread 삭제) 없이
            // 새 thread로 자가치유한다(리뷰 findings: gemini high + CodeRabbit).
            Err(e) => {
                eprintln!("[codex-inject] thread/resume 실패({e}), 새 thread 시작(자가치유)");
                start_thread(
                    &mut sink,
                    &mut stream,
                    &mut next_id,
                    approval,
                    sandbox,
                    cwd.as_deref(),
                    deadline,
                    &thread_path,
                )
                .await?
            }
        }
    } else {
        start_thread(
            &mut sink,
            &mut stream,
            &mut next_id,
            approval,
            sandbox,
            cwd.as_deref(),
            deadline,
            &thread_path,
        )
        .await?
    };
    pump_turn(
        &mut sink,
        &mut stream,
        &mut next_id,
        &thread_id,
        text,
        approval,
        deadline,
    )
    .await
}

/// turn/start 전송 -> turn/completed까지 알림 펌프(승인은 handle_incoming이 자동응답).
/// 최종답(PrintText)을 누적해 반환한다 - CLI는 handle_incoming이 이미 stdout에 출력하고,
/// codex-relay는 이 반환값을 결과로 쓴다.
/// thread 확보 두 경로(파일 모드·--thread 직지정)가 공유한다(v2-46).
async fn pump_turn(
    sink: &mut WsSink,
    stream: &mut WsRead,
    next_id: &mut u64,
    thread_id: &str,
    text: &str,
    approval: ApprovalPolicy,
    deadline: Instant,
) -> Result<String, String> {
    eprintln!("[codex-inject] thread={thread_id}로 turn/start 주입");
    *next_id += 1;
    send_json(
        sink,
        &build_turn_start_request(*next_id, thread_id, text, Some(approval)),
    )
    .await?;

    let mut answer = String::new();
    loop {
        let msg = recv_json(stream, deadline).await?;
        let Some(incoming) = classify_message(&msg) else {
            continue;
        };
        match handle_incoming(sink, &incoming, thread_id).await? {
            InjectAction::Complete => {
                eprintln!("[codex-inject] turn/completed 수신, 종료");
                return Ok(answer);
            }
            InjectAction::Fail(e) => return Err(format!("codex-inject: 턴 실패: {e}")),
            InjectAction::PrintText(t) => {
                if !answer.is_empty() {
                    answer.push('\n');
                }
                answer.push_str(&t);
            }
            _ => {}
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

    #[test]
    fn safe_agent_filename_blocks_path_traversal() {
        assert_eq!(safe_agent_filename("win-codex-sup"), "win-codex-sup"); // 정상 id는 불변
        assert_eq!(safe_agent_filename("a_b-1"), "a_b-1");
        assert_eq!(safe_agent_filename(""), "default");
        // 경로 문자는 전부 _로 치환되어 네임스페이스를 못 벗어난다.
        let evil = safe_agent_filename("../../etc/passwd");
        assert!(
            !evil.contains('/') && !evil.contains('.'),
            "경로 문자 잔존: {evil}"
        );
        assert!(
            !thread_file_path("../../etc/passwd").contains(".."),
            "경로 탈출 잔존"
        );
    }

    // -- 정책 파싱 -------------------------------------------------------------

    #[test]
    fn parse_approval_policy_accepts_all_variants() {
        assert_eq!(
            parse_approval_policy("untrusted"),
            Ok(ApprovalPolicy::Untrusted)
        );
        assert_eq!(
            parse_approval_policy("on-failure"),
            Ok(ApprovalPolicy::OnFailure)
        );
        assert_eq!(
            parse_approval_policy("on-request"),
            Ok(ApprovalPolicy::OnRequest)
        );
        assert_eq!(parse_approval_policy("never"), Ok(ApprovalPolicy::Never));
    }

    #[test]
    fn parse_approval_policy_rejects_unknown() {
        assert!(parse_approval_policy("yolo").is_err());
    }

    #[test]
    fn parse_sandbox_mode_accepts_all_variants() {
        assert_eq!(parse_sandbox_mode("read-only"), Ok(SandboxMode::ReadOnly));
        assert_eq!(
            parse_sandbox_mode("workspace-write"),
            Ok(SandboxMode::WorkspaceWrite)
        );
        assert_eq!(
            parse_sandbox_mode("danger-full-access"),
            Ok(SandboxMode::DangerFullAccess)
        );
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
            params: json!({ "threadId": "tid-1", "turn": { "id": "turn-9" } }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Complete);
    }

    #[test]
    fn decide_action_turn_completed_other_thread_ignored() {
        let msg = IncomingMessage::Notification {
            method: "turn/completed".to_string(),
            params: json!({ "threadId": "tid-OTHER", "turn": { "id": "turn-9" } }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
        // is_turn_completed 자체 파싱도 재확인(회귀 방지 겸용). turn_id는 turn.id에서.
        assert_eq!(
            is_turn_completed(
                "turn/completed",
                &json!({"threadId":"tid-OTHER","turn":{"id":"turn-9"}})
            ),
            Some(TurnCompleted {
                thread_id: "tid-OTHER".to_string(),
                turn_id: "turn-9".to_string()
            })
        );
    }

    #[test]
    fn decide_action_final_agent_message_prints_text() {
        let msg = IncomingMessage::Notification {
            method: "item/completed".to_string(),
            params: json!({
                "threadId": "tid-1",
                "item": { "type": "agentMessage", "id": "m1", "text": "완료 답변", "phase": "final_answer" }
            }),
        };
        assert_eq!(
            decide_action(&msg, "tid-1"),
            InjectAction::PrintText("완료 답변".to_string())
        );
    }

    #[test]
    fn decide_action_final_agent_message_other_thread_ignored() {
        // 같은 ws에 붙은 다른 thread의 최종 답변은 우리 결과로 출력하지 않는다(교차 오염 방지).
        let msg = IncomingMessage::Notification {
            method: "item/completed".to_string(),
            params: json!({
                "threadId": "tid-OTHER",
                "item": { "type": "agentMessage", "id": "m1", "text": "남의 답변", "phase": "final_answer" }
            }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
    }

    #[test]
    fn decide_action_error_notification_our_thread_fails() {
        let msg = IncomingMessage::Notification {
            method: "error".to_string(),
            params: json!({ "error": {"message":"boom"}, "threadId": "tid-1", "turnId": "t9", "willRetry": false }),
        };
        match decide_action(&msg, "tid-1") {
            InjectAction::Fail(s) => assert!(s.contains("boom"), "에러 메시지 포함 기대: {s}"),
            other => panic!("Fail을 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn decide_action_error_notification_other_thread_ignored() {
        let msg = IncomingMessage::Notification {
            method: "error".to_string(),
            params: json!({ "error": {"message":"boom"}, "threadId": "tid-OTHER", "willRetry": false }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
    }

    #[test]
    fn decide_action_error_response_fails() {
        // turn/start 거부 등 에러 응답은 fire-and-forget이라 펌프에서만 잡히므로 Fail로 surface.
        let msg = IncomingMessage::Response {
            id: json!(3),
            result: None,
            error: Some(json!({ "code": -32602, "message": "invalid params" })),
        };
        match decide_action(&msg, "tid-1") {
            InjectAction::Fail(s) => assert!(s.contains("invalid params"), "에러 포함 기대: {s}"),
            other => panic!("Fail을 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn decide_action_agent_message_delta_is_ignored() {
        // 델타는 로그로 흘리지 않고 무시한다(최종답만 출력). extract_agent_message_delta 함수 자체의
        // 파싱은 별도 테스트로 유지.
        let msg = IncomingMessage::Notification {
            method: "item/agentMessage/delta".to_string(),
            params: json!({ "delta": "진행중..." }),
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
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
        let msg = IncomingMessage::Response {
            id: json!(1),
            result: Some(json!({})),
            error: None,
        };
        assert_eq!(decide_action(&msg, "tid-1"), InjectAction::Ignore);
    }

    // -- thread 영속 파일 IO ------------------------------------------------------

    #[test]
    fn persist_and_read_thread_id_roundtrip() {
        let dir = std::env::temp_dir().join(format!(
            "tunaround-codex-inject-test-{}",
            std::process::id()
        ));
        let path = dir.join("nested").join("agent.thread");
        let path_str = path.to_string_lossy().to_string();

        assert_eq!(
            read_persisted_thread_id(&path_str),
            None,
            "파일이 없으면 None"
        );

        persist_thread_id(&path_str, "019f2f6d-thread-id").expect("중첩 디렉터리도 생성돼야 함");
        assert_eq!(
            read_persisted_thread_id(&path_str).as_deref(),
            Some("019f2f6d-thread-id")
        );

        // 정리(테스트 오염 방지, 실패해도 무해).
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn read_persisted_thread_id_treats_blank_file_as_none() {
        let dir = std::env::temp_dir().join(format!(
            "tunaround-codex-inject-blank-{}",
            std::process::id()
        ));
        let path = dir.join("agent.thread");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(&path, "   \n").unwrap();
        assert_eq!(read_persisted_thread_id(&path.to_string_lossy()), None);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
