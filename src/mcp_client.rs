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
    pub async fn connect(mcp_url: impl Into<String>, token: Option<String>) -> Result<Self, String> {
        let mcp_url = mcp_url.into();
        // 타임아웃 없는 기본 클라이언트는 응답이 멎은 코어(방화벽 drop 등)에 무기한 대기해
        // 상주 데몬(presence-scan·poll) 전체를 정지시킨다(봇리뷰 Major). MCP 툴 호출은 단문이라
        // 60초면 충분히 관대하고, 연결 수립은 10초로 짧게 끊어 재시도 루프가 살아 있게 한다.
        let http = reqwest::Client::builder()
            .connect_timeout(std::time::Duration::from_secs(10))
            .timeout(std::time::Duration::from_secs(60))
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

    /// tools/call 한 번의 시도를 수행한다(재시도 로직 없이 순수 단발 호출). 현재 session_id 스냅샷을
    /// 읽어 헤더에 싣고, 실패 시 상태코드를 함께 반환해 호출부가 재연결 여부를 판단하게 한다.
    async fn try_call_once(
        &self,
        name: &str,
        args: &Value,
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
        let resp = req.body(body.to_string()).send().await.map_err(|e| {
            (
                reqwest::StatusCode::BAD_GATEWAY,
                format!("tools/call({name}) 요청 실패: {e}"),
            )
        })?;

        let status = resp.status();
        if !status.is_success() {
            return Err((status, format!("tools/call({name}) 응답 실패: HTTP {status}")));
        }

        let text = resp
            .text()
            .await
            .map_err(|e| (status, format!("tools/call({name}) 응답 읽기 실패: {e}")))?;
        parse_jsonrpc_sse(&text, name).map_err(|e| (status, e))
    }

    /// tools/call로 원격 MCP 툴 하나를 호출하고 결과 텍스트를 반환한다. 첫 시도가 세션 만료류(404)로
    /// 실패하면 핸드셰이크를 다시 수행해 session_id를 갱신한 뒤 같은 요청을 딱 한 번만 재시도한다.
    /// 재귀 호출이 아니라 순차 코드 흐름(시도 -> 실패 판단 -> 재연결 -> 재시도)이라 자연히 최대
    /// 2회(원 시도 1 + 재시도 1)로 끝나고 무한 루프가 될 수 없다.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<String, String> {
        match self.try_call_once(name, &args).await {
            Ok(text) => Ok(text),
            Err((status, first_err)) => {
                if !Self::is_session_expired(status) {
                    return Err(first_err);
                }

                // 세션 만료로 판단 -> 핸드셰이크 재수행 -> 성공 시 session_id 갱신 후 1회 재시도.
                match Self::handshake(&self.http, &self.mcp_url, &self.token).await {
                    Ok(new_session_id) => {
                        *self.session_id.lock().unwrap() = new_session_id;
                    }
                    Err(reconnect_err) => {
                        return Err(format!("{first_err} (재연결 시도도 실패: {reconnect_err})"));
                    }
                }

                self.try_call_once(name, &args).await.map_err(|(_, e)| e)
            }
        }
    }

    /// poll_tasks(agent) 얇은 래퍼.
    pub async fn poll_tasks(&self, agent: &str) -> Result<String, String> {
        self.call_tool("poll_tasks", json!({ "agent": agent })).await
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
        self.call_tool("fail_task", json!({ "task_id": task_id, "reason": reason, "agent": agent }))
            .await
    }

    /// register_agent(uuid, tags, display_name) 얇은 래퍼(워커/세션 자기 등록).
    pub async fn register_agent(
        &self,
        uuid: &str,
        tags: Option<&str>,
        display_name: Option<&str>,
    ) -> Result<String, String> {
        self.call_tool("register_agent", json!({ "uuid": uuid, "tags": tags, "display_name": display_name }))
            .await
    }

    /// heartbeat(uuid) 얇은 래퍼(주기 ping으로 online 유지).
    pub async fn heartbeat(&self, uuid: &str) -> Result<String, String> {
        self.call_tool("heartbeat", json!({ "uuid": uuid })).await
    }

    /// list_agents(selector) 얇은 래퍼(online 에이전트 발견).
    pub async fn list_agents(&self, selector: Option<&str>) -> Result<String, String> {
        self.call_tool("list_agents", json!({ "selector": selector })).await
    }

    /// report_candidates(candidates) 얇은 래퍼(발견 리포터가 후보 배열 보고).
    /// candidates는 `[{uuid,runner,project?,source,age_secs}, ...]` JSON 배열.
    pub async fn report_candidates(&self, candidates: Value) -> Result<String, String> {
        self.call_tool("report_candidates", json!({ "candidates": candidates })).await
    }

    /// list_candidates() 얇은 래퍼(발견된 미무장 세션 후보 조회, armed overlay 포함).
    pub async fn list_candidates(&self) -> Result<String, String> {
        self.call_tool("list_candidates", json!({})).await
    }

    /// report_presence(machine, sessions) 얇은 래퍼(presence 스캐너의 일괄 동기화, v2-44).
    /// sessions는 `[{uuid,runner,project?,display_name?}, ...]` JSON 배열.
    pub async fn report_presence(&self, machine: &str, sessions: Value) -> Result<String, String> {
        self.call_tool("report_presence", json!({ "machine": machine, "sessions": sessions })).await
    }

    /// get_task(task_id) 얇은 래퍼(task 상태·결과 확인, `tunaround task get`용).
    pub async fn get_task(&self, task_id: &str) -> Result<String, String> {
        self.call_tool("get_task", json!({ "task_id": task_id })).await
    }
}

