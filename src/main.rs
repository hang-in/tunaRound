// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use clap::Parser;
use tunaround::orchestrator::{ContextMode, MapRegistry, Participant};
use tunaround::repl::{Session, StepOutcome, parse_command};

mod cli;
#[cfg(feature = "worker")]
mod cli_daemons;
mod cli_node;
mod cli_run;

use cli::*;

// 일부 feature 조합(예: --no-default-features)에서는 남는 서브커맨드 분기 수가 줄어
// 아래 지역변수 중 일부를 모든 분기가 채우게 되어 초기값이 그 조합에서만 dead store로 잡힌다.
// 조합마다 다른 변수 집합이라 개별 분기 재설계보다 함수 단위 allow가 더 안전하다(동작 무변경).
#[allow(unused_assignments)]
fn main() {
    let cli = Cli::parse();
    let command = cli
        .command
        .unwrap_or_else(|| Commands::Chat(ChatArgs::default()));

    let mut roster_path: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut observe_id: Option<String> = None;
    // --session <id>: 재개할 SQLite 세션 id(--db 있을 때만 재개). 프로파일 session 키와 동일 의미.
    let mut session_arg: Option<String> = None;
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
    // poll <...>: 감시 전용 옵션(worker 피처 전용).
    #[cfg(feature = "worker")]
    let mut poll_args: Option<PollArgs> = None;
    // codex-inject <...>: codex app-server ws 주입 옵션(worker 피처 전용).
    #[cfg(feature = "worker")]
    let mut codex_inject_args: Option<CodexInjectArgs> = None;
    // watch-results <...>: 총괄 결과 인박스 옵션(worker 피처 전용).
    #[cfg(feature = "worker")]
    let mut watch_results_args: Option<WatchResultsArgs> = None;
    // presence-scan <...>: 머신당 presence 스캐너 옵션(worker 피처 전용, v2-44).
    #[cfg(feature = "worker")]
    let mut presence_scan_args: Option<PresenceScanArgs> = None;
    // task <...>: A2A task 수동 조작 CLI 옵션(worker 피처 전용, v2-44 W3).
    #[cfg(feature = "worker")]
    let mut task_cli_args: Option<TaskArgs> = None;
    // codex-relay <...>: 머신당 codex 배달 데몬 옵션(worker 피처 전용, v2-46).
    #[cfg(feature = "worker")]
    let mut codex_relay_args: Option<CodexRelayArgs> = None;
    // node <...>: 워커 노드 상주 옵션(serve+worker 피처 전용).
    #[cfg(all(feature = "serve", feature = "worker"))]
    let mut node_args: Option<NodeArgs> = None;
    // doctor <...>: node 설정 진단 옵션(serve+worker 피처 전용).
    #[cfg(all(feature = "serve", feature = "worker"))]
    let mut doctor_args: Option<DoctorArgs> = None;
    // init <...>: 온보딩(node.toml 생성) 옵션(serve+worker 피처 전용).
    #[cfg(all(feature = "serve", feature = "worker"))]
    let mut init_args: Option<InitArgs> = None;

    // 서브커맨드별 옵션을 기존 모드 본문이 쓰던 지역변수로 옮긴다(본문 로직은 아래에서 불변).
    match command {
        Commands::Chat(a) => {
            state_path = a.state_file;
            observe_id = a.observe;
            roster_path = a.common.roster;
            recent_turns = a.common.recent_turns;
            pull_context = a.common.pull_context;
            session_arg = a.common.session;
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
            session_arg = a.common.session;
            search_url = a.common.search_url;
            search_token = a.common.search_token;
            config_path = a.common.config;
            profile_name = a.common.profile;
            profile_capable = true;
            // 토큰은 --token 우선, 없으면 TUNA_BROKER_TOKEN env 폴백(argv 노출 회피).
            serve_token = a.token.or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
            core_addr = Some(a.addr);
            db_path = a.common.db;
        }
        #[cfg(feature = "serve")]
        Commands::Serve(a) => {
            serve_mcp_addr = Some(a.addr);
            // 토큰은 --token 우선, 없으면 TUNA_BROKER_TOKEN env 폴백(argv 노출 회피).
            serve_token = a.token.or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
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
        #[cfg(feature = "worker")]
        Commands::Poll(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            poll_args = Some(a);
        }
        #[cfg(feature = "worker")]
        Commands::CodexInject(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            codex_inject_args = Some(a);
        }
        #[cfg(feature = "worker")]
        Commands::WatchResults(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            watch_results_args = Some(a);
        }
        #[cfg(feature = "worker")]
        Commands::PresenceScan(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            presence_scan_args = Some(a);
        }
        #[cfg(feature = "worker")]
        Commands::Task(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            task_cli_args = Some(a);
        }
        #[cfg(feature = "worker")]
        Commands::CodexRelay(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            codex_relay_args = Some(a);
        }
        #[cfg(all(feature = "serve", feature = "worker"))]
        Commands::Node(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            node_args = Some(a);
        }
        #[cfg(all(feature = "serve", feature = "worker"))]
        Commands::Doctor(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            doctor_args = Some(a);
        }
        #[cfg(all(feature = "serve", feature = "worker"))]
        Commands::Init(a) => {
            #[cfg(feature = "sqlite")]
            {
                db_path = None;
            }
            init_args = Some(a);
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
            Ok(Some(cfg)) => {
                match tunaround::config::select_profile(&cfg, profile_name.as_deref(), true) {
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
                                session: session_arg.clone(),
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
                        session_arg = merged.session;
                        search_url = merged.search_url;
                        search_token = merged.search_token;
                    }
                    Err(e) => {
                        eprintln!("[설정] {e}");
                        std::process::exit(1);
                    }
                }
            }
            Err(e) => {
                eprintln!("[설정] {e}");
                std::process::exit(1);
            }
        }
    }

    // tokio 런타임: HTTP MCP 서버(serve-mcp/core/node) + 워커 데몬 경로에서만 사용.
    // (--observe/--session/flush의 Redis 경로가 사라져 기본 빌드에선 rt가 필요 없다.)
    #[cfg(any(feature = "mcp", feature = "worker"))]
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // --observe 모드: REPL 대신 SQLite 세션을 폴링 tail(read-only). msg_id 커서로 새 발언만 출력.
    if let Some(sid) = observe_id {
        #[cfg(feature = "sqlite")]
        cli_run::run_observe(sid, db_path.clone());
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = sid;
            eprintln!("[observe] sqlite 피처가 필요합니다");
            std::process::exit(1);
        }
    }

    // --reindex 모드: 모든 세션의 FTS·벡터 인덱스를 messages(SoR)에서 재생성(sqlite 피처 전용).
    #[cfg(feature = "sqlite")]
    if reindex {
        cli_run::run_reindex(&db_path);
        return;
    }

    // --mcp-search 모드: REPL 대신 stdio MCP 검색 서버 기동(mcp 피처 전용).
    #[cfg(feature = "mcp")]
    if mcp_search {
        cli_run::run_mcp_search(&rt, &db_path, mcp_session_id.clone());
        return;
    }

    // --serve-mcp 모드: REPL 대신 HTTP MCP 서버 상주(serve 피처 전용).
    #[cfg(feature = "serve")]
    if let Some(ref addr) = serve_mcp_addr {
        cli_run::run_serve_mcp(&rt, addr, &db_path, serve_token.clone());
        return;
    }

    // work 모드: 원격 코어를 auto-poll->claim->실행->complete하는 헤드리스 워커 데몬(worker 피처 전용).
    #[cfg(feature = "worker")]
    if let Some(a) = work_args {
        return cli_daemons::work(&rt, a);
    }

    // poll <...>: 감시 전용(claim/실행 없음). 코어에 연결해 새 task를 stdout으로 알린다.
    #[cfg(feature = "worker")]
    if let Some(a) = poll_args {
        return cli_daemons::poll(&rt, a);
    }

    // watch-results <...>: 총괄이 던진 task의 완료/실패를 브로커 SSE로 받아 stdout으로 알린다(worker 피처).
    #[cfg(feature = "worker")]
    if let Some(a) = watch_results_args {
        return cli_daemons::watch_results(&rt, a);
    }

    // presence-scan <...>: 머신당 스캐너 데몬 = 라이브 세션 전집합을 브로커에 일괄 동기화(v2-44).
    #[cfg(feature = "worker")]
    if let Some(a) = presence_scan_args {
        return cli_daemons::presence_scan(&rt, a);
    }

    // task <...>: A2A task 수동 조작 CLI(v2-44 W3). 결과 텍스트를 그대로 stdout에 낸다(컴팩트).
    #[cfg(feature = "worker")]
    if let Some(a) = task_cli_args {
        return cli_daemons::task_cli(&rt, a);
    }

    // codex-inject <...>: codex app-server 라이브 thread에 turn/start로 유저 턴 1건 주입(worker 피처).
    #[cfg(feature = "worker")]
    if let Some(a) = codex_inject_args {
        return cli_daemons::codex_inject(&rt, a);
    }

    // codex-relay <...>: 머신당 codex 배달 데몬(v2-46). 세션 thread 직주입.
    #[cfg(feature = "worker")]
    if let Some(a) = codex_relay_args {
        return cli_daemons::codex_relay(&rt, a);
    }

    // init <...>: node.toml 생성 후 exit.
    #[cfg(all(feature = "serve", feature = "worker"))]
    if let Some(a) = init_args {
        std::process::exit(cli_node::run_init(&a));
    }

    // doctor <...>: node 설정 진단 후 exit code로 결과 보고.
    #[cfg(all(feature = "serve", feature = "worker"))]
    if let Some(a) = doctor_args {
        std::process::exit(cli_node::run_doctor(a.config.as_deref()));
    }

    // node <...>: node.toml대로 브로커(self)+자동 워커 레인들을 한 프로세스로 상주.
    #[cfg(all(feature = "serve", feature = "worker"))]
    if let Some(a) = node_args {
        cli_run::run_node(&rt, a);
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
            eprintln!(
                "[core] 명시 --search-url이 있어 로컬 좌석은 그 URL을 사용합니다(코어는 {addr}에서 원격용으로 서빙)."
            );
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
    let sid = session_arg.clone().unwrap_or_else(|| "default".to_string());

    // 로스터 파일이 있으면 동적 좌석, 없으면 기본 2자리(claude proposer + codex reviewer).
    let (participants, registry): (Vec<Participant>, MapRegistry) = cli_run::build_participants(
        &roster_path,
        #[cfg(feature = "mcp")]
        &db_path,
        &search_url,
        &search_token,
        #[cfg(feature = "mcp")]
        &sid,
    );

    // --core 로스터 스냅샷: participants가 session에 이동되기 전 좌석 구성을 캡처(get_roster 노출용).
    #[cfg(feature = "serve")]
    let core_roster: Option<Vec<tunaround::orchestrator::RosterSeat>> =
        core_addr.as_ref().map(|_| {
            participants
                .iter()
                .map(|p| tunaround::orchestrator::RosterSeat {
                    engine: p.engine.clone(),
                    role: p.role.clone(),
                })
                .collect()
        });

    // SQLite 인덱서 생성(feature-gated). --db 없거나 sqlite off면 None=기존 동작 불변.
    #[cfg(feature = "sqlite")]
    let indexer = cli_run::build_indexer(&db_path);
    #[cfg(not(feature = "sqlite"))]
    let indexer: Option<Box<dyn tunaround::store::indexer::MessageIndexer>> = None;

    // SQLite retriever 생성(feature-gated). indexer와 별개의 읽기 연결(WAL 동시 reader OK).
    #[cfg(feature = "sqlite")]
    let retriever = cli_run::build_retriever(&db_path);
    #[cfg(not(feature = "sqlite"))]
    let retriever: Option<Box<dyn tunaround::orchestrator::ContextRetriever>> = None;

    // 세션 초기 상태 결정(우선순위: 파일 resume > SQLite 세션 재개 > 신규).
    let resume_existing = state_path
        .as_deref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);

    let session = if resume_existing {
        // 파일에서 트리 상태를 로드해 인메모리 세션을 seed한다.
        let p = state_path.as_deref().unwrap();
        match tunaround::store::load_session(p) {
            Ok(ss) => {
                println!("(이어받음: {p})");
                let mut s = Session::new_with_indexer(
                    participants,
                    Box::new(registry),
                    sid.clone(),
                    indexer,
                );
                s.seed_from(ss);
                s
            }
            Err(e) => {
                eprintln!("[resume 실패: {e}] 종료합니다.");
                std::process::exit(1);
            }
        }
    } else if session_arg.is_some() {
        // --session <id>: SQLite 세션(--db)에서 재개. --db 없으면 새 세션(안내).
        #[cfg(feature = "sqlite")]
        {
            let mut s =
                Session::new_with_indexer(participants, Box::new(registry), sid.clone(), indexer);
            match &db_path {
                Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
                    Ok(store) => match store.load_session(&sid) {
                        Ok(Some(ss)) => {
                            println!("(SQLite 세션 재개: {sid})");
                            s.seed_from(ss.into()); // StoredSession → ConversationSnapshot(경계 변환).
                        }
                        Ok(None) => println!("(새 세션: {sid})"),
                        Err(e) => eprintln!("[session] 세션 로드 실패(새 세션으로 시작): {e}"),
                    },
                    Err(e) => eprintln!("[session] DB 열기 실패(새 세션으로 시작): {e}"),
                },
                None => eprintln!("[session] --db 없이 --session 재개 불가(무시)"),
            }
            s
        }
        #[cfg(not(feature = "sqlite"))]
        {
            eprintln!("[session] --db 없이 --session 재개 불가(무시)");
            Session::new_with_indexer(participants, Box::new(registry), sid.clone(), indexer)
        }
    } else {
        Session::new_with_indexer(participants, Box::new(registry), sid.clone(), indexer)
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
    let validity_sink = cli_run::build_validity_sink(&db_path);
    #[cfg(not(feature = "sqlite"))]
    let validity_sink: Option<Box<dyn tunaround::orchestrator::ValiditySink>> = None;

    // 큐레이션 지정 sink(--db 있으면 배선). /annotate가 message_validity의 abstraction/anchors에 쓴다.
    #[cfg(feature = "sqlite")]
    let annotation_sink = cli_run::build_annotation_sink(&db_path);
    #[cfg(not(feature = "sqlite"))]
    let annotation_sink: Option<Box<dyn tunaround::orchestrator::AnnotationSink>> = None;

    let mut session = session
        .with_retriever(retriever)
        .with_recent_turns(recent_turns)
        .with_context_mode(context_mode)
        .with_validity_sink(validity_sink)
        .with_annotation_sink(annotation_sink);

    // --core 배선(participants/session 빌드 후): seed→코어 DB 권위 반영 → HTTP MCP 서버 spawn(로스터 주입)
    //  → REPL core-sync 연결. 이 순서라야 로스터 스냅샷과 권위 트리가 일관된다.
    #[cfg(feature = "serve")]
    if let Some(addr) = core_addr.clone() {
        let db_str = db_path.clone().expect("--core는 위에서 --db를 검증함");
        // seed(파일/redis 재개)가 있으면 코어 DB에 먼저 전량 반영해 DB를 단일 권위로 만든다
        // (이후 core-sync adopt가 DB를 채택하므로 seed 유실/이드 충돌 방지).
        // 단, DB가 이미 seed보다 앞서 있으면(외부 post_turn 등) save_session의 전량 교체가 그
        // 발언들을 영구 삭제하므로, DB 권위를 보존하기 위해 덮어쓰기를 건너뛴다(결함 #1, 보수적).
        if session.message_count() > 0 {
            match tunaround::store::sqlite::SqliteStore::open(&db_str) {
                Ok(store) => {
                    let tok = cli_run::build_index_tokenizer("core");
                    // 중립 snapshot → 영속 DTO(SqliteStore::save_session은 StoredSession 유지).
                    let ss = tunaround::store::StoredSession::from(session.snapshot());
                    match store.load_session(&sid) {
                        Ok(db_ss) => {
                            if cli_run::db_has_newer_content(&ss, db_ss.as_ref()) {
                                eprintln!(
                                    "[core] DB에 더 새로운 발언이 있어 seed 덮어쓰기를 건너뜁니다"
                                );
                            } else if let Err(e) = store.save_session(&sid, &ss, |t| tok(t)) {
                                eprintln!("[core] seed 코어 DB 반영 실패: {e}");
                            }
                        }
                        Err(e) => eprintln!(
                            "[core] seed 비교용 DB 조회 실패, 안전을 위해 seed 덮어쓰기를 건너뜁니다: {e}"
                        ),
                    }
                }
                Err(e) => eprintln!("[core] seed 반영용 DB 열기 실패: {e}"),
            }
        }
        // HTTP MCP 코어 백엔드 + 전용 스레드(자체 런타임 block_on) 서빙(로스터 포함).
        let (retriever_arc, reader_arc, writer_arc, a2a_store_arc) =
            cli_run::build_http_mcp_backends("core", &db_str);
        let serve_tok = serve_token.clone();
        let addr_owned = addr.clone();
        // bind 성공/실패를 REPL 시작 전에 동기 확인하기 위한 oneshot 채널(결함 #8). 서버 스레드가
        // bind 직후(서빙 진입 전) 결과를 1회 보내고, 이후 서빙 중 종료는 기존처럼 stderr로만 알린다.
        let (bind_tx, bind_rx) = tokio::sync::oneshot::channel::<Result<(), String>>();
        // 메인 스레드는 동기 블로킹 REPL(std stdin)이라 공유 rt에 spawn하면 서버 accept 루프가
        // 유휴 중 간헐적으로만 구동된다(신뢰 불가). 전용 OS 스레드의 자체 런타임 block_on이 서버를
        // 계속 구동해 원격 클라이언트(curl·에이전트)에 안정적으로 응답한다.
        std::thread::spawn(move || {
            let srt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[core] 서버 런타임 생성 실패: {e}");
                    let _ = bind_tx.send(Err(format!("서버 런타임 생성 실패: {e}")));
                    return;
                }
            };
            srt.block_on(async move {
                // bind를 서빙 진입과 분리해, 실패를 REPL 시작 전에 채널로 알릴 수 있게 한다.
                let listener = match tokio::net::TcpListener::bind(&addr_owned).await {
                    Ok(l) => l,
                    Err(e) => {
                        eprintln!("[core] bind 실패({addr_owned}): {e}");
                        let _ = bind_tx.send(Err(format!("bind 실패({addr_owned}): {e}")));
                        return;
                    }
                };
                let _ = bind_tx.send(Ok(()));
                if let Err(e) = tunaround::mcp::serve_http_mcp_on_listener(
                    listener,
                    retriever_arc,
                    reader_arc,
                    Some(writer_arc),
                    core_roster,
                    serve_tok,
                    a2a_store_arc,
                )
                .await
                {
                    eprintln!("[core] HTTP MCP 서버 종료: {e}");
                }
            });
        });
        // REPL 시작 전 bind 결과를 동기 대기한다. 실패면 core-sync를 배선하지 않고 로컬 전용으로
        // 명시 degrade한다(결함 #8: bind 실패가 조용히 묻혀 REPL이 core-sync 상태로 진행되는 것 방지).
        let bind_ok = match bind_rx.blocking_recv() {
            Ok(Ok(())) => true,
            Ok(Err(e)) => {
                eprintln!(
                    "[core] 코어 서버 기동 실패: {e}. core-sync 없이 로컬 전용으로 계속합니다."
                );
                false
            }
            Err(_) => {
                eprintln!(
                    "[core] 코어 서버 기동 확인 실패(채널 종료). core-sync 없이 로컬 전용으로 계속합니다."
                );
                false
            }
        };
        // REPL core-sync: 코어 DB(--db)를 권위로 삼아 매 라운드 adopt + append_turn 쓰기.
        if bind_ok {
            match tunaround::store::sqlite::SqliteStore::open(&db_str) {
                Ok(store) => {
                    let core_sync = tunaround::store::retriever::SqliteCoreSync::new(
                        store,
                        cli_run::build_index_tokenizer("core"),
                    );
                    session = session.with_core_sync(Some(Box::new(core_sync)));
                }
                Err(e) => eprintln!("[core] core-sync DB 열기 실패(병합 비활성): {e}"),
            }
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
}
