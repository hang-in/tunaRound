// HTTP 전송 계층과 총감독 대시보드: axum 서버·SSE·라우트·정적 자산 서빙을 담당한다.

use super::*;

/// 비-loopback 바인드에 토큰이 없으면 경고 메시지를 만든다(순수 함수, 단위테스트 대상). soft
/// enforcement 방침: 하드 거부가 아니라 기동 시 눈에 띄게 경고만 한다(프론티어 모델 지시준수 신뢰와
/// 최소놀람 원칙). loopback이거나 토큰이 있으면 None을 반환하고, 주소 파싱이 애매해 호스트를 확정
/// 못하면 오탐 방지를 위해 경고를 생략한다.
#[cfg(feature = "serve")]
fn warn_if_insecure_bind(addr: &str, has_token: bool) -> Option<String> {
    if has_token {
        return None;
    }
    // 와일드카드 표기는 파싱 없이 바로 비-loopback으로 취급한다(0.0.0.0/::/[::] 프리픽스).
    let is_non_loopback =
        if addr.starts_with("0.0.0.0") || addr.starts_with("::") || addr.starts_with("[::]") {
            true
        } else {
            // "host:port" 또는 "[ipv6]:port"에서 host만 뽑아 IpAddr로 파싱한다.
            let host = if let Some(rest) = addr.strip_prefix('[') {
                rest.split(']').next().unwrap_or("")
            } else {
                addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
            };
            match host.parse::<std::net::IpAddr>() {
                Ok(ip) => !ip.is_loopback(),
                // 파싱 불가(포트 없는 호스트명 등)는 애매하니 경고 생략(오탐 방지).
                Err(_) => false,
            }
        };
    if is_non_loopback {
        Some(format!(
            "[serve] 경고: 비-loopback({addr})에 토큰 없이 바인드 - /mcp·/a2a·대시보드 쓰기가 무인증으로 원격 노출됩니다. TUNA_BROKER_TOKEN 또는 --token 설정을 권장합니다."
        ))
    } else {
        None
    }
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
    serve_http_mcp_on_listener(
        listener, retriever, reader, writer, roster, token, a2a_store,
    )
    .await
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
    // 비-loopback 바인드 + 무토큰 = 무인증 원격 노출 가능성을 기동 시 가시화한다(soft enforcement,
    // 하드 거부 아님 - 보안 하드닝 확정 방침).
    if let Some(warning) = warn_if_insecure_bind(&bound_addr.to_string(), token.is_some()) {
        eprintln!("{warning}");
    }
    let a2a_url = core_a2a_url(&bound_addr.to_string());
    let agent_card = crate::a2a_server::build_agent_card(&a2a_url);
    // MCP inbox 툴(poll_tasks/claim_task/complete_task)도 같은 a2a_store Arc를 공유한다(새 커넥션을
    // 만들지 않고 단일 mutex로 직렬화. Phase 1 저볼륨 전제. docs/design/v2-a2a-partner-delegation_2026-07-02.md §10-1).
    let a2a_store_for_mcp = a2a_store.clone();
    // 대시보드 SSE/roster용 clone(같은 store = 같은 이벤트버스·로스터를 공유한다).
    let a2a_store_for_dash = a2a_store.clone();
    // v2-47 #3: 브로커 기동 시각을 config에 기록한다(헬스 패널 uptime 소스). 매 기동 덮어씀(프로세스별
    // uptime). serve/core/node 세 진입점이 이 함수로 수렴하므로 여기 한 곳이면 전부 커버한다. axum::serve
    // 진입(아래) 이전 동기 기록이라 첫 헬스 요청 전에 항상 존재. best-effort(실패는 로그만, 기동 막지 않음).
    {
        let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
        match store
            .now()
            .and_then(|n| store.set_config("broker_started_at", &n))
        {
            Ok(()) => {}
            Err(e) => eprintln!("[serve] 브로커 기동 시각 기록 실패(무시): {e}"),
        }
        // v2-56 기동 고아 sweep: 재기동으로 driver(인메모리)가 소멸한 토론의 열린 task를 failed로
        // 전이한다(사유=broker restart, failed terminal이 곧 watch-results 통지). 서빙 개시 전 동기
        // 실행이라 재기동 직후 시작된 새 토론의 task를 고아로 오인할 창이 없고, 방금 실패 처리된
        // task는 아래 백그라운드 backfill이 이번 기동에 색인한다.
        match store.fail_orphan_debate_tasks() {
            Ok(n) if n > 0 => eprintln!("[debate-sweep] 재기동 고아 토론 task {n}건 실패 처리"),
            Ok(_) => {}
            Err(e) => eprintln!("[debate-sweep] 고아 sweep 실패(무시): {e}"),
        }
    }
    // v2-45 P6a/P6b: 기동 housekeeping을 하나의 백그라운드 태스크로 던진다(서버 기동을 막지 않음,
    // gemini 리뷰). ① 미색인 종결 task를 mesh 기억에 백필(구 바이너리 완료분·유실 보완) → ② 그 뒤
    // 색인된 오래된 종결을 슬림화(history·completed 요청 비움, artifacts·실패사유 보존) + WAL 체크포인트.
    // 한 태스크로 묶어 백필→prune 순서를 지킨다(방금 색인된 오래된 task도 이번에 슬림화). a2a_store는
    // mutex라 라이브 핸들러와 직렬화되어 동시 실행 안전. best-effort(실패는 다음 기동 재시도).
    {
        let a2a = a2a_store.clone();
        let w = writer.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(w) = &w {
                crate::mcp::backfill_unindexed_terminal_tasks(&a2a, w);
            }
            let store = a2a.lock().unwrap_or_else(|e| e.into_inner());
            match store.prune_terminal_tasks(TERMINAL_RETAIN_DAYS) {
                Ok(n) if n > 0 => eprintln!("[retention] 오래된 종결 task {n}건 슬림화"),
                Ok(_) => {}
                Err(e) => eprintln!("[retention] 슬림화 실패(무시): {e}"),
            }
            if let Err(e) = store.wal_checkpoint() {
                eprintln!("[retention] WAL 체크포인트 실패(무시): {e}");
            }
        });
    }
    let a2a_router = crate::a2a_server::build_router(a2a_store, agent_card);

    let retriever2 = retriever.clone();
    // /dashboard/search 서브라우터 state용(MCP search_context와 같은 retriever = 형태소+FTS 재사용).
    let retriever_for_search = retriever.clone();
    let reader2 = reader.clone();
    let writer2 = writer.clone();
    let roster2 = roster.clone();
    // v2-56 mesh 토론 레지스트리: 브로커 프로세스당 1개(동시 1건 제한의 단일 소스). 요청마다 새로
    // 만드는 TunaSearchServer 인스턴스들이 같은 Arc를 공유해야 start/stop이 서로 보인다.
    let discussions = Arc::new(crate::discussion::DiscussionRegistry::new());
    // service_factory: 요청마다 새 TunaSearchServer 인스턴스를 생성한다(Clone 불필요, Arc 공유).
    let service: StreamableHttpService<TunaSearchServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                let mut s = TunaSearchServer::new(retriever2.clone())
                    .with_a2a_store(a2a_store_for_mcp.clone())
                    .with_discussions(discussions.clone());
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
    let merged = Router::new()
        .nest_service("/mcp", service)
        .merge(a2a_router);

    // 대시보드 쓰기 게이트(human-ping·deregister)용 토큰 사본: 원격 훅이 Bearer로 인증한다
    // (크로스머신 총감독, v2-43 비범위 해제 2026-07-10).
    let dash_token: Arc<Option<String>> = Arc::new(token.clone());

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
                if constant_time_eq(auth.as_bytes(), expected.as_bytes()) {
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
        .route("/dashboard/health", get(dashboard_health_handler))
        .route(
            "/dashboard/presence-timeline",
            get(dashboard_presence_timeline_handler),
        )
        .route(
            "/dashboard/goal",
            axum::routing::post(dashboard_goal_handler),
        )
        .route(
            "/dashboard/human-ping",
            axum::routing::post(dashboard_human_ping_handler),
        )
        .route(
            "/dashboard/turn-ping",
            axum::routing::post(dashboard_turn_ping_handler),
        )
        .route(
            "/dashboard/deregister",
            axum::routing::post(dashboard_deregister_handler),
        );
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
    // /dashboard/search는 store가 아니라 retriever를 state로 쓰므로 별도 서브라우터로 만들어 merge한다
    // (with_state 후 Router<()>가 되어 서로 다른 state의 라우터도 합쳐진다).
    let search_router = Router::new()
        .route("/dashboard/search", get(dashboard_search_handler))
        .with_state(retriever_for_search);
    let app = dashboard
        .with_state(a2a_store_for_dash)
        .layer(axum::Extension(dash_token))
        .merge(authed)
        .merge(search_router);

    #[cfg(feature = "dashboard")]
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드 SPA: /dashboard)");
    #[cfg(not(feature = "dashboard"))]
    eprintln!(
        "[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드: dashboard 피처 없이 빌드됨)"
    );
    // ConnectInfo(peer addr)로 /dashboard/goal의 loopback 판정을 하기 위해 connect-info make service를 쓴다.
    axum::serve(
        listener,
        app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
    )
    .await?;
    Ok(())
}

/// 종결 task 보존기간(일, v2-45 P6b). 이보다 오래된 색인 종결 task는 기동 시 슬림화된다. §5-5:
/// 보존기간 > 재생 지평선 + 피드 창(최근 N건, 현재 200)이라 슬림화가 재생·피드를 침해하지 않는다.
#[cfg(feature = "serve")]
const TERMINAL_RETAIN_DAYS: u32 = 30;

// #138 B: 도메인별 서브모듈(순수 이동). 항목들은 glob으로 이 모듈 스코프에 유지되어
// 라우터 조립·테스트(`use super::*`)의 기존 경로가 그대로 해석된다.
mod assets;
mod feed;
mod read_api;
mod write_api;

use assets::*;
use feed::*;
use read_api::*;
use write_api::*;

#[cfg(test)]
mod tests;
