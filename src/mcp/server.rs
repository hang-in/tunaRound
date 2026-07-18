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
            // v2-56 기동 고아 sweep: 재기동으로 driver(인메모리)가 소멸한 토론의 열린 task를 failed로
            // 전이한다(사유=broker restart, failed terminal이 곧 watch-results 통지). backfill보다 먼저
            // 돌려 방금 실패 처리된 task도 이번 기동에 색인되게 한다.
            {
                let store = a2a.lock().unwrap_or_else(|e| e.into_inner());
                match store.fail_orphan_debate_tasks() {
                    Ok(n) if n > 0 => {
                        eprintln!("[debate-sweep] 재기동 고아 토론 task {n}건 실패 처리")
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("[debate-sweep] 고아 sweep 실패(무시): {e}"),
                }
            }
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
        Some(content) => (
            [(axum::http::header::CONTENT_TYPE, mime_for_path(path))],
            content.data.into_owned(),
        )
            .into_response(),
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

/// task 스냅샷 하나를 대시보드 SSE envelope JSON 문자열로 만든다(라이브·재생 공용, v2-45 P2 §3).
/// 매핑 = state가 completed일 때만 event="completed", 그 외(failed/canceled 포함) 전부 "status"
/// (§5-2 고정 계약). completed 상태는 try_complete/complete_task 전이(=Completed 이벤트)로만
/// 도달하므로 state 기준 재구성이 라이브 버스의 variant 기준 매핑과 일치한다. 라이브 스트림과
/// 재생 스냅샷이 이 한 함수를 공유해 두 경로의 매핑이 갈라지지 않게 한다.
#[cfg(feature = "serve")]
fn dashboard_envelope_json(task: &crate::store::a2a::Task) -> String {
    let kind = if task.state == TaskState::Completed {
        "completed"
    } else {
        "status"
    };
    let envelope = serde_json::json!({ "event": kind, "task": task });
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

/// broadcast Lagged 발생 시 흘리는 특수 신호 프레임(#2, at-least-once 취약성 완화). 정상 envelope
/// (`{"event":"status"|"completed","task":{...}}`)과 달리 "task" 필드가 없어, 이를 모르는 기존 소비자
/// (dashboard_envelope_json 파서·watch-results parse_result_line)는 조용히 무시한다(하위호환). 인지하는
/// 소비자(watch-results)만 이 프레임을 보고 워터마크 전진을 보류·재접속한다.
#[cfg(feature = "serve")]
fn lagged_signal_json(skipped: u64) -> String {
    serde_json::json!({ "event": "lagged", "skipped": skipped }).to_string()
}

/// 전역 task 이벤트를 JSON data 문자열로 흘리는 순수 스트림(단위테스트 대상). task_id 필터 없이 모든
/// TaskEvent를 내보낸다. Lagged는 조용히 스킵하지 않고 `lagged_signal_json` 프레임으로 알린 뒤 계속,
/// Closed면 종료한다(#2: 조용히 skip하면 워터마크 소비자가 갭을 인지 못해 completed/failed가 재생에서
/// 영구 누락될 수 있다 - 서버측 최소 개선안).
#[cfg(feature = "serve")]
fn dashboard_event_json_stream(
    rx: tokio::sync::broadcast::Receiver<crate::store::a2a::TaskEvent>,
) -> impl futures_util::Stream<Item = String> {
    use crate::store::a2a::TaskEvent;
    futures_util::stream::unfold(rx, |mut rx| async move {
        // unfold가 항목마다 이 클로저를 재호출하므로 여기선 한 번만 recv한다(Lagged도 신호 프레임을
        // yield하고 다음 호출에서 계속). 모든 분기가 즉시 항목/종료를 내므로 loop는 불필요하다.
        match rx.recv().await {
            Ok(ev) => {
                // Status/Completed 모두 변이 후 Task 전체 스냅샷을 담으므로 envelope 매핑은
                // state 기준 공용 헬퍼로 수렴한다(§5-2, dashboard_envelope_json 주석 참조).
                let task = match &ev {
                    TaskEvent::Status(t) | TaskEvent::Completed(t) => t,
                };
                Some((dashboard_envelope_json(task), rx))
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                eprintln!("[dashboard/events] 브로드캐스트 지연(Lagged) 감지: {n}건 건너뜀");
                Some((lagged_signal_json(n), rx))
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => None,
        }
    })
}

/// GET /dashboard/events 쿼리 파라미터(v2-45 P2 §3). axum Query 추출기는 "query" 피처(미채택 =
/// serde_urlencoded 신규 의존)라 Uri에서 직접 파싱한다.
#[cfg(feature = "serve")]
#[derive(Debug, Clone, Default, PartialEq)]
struct DashboardEventsQuery {
    /// 최근 N건 스냅샷 선행(전 상태 포함, 피드 전용). 0=현행 유지(라이브만).
    replay: usize,
    /// 이 시각(updated_at, DB datetime 포맷) 이후의 completed/failed만 선행(watch-results 재생 전용).
    since: Option<String>,
    /// since와 조합해 from_agent 필터(빈 값=전체, watch-results 의미와 일치).
    dispatcher: Option<String>,
}

/// 재생 상한(replay·since 두 경로 공통). 재생은 피드 창(최근 N건, 현재 200)용 표면이라 전 테이블 덤프
/// 수준의 N을 막는다(원격 관전자도 무인증으로 붙는 엔드포인트라 방어적 상한). since 경로는 이 상한에서
/// 잘리면 스냅샷만 보내고 스트림을 정상 종료한다(catch-up 연쇄, 핸들러 주석 참조).
#[cfg(feature = "serve")]
const DASHBOARD_REPLAY_MAX: usize = 500;

/// 종결 task 보존기간(일, v2-45 P6b). 이보다 오래된 색인 종결 task는 기동 시 슬림화된다. §5-5:
/// 보존기간 > 재생 지평선 + 피드 창(최근 N건, 현재 200)이라 슬림화가 재생·피드를 침해하지 않는다.
#[cfg(feature = "serve")]
const TERMINAL_RETAIN_DAYS: u32 = 30;

/// application/x-www-form-urlencoded 값 디코딩('+' -> 공백, %XX -> 바이트). since의
/// "YYYY-MM-DD HH:MM:SS"가 %20/+로 인코딩되어 오는 것을 원복한다. 불완전한 %시퀀스는 그대로 둔다.
#[cfg(feature = "serve")]
fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b'%' if i + 2 < bytes.len() => {
                let hi = (bytes[i + 1] as char).to_digit(16);
                let lo = (bytes[i + 2] as char).to_digit(16);
                match (hi, lo) {
                    (Some(hi), Some(lo)) => {
                        out.push((hi * 16 + lo) as u8);
                        i += 3;
                    }
                    _ => {
                        out.push(b'%');
                        i += 1;
                    }
                }
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// raw query 문자열("a=1&b=2")을 DashboardEventsQuery로 파싱한다(순수 함수, 단위테스트 대상).
/// 알 수 없는 키·파싱 불가 replay 값은 조용히 무시한다(기본 0 = 현행 라이브 전용과 동일).
#[cfg(feature = "serve")]
fn parse_dashboard_events_query(query: &str) -> DashboardEventsQuery {
    let mut q = DashboardEventsQuery::default();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value);
        match key {
            "replay" => q.replay = value.parse().unwrap_or(0).min(DASHBOARD_REPLAY_MAX),
            // 'T'→' ' 정규화(P2 리뷰 이월, §5-3 하드닝): ISO8601("...T09:00:00")이 혼입되면
            // 'T' > ' ' 사전순이라 updated_at >= since 비교가 왜곡된다. 말미 'Z'(UTC 표기)도
            // DB 포맷에 없으므로 함께 걷어낸다. 정규화 후 빈 값은 "빈 since = None" 의미 유지.
            "since" if !value.is_empty() => {
                let norm = value.replace('T', " ").trim_end_matches('Z').to_string();
                if !norm.is_empty() {
                    q.since = Some(norm);
                }
            }
            "dispatcher" if !value.is_empty() => q.dispatcher = Some(value),
            _ => {}
        }
    }
    q
}

/// GET /dashboard/events: 전역 task 이벤트 SSE(대시보드 라이브 피드). 브라우저 EventSource가 구독한다.
///
/// v2-45 P2: opt-in 쿼리 파라미터로 과거 task를 스냅샷 프레임으로 선행 재생한다(SoR = tasks 테이블,
/// §5-1). `?replay=N` = 최근 N건 전 상태(피드 전용) / `?since=TS[&dispatcher=X]` = TS 이후
/// completed/failed만(watch-results 재생 표면, P3가 소비). **둘 다 지정되면 since 우선·replay 무시**
/// (소비자가 다르다 - since는 인박스 재생 의미론이라 전 상태 스냅샷과 섞으면 계약이 흐려진다).
/// 무파라미터 = 현행 그대로 라이브만(watch-results 무파라미터 구독 회귀 없음).
/// since 스냅샷이 상한(DASHBOARD_REPLAY_MAX)에서 잘리면 라이브 없이 스냅샷만 보내고 정상 종료한다
/// (클라이언트 재접속 연쇄가 이어받음, 본문 주석 참조).
#[cfg(feature = "serve")]
async fn dashboard_events_handler(
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
    uri: axum::http::Uri,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    use futures_util::StreamExt;
    let query = parse_dashboard_events_query(uri.query().unwrap_or(""));
    let sender = {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        store.task_event_sender()
    };
    let Some(sender) = sender else {
        return (
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "task event bus 미활성",
        )
            .into_response();
    };
    // 순서 계약(§3): 스냅샷 질의보다 먼저 구독해야 질의 중 일어난 전이도 rx에 버퍼되어 유실되지
    // 않는다(a2a_server handle_subscribe_to_task의 subscribe-먼저 순서 답습). 그 대가로 스냅샷과
    // 라이브가 같은 전이를 중복 운반할 수 있는데, 소비자(피드 updatedAt 가드·watch-results seen)가
    // dedup한다.
    let rx = sender.subscribe();
    // #8: 재생 질의 실패를 빈 스냅샷(200)으로 위장하지 않는다(health 핸들러가 확립한 fail-visible
    // 패턴, PR #68). 조용히 빈 스냅샷으로 진행하면 클라이언트(watch-results)가 "재생 실패"와 "재생할
    // 것 없음"을 구분 못 해, 이어지는 라이브 이벤트가 워터마크를 전진시키면 실패한 재생 창의
    // completed/failed가 영구 유실된다. Err는 spawn_blocking 클로저 안에서 `?`로 모아 반환한다.
    let snapshot_result: Result<(Vec<String>, bool), String> = if query.since.is_some()
        || query.replay > 0
    {
        // lock은 spawn_blocking 안에서 짧게 잡는다(roster 핸들러 패턴). SSE 스트림 안에서 lock 보유 금지.
        tokio::task::spawn_blocking(move || {
            use crate::store::sqlite::ReplayLimit;
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(since) = query.since.as_deref() {
                let dispatcher = query.dispatcher.as_deref().filter(|d| !d.is_empty());
                // since 경로에도 상한 적용(P2 리뷰 이월). 방향은 Oldest - 잘려도 오래된 것부터
                // 이어받아야 클라이언트 워터마크가 앞에서부터 전진한다. 상한+1로 조회해 잘림을
                // 모호하지 않게 판정한다(정확히 상한 개수인 스냅샷을 잘림으로 오판해 불필요한
                // 재접속 연쇄를 만들지 않기 위해).
                let mut tasks = store.list_tasks_replay(
                    dispatcher,
                    Some(since),
                    &["completed", "failed"],
                    ReplayLimit::Oldest(DASHBOARD_REPLAY_MAX + 1),
                )?;
                let truncated = tasks.len() > DASHBOARD_REPLAY_MAX;
                tasks.truncate(DASHBOARD_REPLAY_MAX);
                Ok((
                    tasks.iter().map(dashboard_envelope_json).collect(),
                    truncated,
                ))
            } else {
                // replay = 전 상태 스냅샷(canceled·열린 task 포함 - 피드는 전 상태 뷰).
                // replay 값은 파서에서 이미 상한으로 클램프됨 = 잘림 판정 불요(창 뷰 의미론).
                let tasks =
                    store.list_tasks_replay(None, None, &[], ReplayLimit::Newest(query.replay))?;
                Ok((tasks.iter().map(dashboard_envelope_json).collect(), false))
            }
        })
        .await
        .unwrap_or_else(|e| Err(format!("spawn_blocking 실패: {e}")))
    } else {
        Ok((Vec::new(), false))
    };
    let (snapshot, truncated) = match snapshot_result {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[dashboard/events] 재생 스냅샷 질의 실패: {e}");
            return (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "재생 스냅샷 조회 실패",
            )
                .into_response();
        }
    };
    if truncated {
        // 잘림 = 라이브를 chain하지 않고 스냅샷만 보낸 뒤 스트림을 정상 종료한다(P2 리뷰 이월).
        // 잘린 스냅샷 뒤에 라이브를 붙이면 "상한 지점~현재" 구간의 종결이 이 접속에서 영영 안 보이는
        // 갭이 생긴다. 종료하면 클라이언트(P1 재접속 루프)가 전진한 워터마크로 즉시 재접속
        // = catch-up 연쇄로 갭 없이 따라잡는다.
        let stream = futures_util::stream::iter(snapshot).map(|data| {
            Ok::<_, std::convert::Infallible>(axum::response::sse::Event::default().data(data))
        });
        return axum::response::sse::Sse::new(stream).into_response();
    }
    let stream = futures_util::stream::iter(snapshot)
        .chain(dashboard_event_json_stream(rx))
        .map(|data| {
            Ok::<_, std::convert::Infallible>(axum::response::sse::Event::default().data(data))
        });
    axum::response::sse::Sse::new(stream)
        .keep_alive(axum::response::sse::KeepAlive::default())
        .into_response()
}

/// GET /dashboard/roster: online 감독 roster(list_agents, 빈 selector = 전체) JSON. 브라우저가 주기 폴.
/// axum "json" 피처(신규 의존) 없이 serde_json(기존 의존)만으로 application/json 응답을 만든다.
#[cfg(feature = "serve")]
async fn dashboard_roster_handler(
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
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
        human_input_at: Option<String>,
        /// 이 세션이 지금 실제로 일하는 중인지(=state=working이면서 updated_at이 신선한 열린 task의
        /// 대상). 대시보드가 presence 닷 위에 "동작 중" 스피너를 얹는 소프트 신호(스피너·신선도
        /// 게이트=이슈 #94. 과거 주석의 v2-54/v2-55 표기는 설계 문서 번호와 충돌해 이슈 번호로 정리).
        /// 조회 실패 시 false(스피너만 안 뜸).
        busy: bool,
    }
    // health 핸들러와 동일 패턴(fail-visible): spawn_blocking JoinError(내부 패닉/취소)를 빈 배열
    // 200으로 위장하지 않고 500으로 표면화한다.
    let result: Result<Vec<DashAgent>, String> = tokio::task::spawn_blocking(move || {
        use crate::mcp::format::is_busy_fresh;
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        // "동작 중" 세션 = 열린 task 중 state=working이면서 updated_at이 BUSY_FRESH_SECS(5분) 이내로
        // 갱신된 것의 to_agent 집합(이슈 #94: 갱신 없는 오래된 working=정체라 스피너 FP였다). 워커
        // 러너 heartbeat(#98)·relay 주입 heartbeat/lease(#112)가 5분 안에 updated_at을 갱신하므로
        // 실제 진행 중인 task는 이 창 안에 든다. relay 대리 claim도 to_agent가 대상 세션 uuid라 그대로
        // 매칭된다. 소프트 인디케이터라 조회 실패는 빈 집합으로 무시한다.
        let busy: std::collections::HashSet<String> = store
            .list_all_open_tasks()
            .unwrap_or_default()
            .into_iter()
            .filter(|t| is_busy_fresh(t, &now))
            .map(|t| t.to_agent)
            .collect();
        // TTL=i64::MAX로 오프라인 포함 전체 조회 후, online은 실제 TTL(AGENT_TTL_SECS)로 per-agent 계산.
        Ok(store
            .list_agents(&BTreeMap::new(), &now, i64::MAX)
            .into_iter()
            .map(|a| {
                let online =
                    crate::store::agents::is_online(&a.last_heartbeat, &now, AGENT_TTL_SECS);
                let busy = busy.contains(&a.uuid);
                DashAgent {
                    uuid: a.uuid,
                    tags: a.tags,
                    display_name: a.display_name,
                    last_heartbeat: a.last_heartbeat,
                    online,
                    human_input_at: a.human_input_at,
                    busy,
                }
            })
            .collect())
    })
    .await
    .unwrap_or_else(|e| Err(format!("spawn_blocking 실패: {e}")));
    match result {
        Ok(agents) => {
            let body = serde_json::to_vec(&agents).unwrap_or_else(|_| b"[]".to_vec());
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("[dashboard/roster] 조회 실패: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "roster 조회 실패",
            )
                .into_response()
        }
    }
}

