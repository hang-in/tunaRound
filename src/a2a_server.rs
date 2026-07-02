// A2A JSON-RPC 2.0 서버: SendMessage/GetTask/CancelTask 핸들러와 Agent Card, axum 라우트 마운트.

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::store::a2a::{task_event_to_frames, Message, StreamResponse, Task, TaskEvent, TaskState};
use crate::store::sqlite::SqliteStore;

// JSON-RPC 2.0 표준 에러 코드(A2A 스펙 §9.5 "Standard JSON-RPC Error Codes").
const CODE_PARSE_ERROR: i64 = -32700;
const CODE_METHOD_NOT_FOUND: i64 = -32601;
const CODE_INVALID_PARAMS: i64 = -32602;
const CODE_INTERNAL_ERROR: i64 = -32603;
// A2A 전용 에러 코드(A2A 스펙 §5.4 "Error Code Mappings"): TaskNotFoundError.
const CODE_TASK_NOT_FOUND: i64 = -32001;
// A2A 전용 에러 코드: UnsupportedOperationError(§3.3.2, capability 게이트 - streaming 미활성 store에서
// 스트리밍 메서드 호출 시 반환).
const CODE_UNSUPPORTED_OPERATION: i64 = -32004;

/// A2A JSON-RPC 메서드 문자열. 최신 A2A 스펙(a2a-protocol.org, ADR-001 protojson 채택 이후)은
/// "Method Naming: PascalCase method names matching gRPC conventions" (§9.1)이라 SendMessage/GetTask/
/// CancelTask를 그대로 쓴다(구 스펙의 message/send 등 슬래시 표기는 폐기됨. §5.3 Method Mapping Reference로 확정).
mod methods {
    pub const SEND_MESSAGE: &str = "SendMessage";
    pub const GET_TASK: &str = "GetTask";
    pub const CANCEL_TASK: &str = "CancelTask";
    /// SSE 스트리밍 엔드포인트(§3.1.2). T3에서 구현. SubscribeToTask(§3.1.6)는 별도 태스크(T4).
    pub const SEND_STREAMING_MESSAGE: &str = "SendStreamingMessage";
}

/// JSON-RPC 2.0 요청 봉투. id는 string|number|null 어느 쪽도 올 수 있어 Value로 받는다.
#[derive(Debug, Clone, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    #[allow(dead_code)]
    pub jsonrpc: String,
    #[serde(default)]
    pub id: serde_json::Value,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// JSON-RPC 2.0 에러 객체(code/message만. data는 Phase 1 범위 밖).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

/// JSON-RPC 2.0 응답 봉투. result/error는 상호 배타적(성공 시 result만, 실패 시 error만 채운다).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    /// 성공 응답을 만든다. 직렬화 실패는(우리 타입은 항상 직렬화 가능하므로) null result로 폴백한다.
    fn success<T: Serialize>(id: serde_json::Value, result: &T) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(serde_json::to_value(result).unwrap_or(serde_json::Value::Null)),
            error: None,
        }
    }

    /// 에러 응답을 만든다.
    fn error(id: serde_json::Value, code: i64, message: impl Into<String>) -> Self {
        JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(JsonRpcError { code, message: message.into() }),
        }
    }
}

/// SendMessage params. message는 표준 A2A 필드, fromAgent/toAgent는 tunaRound 중앙-브로커 라우팅
/// 확장이다(순정 A2A는 대상 agent URL로 라우팅하지만 우리는 단일 코어라 명시 필드로 받는다.
/// docs/design/v2-a2a-partner-delegation_2026-07-02.md §4/§10-1).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SendParams {
    pub message: Message,
    pub from_agent: String,
    pub to_agent: String,
}

/// GetTask/CancelTask 공용 params.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskIdParams {
    pub id: String,
    /// A2A 스펙에는 있으나(history 절단) Phase 1은 항상 전체 history를 반환하므로 무시한다.
    #[serde(default)]
    #[allow(dead_code)]
    pub history_length: Option<i64>,
}

