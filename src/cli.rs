// tunaround 바이너리의 CLI 정의(clap): 서브커맨드·인자 구조체·파싱 테스트 (main.rs에서 분할, T4.5).

use clap::{Args, Parser, Subcommand};

// env 키 상수: 문자열 리터럴 직접 조회는 오타가 컴파일 타임에 안 잡힌다(DeepSource RS-E1011).
#[cfg_attr(not(feature = "worker"), allow(dead_code))] // worker 데몬 경로에서만 사용
pub const ENV_BROKER_CORE: &str = "TUNA_BROKER_CORE";
#[cfg_attr(not(any(feature = "serve", feature = "worker")), allow(dead_code))]
pub const ENV_BROKER_TOKEN: &str = "TUNA_BROKER_TOKEN";
#[cfg_attr(not(feature = "worker"), allow(dead_code))]
pub const ENV_USERPROFILE: &str = "USERPROFILE";
#[cfg_attr(not(feature = "worker"), allow(dead_code))]
pub const ENV_HOME: &str = "HOME";

/// tunaRound CLI. 서브커맨드 없이 실행하면 기본 REPL(chat)로 동작한다(하위호환: 인자 없는 `tunaround` = 지금처럼 REPL).
#[derive(Parser)]
#[command(name = "tunaround", version, about = "tunaRound - 2-에이전트 설계 토론 REPL")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

/// 서브커맨드 목록. serve/core/mcp-search/reindex는 해당 피처가 꺼지면 clap enum에서 아예 빠진다
/// (= 미지원 서브커맨드가 됨. 기존 flag soup의 "피처 없으면 조용히 무시"와 동등한 graceful degrade).
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// 기본 REPL(사람이 운전하는 2-에이전트 토론).
    Chat(ChatArgs),
    /// front=core 단일 프로세스: REPL + in-process HTTP MCP 코어(serve 피처 전용).
    #[cfg(feature = "serve")]
    Core(CoreArgs),
    /// HTTP MCP 서버 상주(헤드리스, REPL 없음. serve 피처 전용).
    #[cfg(feature = "serve")]
    Serve(ServeArgs),
    /// 원격 코어 접속 프리셋(= chat + --search-url/--pull-context 기본 on).
    Join(JoinArgs),
    /// stdio MCP 검색 서버(내부용, 러너가 self-exe로 spawn. mcp 피처 전용).
    #[cfg(feature = "mcp")]
    McpSearch(McpSearchArgs),
    /// 모든 세션의 FTS·벡터 인덱스를 SoR(messages)에서 재생성(sqlite 피처 전용).
    #[cfg(feature = "sqlite")]
    Reindex(ReindexArgs),
    /// 헤드리스 자율 워커 데몬: 원격 코어를 auto-poll->claim->실행->complete(worker 피처 전용).
    #[cfg(feature = "worker")]
    Work(WorkArgs),
    /// 감시 전용: agent 앞 새 task를 stdout으로 알림만(claim/실행 없음). Monitor로 감싸 감독 레인 wake용(worker 피처).
    #[cfg(feature = "worker")]
    Poll(PollArgs),
    /// codex app-server 라이브 thread에 turn/start로 유저 턴 1건을 ws로 주입(worker 피처).
    #[cfg(feature = "worker")]
    CodexInject(CodexInjectArgs),
    /// 발견 리포터: 로컬 Claude Code 세션을 열거해 브로커에 미무장 후보로 보고한다(v2-40 S2, worker 피처).
    #[cfg(feature = "worker")]
    Discover(DiscoverArgs),
    /// 총괄 결과 인박스: 내가 던진 task의 완료/실패를 브로커 SSE로 받아 알린다(책임의 이전, worker 피처).
    #[cfg(feature = "worker")]
    WatchResults(WatchResultsArgs),
    /// 머신당 presence 스캐너 데몬: 라이브 세션(claude·codex)을 스캔해 브로커 로스터에 일괄 동기화(v2-44, worker 피처).
    #[cfg(feature = "worker")]
    PresenceScan(PresenceScanArgs),
    /// A2A task 수동 조작 CLI: poll/claim/get/complete/fail(MCP 미로드 세션의 0토큰 경로, worker 피처).
    #[cfg(feature = "worker")]
    Task(TaskArgs),
    /// 머신당 codex 배달 데몬: 로컬 codex 세션들 앞 task를 대리 claim해 그 세션 thread로 주입(v2-46, worker 피처).
    #[cfg(feature = "worker")]
    CodexRelay(CodexRelayArgs),
    /// 워커 노드 상주: node.toml대로 브로커(self)+자동 워커 레인들을 한 프로세스로(serve+worker 피처).
    #[cfg(all(feature = "serve", feature = "worker"))]
    Node(NodeArgs),
    /// node 설정 진단: config/코어 도달/토큰/러너 바이너리/project 경로 체크(serve+worker 피처).
    #[cfg(all(feature = "serve", feature = "worker"))]
    Doctor(DoctorArgs),
    /// 온보딩: node.toml 생성(러너 자동 탐지 + 다음 단계 안내, serve+worker 피처).
    #[cfg(all(feature = "serve", feature = "worker"))]
    Init(InitArgs),
}

