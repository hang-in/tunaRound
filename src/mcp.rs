// 토론 맥락 검색 MCP 서버: rmcp stdio 서버로 search_context 툴 하나를 노출한다.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::orchestrator::{ContextRetriever, RosterSeat, TranscriptReader, TranscriptWriter, Utterance};
use crate::store::a2a::{Artifact, Message, Part, TaskState};
use crate::store::agents::{
    format_ambiguous_candidates, parse_tags, validate_send_target, AgentEntry, SendTarget, AGENT_TTL_SECS,
};
use crate::store::candidates::{CandidateEntry, CANDIDATE_TTL_SECS};
use crate::store::sqlite::SqliteStore;

/// search_context 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// 검색 질의.
    pub query: String,
    /// 최대 결과(기본 10).
    pub limit: Option<usize>,
}

/// read_transcript 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscriptParams {
    /// 세션 id(기본 "default").
    pub session_id: Option<String>,
    /// 마지막 N턴만(생략=전체).
    pub max_turns: Option<usize>,
}

/// post_turn 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PostTurnParams {
    /// 세션 id(기본 "default").
    pub session_id: Option<String>,
    /// 발언자 라벨(예: "claude/proposer").
    pub speaker: String,
    /// 발언 본문.
    pub content: String,
}

/// get_roster 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RosterParams {
    /// 세션 id(현재는 단일 로스터라 참고용).
    pub session_id: Option<String>,
}

/// poll_tasks 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PollTasksParams {
    /// 조회할 에이전트 id(A2A task의 to_agent).
    pub agent: String,
}

/// claim_task 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClaimTaskParams {
    /// 착수할 task id.
    pub task_id: String,
    /// 착수하는 에이전트 id(lease 소유자 표시, first-completer-wins 판별용). 생략 시(하위호환, raw
    /// curl 등) None → claimed_by는 NULL로 남고 completer 가드는 무력화된다.
    pub agent: Option<String>,
    /// 처리하는 러너 종류(claude/codex 등), 트레이스용. 생략 가능.
    pub runner: Option<String>,
}

/// complete_task 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CompleteTaskParams {
    /// 완료할 task id.
    pub task_id: String,
    /// 결과 텍스트(단일 텍스트 Artifact로 감싸 저장한다).
    pub result: String,
    /// 완료 보고하는 에이전트 id(first-completer-wins: claimed_by와 불일치하면 거부). 생략 시(하위호환)
    /// None → 가드 무력화(기존 동작).
    pub agent: Option<String>,
}

/// fail_task 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct FailTaskParams {
    /// 실패 처리할 task id.
    pub task_id: String,
    /// 실패 사유(상태 메시지로 저장해 dispatcher가 읽는다).
    pub reason: String,
    /// 실패 보고하는 에이전트 id(first-completer-wins: claimed_by와 불일치하면 거부). 생략 시(하위호환)
    /// None → 가드 무력화(기존 동작).
    pub agent: Option<String>,
}

/// send_task 툴 파라미터(dispatcher가 새 A2A task를 위임할 때 사용). to_agent(구체 대상)와
/// to_selector(태그 발견, 발송 시점에 uuid로 해석) 중 정확히 하나만 지정한다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SendTaskParams {
    /// 보내는 에이전트 id(A2A task의 from_agent).
    pub from_agent: String,
    /// 받는 에이전트 id(A2A task의 to_agent). to_selector와 배타적.
    pub to_agent: Option<String>,
    /// 작업 지시 본문.
    pub text: String,
    /// 대화 맥락 id(생략 가능).
    pub context_id: Option<String>,
    /// 태그 셀렉터 "k=v,k=v"(발견 후 uuid로 라우팅). to_agent와 배타적.
    pub to_selector: Option<String>,
}

/// register_agent 툴 파라미터(워커/세션이 뜰 때 로스터에 자기 등록).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct RegisterAgentParams {
    /// 에이전트 고유 id(워커 자가 발급 uuid 권장, 라우팅 키).
    pub uuid: String,
    /// 발견용 태그 "k=v,k=v"(예: "machine=win,runner=claude,role=worker"). 생략 가능.
    pub tags: Option<String>,
    /// 로스터 가독용 표시 이름(생략 가능).
    pub display_name: Option<String>,
}

/// heartbeat 툴 파라미터(주기 ping으로 online 유지).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct HeartbeatParams {
    /// heartbeat를 갱신할 에이전트 id.
    pub uuid: String,
}

/// list_agents 툴 파라미터(online 에이전트 발견, selector로 필터).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListAgentsParams {
    /// 태그 셀렉터 "k=v,k=v"(부분집합 매칭). 생략 시 online 전부.
    pub selector: Option<String>,
}

/// report_candidates 툴의 후보 한 건(발견 리포터가 열거해 보고). reported_at은 브로커가 수신 시각으로 채운다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CandidateInput {
    /// 세션 id(claude=jsonl 파일 stem). roster uuid와 같은 공간이라 armed overlay가 가능하다.
    pub uuid: String,
    /// 러너 종류(claude | codex | ...).
    pub runner: String,
    /// 추정 프로젝트(불명이면 생략).
    pub project: Option<String>,
    /// 리포터 머신(win|mac|unix). 크로스머신 발견 시 머신 구분용(생략 가능).
    pub machine: Option<String>,
    /// 발견 출처(예: claude-jsonl).
    pub source: String,
    /// 세션 활동 경과 초(claude=jsonl mtime 유래).
    pub age_secs: i64,
}

/// report_candidates 툴 파라미터(발견 리포터가 로컬 세션 후보 배열을 보고).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportCandidatesParams {
    /// 보고할 후보 목록(uuid 단위 upsert, 재보고 없으면 TTL로 소멸).
    pub candidates: Vec<CandidateInput>,
}

/// list_candidates 툴 파라미터(필드 없음). fresh 후보 전체를 armed overlay와 함께 반환한다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListCandidatesParams {}

/// get_task 툴 파라미터(dispatcher가 위임한 task의 상태·결과를 확인할 때 사용).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTaskParams {
    /// 조회할 task id.
    pub task_id: String,
}

/// tasks 툴 파라미터(필드 없음). 브로커 전역 열린 task 조망은 대상을 지정할 필요가 없다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksParams {}

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

/// working이 이 초과 갱신정지면 stuck? 표시(claim 후 사망 의심).
const STUCK_WORKING_SECS: i64 = 15 * 60;
/// submitted가 이 초과 미claim이면 no-consumer? 표시(폴러 없음 의심).
const NO_CONSUMER_SUBMITTED_SECS: i64 = 5 * 60;

/// task의 미배달/고착 의심 주석을 만든다(표시 전용, 상태 전이·저장 없음). working은 updated_at(claim
/// 시각) 기준 STUCK_WORKING_SECS 초과면 " ⚠stuck?(<분>m)", submitted는 created_at 기준
/// NO_CONSUMER_SUBMITTED_SECS 초과면 " ⚠no-consumer?(<분>m)"을 붙인다. 그 외(다른 상태, 임계 이내,
/// now 파싱 실패)는 빈 문자열.
fn health_annotation(task: &crate::store::a2a::Task, now: &str) -> String {
    use crate::store::a2a::age_secs;
    match task.state {
        TaskState::Working => match age_secs(now, &task.updated_at) {
            Some(secs) if secs > STUCK_WORKING_SECS => format!(" ⚠stuck?({}m)", secs / 60),
            _ => String::new(),
        },
        TaskState::Submitted => match age_secs(now, &task.created_at) {
            Some(secs) if secs > NO_CONSUMER_SUBMITTED_SECS => format!(" ⚠no-consumer?({}m)", secs / 60),
            _ => String::new(),
        },
        _ => String::new(),
    }
}

