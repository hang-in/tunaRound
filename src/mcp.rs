// 토론 맥락 검색 MCP 서버: rmcp stdio 서버로 search_context 툴 하나를 노출한다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};

use crate::orchestrator::{ContextRetriever, RosterSeat, TranscriptReader, TranscriptWriter, Utterance};
use crate::store::a2a::{Task, TaskState};
use crate::store::agents::{parse_tags, AGENT_TTL_SECS};
use crate::store::sqlite::SqliteStore;

// ---------------------------------------------------------------------------
// v2-45 P6a: mesh 기억화 = 종결 task의 요청문+결과를 messages/FTS에 색인(search_context로 위임 이력 검색).
// ---------------------------------------------------------------------------

/// 종결 task 색인에 필요한 최소 정보(락 밖에서 writer로 색인하기 위해 락 안에서 미리 뽑는다).
struct TerminalIndexPayload {
    task_id: String,
    from_agent: String,
    to_agent: String,
    runner: Option<String>,
    /// 원 요청문(history[0]). 없으면 결과만 색인.
    request_text: Option<String>,
    /// 결과: completed=artifact 텍스트, failed=상태 메시지 텍스트. 없으면 요청만 색인.
    result_text: Option<String>,
}

/// 종결(completed/failed) task에서 색인 payload를 뽑는다(§5-7 네임스페이스용). 요청=history[0],
/// 결과=completed면 artifact·failed면 status_message. **비종결(canceled·열린)만 None**이다.
/// 결과 텍스트가 없어도 요청문만 있으면 색인한다: 결과 없다고 None을 주면 백필이 색인 없이 indexed_at을
/// 스탬프하고, P6b prune이 그걸 "mesh에 있음"으로 신뢰해 요청(history)을 영구 삭제해버리는 손실이 생긴다
/// (적대 리뷰 confirmed). "indexed_at ⟹ 텍스트 내용이 mesh에(또는 애초에 없음)" 불변식을 지킨다.
fn build_terminal_index_payload(task: &Task) -> Option<TerminalIndexPayload> {
    if !matches!(task.state, TaskState::Completed | TaskState::Failed) {
        return None; // canceled·열린 task는 색인 비대상(§4 P6a).
    }
    let request_text =
        task.history.first().and_then(|m| m.parts.first()).and_then(|p| p.text.clone());
    let result_text = match task.state {
        TaskState::Completed => {
            task.artifacts.first().and_then(|a| a.parts.first()).and_then(|p| p.text.clone())
        }
        _ => task.status_message.as_ref().and_then(|m| m.parts.first()).and_then(|p| p.text.clone()),
    };
    Some(TerminalIndexPayload {
        task_id: task.id.clone(),
        from_agent: task.from_agent.clone(),
        to_agent: task.to_agent.clone(),
        runner: task.runner.clone(),
        request_text,
        result_text,
    })
}

/// 종결 task 하나를 mesh 기억에 색인한다(v2-45 P6a). 네임스페이스(§5-7): session_id=`a2a:<task_id>`,
/// speaker=`a2a/<agent>`(요청=from, 결과=to 또는 runner). writer는 자체 store 연결이라 a2a_store 락과
/// 무관하다(락 순서: a2a_store 해제 후 호출). best-effort - 색인 실패는 종결을 되돌리지 않고 로그만 남기며
/// indexed_at을 스탬프하지 않아 다음 백필이 재시도한다. 양쪽 turn이 성공해야 스탬프한다.
fn index_terminal_task(
    writer: &Arc<dyn TranscriptWriter>,
    a2a_store: &Arc<Mutex<SqliteStore>>,
    p: &TerminalIndexPayload,
) {
    let sid = format!("a2a:{}", p.task_id);
    // 멱등 재색인(적대 리뷰 major): append_turn은 비멱등이고 append 커밋과 indexed_at 스탬프가 서로 다른
    // 커넥션이라, 크래시(taskkill·WMI 재기동 상시)·부분실패로 스탬프 전 죽으면 백필이 turn을 재-append해
    // 중복이 쌓인다. 재색인 전 이 세션의 기존 색인을 비워 delete-then-append로 멱등화한다(재실행=덮어쓰기).
    {
        let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = store.delete_session_messages(&sid) {
            eprintln!("[index] task {} 기존 색인 정리 실패(무시): {e}", p.task_id);
        }
    }
    let mut ok = true;
    if let Some(req) = &p.request_text
        && let Err(e) = writer.append_turn(&sid, &format!("a2a/{}", p.from_agent), req)
    {
        eprintln!("[index] task {} 요청 색인 실패(무시): {e}", p.task_id);
        ok = false;
    }
    let result_speaker = p.runner.as_deref().unwrap_or(&p.to_agent);
    if let Some(res) = &p.result_text
        && let Err(e) = writer.append_turn(&sid, &format!("a2a/{result_speaker}"), res)
    {
        eprintln!("[index] task {} 결과 색인 실패(무시): {e}", p.task_id);
        ok = false;
    }
    if ok {
        let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
        if let Err(e) = store.mark_task_indexed(&p.task_id) {
            eprintln!("[index] task {} indexed_at 스탬프 실패(무시): {e}", p.task_id);
        }
    }
}