/// chat/core가 공유하는 세션 배선 옵션.
#[derive(Args, Default, Debug)]
pub struct CommonSessionArgs {
    /// SQLite DB 경로(검색·영속 인덱서). sqlite 피처 없으면 무시된다.
    #[arg(long)]
    pub db: Option<String>,
    /// 동적 좌석 로스터 JSON 경로(없으면 기본 2자리: claude proposer + codex reviewer).
    #[arg(long)]
    pub roster: Option<String>,
    /// 프롬프트에 재주입할 최근 턴 수 캡(기본: 캡 없음, 통째 재주입).
    #[arg(long = "recent-turns")]
    pub recent_turns: Option<usize>,
    /// Pull 컨텍스트 모드(포인터 프롬프트 + 에이전트가 MCP로 전사를 당김). --db 없으면 무의미(경고 후 Push 유지).
    #[arg(long = "pull-context")]
    pub pull_context: bool,
    /// Redis snapshot에서 세션을 재개(id 지정).
    #[arg(long)]
    pub session: Option<String>,
    /// 원격 HTTP MCP 서버 URL(stdio spawn 대신 접속).
    #[arg(long = "search-url")]
    pub search_url: Option<String>,
    /// 원격 HTTP MCP 서버 bearer 토큰(Authorization 헤더).
    #[arg(long = "search-token")]
    pub search_token: Option<String>,
    /// 설정 파일 경로 명시(지정 시 탐색 없이 이 파일만 사용). 기본 탐색: ./tunaround.toml -> ~/.config/tunaround/config.toml.
    #[arg(long)]
    pub config: Option<String>,
    /// tunaround.toml의 프로파일 이름(미지정 시 default_profile 또는 자동/대화형 선택).
    #[arg(long)]
    pub profile: Option<String>,
}

/// `chat` 서브커맨드(기본 REPL) 옵션.
#[derive(Args, Default, Debug)]
pub struct ChatArgs {
    /// 세션 상태 파일 경로(있으면 이어받고, 종료 시 저장).
    pub state_file: Option<String>,
    /// 관찰 모드: REPL 대신 세션 id를 라이브 구독(read-only).
    #[arg(long)]
    pub observe: Option<String>,
    #[command(flatten)]
    pub common: CommonSessionArgs,
}

/// `core <addr>` 서브커맨드(serve 피처 전용) 옵션.
#[cfg(feature = "serve")]
#[derive(Args, Debug)]
pub struct CoreArgs {
    /// in-process HTTP MCP 코어가 바인드할 주소(예: 127.0.0.1:8770).
    pub addr: String,
    /// 세션 상태 파일 경로(있으면 이어받고, 종료 시 저장).
    pub state_file: Option<String>,
    /// bearer 토큰 인증(HTTP MCP 코어).
    #[arg(long)]
    pub token: Option<String>,
    #[command(flatten)]
    pub common: CommonSessionArgs,
}

/// `serve <addr>` 서브커맨드(serve 피처 전용) 옵션.
#[cfg(feature = "serve")]
#[derive(Args, Debug)]
pub struct ServeArgs {
    /// HTTP MCP 서버가 바인드할 주소.
    pub addr: String,
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    pub db: Option<String>,
    /// bearer 토큰 인증.
    #[arg(long)]
    pub token: Option<String>,
}

