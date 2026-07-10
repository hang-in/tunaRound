// A2A/토론 MCP 툴의 요청 파라미터 구조체 모음(serde 역직렬화 + JSON 스키마).

use schemars::JsonSchema;
use serde::Deserialize;

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

/// report_presence 툴의 라이브 세션 한 건(presence 스캐너가 보고, 설계 v2-44 §6).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct PresenceSessionInput {
    /// 세션 id(claude=jsonl stem, codex=rollout uuid). roster uuid와 같은 공간.
    pub uuid: String,
    /// 러너 종류(claude | codex).
    pub runner: String,
    /// 추정 프로젝트(불명이면 생략).
    pub project: Option<String>,
    /// 로스터 가독용 표시 이름(예: win-claude-tunaRound). 생략 가능.
    pub display_name: Option<String>,
}

/// report_presence 툴 파라미터(머신당 스캐너 데몬이 라이브 세션 전집합을 일괄 보고).
/// 같은 machine의 스캐너 소유(src=scan) 항목 중 보고에 없는 것은 브로커가 제거한다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ReportPresenceParams {
    /// 이 스캐너의 머신 식별자(win|mac|unix).
    pub machine: String,
    /// 이 머신의 라이브 세션 전집합.
    pub sessions: Vec<PresenceSessionInput>,
}

/// get_task 툴 파라미터(dispatcher가 위임한 task의 상태·결과를 확인할 때 사용).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct GetTaskParams {
    /// 조회할 task id.
    pub task_id: String,
}

/// tasks 툴 파라미터(필드 없음). 브로커 전역 열린 task 조망은 대상을 지정할 필요가 없다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksParams {}
