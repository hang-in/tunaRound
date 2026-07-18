// 원격 tunaRound 코어의 MCP 툴을 HTTP(streamable-http)로 호출하는 워커용 클라이언트.
//
// rmcp streamable-http 서버는 initialize에서 발급한 mcp-session-id를 일정 유휴 시간 뒤 만료시킨다.
// 워커는 러너(codex/claude) 실행에 수 분이 걸릴 수 있어, claim_task 이후 complete_task를 부를 때쯤
// 세션이 이미 죽어 HTTP 404가 나는 경우가 있다. 이 클라이언트는 404류 응답을 감지하면 핸드셰이크를
// 다시 수행해 새 세션을 얻고, 원래 요청을 한 번만 재시도한다.

use serde_json::{Value, json};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

/// MCP 2025-03-26 protocolVersion(서버측 `src/mcp.rs` 테스트의 INIT_BODY와 동일 값).
const PROTOCOL_VERSION: &str = "2025-03-26";
const ACCEPT_HEADER: &str = "application/json, text/event-stream";

/// 일시적 오류(네트워크·5xx) 재시도 백오프 스케줄(ms, #6): 1차 재시도 전 200ms, 2차 재시도 전 500ms.
const RETRY_BACKOFF_MS: [u64; 2] = [200, 500];

/// 클라이언트 전역 요청 타임아웃(초). MCP 툴 호출은 단문이라는 전제의 값이며, 이 전제를 깨는 유일한
/// 예외인 get_task long-poll(wait_secs ≤ 120 > 60)은 per-request 타임아웃으로 따로 상향한다(#138 A-1).
const DEFAULT_REQUEST_TIMEOUT_SECS: u64 = 60;

/// get_task long-poll 요청의 per-request 타임아웃 여유분(초). 서버가 wait_secs를 꽉 채워 대기해도
/// 클라이언트가 그보다 늦게 끊도록 서버 상한에 더해진다(네트워크·직렬화 지연 흡수).
const GET_TASK_TIMEOUT_MARGIN_SECS: u64 = 30;

/// get_task(wait_secs) 요청에 적용할 per-request 타임아웃(순수 함수). wait를 서버 상한
/// (`GET_TASK_MAX_WAIT_SECS`)으로 먼저 clamp한 뒤 여유분을 더하므로, 서버가 허용하는 어떤 대기보다
/// 항상 길다(계약 = 아래 테스트가 단언).
fn get_task_request_timeout(wait_secs: u64) -> std::time::Duration {
    let wait = wait_secs.min(crate::a2a_wire::GET_TASK_MAX_WAIT_SECS);
    std::time::Duration::from_secs(wait + GET_TASK_TIMEOUT_MARGIN_SECS)
}

// 이 상향 로직의 존재 이유를 컴파일 타임 계약으로 고정한다: 서버 대기 상한(120)이 클라이언트 전역
// 기본(60)을 넘는 동안만 per-request 상향이 필요하다. 전역 기본을 상한 초과로 올려 이 관계가
// 뒤집히면 상향 로직이 잉여가 되므로, 그때 이 단언이 함께 재검토를 강제한다(#138 A-1).
const _: () = assert!(crate::a2a_wire::GET_TASK_MAX_WAIT_SECS > DEFAULT_REQUEST_TIMEOUT_SECS);

/// 원격 코어 `/mcp` 엔드포인트에 세션을 맺고 tools/call을 반복 호출하는 클라이언트.
pub struct McpHttpClient {
    http: reqwest::Client,
    mcp_url: String,
    token: Option<String>,
    /// 세션 만료 시 재연결로 갱신되어야 하므로 Mutex로 감싼다(구조체 자체는 &self로만 쓰임).
    session_id: Mutex<String>,
    next_id: AtomicU64,
}