/// `join <url>` 서브커맨드 옵션(= chat + 원격 코어 프리셋).
#[derive(Args, Debug)]
pub struct JoinArgs {
    /// 원격 HTTP MCP 코어 URL.
    pub url: String,
    /// 세션 상태 파일 경로.
    pub state_file: Option<String>,
    /// bearer 토큰(내부적으로 search-token으로 배선).
    #[arg(long)]
    pub token: Option<String>,
    /// SQLite DB 경로(로컬 인덱서, 선택).
    #[arg(long)]
    pub db: Option<String>,
    /// 동적 좌석 로스터 JSON 경로.
    #[arg(long)]
    pub roster: Option<String>,
    /// 설정 파일 경로 명시(지정 시 탐색 없이 이 파일만 사용). 기본 탐색: ./tunaround.toml -> ~/.config/tunaround/config.toml.
    #[arg(long)]
    pub config: Option<String>,
    /// tunaround.toml의 프로파일 이름(미지정 시 default_profile 또는 자동/대화형 선택).
    #[arg(long)]
    pub profile: Option<String>,
}

/// `mcp-search` 서브커맨드(mcp 피처 전용, 러너가 self-exe로 spawn하는 내부 모드) 옵션.
#[cfg(feature = "mcp")]
#[derive(Args, Debug)]
pub struct McpSearchArgs {
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    pub db: Option<String>,
    /// 전사 조회 기본 세션 id(없으면 "default").
    #[arg(long = "session-id")]
    pub session_id: Option<String>,
}

/// `reindex` 서브커맨드(sqlite 피처 전용) 옵션.
#[cfg(feature = "sqlite")]
#[derive(Args, Debug)]
pub struct ReindexArgs {
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    pub db: Option<String>,
}

/// `work` 서브커맨드(worker 피처 전용) 옵션: 원격 코어를 auto-poll->claim->실행->complete하는 헤드리스 데몬.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct WorkArgs {
    /// 코어 `/mcp` 절대 URL(예: http://192.0.2.10:8770/mcp, `/mcp` 경로까지 포함해서 지정).
    #[arg(long)]
    pub core: String,
    /// bearer 토큰(코어가 --token으로 띄워졌다면 필요).
    #[arg(long)]
    pub token: Option<String>,
    /// 이 워커의 to_agent id(예: win-worker). poll_tasks가 이 agent 앞 task만 본다.
    /// 미지정 시 자가 uuid 생성(generate_agent_uuid).
    #[arg(long)]
    pub agent: Option<String>,
    /// 로스터 발견용 태그 "k=v,k=v"(예: "machine=win,runner=claude,role=worker"). dispatcher가
    /// to_selector로 이 워커를 발견한다. 생략 가능.
    #[arg(long)]
    pub tags: Option<String>,
    /// task를 실행할 러너 종류(기본 claude).
    #[arg(long, value_enum, default_value_t = WorkRunner::Claude)]
    pub runner: WorkRunner,
    /// 러너에 넘길 모델 이름(옵션, 러너별 기본값 사용 가능).
    #[arg(long)]
    pub model: Option<String>,
    /// 러너가 작업할 로컬 레포 경로(옵션). task의 context_id가 --context-map에 없을 때의 기본값.
    #[arg(long = "project-path")]
    pub project_path: Option<String>,
    /// context_id -> project-path 매핑(프로젝트별 라우팅). 형식: "projA=/repos/A,projB=/repos/B".
    /// 데몬 하나가 여러 프로젝트를 배분한다(매핑에 없으면 --project-path로 폴백).
    #[arg(long = "context-map")]
    pub context_map: Option<String>,
    /// --runner http 전용: OpenAI 호환 chat API의 base URL(예: http://localhost:11434).
    #[arg(long = "http-base-url")]
    pub http_base_url: Option<String>,
    /// --runner a2a 전용: 외부 표준 A2A 에이전트 카드 발견 URL(예: http://some-agent.example/).
    #[arg(long = "a2a-card")]
    pub a2a_card: Option<String>,
    /// --runner a2a 전용: 그 외부 에이전트 인증 토큰(코어 --token과 별개).
    #[arg(long = "a2a-token")]
    pub a2a_token: Option<String>,
    /// poll 간격(초, 기본 15).
    #[arg(long, default_value_t = 15)]
    pub interval: u64,
    /// 한 패스만 실행하고 종료(테스트·수동 실행용).
    #[arg(long)]
    pub once: bool,
    /// Write 모드로 실행(기본 ReadOnly=behavioral read-only 유지).
    #[arg(long)]
    pub write: bool,
}

