// codex app-server JSON-RPC 프로토콜의 요청/알림 타입과 순수 파싱
//
// 여기는 순수부만 다룬다(요청 빌더 + 들어오는 메시지 분류/파싱). ws 클라이언트 배선과 라이브 codex
// 실행은 T2 몫이라 이 파일에는 없다. 필드/메서드명은 설계 정본(docs/design/
// v2-codex-live-supervisor-appserver_2026-07-05.md)과 codex app-server가 내보내는 JSON 스키마
// (ClientRequest/ServerNotification/ServerRequest 및 각 *Params/*Response) 실측을 근거로 한다.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

/// turn/thread 승인 정책(`AskForApproval`의 문자열 variant만). granular 객체 변형은 범위 밖(YAGNI).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ApprovalPolicy {
    Untrusted,
    OnFailure,
    OnRequest,
    Never,
}

/// turn/thread 샌드박스 모드(`SandboxMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SandboxMode {
    ReadOnly,
    WorkspaceWrite,
    DangerFullAccess,
}

// ---------------------------------------------------------------------------
// 나가는 요청 빌더 (순수함수, JSON-RPC 봉투까지 포함해 반환)
// ---------------------------------------------------------------------------

/// JSON-RPC 2.0 요청 봉투 `{jsonrpc, id, method, params}`을 만든다.
fn envelope(id: u64, method: &str, params: Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": method,
        "params": params,
    })
}

/// `Some(v)`일 때만 직렬화해 map에 꽂는다(스키마상 옵션 필드는 생략이 기본, null을 명시적으로 보내지 않는다).
fn insert_opt<T: Serialize>(map: &mut Map<String, Value>, key: &str, value: Option<T>) {
    if let Some(v) = value {
        map.insert(
            key.to_string(),
            serde_json::to_value(v).expect("ApprovalPolicy/SandboxMode/String 직렬화는 실패하지 않음"),
        );
    }
}

/// `initialize` 요청. params = `{ clientInfo: { name, version } }`.
pub fn build_initialize_request(id: u64, client_name: &str, client_version: &str) -> Value {
    let params = json!({
        "clientInfo": { "name": client_name, "version": client_version },
    });
    envelope(id, "initialize", params)
}

/// `thread/start` 요청. approvalPolicy/sandbox/cwd 전부 옵션.
pub fn build_thread_start_request(
    id: u64,
    approval_policy: Option<ApprovalPolicy>,
    sandbox: Option<SandboxMode>,
    cwd: Option<&str>,
) -> Value {
    let mut params = Map::new();
    insert_opt(&mut params, "approvalPolicy", approval_policy);
    insert_opt(&mut params, "sandbox", sandbox);
    insert_opt(&mut params, "cwd", cwd.map(str::to_string));
    envelope(id, "thread/start", Value::Object(params))
}

/// `thread/resume` 요청. threadId만 필수.
pub fn build_thread_resume_request(
    id: u64,
    thread_id: &str,
    approval_policy: Option<ApprovalPolicy>,
    sandbox: Option<SandboxMode>,
) -> Value {
    let mut params = Map::new();
    params.insert("threadId".to_string(), Value::String(thread_id.to_string()));
    insert_opt(&mut params, "approvalPolicy", approval_policy);
    insert_opt(&mut params, "sandbox", sandbox);
    envelope(id, "thread/resume", Value::Object(params))
}

/// `UserInput`의 텍스트 변형(`{type:"text", text}`). image/localImage/skill/mention 변형은 범위 밖.
pub fn text_input(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

/// `turn/start` 요청. input은 텍스트 아이템 1개로 고정(감독 주입 용도라 다중 입력은 범위 밖).
pub fn build_turn_start_request(
    id: u64,
    thread_id: &str,
    text: &str,
    approval_policy: Option<ApprovalPolicy>,
) -> Value {
    let mut params = Map::new();
    params.insert("threadId".to_string(), Value::String(thread_id.to_string()));
    params.insert("input".to_string(), Value::Array(vec![text_input(text)]));
    insert_opt(&mut params, "approvalPolicy", approval_policy);
    envelope(id, "turn/start", Value::Object(params))
}

// ---------------------------------------------------------------------------
// 들어오는 메시지 분류 (순수함수)
// ---------------------------------------------------------------------------

/// 파싱된 JSON-RPC 메시지(서버->클라이언트 방향)를 세 갈래로 분류한 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum IncomingMessage {
    /// 우리가 보낸 요청의 응답. id 있고 method 없음.
    Response {
        id: Value,
        result: Option<Value>,
        error: Option<Value>,
    },
    /// 서버가 클라이언트 응답을 기대하는 요청(승인/elicitation 등). id와 method 둘 다 있음.
    ServerRequest {
        id: Value,
        method: String,
        params: Value,
    },
    /// 서버가 보내는 일방 알림. id 없음.
    Notification { method: String, params: Value },
}