/// 기동 시 미색인 종결 task를 mesh 기억에 백필한다(v2-45 P6a). 구 바이너리 시절 완료분·색인 유실
/// (expire_stale_claims 등)을 재기동 때 메운다. best-effort(개별 실패는 다음 기동이 재시도).
pub fn backfill_unindexed_terminal_tasks(
    a2a_store: &Arc<Mutex<SqliteStore>>,
    writer: &Arc<dyn TranscriptWriter>,
) {
    let tasks = {
        let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
        match store.list_unindexed_terminal_tasks() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[index] 백필 조회 실패(무시): {e}");
                return;
            }
        }
    };
    if tasks.is_empty() {
        return;
    }
    let n = tasks.len();
    for task in &tasks {
        match build_terminal_index_payload(task) {
            Some(payload) => index_terminal_task(writer, a2a_store, &payload),
            None => {
                // 결과 텍스트 없는 종결(레거시·expire→failed 등): 색인할 것이 없으니 스탬프만 해
                // 목록에서 제외한다(적대 리뷰 minor: 미스탬프 시 매 기동 무한 재스캔·비수렴).
                let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
                let _ = store.mark_task_indexed(&task.id);
            }
        }
    }
    eprintln!("[index] 기동 백필: 미색인 종결 task {n}건 처리");
}

#[cfg(feature = "serve")]
mod server;
#[cfg(feature = "serve")]
pub use server::{serve_http_mcp_on_listener, start_http_mcp_server};

mod params;
pub use params::*;
mod format;
pub use format::*;

/// rmcp MCP 서버 핸들러. ContextRetriever를 감싸 search_context/read_transcript 툴을 노출한다.
#[derive(Clone)]
pub struct TunaSearchServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    writer: Option<Arc<dyn TranscriptWriter>>,
    roster: Option<Vec<RosterSeat>>,
    /// session_id 파라미터 생략 시 기본으로 사용할 세션 id.
    default_session: String,
    /// A2A task 저장소(inbox 툴 poll_tasks/claim_task/complete_task 전용). None이면 세 툴 모두 비활성
    /// 안내 텍스트를 반환한다(stdio mcp-search 경로처럼 A2A가 배선되지 않은 경우).
    a2a_store: Option<Arc<Mutex<SqliteStore>>>,
}

impl TunaSearchServer {
    /// retriever Arc를 받아 새 서버 인스턴스를 반환한다(reader/writer/a2a_store=None, default_session="default", 기존 시그니처 유지).
    pub fn new(retriever: Arc<dyn ContextRetriever>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            retriever,
            reader: None,
            writer: None,
            roster: None,
            default_session: "default".to_string(),
            a2a_store: None,
        }
    }

    /// 전사 리더를 연결한 빌더 메서드(기존 new 시그니처 무영향).
    pub fn with_transcript_reader(mut self, reader: Arc<dyn TranscriptReader>) -> Self {
        self.reader = Some(reader);
        self
    }

    /// 전사 writer를 연결한 빌더 메서드(post_turn 활성화).
    pub fn with_transcript_writer(mut self, writer: Arc<dyn TranscriptWriter>) -> Self {
        self.writer = Some(writer);
        self
    }

    /// 로스터 스냅샷을 연결한 빌더 메서드(get_roster 활성화).
    pub fn with_roster(mut self, roster: Vec<RosterSeat>) -> Self {
        self.roster = Some(roster);
        self
    }

    /// session_id 파라미터 생략 시 사용할 기본 세션 id를 설정한다.
    pub fn with_default_session(mut self, session: String) -> Self {
        self.default_session = session;
        self
    }

    /// A2A task 저장소를 연결한 빌더 메서드(poll_tasks/claim_task/complete_task 활성화). 호출자가 이미
    /// 들고 있는 Arc를 그대로 넘겨 같은 mutex를 공유한다(새 SQLite 커넥션을 열지 않음).
    pub fn with_a2a_store(mut self, store: Arc<Mutex<SqliteStore>>) -> Self {
        self.a2a_store = Some(store);
        self
    }
}