/// poll_tasks 순수 로직: agent 앞으로 열린(submitted/working/input_required) task 목록을 사람이 읽기
/// 쉬운 텍스트로 조립한다. SQLite 호출은 하되 MCP/async 계층과 무관해 in-memory store로 단위테스트 가능.
/// 조회 전에 lease 만료 지연 sweep(expire_stale_claims)을 먼저 돌려, 죽은 워커가 물고 있던 task를
/// poll에 반영되기 전에 회수한다(별도 타이머 없이 poll 경로에 얹는 설계).
fn poll_tasks_text(store: &SqliteStore, agent: &str) -> Result<String, String> {
    // sweep 실패는 poll 자체를 막지 않는다(목록 조회는 sweep 여부와 무관하게 계속 유용하므로 로그만).
    if let Err(e) = store.expire_stale_claims() {
        eprintln!("[poll_tasks] expire_stale_claims 실패(무시하고 계속): {e}");
    }
    let tasks = store.list_open_tasks_for(agent)?;
    let now = store.now()?;
    Ok(format_open_tasks(agent, &tasks, &now))
}

/// task 목록을 `[id] from=... state=... msg=...` 줄들로 조립하는 순수 함수(SQLite 없이 테스트 가능).
/// now는 health_annotation(표시 전용 stuck?/no-consumer? 주석)에 쓰인다.
fn format_open_tasks(agent: &str, tasks: &[crate::store::a2a::Task], now: &str) -> String {
    if tasks.is_empty() {
        return format!("{agent} 앞 열린 task 없음");
    }
    tasks
        .iter()
        .map(|t| {
            let msg = t
                .status_message
                .as_ref()
                .and_then(|m| m.parts.first())
                .and_then(|p| p.text.as_deref())
                .unwrap_or("(본문 없음)");
            // ctx=<context_id>는 워커가 프로젝트별 라우팅(--context-map)에 쓴다. 없으면 "-".
            let ctx = t.context_id.as_deref().unwrap_or("-");
            let annotation = health_annotation(t, now);
            format!(
                "[{}] from={} state={}{} ctx={} msg={}",
                t.id,
                t.from_agent,
                t.state.as_str(),
                annotation,
                ctx,
                msg
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// claim_task 순수 로직: task를 working으로 전이하고 확인 텍스트를 만든다. 대상 task가 없거나 이미
/// working 이상으로 전이돼 있으면(다른 워커가 먼저 claim) try_claim이 Err를 반환하고 그대로 위로
/// 전파한다(레이스 컨디션 방지, R2). agent는 lease 소유자(claimed_by)로 기록되어 first-completer-wins
/// 판별에 쓰인다(None이면 하위호환 - claimed_by NULL). runner는 처리하는 러너 종류(트레이스용, v8).
fn claim_task_text(
    store: &SqliteStore,
    task_id: &str,
    agent: Option<&str>,
    runner: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    store.try_claim(task_id, agent, runner)?;
    Ok(format!("착수됨: task_id={task_id} state=working"))
}

/// complete_task 순수 로직: result 텍스트를 단일 Artifact로 감싸 completed로 마감한다. 대상 task가
/// 없으면 Err. artifact_id는 store.new_task_id()로 발급받아 신규 crate 의존 없이 고유성을 확보한다.
/// working 상태가 아니면(예: 아직 claim 안 됨, 또는 이미 completed/canceled로 종료) try_complete가
/// Err를 반환하고 그대로 위로 전파한다(레이스 컨디션 방지, R2). agent는 first-completer-wins 완료자
/// 검증에 쓰인다(claimed_by와 불일치하면 거부, None이면 하위호환 - 가드 무력화).
fn complete_task_text(
    store: &SqliteStore,
    task_id: &str,
    result: &str,
    agent: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    let artifact_id = store.new_task_id()?;
    let artifacts =
        vec![Artifact { artifact_id, name: None, parts: vec![Part { text: Some(result.to_string()), ..Default::default() }] }];
    store.try_complete(task_id, &artifacts, agent)?;
    Ok(format!("완료됨: task_id={task_id} state=completed"))
}

/// fail_task 순수 로직: task를 failed로 전이하고 사유를 상태 메시지로 남긴다. 대상 task가 없으면 Err.
/// 러너 실행이 실패했을 때 completed로 위장하지 않고 failed로 구분해 dispatcher가 성패를 알 수 있게 한다.
/// 이미 completed/canceled로 종료된 task면 try_fail이 Err를 반환하고 그대로 위로 전파한다(레이스
/// 컨디션 방지, R2 - 종료 상태를 failed로 덮어쓰지 못함).
fn fail_task_text(
    store: &SqliteStore,
    task_id: &str,
    reason: &str,
    agent: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    let message_id = store.new_task_id()?;
    let message = Message {
        message_id,
        role: "agent".to_string(),
        parts: vec![Part { text: Some(reason.to_string()), ..Default::default() }],
        task_id: None,
        context_id: None,
    };
    store.try_fail(task_id, Some(&message), agent)?;
    Ok(format!("실패 처리됨: task_id={task_id} state=failed"))
}

/// send_task 순수 로직: text 하나를 A2A Message로 감싸 store::create_task_from_message에 위임한다.
/// message_id는 store.new_task_id()로 발급(신규 crate 의존 없이 고유성 확보, complete_task_text의
/// artifact_id 발급과 같은 관례).
fn send_task_text(
    store: &SqliteStore,
    from_agent: &str,
    to_agent: &str,
    text: &str,
    context_id: Option<String>,
) -> Result<String, String> {
    let message_id = store.new_task_id()?;
    let message = Message {
        message_id,
        role: "user".to_string(),
        parts: vec![Part { text: Some(text.to_string()), ..Default::default() }],
        task_id: None,
        context_id,
    };
    let task = store.create_task_from_message(from_agent, to_agent, message)?;
    Ok(format!("생성됨: task_id={} state={}", task.id, task.state.as_str()))
}

/// AgentEntry 목록을 사람이 읽는 텍스트로 조립한다(비면 "online 에이전트 없음").
pub fn format_agents(agents: &[AgentEntry]) -> String {
    if agents.is_empty() {
        return "online 에이전트 없음".to_string();
    }
    agents
        .iter()
        .map(|a| {
            let name = a.display_name.as_deref().unwrap_or("-");
            let tags = a
                .tags
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}] {} tags: {} (heartbeat={})", a.uuid, name, tags, a.last_heartbeat)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// CandidateEntry 목록을 사람이 읽는 텍스트로 조립한다(비면 "발견된 세션 없음").
/// armed_uuids(online roster 소속)에 있으면 armed, 아니면 candidate로 표시한다.
pub fn format_candidates(
    candidates: &[CandidateEntry],
    armed_uuids: &std::collections::HashSet<String>,
) -> String {
    if candidates.is_empty() {
        return "발견된 세션 없음".to_string();
    }
    candidates
        .iter()
        .map(|c| {
            let armed = if armed_uuids.contains(&c.uuid) { "armed" } else { "candidate" };
            let project = c.project.as_deref().unwrap_or("-");
            let machine = c.machine.as_deref().unwrap_or("-");
            format!(
                "[{}] {}/{} machine={} project={} age={}s ({})",
                c.uuid, c.runner, c.source, machine, project, c.age_secs, armed
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// send_task 셀렉터 인지 버전. Agent면 concrete 발송(send_task_text 위임), Selector면 resolve 후
/// 0개=no-consumer 안내(생성 안 함), 1개=그 uuid로 발송, 2개+=후보 목록(생성 안 함).
fn send_task_routed(
    store: &SqliteStore,
    from_agent: &str,
    to_agent: Option<&str>,
    to_selector: Option<&str>,
    text: &str,
    context_id: Option<String>,
) -> Result<String, String> {
    match validate_send_target(to_agent, to_selector)? {
        SendTarget::Agent(agent) => send_task_text(store, from_agent, &agent, text, context_id),
        SendTarget::Selector(selector) => {
            let sel = parse_tags(&selector)?;
            let now = store.now()?;
            let uuids = store.resolve_selector(&sel, &now, AGENT_TTL_SECS);
            match uuids.len() {
                0 => Ok(format!(
                    "대상 없음(no-consumer): 셀렉터 '{selector}'에 매칭되는 online 에이전트가 없습니다. list_agents로 확인하세요."
                )),
                1 => send_task_text(store, from_agent, &uuids[0], text, context_id),
                _ => Ok(format_ambiguous_candidates(&selector, &uuids)),
            }
        }
    }
}

/// get_task 순수 로직: task를 조회해 상태를 요약한다. completed면 artifact 텍스트들을 이어붙인다.
/// 대상 task가 없어도 Err가 아니라 안내 문구를 Ok로 반환한다(poll_tasks의 빈 목록 관례와 동일 - "없음"은
/// 실패가 아니라 정상적인 조회 결과이므로).
fn get_task_text(store: &SqliteStore, task_id: &str) -> Result<String, String> {
    match store.get_task(task_id)? {
        None => Ok(format!("task 없음: task_id={task_id}")),
        Some(task) => {
            let now = store.now()?;
            Ok(format_task_status(&task, &now))
        }
    }
}

/// task 상태를 `[id] state=...` 한 줄로 조립하고, completed면 artifact 텍스트를 이어붙이는 순수 함수
/// (SQLite 없이 테스트 가능). now는 health_annotation(표시 전용 stuck?/no-consumer? 주석)에 쓰인다.
/// runner가 기록돼 있으면(v8, claim한 워커의 러너 종류) ` runner=<x>`를 덧붙인다. 표시 전용, 없으면 생략.
fn format_task_status(task: &crate::store::a2a::Task, now: &str) -> String {
    let mut out = format!("[{}] state={}{}", task.id, task.state.as_str(), health_annotation(task, now));
    if let Some(runner) = task.runner.as_deref() {
        out.push_str(&format!(" runner={runner}"));
    }
    if task.state == TaskState::Completed {
        let texts: Vec<&str> =
            task.artifacts.iter().flat_map(|a| a.parts.iter()).filter_map(|p| p.text.as_deref()).collect();
        if !texts.is_empty() {
            out.push('\n');
            out.push('\n');
            out.push_str(&texts.join("\n\n"));
        }
    }
    out
}

/// tasks 순수 로직: 브로커 전역에서 열려 있는 task를 to_agent 무관하게 전부 조회해 사람이 읽는 텍스트로
/// 조립한다(운영자 조망용, poll_tasks의 agent 필터판과 대비). health_annotation의 stuck?/no-consumer?
/// 표시가 그대로 붙어 미배달/고착 의심 task를 한눈에 볼 수 있다. poll_tasks_text와 동일하게 조회 전
/// lease 만료 지연 sweep을 먼저 돌린다(운영자 조망에도 최신 회수 결과가 반영되도록).
fn list_all_tasks_text(store: &SqliteStore, now: &str) -> Result<String, String> {
    if let Err(e) = store.expire_stale_claims() {
        eprintln!("[list_all_tasks] expire_stale_claims 실패(무시하고 계속): {e}");
    }
    let tasks = store.list_all_open_tasks()?;
    if tasks.is_empty() {
        return Ok("열린 task 없음".to_string());
    }
    Ok(tasks
        .iter()
        .map(|t| {
            let msg = t
                .status_message
                .as_ref()
                .and_then(|m| m.parts.first())
                .and_then(|p| p.text.as_deref())
                .unwrap_or("(본문 없음)");
            let ctx = t.context_id.as_deref().unwrap_or("-");
            let annotation = health_annotation(t, now);
            format!(
                "[{}] from={} to={} state={}{} ctx={} msg={}",
                t.id,
                t.from_agent,
                t.to_agent,
                t.state.as_str(),
                annotation,
                ctx,
                msg
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n"))
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
            complete_task_text(&store, &task_id, &result, agent.as_deref())
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
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
            fail_task_text(&store, &task_id, &reason, agent.as_deref())
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
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

    #[tool(description = "발견한 미무장 세션 후보를 브로커에 보고한다(발견 리포터용, uuid 단위 upsert).")]
    async fn report_candidates(
        &self,
        Parameters(p): Parameters<ReportCandidatesParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(report_candidates 비활성)".to_string(),
            )]));
        };
        let inputs = p.candidates;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let count = inputs.len();
            let entries: Vec<CandidateEntry> = inputs
                .into_iter()
                .map(|c| CandidateEntry {
                    uuid: c.uuid,
                    runner: c.runner,
                    project: c.project,
                    machine: c.machine,
                    source: c.source,
                    age_secs: c.age_secs,
                    reported_at: now.clone(),
                })
                .collect();
            store.report_candidates(entries, &now);
            Ok::<String, String>(format!("후보 {count}건 보고됨"))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 보고 실패(now 오류)를 success로 위장하지 않는다.
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("보고 실패: {e}"))])),
        }
    }

    #[tool(description = "발견된(미무장) 세션 후보를 조회한다(armed overlay: online roster 소속이면 armed).")]
    async fn list_candidates(
        &self,
        Parameters(_p): Parameters<ListCandidatesParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(list_candidates 비활성)".to_string(),
            )]));
        };
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let candidates = store.list_candidates(&now, CANDIDATE_TTL_SECS);
            // armed overlay: online roster에 있는 uuid는 이미 무장된 것으로 표시.
            let armed: std::collections::HashSet<String> = store
                .resolve_selector(&BTreeMap::new(), &now, AGENT_TTL_SECS)
                .into_iter()
                .collect();
            Ok::<String, String>(format_candidates(&candidates, &armed))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 조회 실패(now 오류)를 success로 위장하지 않는다.
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!("조회 실패: {e}"))])),
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
                 발견 리포터는 report_candidates(candidates)로 미무장 세션 후보를 보고하고 list_candidates()로 후보를 조회합니다(armed=online roster 소속). \
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