/// 파싱된 JSON 값을 [`IncomingMessage`] 중 하나로 분류한다. id/method 조합으로 판별하며,
/// 객체가 아니거나 id/method가 둘 다 없으면 None(프레이밍 오류 등).
pub fn classify_message(msg: &Value) -> Option<IncomingMessage> {
    let obj = msg.as_object()?;
    let id = obj.get("id").cloned();
    let method = obj
        .get("method")
        .and_then(|m| m.as_str())
        .map(str::to_string);

    match (id, method) {
        (Some(id), Some(method)) => Some(IncomingMessage::ServerRequest {
            id,
            method,
            params: obj.get("params").cloned().unwrap_or(Value::Null),
        }),
        (Some(id), None) => Some(IncomingMessage::Response {
            id,
            result: obj.get("result").cloned(),
            error: obj.get("error").cloned(),
        }),
        (None, Some(method)) => Some(IncomingMessage::Notification {
            method,
            params: obj.get("params").cloned().unwrap_or(Value::Null),
        }),
        (None, None) => None,
    }
}

// ---------------------------------------------------------------------------
// 파싱 헬퍼 (순수함수)
// ---------------------------------------------------------------------------

/// thread/start(또는 thread/resume) 응답 전체(JSON-RPC 봉투 포함)에서 thread id를 뽑는다.
/// 경로는 `result.thread.id`다(`result.threadId`가 **아님**, P0 실측 §7).
pub fn parse_thread_id(thread_start_response: &Value) -> Option<String> {
    thread_start_response
        .get("result")?
        .get("thread")?
        .get("id")?
        .as_str()
        .map(String::from)
}

/// `turn/completed` 알림에서 뽑은 (threadId, turnId).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCompleted {
    pub thread_id: String,
    pub turn_id: String,
}

/// method/params가 `turn/completed` 알림이면 threadId/turnId를 뽑는다.
/// 실측 params 구조는 `{threadId: string, turn: {id, ...}}`다(P0에선 method만 봤고 params 구조는
/// 라이브 스모크에서야 확인 - `turnId` 평면 필드가 아니라 `turn.id` 중첩). thread_id가 완료 매칭의
/// 필수 키이고 turn_id는 참고용이라, turn.id가 없으면 빈 문자열로 둔다.
pub fn is_turn_completed(method: &str, params: &Value) -> Option<TurnCompleted> {
    if method != "turn/completed" {
        return None;
    }
    let thread_id = params.get("threadId")?.as_str()?.to_string();
    let turn_id = params
        .get("turn")
        .and_then(|t| t.get("id"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    Some(TurnCompleted { thread_id, turn_id })
}

/// method/params가 `item/completed` 알림이고 그 item이 최종 답변(agentMessage, phase=final_answer)이면
/// 텍스트를 뽑는다. 다른 item.type이나 phase(예: commentary)는 None.
pub fn extract_final_agent_message(method: &str, params: &Value) -> Option<String> {
    if method != "item/completed" {
        return None;
    }
    let item = params.get("item")?;
    if item.get("type")?.as_str()? != "agentMessage" {
        return None;
    }
    if item.get("phase")?.as_str()? != "final_answer" {
        return None;
    }
    item.get("text")?.as_str().map(String::from)
}

/// method/id가 `mcpServer/elicitation/request` ServerRequest면 그 요청 id를 반환한다.
/// T3에서 이 id로 [`build_elicitation_accept`] 응답을 만든다.
pub fn is_mcp_elicitation(method: &str, id: &Value) -> Option<Value> {
    if method == "mcpServer/elicitation/request" {
        Some(id.clone())
    } else {
        None
    }
}

/// `mcpServer/elicitation/request`에 대한 자동 accept 응답 `{jsonrpc, id, result:{action:"accept"}}`.
/// §5.2 결정(감독 행위=tuna-broker MCP 호출뿐이라 자동 accept가 안전)을 코드로 반영한다.
pub fn build_elicitation_accept(id: &Value) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": { "action": "accept" },
    })
}

