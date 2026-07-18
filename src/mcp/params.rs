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

/// extend_task_lease 툴 파라미터(워커가 실행 중 자기 task의 lease를 연장, v2-49 #6).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ExtendLeaseParams {
    /// lease를 연장할 task id.
    pub task_id: String,
    /// 연장하는 에이전트 id(claimed_by와 일치해야 성공, 소유자 확인용).
    pub agent: String,
}

/// cancel_task 툴 파라미터(잘못 보냈거나 더 필요 없는 열린 task를 취소, v2-49 #4).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct CancelTaskParams {
    /// 취소할 task id.
    pub task_id: String,
    /// 취소 사유(선택, 로그·표시용). 상태는 canceled로만 전이하고 사유는 별도 저장하지 않는다.
    pub reason: Option<String>,
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

/// start_discussion 좌석 파라미터(v2-56 mesh 토론).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct DiscussionSeatParams {
    /// 좌석 에이전트 uuid(로스터 online 필수, list_agents로 발견).
    pub agent: String,
    /// 전사·프롬프트 표기용 라벨(생략 시 로스터 display_name, 그것도 없으면 uuid 앞 8자).
    pub label: Option<String>,
    /// 토론 역할(proposer/reviewer/verifier/synthesizer 별칭 포함. 그 외 문자열은 역할 지시문 없이 무시).
    pub role: Option<String>,
    /// 좌석별 추가 지시(자유 텍스트, 역할 지시문 뒤에 덧붙는다).
    pub instruction: Option<String>,
    /// 라이브 세션 좌석 명시 동의(v2-56 §8-3). 로스터 role=session 에이전트는 true 필수
    /// (사람이 쓰는 세션을 실수로 토론에 끌어들이는 것 방지).
    pub live: Option<bool>,
}

/// start_discussion 툴 파라미터(v2-56 mesh 토론 시작).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StartDiscussionParams {
    /// 토론 주제(모든 라운드 프롬프트에 실린다).
    pub topic: String,
    /// 좌석 2~6석. 배열 순서 = 라운드 내 발언 순서(순차-인지: 뒤 좌석이 앞 좌석 답을 본다).
    pub seats: Vec<DiscussionSeatParams>,
    /// 라운드 수(기본 3, 1~10로 클램프). 소진 후 synthesizer 종합 1회가 추가로 돈다.
    pub rounds: Option<u32>,
    /// 라운드 간 사람 승인 게이트(이슈 #131, 옵트인·기본 false). true면 각 라운드 완료 시(종합 직전
    /// 포함) 다이제스트가 인박스로 배달되고 continue_discussion까지 대기한다. 승인 주체는 사람이다.
    pub gate: Option<bool>,
}

/// stop_discussion 툴 파라미터(이후 라운드 발행 중단).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct StopDiscussionParams {
    /// start_discussion이 반환한 토론 id.
    pub discussion_id: String,
}

/// continue_discussion 툴 파라미터(게이트 해제, 이슈 #131).
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ContinueDiscussionParams {
    /// start_discussion이 반환한 토론 id.
    pub discussion_id: String,
    /// 조향 지시(선택). 전사에 debate/user 턴("[사용자 조향 지시]" 프리픽스)으로 남고 다음 라운드
    /// 프롬프트에 주입된다.
    pub steer: Option<String>,
    /// true면 남은 라운드를 건너뛰고 synthesizer 종합으로 직행한다(사람의 "충분" 판단).
    pub conclude: Option<bool>,
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
    /// 스캐너가 관측한 마지막 사람 입력 시각(v2-45 P5, codex rollout user_message). DB datetime 포맷.
    /// 생략 가능(claude 세션·신호 없음). 브로커가 인메모리·영속과 max-merge한다.
    #[serde(default)]
    pub human_input_at: Option<String>,
    /// 스캐너가 관측한 마지막 활동 시각(이슈 #123, codex rollout mtime). DB datetime 포맷.
    /// 생략 가능(claude=turn-ping 훅 경로). 브로커가 인메모리 turn_active_at과 max-merge한다.
    #[serde(default)]
    pub active_at: Option<String>,
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
    /// terminal(completed/failed/canceled)까지 최대 이 시간(초) 서버에서 대기 후 반환한다
    /// (long-poll, v2-54 G7: 호출자가 폴링 간격을 관리하지 않아도 됨). 생략=즉시 반환(기존 동작).
    /// 1~120으로 클램프. 시간 내 terminal이 안 되면 그 시점 상태를 그대로 반환한다(에러 아님).
    pub wait_secs: Option<u64>,
}

/// tasks 툴 파라미터(필드 없음). 브로커 전역 열린 task 조망은 대상을 지정할 필요가 없다.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct ListTasksParams {}