impl McpHttpClient {
    /// initialize(POST) -> 응답 헤더 mcp-session-id 캡처 -> notifications/initialized(POST) 순서로
    /// 핸드셰이크해 새 session_id를 발급받는다. 최초 connect와 재연결이 동일한 절차를 공유하도록
    /// 뽑아낸 헬퍼이며, 이 함수 자체는 Self 상태를 만들지 않고 문자열 session_id만 반환한다.
    async fn handshake(
        http: &reqwest::Client,
        mcp_url: &str,
        token: &Option<String>,
    ) -> Result<String, String> {
        let init_body = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {},
                "clientInfo": {"name": "tunaround-worker", "version": env!("CARGO_PKG_VERSION")}
            }
        });

        let mut req = http
            .post(mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", ACCEPT_HEADER);
        if let Some(tok) = token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }
        let resp = req
            .body(init_body.to_string())
            .send()
            .await
            .map_err(|e| format!("initialize 요청 실패: {e}"))?;

        if !resp.status().is_success() {
            return Err(format!("initialize 응답 실패: HTTP {}", resp.status()));
        }

        let session_id = resp
            .headers()
            .get("mcp-session-id")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.to_string())
            .ok_or_else(|| "initialize 응답에 mcp-session-id 헤더 없음".to_string())?;

        // notifications/initialized는 알림이라 서버가 바디를 돌려주지 않을 수 있다. 네트워크 자체
        // 에러(연결 끊김 등)만 치명적으로 취급하고, 응답 바디 내용은 검사하지 않는다.
        let mut req = http
            .post(mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", ACCEPT_HEADER)
            .header("mcp-session-id", &session_id);
        if let Some(tok) = token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }
        req.body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
            .send()
            .await
            .map_err(|e| format!("notifications/initialized 요청 실패: {e}"))?;

        Ok(session_id)
    }

    /// 핸드셰이크를 수행해 클라이언트를 구성한다.
    pub async fn connect(
        mcp_url: impl Into<String>,
        token: Option<String>,
    ) -> Result<Self, String> {
        let mcp_url = mcp_url.into();
        // 타임아웃 없는 기본 클라이언트는 응답이 멎은 코어(방화벽 drop 등)에 무기한 대기해
        // 상주 데몬(presence-scan·poll) 전체를 정지시킨다(봇리뷰 Major). MCP 툴 호출은 단문이라
        // 60초면 충분히 관대하고, 연결 수립은 10초로 짧게 끊어 재시도 루프가 살아 있게 한다.
        // 단문 전제를 깨는 get_task long-poll만 요청 단위 타임아웃으로 이 값을 덮어쓴다(#138 A-1).
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(DEFAULT_REQUEST_TIMEOUT_SECS))
            .build()
            .map_err(|e| format!("HTTP 클라이언트 구성 실패: {e}"))?;
        let session_id = Self::handshake(&http, &mcp_url, &token).await?;

        Ok(Self {
            http,
            mcp_url,
            token,
            session_id: Mutex::new(session_id),
            next_id: AtomicU64::new(2),
        })
    }

    /// 세션 만료류 응답인지 판단한다. 401(인증 실패)은 토큰 자체 문제라 재연결해도 소용없으므로
    /// 제외하고, 우선 404를 세션 만료로 취급한다(향후 다른 상태코드가 관찰되면 이 매치에 추가).
    fn is_session_expired(status: reqwest::StatusCode) -> bool {
        matches!(status, reqwest::StatusCode::NOT_FOUND)
    }

    /// 일시적 오류라 재시도할 가치가 있는지 판단한다(순수 함수, #6). 네트워크 오류(try_call_once가
    /// BAD_GATEWAY로 매핑)·서버 5xx는 LAN 블립·브로커 순간 과부하처럼 곧 풀릴 수 있어 재시도 대상이다.
    /// 404(세션 만료)는 재핸드셰이크 경로(is_session_expired)가 따로 처리하므로 여기 포함하지 않고,
    /// 그 외 4xx(401 등 클라이언트 오류)는 재시도해도 소용없는 영구 오류라 제외한다.
    fn is_transient_error(status: reqwest::StatusCode) -> bool {
        status.is_server_error()
    }

    /// tools/call 한 번의 시도를 수행한다(재시도 로직 없이 순수 단발 호출). 현재 session_id 스냅샷을
    /// 읽어 헤더에 싣고, 실패 시 상태코드를 함께 반환해 호출부가 재연결 여부를 판단하게 한다.
    /// timeout이 Some이면 이 요청에 한해 클라이언트 전역 타임아웃(60초)을 덮어쓴다(get_task long-poll용).
    async fn try_call_once(
        &self,
        name: &str,
        args: &Value,
        timeout: Option<std::time::Duration>,
    ) -> Result<String, (reqwest::StatusCode, String)> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let body = json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "tools/call",
            "params": {"name": name, "arguments": args}
        });

        // 락은 현재 session_id를 복제하는 짧은 구간에서만 잡고, 즉시 드롭한다(await 경계를 넘겨
        // 들고 있지 않도록 clone 후 바로 스코프를 벗어나게 한다).
        let session_id = self.session_id.lock().unwrap().clone();

        let mut req = self
            .http
            .post(&self.mcp_url)
            .header("Content-Type", "application/json")
            .header("Accept", ACCEPT_HEADER)
            .header("mcp-session-id", &session_id);
        if let Some(tok) = &self.token {
            req = req.header("Authorization", format!("Bearer {tok}"));
        }
        if let Some(t) = timeout {
            req = req.timeout(t);
        }
        let resp = req.body(body.to_string()).send().await.map_err(|e| {
            (
                reqwest::StatusCode::BAD_GATEWAY,
                format!("tools/call({name}) 요청 실패: {e}"),
            )
        })?;

        let status = resp.status();
        if !status.is_success() {
            return Err((
                status,
                format!("tools/call({name}) 응답 실패: HTTP {status}"),
            ));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| (status, format!("tools/call({name}) 응답 읽기 실패: {e}")))?;
        parse_jsonrpc_sse(&text, name, id).map_err(|e| (status, e))
    }

    /// tools/call로 원격 MCP 툴 하나를 호출하고 결과 텍스트를 반환한다. 첫 시도가 세션 만료류(404)로
    /// 실패하면 핸드셰이크를 다시 수행해 session_id를 갱신한 뒤 같은 요청을 딱 한 번만 재시도한다.
    /// 첫 시도가 일시적 오류(네트워크 오류·서버 5xx)로 실패하면 짧은 백오프(`RETRY_BACKOFF_MS`)로
    /// 최대 그 개수만큼 추가 재시도한다(#6: 순간 LAN 블립·브로커 과부하 한 번에 수십 분짜리 러너
    /// 결과가 유실 -> lease 만료 후 처음부터 재실행되는 사고를 막는다. complete_task는
    /// first-completer-wins 가드가 있어 이중 완료가 무해하므로 재시도해도 안전하다). 그 외(4xx 등)는
    /// 즉시 실패로 반환한다. 재귀 호출이 아니라 순차 코드 흐름이라 무한 루프가 될 수 없다.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        self.call_tool_with_timeout(name, args, None).await
    }

    /// call_tool과 동일하되 이 호출의 모든 시도(재시도 포함)에 per-request 타임아웃을 적용한다.
    /// 전역 60초보다 오래 걸리는 게 정상인 호출(get_task long-poll)이 서버 정상 대기 중에
    /// 클라이언트 선실패로 끊기지 않게 한다(#138 A-1).
    async fn call_tool_with_timeout(
        &self,
        name: &str,
        args: Value,
        timeout: Option<std::time::Duration>,
    ) -> Result<String, String> {
        match self.try_call_once(name, &args, timeout).await {
            Ok(text) => Ok(text),
            Err((status, first_err)) => {
                if Self::is_session_expired(status) {
                    // 세션 만료로 판단 -> 핸드셰이크 재수행 -> 성공 시 session_id 갱신 후 1회 재시도.
                    return match Self::handshake(&self.http, &self.mcp_url, &self.token).await {
                        Ok(new_session_id) => {
                            *self.session_id.lock().unwrap() = new_session_id;
                            self.try_call_once(name, &args, timeout)
                                .await
                                .map_err(|(_, e)| e)
                        }
                        Err(reconnect_err) => {
                            Err(format!("{first_err} (재연결 시도도 실패: {reconnect_err})"))
                        }
                    };
                }

                if !Self::is_transient_error(status) {
                    return Err(first_err);
                }

                // 일시적 오류(네트워크 블립·5xx) - 짧은 백오프로 재시도(#6). 재시도 도중 세션이
                // 마침 만료(404)되는 것처럼 두 드문 실패가 겹치는 경계 케이스는 여기서 별도
                // 재핸드셰이크 없이 그 오류를 그대로 반환한다(다음 call_tool 호출이 정상적으로
                // 재핸드셰이크하므로 단순성 우선).
                let mut last_err = first_err;
                for backoff_ms in RETRY_BACKOFF_MS {
                    tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
                    match self.try_call_once(name, &args, timeout).await {
                        Ok(text) => return Ok(text),
                        Err((_, retry_err)) => last_err = retry_err,
                    }
                }
                Err(last_err)
            }
        }
    }

    /// poll_tasks(agent) 얇은 래퍼.
    pub async fn poll_tasks(&self, agent: &str) -> Result<String, String> {
        self.call_tool("poll_tasks", json!({ "agent": agent }))
            .await
    }

    /// claim_task(task_id, agent, runner) 얇은 래퍼. agent는 lease 소유자(claimed_by)로 기록되어
    /// first-completer-wins 판별에 쓰인다(None이면 서버측 하위호환 경로 - claimed_by NULL). runner는
    /// 처리하는 러너 종류(트레이스용, v8, 생략 가능).
    pub async fn claim_task(
        &self,
        task_id: &str,
        agent: Option<&str>,
        runner: Option<&str>,
    ) -> Result<String, String> {
        self.call_tool(
            "claim_task",
            json!({ "task_id": task_id, "agent": agent, "runner": runner }),
        )
        .await
    }

    /// complete_task(task_id, result, agent) 얇은 래퍼. agent는 first-completer-wins 완료자 검증에
    /// 쓰인다(claimed_by와 불일치하면 서버가 거부).
    pub async fn complete_task(
        &self,
        task_id: &str,
        result: &str,
        agent: Option<&str>,
    ) -> Result<String, String> {
        self.call_tool(
            "complete_task",
            json!({ "task_id": task_id, "result": result, "agent": agent }),
        )
        .await
    }

    /// fail_task(task_id, reason) 얇은 래퍼(러너 실패 시 completed 대신 failed로 전이).
    pub async fn fail_task(
        &self,
        task_id: &str,
        reason: &str,
        agent: Option<&str>,
    ) -> Result<String, String> {
        self.call_tool(
            "fail_task",
            json!({ "task_id": task_id, "reason": reason, "agent": agent }),
        )
        .await
    }

    /// register_agent(uuid, tags, display_name) 얇은 래퍼(워커/세션 자기 등록).
    pub async fn register_agent(
        &self,
        uuid: &str,
        tags: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<String, String> {
        self.call_tool(
            "register_agent",
            json!({ "uuid": uuid, "tags": tags, "display_name": display_name }),
        )
        .await
    }

    /// heartbeat(uuid) 얇은 래퍼(주기 ping으로 online 유지).
    pub async fn heartbeat(&self, uuid: &str) -> Result<String, String> {
        self.call_tool("heartbeat", json!({ "uuid": uuid })).await
    }

    /// list_agents(selector) 얇은 래퍼(online 에이전트 발견).
    pub async fn list_agents(&self, selector: Option<&str>) -> Result<String, String> {
        self.call_tool("list_agents", json!({ "selector": selector }))
            .await
    }

    /// report_presence(machine, sessions) 얇은 래퍼(presence 스캐너의 일괄 동기화, v2-44).
    /// sessions는 `[{uuid,runner,project?,display_name?}, ...]` JSON 배열.
    pub async fn report_presence(&self, machine: &str, sessions: Value) -> Result<String, String> {
        self.call_tool(
            "report_presence",
            json!({ "machine": machine, "sessions": sessions }),
        )
        .await
    }

    /// get_task(task_id, wait_secs?) 얇은 래퍼(task 상태·결과 확인, `tunaround task get`용).
    /// wait_secs(1~120)를 주면 서버가 terminal까지 long-poll하므로(v2-54 G7), 그 요청에 한해
    /// per-request 타임아웃을 wait+여유분으로 상향한다 - 전역 60초 기본이면 wait_secs≥60에서
    /// 서버 정상 대기 중 클라이언트가 먼저 끊긴다(#138 A-1). 0/None은 기존 즉시 조회 경로 그대로.
    pub async fn get_task(&self, task_id: &str, wait_secs: Option<u64>) -> Result<String, String> {
        let wait = wait_secs
            .unwrap_or(0)
            .min(crate::a2a_wire::GET_TASK_MAX_WAIT_SECS);
        if wait == 0 {
            return self
                .call_tool("get_task", json!({ "task_id": task_id }))
                .await;
        }
        self.call_tool_with_timeout(
            "get_task",
            json!({ "task_id": task_id, "wait_secs": wait }),
            Some(get_task_request_timeout(wait)),
        )
        .await
    }

    /// extend_task_lease(task_id, agent) 얇은 래퍼(워커가 실행 중 자기 task의 lease를 주기 연장,
    /// 장기 task requeue 방지, v2-49 #6). agent는 claimed_by와 일치해야 성공한다.
    pub async fn extend_lease(&self, task_id: &str, agent: &str) -> Result<String, String> {
        self.call_tool(
            "extend_task_lease",
            json!({ "task_id": task_id, "agent": agent }),
        )
        .await
    }

    /// cancel_task(task_id, reason) 얇은 래퍼(잘못 보냈거나 더 필요 없는 열린 task 취소, `tunaround
    /// task cancel`용).
    pub async fn cancel_task(&self, task_id: &str, reason: Option<&str>) -> Result<String, String> {
        self.call_tool(
            "cancel_task",
            json!({ "task_id": task_id, "reason": reason }),
        )
        .await
    }
}

