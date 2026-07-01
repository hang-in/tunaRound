// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use tunaround::orchestrator::{ContextMode, MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

fn main() {
    // 인자: [--roster <path>] [--observe <id>] [--session <id>] [--mcp-search] [--db <path>] [--session-id <id>] [--pull-context] [<state.json>]
    let args: Vec<String> = std::env::args().skip(1).collect();
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
    #[cfg(feature = "sqlite")]
    let mut db_path: Option<String> = None;
    #[cfg(feature = "mcp")]
    let mut mcp_search = false;
    // --serve-mcp <addr>: HTTP MCP 서버 상주 모드(헤드리스, REPL 없음. serve 피처 전용).
    #[cfg(feature = "serve")]
    let mut serve_mcp_addr: Option<String> = None;
    // --core <addr>: front=core 단일 프로세스(REPL + in-process HTTP MCP 코어. serve 피처 전용).
    #[cfg(feature = "serve")]
    let mut core_addr: Option<String> = None;
    // --token <tok>: bearer 토큰 인증(serve 모드 전용).
    #[cfg(feature = "serve")]
    let mut serve_token: Option<String> = None;
    // --search-url <url>: 원격 HTTP MCP 서버 URL(stdio spawn 대신 접속).
    let mut search_url: Option<String> = None;
    // --search-token <tok>: HTTP MCP 서버 bearer 토큰(Authorization 헤더).
    let mut search_token: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--roster" => {
                roster_path = args.get(i + 1).cloned();
                i += 2;
            }
            "--observe" => {
                observe_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--session" => {
                redis_session_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--session-id" => {
                #[cfg(feature = "mcp")]
                { mcp_session_id = args.get(i + 1).cloned(); }
                i += 2;
            }
            "--db" => {
                #[cfg(feature = "sqlite")]
                { db_path = args.get(i + 1).cloned(); }
                i += 2;
            }
            "--recent-turns" => {
                if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<usize>().ok()) {
                    recent_turns = Some(v);
                }
                i += 2;
            }
            "--mcp-search" => {
                #[cfg(feature = "mcp")]
                { mcp_search = true; }
                i += 1;
            }
            "--serve-mcp" => {
                #[cfg(feature = "serve")]
                { serve_mcp_addr = args.get(i + 1).cloned(); }
                i += 2;
            }
            "--core" => {
                #[cfg(feature = "serve")]
                { core_addr = args.get(i + 1).cloned(); }
                i += 2;
            }
            "--token" => {
                #[cfg(feature = "serve")]
                { serve_token = args.get(i + 1).cloned(); }
                i += 2;
            }
            "--search-url" => {
                search_url = args.get(i + 1).cloned();
                i += 2;
            }
            "--search-token" => {
                search_token = args.get(i + 1).cloned();
                i += 2;
            }
            "--pull-context" => {
                pull_context = true;
                i += 1;
            }
            other => {
                if state_path.is_none() {
                    state_path = Some(other.to_string());
                }
                i += 1;
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
            let endpoint = std::env::var("TUNAROUND_OLLAMA_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:11435".to_string());
            Some(Box::new(tunaround::store::embedding::OllamaEmbedder::new(&endpoint, "bge-m3")))
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
        let (retriever_arc, reader_arc, writer_arc) = build_http_mcp_backends("serve-mcp", &db_str);
        // 헤드리스 코어: post_turn 활성(단일 writer라 클로버 없음), 로스터 없음.
        if let Err(e) = rt.block_on(tunaround::mcp::start_http_mcp_server(
            addr, retriever_arc, reader_arc, Some(writer_arc), None, serve_token.clone(),
        )) {
            eprintln!("[serve-mcp] 서버 오류: {e}");
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
                    let endpoint = std::env::var("TUNAROUND_OLLAMA_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:11435".to_string());
                    Some(Box::new(tunaround::store::embedding::OllamaEmbedder::new(
                        &endpoint, "bge-m3",
                    )))
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
                    let endpoint = std::env::var("TUNAROUND_OLLAMA_URL")
                        .unwrap_or_else(|_| "http://127.0.0.1:11435".to_string());
                    Some(Box::new(tunaround::store::embedding::OllamaEmbedder::new(
                        &endpoint, "bge-m3",
                    )))
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
        let (retriever_arc, reader_arc, writer_arc) = build_http_mcp_backends("core", &db_str);
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
                    &addr_owned, retriever_arc, reader_arc, Some(writer_arc), core_roster, serve_tok,
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

/// build_http_mcp_backends 반환 묶음: (retriever, 전사 리더, writer).
#[cfg(feature = "serve")]
type HttpMcpBackends = (
    std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>,
    Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>>,
    std::sync::Arc<dyn tunaround::orchestrator::TranscriptWriter>,
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
        let endpoint = std::env::var("TUNAROUND_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11435".to_string());
        Some(Box::new(tunaround::store::embedding::OllamaEmbedder::new(&endpoint, "bge-m3")))
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
    let retriever = tunaround::store::retriever::SqliteRetriever::new(store, tok, emb);
    let retriever_arc = std::sync::Arc::new(retriever)
        as std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>;
    let transcript_reader = tunaround::store::retriever::SqliteTranscriptReader::new(store2);
    let reader_arc: Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>> =
        Some(std::sync::Arc::new(transcript_reader));
    let writer = tunaround::store::retriever::SqliteTranscriptWriter::new(store3, build_index_tokenizer(ctx));
    let writer_arc = std::sync::Arc::new(writer)
        as std::sync::Arc<dyn tunaround::orchestrator::TranscriptWriter>;
    (retriever_arc, reader_arc, writer_arc)
}