/// GET /dashboard/health: mesh 건강 한눈 요약(read-only). 열린 task 수, 미배달(no-consumer)·고착(stuck)
/// 집계(tasks() MCP와 같은 임계 = classify_task_health 단일 소스), 머신별 presence 스캐너 도달성,
/// 브로커 uptime(기동 후 경과 초, broker_started_at config 기준), WAL 사이드카 크기(v2-47 #3).
/// uptime·WAL은 임계 없는 raw 게이지다(task-health 아님).
#[cfg(feature = "serve")]
async fn dashboard_health_handler(
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    #[derive(serde::Serialize)]
    struct ScannerHealth {
        machine: String,
        last_heartbeat: String,
        age_secs: i64,
        online: bool,
    }
    /// tasks 테이블 상태별 라이브 카운트(StatTiles 서버소스화, v2-53). working=진행 중(open)=
    /// submitted+working+input_required(목업이 진행중==열린 동일값). completed/failed는 종결 카운트.
    #[derive(serde::Serialize)]
    struct TaskCounts {
        working: usize,
        completed: usize,
        failed: usize,
    }
    #[derive(serde::Serialize)]
    struct Health {
        /// 브로커 바이너리 버전(CARGO_PKG_VERSION). 헤더 v{version} 표시용(v2-53).
        version: String,
        open_tasks: usize,
        no_consumer: usize,
        stuck: usize,
        /// tasks 테이블 상태별 라이브 카운트(StatTiles 서버소스, 리로드 안정. v2-53).
        task_counts: TaskCounts,
        scanners: Vec<ScannerHealth>,
        now: String,
        /// 브로커(serve 프로세스) 기동 후 경과 초(broker_started_at config 기준). row 부재 시 0.
        uptime_secs: i64,
        /// WAL 사이드카(`<db>-wal`) 현재 바이트. 체크포인트 직후=0(정상).
        wal_bytes: u64,
    }
    // 헬스는 "실패를 정상으로 위장하지 않는다"가 핵심(전부 0 = 정상처럼 보이는데 실은 조회 실패면 관제 오판).
    // 내부 쿼리 실패·spawn_blocking 패닉/취소는 Err로 모아 500으로 표면화 → 프론트가 "조회 실패"를 띄운다.
    let result: Result<Health, String> = tokio::task::spawn_blocking(move || {
        use crate::mcp::format::{TaskHealth, classify_task_health};
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now()?;
        let open = store.list_all_open_tasks()?;
        let (mut no_consumer, mut stuck) = (0usize, 0usize);
        for t in &open {
            match classify_task_health(t, &now) {
                TaskHealth::NoConsumer(_) => no_consumer += 1,
                TaskHealth::Stuck(_) => stuck += 1,
                TaskHealth::Ok => {}
            }
        }
        // presence 스캐너(role=infra, purpose=presence)만 선별해 머신 도달성으로 노출한다
        // (i64::MAX = 오프라인 스캐너 = 도달 불가 머신도 회색으로 보여준다).
        let mut selector = BTreeMap::new();
        selector.insert("role".to_string(), "infra".to_string());
        selector.insert("purpose".to_string(), "presence".to_string());
        let scanners = store
            .list_agents(&selector, &now, i64::MAX)
            .into_iter()
            .map(|a| {
                let age_secs = crate::store::a2a::age_secs(&now, &a.last_heartbeat).unwrap_or(-1);
                let online =
                    crate::store::agents::is_online(&a.last_heartbeat, &now, AGENT_TTL_SECS);
                let machine = a
                    .tags
                    .get("machine")
                    .cloned()
                    .unwrap_or_else(|| a.uuid.clone());
                ScannerHealth {
                    machine,
                    last_heartbeat: a.last_heartbeat,
                    age_secs,
                    online,
                }
            })
            .collect();
        // uptime = 기동 시각(config) 대비 경과. row 부재(기동 write 이전=사실상 불가)면 0.
        // row는 있으나 형식 손상(age_secs=None)이면 정상 0으로 위장하지 않고 500으로 표면화한다
        // (fail-visible: 부재만 0, 손상/조회 오류는 500). WAL 크기는 부재=체크포인트됨=0, 실 IO 오류만 500.
        let uptime_secs = match store.get_config("broker_started_at")? {
            None => 0,
            Some(started_at) => crate::store::a2a::age_secs(&now, &started_at)
                .ok_or_else(|| "broker_started_at 형식 손상".to_string())?,
        };
        let wal_bytes = store.wal_bytes()?;
        // StatTiles 서버소스: 단일 GROUP BY 질의로 상태별 카운트를 얻어 진행중/완료/실패를 도출한다
        // (피드에서 세지 않아 리로드에도 안정). fail-visible: 질의 실패는 500으로 표면화(정상 0 위장 금지).
        let by_state = store.count_by_state()?;
        let count_of = |s: &str| by_state.get(s).copied().unwrap_or(0);
        let task_counts = TaskCounts {
            working: (count_of("submitted") + count_of("working") + count_of("input_required"))
                as usize,
            completed: count_of("completed") as usize,
            failed: count_of("failed") as usize,
        };
        Ok(Health {
            version: env!("CARGO_PKG_VERSION").to_string(),
            open_tasks: open.len(),
            no_consumer,
            stuck,
            task_counts,
            scanners,
            now,
            uptime_secs,
            wal_bytes,
        })
    })
    .await
    .unwrap_or_else(|e| Err(format!("spawn_blocking 실패: {e}")));
    match result {
        Ok(health) => {
            let body = serde_json::to_vec(&health).unwrap_or_else(|_| b"{}".to_vec());
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("[dashboard/health] 조회 실패: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "health 조회 실패",
            )
                .into_response()
        }
    }
}