/// poll 서브커맨드: 감시 전용(claim/실행 없음). Claude Code 세션이 Monitor로 감싸 감독 레인을 유휴 0토큰으로 운용.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct PollArgs {
    /// 코어 `/mcp` 절대 URL(예: http://192.0.2.10:8770/mcp).
    #[arg(long)]
    pub core: String,
    /// bearer 토큰(코어가 --token으로 띄워졌다면 필요).
    #[arg(long)]
    pub token: Option<String>,
    /// 감시할 to_agent id(이 agent 앞 새 submitted task만 알린다).
    #[arg(long)]
    pub agent: String,
    /// 로스터 발견용 태그 "k=v,k=v"(예: "machine=win,runner=codex,role=supervised,project=tunaround").
    /// dispatcher가 to_selector로 이 감독을 발견한다. 생략 가능.
    #[arg(long)]
    pub tags: Option<String>,
    /// 로스터 가독용 표시 이름(생략 가능). uuid(=--agent)는 라우팅·발견 overlay 키라 세션 id를 쓰고,
    /// 사람이 읽는 이름은 이걸로 분리한다(예: --agent <session-id> --display-name win-opus-boss).
    #[arg(long)]
    pub display_name: Option<String>,
    /// poll 간격(초, 기본 15).
    #[arg(long, default_value_t = 15)]
    pub interval: u64,
    /// 한 패스만 실행하고 종료(테스트·수동 실행용).
    #[arg(long)]
    pub once: bool,
    /// 수신 전용 모드: 로스터 등록·heartbeat를 하지 않는다(v2-44: presence=머신 스캐너 소관.
    /// 세션 수신 poll이 태그 없이 재등록해 스캐너 항목을 덮는 '기타' 유령·깜빡임 방지).
    #[arg(long)]
    pub no_register: bool,
    /// task 도착 시 실행할 명령(선택). `{id}`가 task id로 치환되고 TUNAROUND_TASK_ID/TUNAROUND_TASK_MSG
    /// 환경변수도 설정된다. Monitor가 없는 하네스(codex 등)의 0토큰 wake 글루.
    /// 예: --on-task 'codex exec resume --last "브로커 task {id}를 claim해서 처리하고 complete로 보고"'.
    #[arg(long)]
    pub on_task: Option<String>,
}

/// `discover` 서브커맨드(worker 피처 전용) 옵션: 로컬 Claude Code 세션을 열거해 브로커에 미무장
/// 후보로 보고한다(v2-40 S2). 무장(S1) 안 한 세션도 대시보드 "발견된 세션" 패널에 뜨게 한다.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct DiscoverArgs {
    /// 코어 `/mcp` 절대 URL(예: http://127.0.0.1:8770/mcp).
    #[arg(long)]
    pub core: String,
    /// bearer 토큰(코어가 --token으로 띄워졌다면 필요). report는 write 경계라 토큰이 필요하다.
    #[arg(long)]
    pub token: Option<String>,
    /// 스캔할 projects 디렉토리(생략 시 ~/.claude/projects).
    #[arg(long)]
    pub projects_dir: Option<String>,
    /// 세션을 후보로 리포트할 "잊기 지평"(분). jsonl mtime이 이 시간 이내면 리포트한다. 활성/유휴 분리는
    /// 대시보드가 age로 하므로(설계 v2-41, 활성<60분/유휴>=60분), 이 값은 유휴 창을 덮게 커야 한다(기본 240분=4시간).
    #[arg(long, default_value_t = 240)]
    pub stale_mins: u64,
    /// 이 리포터의 머신 식별자(win|mac|unix 등). 생략 시 TUNA_MACHINE env 또는 빌드 타깃 OS로 추정.
    #[arg(long)]
    pub machine: Option<String>,
    /// 보고 간격(초, 기본 30).
    #[arg(long, default_value_t = 30)]
    pub interval: u64,
    /// 한 번만 열거·보고하고 종료(테스트·수동 실행용).
    #[arg(long)]
    pub once: bool,
}