/// HTTP MCP 서버를 기동한다. serve 피처 전용.
#[cfg(feature = "serve")]
pub async fn start_http_mcp_server(
    addr: &str,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    writer: Option<Arc<dyn TranscriptWriter>>,
    roster: Option<Vec<RosterSeat>>,
    token: Option<String>,
    a2a_store: Arc<std::sync::Mutex<crate::store::sqlite::SqliteStore>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_http_mcp_on_listener(listener, retriever, reader, writer, roster, token, a2a_store).await
}

/// 이미 바인드된 TcpListener로 HTTP MCP 서버를 서빙한다(테스트에서도 재사용).
/// 같은 axum app에 MCP(`/mcp`)와 A2A(`/a2a`, `/.well-known/agent-card.json`)를 함께 마운트한다
/// (docs/design/v2-a2a-partner-delegation_2026-07-02.md §4: "코어 = A2A 서버 + 기존 axum HTTP 재사용").
#[cfg(feature = "serve")]
pub async fn serve_http_mcp_on_listener(
    listener: tokio::net::TcpListener,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    writer: Option<Arc<dyn TranscriptWriter>>,
    roster: Option<Vec<RosterSeat>>,
    token: Option<String>,
    a2a_store: Arc<std::sync::Mutex<crate::store::sqlite::SqliteStore>>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use axum::{
        Router,
        extract::Request,
        http::{StatusCode, header::AUTHORIZATION},
        middleware::{self, Next},
        response::IntoResponse,
        routing::get,
    };
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    // A2A Agent Card는 bind 주소에서 파생되는 정적 값이라 router 조립 전에 먼저 만든다.
    let bound_addr = listener.local_addr()?;
    let a2a_url = core_a2a_url(&bound_addr.to_string());
    let agent_card = crate::a2a_server::build_agent_card(&a2a_url);
    // MCP inbox 툴(poll_tasks/claim_task/complete_task)도 같은 a2a_store Arc를 공유한다(새 커넥션을
    // 만들지 않고 단일 mutex로 직렬화. Phase 1 저볼륨 전제. docs/design/v2-a2a-partner-delegation_2026-07-02.md §10-1).
    let a2a_store_for_mcp = a2a_store.clone();
    // 대시보드 SSE/roster용 clone(같은 store = 같은 이벤트버스·로스터를 공유한다).
    let a2a_store_for_dash = a2a_store.clone();
    let a2a_router = crate::a2a_server::build_router(a2a_store, agent_card);

    let retriever2 = retriever.clone();
    let reader2 = reader.clone();
    let writer2 = writer.clone();
    let roster2 = roster.clone();
    // service_factory: 요청마다 새 TunaSearchServer 인스턴스를 생성한다(Clone 불필요, Arc 공유).
    let service: StreamableHttpService<TunaSearchServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                let mut s =
                    TunaSearchServer::new(retriever2.clone()).with_a2a_store(a2a_store_for_mcp.clone());
                if let Some(r) = &reader2 {
                    s = s.with_transcript_reader(r.clone());
                }
                if let Some(w) = &writer2 {
                    s = s.with_transcript_writer(w.clone());
                }
                if let Some(rs) = &roster2 {
                    s = s.with_roster(rs.clone());
                }
                Ok(s)
            },
            Default::default(), // Arc::new(LocalSessionManager::default())
            // 원격 에이전트 접속을 위해 호스트 제한을 해제하고 bearer 토큰으로 인증한다.
            StreamableHttpServerConfig::default().disable_allowed_hosts(),
        );

    // MCP(/mcp)와 A2A(/a2a, /.well-known/agent-card.json)를 같은 axum app으로 병합한다.
    let merged = Router::new().nest_service("/mcp", service).merge(a2a_router);

    let authed: Router = if let Some(tok) = token {
        let tok = Arc::new(tok);
        let bearer = middleware::from_fn(move |request: Request, next: Next| {
            let tok = tok.clone();
            async move {
                let auth = request
                    .headers()
                    .get(AUTHORIZATION)
                    .and_then(|v| v.to_str().ok())
                    .unwrap_or("");
                let expected = format!("Bearer {tok}");
                if auth == expected {
                    next.run(request).await
                } else {
                    StatusCode::UNAUTHORIZED.into_response()
                }
            }
        });
        merged.layer(bearer)
    } else {
        merged
    };

    // 대시보드 라우트(무인증 outer, read-only). events/roster API는 serve 피처에 항상 존재한다
    // (SPA 유무 무관, 다른 클라이언트도 쓴다). SPA 정적 에셋은 dashboard 피처에서만 임베드 서빙하고,
    // 피처가 없으면 /dashboard는 안내 페이지다. write(goal 폼)는 SPA가 /a2a bearer로 게이트한다.
    let mut dashboard = Router::new()
        .route("/dashboard/events", get(dashboard_events_handler))
        .route("/dashboard/roster", get(dashboard_roster_handler))
        .route("/dashboard/candidates", get(dashboard_candidates_handler))
        .route("/dashboard/goal", axum::routing::post(dashboard_goal_handler))
        .route("/dashboard/control", axum::routing::post(dashboard_control_handler));
    #[cfg(feature = "dashboard")]
    {
        // Vite base=/dashboard/라 에셋은 /dashboard/assets/*로 나가 events/roster와 경로 미충돌.
        dashboard = dashboard
            .route("/dashboard", get(dashboard_index))
            .route("/dashboard/favicon.svg", get(dashboard_favicon))
            .route("/dashboard/assets/{*path}", get(dashboard_asset));
    }
    #[cfg(not(feature = "dashboard"))]
    {
        dashboard = dashboard.route("/dashboard", get(dashboard_fallback_page));
    }
    let app = dashboard.with_state(a2a_store_for_dash).merge(authed);

    #[cfg(feature = "dashboard")]
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드 SPA: /dashboard)");
    #[cfg(not(feature = "dashboard"))]
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드: dashboard 피처 없이 빌드됨)");
    // ConnectInfo(peer addr)로 /dashboard/goal의 loopback 판정을 하기 위해 connect-info make service를 쓴다.
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
    Ok(())
}

