// tunaRound 서브커맨드별 실행 진입과 코어 HTTP MCP 백엔드 구성을 담는 모듈. main()의 dispatch를 얇게 유지한다.

use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

/// 색인용 FTS 토크나이저 closure(fts_index: 형태소+raw). indexer/writer/reindex 공용.
#[cfg(feature = "sqlite")]
pub(crate) fn build_index_tokenizer(ctx: &str) -> Box<dyn Fn(&str) -> String + Send + Sync> {
    #[cfg(feature = "morphology")]
    {
        match tunaround::search::tokenizer::create_tokenizer("kiwi") {
            Ok(t) => Box::new(move |s: &str| t.fts_index(s)),
            Err(e) => {
                eprintln!("[{ctx}] 색인 토크나이저 실패, 폴백: {e}");
                Box::new(|s: &str| tunaround::search::fallback_fts_index(s))
            }
        }
    }
    #[cfg(not(feature = "morphology"))]
    {
        let _ = ctx;
        Box::new(|s: &str| tunaround::search::fallback_fts_index(s))
    }
}

/// 질의용 FTS 토크나이저 closure(fts_query: prefix). retriever/mcp-search/코어 공용.
#[cfg(feature = "sqlite")]
pub(crate) fn build_query_tokenizer(ctx: &str) -> Box<dyn Fn(&str) -> String + Send + Sync> {
    #[cfg(feature = "morphology")]
    {
        match tunaround::search::tokenizer::create_tokenizer("kiwi") {
            Ok(t) => Box::new(move |s: &str| t.fts_query(s)),
            Err(e) => {
                eprintln!("[{ctx}] 토크나이저 실패, 폴백: {e}");
                Box::new(|s: &str| tunaround::search::fallback_fts_query(s))
            }
        }
    }
    #[cfg(not(feature = "morphology"))]
    {
        let _ = ctx;
        Box::new(|s: &str| tunaround::search::fallback_fts_query(s))
    }
}

/// 벡터 임베더 인스턴스(semantic이면 OllamaEmbedder, 아니면 None). indexer/retriever/reindex/코어 공용.
/// 연결 실패는 best-effort(구성 자체는 무오류라 ctx 프리픽스 불요).
#[cfg(feature = "sqlite")]
pub(crate) fn build_embedder() -> Option<Box<dyn tunaround::store::embedding::Embedder>> {
    #[cfg(feature = "semantic")]
    {
        Some(Box::new(tunaround::store::embedding::OllamaEmbedder::from_env()))
    }
    #[cfg(not(feature = "semantic"))]
    {
        None
    }
}

/// build_http_mcp_backends 반환 묶음: (retriever, 전사 리더, writer, A2A store).
#[cfg(feature = "serve")]
pub(crate) type HttpMcpBackends = (
    std::sync::Arc<dyn tunaround::orchestrator::ContextRetriever>,
    Option<std::sync::Arc<dyn tunaround::orchestrator::TranscriptReader>>,
    std::sync::Arc<dyn tunaround::orchestrator::TranscriptWriter>,
    std::sync::Arc<std::sync::Mutex<tunaround::store::sqlite::SqliteStore>>,
);