/// `watch-results` 서브커맨드(worker 피처 전용): 총괄이 던진 task의 완료/실패를 브로커 SSE로 받아
/// stdout으로 알린다(책임의 이전 = 결과 push). 총괄 세션이 Monitor로 감싸면 던지고 자리 떠도 결과가 깨운다.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct WatchResultsArgs {
    /// 코어 베이스 URL(예: http://127.0.0.1:8770). `/dashboard/events` SSE를 무인증으로 구독한다.
    #[arg(long)]
    pub core: String,
    /// 관측할 dispatcher id(이 값이 fromAgent인 task의 완료/실패만 알림). 대시보드 goal은 `dashboard`.
    /// 생략/빈 값이면 모든 완료를 관측한다.
    #[arg(long, default_value = "dashboard")]
    pub dispatcher: String,
    /// completed 묶음 구간(초, 기본 0=즉시). >0이면 completed는 구간 내 묶어 한 번에 알리고
    /// failed는 즉시 알린다(총괄 wake 절약, v2-44 W5).
    #[arg(long, default_value_t = 0)]
    pub digest: u64,
}

/// `presence-scan` 서브커맨드(worker 피처 전용): 머신당 1개 상주하며 로컬 라이브 세션 전집합을
/// 브로커에 일괄 동기화한다(설계 v2-44 §3). poll·훅·래퍼 비의존 = 유령·소멸 원천 차단.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct PresenceScanArgs {
    /// 코어 `/mcp` 절대 URL(예: http://127.0.0.1:8770/mcp). 생략 시 TUNA_BROKER_CORE env.
    #[arg(long)]
    pub core: Option<String>,
    /// bearer 토큰(생략 시 TUNA_BROKER_TOKEN env 폴백. argv 노출 회피 권장).
    #[arg(long)]
    pub token: Option<String>,
    /// 이 스캐너의 머신 식별자(win|mac|unix). 생략 시 TUNA_MACHINE env 또는 빌드 타깃 OS.
    #[arg(long)]
    pub machine: Option<String>,
    /// 스캔할 Claude projects 디렉토리(생략 시 ~/.claude/projects).
    #[arg(long)]
    pub projects_dir: Option<String>,
    /// 스캔할 codex sessions 디렉토리(생략 시 ~/.codex/sessions).
    #[arg(long)]
    pub codex_dir: Option<String>,
    /// 라이브로 간주할 활동 신선도 창(분, 기본 240). 개별 크래시 유령의 상한이기도 하다.
    #[arg(long, default_value_t = 240)]
    pub stale_mins: u64,
    /// 스캔·보고 간격(초, 기본 15 = heartbeat 간격과 동일).
    #[arg(long, default_value_t = 15)]
    pub interval: u64,
    /// 한 번만 스캔·보고하고 종료(테스트·수동 실행용).
    #[arg(long)]
    pub once: bool,
}

/// `codex-relay` 서브커맨드(worker 피처 전용): 머신당 1개 상주하는 codex 배달 데몬(설계 v2-46).
/// 로컬 라이브 codex 세션들(uuid=threadId) 앞 task를 대리 claim해 그 세션 thread로 in-process 주입한다.
/// sup 정체성·글루 thread·.cmd 핸들러의 대체.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct CodexRelayArgs {
    /// 코어 `/mcp` 절대 URL(예: http://127.0.0.1:8770/mcp). 생략 시 TUNA_BROKER_CORE env.
    #[arg(long)]
    pub core: Option<String>,
    /// bearer 토큰(생략 시 TUNA_BROKER_TOKEN env 폴백. argv 노출 회피 권장).
    #[arg(long)]
    pub token: Option<String>,
    /// codex app-server ws URL(기본 ws://127.0.0.1:8790, 로컬 무인증).
    #[arg(long, default_value = "ws://127.0.0.1:8790")]
    pub ws: String,
    /// 이 relay의 머신 식별자(win|mac|unix). 생략 시 TUNA_MACHINE env 또는 빌드 타깃 OS.
    #[arg(long)]
    pub machine: Option<String>,
    /// 스캔할 codex sessions 디렉토리(생략 시 ~/.codex/sessions).
    #[arg(long)]
    pub codex_dir: Option<String>,
    /// 라이브로 간주할 활동 신선도 창(분, 기본 240. presence 스캐너와 동일 규약).
    #[arg(long, default_value_t = 240)]
    pub stale_mins: u64,
    /// 폴 간격(초, 기본 15).
    #[arg(long, default_value_t = 15)]
    pub interval: u64,
    /// 주입 1건의 turn/completed 대기 타임아웃(초, 기본 1800 = 워커 on-task 상한과 동일).
    #[arg(long, default_value_t = 1800)]
    pub inject_timeout: u64,
    /// 한 패스만 실행하고 종료(테스트·수동 실행용).
    #[arg(long)]
    pub once: bool,
}