/// 대시보드 SPA(dashboard 피처) 임베드 자산. release=바이너리 내장, debug=디스크(frontend/dist) 읽기.
/// frontend를 `npm run build`한 뒤 `cargo build --features dashboard`로 임베드한다.
#[cfg(feature = "dashboard")]
#[derive(rust_embed::RustEmbed)]
#[folder = "frontend/dist"]
struct DashAssets;

/// 임베드된 SPA 자산 하나를 확장자 기반 MIME으로 서빙한다(없으면 404).
#[cfg(feature = "dashboard")]
fn serve_embedded(path: &str) -> axum::response::Response {
    use axum::response::IntoResponse;
    match DashAssets::get(path) {
        Some(content) => {
            ([(axum::http::header::CONTENT_TYPE, mime_for_path(path))], content.data.into_owned()).into_response()
        }
        None => axum::http::StatusCode::NOT_FOUND.into_response(),
    }
}

/// 경로 확장자로 정적 자산 Content-Type을 고른다(SPA 번들이 쓰는 종류만, 신규 의존 회피).
#[cfg(feature = "dashboard")]
fn mime_for_path(path: &str) -> &'static str {
    match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("woff2") => "font/woff2",
        Some("woff") => "font/woff",
        Some("json") => "application/json",
        Some("ico") => "image/x-icon",
        Some("png") => "image/png",
        _ => "application/octet-stream",
    }
}

/// GET /dashboard: SPA 진입 index.html.
#[cfg(feature = "dashboard")]
async fn dashboard_index() -> axum::response::Response {
    serve_embedded("index.html")
}

/// GET /dashboard/favicon.svg: SPA 파비콘.
#[cfg(feature = "dashboard")]
async fn dashboard_favicon() -> axum::response::Response {
    serve_embedded("favicon.svg")
}

/// GET /dashboard/assets/{*path}: Vite 번들 자산(js/css/폰트 등).
#[cfg(feature = "dashboard")]
async fn dashboard_asset(
    axum::extract::Path(path): axum::extract::Path<String>,
) -> axum::response::Response {
    serve_embedded(&format!("assets/{path}"))
}

/// dashboard 피처 없이 빌드된 경우의 /dashboard 안내 페이지(API events/roster는 그대로 동작).
#[cfg(all(feature = "serve", not(feature = "dashboard")))]
async fn dashboard_fallback_page() -> axum::response::Html<&'static str> {
    axum::response::Html(
        "<!DOCTYPE html><html lang=\"ko\"><head><meta charset=\"utf-8\"><title>총감독 대시보드</title></head>\
         <body style=\"font-family:system-ui;margin:2rem\"><h1>대시보드 미포함 빌드</h1>\
         <p>이 바이너리는 <code>dashboard</code> 피처 없이 빌드되었습니다. \
         <code>cargo build --features dashboard</code>로 빌드하거나 release 바이너리를 사용하세요. \
         API <code>/dashboard/events</code>, <code>/dashboard/roster</code>는 동작합니다.</p></body></html>",
    )
}