/// SSE 프레이밍(`data: ...` 라인들) 안에서 JSON-RPC 응답 페이로드를 찾아 파싱한다. 서버(rmcp
/// StreamableHttpService)는 빈 하트비트 `data: \n` 라인과 실제 페이로드 `data: {json}\n` 라인을 함께
/// 내려보낼 수 있다(관찰된 원문 예: `data: \nid: 0/0\nretry: 3000\n\ndata: {"jsonrpc":"2.0","id":2,
/// "result":{...}}\nid: 1/0\n\n`). 서버가 결과 앞에 알림(예: notifications/message, id 없음)을 흘릴
/// 수 있으므로, 파싱되는 첫 줄이 아니라 **요청 id(`expected_id`)와 일치하는** data 줄을 고른다(#5).
/// `data: ` 라인이 하나도 없으면(예: 스트리밍이 아닌 plain JSON 바디 응답) 바디 전체를 JSON으로
/// 파싱하는 폴백을 쓴다(#5b).
fn parse_jsonrpc_sse(text: &str, tool_name: &str, expected_id: u64) -> Result<String, String> {
    let data_lines: Vec<&str> = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| !data.is_empty())
        .collect();

    let payload: Value = if data_lines.is_empty() {
        // 'data: ' 프레이밍 자체가 없는 순수 JSON 바디 폴백.
        serde_json::from_str::<Value>(text.trim()).map_err(|_| {
            format!("tools/call({tool_name}) 응답에서 JSON-RPC 페이로드를 못 찾음: {text}")
        })?
    } else {
        data_lines
            .into_iter()
            .filter_map(|data| serde_json::from_str::<Value>(data).ok())
            .find(|v| v.get("id") == Some(&json!(expected_id)))
            .ok_or_else(|| {
                format!(
                    "tools/call({tool_name}) 응답에서 id={expected_id} 페이로드를 못 찾음(알림만 있거나 응답 누락): {text}"
                )
            })?
    };

    if let Some(err) = payload.get("error") {
        return Err(format!("tools/call({tool_name}) JSON-RPC 에러: {err}"));
    }

    let result = payload
        .get("result")
        .ok_or_else(|| format!("tools/call({tool_name}) 응답에 result 없음: {text}"))?;

    let is_error = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let content_text = result
        .get("content")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|first| first.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string());

    if is_error {
        return Err(content_text.unwrap_or_else(|| {
            format!("tools/call({tool_name}) isError=true(본문 파싱 실패): {text}")
        }));
    }

    content_text.ok_or_else(|| {
        format!("tools/call({tool_name}) 응답에서 content[0].text를 못 찾음: {text}")
    })
}

