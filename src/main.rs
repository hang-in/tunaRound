// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

fn main() {
    // 인자: [--roster <path>] [--observe <id>] [--session <id>] [--mcp-search] [--db <path>] [<state.json>]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut roster_path: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut observe_id: Option<String> = None;
    let mut redis_session_id: Option<String> = None;
    let mut recent_turns: Option<usize> = None;
    #[cfg(feature = "sqlite")]
    let mut db_path: Option<String> = None;
    #[cfg(feature = "mcp")]
    let mut mcp_search = false;
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
        let retriever = tunaround::store::retriever::SqliteRetriever::new(store, tok, emb);
        let retriever_arc =
            std::sync::Arc::new(retriever) as std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>;
        if let Err(e) = rt.block_on(tunaround::mcp::start_mcp_server(retriever_arc)) {
            eprintln!("[mcp-search] 서버 오류: {e}");
            std::process::exit(1);
        }
        return;
    }

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
            let reg = match tunaround::roster::build_registry(&roster, roster_search_db) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            (parts, reg)
        }
        None => {
            let mut reg = MapRegistry::new();
            #[cfg(feature = "mcp")]
            let claude_runner = ClaudeRunner::new().with_search_db(db_path.clone());
            #[cfg(not(feature = "mcp"))]
            let claude_runner = ClaudeRunner::new();
            reg.insert("claude", Box::new(claude_runner));
            #[cfg(feature = "mcp")]
            let codex_runner = CodexRunner::new().with_search_db(db_path.clone());
            #[cfg(not(feature = "mcp"))]
            let codex_runner = CodexRunner::new();
            reg.insert("codex", Box::new(codex_runner));
            let parts = vec![
                Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
                Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
            ];
            (parts, reg)
        }
    };

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

    // session_id: --session <id> 값 또는 "default".
    let sid = redis_session_id.clone().unwrap_or_else(|| "default".to_string());

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

    // retriever + recent_turns 1회 배선(session 생성 if/else 이후 단일 적용).
    let mut session = session.with_retriever(retriever).with_recent_turns(recent_turns);

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
