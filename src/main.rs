// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use clap::{Args, Parser, Subcommand};
use tunaround::orchestrator::{ContextMode, MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

/// tunaRound CLI. 서브커맨드 없이 실행하면 기본 REPL(chat)로 동작한다(하위호환: 인자 없는 `tunaround` = 지금처럼 REPL).
#[derive(Parser)]
#[command(name = "tunaround", version, about = "tunaRound - 2-에이전트 설계 토론 REPL")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// 서브커맨드 목록. serve/core/mcp-search/reindex는 해당 피처가 꺼지면 clap enum에서 아예 빠진다
/// (= 미지원 서브커맨드가 됨. 기존 flag soup의 "피처 없으면 조용히 무시"와 동등한 graceful degrade).
#[derive(Subcommand, Debug)]
enum Commands {
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
}

/// chat/core가 공유하는 세션 배선 옵션.
#[derive(Args, Default, Debug)]
struct CommonSessionArgs {
    /// SQLite DB 경로(검색·영속 인덱서). sqlite 피처 없으면 무시된다.
    #[arg(long)]
    db: Option<String>,
    /// 동적 좌석 로스터 JSON 경로(없으면 기본 2자리: claude proposer + codex reviewer).
    #[arg(long)]
    roster: Option<String>,
    /// 프롬프트에 재주입할 최근 턴 수 캡(기본: 캡 없음, 통째 재주입).
    #[arg(long = "recent-turns")]
    recent_turns: Option<usize>,
    /// Pull 컨텍스트 모드(포인터 프롬프트 + 에이전트가 MCP로 전사를 당김). --db 없으면 무의미(경고 후 Push 유지).
    #[arg(long = "pull-context")]
    pull_context: bool,
    /// Redis snapshot에서 세션을 재개(id 지정).
    #[arg(long)]
    session: Option<String>,
    /// 원격 HTTP MCP 서버 URL(stdio spawn 대신 접속).
    #[arg(long = "search-url")]
    search_url: Option<String>,
    /// 원격 HTTP MCP 서버 bearer 토큰(Authorization 헤더).
    #[arg(long = "search-token")]
    search_token: Option<String>,
    /// 설정 파일 경로 명시(지정 시 탐색 없이 이 파일만 사용). 기본 탐색: ./tunaround.toml -> ~/.config/tunaround/config.toml.
    #[arg(long)]
    config: Option<String>,
    /// tunaround.toml의 프로파일 이름(미지정 시 default_profile 또는 자동/대화형 선택).
    #[arg(long)]
    profile: Option<String>,
}

/// `chat` 서브커맨드(기본 REPL) 옵션.
#[derive(Args, Default, Debug)]
struct ChatArgs {
    /// 세션 상태 파일 경로(있으면 이어받고, 종료 시 저장).
    state_file: Option<String>,
    /// 관찰 모드: REPL 대신 세션 id를 라이브 구독(read-only).
    #[arg(long)]
    observe: Option<String>,
    #[command(flatten)]
    common: CommonSessionArgs,
}

/// `core <addr>` 서브커맨드(serve 피처 전용) 옵션.
#[cfg(feature = "serve")]
#[derive(Args, Debug)]
struct CoreArgs {
    /// in-process HTTP MCP 코어가 바인드할 주소(예: 127.0.0.1:8770).
    addr: String,
    /// 세션 상태 파일 경로(있으면 이어받고, 종료 시 저장).
    state_file: Option<String>,
    /// bearer 토큰 인증(HTTP MCP 코어).
    #[arg(long)]
    token: Option<String>,
    #[command(flatten)]
    common: CommonSessionArgs,
}

/// `serve <addr>` 서브커맨드(serve 피처 전용) 옵션.
#[cfg(feature = "serve")]
#[derive(Args, Debug)]
struct ServeArgs {
    /// HTTP MCP 서버가 바인드할 주소.
    addr: String,
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    db: Option<String>,
    /// bearer 토큰 인증.
    #[arg(long)]
    token: Option<String>,
}

/// `join <url>` 서브커맨드 옵션(= chat + 원격 코어 프리셋).
#[derive(Args, Debug)]
struct JoinArgs {
    /// 원격 HTTP MCP 코어 URL.
    url: String,
    /// 세션 상태 파일 경로.
    state_file: Option<String>,
    /// bearer 토큰(내부적으로 search-token으로 배선).
    #[arg(long)]
    token: Option<String>,
    /// SQLite DB 경로(로컬 인덱서, 선택).
    #[arg(long)]
    db: Option<String>,
    /// 동적 좌석 로스터 JSON 경로.
    #[arg(long)]
    roster: Option<String>,
    /// 설정 파일 경로 명시(지정 시 탐색 없이 이 파일만 사용). 기본 탐색: ./tunaround.toml -> ~/.config/tunaround/config.toml.
    #[arg(long)]
    config: Option<String>,
    /// tunaround.toml의 프로파일 이름(미지정 시 default_profile 또는 자동/대화형 선택).
    #[arg(long)]
    profile: Option<String>,
}

/// `mcp-search` 서브커맨드(mcp 피처 전용, 러너가 self-exe로 spawn하는 내부 모드) 옵션.
#[cfg(feature = "mcp")]
#[derive(Args, Debug)]
struct McpSearchArgs {
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    db: Option<String>,
    /// 전사 조회 기본 세션 id(없으면 "default").
    #[arg(long = "session-id")]
    session_id: Option<String>,
}

/// `reindex` 서브커맨드(sqlite 피처 전용) 옵션.
#[cfg(feature = "sqlite")]
#[derive(Args, Debug)]
struct ReindexArgs {
    /// SQLite DB 경로(필수, 진입 시 검증).
    #[arg(long)]
    db: Option<String>,
}

/// `work` 서브커맨드(worker 피처 전용) 옵션: 원격 코어를 auto-poll->claim->실행->complete하는 헤드리스 데몬.
#[cfg(feature = "worker")]
#[derive(Args, Debug)]
struct WorkArgs {
    /// 코어 `/mcp` 절대 URL(예: http://192.0.2.10:8770/mcp, `/mcp` 경로까지 포함해서 지정).
    #[arg(long)]
    core: String,
    /// bearer 토큰(코어가 --token으로 띄워졌다면 필요).
    #[arg(long)]
    token: Option<String>,
    /// 이 워커의 to_agent id(예: win-worker). poll_tasks가 이 agent 앞 task만 본다.
    #[arg(long)]
    agent: String,
    /// task를 실행할 러너 종류(기본 claude).
    #[arg(long, value_enum, default_value_t = WorkRunner::Claude)]
    runner: WorkRunner,
    /// 러너에 넘길 모델 이름(옵션, 러너별 기본값 사용 가능).
    #[arg(long)]
    model: Option<String>,
    /// 러너가 작업할 로컬 레포 경로(옵션).
    #[arg(long = "project-path")]
    project_path: Option<String>,
    /// --runner http 전용: OpenAI 호환 chat API의 base URL(예: http://localhost:11434).
    #[arg(long = "http-base-url")]
    http_base_url: Option<String>,
    /// poll 간격(초, 기본 15).
    #[arg(long, default_value_t = 15)]
    interval: u64,
    /// 한 패스만 실행하고 종료(테스트·수동 실행용).
    #[arg(long)]
    once: bool,
    /// Write 모드로 실행(기본 ReadOnly=behavioral read-only 유지).
    #[arg(long)]
    write: bool,
}

/// `--runner` 선택지: 기존 Runner trait 구현체 중 어느 것으로 task를 실행할지.
#[cfg(feature = "worker")]
#[derive(clap::ValueEnum, Clone, Debug)]
enum WorkRunner {
    Claude,
    Codex,
    Opencode,
    Http,
}

// 일부 feature 조합(예: --no-default-features)에서는 남는 서브커맨드 분기 수가 줄어
// 아래 지역변수 중 일부를 모든 분기가 채우게 되어 초기값이 그 조합에서만 dead store로 잡힌다.
// 조합마다 다른 변수 집합이라 개별 분기 재설계보다 함수 단위 allow가 더 안전하다(동작 무변경).
#[allow(unused_assignments)]
fn main() {
    let cli = Cli::parse();
    let command = cli.command.unwrap_or_else(|| Commands::Chat(ChatArgs::default()));

    let mut roster_path: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut observe_id: Option<String> = None;
    let mut redis_session_id: Option<String> = None;
    let mut recent_turns: Option<usize> = None;
    // MCP 서버 모드에서 --session-id로 받은 기본 세션 id(없으면 "default"). mcp 피처 전용.
    #[cfg(feature = "mcp")]
    let mut mcp_session_id: Option<String> = None;
    // Pull 컨텍스트 모드 활성화 플래그. --db 없으면 무의미하므로 경고 후 Push 유지.
    let mut pull_context = false;
    // 모든 서브커맨드 분기가 정확히 1회 채운다(초기값 없이 선언 -> dead store 경고 회피).
    // mut인 이유: match 후 tunaround.toml 프로파일 병합이 CLI 미지정 필드를 채울 수 있어서다.
    #[cfg(feature = "sqlite")]
    let mut db_path: Option<String>;
    // --config <경로>: 설정 파일 명시 경로(chat/core/join 전용, 그 외 서브커맨드는 None 유지=무시).
    let mut config_path: Option<String> = None;
    // --profile <이름>: tunaround.toml 프로파일 이름(chat/core/join 전용).
    let mut profile_name: Option<String> = None;
    // config/profile 병합이 적용되는 서브커맨드인지(chat/core/join만 true).
    let mut profile_capable = false;
    // reindex: 모든 세션의 FTS·벡터 인덱스를 SoR(messages)에서 재생성(모델·토크나이저 교체 후 복구).
    #[cfg(feature = "sqlite")]
    let mut reindex = false;
    #[cfg(feature = "mcp")]
    let mut mcp_search = false;
    // serve <addr>: HTTP MCP 서버 상주 모드(헤드리스, REPL 없음. serve 피처 전용).
    #[cfg(feature = "serve")]
    let mut serve_mcp_addr: Option<String> = None;
    // core <addr>: front=core 단일 프로세스(REPL + in-process HTTP MCP 코어. serve 피처 전용).
    #[cfg(feature = "serve")]
    let mut core_addr: Option<String> = None;
    // --token <tok>: bearer 토큰 인증(serve 모드 전용).
    #[cfg(feature = "serve")]
    let mut serve_token: Option<String> = None;
    // --search-url <url>: 원격 HTTP MCP 서버 URL(stdio spawn 대신 접속).
    let mut search_url: Option<String> = None;
    // --search-token <tok>: HTTP MCP 서버 bearer 토큰(Authorization 헤더).
    let mut search_token: Option<String> = None;
    // work <...>: 헤드리스 자율 워커 데몬 옵션(worker 피처 전용).
    #[cfg(feature = "worker")]
    let mut work_args: Option<WorkArgs> = None;

    // 서브커맨드별 옵션을 기존 모드 본문이 쓰던 지역변수로 옮긴다(본문 로직은 아래에서 불변).
    match command {
        Commands::Chat(a) => {
            state_path = a.state_file;
            observe_id = a.observe;
            roster_path = a.common.roster;
            recent_turns = a.common.recent_turns;
            pull_context = a.common.pull_context;
            redis_session_id = a.common.session;
            search_url = a.common.search_url;
            search_token = a.common.search_token;
            config_path = a.common.config;
            profile_name = a.common.profile;
            profile_capable = true;
            #[cfg(feature = "sqlite")]
            {
                db_path = a.common.db;
            }
        }
        Commands::Join(a) => {
            state_path = a.state_file;
            roster_path = a.roster;
            pull_context = true;
            search_url = Some(a.url);
            search_token = a.token;
            config_path = a.config;
            profile_name = a.profile;
            profile_capable = true;
            #[cfg(feature = "sqlite")]
            {
                db_path = a.db;
            }
        }
        #[cfg(feature = "serve")]
        Commands::Core(a) => {
            state_path = a.state_file;
            roster_path = a.common.roster;
            recent_turns = a.common.recent_turns;
            pull_context = a.common.pull_context;
            redis_session_id = a.common.session;
            search_url = a.common.search_url;
            search_token = a.common.search_token;
            config_path = a.common.config;
            profile_name = a.common.profile;
            profile_capable = true;
            serve_token = a.token;
            core_addr = Some(a.addr);
            db_path = a.common.db;
        }
        #[cfg(feature = "serve")]
        Commands::Serve(a) => {
            serve_mcp_addr = Some(a.addr);
            serve_token = a.token;
            db_path = a.db;
        }
        #[cfg(feature = "mcp")]
        Commands::McpSearch(a) => {
            mcp_search = true;
            mcp_session_id = a.session_id;
            db_path = a.db;
        }
        #[cfg(feature = "sqlite")]
        Commands::Reindex(a) => {
            reindex = true;
            db_path = a.db;
        }
        #[cfg(feature = "worker")]
        Commands::Work(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            work_args = Some(a);
        }
    }

    // tunaround.toml 프로파일 병합(chat/core/join 경로에서만, profile_capable=false면 완전히 건너뜀
    // = serve/mcp-search/reindex는 --config/--profile을 아예 안 받으니 기존 동작과 100% 동일).
    // 우선순위: CLI 플래그(위 match에서 이미 채운 값) > 선택된 프로파일 > 각 로컬의 초기 기본값.
    if profile_capable {
        match tunaround::config::load_config(config_path.as_deref()) {
            Ok(None) => {
                if profile_name.is_some() {
                    eprintln!(
                        "[설정] --profile이 지정됐으나 설정 파일을 찾을 수 없습니다. \
                         --config <경로> 또는 ./tunaround.toml, ~/.config/tunaround/config.toml을 확인하세요."
                    );
                    std::process::exit(1);
                }
            }
            Ok(Some(cfg)) => match tunaround::config::select_profile(&cfg, profile_name.as_deref(), true) {
                Ok(selected) => {
                    #[cfg(feature = "sqlite")]
                    let db_for_merge = db_path.clone();
                    #[cfg(not(feature = "sqlite"))]
                    let db_for_merge: Option<String> = None;
                    let merged = tunaround::config::merge_profile_into(
                        tunaround::config::MergedSessionArgs {
                            db: db_for_merge,
                            roster: roster_path.clone(),
                            recent_turns,
                            pull_context,
                            session: redis_session_id.clone(),
                            search_url: search_url.clone(),
                            search_token: search_token.clone(),
                        },
                        selected,
                    );
                    #[cfg(feature = "sqlite")]
                    {
                        db_path = merged.db;
                    }
                    roster_path = merged.roster;
                    recent_turns = merged.recent_turns;
                    pull_context = merged.pull_context;
                    redis_session_id = merged.session;
                    search_url = merged.search_url;
                    search_token = merged.search_token;
                }
                Err(e) => {
                    eprintln!("[설정] {e}");
                    std::process::exit(1);
                }
            },
            Err(e) => {
                eprintln!("[설정] {e}");
                std::process::exit(1);
            }
        }
    }

    // tokio 런타임: read path(--observe/--session snapshot GET) + owner refresh에만 사용.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // --observe 모드: REPL 대신 라이브 구독 루프(read-only).
    if let Some(sid) = observe_id {
        let Some(bus) = tunaround::session_bus::RedisBus::open_from_env() else {
            eprintln!("[observe] TUNAROUND_REDIS_URL 필요");
            std::process::exit(1);
        };
        rt.block_on(async move {
            if let Ok(Some(snap)) = bus.get_snapshot(&sid).await {
                println!("=== 현재 스냅샷 ===\n{snap}\n=== 라이브 ===");
            }
            let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(256);
            let subscriber = {
                let bus = bus.clone();
                let sid = sid.clone();
                tokio::spawn(async move {
                    let _ = bus.subscribe_events(&sid, tx).await;
                })
            };
            while let Ok(payload) = rx.recv().await {
                println!("{payload}");
            }
            let _ = subscriber.await;
        });
        return;
    }

    // --reindex 모드: 모든 세션의 FTS·벡터 인덱스를 messages(SoR)에서 재생성(sqlite 피처 전용).
    #[cfg(feature = "sqlite")]
    if reindex {
        let db_str = match &db_path {
            Some(p) => p.clone(),
            None => { eprintln!("[reindex] --db <경로> 필요"); std::process::exit(1); }
        };
        let store = match tunaround::store::sqlite::SqliteStore::open(&db_str) {
            Ok(s) => s,
            Err(e) => { eprintln!("[reindex] DB 열기 실패: {e}"); std::process::exit(1); }
        };
        // 색인용 fts 토크나이저(fts_index: 형태소+raw).
        #[cfg(feature = "morphology")]
        let tok: Box<dyn Fn(&str) -> String + Send + Sync> = {
            match tunaround::search::tokenizer::create_tokenizer("kiwi") {
                Ok(t) => Box::new(move |s: &str| t.fts_index(s)),
                Err(e) => {
                    eprintln!("[reindex] 토크나이저 실패, 폴백: {e}");
                    Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "))
                }
            }
        };
        #[cfg(not(feature = "morphology"))]
        let tok: Box<dyn Fn(&str) -> String + Send + Sync> =
            Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "));
        // 벡터 임베더(semantic이면 재임베딩; model_id 키로 모델 교체 시 갱신).
        #[cfg(feature = "semantic")]
        let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = {
            Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
        };
        #[cfg(not(feature = "semantic"))]
        let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = None;

        let before = store.index_stats().unwrap_or((0, 0, 0, 0, 0));
        let sessions = match store.list_sessions() {
            Ok(v) => v,
            Err(e) => { eprintln!("[reindex] 세션 목록 실패: {e}"); std::process::exit(1); }
        };
        println!("[reindex] 세션 {}개 재색인 시작...", sessions.len());
        let mut ok = 0usize;
        for sid in &sessions {
            let Ok(Some(ss)) = store.load_session(sid) else { continue; };
            // FTS 재생성(전량 교체).
            if let Err(e) = store.save_session(sid, &ss, |t| tok(t)) {
                eprintln!("[reindex] {sid} FTS 재색인 실패: {e}");
                continue;
            }
            // 벡터 재색인(best-effort; model_id 키로 모델 교체 시 재임베딩).
            if let Some(e) = &emb
                && let Err(err) = store.index_vectors(sid, &ss, e.as_ref())
            {
                eprintln!("[reindex] {sid} 벡터 재색인 경고: {err}");
            }
            ok += 1;
        }
        let after = store.index_stats().unwrap_or((0, 0, 0, 0, 0));
        println!("[reindex] 완료: {ok}/{} 세션. 인덱스(전): sessions={} messages={} fts={} vectors={} validity={}",
            sessions.len(), before.0, before.1, before.2, before.3, before.4);
        println!("[reindex] 인덱스(후): sessions={} messages={} fts={} vectors={} validity={}",
            after.0, after.1, after.2, after.3, after.4);
        return;
    }

    // --mcp-search 모드: REPL 대신 stdio MCP 검색 서버 기동(mcp 피처 전용).
    #[cfg(feature = "mcp")]
    if mcp_search {
        let db_str = match &db_path {
            Some(p) => p.clone(),
            None => {
                eprintln!("[mcp-search] --db <경로> 필요");
                std::process::exit(1);
            }
        };
        let store = match tunaround::store::sqlite::SqliteStore::open(&db_str) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[mcp-search] DB 열기 실패: {e}");
                std::process::exit(1);
            }
        };
        #[cfg(feature = "morphology")]
        let tok: Box<dyn Fn(&str) -> String + Send + Sync> = {
            match tunaround::search::tokenizer::create_tokenizer("kiwi") {
                Ok(t) => Box::new(move |s: &str| t.fts_query(s)),
                Err(e) => {
                    eprintln!("[mcp-search] 토크나이저 실패, 폴백: {e}");
                    Box::new(|s: &str| {
                        let mut toks = tunaround::search::tokenize_fallback(s);
                        let al: Vec<String> = toks.iter().flat_map(|t| tunaround::search::loanword_aliases(t)).collect();
                        toks.extend(al);
                        toks.sort();
                        toks.dedup();
                        toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
                    })
                }
            }
        };
        #[cfg(not(feature = "morphology"))]
        let tok: Box<dyn Fn(&str) -> String + Send + Sync> = Box::new(|s: &str| {
            let mut toks = tunaround::search::tokenize_fallback(s);
            let al: Vec<String> = toks.iter().flat_map(|t| tunaround::search::loanword_aliases(t)).collect();
            toks.extend(al);
            toks.sort();
            toks.dedup();
            toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
        });
        #[cfg(feature = "semantic")]
        let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = {
            Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
        };
        #[cfg(not(feature = "semantic"))]
        let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = None;
        let store2 = match tunaround::store::sqlite::SqliteStore::open(&db_str) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("[mcp-search] 전사 리더 DB 열기 실패: {e}");
                std::process::exit(1);
            }
        };
        let retriever = tunaround::store::retriever::SqliteRetriever::new(store, tok, emb);
        let retriever_arc =
            std::sync::Arc::new(retriever) as std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>;
        let transcript_reader = tunaround::store::retriever::SqliteTranscriptReader::new(store2);
        let reader_arc: Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>> =
            Some(std::sync::Arc::new(transcript_reader));
        let mcp_default_session = mcp_session_id.clone().unwrap_or_else(|| "default".to_string());
        if let Err(e) = rt.block_on(tunaround::mcp::start_mcp_server(retriever_arc, reader_arc, mcp_default_session)) {
            eprintln!("[mcp-search] 서버 오류: {e}");
            std::process::exit(1);
        }
        return;
    }

    // --serve-mcp 모드: REPL 대신 HTTP MCP 서버 상주(serve 피처 전용).
    #[cfg(feature = "serve")]
    if let Some(ref addr) = serve_mcp_addr {
        let db_str = {
            #[cfg(feature = "sqlite")]
            {
                match &db_path {
                    Some(p) => p.clone(),
                    None => { eprintln!("[serve-mcp] --db <경로> 필요"); std::process::exit(1); }
                }
            }
            #[cfg(not(feature = "sqlite"))]
            { eprintln!("[serve-mcp] sqlite 피처 없음"); std::process::exit(1); }
        };
        let (retriever_arc, reader_arc, writer_arc, a2a_store_arc) = build_http_mcp_backends("serve-mcp", &db_str);
        // 헤드리스 코어: post_turn 활성(단일 writer라 클로버 없음), 로스터 없음.
        if let Err(e) = rt.block_on(tunaround::mcp::start_http_mcp_server(
            addr, retriever_arc, reader_arc, Some(writer_arc), None, serve_token.clone(), a2a_store_arc,
        )) {
            eprintln!("[serve-mcp] 서버 오류: {e}");
            std::process::exit(1);
        }
        return;
    }

    // work 모드: 원격 코어를 auto-poll->claim->실행->complete하는 헤드리스 워커 데몬(worker 피처 전용).
    #[cfg(feature = "worker")]
    if let Some(a) = work_args {
        let mode = if a.write {
            tunaround::runner::RunMode::Write
        } else {
            tunaround::runner::RunMode::ReadOnly
        };
        let runner: std::sync::Arc<dyn tunaround::runner::Runner + Send + Sync> = match a.runner {
            WorkRunner::Claude => std::sync::Arc::new(tunaround::runner::claude::ClaudeRunner::new()),
            WorkRunner::Codex => std::sync::Arc::new(tunaround::runner::codex::CodexRunner::new()),
            WorkRunner::Opencode => {
                std::sync::Arc::new(tunaround::runner::opencode::OpencodeRunner::new().with_model(a.model.clone()))
            }
            #[cfg(feature = "engines")]
            WorkRunner::Http => {
                let base_url = match &a.http_base_url {
                    Some(u) => u.clone(),
                    None => {
                        eprintln!("[work] --runner http 는 --http-base-url <url>이 필요합니다");
                        std::process::exit(1);
                    }
                };
                std::sync::Arc::new(tunaround::runner::http::OpenAiChatRunner::new(
                    &base_url,
                    a.model.as_deref().unwrap_or(""),
                    a.token.clone(),
                ))
            }
            #[cfg(not(feature = "engines"))]
            WorkRunner::Http => {
                eprintln!("[work] --runner http 는 engines 피처가 필요합니다");
                std::process::exit(1);
            }
        };

        let result = rt.block_on(async {
            let client = tunaround::mcp_client::McpHttpClient::connect(a.core.clone(), a.token.clone()).await?;
            tunaround::worker::run_worker_loop(
                &client,
                runner,
                &a.agent,
                a.model.clone(),
                a.project_path.clone(),
                mode,
                a.interval,
                a.once,
            )
            .await
        });
        if let Err(e) = result {
            eprintln!("[work] 오류: {e}");
            std::process::exit(1);
        }
        return;
    }

    // --core <addr> 모드(serve 전용): 로컬 좌석을 in-process 코어에 HTTP로 배선한다.
    // 실제 서버 spawn + REPL core-sync 배선은 participants 빌드 후(아래)에서 한다(로스터 주입 위해).
    #[cfg(feature = "serve")]
    if let Some(addr) = core_addr.clone() {
        if db_path.is_none() {
            eprintln!("[core] --core는 --db <경로>가 필요합니다");
            std::process::exit(1);
        }
        // 로컬 좌석을 in-process 코어에 배선(명시 --search-url이 있으면 그쪽 우선).
        if search_url.is_some() {
            eprintln!("[core] 명시 --search-url이 있어 로컬 좌석은 그 URL을 사용합니다(코어는 {addr}에서 원격용으로 서빙).");
        } else {
            let local_url = tunaround::mcp::core_local_url(&addr);
            eprintln!("[core] 로컬 좌석을 in-process 코어에 배선: {local_url}");
            search_url = Some(local_url);
            if search_token.is_none() {
                search_token = serve_token.clone();
            }
        }
    }

    // session_id: --session <id> 값 또는 "default". 러너 생성 전에 계산한다.
    let sid = redis_session_id.clone().unwrap_or_else(|| "default".to_string());

    // 로스터 파일이 있으면 동적 좌석, 없으면 기본 2자리(claude proposer + codex reviewer).
    let (participants, registry): (Vec<Participant>, MapRegistry) = match &roster_path {
        Some(p) => {
            let roster = match tunaround::roster::load_roster(p) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            let parts = match tunaround::roster::build_participants_checked(&roster) {
                Ok(v) => v,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            #[cfg(feature = "mcp")]
            let roster_search_db: Option<&str> = db_path.as_deref();
            #[cfg(not(feature = "mcp"))]
            let roster_search_db: Option<&str> = None;
            let reg = match tunaround::roster::build_registry(
                &roster,
                roster_search_db,
                search_url.as_deref(),
                search_token.as_deref(),
            ) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            (parts, reg)
        }
        None => {
            let mut reg = MapRegistry::new();
            #[cfg(feature = "mcp")]
            let claude_runner = ClaudeRunner::new()
                .with_search_db(db_path.clone())
                .with_search_session(Some(sid.clone()))
                .with_search_url(search_url.clone(), search_token.clone());
            #[cfg(not(feature = "mcp"))]
            let claude_runner = ClaudeRunner::new()
                .with_search_url(search_url.clone(), search_token.clone());
            reg.insert("claude", Box::new(claude_runner));
            #[cfg(feature = "mcp")]
            let codex_runner = CodexRunner::new()
                .with_search_db(db_path.clone())
                .with_search_session(Some(sid.clone()))
                .with_search_url(search_url.clone(), search_token.clone());
            #[cfg(not(feature = "mcp"))]
            let codex_runner = CodexRunner::new()
                .with_search_url(search_url.clone(), search_token.clone());
            reg.insert("codex", Box::new(codex_runner));
            let parts = vec![
                Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
                Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
            ];
            (parts, reg)
        }
    };

    // --core 로스터 스냅샷: participants가 session에 이동되기 전 좌석 구성을 캡처(get_roster 노출용).
    #[cfg(feature = "serve")]
    let core_roster: Option<Vec<tunaround::orchestrator::RosterSeat>> = core_addr.as_ref().map(|_| {
        participants
            .iter()
            .map(|p| tunaround::orchestrator::RosterSeat {
                engine: p.engine.clone(),
                role: p.role.clone(),
            })
            .collect()
    });

    // bus 핸들 준비: TUNAROUND_REDIS_URL 없으면 None(기존 동작 불변).
    // RedisBusHandle::spawn은 tokio::spawn을 내부 호출하므로 rt.enter() 가드 안에서 생성.
    let _g = rt.enter();
    let bus_handle = tunaround::session_bus::RedisBusHandle::spawn_from_env();
    drop(_g);
    let bus_boxed = bus_handle.map(|h| Box::new(h) as Box<dyn tunaround::session_bus::SessionBus>);

    // SQLite 인덱서 생성(feature-gated). --db 없거나 sqlite off면 None=기존 동작 불변.
    #[cfg(feature = "sqlite")]
    let indexer: Option<Box<dyn tunaround::store::indexer::MessageIndexer>> = match &db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => {
                #[cfg(feature = "morphology")]
                let tok: Box<dyn Fn(&str) -> String + Send + Sync> = {
                    match tunaround::search::tokenizer::create_tokenizer("kiwi") {
                        Ok(t) => Box::new(move |s: &str| t.fts_index(s)),
                        Err(e) => {
                            eprintln!("[tunaRound] 토크나이저 실패, 폴백: {e}");
                            Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "))
                        }
                    }
                };
                #[cfg(not(feature = "morphology"))]
                let tok: Box<dyn Fn(&str) -> String + Send + Sync> =
                    Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "));
                // semantic 피처: OllamaEmbedder 인스턴스(indexer용). 연결 실패는 best-effort.
                #[cfg(feature = "semantic")]
                let emb_idx: Option<Box<dyn tunaround::store::embedding::Embedder>> = {
                    Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
                };
                #[cfg(not(feature = "semantic"))]
                let emb_idx: Option<Box<dyn tunaround::store::embedding::Embedder>> = None;
                Some(Box::new(tunaround::store::indexer::SqliteIndexer::new(store, tok, emb_idx))
                    as Box<dyn tunaround::store::indexer::MessageIndexer>)
            }
            Err(e) => {
                eprintln!("[tunaRound] --db 열기 실패: {e}");
                None
            }
        },
        None => None,
    };
    #[cfg(not(feature = "sqlite"))]
    let indexer: Option<Box<dyn tunaround::store::indexer::MessageIndexer>> = None;

    // SQLite retriever 생성(feature-gated). indexer와 별개의 읽기 연결(WAL 동시 reader OK).
    #[cfg(feature = "sqlite")]
    let retriever: Option<Box<dyn tunaround::orchestrator::ContextRetriever>> = match &db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => {
                #[cfg(feature = "morphology")]
                let tok2: Box<dyn Fn(&str) -> String + Send + Sync> = {
                    match tunaround::search::tokenizer::create_tokenizer("kiwi") {
                        Ok(t) => Box::new(move |s: &str| t.fts_query(s)),
                        Err(e) => {
                            eprintln!("[tunaRound] retriever 토크나이저 실패, 폴백: {e}");
                            Box::new(|s: &str| {
                                let mut toks = tunaround::search::tokenize_fallback(s);
                                toks.sort();
                                toks.dedup();
                                toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
                            })
                        }
                    }
                };
                #[cfg(not(feature = "morphology"))]
                let tok2: Box<dyn Fn(&str) -> String + Send + Sync> = Box::new(|s: &str| {
                    let mut toks = tunaround::search::tokenize_fallback(s);
                    toks.sort();
                    toks.dedup();
                    toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
                });
                // semantic 피처: OllamaEmbedder 인스턴스(retriever용). 연결 실패는 best-effort.
                #[cfg(feature = "semantic")]
                let emb_ret: Option<Box<dyn tunaround::store::embedding::Embedder>> = {
                    Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
                };
                #[cfg(not(feature = "semantic"))]
                let emb_ret: Option<Box<dyn tunaround::store::embedding::Embedder>> = None;
                Some(Box::new(tunaround::store::retriever::SqliteRetriever::new(store, tok2, emb_ret))
                    as Box<dyn tunaround::orchestrator::ContextRetriever>)
            }
            Err(e) => {
                eprintln!("[tunaRound] retriever --db 열기 실패: {e}");
                None
            }
        },
        None => None,
    };
    #[cfg(not(feature = "sqlite"))]
    let retriever: Option<Box<dyn tunaround::orchestrator::ContextRetriever>> = None;

    // 세션 초기 상태 결정(우선순위: 파일 resume > Redis snapshot > 신규).
    let resume_existing = state_path
        .as_deref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);

    let session = if resume_existing {
        // 파일에서 트리 상태를 로드하고 new_with_bus로 bus를 연결한다.
        let p = state_path.as_deref().unwrap();
        match tunaround::store::load_session(p) {
            Ok(ss) => {
                println!("(이어받음: {p})");
                let mut s = Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer);
                s.seed_from(ss);
                s
            }
            Err(e) => {
                eprintln!("[resume 실패: {e}] 종료합니다.");
                std::process::exit(1);
            }
        }
    } else if redis_session_id.is_some() {
        // --session <id>: Redis snapshot에서 재개(라이브 Redis 있을 때만 실제 동작).
        if let Some(raw_bus) = tunaround::session_bus::RedisBus::open_from_env() {
            match rt.block_on(raw_bus.get_snapshot(&sid)) {
                Ok(Some(json)) => {
                    match serde_json::from_str::<tunaround::store::StoredSession>(&json) {
                        Ok(ss) => {
                            println!("(Redis snapshot 재개: {sid})");
                            // 위에서 만든 bus_boxed를 재사용한다(중복 핸들 spawn 방지).
                            let mut s = Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer);
                            s.seed_from(ss);
                            // owner lease 시도.
                            let worker_id = std::process::id().to_string();
                            match rt.block_on(raw_bus.try_acquire_owner(&sid, &worker_id, 60)) {
                                Ok(true) => {
                                    // 백그라운드 owner refresh.
                                    let refresh_bus = raw_bus.clone();
                                    let refresh_sid = sid.clone();
                                    let refresh_wid = worker_id.clone();
                                    rt.spawn(async move {
                                        loop {
                                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                            let _ = refresh_bus.refresh_owner(&refresh_sid, &refresh_wid, 60).await;
                                        }
                                    });
                                }
                                Ok(false) => eprintln!("[경고] 다른 프로세스가 driver일 수 있음: {sid}"),
                                Err(e) => eprintln!("[owner lease 실패] {e}"),
                            }
                            s
                        }
                        Err(e) => {
                            eprintln!("[snapshot 파싱 실패: {e}] 신규 세션 시작.");
                            Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer)
                        }
                    }
                }
                _ => {
                    eprintln!("[snapshot 없음] 신규 세션 시작.");
                    Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer)
                }
            }
        } else {
            eprintln!("[--session] TUNAROUND_REDIS_URL 없음: 로컬 단일세션으로 시작.");
            Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer)
        }
    } else {
        Session::new_with_indexer(participants, Box::new(registry), sid.clone(), bus_boxed, indexer)
    };

    // --pull-context 적용. --db 없으면(MCP 백엔드 없음) pull 무의미 → 경고 후 Push 유지.
    let context_mode = if pull_context {
        #[cfg(feature = "sqlite")]
        let has_db = db_path.is_some();
        #[cfg(not(feature = "sqlite"))]
        let has_db = false;
        if !has_db {
            eprintln!("[경고] --pull-context는 --db 없이는 무의미합니다. Push 모드를 유지합니다.");
            ContextMode::Push
        } else {
            ContextMode::Pull
        }
    } else {
        ContextMode::Push
    };

    // retriever + recent_turns + context_mode 1회 배선(session 생성 if/else 이후 단일 적용).
    // 유효성 지정 sink(--db 있으면 배선). /supersede·/reject가 message_validity에 쓴다.
    #[cfg(feature = "sqlite")]
    let validity_sink: Option<Box<dyn tunaround::orchestrator::ValiditySink>> = match &db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => Some(Box::new(tunaround::store::retriever::SqliteValiditySink::new(store))),
            Err(e) => {
                eprintln!("[tunaRound] validity sink DB 열기 실패: {e}");
                None
            }
        },
        None => None,
    };
    #[cfg(not(feature = "sqlite"))]
    let validity_sink: Option<Box<dyn tunaround::orchestrator::ValiditySink>> = None;

    let mut session = session
        .with_retriever(retriever)
        .with_recent_turns(recent_turns)
        .with_context_mode(context_mode)
        .with_validity_sink(validity_sink);

    // --core 배선(participants/session 빌드 후): seed→코어 DB 권위 반영 → HTTP MCP 서버 spawn(로스터 주입)
    //  → REPL core-sync 연결. 이 순서라야 로스터 스냅샷과 권위 트리가 일관된다.
    #[cfg(feature = "serve")]
    if let Some(addr) = core_addr.clone() {
        let db_str = db_path.clone().expect("--core는 위에서 --db를 검증함");
        // seed(파일/redis 재개)가 있으면 코어 DB에 먼저 전량 반영해 DB를 단일 권위로 만든다
        // (이후 core-sync adopt가 DB를 채택하므로 seed 유실/이드 충돌 방지).
        if session.message_count() > 0 {
            match tunaround::store::sqlite::SqliteStore::open(&db_str) {
                Ok(store) => {
                    let tok = build_index_tokenizer("core");
                    if let Err(e) = store.save_session(&sid, &session.to_stored(), |t| tok(t)) {
                        eprintln!("[core] seed 코어 DB 반영 실패: {e}");
                    }
                }
                Err(e) => eprintln!("[core] seed 반영용 DB 열기 실패: {e}"),
            }
        }
        // HTTP MCP 코어 백엔드 + 전용 스레드(자체 런타임 block_on) 서빙(로스터 포함).
        let (retriever_arc, reader_arc, writer_arc, a2a_store_arc) = build_http_mcp_backends("core", &db_str);
        let serve_tok = serve_token.clone();
        let addr_owned = addr.clone();
        // 메인 스레드는 동기 블로킹 REPL(std stdin)이라 공유 rt에 spawn하면 서버 accept 루프가
        // 유휴 중 간헐적으로만 구동된다(신뢰 불가). 전용 OS 스레드의 자체 런타임 block_on이 서버를
        // 계속 구동해 원격 클라이언트(curl·에이전트)에 안정적으로 응답한다.
        std::thread::spawn(move || {
            let srt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => { eprintln!("[core] 서버 런타임 생성 실패: {e}"); return; }
            };
            srt.block_on(async move {
                if let Err(e) = tunaround::mcp::start_http_mcp_server(
                    &addr_owned, retriever_arc, reader_arc, Some(writer_arc), core_roster, serve_tok, a2a_store_arc,
                ).await {
                    eprintln!("[core] HTTP MCP 서버 종료: {e}");
                }
            });
        });
        // REPL core-sync: 코어 DB(--db)를 권위로 삼아 매 라운드 adopt + append_turn 쓰기.
        match tunaround::store::sqlite::SqliteStore::open(&db_str) {
            Ok(store) => {
                let core_sync = tunaround::store::retriever::SqliteCoreSync::new(store, build_index_tokenizer("core"));
                session = session.with_core_sync(Some(Box::new(core_sync)));
            }
            Err(e) => eprintln!("[core] core-sync DB 열기 실패(병합 비활성): {e}"),
        }
    }

    println!("tunaRound - 메시지를 입력하세요. /help, /save, /quit.");
    let stdin = io::stdin();
    loop {
        print!("\n> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        match session.step(parse_command(&line)) {
            StepOutcome::Print(text) => println!("{text}"),
            StepOutcome::Noop => {}
            StepOutcome::Save { path, markdown } => match std::fs::write(&path, markdown) {
                Ok(()) => println!("저장됨: {path}"),
                Err(e) => println!("[저장 실패] {e}"),
            },
            StepOutcome::Exit => break,
        }
    }
    if let Some(p) = &state_path {
        match session.save_state(p) {
            Ok(()) => println!("세션 저장됨: {p}"),
            Err(e) => println!("[세션 저장 실패] {e}"),
        }
    }

    // bus 미러는 fire-and-forget이라 마지막 라운드 스냅샷이 종료 시 유실될 수 있다.
    // resume 정확성을 위해 종료 직전 최종 스냅샷을 동기로 1회 기록한다(Redis 있을 때만).
    if session.message_count() > 0 {
        let flush_bus = {
            let _g = rt.enter();
            tunaround::session_bus::RedisBus::open_from_env()
        };
        if let Some(rb) = flush_bus {
            let _ = rt.block_on(rb.set_snapshot(&sid, &session.snapshot_json()));
        }
    }
}