#[tool_router]
impl TunaSearchServer {
    #[tool(description = "토론 맥락 검색: 과거·다른 분기의 관련 발언을 찾는다")]
    async fn search_context(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        // retrieve는 SQLite 락 + (semantic 시) 동기 임베딩 HTTP 호출이라 blocking이다.
        // async executor 스레드를 막지 않도록 spawn_blocking으로 넘긴다.
        let retriever = Arc::clone(&self.retriever);
        let query = p.query;
        let limit = p.limit.unwrap_or(10).min(50);
        // retrieve Err(1차 검색 경로 DB 장애, R7) = success로 위장하지 않는다. R1 계약(isError=true)으로 반환해
        // 클라(McpHttpClient::parse_jsonrpc_sse)가 "결과 없음"과 "검색 실패"를 구분하게 한다.
        let outcome: Result<Vec<Utterance>, String> =
            tokio::task::spawn_blocking(move || retriever.retrieve(&query, limit))
                .await
                .unwrap_or_else(|e| Err(format!("검색 태스크 실패: {e}")));
        let hits = match outcome {
            Ok(h) => h,
            Err(e) => return Ok(CallToolResult::error(vec![Content::text(format!("검색 실패: {e}"))])),
        };
        let text = if hits.is_empty() {
            "검색 결과 없음".to_string()
        } else {
            hits.iter()
                .map(|u| format!("[{}] {}", u.speaker, u.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "현재 토론 전사를 읽는다(활성 경로). 검색이 아니라 통째 맥락이 필요할 때.")]
    async fn read_transcript(
        &self,
        Parameters(p): Parameters<TranscriptParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(reader) = &self.reader else {
            return Ok(CallToolResult::success(vec![Content::text(
                "전사 리더 미연결".to_string(),
            )]));
        };
        let sid = p.session_id.unwrap_or_else(|| self.default_session.clone());
        // read_transcript Err(세션 로드 DB 장애, R7) = "전사 없음"으로 위장하지 않고 R1 계약으로 반환.
        let utts = match reader.read_transcript(&sid, p.max_turns) {
            Ok(u) => u,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!("전사 읽기 실패: {e}"))]));
            }
        };
        let text = if utts.is_empty() {
            "전사 없음".to_string()
        } else {
            utts.iter()
                .map(|u| format!("[{}] {}", u.speaker, u.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "토론에 발언을 추가한다(원격 참가자가 코어 전사에 자기 턴을 씀).")]
    async fn post_turn(
        &self,
        Parameters(p): Parameters<PostTurnParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(writer) = &self.writer else {
            return Ok(CallToolResult::success(vec![Content::text(
                "전사 writer 미연결(post_turn 비활성)".to_string(),
            )]));
        };
        let sid = p.session_id.unwrap_or_else(|| self.default_session.clone());
        match writer.append_turn(&sid, &p.speaker, &p.content) {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(format!(
                "추가됨: session={sid} msg_id={id}"
            ))])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!(
                "추가 실패: {e}"
            ))])),
        }
    }

    #[tool(description = "현재 토론 참가자(좌석) 구성을 조회한다.")]
    async fn get_roster(
        &self,
        Parameters(_p): Parameters<RosterParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = match &self.roster {
            None => "로스터 미연결".to_string(),
            Some(seats) if seats.is_empty() => "참가자 없음".to_string(),
            Some(seats) => seats
                .iter()
                .map(|s| match &s.role {
                    Some(r) => format!("{} ({})", s.engine, r),
                    None => s.engine.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "내 앞으로 온 A2A task 목록을 조회한다(열린 상태: submitted/working/input_required).")]
    async fn poll_tasks(
        &self,
        Parameters(p): Parameters<PollTasksParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(poll_tasks 비활성)".to_string(),
            )]));
        };
        // SQLite 락 호출이라 blocking이다. a2a_store는 A2A JSON-RPC 엔드포인트(a2a_server::a2a_handler)와
        // 동시에 경합할 수 있어 async executor 스레드를 막지 않도록 spawn_blocking으로 넘긴다(같은 관례).
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            poll_tasks_text(&store, &agent)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("조회 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "task에 착수했음을 표시한다(submitted/input_required -> working).")]
    async fn claim_task(
        &self,
        Parameters(p): Parameters<ClaimTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(claim_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let agent = p.agent;
        let runner = p.runner;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            claim_task_text(&store, &task_id, agent.as_deref(), runner.as_deref())
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다. isError=true라야 클라(McpHttpClient::
        // parse_jsonrpc_sse)가 Err로 인식하고, 워커(run_one_pass)가 claim 실패로 보고 러너를 안 돌린다.
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("착수 실패: {e}"))])),
        }
    }

    #[tool(description = "task 결과를 보고하고 완료 처리한다(-> completed, 결과는 텍스트 Artifact로 저장).")]
    async fn complete_task(
        &self,
        Parameters(p): Parameters<CompleteTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(complete_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let result = p.result;
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let text = complete_task_text(&store, &task_id, &result, agent.as_deref())?;
            // v2-45 P6a: 종결 성공 후 색인 payload를 같은 락 안에서 구성(요청=history[0], 결과=artifact).
            let payload =
                store.get_task(&task_id).ok().flatten().as_ref().and_then(build_terminal_index_payload);
            Ok::<_, String>((text, payload))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok((t, payload)) => {
                // a2a_store 락 해제 후 writer로 mesh 기억 색인(best-effort, 종결 응답과 독립).
                if let (Some(writer), Some(a2a), Some(payload)) =
                    (self.writer.clone(), self.a2a_store.clone(), payload)
                {
                    // best-effort·종결 응답과 독립: 색인을 백그라운드로 던지고 응답을 막지 않는다
                    // (gemini 리뷰). 크래시로 미완료 시 재기동 백필이 멱등 재색인한다(delete-then-append).
                    tokio::task::spawn_blocking(move || index_terminal_task(&writer, &a2a, &payload));
                }
                Ok(CallToolResult::success(vec![Content::text(t)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("완료 처리 실패: {e}"))])),
        }
    }

    #[tool(description = "task 실행이 실패했음을 보고한다(-> failed, 사유는 상태 메시지로 저장). completed와 구분되어 dispatcher가 실패를 인지한다.")]
    async fn fail_task(
        &self,
        Parameters(p): Parameters<FailTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(fail_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let reason = p.reason;
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let text = fail_task_text(&store, &task_id, &reason, agent.as_deref())?;
            // v2-45 P6a: 종결 성공 후 색인 payload 구성(요청=history[0], 결과=실패 사유 status_message).
            let payload =
                store.get_task(&task_id).ok().flatten().as_ref().and_then(build_terminal_index_payload);
            Ok::<_, String>((text, payload))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok((t, payload)) => {
                if let (Some(writer), Some(a2a), Some(payload)) =
                    (self.writer.clone(), self.a2a_store.clone(), payload)
                {
                    // best-effort·종결 응답과 독립: 색인을 백그라운드로 던지고 응답을 막지 않는다
                    // (gemini 리뷰). 크래시로 미완료 시 재기동 백필이 멱등 재색인한다(delete-then-append).
                    tokio::task::spawn_blocking(move || index_terminal_task(&writer, &a2a, &payload));
                }
                Ok(CallToolResult::success(vec![Content::text(t)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("실패 처리 실패: {e}"))])),
        }
    }

    #[tool(description = "다른 에이전트에게 새 A2A task를 위임한다(생성 즉시 submitted 상태, dispatcher용).")]
    async fn send_task(
        &self,
        Parameters(p): Parameters<SendTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(send_task 비활성)".to_string(),
            )]));
        };
        let SendTaskParams { from_agent, to_agent, text, context_id, to_selector } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            send_task_routed(&store, &from_agent, to_agent.as_deref(), to_selector.as_deref(), &text, context_id)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("전송 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "이 에이전트를 브로커 로스터에 등록한다(uuid+태그, 워커/세션 자기 등록용).")]
    async fn register_agent(
        &self,
        Parameters(p): Parameters<RegisterAgentParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(register_agent 비활성)".to_string(),
            )]));
        };
        let RegisterAgentParams { uuid, tags, display_name } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let tags = match tags {
                Some(s) => parse_tags(&s)?,
                None => BTreeMap::new(),
            };
            let tags_len = tags.len();
            store.register_agent(&uuid, tags, display_name, &now);
            Ok::<String, String>(format!("등록됨: uuid={uuid} tags={tags_len}개"))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 등록 실패(now/parse_tags 오류)를 success로 위장하지 않는다(클라가 감지하게 isError).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("등록 실패: {e}"))])),
        }
    }

    #[tool(description = "로스터에 자기 존재를 갱신한다(online 유지, 주기 호출).")]
    async fn heartbeat(
        &self,
        Parameters(p): Parameters<HeartbeatParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(heartbeat 비활성)".to_string(),
            )]));
        };
        let uuid = p.uuid;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let ok = store.heartbeat_agent(&uuid, &now);
            Ok::<String, String>(if ok {
                format!("heartbeat 갱신: {uuid}")
            } else {
                format!("미등록 uuid={uuid}(register_agent 먼저 호출하세요)")
            })
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 실제 실패(now 오류)만 isError. "미등록..."은 클로저에서 Ok라 success로 남아 워커의
        // 재등록 로직(needs_reregister)이 그 텍스트를 받는다(정상 흐름, 실패 아님).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("heartbeat 실패: {e}"))])),
        }
    }

    #[tool(description = "online 에이전트를 발견한다(selector 태그로 필터, dispatcher 라우팅용).")]
    async fn list_agents(
        &self,
        Parameters(p): Parameters<ListAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(list_agents 비활성)".to_string(),
            )]));
        };
        let selector = p.selector;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let sel = match selector {
                Some(s) => parse_tags(&s)?,
                None => BTreeMap::new(),
            };
            let agents = store.list_agents(&sel, &now, AGENT_TTL_SECS);
            Ok::<String, String>(format_agents(&agents))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 조회 실패(now/parse_tags 오류)를 success로 위장하지 않는다(클라가 감지하게 isError).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("조회 실패: {e}"))])),
        }
    }

    #[tool(description = "머신당 presence 스캐너가 라이브 세션 전집합을 일괄 보고한다(upsert+소유분 제거, v2-44).")]
    async fn report_presence(
        &self,
        Parameters(p): Parameters<ReportPresenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(report_presence 비활성)".to_string(),
            )]));
        };
        let ReportPresenceParams { machine, sessions } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let entries: Vec<crate::store::agents::PresenceUpsert> = sessions
                .into_iter()
                .map(|s| crate::store::agents::PresenceUpsert {
                    uuid: s.uuid,
                    runner: s.runner,
                    project: s.project,
                    display_name: s.display_name,
                    human_input_at: s.human_input_at,
                })
                .collect();
            let (upserted, removed) = store.sync_presence(&machine, &entries, &now);
            Ok::<String, String>(format!("presence 동기화(machine={machine}): upsert {upserted}건, 제거 {removed}건"))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // 동기화 실패(now 오류)를 success로 위장하지 않는다(R1 계약과 동일).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("presence 동기화 실패: {e}"))])),
        }
    }

    #[tool(description = "위임한 A2A task의 상태를 조회한다(completed면 결과 텍스트도 함께 반환, dispatcher용).")]
    async fn get_task(
        &self,
        Parameters(p): Parameters<GetTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(get_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            get_task_text(&store, &task_id)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("조회 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "브로커 전역에서 열려 있는 A2A task를 to_agent 무관하게 전부 조회한다(운영자 조망용, 미배달/고착 의심 주석 포함).")]
    async fn tasks(&self, Parameters(_p): Parameters<ListTasksParams>) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(tasks 비활성)".to_string(),
            )]));
        };
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            list_all_tasks_text(&store, &now)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("조회 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[tool_handler]
impl ServerHandler for TunaSearchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "토론 맥락을 검색하려면 search_context(query)를, 전사 통째를 읽으려면 read_transcript(session_id?, max_turns?)를 호출하세요. \
                 작업을 맡기는 쪽(dispatcher)은 send_task(from_agent, to_agent 또는 to_selector, text, context_id?)로 위임하고 get_task(task_id)로 결과를 확인하세요. \
                 작업을 받는 쪽(worker)은 poll_tasks(agent)로 확인하고 claim_task(task_id)로 착수, complete_task(task_id, result)로 완료를 보고하세요. \
                 워커/세션은 register_agent(uuid, tags?, display_name?)로 로스터에 등록하고 heartbeat(uuid)로 주기 갱신하며, \
                 dispatcher는 list_agents(selector?)로 online 에이전트를 발견합니다. \
                 머신당 presence 스캐너는 report_presence(machine, sessions)로 라이브 세션 전집합을 일괄 동기화합니다(v2-44). \
                 브로커 운영자는 tasks()로 전체 열린 task를 미배달(no-consumer?)/고착(stuck?) 주석과 함께 조망할 수 있습니다."
                    .to_string(),
            )
    }
}