/// SSE 프레이밍(`data: ...` 라인들) 안에서 JSON-RPC 응답 페이로드를 찾아 파싱한다. 서버(rmcp
/// StreamableHttpService)는 빈 하트비트 `data: \n` 라인과 실제 페이로드 `data: {json}\n` 라인을 함께
/// 내려보낼 수 있다(관찰된 원문 예: `data: \nid: 0/0\nretry: 3000\n\ndata: {"jsonrpc":"2.0","id":2,
/// "result":{...}}\nid: 1/0\n\n`). `data: ` 접두를 뗀 뒤 비어있지 않고 JSON으로 파싱되는 첫 줄을 쓴다.
fn parse_jsonrpc_sse(text: &str, tool_name: &str) -> Result<String, String> {
    let payload: Value = text
        .lines()
        .filter_map(|line| line.strip_prefix("data: "))
        .filter(|data| !data.is_empty())
        .find_map(|data| serde_json::from_str::<Value>(data).ok())
        .ok_or_else(|| format!("tools/call({tool_name}) 응답에서 JSON-RPC 페이로드를 못 찾음: {text}"))?;

    if let Some(err) = payload.get("error") {
        return Err(format!("tools/call({tool_name}) JSON-RPC 에러: {err}"));
    }

    let result = payload
        .get("result")
        .ok_or_else(|| format!("tools/call({tool_name}) 응답에 result 없음: {text}"))?;

    let is_error = result.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);

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

    content_text
        .ok_or_else(|| format!("tools/call({tool_name}) 응답에서 content[0].text를 못 찾음: {text}"))
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
        let client = McpHttpClient::connect(url, None).await.expect("connect 성공해야 함");

        let register_text = client
            .register_agent("worker-uuid-1", Some("runner=claude,machine=win"), Some("win-claude"))
            .await
            .expect("register_agent 성공해야 함");
        assert!(register_text.contains("worker-uuid-1"), "register 응답 불일치: {register_text}");

        let list_text = client.list_agents(Some("runner=claude")).await.expect("list_agents 성공해야 함");
        assert!(list_text.contains("worker-uuid-1"), "list_agents에 등록된 uuid 없음: {list_text}");

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
        assert!(send_text.contains("state=submitted"), "send_task 응답 불일치: {send_text}");
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
        assert!(get_text.contains(&task_id), "get_task 응답에 task_id 없음: {get_text}");
        assert!(get_text.contains("state=submitted"), "get_task는 아직 완료 아니어야 함: {get_text}");
    }
}