#[cfg(all(test, feature = "worker", feature = "serve"))]
mod tests {
    use super::*;
    use std::sync::Arc;

    /// 테스트 전용 빈 retriever(mcp.rs의 NullRetriever와 동등, 이 모듈 자체에는 검색 로직 불요).
    struct NullRetriever;
    impl crate::orchestrator::ContextRetriever for NullRetriever {
        fn retrieve(
            &self,
            _q: &str,
            _limit: usize,
        ) -> Result<Vec<crate::orchestrator::Utterance>, String> {
            Ok(vec![])
        }
    }

    /// 인메모리 A2A store(mcp.rs test_a2a_store()와 동등한 최소 헬퍼).
    fn test_a2a_store() -> Arc<std::sync::Mutex<crate::store::sqlite::SqliteStore>> {
        Arc::new(std::sync::Mutex::new(
            crate::store::sqlite::SqliteStore::open_memory().expect("in-memory sqlite"),
        ))
    }

    /// ephemeral 포트로 HTTP MCP 서버를 띄우고 그 base URL("http://127.0.0.1:PORT/mcp")을 반환한다.
    async fn spawn_test_server(token: Option<String>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
        tokio::spawn(async move {
            let _ = crate::mcp::serve_http_mcp_on_listener(
                listener,
                retriever,
                None,
                None,
                None,
                token,
                test_a2a_store(),
            )
            .await;
        });
        // axum이 accept를 시작할 시간을 준다(기존 mcp.rs 테스트와 동일한 관례).
        tokio::time::sleep(std::time::Duration::from_millis(120)).await;
        format!("http://127.0.0.1:{port}/mcp")
    }