/// SendMessage 순수 로직: 얇은 래퍼. task 조립(task_id·시각 발급, status_message/history 세팅,
/// 영속)은 store::create_task_from_message로 위임한다(mcp::send_task 툴과 공유하는 헬퍼 - DRY 우선,
/// serve<->mcp 크로스피처 직접의존 회피. docs/design/v2-a2a-partner-delegation_2026-07-02.md §10-1).
pub fn handle_send(store: &SqliteStore, params: SendParams) -> Result<Task, String> {
    store.create_task_from_message(&params.from_agent, &params.to_agent, params.message)
}

/// GetTask 순수 로직: 단순 조회 위임(없으면 Ok(None)).
pub fn handle_get(store: &SqliteStore, params: TaskIdParams) -> Result<Option<Task>, String> {
    store.get_task(&params.id)
}

/// CancelTask 순수 로직: 존재할 때만 canceled로 전이하고 갱신된 Task를 반환한다.
pub fn handle_cancel(store: &SqliteStore, params: TaskIdParams) -> Result<Option<Task>, String> {
    if store.get_task(&params.id)?.is_none() {
        return Ok(None);
    }
    store.update_task_state(&params.id, TaskState::Canceled, None)?;
    store.get_task(&params.id)
}

/// JSON-RPC 요청 하나를 처리한다(파싱 -> store 호출 -> 응답 조립, 순수 함수). axum 등 전송 계층과
/// 무관하게 단위테스트 가능(in-memory SqliteStore로 검증).
pub fn dispatch(store: &SqliteStore, req: JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        methods::SEND_MESSAGE => {
            let params: SendParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(req.id, CODE_INVALID_PARAMS, format!("Invalid parameters: {e}"));
                }
            };
            match handle_send(store, params) {
                Ok(task) => JsonRpcResponse::success(req.id, &task),
                Err(e) => JsonRpcResponse::error(req.id, CODE_INTERNAL_ERROR, format!("Internal error: {e}")),
            }
        }
        methods::GET_TASK => {
            let params: TaskIdParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(req.id, CODE_INVALID_PARAMS, format!("Invalid parameters: {e}"));
                }
            };
            match handle_get(store, params) {
                Ok(Some(task)) => JsonRpcResponse::success(req.id, &task),
                Ok(None) => JsonRpcResponse::error(req.id, CODE_TASK_NOT_FOUND, "Task not found"),
                Err(e) => JsonRpcResponse::error(req.id, CODE_INTERNAL_ERROR, format!("Internal error: {e}")),
            }
        }
        methods::CANCEL_TASK => {
            let params: TaskIdParams = match serde_json::from_value(req.params) {
                Ok(p) => p,
                Err(e) => {
                    return JsonRpcResponse::error(req.id, CODE_INVALID_PARAMS, format!("Invalid parameters: {e}"));
                }
            };
            match handle_cancel(store, params) {
                Ok(Some(task)) => JsonRpcResponse::success(req.id, &task),
                Ok(None) => JsonRpcResponse::error(req.id, CODE_TASK_NOT_FOUND, "Task not found"),
                Err(e) => JsonRpcResponse::error(req.id, CODE_INTERNAL_ERROR, format!("Internal error: {e}")),
            }
        }
        other => JsonRpcResponse::error(req.id, CODE_METHOD_NOT_FOUND, format!("Method not found: {other}")),
    }
}

/// A2A capabilities 최소 서브셋(Phase 1: 스트리밍·푸시 알림 둘 다 미지원).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCapabilities {
    pub streaming: bool,
    pub push_notifications: bool,
}

/// A2A Agent Card 최소 서브셋(Phase 1). skills는 항상 빈 배열(스킬 광고는 이기종 파트너 확장인
/// Phase 2에서 채운다. docs/design/v2-a2a-partner-delegation_2026-07-02.md §8).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentCard {
    pub name: String,
    pub description: String,
    pub url: String,
    pub version: String,
    pub capabilities: AgentCapabilities,
    pub default_input_modes: Vec<String>,
    pub default_output_modes: Vec<String>,
    pub skills: Vec<serde_json::Value>,
}