/// HTTP MCP 코어용 retriever + 전사 리더 + writer를 --db 경로로 빌드한다(--serve-mcp / --core 공용).
/// serve→mcp→sqlite라 sqlite는 항상 켜짐. 실패 시 ctx 프리픽스로 에러를 찍고 종료한다.
#[cfg(feature = "serve")]
pub(crate) fn build_http_mcp_backends(ctx: &str, db_str: &str) -> HttpMcpBackends {
    let store = match tunaround::store::sqlite::SqliteStore::open(db_str) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[{ctx}] DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    // 질의용 토크나이저(fts_query: prefix) + 벡터 임베더.
    let tok = build_query_tokenizer(ctx);
    let emb = build_embedder();
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

/// --observe 모드: REPL 대신 SQLite 세션을 폴링 tail(read-only). msg_id 커서로 새 발언만 출력.
/// 반환하지 않는다(무한 폴링 루프 또는 프로세스 종료).
#[cfg(feature = "sqlite")]
pub(crate) fn run_observe(sid: String, db_path: Option<String>) {
    let Some(db) = db_path else {
        eprintln!("[observe] --db <경로> 필요");
        std::process::exit(1);
    };
    let store = match tunaround::store::sqlite::SqliteStore::open(&db) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[observe] DB 열기 실패: {e}");
            std::process::exit(1);
        }
    };
    // 활성 경로(path_to_root)만 tail한다: 세션 전체 메시지엔 /checkout 분기가 섞여 있으므로,
    // 다른 표시 로직(transcript/read_transcript)과 동일하게 head→root 활성 경로만 고른다
    // (봇 리뷰 Major: 분기 혼입 방지). 이미 출력한 발언 수를 커서로 삼아 새 발언만 흘린다.
    // 초기 스냅샷과 폴링을 한 루프로 통합해 로드 에러를 대칭 처리한다(초기 에러도 즉사 안 하고
    // 재시도, 리뷰 nit). load_session은 세션 부재를 Ok(None)으로 주므로 Err는 실제 DB 에러뿐이다.
    println!("=== observe {sid} (2초 폴링, 활성 경로) ===");
    let mut printed = 0usize;
    loop {
        match store.load_session(&sid) {
            Ok(Some(ss)) => {
                let path = tunaround::store::path_to_root(&ss.messages, ss.head);
                // /checkout으로 활성 경로가 짧아지면(분기 전환) 커서를 재동기화한다.
                if path.len() < printed {
                    printed = 0;
                }
                for u in &path[printed..] {
                    println!("{}: {}", u.speaker, u.content);
                }
                printed = path.len();
            }
            // 세션이 삭제됐다가 같은 sid로 재생성되면 커서를 초기화해 처음부터 다시 흘린다(봇 리뷰).
            Ok(None) => printed = 0,
            Err(e) => eprintln!("[observe] 세션 로드 실패(재시도): {e}"),
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

/// --reindex 모드: 모든 세션의 FTS·벡터 인덱스를 messages(SoR)에서 재생성(sqlite 피처 전용).
#[cfg(feature = "sqlite")]
pub(crate) fn run_reindex(db_path: &Option<String>) {
    let db_str = match db_path {
        Some(p) => p.clone(),
        None => { eprintln!("[reindex] --db <경로> 필요"); std::process::exit(1); }
    };
    let store = match tunaround::store::sqlite::SqliteStore::open(&db_str) {
        Ok(s) => s,
        Err(e) => { eprintln!("[reindex] DB 열기 실패: {e}"); std::process::exit(1); }
    };
    // 색인용 fts 토크나이저 + 벡터 임베더(semantic이면 재임베딩; model_id 키로 모델 교체 시 갱신).
    let tok = build_index_tokenizer("reindex");
    let emb = build_embedder();

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
}

/// --mcp-search 모드: REPL 대신 stdio MCP 검색 서버 기동(mcp 피처 전용).
#[cfg(feature = "mcp")]
pub(crate) fn run_mcp_search(
    rt: &tokio::runtime::Runtime,
    db_path: &Option<String>,
    mcp_session_id: Option<String>,
) {
    let db_str = match db_path {
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
    let tok = build_query_tokenizer("mcp-search");
    let emb = build_embedder();
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
}

/// --serve-mcp 모드: REPL 대신 HTTP MCP 서버 상주(serve 피처 전용).
#[cfg(feature = "serve")]
pub(crate) fn run_serve_mcp(
    rt: &tokio::runtime::Runtime,
    addr: &str,
    db_path: &Option<String>,
    serve_token: Option<String>,
) {
    let db_str = {
        #[cfg(feature = "sqlite")]
        {
            match db_path {
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
}

/// node 모드: node.toml대로 브로커(self)+자동 워커 레인들을 한 프로세스로 상주(serve+worker 전용).
#[cfg(all(feature = "serve", feature = "worker"))]
pub(crate) fn run_node(rt: &tokio::runtime::Runtime, a: crate::cli::NodeArgs) {
    let cfg = match tunaround::config::load_node_config(a.config.as_deref()) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[node] {e}");
            std::process::exit(1);
        }
    };
    let token = tunaround::config::resolve_node_token(cfg.token.as_deref());

    // 코어 URL 결정. core="self"면 이 프로세스가 브로커를 전용 스레드로 기동한다.
    let core_url = if cfg.core == "self" {
        let listen = cfg.listen.clone().unwrap_or_else(|| "0.0.0.0:8770".to_string());
        let db_str =
            tunaround::config::expand_home(cfg.db.as_deref().unwrap_or("~/.tunaround/broker.db"));
        // set-and-forget: 브로커 db 상위 디렉터리를 자동 생성(첫 실행 시 ~/.tunaround 없을 수 있음).
        if let Some(parent) = std::path::Path::new(&db_str).parent()
            && !parent.as_os_str().is_empty()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("[node] db 디렉터리 생성 실패 {}: {e}", parent.display());
        }
        let (retriever_arc, reader_arc, writer_arc, a2a_store_arc) =
            build_http_mcp_backends("node", &db_str);
        let tok2 = token.clone();
        let addr2 = listen.clone();
        // 메인 rt는 아래 워커 루프를 돌리므로, 브로커는 전용 OS 스레드의 자체 런타임 block_on으로
        // 계속 구동한다(Stage 3a 교훈: 공유 rt spawn은 유휴 중 신뢰불가).
        std::thread::spawn(move || {
            let srt = match tokio::runtime::Runtime::new() {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("[node] 브로커 런타임 생성 실패: {e}");
                    return;
                }
            };
            srt.block_on(async move {
                if let Err(e) = tunaround::mcp::start_http_mcp_server(
                    &addr2, retriever_arc, reader_arc, Some(writer_arc), None, tok2, a2a_store_arc,
                )
                .await
                {
                    eprintln!("[node] 브로커 종료: {e}");
                }
            });
        });
        eprintln!("[node] 브로커 기동(self) listen={listen} db={db_str}");
        tunaround::mcp::core_local_url(&listen)
    } else {
        cfg.core.clone()
    };

    // 감독 레인: 데몬화할 수 없으니(세션 부착 본질) watcher 실행 명령만 안내한다.
    // runner별로 wake 경로가 다르다: claude=세션 하네스 Monitor+poll, codex=app-server 라이브 thread에
    // codex-inject로 turn/start 주입(codex엔 Monitor가 없음, 설계 v2-37).
    for l in cfg.lane.iter().filter(|l| l.is_supervised()) {
        match l.runner.as_str() {
            "codex" => eprintln!(
                "[node] 감독 레인 '{}'(codex): app-server 라이브 감독으로 운용하세요(설계 v2-37, 관전 결정 2026-07-07).\n  \
                 1) app-server 기동(토큰 env 필수): TUNA_BROKER_TOKEN=<TOKEN> codex app-server --listen ws://127.0.0.1:<PORT>\n  \
                 2) 라이브 관전(codex 추론 실시간) = codex 네이티브 TUI 부착: codex --remote ws://127.0.0.1:<PORT>. 대시보드(/dashboard)는 전 에이전트 task 활동을 통합 로그로 보여준다(사후, 항상 켜둘 필요 없음).\n  \
                 3) 감시+주입: tunaround poll --core {} --token <TOKEN> --agent {} --on-task 'tunaround codex-inject --ws ws://127.0.0.1:<PORT> --agent {} --text \"브로커 task {{id}}를 claim_task로 가져와 요청을 읽고, 그 요청에 직접 답하라(답변 내용을 네 메시지로 출력). claim/complete는 처리 절차일 뿐이니 절차를 설명하지 말고 요청에 대한 실제 답을 내라. 그 답변 텍스트를 result로 complete_task를 호출해 마감하라\"'",
                l.agent, core_url, l.agent, l.agent
            ),
            "claude" => eprintln!(
                "[node] 감독 레인 '{}'(claude): 클로드코드 세션에서 아래를 Monitor로 실행하세요\n  tunaround poll --core {} --token <TOKEN> --agent {}",
                l.agent, core_url, l.agent
            ),
            // opencode/http/a2a 등은 감독 레인 자동 wake 메커니즘이 없다(claude=Monitor, codex=app-server만).
            other => eprintln!(
                "[node] 감독 레인 '{}'(runner={other}): 이 runner는 감독(라이브) 자동 wake를 아직 지원하지 않습니다. \
                 claude(Monitor+poll) 또는 codex(app-server+codex-inject)로 두거나, 자동 레인(kind 미지정/auto)으로 운용하세요.",
                l.agent
            ),
        }
    }

    // 자동 레인: 각 워커 루프를 동시에 상주 실행한다.
    let auto: Vec<tunaround::config::Lane> =
        cfg.lane.iter().filter(|l| !l.is_supervised()).cloned().collect();
    eprintln!("[node] 자동 레인 {}개, core={core_url}", auto.len());

    rt.block_on(async {
        let mut handles = Vec::new();
        for l in auto {
            let core_url = core_url.clone();
            let token = token.clone();
            // 각 레인은 자기 에러를 즉시 로그하고 () 로 끝낸다. run_worker_loop는 무한 루프라
            // join_all에 Result를 넘겨 사후 처리하면, 정상 레인이 영원히 안 끝나 실패 레인 에러가
            // 영영 출력되지 않는다(gemini 지적). 그래서 실패를 레인 안에서 바로 가시화한다.
            handles.push(async move {
                let run = async {
                    let runner = crate::cli_node::build_lane_runner(&l, &token)?;
                    let mode = if l.is_write() {
                        tunaround::runner::RunMode::Write
                    } else {
                        tunaround::runner::RunMode::ReadOnly
                    };
                    let project = l.project.as_deref().map(tunaround::config::expand_home);
                    // 워커 격리 가드레일(거버넌스 #5): write 레인의 작업 디렉터리가 node 실행 클론과
                    // 겹치면 자기 클론을 갈아엎어 워커가 자살한다(2026-07-03 뱃지 task). 그 레인만
                    // 거부하고(다른 레인은 계속) 별도 클론/워크트리 지정을 안내한다.
                    if l.is_write() {
                        let cwd = std::env::current_dir().unwrap_or_default();
                        let pp = project.as_deref().map(std::path::Path::new);
                        if tunaround::worker::write_lane_disrupts_node(pp, &cwd) {
                            return Err(format!(
                                "write 레인의 project({})가 node 실행 디렉터리({})와 겹칩니다. \
                                 자기 클론 갈아엎기(self-disruption)를 막기 위해 거부합니다. \
                                 별도 클론/워크트리를 project로 지정하세요.",
                                project.as_deref().unwrap_or("<미지정=cwd>"),
                                cwd.display()
                            ));
                        }
                    }
                    let context_map = match l.context_map.as_deref() {
                        Some(spec) => tunaround::worker::parse_context_map(spec)?,
                        None => std::collections::HashMap::new(),
                    };
                    // 로스터 태그 형식 검증(k=v,k=v). 잘못된 형식이면 register 전에 이 레인만 거부한다
                    // (parse_context_map fail-fast와 동일 패턴, register_agent가 쓰는 parse_tags 재사용).
                    if let Some(t) = &l.tags {
                        tunaround::store::agents::parse_tags(t)
                            .map_err(|e| format!("lane '{}' tags 형식 오류(k=v,k=v 필요): {e}", l.agent))?;
                    }
                    let client = crate::cli_node::connect_with_retry(&core_url, &token, 20).await?;
                    eprintln!("[node] 레인 '{}' 연결 OK, 폴링 시작(interval {}s)", l.agent, l.interval);
                    tunaround::worker::run_worker_loop(
                        &client,
                        runner,
                        &l.agent,
                        &l.runner, // v8 트레이스: 레인 설정의 runner 이름을 그대로 기록.
                        l.tags.clone(), // 로스터 발견용 태그(node.toml lane.tags, 셀렉터 라우팅)
                        l.model.clone(),
                        project,
                        context_map,
                        mode,
                        l.interval,
                        false,
                    )
                    .await
                };
                if let Err(e) = run.await {
                    eprintln!("[node] 레인 '{}' 종료(다른 레인은 계속): {e}", l.agent);
                }
            });
        }
        if handles.is_empty() {
            // 자동 레인 없이 브로커만 상주하는 경우에도 프로세스가 안 죽게 대기한다.
            eprintln!("[node] 자동 레인 없음(브로커만 상주).");
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
            }
        }
        // 정상 레인들은 무한히 돈다. 실패 레인은 위에서 이미 로그하고 빠졌다.
        futures_util::future::join_all(handles).await;
    });
}

/// 로스터 파일이 있으면 동적 좌석, 없으면 기본 2자리(claude proposer + codex reviewer)를 만든다.
/// db_path·sid는 러너의 MCP 검색 배선(mcp 피처)에서만 쓰여 그 피처에서만 파라미터로 받는다.
pub(crate) fn build_participants(
    roster_path: &Option<String>,
    #[cfg(feature = "mcp")] db_path: &Option<String>,
    search_url: &Option<String>,
    search_token: &Option<String>,
    #[cfg(feature = "mcp")] sid: &str,
) -> (Vec<Participant>, MapRegistry) {
    match roster_path {
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
                .with_search_session(Some(sid.to_string()))
                .with_search_url(search_url.clone(), search_token.clone());
            #[cfg(not(feature = "mcp"))]
            let claude_runner = ClaudeRunner::new()
                .with_search_url(search_url.clone(), search_token.clone());
            reg.insert("claude", Box::new(claude_runner));
            #[cfg(feature = "mcp")]
            let codex_runner = CodexRunner::new()
                .with_search_db(db_path.clone())
                .with_search_session(Some(sid.to_string()))
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
    }
}

/// SQLite 인덱서 생성(feature-gated). --db 없거나 열기 실패면 None=기존 동작 불변.
#[cfg(feature = "sqlite")]
pub(crate) fn build_indexer(
    db_path: &Option<String>,
) -> Option<Box<dyn tunaround::store::indexer::MessageIndexer>> {
    match db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => {
                let tok = build_index_tokenizer("tunaRound");
                let emb_idx = build_embedder();
                Some(Box::new(tunaround::store::indexer::SqliteIndexer::new(store, tok, emb_idx))
                    as Box<dyn tunaround::store::indexer::MessageIndexer>)
            }
            Err(e) => {
                eprintln!("[tunaRound] --db 열기 실패: {e}");
                None
            }
        },
        None => None,
    }
}