/// 색인용 FTS 토크나이저 closure(fts_index: 형태소+raw, indexer/writer 공용). serve 전용.
#[cfg(feature = "serve")]
fn build_index_tokenizer(ctx: &str) -> Box<dyn Fn(&str) -> String + Send + Sync> {
    #[cfg(feature = "morphology")]
    {
        match tunaround::search::tokenizer::create_tokenizer("kiwi") {
            Ok(t) => Box::new(move |s: &str| t.fts_index(s)),
            Err(e) => {
                eprintln!("[{ctx}] 색인 토크나이저 실패, 폴백: {e}");
                Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "))
            }
        }
    }
    #[cfg(not(feature = "morphology"))]
    {
        let _ = ctx;
        Box::new(|s: &str| tunaround::search::tokenize_fallback(s).join(" "))
    }
}

/// build_http_mcp_backends 반환 묶음: (retriever, 전사 리더, writer, A2A store).
#[cfg(feature = "serve")]
type HttpMcpBackends = (
    std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>,
    Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>>,
    std::sync::Arc<dyn tunaround::orchestrator::TranscriptWriter>,
    std::sync::Arc<std::sync::Mutex<tunaround::store::sqlite::SqliteStore>>,
);

/// HTTP MCP 코어용 retriever + 전사 리더 + writer를 --db 경로로 빌드한다(--serve-mcp / --core 공용).
/// serve→mcp→sqlite라 sqlite는 항상 켜짐. 실패 시 ctx 프리픽스로 에러를 찍고 종료한다.
#[cfg(feature = "serve")]
fn build_http_mcp_backends(ctx: &str, db_str: &str) -> HttpMcpBackends {
    let store = match tunaround::store::sqlite::SqliteStore::open(db_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{ctx}] DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    // 질의용 토크나이저(fts_query: prefix). retriever 전용.
    #[cfg(feature = "morphology")]
    let tok: Box<dyn Fn(&str) -> String + Send + Sync> = {
        match tunaround::search::tokenizer::create_tokenizer("kiwi") {
            Ok(t) => Box::new(move |s: &str| t.fts_query(s)),
            Err(e) => {
                eprintln!("[{ctx}] 토크나이저 실패, 폴백: {e}");
                Box::new(|s: &str| {
                    let mut toks = tunaround::search::tokenize_fallback(s);
                    toks.sort();
                    toks.dedup();
                    toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
                })
            }
        }
    };
    #[cfg(not(feature = "morphology"))]
    let tok: Box<dyn Fn(&str) -> String + Send + Sync> = Box::new(|s: &str| {
        let mut toks = tunaround::search::tokenize_fallback(s);
        toks.sort();
        toks.dedup();
        toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" ")
    });
    #[cfg(feature = "semantic")]
    let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = {
        Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
    };
    #[cfg(not(feature = "semantic"))]
    let emb: Option<Box<dyn tunaround::store::embedding::Embedder>> = None;
    let store2 = match tunaround::store::sqlite::SqliteStore::open(db_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{ctx}] 전사 리더 DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    let store3 = match tunaround::store::sqlite::SqliteStore::open(db_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{ctx}] writer DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    let store4 = match tunaround::store::sqlite::SqliteStore::open(db_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{ctx}] A2A store DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    let retriever = tunaround::store::retriever::SqliteRetriever::new(store, tok, emb);
    let retriever_arc = std::sync::Arc::new(retriever)
        as std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>;
    let transcript_reader = tunaround::store::retriever::SqliteTranscriptReader::new(store2);
    let reader_arc: Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>> =
        Some(std::sync::Arc::new(transcript_reader));
    let writer = tunaround::store::retriever::SqliteTranscriptWriter::new(store3, build_index_tokenizer(ctx));
    let writer_arc = std::sync::Arc::new(writer)
        as std::sync::Arc<dyn tunaround::orchestrator::TranscriptWriter>;
    // A2A JSON-RPC 핸들러(a2a_server)가 create_task/get_task 등 SqliteStore 메서드를 직접 호출한다
    // (다른 백엔드처럼 orchestrator 트레이트로 감싸지 않음. A2A는 orchestrator 개념과 무관한 신규 축).
    // A2A 이벤트 버스 활성화(SendStreamingMessage SSE 구독용, docs/design/v2-a2a-streaming_2026-07-03.md
    // §2.1). worker(claim/complete)와 SSE 구독이 같은 store Arc를 공유하므로 여기서 한 번만 켜면 된다.
    let a2a_store_arc = std::sync::Arc::new(std::sync::Mutex::new(store4.with_task_events()));
    (retriever_arc, reader_arc, writer_arc, a2a_store_arc)
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
}