/// A2A 엔드포인트 URL로 최소 Agent Card를 조립한다(순수 함수, url 이외는 정적 값).
pub fn build_agent_card(a2a_url: &str) -> AgentCard {
    AgentCard {
        name: "tunaround-core".to_string(),
        description: "tunaRound A2A 코어: 파트너 에이전트에게 작업을 위임·수신하는 중앙 브로커.".to_string(),
        url: a2a_url.to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        capabilities: AgentCapabilities { streaming: false, push_notifications: false },
        default_input_modes: vec!["text/plain".to_string()],
        default_output_modes: vec!["text/plain".to_string()],
        skills: Vec::new(),
    }
}

// --- axum 배선(serve 피처 전용. 이 파일 전체가 이미 serve 게이트이므로 별도 cfg 불필요) ---

/// A2A 라우트가 공유하는 상태: task 저장소 + 정적 Agent Card.
#[derive(Clone)]
struct A2aState {
    store: Arc<Mutex<SqliteStore>>,
    card: Arc<AgentCard>,
}

/// A2A JSON-RPC(`/a2a`) + Agent Card(`/.well-known/agent-card.json`) 라우트로 axum Router를 만든다.
/// 호출자(mcp::serve_http_mcp_on_listener)가 기존 `/mcp` 라우터에 merge해 같은 axum app으로 서빙한다.
pub fn build_router(store: Arc<Mutex<SqliteStore>>, card: AgentCard) -> axum::Router {
    let state = A2aState { store, card: Arc::new(card) };
    axum::Router::new()
        .route("/a2a", axum::routing::post(a2a_handler))
        .route("/.well-known/agent-card.json", axum::routing::get(agent_card_handler))
        .with_state(state)
}

async fn agent_card_handler(
    axum::extract::State(state): axum::extract::State<A2aState>,
) -> axum::response::Response {
    json_response(&*state.card)
}

async fn a2a_handler(
    axum::extract::State(state): axum::extract::State<A2aState>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            let resp =
                JsonRpcResponse::error(serde_json::Value::Null, CODE_PARSE_ERROR, format!("Invalid JSON payload: {e}"));
            return json_response(&resp);
        }
    };

    // SendStreamingMessage는 SSE 응답이라 기존 unary 경로(JSON 응답)와 분리한다. 그 외 메서드는
    // 기존 dispatch 경로를 그대로 탄다(unary 동작·테스트 무변경).
    if req.method == methods::SEND_STREAMING_MESSAGE {
        return handle_send_streaming_message(state, req).await;
    }

    // SQLite 호출은 블로킹이라 async 실행기 스레드를 막지 않도록 spawn_blocking으로 넘긴다
    // (mcp.rs의 search_context와 동일한 관례).
    let resp = tokio::task::spawn_blocking(move || {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        dispatch(&store, req)
    })
    .await
    .unwrap_or_else(|e| {
        JsonRpcResponse::error(serde_json::Value::Null, CODE_INTERNAL_ERROR, format!("작업 실패: {e}"))
    });
    json_response(&resp)
}

/// `SendStreamingMessage` 처리: task를 생성하고(SendMessage와 동일 파라미터) 그 task의 상태변이를
/// SSE로 실시간 구독한다(§2.2 1번). 버스가 비활성(streaming capability 미가동)이면 UnsupportedOperationError.
async fn handle_send_streaming_message(state: A2aState, req: JsonRpcRequest) -> axum::response::Response {
    use axum::response::IntoResponse;

    // capability 게이트: 버스가 없으면 스트리밍 미지원(§2.3). 이 lock은 필드 clone만 하는 순간적 조회라
    // SQLite I/O가 없으므로 spawn_blocking 불필요.
    let sender = {
        let store = state.store.lock().unwrap_or_else(|e| e.into_inner());
        store.task_event_sender()
    };
    let Some(sender) = sender else {
        let resp = JsonRpcResponse::error(
            req.id,
            CODE_UNSUPPORTED_OPERATION,
            "UnsupportedOperationError: streaming not enabled",
        );
        return json_response(&resp);
    };

    // task 생성(store 커밋)보다 먼저 구독해야 초기 submitted 이벤트를 놓치지 않는다.
    let rx = sender.subscribe();

    let params: SendParams = match serde_json::from_value(req.params.clone()) {
        Ok(p) => p,
        Err(e) => {
            let resp = JsonRpcResponse::error(req.id, CODE_INVALID_PARAMS, format!("Invalid parameters: {e}"));
            return json_response(&resp);
        }
    };

    let req_id = req.id.clone();
    let store = state.store.clone();
    let created = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        handle_send(&store, params)
    })
    .await;

    let task = match created {
        Ok(Ok(task)) => task,
        Ok(Err(e)) => {
            let resp = JsonRpcResponse::error(req_id, CODE_INTERNAL_ERROR, format!("Internal error: {e}"));
            return json_response(&resp);
        }
        Err(e) => {
            let resp = JsonRpcResponse::error(req_id, CODE_INTERNAL_ERROR, format!("작업 실패: {e}"));
            return json_response(&resp);
        }
    };

    let stream = task_frame_stream(rx, task.id, req_id);
    axum::response::sse::Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::default()).into_response()
}