/// 전역 task 이벤트를 JSON data 문자열로 흘리는 순수 스트림(단위테스트 대상). task_id 필터 없이 모든
/// TaskEvent를 내보낸다. Lagged는 스킵하고 계속, Closed면 종료한다.
#[cfg(feature = "serve")]
fn dashboard_event_json_stream(
    rx: tokio::sync::broadcast::Receiver<crate::store::a2a::TaskEvent>,
) -> impl futures_util::Stream<Item = String> {
    use crate::store::a2a::TaskEvent;
    futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    let (kind, task) = match &ev {
                        TaskEvent::Status(t) => ("status", t),
                        TaskEvent::Completed(t) => ("completed", t),
                    };
                    let envelope = serde_json::json!({ "event": kind, "task": task });
                    let data = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                    return Some((data, rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// 위 JSON 문자열을 axum SSE Event로 감싼다(HTTP 핸들러 전용 얇은 래퍼).
#[cfg(feature = "serve")]
fn dashboard_event_stream(
    rx: tokio::sync::broadcast::Receiver<crate::store::a2a::TaskEvent>,
) -> impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> {
    use futures_util::StreamExt;
    dashboard_event_json_stream(rx).map(|data| Ok(axum::response::sse::Event::default().data(data)))
}

/// GET /dashboard/events: 전역 task 이벤트 SSE(대시보드 라이브 피드). 브라우저 EventSource가 구독한다.
#[cfg(feature = "serve")]
async fn dashboard_events_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let sender = {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        store.task_event_sender()
    };
    let Some(sender) = sender else {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "task event bus 미활성").into_response();
    };
    let rx = sender.subscribe();
    let stream = dashboard_event_stream(rx);
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// GET /dashboard/roster: online 감독 roster(list_agents, 빈 selector = 전체) JSON. 브라우저가 주기 폴.
/// axum "json" 피처(신규 의존) 없이 serde_json(기존 의존)만으로 application/json 응답을 만든다.
#[cfg(feature = "serve")]
async fn dashboard_roster_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // 대시보드는 오프라인 감독도 회색 닷으로 보여줘야 하므로 전체를 반환하고 online 플래그를 붙인다
    // (라우팅용 list_agents는 online만 반환하지만, 이 뷰는 등록된 전원 + 상태를 노출한다).
    #[derive(serde::Serialize)]
    struct DashAgent {
        uuid: String,
        tags: BTreeMap<String, String>,
        display_name: Option<String>,
        last_heartbeat: String,
        online: bool,
    }
    let agents: Vec<DashAgent> = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        // TTL=i64::MAX로 오프라인 포함 전체 조회 후, online은 실제 TTL(AGENT_TTL_SECS)로 per-agent 계산.
        store
            .list_agents(&BTreeMap::new(), &now, i64::MAX)
            .into_iter()
            .map(|a| {
                let online = crate::store::agents::is_online(&a.last_heartbeat, &now, AGENT_TTL_SECS);
                DashAgent {
                    uuid: a.uuid,
                    tags: a.tags,
                    display_name: a.display_name,
                    last_heartbeat: a.last_heartbeat,
                    online,
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    let body = serde_json::to_vec(&agents).unwrap_or_else(|_| b"[]".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

/// GET /dashboard/candidates: 발견된(미무장) 세션 후보 JSON. 브라우저가 주기 폴(S3 "발견된 세션" 패널).
/// armed는 저장값이 아니라 online roster 소속으로 계산한 overlay다(무장되면 자동 armed=true로 승격 표시).
#[cfg(feature = "serve")]
async fn dashboard_candidates_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    #[derive(serde::Serialize)]
    struct DashCandidate {
        uuid: String,
        runner: String,
        project: Option<String>,
        machine: Option<String>,
        source: String,
        age_secs: i64,
        reported_at: String,
        armed: bool,
    }
    let candidates: Vec<DashCandidate> = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        // armed overlay: online roster(AGENT_TTL_SECS)에 있는 uuid는 이미 무장된 것으로 표시.
        let armed: std::collections::HashSet<String> = store
            .resolve_selector(&BTreeMap::new(), &now, AGENT_TTL_SECS)
            .into_iter()
            .collect();
        store
            .list_candidates(&now, CANDIDATE_TTL_SECS)
            .into_iter()
            .map(|c| {
                let is_armed = armed.contains(&c.uuid);
                DashCandidate {
                    uuid: c.uuid,
                    runner: c.runner,
                    project: c.project,
                    machine: c.machine,
                    source: c.source,
                    age_secs: c.age_secs,
                    reported_at: c.reported_at,
                    armed: is_armed,
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    let body = serde_json::to_vec(&candidates).unwrap_or_else(|_| b"[]".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

/// POST /dashboard/goal: 로컬(loopback) 총감독이 선택한 감독들에게 목표를 던진다(대상마다 1 task).
/// 원격(비-loopback)은 read-only 관전이라 403. 무인증이지만 loopback 신뢰라 로컬 한정 write.
/// body = {"text": "...", "targets": ["uuid", ...]}. 응답 = {"created":[{taskId,toAgent}], "errors":[...]}.
#[cfg(feature = "serve")]
async fn dashboard_goal_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // loopback만 목표 제출 허용. 원격은 관전 전용.
    if !peer.ip().is_loopback() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 관전 모드: 목표 제출은 로컬(loopback)에서만 가능합니다.",
        )
            .into_response();
    }
    #[derive(serde::Deserialize)]
    struct GoalReq {
        text: String,
        targets: Vec<String>,
    }
    let req: GoalReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
    };
    if req.text.trim().is_empty() || req.targets.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "목표(text)와 대상(targets)이 필요합니다.").into_response();
    }
    #[derive(serde::Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Created {
        task_id: String,
        to_agent: String,
    }
    #[derive(serde::Serialize)]
    struct GoalResp {
        created: Vec<Created>,
        errors: Vec<String>,
    }
    let resp = tokio::task::spawn_blocking(move || {
        use crate::store::a2a::{Message, Part};
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let mut created = Vec::new();
        let mut errors = Vec::new();
        for uuid in &req.targets {
            let msg_id = match store.new_task_id() {
                Ok(id) => id,
                Err(e) => {
                    errors.push(format!("{uuid}: {e}"));
                    continue;
                }
            };
            let message = Message {
                message_id: msg_id,
                role: "user".to_string(),
                parts: vec![Part { text: Some(req.text.clone()), ..Default::default() }],
                task_id: None,
                context_id: None,
            };
            match store.create_task_from_message("dashboard", uuid, message) {
                Ok(task) => created.push(Created { task_id: task.id, to_agent: task.to_agent }),
                Err(e) => errors.push(format!("{uuid}: {e}")),
            }
        }
        GoalResp { created, errors }
    })
    .await
    .unwrap_or(GoalResp { created: vec![], errors: vec!["작업 실패".to_string()] });
    let bodyv = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bodyv,
    )
        .into_response()
}


/// POST /dashboard/control: 로컬(loopback) 총감독이 codex app-server 세션에 turn/start를 직접 주입한다
/// (v2-40 S4). body = {"ws":"ws://127.0.0.1:8790","text":"지시","agent"?:"...","timeout"?:300}.
/// 응답 = {"answer":"codex 최종답"} 또는 에러. 원격(비-loopback)은 403(관전 전용). 실제 주입은 worker 피처.
#[cfg(feature = "serve")]
async fn dashboard_control_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // loopback만 제어 허용. 원격은 관전 전용(goal과 동일 신뢰 경계).
    if !peer.ip().is_loopback() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 관전 모드: codex 제어는 로컬(loopback)에서만 가능합니다.",
        )
            .into_response();
    }
    // agent·timeout은 worker 피처 빌드에서만 읽힌다(worker 없이 빌드하면 501). 조건부 DTO라 dead_code 허용.
    #[derive(serde::Deserialize)]
    #[cfg_attr(not(feature = "worker"), allow(dead_code))]
    struct ControlReq {
        ws: String,
        text: String,
        agent: Option<String>,
        timeout: Option<u64>,
    }
    let req: ControlReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
    };
    if req.ws.trim().is_empty() || req.text.trim().is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "ws와 text가 필요합니다.").into_response();
    }
    #[cfg(feature = "worker")]
    {
        let agent = req.agent.unwrap_or_else(|| "dashboard-control".to_string());
        let timeout = req.timeout.unwrap_or(300);
        // 제어 주입은 tuna-broker MCP 호출 자동승인(never) + workspace-write(감독 레시피와 동일).
        let approval = crate::codex_appserver::ApprovalPolicy::Never;
        let sandbox = crate::codex_appserver::SandboxMode::WorkspaceWrite;
        match crate::codex_inject::run(&req.ws, &agent, &req.text, approval, sandbox, timeout, false).await {
            Ok(answer) => {
                #[derive(serde::Serialize)]
                struct Resp {
                    answer: String,
                }
                let bodyv = serde_json::to_vec(&Resp { answer }).unwrap_or_else(|_| b"{}".to_vec());
                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    bodyv,
                )
                    .into_response()
            }
            Err(e) => (axum::http::StatusCode::BAD_GATEWAY, format!("codex 제어 실패: {e}")).into_response(),
        }
    }
    #[cfg(not(feature = "worker"))]
    {
        let _ = req;
        (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            "worker 피처 없이 빌드됨: codex 제어(codex-inject) 비활성입니다.",
        )
            .into_response()
    }
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

    // HTTP MCP 서버 통합 테스트: serve 피처 전용.
    #[cfg(feature = "serve")]
    mod http_serve {
        use super::super::*;

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

        /// initialize 요청 본문(MCP 2025-03-26 프로토콜).
        const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

        /// 공유 벡터를 쓰는 가짜 writer + 읽는 가짜 reader(HTTP 통합 e2e용).
        #[derive(Clone, Default)]
        struct SharedLog(std::sync::Arc<std::sync::Mutex<Vec<(String, String)>>>);
        impl crate::orchestrator::TranscriptWriter for SharedLog {
            fn append_turn(&self, _sid: &str, speaker: &str, content: &str) -> Result<u64, String> {
                let mut v = self.0.lock().unwrap();
                v.push((speaker.to_string(), content.to_string()));
                Ok(v.len() as u64)
            }
        }
        impl crate::orchestrator::TranscriptReader for SharedLog {
            fn read_transcript(
                &self,
                _sid: &str,
                _max: Option<usize>,
            ) -> Result<Vec<crate::orchestrator::Utterance>, String> {
                Ok(self
                    .0
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|(s, c)| crate::orchestrator::Utterance { speaker: s.clone(), content: c.clone() })
                    .collect())
            }
        }

        /// tools/call 본문 생성.
        fn call_body(id: u32, name: &str, args: &str) -> String {
            format!(
                r#"{{"jsonrpc":"2.0","id":{id},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#
            )
        }

        /// serve_http_mcp_on_listener 테스트 호출용 인메모리 A2A store(MCP 자체와 무관, 배선 검증용).
        fn test_a2a_store() -> Arc<std::sync::Mutex<crate::store::sqlite::SqliteStore>> {
            Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory().expect("in-memory sqlite"),
            ))
        }

        /// HTTP MCP로 get_roster·post_turn·read_transcript를 실제 왕복 검증한다.
        /// 핸드셰이크: initialize→(mcp-session-id 캡처)→initialized→tools/call들.
        #[tokio::test]
        async fn http_post_turn_get_roster_read_transcript_e2e() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().unwrap().port();

            let log = SharedLog::default();
            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let reader = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptReader>);
            let writer = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptWriter>);
            let roster = Some(vec![
                RosterSeat { engine: "claude".into(), role: Some("proposer".into()) },
                RosterSeat { engine: "codex".into(), role: Some("reviewer".into()) },
            ]);
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, reader, writer, roster, None, test_a2a_store()).await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");
            let accept = "application/json, text/event-stream";

            // initialize → mcp-session-id 헤더 캡처.
            let init = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", accept)
                .body(INIT_BODY)
                .send()
                .await
                .expect("init");
            assert_eq!(init.status(), 200);
            let sid = init
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .expect("mcp-session-id 헤더 필요");

            // initialized 알림(세션 헤더 포함).
            let _ = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", accept)
                .header("mcp-session-id", &sid)
                .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .send()
                .await
                .expect("initialized");

            let post = |body: String| {
                let client = client.clone();
                let url = url.clone();
                let sid = sid.clone();
                async move {
                    client
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .header("Accept", accept)
                        .header("mcp-session-id", &sid)
                        .body(body)
                        .send()
                        .await
                        .expect("call")
                        .text()
                        .await
                        .expect("text")
                }
            };

            // get_roster → 좌석 목록.
            let roster_text = post(call_body(2, "get_roster", "{}")).await;
            assert!(roster_text.contains("claude (proposer)"), "get_roster 응답: {roster_text}");

            // post_turn → 추가됨.
            let post_text =
                post(call_body(3, "post_turn", r#"{"speaker":"remote/agent","content":"원격 발언 핵심어 살구"}"#)).await;
            assert!(post_text.contains("msg_id="), "post_turn 응답: {post_text}");

            // read_transcript → 방금 post한 발언이 보임(쓰기→읽기 일관).
            let read_text = post(call_body(4, "read_transcript", "{}")).await;
            assert!(read_text.contains("살구"), "read_transcript에 post_turn 내용 없음: {read_text}");
        }

        /// HTTP MCP로 poll_tasks→claim_task→complete_task 왕복을 검증한다. Task 2(a2a_server)가 만든
        /// a2a_store Arc를 serve_http_mcp_on_listener가 TunaSearchServer와 실제로 공유하는지까지 확인한다.
        #[tokio::test]
        async fn http_poll_claim_complete_task_e2e() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().unwrap().port();

            let store = test_a2a_store();
            // 미리 task 하나를 심어둔다(mac-claude 앞).
            let seeded_id = {
                let s = store.lock().unwrap();
                let now = s.now().unwrap();
                let id = s.new_task_id().unwrap();
                let task = crate::store::a2a::Task::new(id, None, "win-claude", "mac-claude", now);
                s.create_task(&task).unwrap();
                task.id
            };

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ =
                    serve_http_mcp_on_listener(listener, retriever, None, None, None, None, store_for_server).await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");
            let accept = "application/json, text/event-stream";

            let init = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", accept)
                .body(INIT_BODY)
                .send()
                .await
                .expect("init");
            let sid = init
                .headers()
                .get("mcp-session-id")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
                .expect("mcp-session-id 헤더 필요");
            let _ = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", accept)
                .header("mcp-session-id", &sid)
                .body(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#)
                .send()
                .await
                .expect("initialized");

            let post = |body: String| {
                let client = client.clone();
                let url = url.clone();
                let sid = sid.clone();
                async move {
                    client
                        .post(&url)
                        .header("Content-Type", "application/json")
                        .header("Accept", accept)
                        .header("mcp-session-id", &sid)
                        .body(body)
                        .send()
                        .await
                        .expect("call")
                        .text()
                        .await
                        .expect("text")
                }
            };

            // poll_tasks → 심어둔 task가 보임.
            let poll_text = post(call_body(2, "poll_tasks", r#"{"agent":"mac-claude"}"#)).await;
            assert!(poll_text.contains(&seeded_id), "poll_tasks 응답에 task_id 없음: {poll_text}");

            // claim_task → working 전이.
            let claim_body = format!(r#"{{"task_id":"{seeded_id}"}}"#);
            let claim_text = post(call_body(3, "claim_task", &claim_body)).await;
            assert!(claim_text.contains("state=working"), "claim_task 응답: {claim_text}");

            // complete_task → completed 전이 + artifact 저장.
            let complete_body = format!(r#"{{"task_id":"{seeded_id}","result":"작업 결과 요약"}}"#);
            let complete_text = post(call_body(4, "complete_task", &complete_body)).await;
            assert!(complete_text.contains("state=completed"), "complete_task 응답: {complete_text}");

            // DB 상태 최종 확인(HTTP 왕복 후 실제로 반영됐는지. serve_http_mcp_on_listener가 넘겨받은
            // 그 a2a_store Arc가 TunaSearchServer 쪽에도 공유됐다는 증거).
            let final_task = store.lock().unwrap().get_task(&seeded_id).unwrap().expect("존재해야 함");
            assert_eq!(final_task.state, TaskState::Completed);
            assert_eq!(final_task.artifacts.len(), 1);
            assert_eq!(final_task.artifacts[0].parts[0].text.as_deref(), Some("작업 결과 요약"));
        }

        #[test]
        fn core_local_url_maps_wildcards_to_loopback() {
            // 와일드카드 host는 loopback으로, 일반 host는 그대로.
            assert_eq!(core_local_url("0.0.0.0:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("[::]:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("127.0.0.1:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("192.0.2.20:9000"), "http://192.0.2.20:9000/mcp");
        }

        #[test]
        fn core_a2a_url_mirrors_core_local_url_with_a2a_suffix() {
            // core_local_url과 동일한 host 매핑 + /a2a 접미사(Agent Card url 필드용).
            assert_eq!(core_a2a_url("0.0.0.0:8771"), "http://127.0.0.1:8771/a2a");
            assert_eq!(core_a2a_url("127.0.0.1:8771"), "http://127.0.0.1:8771/a2a");
        }

        #[tokio::test]
        async fn http_mcp_bearer_auth() {
            // 포트 :0 으로 바인드해 OS가 빈 포트를 할당하도록 한다(포트 경합 없음).
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let token = Some("secret-tok".to_string());

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, None, None, token, test_a2a_store()).await;
            });
            // axum이 accept를 시작할 시간을 준다.
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");

            // 토큰 없음 → 401.
            let resp = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(INIT_BODY)
                .send()
                .await
                .expect("요청 실패");
            assert_eq!(resp.status(), 401, "토큰 없이 401이어야 함");

            // 잘못된 토큰 → 401.
            let resp = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .header("Authorization", "Bearer wrongtoken")
                .body(INIT_BODY)
                .send()
                .await
                .expect("요청 실패");
            assert_eq!(resp.status(), 401, "잘못된 토큰으로 401이어야 함");

            // 올바른 토큰 → 200(MCP initialize 핸드셰이크 성공).
            let resp = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .header("Authorization", "Bearer secret-tok")
                .body(INIT_BODY)
                .send()
                .await
                .expect("요청 실패");
            assert_eq!(resp.status(), 200, "올바른 토큰으로 200이어야 함");

            // A2A 라우트도 같은 bearer 미들웨어를 공유한다(마운트·인증 재사용 확인).
            let card_url = format!("http://127.0.0.1:{port}/.well-known/agent-card.json");
            let resp = client.get(&card_url).send().await.expect("요청 실패");
            assert_eq!(resp.status(), 401, "A2A도 토큰 없이 401이어야 함");
            let resp = client
                .get(&card_url)
                .header("Authorization", "Bearer secret-tok")
                .send()
                .await
                .expect("요청 실패");
            assert_eq!(resp.status(), 200, "A2A도 올바른 토큰으로 200이어야 함");
        }

        #[tokio::test]
        async fn http_mcp_no_token_allows_all() {
            // token=None이면 미들웨어 없이 모든 요청 통과.
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, None, None, None, test_a2a_store()).await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;

            let client = reqwest::Client::new();
            let url = format!("http://127.0.0.1:{port}/mcp");

            // token=None 이므로 인증 헤더 없이도 200.
            let resp = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(INIT_BODY)
                .send()
                .await
                .expect("요청 실패");
            assert_eq!(resp.status(), 200, "token=None이면 200이어야 함");

            // A2A 라우트도 같은 app에 마운트되어 응답한다(404가 아님).
            let card_url = format!("http://127.0.0.1:{port}/.well-known/agent-card.json");
            let resp = client.get(&card_url).send().await.expect("요청 실패");
            assert_eq!(resp.status(), 200, "agent-card.json이 마운트되어야 함");
            let body: serde_json::Value = resp.json().await.expect("agent card json 파싱");
            assert_eq!(body["name"], "tunaround-core");
        }
    }

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

    #[test]
    fn format_open_tasks_empty_says_no_open_tasks() {
        let text = format_open_tasks("mac-claude", &[], "2026-07-02 09:00:00");
        assert!(text.contains("mac-claude"), "agent 언급 없음: {text}");
        assert!(text.contains("없음"), "빈 목록 안내가 아님: {text}");
    }

    #[test]
    fn format_open_tasks_lists_task_id_from_agent_state_and_message() {
        let mut task =
            crate::store::a2a::Task::new("t1", None, "win-claude", "mac-claude", "2026-07-02 09:00:00");
        task.status_message = Some(crate::store::a2a::Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part { text: Some("리뷰 부탁".into()), ..Default::default() }],
            task_id: Some("t1".into()),
            context_id: None,
        });
        // now를 created_at과 같게 둬 stuck?/no-consumer? 주석이 안 붙게 한다(이 테스트는 그 표시를 검증하지 않음).
        let text = format_open_tasks("mac-claude", &[task], "2026-07-02 09:00:00");
        assert!(text.contains("t1"), "task id 누락: {text}");
        assert!(text.contains("win-claude"), "from_agent 누락: {text}");
        assert!(text.contains("submitted"), "state 누락: {text}");
        assert!(text.contains("리뷰 부탁"), "메시지 본문 누락: {text}");
    }

    #[test]
    fn poll_tasks_text_filters_agent_and_excludes_completed() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00"); // open, mac 앞.
        let mut t2 = crate::store::a2a::Task::new("t2", None, "win", "mac", "2026-07-02 09:05:00");
        t2.state = TaskState::Completed;
        store.create_task(&t2).unwrap(); // completed, mac 앞 → 제외돼야 함.
        seed_task(&store, "t3", "win", "other", "2026-07-02 09:10:00"); // open, other 앞 → 제외돼야 함.

        let text = poll_tasks_text(&store, "mac").unwrap();
        assert!(text.contains("t1"), "열린 task 누락: {text}");
        assert!(!text.contains("t2"), "completed가 섞여 들어옴: {text}");
        assert!(!text.contains("t3"), "다른 agent 앞 task가 섞여 들어옴: {text}");
    }

    #[test]
    fn poll_tasks_text_sweeps_expired_lease_before_listing() {
        // poll_tasks_text 호출 자체가 지연 sweep을 트리거해, 죽은 워커가 물고 있던 task가 다시
        // submitted로 노출되어야 한다(별도 타이머 없이 poll 경로에 얹는 설계의 핵심).
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        store.try_claim("t1", Some("worker-a"), None).unwrap();
        // lease를 과거로 강제 심어 만료를 시뮬레이션(테스트 전용 pub(crate) 헬퍼, conn은 private).
        store.test_force_lease_expired("t1");

        let text = poll_tasks_text(&store, "mac").unwrap();
        assert!(text.contains("state=submitted"), "sweep 후 submitted로 복귀 안 됨: {text}");
    }

    #[test]
    fn claim_task_text_transitions_to_working() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = claim_task_text(&store, "t1", None, None).unwrap();
        assert!(text.contains("state=working"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[test]
    fn claim_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = claim_task_text(&store, "nope", None, None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    #[test]
    fn complete_task_text_sets_completed_with_artifact() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        claim_task_text(&store, "t1", None, None).unwrap();
        let text = complete_task_text(&store, "t1", "작업 결과", None).unwrap();
        assert!(text.contains("state=completed"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Completed);
        assert_eq!(reloaded.artifacts.len(), 1);
        assert_eq!(reloaded.artifacts[0].parts[0].text.as_deref(), Some("작업 결과"));
    }

    #[test]
    fn complete_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = complete_task_text(&store, "nope", "결과", None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    #[test]
    fn fail_task_text_sets_failed_with_reason() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = fail_task_text(&store, "t1", "러너 타임아웃", None).unwrap();
        assert!(text.contains("state=failed"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Failed);
        // 사유는 상태 메시지로 남아 dispatcher가 읽을 수 있다.
        assert_eq!(
            reloaded.status_message.and_then(|m| m.parts[0].text.clone()).as_deref(),
            Some("러너 타임아웃")
        );
    }

    #[test]
    fn fail_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = fail_task_text(&store, "nope", "사유", None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    // --- health_annotation(표시 전용 stuck?/no-consumer? 주석): 순수 함수 단위테스트 ---

    #[test]
    fn health_annotation_working_stuck_past_threshold() {
        let mut task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        task.state = TaskState::Working;
        task.updated_at = "2026-07-02 09:00:00".into(); // claim 시각.
        // STUCK_WORKING_SECS(15분) 초과: 09:00:00 -> 09:20:00 = 20분.
        let annotation = health_annotation(&task, "2026-07-02 09:20:00");
        assert!(annotation.contains("stuck?"), "stuck 표시 누락: {annotation}");
        assert!(annotation.contains("20m"), "경과분 표시 불일치: {annotation}");
    }

    #[test]
    fn health_annotation_submitted_no_consumer_past_threshold() {
        let task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        // NO_CONSUMER_SUBMITTED_SECS(5분) 초과: 09:00:00 -> 09:10:00 = 10분.
        let annotation = health_annotation(&task, "2026-07-02 09:10:00");
        assert!(annotation.contains("no-consumer?"), "no-consumer 표시 누락: {annotation}");
        assert!(annotation.contains("10m"), "경과분 표시 불일치: {annotation}");
    }

    #[test]
    fn health_annotation_recent_task_is_empty() {
        let task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        // 임계(5분) 이내: 09:00:00 -> 09:01:00 = 1분.
        let annotation = health_annotation(&task, "2026-07-02 09:01:00");
        assert_eq!(annotation, "", "임계 이내인데 주석이 붙음: {annotation}");
    }

    #[test]
    fn health_annotation_terminal_state_is_always_empty() {
        let mut task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        task.state = TaskState::Completed;
        task.updated_at = "2026-07-02 09:00:00".into();
        // 아주 오래 지났어도 종료 상태(completed)는 주석을 붙이지 않는다.
        let annotation = health_annotation(&task, "2026-07-03 09:00:00");
        assert_eq!(annotation, "", "종료 상태인데 주석이 붙음: {annotation}");
    }

    // --- tasks 툴(list_all_tasks_text): 순수 함수 단위테스트 ---

    #[test]
    fn list_all_tasks_text_mixes_multiple_to_agents() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        seed_task(&store, "t2", "win", "codex", "2026-07-02 09:05:00");
        let text = list_all_tasks_text(&store, "2026-07-02 09:06:00").unwrap();
        assert!(text.contains("t1"), "t1 누락: {text}");
        assert!(text.contains("to=mac"), "to=mac 누락: {text}");
        assert!(text.contains("t2"), "t2 누락: {text}");
        assert!(text.contains("to=codex"), "to=codex 누락: {text}");
    }

    #[test]
    fn list_all_tasks_text_shows_no_consumer_annotation_for_stale_submitted() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // now를 미래로 둬 NO_CONSUMER_SUBMITTED_SECS(5분)을 넘긴다.
        let text = list_all_tasks_text(&store, "2026-07-02 09:30:00").unwrap();
        assert!(text.contains("no-consumer?"), "no-consumer 주석 누락: {text}");
    }

    #[test]
    fn list_all_tasks_text_empty_says_no_open_tasks() {
        let store = SqliteStore::open_memory().unwrap();
        let text = list_all_tasks_text(&store, "2026-07-02 09:00:00").unwrap();
        assert!(text.contains("없음"), "빈 목록 안내가 아님: {text}");
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

    // --- A2A dispatcher 툴(send_task/get_task): 순수 함수 단위테스트 ---

    #[test]
    fn send_task_text_creates_submitted_task_and_preserves_text() {
        let store = SqliteStore::open_memory().unwrap();
        let text =
            send_task_text(&store, "win-claude", "mac-claude", "리뷰 부탁", Some("ctx1".into())).unwrap();
        assert!(text.contains("state=submitted"), "응답 불일치: {text}");

        // store에 실제로 submitted task가 생겼는지, 메시지 본문이 보존됐는지 확인(round-trip).
        let tasks = store.list_open_tasks_for("mac-claude").unwrap();
        assert_eq!(tasks.len(), 1, "mac-claude 앞 task 하나가 생겨야 함");
        let task = &tasks[0];
        assert_eq!(task.from_agent, "win-claude");
        assert_eq!(task.context_id.as_deref(), Some("ctx1"));
        assert_eq!(
            task.status_message.as_ref().and_then(|m| m.parts.first()).and_then(|p| p.text.as_deref()),
            Some("리뷰 부탁")
        );
    }

    #[test]
    fn get_task_text_missing_task_says_not_found() {
        let store = SqliteStore::open_memory().unwrap();
        let text = get_task_text(&store, "nope").unwrap();
        assert!(text.contains("없음"), "미존재 안내 불일치: {text}");
        assert!(text.contains("nope"), "task_id 언급 없음: {text}");
    }

    // --- 레지스트리 라우팅: 순수 함수 단위테스트는 store::agents로 이동(Plan v2-34 T3) ---

    #[test]
    fn format_agents_empty_says_none_online() {
        assert_eq!(format_agents(&[]), "online 에이전트 없음");
    }

    #[test]
    fn format_agents_formats_uuid_name_tags_heartbeat() {
        let mut tags = BTreeMap::new();
        tags.insert("machine".to_string(), "win".to_string());
        let agents = vec![AgentEntry {
            uuid: "uuid-1".to_string(),
            tags,
            display_name: Some("win-claude".to_string()),
            last_heartbeat: "2026-07-04 10:00:00".to_string(),
        }];
        let text = format_agents(&agents);
        assert!(text.contains("uuid-1"));
        assert!(text.contains("win-claude"));
        assert!(text.contains("machine=win"));
        assert!(text.contains("2026-07-04 10:00:00"));
    }

    #[test]
    fn format_candidates_marks_armed_overlay() {
        let cands = vec![
            CandidateEntry {
                uuid: "s1".to_string(),
                runner: "claude".to_string(),
                project: Some("tunaround".to_string()),
                machine: Some("win".to_string()),
                source: "claude-jsonl".to_string(),
                age_secs: 5,
                reported_at: "2026-07-06 10:00:00".to_string(),
            },
            CandidateEntry {
                uuid: "s2".to_string(),
                runner: "codex".to_string(),
                project: None,
                machine: Some("mac".to_string()),
                source: "claude-jsonl".to_string(),
                age_secs: 9,
                reported_at: "2026-07-06 10:00:00".to_string(),
            },
        ];
        // s1은 online roster 소속(armed), s2는 미무장(candidate).
        let armed: std::collections::HashSet<String> = ["s1".to_string()].into_iter().collect();
        let text = format_candidates(&cands, &armed);
        assert!(text.contains("[s1]"));
        assert!(text.contains("(armed)"));
        assert!(text.contains("[s2]"));
        assert!(text.contains("(candidate)"));
        assert!(text.contains("project=tunaround"));
        assert!(text.contains("project=-")); // s2는 project 불명
        assert!(text.contains("machine=win"));
        assert!(text.contains("machine=mac"));
    }

    #[test]
    fn format_candidates_empty_says_none() {
        let armed = std::collections::HashSet::new();
        assert_eq!(format_candidates(&[], &armed), "발견된 세션 없음");
    }

    #[test]
    fn send_task_routed_selector_zero_matches_is_no_consumer_and_no_task_created() {
        let store = SqliteStore::open_memory().unwrap();
        let text = send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None)
            .unwrap();
        assert!(text.contains("no-consumer") || text.contains("대상 없음"), "안내 불일치: {text}");
        assert!(store.list_all_open_tasks().unwrap().is_empty(), "task가 생성되면 안 됨");
    }

    #[test]
    fn send_task_routed_selector_one_match_creates_task_to_that_uuid() {
        let store = SqliteStore::open_memory().unwrap();
        let now = store.now().unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("runner".to_string(), "claude".to_string());
        store.register_agent("uuid-only", tags, None, &now);

        let text =
            send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None).unwrap();
        assert!(text.contains("state=submitted"), "응답 불일치: {text}");

        let tasks = store.list_open_tasks_for("uuid-only").unwrap();
        assert_eq!(tasks.len(), 1, "uuid-only 앞으로 정확히 하나 생성돼야 함");
    }

    #[test]
    fn send_task_routed_selector_multiple_matches_lists_candidates_and_no_task_created() {
        let store = SqliteStore::open_memory().unwrap();
        let now = store.now().unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("runner".to_string(), "claude".to_string());
        store.register_agent("uuid-a", tags.clone(), None, &now);
        store.register_agent("uuid-b", tags, None, &now);

        let text =
            send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None).unwrap();
        assert!(text.contains("uuid-a"), "후보 목록 불일치: {text}");
        assert!(text.contains("uuid-b"), "후보 목록 불일치: {text}");
        assert!(store.list_all_open_tasks().unwrap().is_empty(), "task가 생성되면 안 됨");
    }

    #[test]
    fn get_task_text_open_task_shows_state_without_artifacts() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = get_task_text(&store, "t1").unwrap();
        assert!(text.contains("state=submitted"), "state 누락: {text}");
    }

    #[test]
    fn get_task_text_completed_task_appends_artifact_text() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        claim_task_text(&store, "t1", None, None).unwrap();
        complete_task_text(&store, "t1", "작업 결과 요약", None).unwrap();
        let text = get_task_text(&store, "t1").unwrap();
        assert!(text.contains("state=completed"), "state 누락: {text}");
        assert!(text.contains("작업 결과 요약"), "artifact 텍스트 누락: {text}");
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

    // 대시보드 전역 SSE 순수 스트림: Status/Completed 이벤트를 필터 없이 순서대로 JSON으로 내보내는지 검증한다.
    #[cfg(feature = "serve")]
    #[tokio::test]
    async fn dashboard_event_json_stream_emits_status_then_completed() {
        use crate::store::a2a::{Task, TaskEvent};
        use futures_util::StreamExt;

        let (tx, rx) = tokio::sync::broadcast::channel::<TaskEvent>(16);
        let stream = dashboard_event_json_stream(rx);
        futures_util::pin_mut!(stream);

        let task_a = Task::new("task-a", None, "win-claude", "mac-claude", "2026-07-06 10:00:00");
        let mut task_b = Task::new("task-b", None, "win-claude", "mac-codex", "2026-07-06 10:01:00");
        task_b.state = TaskState::Completed;
        tx.send(TaskEvent::Status(task_a.clone())).unwrap();
        tx.send(TaskEvent::Completed(task_b.clone())).unwrap();

        let f1: serde_json::Value =
            serde_json::from_str(&stream.next().await.expect("frame1 있어야 함")).unwrap();
        assert_eq!(f1["event"], "status");
        assert_eq!(f1["task"]["id"], "task-a");

        let f2: serde_json::Value =
            serde_json::from_str(&stream.next().await.expect("frame2 있어야 함")).unwrap();
        assert_eq!(f2["event"], "completed");
        assert_eq!(f2["task"]["id"], "task-b");
    }
}