    #[tokio::test]
    async fn connect_and_poll_tasks_returns_empty_message() {
        let url = spawn_test_server(None).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let text = client
            .poll_tasks("nobody")
            .await
            .expect("poll_tasks 성공해야 함");
        assert!(
            text.contains("nobody 앞 열린 task 없음"),
            "poll_tasks 빈 결과 문구 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn connect_with_bearer_token_succeeds() {
        let url = spawn_test_server(Some("secret-tok".to_string())).await;
        let client = McpHttpClient::connect(url, Some("secret-tok".to_string()))
            .await
            .expect("토큰으로 connect 성공해야 함");

        let text = client
            .poll_tasks("mac-claude")
            .await
            .expect("토큰 인증 후 call_tool 성공해야 함");
        assert!(
            text.contains("mac-claude 앞 열린 task 없음"),
            "poll_tasks 빈 결과 문구 불일치: {text}"
        );
    }

    /// 세션 만료 자동 재연결 검증: connect 후 클라이언트의 session_id를 존재하지 않는 값으로
    /// 강제 교체해(Mutex라 테스트 코드에서 직접 가능) 다음 call_tool이 서버로부터 404를 받도록
    /// 유도한다. rmcp StreamableHttpService가 미등록 mcp-session-id 헤더에 실제로 404를 주는지
    /// 실험한 결과, 실제로 그렇게 동작함을 확인했다(방식 a 채택). call_tool이 404를 감지해
    /// 핸드셰이크를 재수행하고 요청을 재시도해 결국 성공하며, 이 과정에서 session_id가 최신 값으로
    /// 갱신되는 것까지 함께 확인한다.
    #[tokio::test]
    async fn call_tool_reconnects_after_session_expires() {
        let url = spawn_test_server(None).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let stale_session_id = {
            let mut guard = client.session_id.lock().unwrap();
            let stale = "expired-session-id-does-not-exist".to_string();
            *guard = stale.clone();
            stale
        };

        let text = client
            .poll_tasks("nobody")
            .await
            .expect("세션 만료 후에도 자동 재연결로 poll_tasks가 성공해야 함");
        assert!(
            text.contains("nobody 앞 열린 task 없음"),
            "재연결 후 poll_tasks 빈 결과 문구 불일치: {text}"
        );

        let refreshed_session_id = client.session_id.lock().unwrap().clone();
        assert_ne!(
            refreshed_session_id, stale_session_id,
            "재연결 성공 시 session_id가 새 값으로 갱신되어야 함"
        );
    }

    /// 레지스트리 e2e(Plan v2-34 T2): register_agent -> list_agents(발견) -> send_task(to_selector로
    /// 라우팅) -> get_task(존재+미완료 확인) 왕복. register_agent가 last_heartbeat를 now로 세팅하므로
    /// 별도 heartbeat 호출 없이 바로 online이라 list_agents에 뜬다.
    #[tokio::test]
    async fn register_list_send_by_selector_get_task_e2e() {
        let url = spawn_test_server(None).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let register_text = client
            .register_agent(
                "worker-uuid-1",
                Some("runner=claude,machine=win"),
                Some("win-claude"),
            )
            .await
            .expect("register_agent 성공해야 함");
        assert!(
            register_text.contains("worker-uuid-1"),
            "register 응답 불일치: {register_text}"
        );

        let list_text = client
            .list_agents(Some("runner=claude"))
            .await
            .expect("list_agents 성공해야 함");
        assert!(
            list_text.contains("worker-uuid-1"),
            "list_agents에 등록된 uuid 없음: {list_text}"
        );

        let send_text = client
            .call_tool(
                "send_task",
                json!({
                    "from_agent": "dispatcher",
                    "to_agent": null,
                    "to_selector": "runner=claude",
                    "text": "셀렉터 라우팅 테스트",
                    "context_id": null,
                }),
            )
            .await
            .expect("send_task(to_selector) 성공해야 함");
        assert!(
            send_text.contains("state=submitted"),
            "send_task 응답 불일치: {send_text}"
        );
        let task_id = send_text
            .split("task_id=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .expect("task_id 파싱 실패")
            .to_string();

        let get_text = client
            .call_tool("get_task", json!({ "task_id": task_id }))
            .await
            .expect("get_task 성공해야 함");
        assert!(
            get_text.contains(&task_id),
            "get_task 응답에 task_id 없음: {get_text}"
        );
        assert!(
            get_text.contains("state=submitted"),
            "get_task는 아직 완료 아니어야 함: {get_text}"
        );
    }

    // --- #6: 재시도 대상 판별(순수 함수, 서버 불요) ---

    #[test]
    fn is_transient_error_covers_5xx_but_not_4xx() {
        assert!(McpHttpClient::is_transient_error(
            reqwest::StatusCode::BAD_GATEWAY
        ));
        assert!(McpHttpClient::is_transient_error(
            reqwest::StatusCode::INTERNAL_SERVER_ERROR
        ));
        assert!(McpHttpClient::is_transient_error(
            reqwest::StatusCode::SERVICE_UNAVAILABLE
        ));
        assert!(
            !McpHttpClient::is_transient_error(reqwest::StatusCode::NOT_FOUND),
            "404는 세션만료 재핸드셰이크 경로가 처리(여기서 재시도 대상 아님)"
        );
        assert!(!McpHttpClient::is_transient_error(
            reqwest::StatusCode::UNAUTHORIZED
        ));
        assert!(!McpHttpClient::is_transient_error(
            reqwest::StatusCode::BAD_REQUEST
        ));
    }

    // --- #5: parse_jsonrpc_sse id 대조 + plain JSON 폴백(순수 함수, 서버 불요) ---

    #[test]
    fn parse_jsonrpc_sse_selects_response_matching_expected_id_skipping_notifications() {
        // 결과 앞에 id 없는 알림(notifications/message)이 흘러도, 요청 id(7)와 일치하는 data 줄만
        // 골라야 한다(#5a: 첫 파싱 라인 채택이었던 이전 버그의 회귀 가드).
        let text = "data: {\"jsonrpc\":\"2.0\",\"method\":\"notifications/message\",\"params\":{}}\n\n\
                     data: {\"jsonrpc\":\"2.0\",\"id\":7,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"ok\"}]}}\n\n";
        let out = parse_jsonrpc_sse(text, "poll_tasks", 7).expect("id 7 응답을 찾아야 함");
        assert_eq!(out, "ok");
    }

    #[test]
    fn parse_jsonrpc_sse_rejects_when_only_mismatched_id_present() {
        let text = "data: {\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"content\":[{\"type\":\"text\",\"text\":\"stale\"}]}}\n\n";
        let err = parse_jsonrpc_sse(text, "poll_tasks", 7).unwrap_err();
        assert!(
            err.contains("id=7"),
            "에러 메시지에 기대 id가 드러나야 함: {err}"
        );
    }

    #[test]
    fn parse_jsonrpc_sse_falls_back_to_plain_json_body_without_data_framing() {
        // 'data: ' 프레이밍이 없는 순수 JSON 바디도 파싱되어야 한다(#5b).
        let text =
            r#"{"jsonrpc":"2.0","id":3,"result":{"content":[{"type":"text","text":"plain"}]}}"#;
        let out =
            parse_jsonrpc_sse(text, "poll_tasks", 3).expect("plain JSON 바디 폴백이 성공해야 함");
        assert_eq!(out, "plain");
    }

    // --- #138 A-1: get_task long-poll 타임아웃 계약(순수 함수 + 라이브 왕복) ---

    #[test]
    fn get_task_timeout_contract_always_exceeds_server_wait() {
        // 계약: 서버가 wait를 꽉 채워 대기해도(상한 GET_TASK_MAX_WAIT_SECS로 clamp됨) per-request
        // 타임아웃이 항상 그보다 길어야 "서버 정상 대기 중 클라이언트 선실패"가 없다. 상한 초과
        // 입력(u64::MAX)은 clamp가 오버플로 없이 흡수해야 한다.
        for wait in [
            1,
            DEFAULT_REQUEST_TIMEOUT_SECS,
            crate::a2a_wire::GET_TASK_MAX_WAIT_SECS,
            u64::MAX,
        ] {
            let effective_wait = wait.min(crate::a2a_wire::GET_TASK_MAX_WAIT_SECS);
            let timeout = get_task_request_timeout(wait);
            assert!(
                timeout > std::time::Duration::from_secs(effective_wait),
                "wait={wait}: per-request 타임아웃({timeout:?})이 서버 대기({effective_wait}s)보다 길어야 함"
            );
        }
        // "상한(120) > 전역 기본(60)"이라는 존재 이유 자체는 모듈의 컴파일 타임 단언(const _)이 고정한다.
    }

    /// wait_secs가 실제 wire로 전송되어 서버가 long-poll하는지 검증한다: 아무도 처리하지 않는
    /// task를 wait=1로 조회 → 약 1초 대기 후 submitted 반환(즉시 반환이면 wait_secs 미전송 회귀).
    /// wait=None은 기존 즉시 조회 경로 그대로임을 함께 확인한다.
    #[tokio::test]
    async fn get_task_wait_secs_is_wired_to_server_long_poll() {
        let url = spawn_test_server(None).await;
        let client = McpHttpClient::connect(url, None)
            .await
            .expect("connect 성공해야 함");

        let send_text = client
            .call_tool(
                "send_task",
                json!({
                    "from_agent": "dispatcher",
                    "to_agent": "nobody",
                    "text": "long-poll wire 테스트",
                }),
            )
            .await
            .expect("send_task 성공해야 함");
        let task_id = send_text
            .split("task_id=")
            .nth(1)
            .and_then(|rest| rest.split_whitespace().next())
            .expect("task_id 파싱 실패")
            .to_string();

        // 즉시 조회 경로(wait 없음): 대기 없이 현재 상태.
        let started = std::time::Instant::now();
        let instant_text = client
            .get_task(&task_id, None)
            .await
            .expect("get_task(None) 성공해야 함");
        assert!(
            instant_text.contains("state=submitted"),
            "즉시 조회 상태 불일치: {instant_text}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(1),
            "wait 없는 조회는 대기하면 안 됨"
        );

        // long-poll 경로: wait_secs=1이 서버까지 전달되어야 약 1초 대기가 관측된다.
        let started = std::time::Instant::now();
        let polled_text = client
            .get_task(&task_id, Some(1))
            .await
            .expect("get_task(wait=1) 성공해야 함");
        assert!(
            polled_text.contains("state=submitted"),
            "long-poll 소진 후 상태 불일치: {polled_text}"
        );
        assert!(
            started.elapsed() >= std::time::Duration::from_secs(1),
            "wait_secs가 wire로 전송되지 않았다(서버 무대기 = 즉시 반환 회귀)"
        );
    }
}