/// 하나의 JSON-RPC id 아래 StreamResponse 프레임을 감싼 SSE data 문자열을 만든다(순수 함수).
fn sse_frame_json(req_id: &serde_json::Value, frame: &StreamResponse) -> String {
    let envelope = serde_json::json!({
        "jsonrpc": "2.0",
        "id": req_id,
        "result": frame,
    });
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

/// broadcast 구독 상태에서 하나의 task_id에 속하는 SSE data 문자열(JSON-RPC 봉투) 스트림을 만드는
/// 순수 스트림 함수. axum `Event`는 내부 버퍼가 crate-private이라 직접 단위테스트할 수 없으므로,
/// 테스트 가능한 문자열 스트림을 여기서 만들고 `task_frame_stream`이 이를 `Event`로 감싸기만 한다.
///
/// - 다른 task_id의 이벤트는 무시(broadcast는 전역이라 필터링 필수).
/// - 한 번의 rx.recv()가 여러 프레임(예: Completed = artifact들 + 최종 statusUpdate)을 낼 수 있으므로
///   pending 큐에 순서대로 쌓아두고 하나씩 내보낸다.
/// - statusUpdate.final == true인 프레임을 내보낸 뒤 스트림을 종료한다.
/// - rx.recv()가 Err(Closed 또는 Lagged)이면 그 자리에서 스트림을 종료한다.
fn task_frame_json_stream(
    rx: tokio::sync::broadcast::Receiver<TaskEvent>,
    task_id: String,
    req_id: serde_json::Value,
) -> impl futures_util::Stream<Item = String> {
    struct StreamState {
        rx: tokio::sync::broadcast::Receiver<TaskEvent>,
        task_id: String,
        req_id: serde_json::Value,
        pending: std::collections::VecDeque<StreamResponse>,
        done: bool,
    }

    let state = StreamState { rx, task_id, req_id, pending: std::collections::VecDeque::new(), done: false };

    futures_util::stream::unfold(state, |mut st| async move {
        loop {
            if let Some(frame) = st.pending.pop_front() {
                let is_final = frame.status_update.as_ref().map(|s| s.is_final).unwrap_or(false);
                if is_final {
                    st.done = true;
                }
                let data = sse_frame_json(&st.req_id, &frame);
                return Some((data, st));
            }
            if st.done {
                return None;
            }
            match st.rx.recv().await {
                Ok(ev) => {
                    let event_task_id = match &ev {
                        TaskEvent::Status(task) => &task.id,
                        TaskEvent::Completed(task) => &task.id,
                    };
                    if event_task_id != &st.task_id {
                        // 다른 task의 이벤트(전역 버스라 섞여 들어옴) - 무시하고 다음 이벤트 대기.
                        continue;
                    }
                    st.pending.extend(task_event_to_frames(&ev));
                }
                Err(_) => {
                    // Closed 또는 Lagged 모두 스트림 종료(§T3 3번 지시).
                    return None;
                }
            }
        }
    })
}

/// `task_frame_json_stream`의 각 JSON 문자열을 axum SSE `Event`로 감싼다(HTTP 핸들러 전용 얇은 래퍼).
fn task_frame_stream(
    rx: tokio::sync::broadcast::Receiver<TaskEvent>,
    task_id: String,
    req_id: serde_json::Value,
) -> impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> {
    use futures_util::StreamExt;
    task_frame_json_stream(rx, task_id, req_id).map(|data| Ok(axum::response::sse::Event::default().data(data)))
}

/// 값을 JSON으로 직렬화해 HTTP 200 + application/json 응답을 만든다. axum "json" 피처(신규 의존)
/// 없이 serde_json(기존 의존)만으로 처리한다.
fn json_response<T: Serialize>(value: &T) -> axum::response::Response {
    use axum::response::IntoResponse;
    let body = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    (axum::http::StatusCode::OK, [(axum::http::header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::a2a::{Artifact, Part};

    fn sample_message(context_id: Option<&str>) -> Message {
        Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part { text: Some("작업을 부탁해".into()), ..Default::default() }],
            task_id: None,
            context_id: context_id.map(|s| s.to_string()),
        }
    }

    // --- pure handler 단위테스트 ---

    #[test]
    fn handle_send_creates_submitted_task_with_id_and_preserved_message() {
        let store = SqliteStore::open_memory().unwrap();
        let msg = sample_message(Some("ctx1"));
        let task = handle_send(
            &store,
            SendParams { message: msg.clone(), from_agent: "win-claude".into(), to_agent: "mac-claude".into() },
        )
        .unwrap();

        assert_eq!(task.state, TaskState::Submitted);
        assert_eq!(task.id.len(), 32, "task_id는 randomblob(16) hex 32자여야 함: {}", task.id);
        assert_eq!(task.context_id.as_deref(), Some("ctx1"));
        assert_eq!(task.from_agent, "win-claude");
        assert_eq!(task.to_agent, "mac-claude");
        assert_eq!(task.status_message, Some(msg.clone()));
        assert_eq!(task.history, vec![msg]);

        // store에도 실제로 영속되었는지 확인(round-trip).
        let persisted = store.get_task(&task.id).unwrap().expect("영속되어야 함");
        assert_eq!(persisted, task);
    }

    #[test]
    fn handle_send_two_calls_produce_distinct_task_ids() {
        let store = SqliteStore::open_memory().unwrap();
        let t1 = handle_send(
            &store,
            SendParams { message: sample_message(None), from_agent: "a".into(), to_agent: "b".into() },
        )
        .unwrap();
        let t2 = handle_send(
            &store,
            SendParams { message: sample_message(None), from_agent: "a".into(), to_agent: "b".into() },
        )
        .unwrap();
        assert_ne!(t1.id, t2.id);
    }

    #[test]
    fn handle_get_existing_returns_task() {
        let store = SqliteStore::open_memory().unwrap();
        let created = handle_send(
            &store,
            SendParams { message: sample_message(None), from_agent: "a".into(), to_agent: "b".into() },
        )
        .unwrap();
        let got = handle_get(&store, TaskIdParams { id: created.id.clone(), history_length: None }).unwrap();
        assert_eq!(got, Some(created));
    }

    #[test]
    fn handle_get_missing_returns_none() {
        let store = SqliteStore::open_memory().unwrap();
        let got = handle_get(&store, TaskIdParams { id: "nope".into(), history_length: None }).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn handle_cancel_transitions_to_canceled() {
        let store = SqliteStore::open_memory().unwrap();
        let created = handle_send(
            &store,
            SendParams { message: sample_message(None), from_agent: "a".into(), to_agent: "b".into() },
        )
        .unwrap();
        let canceled = handle_cancel(&store, TaskIdParams { id: created.id.clone(), history_length: None })
            .unwrap()
            .expect("존재하는 task여야 함");
        assert_eq!(canceled.state, TaskState::Canceled);
        assert_eq!(canceled.id, created.id);

        // store에도 반영됐는지 확인.
        let reloaded = store.get_task(&created.id).unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Canceled);
    }

    #[test]
    fn handle_cancel_missing_returns_none() {
        let store = SqliteStore::open_memory().unwrap();
        let got = handle_cancel(&store, TaskIdParams { id: "nope".into(), history_length: None }).unwrap();
        assert_eq!(got, None);
    }

    // --- dispatch(JSON-RPC 봉투) 단위테스트 ---

    fn req(method: &str, id: i64, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest { jsonrpc: "2.0".into(), id: serde_json::json!(id), method: method.into(), params }
    }

    #[test]
    fn dispatch_send_message_returns_task_result() {
        let store = SqliteStore::open_memory().unwrap();
        let params = serde_json::json!({
            "message": { "messageId": "m1", "role": "user", "parts": [{"text": "hi"}] },
            "fromAgent": "win",
            "toAgent": "mac",
        });
        let resp = dispatch(&store, req("SendMessage", 1, params));
        assert_eq!(resp.id, serde_json::json!(1));
        assert!(resp.error.is_none(), "에러 없어야 함: {:?}", resp.error);
        let result = resp.result.expect("result 있어야 함");
        assert_eq!(result["state"], "submitted");
        assert_eq!(result["fromAgent"], "win");
    }

    #[test]
    fn dispatch_get_task_missing_returns_task_not_found_error() {
        let store = SqliteStore::open_memory().unwrap();
        let resp = dispatch(&store, req("GetTask", 2, serde_json::json!({"id": "nope"})));
        assert!(resp.result.is_none());
        let err = resp.error.expect("error 있어야 함");
        assert_eq!(err.code, CODE_TASK_NOT_FOUND);
    }

    #[test]
    fn dispatch_get_task_after_send_round_trips() {
        let store = SqliteStore::open_memory().unwrap();
        let send_params = serde_json::json!({
            "message": { "messageId": "m1", "role": "user", "parts": [{"text": "hi"}] },
            "fromAgent": "win",
            "toAgent": "mac",
        });
        let created = dispatch(&store, req("SendMessage", 1, send_params)).result.unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let got = dispatch(&store, req("GetTask", 2, serde_json::json!({"id": id})));
        assert_eq!(got.result.unwrap()["id"], id);
    }

    #[test]
    fn dispatch_cancel_task_transitions_state() {
        let store = SqliteStore::open_memory().unwrap();
        let send_params = serde_json::json!({
            "message": { "messageId": "m1", "role": "user", "parts": [{"text": "hi"}] },
            "fromAgent": "win",
            "toAgent": "mac",
        });
        let created = dispatch(&store, req("SendMessage", 1, send_params)).result.unwrap();
        let id = created["id"].as_str().unwrap().to_string();

        let canceled = dispatch(&store, req("CancelTask", 2, serde_json::json!({"id": id})));
        assert_eq!(canceled.result.unwrap()["state"], "canceled");
    }

    #[test]
    fn dispatch_unknown_method_returns_method_not_found() {
        let store = SqliteStore::open_memory().unwrap();
        let resp = dispatch(&store, req("Frobnicate", 1, serde_json::json!({})));
        assert_eq!(resp.error.unwrap().code, CODE_METHOD_NOT_FOUND);
    }

    #[test]
    fn dispatch_invalid_params_returns_invalid_params_error() {
        let store = SqliteStore::open_memory().unwrap();
        // fromAgent/toAgent 누락 -> SendParams 파싱 실패.
        let resp = dispatch(&store, req("SendMessage", 1, serde_json::json!({"message": {}})));
        assert_eq!(resp.error.unwrap().code, CODE_INVALID_PARAMS);
    }

    // --- Agent Card ---

    #[test]
    fn agent_card_has_required_minimal_fields_and_parses_back() {
        let card = build_agent_card("http://127.0.0.1:8770/a2a");
        assert_eq!(card.url, "http://127.0.0.1:8770/a2a");
        assert!(!card.capabilities.streaming);
        assert!(card.skills.is_empty());

        let v = serde_json::to_value(&card).unwrap();
        // 스펙 최소 필드 + camelCase 방출 확인.
        for key in ["name", "description", "url", "version", "capabilities", "skills"] {
            assert!(v.get(key).is_some(), "필수 필드 누락: {key}");
        }
        assert!(v.get("defaultInputModes").is_some(), "defaultInputModes 누락(camelCase)");
        assert!(v.get("defaultOutputModes").is_some(), "defaultOutputModes 누락(camelCase)");
        assert!(v["capabilities"].get("pushNotifications").is_some());

        let back: AgentCard = serde_json::from_value(v).unwrap();
        assert_eq!(back, card);
    }

    // --- axum 통합테스트(실 HTTP 왕복, 라이브 e2e는 아님: in-process 서버 + reqwest) ---

    #[tokio::test]
    async fn http_send_get_cancel_round_trip_and_agent_card() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let card = build_agent_card("http://127.0.0.1:0/a2a");
        let router = build_router(store, card);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{port}");

        // Agent Card.
        let card_resp = client.get(format!("{base}/.well-known/agent-card.json")).send().await.unwrap();
        assert_eq!(card_resp.status(), 200);
        let card_json: serde_json::Value = card_resp.json().await.unwrap();
        assert_eq!(card_json["name"], "tunaround-core");

        // SendMessage.
        let send_body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "SendMessage",
            "params": {
                "message": {"messageId": "m1", "role": "user", "parts": [{"text": "부탁"}]},
                "fromAgent": "win-claude",
                "toAgent": "mac-claude",
            }
        });
        let send_resp: serde_json::Value =
            client.post(format!("{base}/a2a")).json(&send_body).send().await.unwrap().json().await.unwrap();
        assert!(send_resp.get("error").is_none(), "SendMessage 에러: {send_resp}");
        let task_id = send_resp["result"]["id"].as_str().unwrap().to_string();
        assert_eq!(send_resp["result"]["state"], "submitted");

        // GetTask.
        let get_body = serde_json::json!({"jsonrpc": "2.0", "id": 2, "method": "GetTask", "params": {"id": task_id}});
        let get_resp: serde_json::Value =
            client.post(format!("{base}/a2a")).json(&get_body).send().await.unwrap().json().await.unwrap();
        assert_eq!(get_resp["result"]["id"], task_id);

        // CancelTask.
        let cancel_body =
            serde_json::json!({"jsonrpc": "2.0", "id": 3, "method": "CancelTask", "params": {"id": task_id}});
        let cancel_resp: serde_json::Value =
            client.post(format!("{base}/a2a")).json(&cancel_body).send().await.unwrap().json().await.unwrap();
        assert_eq!(cancel_resp["result"]["state"], "canceled");

        // GetTask(존재하지 않는 id) -> JSON-RPC 에러.
        let missing_body =
            serde_json::json!({"jsonrpc": "2.0", "id": 4, "method": "GetTask", "params": {"id": "does-not-exist"}});
        let missing_resp: serde_json::Value =
            client.post(format!("{base}/a2a")).json(&missing_body).send().await.unwrap().json().await.unwrap();
        assert_eq!(missing_resp["error"]["code"], CODE_TASK_NOT_FOUND);
    }

    // --- T3: SendStreamingMessage ---

    // task_frame_json_stream 순수 로직 단위테스트(axum Event는 내부 버퍼가 crate-private이라 직접
    // 조립 불가 - JSON 문자열 스트림 단계에서 검증한다. task_frame_stream은 이 스트림을 Event로
    // 감싸기만 하는 얇은 래퍼라 여기서 검증하면 충분하다).
    #[tokio::test]
    async fn task_frame_json_stream_yields_ordered_frames_filters_other_tasks_and_stops_at_final() {
        use futures_util::StreamExt;

        let (tx, rx) = tokio::sync::broadcast::channel::<TaskEvent>(16);
        let req_id = serde_json::json!(1);
        let stream = task_frame_json_stream(rx, "t1".to_string(), req_id);
        futures_util::pin_mut!(stream);

        let submitted = Task::new("t1", Some("ctx1".into()), "win-claude", "mac-claude", "2026-07-03 10:00:00");
        tx.send(TaskEvent::Status(submitted.clone())).unwrap();

        // 다른 task_id 이벤트 - 필터링되어 무시되어야 함.
        let other = Task::new("other", None, "win-claude", "mac-claude", "2026-07-03 10:00:01");
        tx.send(TaskEvent::Status(other)).unwrap();

        let mut working = submitted.clone();
        working.state = TaskState::Working;
        working.updated_at = "2026-07-03 10:01:00".into();
        tx.send(TaskEvent::Status(working.clone())).unwrap();

        let mut completed = working.clone();
        completed.state = TaskState::Completed;
        completed.updated_at = "2026-07-03 10:02:00".into();
        completed.artifacts = vec![Artifact { artifact_id: "a1".into(), name: None, parts: vec![] }];
        tx.send(TaskEvent::Completed(completed.clone())).unwrap();

        // frame 1: 초기 task 스냅샷(submitted).
        let f1: serde_json::Value = serde_json::from_str(&stream.next().await.expect("frame1 있어야 함")).unwrap();
        assert_eq!(f1["id"], 1);
        assert_eq!(f1["result"]["task"]["id"], "t1");
        assert_eq!(f1["result"]["task"]["state"], "submitted");

        // frame 2: statusUpdate(working, final=false). "other" task 이벤트는 섞이지 않아야 함.
        let f2: serde_json::Value = serde_json::from_str(&stream.next().await.expect("frame2 있어야 함")).unwrap();
        assert_eq!(f2["result"]["statusUpdate"]["status"]["state"], "working");
        assert_eq!(f2["result"]["statusUpdate"]["final"], false);

        // frame 3: artifactUpdate(lastChunk true).
        let f3: serde_json::Value = serde_json::from_str(&stream.next().await.expect("frame3 있어야 함")).unwrap();
        assert_eq!(f3["result"]["artifactUpdate"]["lastChunk"], true);
        assert_eq!(f3["result"]["artifactUpdate"]["artifact"]["artifactId"], "a1");

        // frame 4: statusUpdate(completed, final=true) - 이후 스트림 종료.
        let f4: serde_json::Value = serde_json::from_str(&stream.next().await.expect("frame4 있어야 함")).unwrap();
        assert_eq!(f4["result"]["statusUpdate"]["status"]["state"], "completed");
        assert_eq!(f4["result"]["statusUpdate"]["final"], true);

        assert!(stream.next().await.is_none(), "final 프레임 이후 스트림이 종료되어야 함");
    }

    #[tokio::test]
    async fn send_streaming_message_returns_unsupported_operation_when_bus_inactive() {
        // with_task_events()를 호출하지 않은 store -> capability 게이트에 걸려야 함(§2.3).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let card = build_agent_card("http://127.0.0.1:0/a2a");
        let router = build_router(store, card);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{port}");
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage",
            "params": {
                "message": {"messageId": "m1", "role": "user", "parts": [{"text": "부탁"}]},
                "fromAgent": "win-claude",
                "toAgent": "mac-claude",
            }
        });
        let resp = client.post(format!("{base}/a2a")).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let content_type = resp.headers().get(axum::http::header::CONTENT_TYPE).unwrap().to_str().unwrap().to_string();
        assert!(content_type.starts_with("application/json"), "버스 비활성 시 JSON 에러여야 함: {content_type}");
        let json: serde_json::Value = resp.json().await.unwrap();
        assert_eq!(json["error"]["code"], CODE_UNSUPPORTED_OPERATION);
    }

    #[tokio::test]
    async fn send_streaming_message_returns_event_stream_content_type_when_bus_active() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap().with_task_events()));
        let card = build_agent_card("http://127.0.0.1:0/a2a");
        let router = build_router(store, card);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let base = format!("http://127.0.0.1:{port}");
        let body = serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "SendStreamingMessage",
            "params": {
                "message": {"messageId": "m1", "role": "user", "parts": [{"text": "부탁"}]},
                "fromAgent": "win-claude",
                "toAgent": "mac-claude",
            }
        });
        let resp = client.post(format!("{base}/a2a")).json(&body).send().await.unwrap();
        assert_eq!(resp.status(), 200);
        let content_type = resp.headers().get(axum::http::header::CONTENT_TYPE).unwrap().to_str().unwrap().to_string();
        assert!(content_type.starts_with("text/event-stream"), "스트리밍 응답 Content-Type이어야 함: {content_type}");
    }
}