/// presence 타임라인 조회 상한(무인증 원격 관전자도 붙는 엔드포인트라 방어적 상한). limit 파라미터는
/// 이 값으로 클램프된다(health의 DASHBOARD_REPLAY_MAX와 같은 방어 원칙).
#[cfg(feature = "serve")]
const PRESENCE_TIMELINE_MAX: usize = 500;

/// GET /dashboard/presence-timeline?limit=&since=: presence 이벤트 이력(read-only, v2-50). 세션 등장
/// (appear)·소멸(disappear, 사유 stale|deregister)·사람입력(human_input)의 raw edge를 최신순으로 돌려준다.
/// health 핸들러 패턴(spawn_blocking + serde_json + fail-visible 500). 조회 실패를 정상 빈 배열로
/// 위장하지 않는다(관제 오판 방지). limit 기본 100·상한 PRESENCE_TIMELINE_MAX, since는 옵션(at >= since).
/// 백엔드는 raw 이벤트만 돌려주고 ★-도출(총감독 판정)은 프론트 activity.ts가 단일 소스로 유지한다.
#[cfg(feature = "serve")]
async fn dashboard_presence_timeline_handler(
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
    uri: axum::http::Uri,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // ?limit=&since= 파싱(events 핸들러와 동일: Query 추출기 미채택이라 Uri에서 직접 + percent_decode).
    let mut limit = 100usize;
    let mut since: Option<String> = None;
    for pair in uri.query().unwrap_or("").split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = percent_decode(value);
        match key {
            "limit" => {
                if let Ok(n) = value.parse::<usize>() {
                    limit = n;
                }
            }
            // events 핸들러와 동일한 'T'→' '·말미 'Z' 정규화(ISO8601 혼입 시 사전순 왜곡 방지).
            "since" if !value.is_empty() => {
                let norm = value.replace('T', " ").trim_end_matches('Z').to_string();
                if !norm.is_empty() {
                    since = Some(norm);
                }
            }
            _ => {}
        }
    }
    let limit = limit.clamp(1, PRESENCE_TIMELINE_MAX);
    // 조회 실패는 Err로 모아 500으로 표면화한다(전부 빈 배열 = 정상처럼 보이는데 실은 조회 실패면 오판).
    let result: Result<Vec<crate::store::agents::PresenceEvent>, String> =
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            store.list_presence_events(since.as_deref(), limit)
        })
        .await
        .unwrap_or_else(|e| Err(format!("spawn_blocking 실패: {e}")));
    match result {
        Ok(events) => {
            let body = serde_json::to_vec(&events).unwrap_or_else(|_| b"[]".to_vec());
            (
                axum::http::StatusCode::OK,
                [(axum::http::header::CONTENT_TYPE, "application/json")],
                body,
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("[dashboard/presence-timeline] 조회 실패: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "presence-timeline 조회 실패",
            )
                .into_response()
        }
    }
}

/// GET /dashboard/search?q=<질의>: 위임 이력 검색(read-only, v2-47 #5). P6a가 종결 task의 요청문·
/// 결과를 messages/FTS에 색인(`a2a:<task_id>` 세션, `a2a/<agent>` 화자)한 것을 MCP search_context와
/// 같은 retriever(형태소+FTS)로 검색한다. 배포 바이너리는 semantic 미포함이라 embedder 네트워크 비의존.
#[cfg(feature = "serve")]
async fn dashboard_search_handler(
    axum::extract::State(retriever): axum::extract::State<
        Arc<dyn crate::orchestrator::ContextRetriever>,
    >,
    uri: axum::http::Uri,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    #[derive(serde::Serialize)]
    struct SearchResult {
        speaker: String,
        content: String,
    }
    #[derive(serde::Serialize)]
    struct SearchResponse {
        query: String,
        results: Vec<SearchResult>,
    }
    // ?q= 파싱(events 핸들러와 동일: Query 추출기 미채택이라 Uri에서 직접 + percent_decode).
    let mut query = String::new();
    for pair in uri.query().unwrap_or("").split('&') {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        if key == "q" {
            query = percent_decode(value);
        }
    }
    let query = query.trim().to_string();
    let json = [(axum::http::header::CONTENT_TYPE, "application/json")];
    if query.is_empty() {
        let body = serde_json::to_vec(&SearchResponse {
            query,
            results: Vec::new(),
        })
        .unwrap_or_else(|_| b"{}".to_vec());
        return (axum::http::StatusCode::OK, json, body).into_response();
    }
    // retrieve는 store 락 + FTS를 도는 동기 작업이라 spawn_blocking으로 감싼다. 검색 실패(진짜 DB 장애·
    // spawn 패닉)는 "결과 없음"으로 위장하지 않고 500으로 표면화한다(헬스 핸들러와 같은 원칙).
    // 위임 이력 검색은 P6a가 색인한 a2a 화자(`a2a/<agent>`)만 노출한다 - 같은 브로커 DB에 섞인 비-a2a
    // 세션버스 전사(post_turn)까지 무인증 대시보드로 새지 않게(적대적 리뷰). 비-a2a 희석 대비 over-fetch.
    let q2 = query.clone();
    match tokio::task::spawn_blocking(move || retriever.retrieve(&q2, 60)).await {
        Ok(Ok(utterances)) => {
            let results = utterances
                .into_iter()
                .filter(|u| u.speaker.starts_with("a2a/"))
                .take(20)
                .map(|u| SearchResult {
                    speaker: u.speaker,
                    content: u.content,
                })
                .collect();
            let body = serde_json::to_vec(&SearchResponse { query, results })
                .unwrap_or_else(|_| b"{}".to_vec());
            (axum::http::StatusCode::OK, json, body).into_response()
        }
        Ok(Err(e)) => {
            eprintln!("[dashboard/search] 검색 실패: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "검색 실패").into_response()
        }
        Err(e) => {
            eprintln!("[dashboard/search] spawn_blocking 실패: {e}");
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, "검색 실패").into_response()
        }
    }
}

/// 로컬 write 엔드포인트(goal 등)의 local CSRF 방어. 브라우저가 붙이는 `Sec-Fetch-Site`가
/// `cross-site`면 다른 사이트가 유도한 요청이므로 거부한다. 헤더가 없으면(curl 등 비브라우저) 허용.
#[cfg(feature = "serve")]
fn is_cross_site(headers: &axum::http::HeaderMap) -> bool {
    matches!(
        headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        Some("cross-site")
    )
}

/// bearer 토큰을 상수시간으로 비교한다(타이밍 사이드채널 방지). 길이 노출은 허용(토큰 길이는
/// 비밀 아님) - 길이가 다르면 즉시 false, 길이가 같으면 전체를 XOR 누적해 조기반환 없이 비교한다.
#[cfg(feature = "serve")]
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// 대시보드 쓰기 게이트(human-ping·deregister): loopback은 기존대로 무조건 신뢰,
/// 원격은 Bearer 토큰 일치 시 허용(크로스머신 총감독 = 맥 세션 핑도 유효, v2-43 비범위 해제).
/// 훅(session-ping·disarm)은 이미 Authorization 헤더를 보내므로 클라이언트 변경 없음.
/// 코어가 무토큰이면 /mcp 전체가 무인증(동일 계약)이므로 원격 쓰기도 게이트하지 않는다.
#[cfg(feature = "serve")]
fn dashboard_write_allowed(
    peer_ip: std::net::IpAddr,
    headers: &axum::http::HeaderMap,
    expected_token: &Option<String>,
) -> bool {
    // to_canonical: dual-stack 소켓에서 IPv4 루프백이 ::ffff:127.0.0.1로 잡히면
    // is_loopback()이 false가 되는 것 교정(IPv4-mapped IPv6).
    if peer_ip.to_canonical().is_loopback() {
        return true;
    }
    match expected_token {
        None => true,
        Some(tok) => headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|a| constant_time_eq(a.as_bytes(), format!("Bearer {tok}").as_bytes())),
    }
}