/// 승인류 ServerRequest(execCommandApproval/applyPatchApproval/item 승인류)에 "승인" 응답을 만든다.
/// 응답 shape은 method별로 다르므로(각 `*ApprovalResponse.json` 스키마) method로 분기한다.
///
/// - `execCommandApproval`/`applyPatchApproval`(레거시): `{decision:"approved"}`(`ReviewDecision`).
/// - `item/commandExecution/requestApproval`/`item/fileChange/requestApproval`(신규):
///   `{decision:"accept"}`(각각 `CommandExecutionApprovalDecision`/`FileChangeApprovalDecision`).
/// - `item/permissions/requestApproval`: `{permissions:{}}`(`GrantedPermissionProfile`, 필드 전부
///   옵션이라 빈 객체가 유효한 "추가 권한 없음" 승인).
/// - 그 외 알려지지 않은 method는 명령 실행류 accept 형태로 최소 침습 폴백한다(T3에서 정책별로
///   세분화할 것. 미결정 사항으로 보고에 명시).
pub fn build_approval_granted(id: &Value, method: &str) -> Value {
    let result = match method {
        "execCommandApproval" | "applyPatchApproval" => json!({ "decision": "approved" }),
        "item/commandExecution/requestApproval" | "item/fileChange/requestApproval" => {
            json!({ "decision": "accept" })
        }
        "item/permissions/requestApproval" => json!({ "permissions": {} }),
        _ => json!({ "decision": "accept" }),
    };
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // -- 요청 빌더 --------------------------------------------------------

    #[test]
    fn initialize_request_has_expected_envelope_and_client_info() {
        let req = build_initialize_request(1, "tunaround-inject", "0.2.2");
        assert_eq!(req["jsonrpc"], "2.0");
        assert_eq!(req["id"], 1);
        assert_eq!(req["method"], "initialize");
        assert_eq!(req["params"]["clientInfo"]["name"], "tunaround-inject");
        assert_eq!(req["params"]["clientInfo"]["version"], "0.2.2");
    }

    #[test]
    fn thread_start_request_includes_optional_fields_when_present() {
        let req = build_thread_start_request(
            2,
            Some(ApprovalPolicy::Never),
            Some(SandboxMode::WorkspaceWrite),
            Some("D:\\privateProject\\tunaRound"),
        );
        assert_eq!(req["method"], "thread/start");
        assert_eq!(req["params"]["approvalPolicy"], "never");
        assert_eq!(req["params"]["sandbox"], "workspace-write");
        assert_eq!(req["params"]["cwd"], "D:\\privateProject\\tunaRound");
    }

    #[test]
    fn thread_start_request_omits_optional_fields_when_absent() {
        let req = build_thread_start_request(2, None, None, None);
        let params = req["params"].as_object().unwrap();
        assert!(!params.contains_key("approvalPolicy"));
        assert!(!params.contains_key("sandbox"));
        assert!(!params.contains_key("cwd"));
    }

    #[test]
    fn thread_resume_request_carries_thread_id() {
        let req = build_thread_resume_request(3, "019f2f6d-202f-7602-a402-a4d1ffdc8d85", None, None);
        assert_eq!(req["method"], "thread/resume");
        assert_eq!(req["params"]["threadId"], "019f2f6d-202f-7602-a402-a4d1ffdc8d85");
    }

    #[test]
    fn turn_start_request_wraps_text_input() {
        let req = build_turn_start_request(
            4,
            "019f2f6d-202f-7602-a402-a4d1ffdc8d85",
            "1+1?",
            Some(ApprovalPolicy::Never),
        );
        assert_eq!(req["method"], "turn/start");
        assert_eq!(req["params"]["threadId"], "019f2f6d-202f-7602-a402-a4d1ffdc8d85");
        assert_eq!(req["params"]["input"][0]["type"], "text");
        assert_eq!(req["params"]["input"][0]["text"], "1+1?");
        assert_eq!(req["params"]["approvalPolicy"], "never");
    }

    // -- thread id 파싱 (P0 실측 픽스처) -----------------------------------

    const THREAD_START_RESPONSE_FIXTURE: &str = r#"{
        "id": 2,
        "result": {
            "thread": {
                "id": "019f2f6d-202f-7602-a402-a4d1ffdc8d85",
                "sessionId": "019f2f6d-0000-0000-0000-000000000000",
                "status": { "type": "idle" },
                "path": "C:\\Users\\example\\.codex\\sessions\\rollout-2026-07-05.jsonl",
                "cwd": "D:\\privateProject\\tunaRound",
                "cliVersion": "0.142.5",
                "turns": []
            },
            "model": "gpt-5.5",
            "modelProvider": "openai"
        }
    }"#;

    #[test]
    fn parse_thread_id_reads_result_thread_id_not_result_thread_id_flat() {
        let resp: Value = serde_json::from_str(THREAD_START_RESPONSE_FIXTURE).unwrap();
        assert_eq!(
            parse_thread_id(&resp).as_deref(),
            Some("019f2f6d-202f-7602-a402-a4d1ffdc8d85")
        );
    }

    #[test]
    fn parse_thread_id_none_when_shape_unexpected() {
        let resp: Value = serde_json::from_str(r#"{"id":2,"result":{"threadId":"x"}}"#).unwrap();
        assert_eq!(parse_thread_id(&resp), None);
    }

    // -- elicitation ServerRequest (P0 실측 픽스처) ------------------------

    const ELICITATION_REQUEST_FIXTURE: &str = r#"{
        "jsonrpc": "2.0",
        "id": 42,
        "method": "mcpServer/elicitation/request",
        "params": {
            "threadId": "019f2f6d-202f-7602-a402-a4d1ffdc8d85",
            "turnId": "019f2f6d-d9ad-0000-0000-000000000000",
            "serverName": "tuna-broker",
            "mode": "form",
            "_meta": { "codex_approval_kind": "mcp_tool_call" }
        }
    }"#;

    #[test]
    fn elicitation_request_classifies_as_server_request() {
        let msg: Value = serde_json::from_str(ELICITATION_REQUEST_FIXTURE).unwrap();
        match classify_message(&msg) {
            Some(IncomingMessage::ServerRequest { id, method, params }) => {
                assert_eq!(id, json!(42));
                assert_eq!(method, "mcpServer/elicitation/request");
                assert_eq!(params["serverName"], "tuna-broker");
            }
            other => panic!("ServerRequest를 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn is_mcp_elicitation_extracts_request_id() {
        let msg: Value = serde_json::from_str(ELICITATION_REQUEST_FIXTURE).unwrap();
        let IncomingMessage::ServerRequest { id, method, .. } = classify_message(&msg).unwrap()
        else {
            panic!("ServerRequest 기대");
        };
        assert_eq!(is_mcp_elicitation(&method, &id), Some(json!(42)));
    }

    #[test]
    fn is_mcp_elicitation_none_for_other_methods() {
        assert_eq!(is_mcp_elicitation("execCommandApproval", &json!(42)), None);
    }

    #[test]
    fn build_elicitation_accept_matches_expected_shape() {
        let resp = build_elicitation_accept(&json!(42));
        assert_eq!(resp, json!({ "jsonrpc": "2.0", "id": 42, "result": { "action": "accept" } }));
    }

    // -- item/completed(agentMessage final) (P0 실측 픽스처) ---------------

    const ITEM_COMPLETED_FINAL_FIXTURE: &str = r#"{
        "method": "item/completed",
        "params": {
            "item": {
                "type": "agentMessage",
                "id": "msg_1",
                "text": "현재 online 에이전트는 2개입니다.",
                "phase": "final_answer"
            }
        }
    }"#;

    #[test]
    fn extract_final_agent_message_from_fixture() {
        let msg: Value = serde_json::from_str(ITEM_COMPLETED_FINAL_FIXTURE).unwrap();
        let IncomingMessage::Notification { method, params } = classify_message(&msg).unwrap()
        else {
            panic!("Notification 기대");
        };
        assert_eq!(
            extract_final_agent_message(&method, &params).as_deref(),
            Some("현재 online 에이전트는 2개입니다.")
        );
    }

    #[test]
    fn extract_final_agent_message_none_for_commentary_phase() {
        let msg: Value = serde_json::from_str(
            r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"msg_2","text":"...","phase":"commentary"}}}"#,
        )
        .unwrap();
        let IncomingMessage::Notification { method, params } = classify_message(&msg).unwrap()
        else {
            panic!("Notification 기대");
        };
        assert_eq!(extract_final_agent_message(&method, &params), None);
    }

    #[test]
    fn extract_final_agent_message_none_for_non_agent_message_item_type() {
        let msg: Value = serde_json::from_str(
            r#"{"method":"item/completed","params":{"item":{"type":"commandExecution","id":"cmd_1","phase":"final_answer"}}}"#,
        )
        .unwrap();
        let IncomingMessage::Notification { method, params } = classify_message(&msg).unwrap()
        else {
            panic!("Notification 기대");
        };
        assert_eq!(extract_final_agent_message(&method, &params), None);
    }

    // -- turn/completed 알림 ------------------------------------------------

    #[test]
    fn turn_completed_notification_classifies_and_parses() {
        // 실측 params 구조: threadId 평면 + turn 객체(turnId 평면 아님).
        let msg: Value = serde_json::from_str(
            r#"{"method":"turn/completed","params":{"threadId":"tid-1","turn":{"id":"turn-1"}}}"#,
        )
        .unwrap();
        let IncomingMessage::Notification { method, params } = classify_message(&msg).unwrap()
        else {
            panic!("Notification 기대");
        };
        let completed = is_turn_completed(&method, &params).expect("turn/completed 파싱 성공 기대");
        assert_eq!(completed.thread_id, "tid-1");
        assert_eq!(completed.turn_id, "turn-1");
    }

    #[test]
    fn is_turn_completed_none_for_other_methods() {
        let params = json!({ "threadId": "tid-1", "turnId": "turn-1" });
        assert_eq!(is_turn_completed("turn/started", &params), None);
    }

    // -- 그 외 알림 분류 -----------------------------------------------------

    #[test]
    fn turn_started_classifies_as_notification() {
        let msg: Value = serde_json::from_str(
            r#"{"method":"turn/started","params":{"threadId":"tid-1","turnId":"turn-1"}}"#,
        )
        .unwrap();
        match classify_message(&msg) {
            Some(IncomingMessage::Notification { method, .. }) => {
                assert_eq!(method, "turn/started");
            }
            other => panic!("Notification을 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn mcp_server_startup_status_updated_classifies_as_notification() {
        let msg: Value = serde_json::from_str(
            r#"{"method":"mcpServer/startupStatus/updated","params":{"serverName":"tuna-broker","status":"ready"}}"#,
        )
        .unwrap();
        match classify_message(&msg) {
            Some(IncomingMessage::Notification { method, .. }) => {
                assert_eq!(method, "mcpServer/startupStatus/updated");
            }
            other => panic!("Notification을 기대했는데 {other:?}"),
        }
    }

    // -- Response 분류 -------------------------------------------------------

    #[test]
    fn response_without_method_classifies_as_response() {
        let msg: Value = serde_json::from_str(r#"{"id":1,"result":{"ok":true}}"#).unwrap();
        match classify_message(&msg) {
            Some(IncomingMessage::Response { id, result, error }) => {
                assert_eq!(id, json!(1));
                assert_eq!(result, Some(json!({ "ok": true })));
                assert_eq!(error, None);
            }
            other => panic!("Response를 기대했는데 {other:?}"),
        }
    }

    #[test]
    fn classify_message_none_when_no_id_and_no_method() {
        let msg: Value = serde_json::from_str(r#"{"foo":"bar"}"#).unwrap();
        assert_eq!(classify_message(&msg), None);
    }

    // -- 승인 응답 빌더 -------------------------------------------------------

    #[test]
    fn build_approval_granted_exec_command_approval_legacy_shape() {
        let resp = build_approval_granted(&json!(7), "execCommandApproval");
        assert_eq!(resp["result"]["decision"], "approved");
    }

    #[test]
    fn build_approval_granted_apply_patch_approval_legacy_shape() {
        let resp = build_approval_granted(&json!(8), "applyPatchApproval");
        assert_eq!(resp["result"]["decision"], "approved");
    }

    #[test]
    fn build_approval_granted_item_command_execution_shape() {
        let resp = build_approval_granted(&json!(9), "item/commandExecution/requestApproval");
        assert_eq!(resp["result"]["decision"], "accept");
    }

    #[test]
    fn build_approval_granted_item_file_change_shape() {
        let resp = build_approval_granted(&json!(10), "item/fileChange/requestApproval");
        assert_eq!(resp["result"]["decision"], "accept");
    }

    #[test]
    fn build_approval_granted_item_permissions_shape() {
        let resp = build_approval_granted(&json!(11), "item/permissions/requestApproval");
        assert_eq!(resp["result"]["permissions"], json!({}));
    }
}