/// bind 주소 문자열(`host:port`)에서 와일드카드 host(0.0.0.0 / :: / [::])를 loopback(127.0.0.1)으로
/// 치환한 base URL("http://host:port", 경로 접미사 없음)을 만든다. core_local_url(/mcp)과
/// a2a_server의 Agent Card(/a2a) 양쪽이 이 매핑을 공유한다.
#[cfg(feature = "serve")]
fn local_base_url(addr: &str) -> String {
    // 마지막 ':'로 host/port 분리. IPv6 "[::]:8771"도 마지막 ':'가 port 앞이라 host="[::]"가 된다.
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return format!("http://{addr}"), // 포트 없음(비정상): 그대로 감싼다.
    };
    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        other => other,
    };
    format!("http://{host}:{port}")
}

/// 코어 bind 주소를 로컬 좌석이 접속할 HTTP MCP URL로 변환한다(`/mcp` 접미사).
#[cfg(feature = "serve")]
pub fn core_local_url(addr: &str) -> String {
    format!("{}/mcp", local_base_url(addr))
}

/// 코어 bind 주소를 A2A JSON-RPC 엔드포인트 URL로 변환한다(`/a2a` 접미사). Agent Card의 `url` 필드에 쓴다.
#[cfg(feature = "serve")]
pub fn core_a2a_url(addr: &str) -> String {
    format!("{}/a2a", local_base_url(addr))
}