/// POST /dashboard/human-ping: 세션의 UserPromptSubmit 훅이 "이 세션이 방금 사람 프롬프트를 받았다"를
/// 보고한다(총감독=human_input_at 최신 세션, 설계 v2-42). body = {"agent": "<세션 uuid>"}.
/// loopback 무조건 허용 + 원격은 Bearer 토큰 필요(dashboard_write_allowed). v2-45 P4: 미등록(무장 전)
/// 이어도 영속 테이블에 선기록하고 200을 준다(404 유실 창 제거 - 이후 register/스캐너가 ★를 복원).
#[cfg(feature = "serve")]
async fn dashboard_human_ping_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
    axum::Extension(dash_token): axum::Extension<Arc<Option<String>>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !dashboard_write_allowed(peer.ip(), &headers, &dash_token) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 핑은 Bearer 토큰이 필요합니다(무토큰 원격 = 관전 전용).",
        )
            .into_response();
    }
    if is_cross_site(&headers) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "cross-site 요청 거부(local CSRF 방어).",
        )
            .into_response();
    }
    #[derive(serde::Deserialize)]
    struct PingReq {
        agent: String,
    }
    let req: PingReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("잘못된 요청: {e}"),
            )
                .into_response();
        }
    };
    // 미등록이어도 영속 선기록되므로(v2-45 P4) 항상 성공이다. 반환 bool은 무시하고 200을 준다.
    tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        // now() 실패 시 빈 타임스탬프("")를 기록하면 인메모리 ★가 사전순 최소로 오염되므로 건너뛴다
        // (CodeRabbit 리뷰). 핑 수신 자체는 성공(200)이고 다음 유효 핑에 정상 기록된다.
        if let Ok(now) = store.now() {
            store.mark_human_input(&req.agent, &now);
        }
    })
    .await
    .ok();
    (axum::http::StatusCode::OK, "ok").into_response()
}

/// POST /dashboard/deregister: 세션의 SessionEnd 훅(disarm)이 "이 세션이 닫혔다"를 보고한다.
/// 로스터에서 즉시 제거해 TTL(90초) 자연소멸을 기다리지 않는다(설계 v2-43 잔존구간 제거).
/// body = {"agent": "<세션 uuid>"}. loopback 무조건 허용 + 원격은 Bearer 토큰 필요
/// (맥 세션 종료도 즉시 등록해제, dashboard_write_allowed). 미등록 uuid면 404(멱등).
#[cfg(feature = "serve")]
async fn dashboard_deregister_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
    axum::Extension(dash_token): axum::Extension<Arc<Option<String>>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !dashboard_write_allowed(peer.ip(), &headers, &dash_token) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 등록해제는 Bearer 토큰이 필요합니다(무토큰 원격 = 관전 전용).",
        )
            .into_response();
    }
    if is_cross_site(&headers) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "cross-site 요청 거부(local CSRF 방어).",
        )
            .into_response();
    }
    #[derive(serde::Deserialize)]
    struct DeregReq {
        agent: String,
    }
    let req: DeregReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("잘못된 요청: {e}"),
            )
                .into_response();
        }
    };
    let ok = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        store.deregister_agent(&req.agent)
    })
    .await
    .unwrap_or(false);
    if ok {
        (axum::http::StatusCode::OK, "ok").into_response()
    } else {
        // 미등록(이미 제거됐거나 무장 안 됨). 훅은 성패에 무관하게 통과한다.
        (
            axum::http::StatusCode::NOT_FOUND,
            "미등록 세션(이미 제거됨)",
        )
            .into_response()
    }
}