/// SQLite retriever 생성(feature-gated). indexer와 별개의 읽기 연결(WAL 동시 reader OK).
#[cfg(feature = "sqlite")]
pub(crate) fn build_retriever(
    db_path: &Option<String>,
) -> Option<Box<dyn tunaround::orchestrator::ContextRetriever>> {
    match db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => {
                let tok2 = build_query_tokenizer("tunaRound");
                let emb_ret = build_embedder();
                Some(Box::new(tunaround::store::retriever::SqliteRetriever::new(store, tok2, emb_ret))
                    as Box<dyn tunaround::orchestrator::ContextRetriever>)
            }
            Err(e) => {
                eprintln!("[tunaRound] retriever --db 열기 실패: {e}");
                None
            }
        },
        None => None,
    }
}

/// 유효성 지정 sink(--db 있으면 배선). /supersede·/reject가 message_validity에 쓴다.
#[cfg(feature = "sqlite")]
pub(crate) fn build_validity_sink(
    db_path: &Option<String>,
) -> Option<Box<dyn tunaround::orchestrator::ValiditySink>> {
    match db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => Some(Box::new(tunaround::store::retriever::SqliteValiditySink::new(store))),
            Err(e) => {
                eprintln!("[tunaRound] validity sink DB 열기 실패: {e}");
                None
            }
        },
        None => None,
    }
}

/// 큐레이션 지정 sink(--db 있으면 배선). /annotate가 message_validity의 abstraction/anchors에 쓴다.
#[cfg(feature = "sqlite")]
pub(crate) fn build_annotation_sink(
    db_path: &Option<String>,
) -> Option<Box<dyn tunaround::orchestrator::AnnotationSink>> {
    match db_path {
        Some(p) => match tunaround::store::sqlite::SqliteStore::open(p) {
            Ok(store) => Some(Box::new(tunaround::store::retriever::SqliteAnnotationSink::new(store))),
            Err(e) => {
                eprintln!("[tunaRound] annotation sink DB 열기 실패: {e}");
                None
            }
        },
        None => None,
    }
}