/// `task` 서브커맨드(worker 피처 전용): A2A task를 CLI로 조작한다(v2-44 W3). tuna-broker MCP가
/// 안 붙은 세션(브로커 사후 기동 등)이 raw curl 대신 쓰는 0토큰 전송 경로.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct TaskArgs {
    /// 코어 `/mcp` 절대 URL(예: http://127.0.0.1:8770/mcp). 생략 시 TUNA_BROKER_CORE env.
    #[arg(long, global = true)]
    pub core: Option<String>,
    /// bearer 토큰(생략 시 TUNA_BROKER_TOKEN env 폴백).
    #[arg(long, global = true)]
    pub token: Option<String>,
    #[command(subcommand)]
    pub action: TaskAction,
}

/// task CLI 동작: 수신 워크플로우(poll→claim→get→complete|fail)의 각 단계.
#[cfg(feature = "worker")]
#[derive(Subcommand, Debug)]
pub enum TaskAction {
    /// 이 agent 앞에 대기 중인 task 목록을 본다.
    Poll {
        /// 확인할 agent id(보통 세션 id).
        agent: String,
    },
    /// task를 선점한다(작업 착수 선언).
    Claim {
        /// 선점할 task id.
        task_id: String,
        /// 선점하는 agent id.
        agent: String,
    },
    /// task의 상태·본문·결과를 본다.
    Get {
        /// 조회할 task id.
        task_id: String,
    },
    /// task를 완료 보고한다.
    Complete {
        /// 완료할 task id.
        task_id: String,
        /// 결과 텍스트. `-`면 stdin에서 읽는다(긴 결과의 argv 한도 회피).
        result: String,
        /// 보고하는 agent id(claim한 agent와 동일해야 함).
        #[arg(long)]
        agent: Option<String>,
    },
    /// task를 실패 보고한다(처리 불가 사유 명시).
    Fail {
        /// 실패 처리할 task id.
        task_id: String,
        /// 실패 사유. `-`면 stdin에서 읽는다.
        reason: String,
        /// 보고하는 agent id.
        #[arg(long)]
        agent: Option<String>,
    },
}

/// `codex-inject` 서브커맨드(worker 피처 전용) 옵션: codex app-server 라이브 thread에 turn/start로
/// 유저 턴 1건을 주입한다(설계 §6 CLI 계약). 한 번 실행 = task 1건 처리(글루가 매 task마다 이 커맨드를 fork).
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
pub struct CodexInjectArgs {
    /// codex app-server ws URL(예: ws://127.0.0.1:8790, 로컬 무인증).
    #[arg(long)]
    pub ws: String,
    /// thread 영속 키. `~/.tunaround/codex-sup-<agent>.thread`에 threadId를 기록/재사용해 맥락을 누적한다.
    /// --thread와 배타적(둘 중 하나 필수).
    #[arg(long, conflicts_with = "thread", required_unless_present = "thread")]
    pub agent: Option<String>,
    /// threadId 직지정(v2-46): 영속 파일 없이 이 thread를 resume해 주입한다. 실패 시 새 thread
    /// 자가치유 없이 즉시 실패(로스터에 보이는 세션 thread에만 답이 생기게). --new와 배타
    /// (직지정 모드는 영속 파일을 안 쓰므로 --new가 무의미).
    #[arg(long, conflicts_with = "new")]
    pub thread: Option<String>,
    /// 주입할 유저 턴 텍스트(브로커 task 처리 지시 + task 메시지).
    #[arg(long)]
    pub text: String,
    /// 승인 정책(기본 never): untrusted/on-failure/on-request/never.
    #[arg(long, default_value = "never")]
    pub approval: String,
    /// 샌드박스 모드(기본 workspace-write): read-only/workspace-write/danger-full-access.
    #[arg(long, default_value = "workspace-write")]
    pub sandbox: String,
    /// turn/completed 대기 타임아웃(초, 기본 300).
    #[arg(long, default_value_t = 300)]
    pub timeout: u64,
    /// 영속 threadId를 무시하고 새 thread를 만든다.
    #[arg(long)]
    pub new: bool,
}