/// POST /dashboard/goal: 로컬(loopback) 총감독이 선택한 감독들에게 목표를 던진다(대상마다 1 task).
/// 원격(비-loopback)은 read-only 관전이라 403. 무인증이지만 loopback 신뢰라 로컬 한정 write.
/// body = {"text": "...", "targets": ["uuid", ...]}. 응답 = {"created":[{taskId,toAgent}], "errors":[...]}.
#[cfg(feature = "serve")]
async fn dashboard_goal_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<
        Arc<Mutex<crate::store::sqlite::SqliteStore>>,
    >,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // loopback만 목표 제출 허용. 원격은 관전 전용. to_canonical로 IPv4-mapped IPv6
    // (::ffff:127.0.0.1, dual-stack 소켓의 로컬 접속)도 loopback으로 인정한다
    // (dashboard_write_allowed와 동일 교정, 리뷰 #29).
    if !peer.ip().to_canonical().is_loopback() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 관전 모드: 목표 제출은 로컬(loopback)에서만 가능합니다.",
        )
            .into_response();
    }
    // local CSRF 방어: 다른 사이트가 유도한 cross-site POST 거부.
    if is_cross_site(&headers) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "cross-site 요청 거부(local CSRF 방어).",
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
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("잘못된 요청: {e}"),
            )
                .into_response();
        }
    };
    if req.text.trim().is_empty() || req.targets.is_empty() {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "목표(text)와 대상(targets)이 필요합니다.",
        )
            .into_response();
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
                parts: vec![Part {
                    text: Some(req.text.clone()),
                    ..Default::default()
                }],
                task_id: None,
                context_id: None,
            };
            match store.create_task_from_message("dashboard", uuid, message) {
                Ok(task) => created.push(Created {
                    task_id: task.id,
                    to_agent: task.to_agent,
                }),
                Err(e) => errors.push(format!("{uuid}: {e}")),
            }
        }
        GoalResp { created, errors }
    })
    .await
    .unwrap_or(GoalResp {
        created: vec![],
        errors: vec!["작업 실패".to_string()],
    });
    let bodyv = serde_json::to_vec(&resp).unwrap_or_else(|_| b"{}".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        bodyv,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    // 대시보드 쓰기 게이트 순수 함수 테스트(원격 peer는 리스너 통합테스트로 재현 불가라 함수 단위로 검증).
    #[cfg(feature = "serve")]
    mod dashboard_write_gate {
        use super::super::*;

        fn headers_with_auth(v: Option<&str>) -> axum::http::HeaderMap {
            let mut h = axum::http::HeaderMap::new();
            if let Some(v) = v {
                h.insert(axum::http::header::AUTHORIZATION, v.parse().unwrap());
            }
            h
        }

        #[test]
        fn loopback_always_allowed_without_token() {
            let ip: std::net::IpAddr = "127.0.0.1".parse().unwrap();
            assert!(dashboard_write_allowed(
                ip,
                &headers_with_auth(None),
                &Some("tok".into())
            ));
            // dual-stack 소켓의 IPv4-mapped IPv6 루프백도 로컬로 인정해야 한다.
            let mapped: std::net::IpAddr = "::ffff:127.0.0.1".parse().unwrap();
            assert!(dashboard_write_allowed(
                mapped,
                &headers_with_auth(None),
                &Some("tok".into())
            ));
            let v6: std::net::IpAddr = "::1".parse().unwrap();
            assert!(dashboard_write_allowed(
                v6,
                &headers_with_auth(None),
                &Some("tok".into())
            ));
        }

        #[test]
        fn remote_with_matching_bearer_allowed() {
            let ip: std::net::IpAddr = "192.168.0.9".parse().unwrap();
            let h = headers_with_auth(Some("Bearer tok"));
            assert!(dashboard_write_allowed(ip, &h, &Some("tok".into())));
        }

        #[test]
        fn remote_with_wrong_or_missing_bearer_denied() {
            let ip: std::net::IpAddr = "192.168.0.9".parse().unwrap();
            assert!(!dashboard_write_allowed(
                ip,
                &headers_with_auth(Some("Bearer nope")),
                &Some("tok".into())
            ));
            assert!(!dashboard_write_allowed(
                ip,
                &headers_with_auth(None),
                &Some("tok".into())
            ));
        }

        #[test]
        fn remote_allowed_when_core_has_no_token() {
            // 무토큰 코어는 /mcp 전체가 무인증(동일 계약)이라 대시보드 쓰기도 게이트하지 않는다.
            let ip: std::net::IpAddr = "192.168.0.9".parse().unwrap();
            assert!(dashboard_write_allowed(ip, &headers_with_auth(None), &None));
        }
    }

    // /dashboard/goal 핸들러의 loopback 게이트(불변식 1: 원격=read-only, 제어=loopback만) 직접 호출 검증.
    // ConnectInfo(SocketAddr)를 조작해 핸들러 함수를 라우터 없이 직접 구동한다.
    #[cfg(feature = "serve")]
    mod dashboard_goal_gate {
        use super::super::*;

        fn test_store() -> Arc<Mutex<crate::store::sqlite::SqliteStore>> {
            Arc::new(Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory().expect("in-memory sqlite"),
            ))
        }

        fn valid_body() -> axum::body::Bytes {
            axum::body::Bytes::from(
                serde_json::json!({"text": "테스트 목표", "targets": ["target-uuid"]}).to_string(),
            )
        }

        async fn read_body(resp: axum::response::Response) -> (axum::http::StatusCode, String) {
            let status = resp.status();
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .expect("본문 읽기");
            (status, String::from_utf8_lossy(&bytes).to_string())
        }

        #[tokio::test]
        async fn loopback_peer_is_allowed_and_creates_task() {
            let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let store = test_store();
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                axum::http::HeaderMap::new(),
                valid_body(),
            )
            .await;
            let (status, body) = read_body(resp).await;
            assert_eq!(
                status,
                axum::http::StatusCode::OK,
                "loopback은 허용돼야 함: {body}"
            );
            assert!(
                body.contains("\"taskId\"") && body.contains("target-uuid"),
                "task가 생성돼야 함: {body}"
            );
        }

        #[tokio::test]
        async fn non_loopback_peer_is_forbidden() {
            let addr: std::net::SocketAddr = "203.0.113.5:9".parse().unwrap();
            let store = test_store();
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                axum::http::HeaderMap::new(),
                valid_body(),
            )
            .await;
            let (status, _body) = read_body(resp).await;
            assert_eq!(
                status,
                axum::http::StatusCode::FORBIDDEN,
                "원격 peer는 목표 제출이 거부돼야 함"
            );
        }

        #[tokio::test]
        async fn ipv4_mapped_ipv6_loopback_is_accepted_as_local() {
            // dashboard_write_allowed(human-ping/deregister)와 동일하게 goal 핸들러도 to_canonical()로
            // ::ffff:127.0.0.1(dual-stack 소켓의 로컬 접속)을 loopback으로 인정해야 한다(리뷰 #29 수정).
            // to_canonical 없이는 loopback으로 안 잡히는 것을 대조군으로 확인한 뒤, 핸들러가 이를 로컬로
            // 받아 403이 아님을(제출 진행) 검증한다.
            let addr: std::net::SocketAddr = "[::ffff:127.0.0.1]:9".parse().unwrap();
            assert!(
                addr.ip().to_canonical().is_loopback() && !addr.ip().is_loopback(),
                "IPv4-mapped IPv6는 to_canonical로만 loopback으로 잡힌다(전제)"
            );
            let store = test_store();
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                axum::http::HeaderMap::new(),
                valid_body(),
            )
            .await;
            let (status, _body) = read_body(resp).await;
            assert_ne!(
                status,
                axum::http::StatusCode::FORBIDDEN,
                "IPv4-mapped IPv6 loopback은 로컬로 인정돼 403이 아니어야 함(리뷰 #29)"
            );
        }

        #[tokio::test]
        async fn cross_site_header_is_forbidden_even_from_loopback() {
            let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let store = test_store();
            let mut headers = axum::http::HeaderMap::new();
            headers.insert("sec-fetch-site", "cross-site".parse().unwrap());
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                headers,
                valid_body(),
            )
            .await;
            let (status, _body) = read_body(resp).await;
            assert_eq!(
                status,
                axum::http::StatusCode::FORBIDDEN,
                "cross-site 요청은 loopback이어도 CSRF 방어로 거부돼야 함"
            );
        }

        #[tokio::test]
        async fn malformed_json_body_is_bad_request() {
            let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let store = test_store();
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                axum::http::HeaderMap::new(),
                axum::body::Bytes::from("이건 JSON이 아님"),
            )
            .await;
            let (status, _body) = read_body(resp).await;
            assert_eq!(status, axum::http::StatusCode::BAD_REQUEST);
        }

        #[tokio::test]
        async fn empty_text_or_targets_is_bad_request() {
            let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let store = test_store();
            let empty_text = axum::body::Bytes::from(
                serde_json::json!({"text": "   ", "targets": ["x"]}).to_string(),
            );
            let resp = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store.clone()),
                axum::http::HeaderMap::new(),
                empty_text,
            )
            .await;
            let (status, _) = read_body(resp).await;
            assert_eq!(
                status,
                axum::http::StatusCode::BAD_REQUEST,
                "공백 text는 거부"
            );

            let empty_targets = axum::body::Bytes::from(
                serde_json::json!({"text": "목표", "targets": []}).to_string(),
            );
            let resp2 = dashboard_goal_handler(
                axum::extract::ConnectInfo(addr),
                axum::extract::State(store),
                axum::http::HeaderMap::new(),
                empty_targets,
            )
            .await;
            let (status2, _) = read_body(resp2).await;
            assert_eq!(
                status2,
                axum::http::StatusCode::BAD_REQUEST,
                "빈 targets는 거부"
            );
        }
    }

    // /dashboard/search의 a2a/ 화자 스코프 필터(비-a2a 세션버스 전사가 무인증 대시보드로 새지 않게)와
    // take(20) 상한·retrieve Err의 500 표면화를 실제 HTTP 왕복으로 검증한다.
    #[cfg(feature = "serve")]
    mod dashboard_search_scope {
        use super::super::*;

        /// 고정 결과(또는 에러)를 내는 가짜 retriever. query는 무시한다(필터·상한 검증에만 집중).
        enum FakeRetriever {
            Fixed(Vec<crate::orchestrator::Utterance>),
            Err(String),
        }
        impl crate::orchestrator::ContextRetriever for FakeRetriever {
            fn retrieve(
                &self,
                _q: &str,
                _limit: usize,
            ) -> Result<Vec<crate::orchestrator::Utterance>, String> {
                match self {
                    FakeRetriever::Fixed(v) => Ok(v.clone()),
                    FakeRetriever::Err(e) => Err(e.clone()),
                }
            }
        }

        fn utter(speaker: &str, content: &str) -> crate::orchestrator::Utterance {
            crate::orchestrator::Utterance {
                speaker: speaker.to_string(),
                content: content.to_string(),
                abstraction: None,
            }
        }

        fn test_a2a_store() -> Arc<Mutex<crate::store::sqlite::SqliteStore>> {
            Arc::new(Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory().expect("in-memory sqlite"),
            ))
        }

        async fn spawn_search_server(retriever: FakeRetriever) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();
            let retriever = Arc::new(retriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    test_a2a_store(),
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;
            format!("http://127.0.0.1:{port}")
        }

        #[tokio::test]
        async fn only_a2a_prefixed_speakers_survive_the_scope_filter() {
            let retriever = FakeRetriever::Fixed(vec![
                utter("a2a/win-claude", "위임 내용 하나"),
                utter("claude/proposer", "비-a2a 세션버스 발언(새면 안 됨)"),
                utter("a2a/mac-claude", "위임 내용 둘"),
                utter("codex/reviewer", "비-a2a 발언 둘(새면 안 됨)"),
            ]);
            let base = spawn_search_server(retriever).await;
            let resp = reqwest::get(format!("{base}/dashboard/search?q=위임"))
                .await
                .expect("search get");
            assert_eq!(resp.status(), 200);
            let body: serde_json::Value = resp.json().await.expect("json");
            let results = body["results"].as_array().expect("results 배열");
            assert_eq!(results.len(), 2, "a2a/ 화자 둘만 남아야 함: {results:?}");
            for r in results {
                let speaker = r["speaker"].as_str().unwrap_or("");
                assert!(
                    speaker.starts_with("a2a/"),
                    "비-a2a 화자가 새면 안 됨: {speaker}"
                );
            }
        }

        #[tokio::test]
        async fn results_are_capped_at_twenty() {
            let items: Vec<_> = (0..25)
                .map(|i| utter("a2a/win-claude", &format!("항목{i}")))
                .collect();
            let base = spawn_search_server(FakeRetriever::Fixed(items)).await;
            let resp = reqwest::get(format!("{base}/dashboard/search?q=항목"))
                .await
                .expect("search get");
            assert_eq!(resp.status(), 200);
            let body: serde_json::Value = resp.json().await.expect("json");
            let results = body["results"].as_array().expect("results 배열");
            assert_eq!(results.len(), 20, "25건 중 20건으로 잘려야 함");
        }

        #[tokio::test]
        async fn retrieve_error_surfaces_as_500() {
            let base = spawn_search_server(FakeRetriever::Err("db 장애".to_string())).await;
            let resp = reqwest::get(format!("{base}/dashboard/search?q=아무거나"))
                .await
                .expect("search get");
            assert_eq!(
                resp.status(),
                500,
                "검색 실패는 빈 결과로 위장하지 않고 500이어야 함"
            );
        }
    }

    // 비-loopback+무토큰 경고(soft enforcement) 순수 함수 테스트.
    #[cfg(feature = "serve")]
    mod insecure_bind_warning {
        use super::super::*;

        #[test]
        fn wildcard_without_token_warns() {
            assert!(warn_if_insecure_bind("0.0.0.0:8770", false).is_some());
        }

        #[test]
        fn loopback_without_token_is_silent() {
            assert!(warn_if_insecure_bind("127.0.0.1:8770", false).is_none());
        }

        #[test]
        fn wildcard_with_token_is_silent() {
            assert!(warn_if_insecure_bind("0.0.0.0:8770", true).is_none());
        }

        #[test]
        fn ipv6_wildcard_without_token_warns() {
            assert!(warn_if_insecure_bind("[::]:8770", false).is_some());
        }

        #[test]
        fn ipv6_loopback_without_token_is_silent() {
            assert!(warn_if_insecure_bind("[::1]:8770", false).is_none());
        }

        #[test]
        fn unparseable_host_is_silent_by_conservative_design() {
            // 포트 없는/애매한 문자열은 오탐 방지를 위해 경고를 생략한다.
            assert!(warn_if_insecure_bind("localhost", false).is_none());
        }
    }

    // bearer 토큰 상수시간 비교 순수 함수 테스트(타이밍 사이드채널 방지).
    #[cfg(feature = "serve")]
    mod constant_time_compare {
        use super::super::*;

        #[test]
        fn equal_bytes_match() {
            assert!(constant_time_eq(b"Bearer abc123", b"Bearer abc123"));
        }

        #[test]
        fn different_bytes_do_not_match() {
            assert!(!constant_time_eq(b"Bearer abc123", b"Bearer xyz999"));
        }

        #[test]
        fn different_length_does_not_match() {
            assert!(!constant_time_eq(b"Bearer abc", b"Bearer abc123"));
        }
    }

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
                    .map(|(s, c)| crate::orchestrator::Utterance {
                        speaker: s.clone(),
                        content: c.clone(),
                        abstraction: None,
                    })
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
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();

            let log = SharedLog::default();
            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let reader =
                Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptReader>);
            let writer =
                Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptWriter>);
            let roster = Some(vec![
                RosterSeat {
                    engine: "claude".into(),
                    role: Some("proposer".into()),
                },
                RosterSeat {
                    engine: "codex".into(),
                    role: Some("reviewer".into()),
                },
            ]);
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    reader,
                    writer,
                    roster,
                    None,
                    test_a2a_store(),
                )
                .await;
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
            assert!(
                roster_text.contains("claude (proposer)"),
                "get_roster 응답: {roster_text}"
            );

            // post_turn → 추가됨.
            let post_text = post(call_body(
                3,
                "post_turn",
                r#"{"speaker":"remote/agent","content":"원격 발언 핵심어 살구"}"#,
            ))
            .await;
            assert!(post_text.contains("msg_id="), "post_turn 응답: {post_text}");

            // read_transcript → 방금 post한 발언이 보임(쓰기→읽기 일관).
            let read_text = post(call_body(4, "read_transcript", "{}")).await;
            assert!(
                read_text.contains("살구"),
                "read_transcript에 post_turn 내용 없음: {read_text}"
            );

            // GET /dashboard/search → 별도 state(retriever) 서브라우터 merge 배선 검증
            // (NullRetriever = 빈 결과, 200). 라우터 merge가 깨지면 여기서 404가 잡힌다.
            let search = reqwest::get(format!("http://127.0.0.1:{port}/dashboard/search?q=test"))
                .await
                .expect("search get");
            assert_eq!(search.status(), 200);
            let search_body = search.text().await.expect("search text");
            assert!(
                search_body.contains("\"results\":[]"),
                "search 응답: {search_body}"
            );
        }

        /// HTTP MCP로 poll_tasks→claim_task→complete_task 왕복을 검증한다. Task 2(a2a_server)가 만든
        /// a2a_store Arc를 serve_http_mcp_on_listener가 TunaSearchServer와 실제로 공유하는지까지 확인한다.
        #[tokio::test]
        async fn http_poll_claim_complete_task_e2e() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
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

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    store_for_server,
                )
                .await;
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
            assert!(
                poll_text.contains(&seeded_id),
                "poll_tasks 응답에 task_id 없음: {poll_text}"
            );

            // claim_task → working 전이.
            let claim_body = format!(r#"{{"task_id":"{seeded_id}"}}"#);
            let claim_text = post(call_body(3, "claim_task", &claim_body)).await;
            assert!(
                claim_text.contains("state=working"),
                "claim_task 응답: {claim_text}"
            );

            // complete_task → completed 전이 + artifact 저장.
            let complete_body = format!(r#"{{"task_id":"{seeded_id}","result":"작업 결과 요약"}}"#);
            let complete_text = post(call_body(4, "complete_task", &complete_body)).await;
            assert!(
                complete_text.contains("state=completed"),
                "complete_task 응답: {complete_text}"
            );

            // DB 상태 최종 확인(HTTP 왕복 후 실제로 반영됐는지. serve_http_mcp_on_listener가 넘겨받은
            // 그 a2a_store Arc가 TunaSearchServer 쪽에도 공유됐다는 증거).
            let final_task = store
                .lock()
                .unwrap()
                .get_task(&seeded_id)
                .unwrap()
                .expect("존재해야 함");
            assert_eq!(final_task.state, TaskState::Completed);
            assert_eq!(final_task.artifacts.len(), 1);
            assert_eq!(
                final_task.artifacts[0].parts[0].text.as_deref(),
                Some("작업 결과 요약")
            );
        }

        /// v2-45 P2: ?replay=N이 과거 task 스냅샷 프레임(전 상태, updated_at 오름차순)을 라이브
        /// 스트림보다 먼저 내보내는지 HTTP 레벨로 검증한다(subscribe-먼저 + chain 배선 확인).
        #[tokio::test]
        async fn dashboard_events_replay_sends_snapshot_frames_before_live() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();

            // 이벤트 버스 활성 store에 종결·취소 task를 미리 심는다(재기동 후 피드 리로드 시나리오).
            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ));
            {
                let s = store.lock().unwrap();
                let mut done = crate::store::a2a::Task::new(
                    "done-task",
                    None,
                    "win",
                    "mac",
                    "2026-07-11 09:00:00",
                );
                done.state = TaskState::Completed;
                s.create_task(&done).unwrap();
                let mut gone = crate::store::a2a::Task::new(
                    "gone-task",
                    None,
                    "win",
                    "mac",
                    "2026-07-11 09:01:00",
                );
                gone.state = TaskState::Canceled;
                s.create_task(&gone).unwrap();
            }

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    store_for_server,
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let resp = reqwest::get(format!(
                "http://127.0.0.1:{port}/dashboard/events?replay=10"
            ))
            .await
            .expect("SSE 접속 실패");
            assert_eq!(resp.status(), 200);

            // 스냅샷 2프레임이 접속 직후(라이브 이벤트 없이) 도착해야 한다. SSE 이벤트는 "\n\n"으로
            // 끝나므로, 청크 경계에서 잘린 미완 프레임은 세지 않는다(마지막 조각 제외).
            fn complete_data_frames(body: &str) -> Vec<&str> {
                let mut parts: Vec<&str> = body.split("\n\n").collect();
                parts.pop(); // 마지막 조각은 아직 미완일 수 있다.
                parts
                    .into_iter()
                    .filter_map(|p| p.trim().strip_prefix("data: "))
                    .collect()
            }
            let mut resp = resp;
            let mut body = String::new();
            while complete_data_frames(&body).len() < 2 {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("스냅샷 프레임 타임아웃")
                    .expect("chunk 읽기 실패")
                    .expect("스트림 조기 종료");
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
            // 순서 = updated_at 오름차순(done 09:00 < gone 09:01) + envelope 매핑(§5-2):
            // completed만 "completed", canceled는 "status".
            let frames: Vec<serde_json::Value> = complete_data_frames(&body)
                .into_iter()
                .map(|d| serde_json::from_str(d).expect("SSE data JSON 파싱 실패"))
                .collect();
            assert_eq!(frames.len(), 2, "스냅샷은 task당 최종 상태 1프레임: {body}");
            assert_eq!(frames[0]["event"], "completed");
            assert_eq!(frames[0]["task"]["id"], "done-task");
            assert_eq!(frames[1]["event"], "status");
            assert_eq!(frames[1]["task"]["state"], "canceled");

            // 스냅샷 뒤로 라이브 스트림이 이어진다(chain): 새 task 생성(Status emit 경로)이 같은
            // 접속에 도착.
            {
                let s = store.lock().unwrap();
                let msg = crate::store::a2a::Message {
                    message_id: "m-live".into(),
                    role: "user".into(),
                    parts: vec![],
                    task_id: None,
                    context_id: None,
                };
                s.create_task_from_message("win", "live-target", msg)
                    .unwrap();
            }
            while !body.contains("live-target") {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("라이브 프레임 타임아웃")
                    .expect("chunk 읽기 실패")
                    .expect("스트림 조기 종료");
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
        }

        /// v2-45 P3(P2 리뷰 이월): since 스냅샷이 상한(DASHBOARD_REPLAY_MAX)에서 잘리면 라이브를
        /// chain하지 않고 스냅샷만 보낸 뒤 스트림을 정상 종료해야 한다. 이어서 클라이언트가 전진한
        /// 워터마크로 재접속하면(P1 재접속 루프) 나머지가 오고 라이브까지 chain된다
        /// = catch-up 연쇄 전체를 HTTP 레벨로 검증.
        #[tokio::test]
        async fn dashboard_events_since_truncation_ends_stream_then_catchup_chains() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();

            // 상한+1건의 completed task를 초 단위로 구분된 updated_at으로 심는다(오래된 순 t-0000..).
            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ));
            {
                let s = store.lock().unwrap();
                for i in 0..=DASHBOARD_REPLAY_MAX {
                    let ts = format!("2026-07-11 09:{:02}:{:02}", i / 60, i % 60);
                    let mut t =
                        crate::store::a2a::Task::new(format!("t-{i:04}"), None, "win", "mac", ts);
                    t.state = TaskState::Completed;
                    s.create_task(&t).unwrap();
                }
            }

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    store_for_server,
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            fn complete_data_frames(body: &str) -> Vec<&str> {
                let mut parts: Vec<&str> = body.split("\n\n").collect();
                parts.pop(); // 마지막 조각은 아직 미완일 수 있다.
                parts
                    .into_iter()
                    .filter_map(|p| p.trim().strip_prefix("data: "))
                    .collect()
            }

            // 1차 접속: 전 구간 since → 상한 초과라 잘림 = 정확히 상한 개수 프레임 후 EOF(정상 종료).
            let mut resp = reqwest::get(format!(
                "http://127.0.0.1:{port}/dashboard/events?since=2026-07-11%2009:00:00&dispatcher=win"
            ))
            .await
            .expect("SSE 접속 실패");
            assert_eq!(resp.status(), 200);
            let mut body = String::new();
            loop {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("잘림 스트림이 종료되지 않음(라이브 chain 잔존 의심)")
                    .expect("chunk 읽기 실패");
                let Some(chunk) = chunk else { break }; // EOF = 서버가 정상 종료함
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
            let frames: Vec<serde_json::Value> = complete_data_frames(&body)
                .into_iter()
                .map(|d| serde_json::from_str(d).expect("SSE data JSON 파싱 실패"))
                .collect();
            assert_eq!(
                frames.len(),
                DASHBOARD_REPLAY_MAX,
                "잘린 스냅샷 = 정확히 상한 개수"
            );
            assert_eq!(
                frames[0]["task"]["id"], "t-0000",
                "Oldest 방향 = 오래된 것부터"
            );
            let last = &frames[frames.len() - 1];
            assert_eq!(
                last["task"]["id"],
                format!("t-{:04}", DASHBOARD_REPLAY_MAX - 1)
            );
            let watermark = last["task"]["updatedAt"]
                .as_str()
                .expect("updatedAt 필요")
                .to_string();

            // 2차 접속(전진한 워터마크): >= 경계라 마지막 1건 재전달 + 나머지 1건, 잘림 아님
            // → 라이브 chain 생존(스냅샷 뒤 라이브 이벤트 도착).
            let mut resp = reqwest::get(format!(
                "http://127.0.0.1:{port}/dashboard/events?since={}&dispatcher=win",
                watermark.replace(' ', "%20")
            ))
            .await
            .expect("2차 SSE 접속 실패");
            assert_eq!(resp.status(), 200);
            let mut body = String::new();
            while complete_data_frames(&body).len() < 2 {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("catch-up 프레임 타임아웃")
                    .expect("chunk 읽기 실패")
                    .expect("스트림 조기 종료(잘림 아니면 라이브 chain이어야 함)");
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
            let frames: Vec<serde_json::Value> = complete_data_frames(&body)
                .into_iter()
                .map(|d| serde_json::from_str(d).expect("SSE data JSON 파싱 실패"))
                .collect();
            assert_eq!(
                frames[0]["task"]["id"],
                format!("t-{:04}", DASHBOARD_REPLAY_MAX - 1),
                "경계(>=) 재전달 - 클라이언트 seen이 dedup할 몫"
            );
            assert_eq!(
                frames[1]["task"]["id"],
                format!("t-{:04}", DASHBOARD_REPLAY_MAX)
            );

            // 라이브 chain 확인: 새 task 생성 이벤트가 같은 접속에 도착.
            {
                let s = store.lock().unwrap();
                let msg = crate::store::a2a::Message {
                    message_id: "m-live".into(),
                    role: "user".into(),
                    parts: vec![],
                    task_id: None,
                    context_id: None,
                };
                s.create_task_from_message("win", "live-after-catchup", msg)
                    .unwrap();
            }
            while !body.contains("live-after-catchup") {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("라이브 프레임 타임아웃")
                    .expect("chunk 읽기 실패")
                    .expect("스트림 조기 종료");
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
        }

        /// 무파라미터 구독은 현행 그대로 라이브 전용이어야 한다(watch-results 재기동 시 과거 재통지
        /// 회귀 금지 - 설계 §4 P2 항목 5).
        #[tokio::test]
        async fn dashboard_events_without_params_sends_no_snapshot() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();

            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ));
            {
                let s = store.lock().unwrap();
                let mut done = crate::store::a2a::Task::new(
                    "done-task",
                    None,
                    "win",
                    "mac",
                    "2026-07-11 09:00:00",
                );
                done.state = TaskState::Completed;
                s.create_task(&done).unwrap();
            }

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    store_for_server,
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let mut resp = reqwest::get(format!("http://127.0.0.1:{port}/dashboard/events"))
                .await
                .expect("SSE 접속 실패");
            assert_eq!(resp.status(), 200);

            // 라이브 이벤트를 하나 흘려 첫 도착 프레임이 (스냅샷이 아니라) 그 이벤트인지 확인한다.
            {
                let s = store.lock().unwrap();
                let msg = crate::store::a2a::Message {
                    message_id: "m-live".into(),
                    role: "user".into(),
                    parts: vec![],
                    task_id: None,
                    context_id: None,
                };
                s.create_task_from_message("win", "live-target", msg)
                    .unwrap();
            }
            let mut body = String::new();
            while !body.contains("live-target") {
                let chunk = tokio::time::timeout(std::time::Duration::from_secs(5), resp.chunk())
                    .await
                    .expect("라이브 프레임 타임아웃")
                    .expect("chunk 읽기 실패")
                    .expect("스트림 조기 종료");
                body.push_str(&String::from_utf8_lossy(&chunk));
            }
            assert!(
                !body.contains("done-task"),
                "무파라미터 구독에 과거 task가 재생되면 안 됨(회귀): {body}"
            );
        }

        #[tokio::test]
        async fn dashboard_presence_timeline_returns_events() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();

            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory().expect("in-memory sqlite"),
            ));
            let up = |uuid: &str, runner: &str, project: Option<&str>, name: Option<&str>| {
                crate::store::agents::PresenceUpsert {
                    uuid: uuid.into(),
                    runner: runner.into(),
                    project: project.map(str::to_string),
                    display_name: name.map(str::to_string),
                    human_input_at: None,
                }
            };
            {
                let s = store.lock().unwrap();
                // s1, s2 등장 → s1 사람입력(claude ping) → s2 소멸(stale).
                s.sync_presence(
                    "win",
                    &[
                        up(
                            "s1",
                            "claude",
                            Some("tunaRound"),
                            Some("win-claude-tunaRound"),
                        ),
                        up("s2", "codex", None, None),
                    ],
                    "2026-07-12 10:00:00",
                );
                s.mark_human_input("s1", "2026-07-12 10:00:05");
                s.sync_presence(
                    "win",
                    &[up(
                        "s1",
                        "claude",
                        Some("tunaRound"),
                        Some("win-claude-tunaRound"),
                    )],
                    "2026-07-12 10:00:15",
                );
            }

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    store_for_server,
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let resp = reqwest::get(format!(
                "http://127.0.0.1:{port}/dashboard/presence-timeline"
            ))
            .await
            .expect("presence-timeline 접속 실패");
            assert_eq!(resp.status(), 200);
            let body = resp.text().await.expect("본문");
            let events: Vec<serde_json::Value> = serde_json::from_str(&body).expect("JSON 배열");
            let types: Vec<&str> = events
                .iter()
                .filter_map(|e| e["event_type"].as_str())
                .collect();
            // appear(s1,s2) + human_input(s1) + disappear(s2 stale) = 4건.
            assert_eq!(events.len(), 4, "이벤트 4건이어야: {body}");
            assert!(types.contains(&"appear"));
            assert!(types.contains(&"human_input"));
            assert!(types.contains(&"disappear"));
            // 최신순(at DESC): 마지막 이벤트(disappear s2 @10:00:15)가 배열 맨 앞.
            assert_eq!(events[0]["event_type"].as_str(), Some("disappear"));
            assert_eq!(events[0]["agent_uuid"].as_str(), Some("s2"));
            assert_eq!(events[0]["detail"].as_str(), Some("stale"));

            // limit 상한 반영.
            let resp2 = reqwest::get(format!(
                "http://127.0.0.1:{port}/dashboard/presence-timeline?limit=1"
            ))
            .await
            .expect("limit 접속 실패");
            assert_eq!(resp2.status(), 200);
            let ev2: Vec<serde_json::Value> =
                serde_json::from_str(&resp2.text().await.expect("본문2")).expect("JSON2");
            assert_eq!(ev2.len(), 1, "limit=1은 최신 1건만");
        }

        #[test]
        fn core_local_url_maps_wildcards_to_loopback() {
            // 와일드카드 host는 loopback으로, 일반 host는 그대로.
            assert_eq!(core_local_url("0.0.0.0:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("[::]:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(
                core_local_url("127.0.0.1:8771"),
                "http://127.0.0.1:8771/mcp"
            );
            assert_eq!(
                core_local_url("192.0.2.20:9000"),
                "http://192.0.2.20:9000/mcp"
            );
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
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let token = Some("secret-tok".to_string());

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    token,
                    test_a2a_store(),
                )
                .await;
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
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    None,
                    test_a2a_store(),
                )
                .await;
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

    // 대시보드 전역 SSE 순수 스트림: Status/Completed 이벤트를 필터 없이 순서대로 JSON으로 내보내는지 검증한다.
    #[cfg(feature = "serve")]
    #[tokio::test]
    async fn dashboard_event_json_stream_emits_status_then_completed() {
        use crate::store::a2a::{Task, TaskEvent};
        use futures_util::StreamExt;

        let (tx, rx) = tokio::sync::broadcast::channel::<TaskEvent>(16);
        let stream = dashboard_event_json_stream(rx);
        futures_util::pin_mut!(stream);

        let task_a = Task::new(
            "task-a",
            None,
            "win-claude",
            "mac-claude",
            "2026-07-06 10:00:00",
        );
        let mut task_b = Task::new(
            "task-b",
            None,
            "win-claude",
            "mac-codex",
            "2026-07-06 10:01:00",
        );
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

    // #2 회귀 방지: Lagged를 조용히 skip하지 않고 신호 프레임으로 흘려보낸 뒤(스트림 종료 없이)
    // 계속 라이브 이벤트를 이어받는지 검증한다.
    #[cfg(feature = "serve")]
    #[tokio::test]
    async fn dashboard_event_json_stream_signals_lagged_then_continues() {
        use crate::store::a2a::{Task, TaskEvent};
        use futures_util::StreamExt;

        // 용량 2: 스트림을 poll하기 전에 용량을 넘겨 보내 다음 recv()가 Err(Lagged)를 받게 한다.
        let (tx, rx) = tokio::sync::broadcast::channel::<TaskEvent>(2);
        let stream = dashboard_event_json_stream(rx);
        futures_util::pin_mut!(stream);

        for i in 0..5 {
            let t = Task::new(
                "flood",
                None,
                "win-claude",
                "mac-claude",
                format!("2026-07-06 10:0{i}:00"),
            );
            tx.send(TaskEvent::Status(t)).unwrap();
        }

        let f1: serde_json::Value =
            serde_json::from_str(&stream.next().await.expect("lagged 프레임 있어야 함")).unwrap();
        assert_eq!(
            f1["event"], "lagged",
            "Lagged는 조용히 skip 대신 신호 프레임으로 알려야 함"
        );
        assert!(
            f1.get("task").is_none(),
            "lagged 프레임은 task 필드가 없어야 기존 파서가 무해히 무시"
        );

        // 신호 이후에도 스트림은 종료되지 않고 라이브 이벤트를 계속 이어받는다. 용량 2 채널이라
        // 아직 소비 안 된 flood 버퍼(및 그 추가 eviction으로 인한 후속 Lagged)가 task-b보다 먼저
        // 올 수 있으므로, task-b의 status 프레임이 나올 때까지 드레인하며 스트림이 살아있음을 확인한다.
        let task_b = Task::new(
            "task-b",
            None,
            "win-claude",
            "mac-claude",
            "2026-07-06 10:10:00",
        );
        tx.send(TaskEvent::Status(task_b)).unwrap();
        let mut saw_task_b = false;
        for _ in 0..8 {
            let f: serde_json::Value =
                serde_json::from_str(&stream.next().await.expect("lagged 이후 프레임 있어야 함"))
                    .unwrap();
            if f["event"] == "status" && f["task"]["id"] == "task-b" {
                saw_task_b = true;
                break;
            }
            // 그 외(버퍼된 flood status·추가 lagged 신호)는 스트림이 계속 살아있다는 증거라 넘어간다.
        }
        assert!(
            saw_task_b,
            "lagged 신호 이후에도 라이브 task-b 이벤트를 이어받아야 함"
        );
    }

    // --- v2-45 P2: envelope 공용 헬퍼 + 쿼리 파싱 단위테스트 ---

    /// §5-2 고정 계약: state가 completed일 때만 "completed", 그 외(failed/canceled 포함) 전부 "status".
    /// failed/canceled를 "completed"로 내보내면 계약 파손(조사 중 자기모순 있었던 지점의 회귀 가드).
    #[cfg(feature = "serve")]
    #[test]
    fn dashboard_envelope_json_maps_only_completed_state_to_completed_event() {
        use crate::store::a2a::Task;
        let expectations = [
            (TaskState::Submitted, "status"),
            (TaskState::Working, "status"),
            (TaskState::InputRequired, "status"),
            (TaskState::Completed, "completed"),
            (TaskState::Failed, "status"),
            (TaskState::Canceled, "status"),
        ];
        for (state, expected) in expectations {
            let mut task = Task::new("t1", None, "win", "mac", "2026-07-11 09:00:00");
            task.state = state;
            let frame: serde_json::Value =
                serde_json::from_str(&dashboard_envelope_json(&task)).unwrap();
            assert_eq!(
                frame["event"], expected,
                "state={state:?}의 envelope 매핑이 §5-2와 다름"
            );
            assert_eq!(frame["task"]["id"], "t1");
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn parse_dashboard_events_query_defaults_and_each_param() {
        // 무파라미터 = 기본(replay 0, since/dispatcher 없음) = 현행 라이브 전용.
        assert_eq!(
            parse_dashboard_events_query(""),
            DashboardEventsQuery::default()
        );
        // replay 단독.
        assert_eq!(parse_dashboard_events_query("replay=50").replay, 50);
        // 파싱 불가 replay는 0(무시), 상한 초과는 상한으로 클램프.
        assert_eq!(parse_dashboard_events_query("replay=abc").replay, 0);
        assert_eq!(
            parse_dashboard_events_query("replay=999999").replay,
            DASHBOARD_REPLAY_MAX
        );
        // since(%20·%3A 인코딩) + dispatcher 조합.
        let q = parse_dashboard_events_query(
            "since=2026-07-11%2009%3A00%3A00&dispatcher=win-opus-boss",
        );
        assert_eq!(q.since.as_deref(), Some("2026-07-11 09:00:00"));
        assert_eq!(q.dispatcher.as_deref(), Some("win-opus-boss"));
        // '+' 공백 인코딩도 동등.
        let q = parse_dashboard_events_query("since=2026-07-11+09:00:00");
        assert_eq!(q.since.as_deref(), Some("2026-07-11 09:00:00"));
        // 빈 값 since/dispatcher는 None(전체 의미, watch-results 의미와 일치).
        let q = parse_dashboard_events_query("since=&dispatcher=");
        assert_eq!(q.since, None);
        assert_eq!(q.dispatcher, None);
        // 알 수 없는 키는 무시.
        assert_eq!(
            parse_dashboard_events_query("foo=bar"),
            DashboardEventsQuery::default()
        );
    }

    /// P2 리뷰 이월(§5-3 하드닝): ISO8601 'T' 구분자·말미 'Z'가 혼입돼도 DB datetime 포맷으로
    /// 정규화된다('T' > ' ' 사전순 왜곡 방어).
    #[cfg(feature = "serve")]
    #[test]
    fn parse_dashboard_events_query_normalizes_iso_since() {
        let q = parse_dashboard_events_query("since=2026-07-11T09:00:00");
        assert_eq!(q.since.as_deref(), Some("2026-07-11 09:00:00"));
        let q = parse_dashboard_events_query("since=2026-07-11T09%3A00%3A00Z");
        assert_eq!(q.since.as_deref(), Some("2026-07-11 09:00:00"));
        // 정규화 후 빈 값(순수 'Z' 등)은 None 유지.
        let q = parse_dashboard_events_query("since=Z");
        assert_eq!(q.since, None);
    }

    #[cfg(feature = "serve")]
    #[test]
    fn percent_decode_handles_plus_hex_and_malformed_sequences() {
        assert_eq!(
            percent_decode("2026-07-11%2009%3A00%3A00"),
            "2026-07-11 09:00:00"
        );
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("plain"), "plain");
        // 불완전/비-hex %시퀀스는 그대로 통과(패닉·소실 없음).
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        // UTF-8 멀티바이트(한글) 복원.
        assert_eq!(percent_decode("%ED%94%BC%EB%93%9C"), "피드");
    }

    // 대시보드 health/human-ping/deregister 핸들러 계약 + 토큰 설정 시 /dashboard/* 읽기가 bearer
    // 게이트 밖(무인증)이라는 라우터 합성 계약(v2-45 관제탑 원칙: 읽기는 항상 무인증 관전 가능).
    #[cfg(feature = "serve")]
    mod dashboard_health_and_write_handlers {
        use super::super::*;

        /// initialize 요청 본문(mcp_client.rs 테스트·http_serve 테스트와 동일한 MCP 2025-03-26 프로토콜).
        const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

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

        fn test_store() -> Arc<Mutex<crate::store::sqlite::SqliteStore>> {
            Arc::new(Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ))
        }

        /// 토큰이 설정된 코어라도 /dashboard/roster·/dashboard/events(읽기)는 인증 없이 200이어야
        /// 한다(관제탑 원칙: 원격 관전은 무인증, /mcp·/a2a만 bearer로 게이트). 라우터 조립에서
        /// dashboard 서브라우터가 bearer 미들웨어 바깥(authed와 별도 merge)에 있다는 계약을 실제
        /// HTTP 왕복으로 고정한다.
        #[tokio::test]
        async fn dashboard_reads_bypass_bearer_gate_even_with_token_configured() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();
            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener,
                    retriever,
                    None,
                    None,
                    None,
                    Some("secret-tok".to_string()),
                    test_store(),
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            // 헤더(Authorization) 없이 GET -> 200이어야 함(무인증 읽기).
            let roster = reqwest::get(format!("http://127.0.0.1:{port}/dashboard/roster"))
                .await
                .expect("roster get");
            assert_eq!(roster.status(), 200, "roster는 토큰 없이도 읽혀야 함");

            let events = reqwest::get(format!("http://127.0.0.1:{port}/dashboard/events"))
                .await
                .expect("events get");
            assert_eq!(events.status(), 200, "events도 토큰 없이 접속 가능해야 함");

            // 대조: /mcp는 같은 코어에서 토큰 없이 401(bearer 게이트 안쪽).
            let mcp_resp = reqwest::Client::new()
                .post(format!("http://127.0.0.1:{port}/mcp"))
                .header("Content-Type", "application/json")
                .header("Accept", "application/json, text/event-stream")
                .body(INIT_BODY)
                .send()
                .await
                .expect("mcp post");
            assert_eq!(
                mcp_resp.status(),
                401,
                "/mcp는 같은 코어에서 토큰 없이 401이어야 함(대시보드와 게이트 분리 대조군)"
            );
        }

        /// health: 조회 실패(broker_started_at 형식 손상)를 정상 0으로 위장하지 않고 500으로
        /// 표면화한다(fail-visible, 관제 오판 방지 원칙).
        #[tokio::test]
        async fn health_surfaces_500_on_corrupted_config_instead_of_faking_zero() {
            let store = test_store();
            {
                let s = store.lock().unwrap();
                // age_secs가 파싱 못 하는 값 -> uptime_secs 계산에서 Err("형식 손상")로 이어져야 한다.
                s.set_config("broker_started_at", "이건-datetime이-아님")
                    .unwrap();
            }
            let resp = dashboard_health_handler(axum::extract::State(store)).await;
            assert_eq!(
                resp.status(),
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "손상된 broker_started_at은 500으로 표면화돼야 함(0 위장 금지)"
            );
        }

        /// health: 정상 상태에서는 200 + task_counts 등 필드가 채워진 JSON을 반환한다(대조군).
        #[tokio::test]
        async fn health_returns_200_with_task_counts_on_healthy_store() {
            let store = test_store();
            let resp = dashboard_health_handler(axum::extract::State(store)).await;
            assert_eq!(resp.status(), axum::http::StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .expect("본문 읽기");
            let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            assert!(
                body.get("task_counts").is_some(),
                "task_counts 필드 필요: {body}"
            );
            assert_eq!(body["open_tasks"], 0);
        }

        /// roster busy: state=Working이면서 updated_at이 신선(5분 이내)한 task의 to_agent만 busy=true여야
        /// 한다(이슈 #94 FP 수정 - 갱신 없는 오래된 working=정체라 스피너를 꺼야 함). submitted는 애초에
        /// working이 아니니 busy=false 대조군으로 같이 확인한다.
        #[tokio::test]
        async fn roster_busy_requires_fresh_updated_at_v2_55() {
            use crate::store::a2a::Task;
            let store = test_store();
            let now = { store.lock().unwrap().now().unwrap() };
            {
                let s = store.lock().unwrap();
                s.register_agent("fresh-worker", BTreeMap::new(), None, &now);
                s.register_agent("stale-worker", BTreeMap::new(), None, &now);
                s.register_agent("idle-worker", BTreeMap::new(), None, &now);

                // 방금 갱신된 working -> busy true.
                let mut fresh = Task::new("t-fresh", None, "win", "fresh-worker", now.as_str());
                fresh.state = TaskState::Working;
                s.create_task(&fresh).unwrap();

                // working이지만 5분(BUSY_FRESH_SECS) 초과 갱신정지 -> busy false(정체로 간주).
                let mut stale = Task::new("t-stale", None, "win", "stale-worker", now.as_str());
                stale.state = TaskState::Working;
                s.create_task(&stale).unwrap();
                s.test_force_task_stale("t-stale", 10);

                // submitted(아직 working 아님) -> busy false.
                let idle = Task::new("t-idle", None, "win", "idle-worker", now.as_str());
                s.create_task(&idle).unwrap();
            }

            let resp = dashboard_roster_handler(axum::extract::State(store)).await;
            assert_eq!(resp.status(), axum::http::StatusCode::OK);
            let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
                .await
                .expect("본문 읽기");
            let body: serde_json::Value = serde_json::from_slice(&bytes).expect("json");
            let agents = body.as_array().expect("배열이어야 함");
            let busy_of = |uuid: &str| -> bool {
                agents
                    .iter()
                    .find(|a| a["uuid"] == uuid)
                    .unwrap_or_else(|| panic!("{uuid} 로스터에 없음: {agents:?}"))["busy"]
                    .as_bool()
                    .unwrap()
            };
            assert!(busy_of("fresh-worker"), "신선한 working은 busy=true여야 함");
            assert!(
                !busy_of("stale-worker"),
                "5분 초과 갱신정지 working은 busy=false여야 함(정체)"
            );
            assert!(!busy_of("idle-worker"), "submitted는 busy=false여야 함");
        }

        /// human-ping: 미등록(무장 전) uuid도 영속 테이블에 선기록되고 200을 반환한다(v2-45 P4,
        /// 404 유실 창 제거). 이후 register_agent가 그 uuid를 로스터에 올리면 영속된 human_input_at이
        /// 복원되는지까지 확인해(register_agent의 load_human_input 폴백 경로) 실제 영속을 검증한다.
        #[tokio::test]
        async fn human_ping_for_unregistered_uuid_returns_200_and_persists() {
            let store = test_store();
            let loopback: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let body =
                axum::body::Bytes::from(serde_json::json!({"agent": "ghost-uuid"}).to_string());
            let resp = dashboard_human_ping_handler(
                axum::extract::ConnectInfo(loopback),
                axum::extract::State(store.clone()),
                axum::Extension(Arc::new(None::<String>)),
                axum::http::HeaderMap::new(),
                body,
            )
            .await;
            assert_eq!(
                resp.status(),
                axum::http::StatusCode::OK,
                "미등록 uuid 핑도 200이어야 함(선기록)"
            );

            // register_agent가 영속된 human_input_at을 복원하는지로 persist_human_input 영속을 검증한다
            // (load_human_input은 registry.rs 내부 private라 직접 호출 불가, 공개 경로로 우회 검증).
            let now = {
                let s = store.lock().unwrap();
                let now = s.now().unwrap();
                s.register_agent("ghost-uuid", BTreeMap::new(), None, &now);
                now
            };
            let agents = store
                .lock()
                .unwrap()
                .list_agents(&BTreeMap::new(), &now, i64::MAX);
            let ghost = agents
                .iter()
                .find(|a| a.uuid == "ghost-uuid")
                .expect("register 후 로스터에 있어야 함");
            assert!(
                ghost.human_input_at.is_some(),
                "핑으로 영속된 human_input_at이 register 시 복원돼야 함: {ghost:?}"
            );
        }

        /// deregister: 미등록 uuid는 404(멱등 - 이미 없거나 애초에 없던 세션도 훅은 실패 취급 안 함).
        #[tokio::test]
        async fn deregister_unregistered_uuid_is_404_idempotent() {
            let store = test_store();
            let loopback: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
            let body = axum::body::Bytes::from(
                serde_json::json!({"agent": "never-registered"}).to_string(),
            );
            let resp = dashboard_deregister_handler(
                axum::extract::ConnectInfo(loopback),
                axum::extract::State(store),
                axum::Extension(Arc::new(None::<String>)),
                axum::http::HeaderMap::new(),
                body,
            )
            .await;
            assert_eq!(
                resp.status(),
                axum::http::StatusCode::NOT_FOUND,
                "미등록 uuid deregister는 404여야 함"
            );
        }
    }
}