/// stdin/stdout을 전송으로 사용하는 stdio MCP 서버를 기동한다.
pub async fn start_mcp_server(
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    default_session: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut server = TunaSearchServer::new(retriever).with_default_session(default_session);
    if let Some(r) = reader {
        server = server.with_transcript_reader(r);
    }
    let (stdin, stdout) = rmcp::transport::io::stdio();
    let service = server.serve((stdin, stdout)).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;

    struct FakeRetriever(Vec<Utterance>);

    impl crate::orchestrator::ContextRetriever for FakeRetriever {
        fn retrieve(&self, _query: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn search_context_delegates_and_returns_ok() {
        let hits = vec![Utterance {
            speaker: "claude/proposer".into(),
            content: "검색 시스템 설계".into(),
        }];
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(hits)));
        let result = server
            .search_context(Parameters(SearchParams {
                query: "검색".into(),
                limit: Some(5),
            }))
            .await;
        assert!(result.is_ok(), "검색이 Ok여야 함: {result:?}");
    }

    #[test]
    fn terminal_index_payload_extracts_request_and_result() {
        use crate::store::a2a::{Artifact, Message, Part};
        let req = Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part { text: Some("피드백 3건 정리해줘".into()), ..Default::default() }],
            task_id: None,
            context_id: None,
        };
        // completed: 요청=history[0], 결과=artifact, 결과 speaker=runner.
        let mut done = Task::new("t1", None, "dashboard", "mac-claude", "2026-07-11 09:00:00");
        done.state = TaskState::Completed;
        done.history = vec![req.clone()];
        done.runner = Some("claude".into());
        done.artifacts = vec![Artifact {
            artifact_id: "a1".into(),
            name: None,
            parts: vec![Part { text: Some("정리 결과".into()), ..Default::default() }],
        }];
        let p = build_terminal_index_payload(&done).unwrap();
        assert_eq!(p.request_text.as_deref(), Some("피드백 3건 정리해줘"));
        assert_eq!(p.result_text.as_deref(), Some("정리 결과"));
        assert_eq!(p.from_agent, "dashboard");
        assert_eq!(p.runner.as_deref(), Some("claude"));
        // failed: 결과=status_message(실패 사유).
        let mut fail = Task::new("t2", None, "dashboard", "mac-claude", "2026-07-11 09:00:00");
        fail.state = TaskState::Failed;
        fail.history = vec![req.clone()];
        fail.status_message = Some(Message {
            message_id: "m2".into(),
            role: "agent".into(),
            parts: vec![Part { text: Some("BLOCKED: 자료 없음".into()), ..Default::default() }],
            task_id: None,
            context_id: None,
        });
        assert_eq!(
            build_terminal_index_payload(&fail).unwrap().result_text.as_deref(),
            Some("BLOCKED: 자료 없음")
        );
        // 결과 없어도 요청만 있으면 색인 대상(적대 리뷰: prune이 미색인 요청을 지우지 않게).
        let mut req_only = Task::new("t2b", None, "d", "m", "2026-07-11 09:00:00");
        req_only.state = TaskState::Completed;
        req_only.history = vec![req.clone()]; // artifact 없음.
        let ro = build_terminal_index_payload(&req_only).unwrap();
        assert_eq!(ro.request_text.as_deref(), Some("피드백 3건 정리해줘"));
        assert_eq!(ro.result_text, None, "결과 없음이어도 payload는 Some(요청 색인용)");
        // canceled·열린 task만 None(색인 비대상).
        let mut cancel = Task::new("t3", None, "d", "m", "2026-07-11 09:00:00");
        cancel.state = TaskState::Canceled;
        assert!(build_terminal_index_payload(&cancel).is_none());
    }

    #[test]
    fn backfill_stamps_result_less_terminal_to_converge() {
        // 결과·요청 텍스트가 전혀 없는 종결(레거시)도 백필이 스탬프해 매 기동 재스캔(비수렴)을 끊어야 한다.
        // (요청만 있으면 색인되어 스탬프되고, 아무것도 없으면 색인 없이 스탬프 - 둘 다 수렴.)
        struct FakeWriter;
        impl crate::orchestrator::TranscriptWriter for FakeWriter {
            fn append_turn(&self, _s: &str, _sp: &str, _c: &str) -> Result<u64, String> {
                Ok(0)
            }
        }
        let db = SqliteStore::open_memory().unwrap();
        // completed인데 artifact 없음 → build_terminal_index_payload가 None.
        let mut t = Task::new("t1", None, "win", "mac", "2026-07-11 09:00:00");
        t.state = TaskState::Completed;
        db.create_task(&t).unwrap();
        assert_eq!(db.list_unindexed_terminal_tasks().unwrap().len(), 1);
        let a2a = Arc::new(Mutex::new(db));
        let writer: Arc<dyn TranscriptWriter> = Arc::new(FakeWriter);
        backfill_unindexed_terminal_tasks(&a2a, &writer);
        assert_eq!(
            a2a.lock().unwrap().list_unindexed_terminal_tasks().unwrap().len(),
            0,
            "결과 없는 종결도 스탬프돼 재스캔 목록에서 빠짐(수렴)"
        );
        // 재백필도 no-op(수렴 유지).
        backfill_unindexed_terminal_tasks(&a2a, &writer);
        assert_eq!(a2a.lock().unwrap().list_unindexed_terminal_tasks().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn search_context_empty_retriever_returns_ok() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .search_context(Parameters(SearchParams {
                query: "없는내용".into(),
                limit: None,
            }))
            .await;
        assert!(result.is_ok());
    }

    /// 고정 Utterance를 반환하는 가짜 전사 리더.
    struct FakeTranscriptReader(Vec<Utterance>);

    impl crate::orchestrator::TranscriptReader for FakeTranscriptReader {
        fn read_transcript(
            &self,
            _session_id: &str,
            _max_turns: Option<usize>,
        ) -> Result<Vec<Utterance>, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn read_transcript_with_reader_returns_content() {
        let utts = vec![
            Utterance { speaker: "claude/proposer".into(), content: "첫 번째 발언".into() },
            Utterance { speaker: "codex/reviewer".into(), content: "두 번째 발언".into() },
        ];
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(Arc::new(FakeTranscriptReader(utts)));
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: Some("test-session".into()),
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok(), "read_transcript가 Ok여야 함: {result:?}");
        let call_result = result.unwrap();
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("첫 번째 발언"), "전사 내용이 포함되어야 함: {text}");
        assert!(text.contains("두 번째 발언"), "전사 내용이 포함되어야 함: {text}");
    }

    #[tokio::test]
    async fn read_transcript_without_reader_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: None,
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("전사 리더 미연결"), "reader=None 안내 불일치: {text}");
    }

    /// session_id를 캡처해 검증하는 전사 리더.
    struct CapturingTranscriptReader {
        captured: std::sync::Mutex<Option<String>>,
        utts: Vec<Utterance>,
    }

    impl CapturingTranscriptReader {
        fn new(utts: Vec<Utterance>) -> Self {
            Self { captured: std::sync::Mutex::new(None), utts }
        }
        fn last_session_id(&self) -> Option<String> {
            self.captured.lock().unwrap().clone()
        }
    }

    impl crate::orchestrator::TranscriptReader for CapturingTranscriptReader {
        fn read_transcript(
            &self,
            session_id: &str,
            _max_turns: Option<usize>,
        ) -> Result<Vec<Utterance>, String> {
            *self.captured.lock().unwrap() = Some(session_id.to_string());
            Ok(self.utts.clone())
        }
    }

    #[tokio::test]
    async fn read_transcript_without_session_id_uses_default_session() {
        // session_id 파라미터 생략 시 default_session이 TranscriptReader에 전달된다.
        let capturing = Arc::new(CapturingTranscriptReader::new(vec![
            Utterance { speaker: "claude".into(), content: "안녕".into() },
        ]));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>)
            .with_default_session("session-xyz".to_string());
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: None, // 생략 → default_session 사용.
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok(), "Ok여야 함: {result:?}");
        assert_eq!(
            capturing.last_session_id().as_deref(),
            Some("session-xyz"),
            "default_session이 TranscriptReader에 전달되어야 함"
        );
    }

    #[tokio::test]
    async fn read_transcript_explicit_session_id_overrides_default() {
        // session_id 명시 시 default_session이 아닌 명시 id가 사용된다.
        let capturing = Arc::new(CapturingTranscriptReader::new(vec![]));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>)
            .with_default_session("should-not-appear".to_string());
        let _ = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: Some("explicit-session".into()),
                max_turns: None,
            }))
            .await;
        assert_eq!(
            capturing.last_session_id().as_deref(),
            Some("explicit-session"),
            "명시 session_id가 우선되어야 함"
        );
    }

    /// append_turn 인자를 캡처하는 가짜 writer.
    struct CapturingWriter {
        captured: std::sync::Mutex<Option<(String, String, String)>>,
    }

    impl crate::orchestrator::TranscriptWriter for CapturingWriter {
        fn append_turn(&self, session_id: &str, speaker: &str, content: &str) -> Result<u64, String> {
            *self.captured.lock().unwrap() =
                Some((session_id.to_string(), speaker.to_string(), content.to_string()));
            Ok(7)
        }
    }

    #[tokio::test]
    async fn post_turn_with_writer_appends_and_uses_default_session() {
        let writer = Arc::new(CapturingWriter { captured: std::sync::Mutex::new(None) });
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_writer(writer.clone() as Arc<dyn crate::orchestrator::TranscriptWriter>)
            .with_default_session("sess-d".to_string());
        let result = server
            .post_turn(Parameters(PostTurnParams {
                session_id: None, // 생략 → default_session.
                speaker: "claude/proposer".into(),
                content: "원격 발언".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("msg_id=7"), "새 id 안내 불일치: {text}");
        let cap = writer.captured.lock().unwrap().clone();
        assert_eq!(cap, Some(("sess-d".into(), "claude/proposer".into(), "원격 발언".into())));
    }

    #[tokio::test]
    async fn post_turn_without_writer_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .post_turn(Parameters(PostTurnParams {
                session_id: None,
                speaker: "x".into(),
                content: "y".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("writer 미연결"), "미연결 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn get_roster_lists_seats() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_roster(vec![
            RosterSeat { engine: "claude".into(), role: Some("proposer".into()) },
            RosterSeat { engine: "codex".into(), role: None },
        ]);
        let result = server.get_roster(Parameters(RosterParams { session_id: None })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("claude (proposer)"), "좌석 표기 불일치: {text}");
        assert!(text.contains("codex"), "역할 없는 좌석 누락: {text}");
    }

    #[tokio::test]
    async fn get_roster_without_roster_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.get_roster(Parameters(RosterParams { session_id: None })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("로스터 미연결"), "미연결 안내 불일치: {text}");
    }

    // --- A2A inbox 툴(poll_tasks/claim_task/complete_task): 순수 함수 단위테스트 ---

    /// task 하나를 심고 store에 영속한다(inbox 테스트 공용 헬퍼).
    fn seed_task(store: &SqliteStore, id: &str, from: &str, to: &str, created_at: &str) {
        let task = crate::store::a2a::Task::new(id, None, from, to, created_at);
        store.create_task(&task).unwrap();
    }


    // --- A2A inbox 툴: MCP 계층(#[tool] 메서드) 테스트 ---

    /// in-memory store를 a2a_store로 연결한 서버를 만든다(inbox 툴 3종 테스트 공용).
    fn server_with_a2a_store(store: SqliteStore) -> TunaSearchServer {
        TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(Arc::new(Mutex::new(store)))
    }

    #[tokio::test]
    async fn poll_tasks_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.poll_tasks(Parameters(PollTasksParams { agent: "mac".into() })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn claim_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: None, runner: None })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn complete_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .complete_task(Parameters(CompleteTaskParams { task_id: "t1".into(), result: "결과".into(), agent: None }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn poll_tasks_tool_returns_open_tasks_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let server = server_with_a2a_store(store);
        let result = server.poll_tasks(Parameters(PollTasksParams { agent: "mac".into() })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("t1"), "task 목록 누락: {text}");
    }

    #[tokio::test]
    async fn claim_task_tool_transitions_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());
        let result = server.claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: None, runner: None })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("state=working"), "응답 불일치: {text}");
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[tokio::test]
    async fn complete_task_tool_completes_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        server.claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: None, runner: None })).await.unwrap();
        let result = server
            .complete_task(Parameters(CompleteTaskParams { task_id: "t1".into(), result: "완료 보고".into(), agent: None }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("state=completed"), "응답 불일치: {text}");
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Completed);
        assert_eq!(reloaded.artifacts[0].parts[0].text.as_deref(), Some("완료 보고"));
    }

    #[tokio::test]
    async fn claim_task_tool_missing_task_returns_error_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server.claim_task(Parameters(ClaimTaskParams { task_id: "nope".into(), agent: None, runner: None })).await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        // R1: 내부 실패는 isError=true라야 클라이언트가 성공으로 오인하지 않는다.
        assert_eq!(call_result.is_error, Some(true), "isError=true여야 함");
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("착수 실패"), "에러 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn complete_task_tool_missing_task_returns_error_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server
            .complete_task(Parameters(CompleteTaskParams { task_id: "nope".into(), result: "결과".into(), agent: None }))
            .await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        assert_eq!(call_result.is_error, Some(true), "isError=true여야 함");
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("완료 처리 실패"), "에러 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn claim_task_tool_already_working_returns_is_error_true() {
        // R2 전이충돌(이미 claim된 task를 다시 claim)이 R1 에러계약(isError=true)으로 정직하게 드러나는지
        // 확인한다(두 리팩토링이 맞물리는 지점). 클라이언트(McpHttpClient::parse_jsonrpc_sse)는 이
        // isError를 보고 Err로 매핑하고, 워커(run_one_pass)는 claim 실패로 보고 러너를 안 돌린다.
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        // 첫 claim은 성공(submitted -> working).
        let first = server.claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: None, runner: None })).await;
        assert!(first.is_ok());
        assert_eq!(first.unwrap().is_error, Some(false), "첫 claim은 성공이어야 함");

        // 둘째 claim(동시 착수 경쟁 시뮬레이션): 이미 working이라 전이충돌 -> isError=true.
        let second = server.claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: None, runner: None })).await;
        assert!(second.is_ok(), "MCP 레벨에서는 항상 Ok(CallToolResult)를 반환해야 함");
        let call_result = second.unwrap();
        assert_eq!(call_result.is_error, Some(true), "전이충돌인데 isError=true가 아님");
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("착수 실패"), "에러 안내 불일치: {text}");

        // 상태는 여전히 working으로 유지돼야 한다(둘째 호출이 조용히 성공한 것처럼 보이면 안 됨).
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[tokio::test]
    async fn complete_task_tool_first_completer_wins_rejects_mismatched_agent() {
        // MCP 툴 계층까지 agent 배선이 실제로 first-completer-wins를 강제하는지 확인한다(claim_task의
        // agent가 claimed_by로 기록되고, complete_task의 agent가 불일치하면 거부되어야 함).
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        let claim = server
            .claim_task(Parameters(ClaimTaskParams { task_id: "t1".into(), agent: Some("worker-a".into()), runner: None }))
            .await;
        assert_eq!(claim.unwrap().is_error, Some(false), "claim은 성공이어야 함");

        // 되살아난(stale) worker-b가 completer로 완료 보고 -> 거부(isError=true).
        let mismatched = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "worker-b의 결과".into(),
                agent: Some("worker-b".into()),
            }))
            .await;
        let mismatched = mismatched.unwrap();
        assert_eq!(mismatched.is_error, Some(true), "completer 불일치인데 isError=true가 아님");
        assert_eq!(
            a2a.lock().unwrap().get_task("t1").unwrap().unwrap().state,
            TaskState::Working,
            "거부 후에도 여전히 working이어야 함"
        );

        // claim한 본인(worker-a)이 completer면 성공.
        let matched = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "worker-a의 결과".into(),
                agent: Some("worker-a".into()),
            }))
            .await;
        assert_eq!(matched.unwrap().is_error, Some(false), "본인 completer는 성공해야 함");
        assert_eq!(
            a2a.lock().unwrap().get_task("t1").unwrap().unwrap().state,
            TaskState::Completed
        );
    }


    // --- A2A dispatcher 툴: MCP 계층(#[tool] 메서드) 테스트 ---

    #[tokio::test]
    async fn send_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .send_task(Parameters(SendTaskParams {
                from_agent: "win".into(),
                to_agent: Some("mac".into()),
                text: "부탁".into(),
                context_id: None,
                to_selector: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn get_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.get_task(Parameters(GetTaskParams { task_id: "t1".into() })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn send_task_tool_creates_task_via_server_and_get_task_reads_it_back() {
        let store = SqliteStore::open_memory().unwrap();
        let a2a = Arc::new(Mutex::new(store));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        let send_result = server
            .send_task(Parameters(SendTaskParams {
                from_agent: "win-claude".into(),
                to_agent: Some("mac-claude".into()),
                text: "리뷰 부탁".into(),
                context_id: None,
                to_selector: None,
            }))
            .await;
        assert!(send_result.is_ok());
        let send_text = format!("{:?}", send_result.unwrap().content);
        assert!(send_text.contains("state=submitted"), "send_task 응답 불일치: {send_text}");

        // 실제로 mac-claude 앞으로 열린 task가 생겼는지 store에서 직접 확인.
        let seeded_id = {
            let s = a2a.lock().unwrap();
            let tasks = s.list_open_tasks_for("mac-claude").unwrap();
            assert_eq!(tasks.len(), 1);
            tasks[0].id.clone()
        };

        // get_task 툴로도 같은 task를 읽을 수 있어야 한다(dispatcher가 보내고 확인하는 왕복).
        let get_result = server.get_task(Parameters(GetTaskParams { task_id: seeded_id.clone() })).await;
        assert!(get_result.is_ok());
        let get_text = format!("{:?}", get_result.unwrap().content);
        assert!(get_text.contains(&seeded_id), "get_task 응답에 task_id 없음: {get_text}");
        assert!(get_text.contains("state=submitted"), "get_task 응답 불일치: {get_text}");
    }

    #[tokio::test]
    async fn get_task_tool_missing_task_returns_not_found_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server.get_task(Parameters(GetTaskParams { task_id: "nope".into() })).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("없음"), "미존재 안내 불일치: {text}");
    }

    // --- tasks 툴(운영자 전역 조망): MCP 계층(#[tool] 메서드) 테스트 ---

    #[tokio::test]
    async fn tasks_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.tasks(Parameters(ListTasksParams {})).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("A2A task 저장소 미구성"), "미구성 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn tasks_tool_lists_tasks_across_agents() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        seed_task(&store, "t2", "win", "codex", "2026-07-02 09:05:00");
        let server = server_with_a2a_store(store);
        let result = server.tasks(Parameters(ListTasksParams {})).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("t1"), "t1 누락: {text}");
        assert!(text.contains("t2"), "t2 누락: {text}");
    }

}