/// `--runner` 선택지: 기존 Runner trait 구현체 중 어느 것으로 task를 실행할지.
#[cfg(feature = "worker")]
#[derive(clap::ValueEnum, Clone, Debug)]
pub enum WorkRunner {
    Claude,
    Codex,
    Opencode,
    Http,
    A2a,
}

/// node 서브커맨드 인자. 나머지 설정(코어·토큰·레인)은 node.toml에서 읽는다.
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Args, Debug)]
pub struct NodeArgs {
    /// node 설정 파일 경로(생략 시 ./tunaround.node.toml, ~/.tunaround/node.toml 순 탐색).
    #[arg(long)]
    pub config: Option<String>,
}


/// doctor 서브커맨드 인자.
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// node 설정 파일 경로(생략 시 자동 탐색).
    #[arg(long)]
    pub config: Option<String>,
}

/// init 서브커맨드 인자. 플래그 주도로 node.toml을 생성한다(대화형 위저드는 후속).
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Args, Debug)]
pub struct InitArgs {
    /// "self"(이 머신이 브로커) 또는 코어 /mcp URL(기본 self).
    #[arg(long)]
    pub core: Option<String>,
    /// core=self일 때 브로커 바인드 주소(기본 0.0.0.0:8770).
    #[arg(long)]
    pub listen: Option<String>,
    /// 이 워커의 agent id(기본 "worker").
    #[arg(long)]
    pub agent: Option<String>,
    /// 자동 레인 러너(기본: 탐지된 것, 없으면 claude).
    #[arg(long)]
    pub runner: Option<String>,
    /// 러너 작업 디렉터리(기본: 현재 디렉터리).
    #[arg(long)]
    pub project: Option<String>,
    /// 토큰을 읽을 환경변수 이름(기본 TUNAROUND_TOKEN).
    #[arg(long = "token-env")]
    pub token_env: Option<String>,
    /// 출력 경로(기본 ~/.tunaround/node.toml).
    #[arg(long)]
    pub out: Option<String>,
    /// 기존 파일 덮어쓰기.
    #[arg(long)]
    pub force: bool,
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    #[test]
    fn no_args_means_no_subcommand_defaults_to_chat_in_main() {
        // clap 자체는 command: None만 준다. main()의 unwrap_or_else가 Chat 기본값으로 채운다(하위호환).
        let cli = Cli::try_parse_from(["tunaround"]).expect("파싱 성공");
        assert!(cli.command.is_none());
    }

    #[test]
    fn chat_parses_positional_and_common_options() {
        let cli = Cli::try_parse_from([
            "tunaround",
            "chat",
            "state.json",
            "--db",
            "x.db",
            "--roster",
            "r.json",
            "--recent-turns",
            "3",
            "--pull-context",
            "--session",
            "sid1",
            "--search-url",
            "http://127.0.0.1:8770/mcp",
            "--search-token",
            "tok",
        ])
        .expect("파싱 성공");
        match cli.command {
            Some(Commands::Chat(a)) => {
                assert_eq!(a.state_file.as_deref(), Some("state.json"));
                assert_eq!(a.common.db.as_deref(), Some("x.db"));
                assert_eq!(a.common.roster.as_deref(), Some("r.json"));
                assert_eq!(a.common.recent_turns, Some(3));
                assert!(a.common.pull_context);
                assert_eq!(a.common.session.as_deref(), Some("sid1"));
                assert_eq!(a.common.search_url.as_deref(), Some("http://127.0.0.1:8770/mcp"));
                assert_eq!(a.common.search_token.as_deref(), Some("tok"));
            }
            other => panic!("Chat 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[test]
    fn chat_observe_option_parses() {
        let cli = Cli::try_parse_from(["tunaround", "chat", "--observe", "sess-9"]).expect("파싱 성공");
        match cli.command {
            Some(Commands::Chat(a)) => assert_eq!(a.observe.as_deref(), Some("sess-9")),
            other => panic!("Chat 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[test]
    fn bare_positional_without_subcommand_is_now_an_error() {
        // 설계 변경점: 기존엔 `tunaround state.json`이 통했으나, 서브커맨드 도입 후엔
        // `tunaround chat state.json`으로 명시해야 한다(인자 0개=chat만 하위호환 보장).
        let res = Cli::try_parse_from(["tunaround", "state.json"]);
        assert!(res.is_err(), "서브커맨드 없는 bare positional은 이제 에러여야 함");
    }

    #[test]
    fn join_sets_url_and_optional_fields() {
        let cli = Cli::try_parse_from([
            "tunaround",
            "join",
            "http://127.0.0.1:8770/mcp",
            "--token",
            "tok2",
            "--db",
            "local.db",
            "--roster",
            "r.json",
            "state.json",
        ])
        .expect("파싱 성공");
        match cli.command {
            Some(Commands::Join(a)) => {
                assert_eq!(a.url, "http://127.0.0.1:8770/mcp");
                assert_eq!(a.token.as_deref(), Some("tok2"));
                assert_eq!(a.db.as_deref(), Some("local.db"));
                assert_eq!(a.roster.as_deref(), Some("r.json"));
                assert_eq!(a.state_file.as_deref(), Some("state.json"));
            }
            other => panic!("Join 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn serve_parses_addr_db_and_token() {
        let cli = Cli::try_parse_from(["tunaround", "serve", "127.0.0.1:8770", "--db", "x.db", "--token", "T"])
            .expect("파싱 성공");
        match cli.command {
            Some(Commands::Serve(a)) => {
                assert_eq!(a.addr, "127.0.0.1:8770");
                assert_eq!(a.db.as_deref(), Some("x.db"));
                assert_eq!(a.token.as_deref(), Some("T"));
            }
            other => panic!("Serve 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn core_parses_addr_and_common_options() {
        let cli = Cli::try_parse_from([
            "tunaround",
            "core",
            "127.0.0.1:8790",
            "--db",
            "core.db",
            "--token",
            "TOK",
            "--pull-context",
        ])
        .expect("파싱 성공");
        match cli.command {
            Some(Commands::Core(a)) => {
                assert_eq!(a.addr, "127.0.0.1:8790");
                assert_eq!(a.token.as_deref(), Some("TOK"));
                assert_eq!(a.common.db.as_deref(), Some("core.db"));
                assert!(a.common.pull_context);
            }
            other => panic!("Core 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "mcp")]
    #[test]
    fn mcp_search_parses_db_and_session_id() {
        let cli = Cli::try_parse_from(["tunaround", "mcp-search", "--db", "x.db", "--session-id", "sid-7"])
            .expect("파싱 성공");
        match cli.command {
            Some(Commands::McpSearch(a)) => {
                assert_eq!(a.db.as_deref(), Some("x.db"));
                assert_eq!(a.session_id.as_deref(), Some("sid-7"));
            }
            other => panic!("McpSearch 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "sqlite")]
    #[test]
    fn reindex_parses_db() {
        let cli = Cli::try_parse_from(["tunaround", "reindex", "--db", "x.db"]).expect("파싱 성공");
        match cli.command {
            Some(Commands::Reindex(a)) => assert_eq!(a.db.as_deref(), Some("x.db")),
            other => panic!("Reindex 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "worker")]
    #[test]
    fn poll_parses_tags_for_supervisor_roster_discovery() {
        // 감독(supervisor) poll watcher도 워커처럼 --tags로 발견 가능해야 한다(로스터 상시 online 유지 배선).
        let cli = Cli::try_parse_from([
            "tunaround",
            "poll",
            "--core",
            "http://127.0.0.1:8770/mcp",
            "--agent",
            "win-sup",
            "--tags",
            "machine=win,runner=codex,role=supervised,project=tunaround",
        ])
        .expect("파싱 성공");
        match cli.command {
            Some(Commands::Poll(a)) => {
                assert_eq!(a.agent, "win-sup");
                assert_eq!(
                    a.tags.as_deref(),
                    Some("machine=win,runner=codex,role=supervised,project=tunaround")
                );
            }
            other => panic!("Poll 서브커맨드 기대, 실제: {other:?}"),
        }
    }

    #[cfg(feature = "worker")]
    #[test]
    fn poll_tags_is_optional() {
        let cli = Cli::try_parse_from(["tunaround", "poll", "--core", "http://x/mcp", "--agent", "a"])
            .expect("파싱 성공");
        match cli.command {
            Some(Commands::Poll(a)) => assert_eq!(a.tags, None),
            other => panic!("Poll 서브커맨드 기대, 실제: {other:?}"),
        }
    }
}

