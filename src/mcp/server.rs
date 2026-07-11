// HTTP 전송 계층과 총감독 대시보드: axum 서버·SSE·라우트·정적 자산 서빙을 담당한다.

use super::*;

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
    serve_http_mcp_on_listener(listener, retriever, reader, writer, roster, token, a2a_store).await
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
    let a2a_url = core_a2a_url(&bound_addr.to_string());
    let agent_card = crate::a2a_server::build_agent_card(&a2a_url);
    // MCP inbox 툴(poll_tasks/claim_task/complete_task)도 같은 a2a_store Arc를 공유한다(새 커넥션을
    // 만들지 않고 단일 mutex로 직렬화. Phase 1 저볼륨 전제. docs/design/v2-a2a-partner-delegation_2026-07-02.md §10-1).
    let a2a_store_for_mcp = a2a_store.clone();
    // 대시보드 SSE/roster용 clone(같은 store = 같은 이벤트버스·로스터를 공유한다).
    let a2a_store_for_dash = a2a_store.clone();
    let a2a_router = crate::a2a_server::build_router(a2a_store, agent_card);

    let retriever2 = retriever.clone();
    let reader2 = reader.clone();
    let writer2 = writer.clone();
    let roster2 = roster.clone();
    // service_factory: 요청마다 새 TunaSearchServer 인스턴스를 생성한다(Clone 불필요, Arc 공유).
    let service: StreamableHttpService<TunaSearchServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                let mut s =
                    TunaSearchServer::new(retriever2.clone()).with_a2a_store(a2a_store_for_mcp.clone());
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
    let merged = Router::new().nest_service("/mcp", service).merge(a2a_router);

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
                if auth == expected {
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
        .route("/dashboard/goal", axum::routing::post(dashboard_goal_handler))
        .route("/dashboard/human-ping", axum::routing::post(dashboard_human_ping_handler))
        .route("/dashboard/deregister", axum::routing::post(dashboard_deregister_handler));
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
    let app = dashboard
        .with_state(a2a_store_for_dash)
        .layer(axum::Extension(dash_token))
        .merge(authed);

    #[cfg(feature = "dashboard")]
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드 SPA: /dashboard)");
    #[cfg(not(feature = "dashboard"))]
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr} (대시보드: dashboard 피처 없이 빌드됨)");
    // ConnectInfo(peer addr)로 /dashboard/goal의 loopback 판정을 하기 위해 connect-info make service를 쓴다.
    axum::serve(listener, app.into_make_service_with_connect_info::<std::net::SocketAddr>()).await?;
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
        Some(content) => {
            ([(axum::http::header::CONTENT_TYPE, mime_for_path(path))], content.data.into_owned()).into_response()
        }
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
    let kind = if task.state == TaskState::Completed { "completed" } else { "status" };
    let envelope = serde_json::json!({ "event": kind, "task": task });
    serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string())
}

/// 전역 task 이벤트를 JSON data 문자열로 흘리는 순수 스트림(단위테스트 대상). task_id 필터 없이 모든
/// TaskEvent를 내보낸다. Lagged는 스킵하고 계속, Closed면 종료한다.
#[cfg(feature = "serve")]
fn dashboard_event_json_stream(
    rx: tokio::sync::broadcast::Receiver<crate::store::a2a::TaskEvent>,
) -> impl futures_util::Stream<Item = String> {
    use crate::store::a2a::TaskEvent;
    futures_util::stream::unfold(rx, |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    // Status/Completed 모두 변이 후 Task 전체 스냅샷을 담으므로 envelope 매핑은
                    // state 기준 공용 헬퍼로 수렴한다(§5-2, dashboard_envelope_json 주석 참조).
                    let task = match &ev {
                        TaskEvent::Status(t) | TaskEvent::Completed(t) => t,
                    };
                    return Some((dashboard_envelope_json(task), rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
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

/// replay 상한. 재생은 피드 창(50건)용 표면이라 전 테이블 덤프 수준의 N을 막는다(원격 관전자도
/// 무인증으로 붙는 엔드포인트라 방어적 상한).
#[cfg(feature = "serve")]
const DASHBOARD_REPLAY_MAX: usize = 500;

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
            "since" if !value.is_empty() => q.since = Some(value),
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
#[cfg(feature = "serve")]
async fn dashboard_events_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
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
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "task event bus 미활성").into_response();
    };
    // 순서 계약(§3): 스냅샷 질의보다 먼저 구독해야 질의 중 일어난 전이도 rx에 버퍼되어 유실되지
    // 않는다(a2a_server handle_subscribe_to_task의 subscribe-먼저 순서 답습). 그 대가로 스냅샷과
    // 라이브가 같은 전이를 중복 운반할 수 있는데, 소비자(피드 updatedAt 가드·watch-results seen)가
    // dedup한다.
    let rx = sender.subscribe();
    let snapshot: Vec<String> = if query.since.is_some() || query.replay > 0 {
        // lock은 spawn_blocking 안에서 짧게 잡는다(roster 핸들러 패턴). SSE 스트림 안에서 lock 보유 금지.
        tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let tasks = if let Some(since) = query.since.as_deref() {
                let dispatcher = query.dispatcher.as_deref().filter(|d| !d.is_empty());
                store.list_tasks_replay(dispatcher, Some(since), &["completed", "failed"], None)
            } else {
                // replay = 전 상태 스냅샷(canceled·열린 task 포함 - 피드는 전 상태 뷰).
                store.list_tasks_replay(None, None, &[], Some(query.replay))
            };
            tasks
                .unwrap_or_else(|e| {
                    eprintln!("[dashboard] 재생 스냅샷 질의 실패(라이브만 제공): {e}");
                    Vec::new()
                })
                .iter()
                .map(dashboard_envelope_json)
                .collect()
        })
        .await
        .unwrap_or_default()
    } else {
        Vec::new()
    };
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
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
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
    }
    let agents: Vec<DashAgent> = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        // TTL=i64::MAX로 오프라인 포함 전체 조회 후, online은 실제 TTL(AGENT_TTL_SECS)로 per-agent 계산.
        store
            .list_agents(&BTreeMap::new(), &now, i64::MAX)
            .into_iter()
            .map(|a| {
                let online = crate::store::agents::is_online(&a.last_heartbeat, &now, AGENT_TTL_SECS);
                DashAgent {
                    uuid: a.uuid,
                    tags: a.tags,
                    display_name: a.display_name,
                    last_heartbeat: a.last_heartbeat,
                    online,
                    human_input_at: a.human_input_at,
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    let body = serde_json::to_vec(&agents).unwrap_or_else(|_| b"[]".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
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
            .is_some_and(|a| a == format!("Bearer {tok}")),
    }
}

/// POST /dashboard/human-ping: 세션의 UserPromptSubmit 훅이 "이 세션이 방금 사람 프롬프트를 받았다"를
/// 보고한다(총감독=human_input_at 최신 세션, 설계 v2-42). body = {"agent": "<세션 uuid>"}.
/// loopback 무조건 허용 + 원격은 Bearer 토큰 필요(dashboard_write_allowed). 미등록 uuid(무장 전)면 404.
#[cfg(feature = "serve")]
async fn dashboard_human_ping_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
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
        return (axum::http::StatusCode::FORBIDDEN, "cross-site 요청 거부(local CSRF 방어).").into_response();
    }
    #[derive(serde::Deserialize)]
    struct PingReq {
        agent: String,
    }
    let req: PingReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
    };
    let ok = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        store.mark_human_input(&req.agent, &now)
    })
    .await
    .unwrap_or(false);
    if ok {
        (axum::http::StatusCode::OK, "ok").into_response()
    } else {
        // 미등록(아직 무장 안 됨). 훅이 무장 후 재핑하면 된다.
        (axum::http::StatusCode::NOT_FOUND, "미등록 세션(무장 선행 필요)").into_response()
    }
}

/// POST /dashboard/deregister: 세션의 SessionEnd 훅(disarm)이 "이 세션이 닫혔다"를 보고한다.
/// 로스터에서 즉시 제거해 TTL(90초) 자연소멸을 기다리지 않는다(설계 v2-43 잔존구간 제거).
/// body = {"agent": "<세션 uuid>"}. loopback 무조건 허용 + 원격은 Bearer 토큰 필요
/// (맥 세션 종료도 즉시 등록해제, dashboard_write_allowed). 미등록 uuid면 404(멱등).
#[cfg(feature = "serve")]
async fn dashboard_deregister_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
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
        return (axum::http::StatusCode::FORBIDDEN, "cross-site 요청 거부(local CSRF 방어).").into_response();
    }
    #[derive(serde::Deserialize)]
    struct DeregReq {
        agent: String,
    }
    let req: DeregReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
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
        (axum::http::StatusCode::NOT_FOUND, "미등록 세션(이미 제거됨)").into_response()
    }
}

/// POST /dashboard/goal: 로컬(loopback) 총감독이 선택한 감독들에게 목표를 던진다(대상마다 1 task).
/// 원격(비-loopback)은 read-only 관전이라 403. 무인증이지만 loopback 신뢰라 로컬 한정 write.
/// body = {"text": "...", "targets": ["uuid", ...]}. 응답 = {"created":[{taskId,toAgent}], "errors":[...]}.
#[cfg(feature = "serve")]
async fn dashboard_goal_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // loopback만 목표 제출 허용. 원격은 관전 전용.
    if !peer.ip().is_loopback() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 관전 모드: 목표 제출은 로컬(loopback)에서만 가능합니다.",
        )
            .into_response();
    }
    // local CSRF 방어: 다른 사이트가 유도한 cross-site POST 거부.
    if is_cross_site(&headers) {
        return (axum::http::StatusCode::FORBIDDEN, "cross-site 요청 거부(local CSRF 방어).").into_response();
    }
    #[derive(serde::Deserialize)]
    struct GoalReq {
        text: String,
        targets: Vec<String>,
    }
    let req: GoalReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
    };
    if req.text.trim().is_empty() || req.targets.is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "목표(text)와 대상(targets)이 필요합니다.").into_response();
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
                parts: vec![Part { text: Some(req.text.clone()), ..Default::default() }],
                task_id: None,
                context_id: None,
            };
            match store.create_task_from_message("dashboard", uuid, message) {
                Ok(task) => created.push(Created { task_id: task.id, to_agent: task.to_agent }),
                Err(e) => errors.push(format!("{uuid}: {e}")),
            }
        }
        GoalResp { created, errors }
    })
    .await
    .unwrap_or(GoalResp { created: vec![], errors: vec!["작업 실패".to_string()] });
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
            assert!(dashboard_write_allowed(ip, &headers_with_auth(None), &Some("tok".into())));
            // dual-stack 소켓의 IPv4-mapped IPv6 루프백도 로컬로 인정해야 한다.
            let mapped: std::net::IpAddr = "::ffff:127.0.0.1".parse().unwrap();
            assert!(dashboard_write_allowed(mapped, &headers_with_auth(None), &Some("tok".into())));
            let v6: std::net::IpAddr = "::1".parse().unwrap();
            assert!(dashboard_write_allowed(v6, &headers_with_auth(None), &Some("tok".into())));
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
            assert!(!dashboard_write_allowed(ip, &headers_with_auth(Some("Bearer nope")), &Some("tok".into())));
            assert!(!dashboard_write_allowed(ip, &headers_with_auth(None), &Some("tok".into())));
        }

        #[test]
        fn remote_allowed_when_core_has_no_token() {
            // 무토큰 코어는 /mcp 전체가 무인증(동일 계약)이라 대시보드 쓰기도 게이트하지 않는다.
            let ip: std::net::IpAddr = "192.168.0.9".parse().unwrap();
            assert!(dashboard_write_allowed(ip, &headers_with_auth(None), &None));
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
                    .map(|(s, c)| crate::orchestrator::Utterance { speaker: s.clone(), content: c.clone() })
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
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().unwrap().port();

            let log = SharedLog::default();
            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let reader = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptReader>);
            let writer = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptWriter>);
            let roster = Some(vec![
                RosterSeat { engine: "claude".into(), role: Some("proposer".into()) },
                RosterSeat { engine: "codex".into(), role: Some("reviewer".into()) },
            ]);
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, reader, writer, roster, None, test_a2a_store()).await;
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
            assert!(roster_text.contains("claude (proposer)"), "get_roster 응답: {roster_text}");

            // post_turn → 추가됨.
            let post_text =
                post(call_body(3, "post_turn", r#"{"speaker":"remote/agent","content":"원격 발언 핵심어 살구"}"#)).await;
            assert!(post_text.contains("msg_id="), "post_turn 응답: {post_text}");

            // read_transcript → 방금 post한 발언이 보임(쓰기→읽기 일관).
            let read_text = post(call_body(4, "read_transcript", "{}")).await;
            assert!(read_text.contains("살구"), "read_transcript에 post_turn 내용 없음: {read_text}");
        }

        /// HTTP MCP로 poll_tasks→claim_task→complete_task 왕복을 검증한다. Task 2(a2a_server)가 만든
        /// a2a_store Arc를 serve_http_mcp_on_listener가 TunaSearchServer와 실제로 공유하는지까지 확인한다.
        #[tokio::test]
        async fn http_poll_claim_complete_task_e2e() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
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

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ =
                    serve_http_mcp_on_listener(listener, retriever, None, None, None, None, store_for_server).await;
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
            assert!(poll_text.contains(&seeded_id), "poll_tasks 응답에 task_id 없음: {poll_text}");

            // claim_task → working 전이.
            let claim_body = format!(r#"{{"task_id":"{seeded_id}"}}"#);
            let claim_text = post(call_body(3, "claim_task", &claim_body)).await;
            assert!(claim_text.contains("state=working"), "claim_task 응답: {claim_text}");

            // complete_task → completed 전이 + artifact 저장.
            let complete_body = format!(r#"{{"task_id":"{seeded_id}","result":"작업 결과 요약"}}"#);
            let complete_text = post(call_body(4, "complete_task", &complete_body)).await;
            assert!(complete_text.contains("state=completed"), "complete_task 응답: {complete_text}");

            // DB 상태 최종 확인(HTTP 왕복 후 실제로 반영됐는지. serve_http_mcp_on_listener가 넘겨받은
            // 그 a2a_store Arc가 TunaSearchServer 쪽에도 공유됐다는 증거).
            let final_task = store.lock().unwrap().get_task(&seeded_id).unwrap().expect("존재해야 함");
            assert_eq!(final_task.state, TaskState::Completed);
            assert_eq!(final_task.artifacts.len(), 1);
            assert_eq!(final_task.artifacts[0].parts[0].text.as_deref(), Some("작업 결과 요약"));
        }

        /// v2-45 P2: ?replay=N이 과거 task 스냅샷 프레임(전 상태, updated_at 오름차순)을 라이브
        /// 스트림보다 먼저 내보내는지 HTTP 레벨로 검증한다(subscribe-먼저 + chain 배선 확인).
        #[tokio::test]
        async fn dashboard_events_replay_sends_snapshot_frames_before_live() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().unwrap().port();

            // 이벤트 버스 활성 store에 종결·취소 task를 미리 심는다(재기동 후 피드 리로드 시나리오).
            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ));
            {
                let s = store.lock().unwrap();
                let mut done =
                    crate::store::a2a::Task::new("done-task", None, "win", "mac", "2026-07-11 09:00:00");
                done.state = TaskState::Completed;
                s.create_task(&done).unwrap();
                let mut gone =
                    crate::store::a2a::Task::new("gone-task", None, "win", "mac", "2026-07-11 09:01:00");
                gone.state = TaskState::Canceled;
                s.create_task(&gone).unwrap();
            }

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener, retriever, None, None, None, None, store_for_server,
                )
                .await;
            });
            tokio::time::sleep(std::time::Duration::from_millis(120)).await;

            let resp = reqwest::get(format!("http://127.0.0.1:{port}/dashboard/events?replay=10"))
                .await
                .expect("SSE 접속 실패");
            assert_eq!(resp.status(), 200);

            // 스냅샷 2프레임이 접속 직후(라이브 이벤트 없이) 도착해야 한다. SSE 이벤트는 "\n\n"으로
            // 끝나므로, 청크 경계에서 잘린 미완 프레임은 세지 않는다(마지막 조각 제외).
            fn complete_data_frames(body: &str) -> Vec<&str> {
                let mut parts: Vec<&str> = body.split("\n\n").collect();
                parts.pop(); // 마지막 조각은 아직 미완일 수 있다.
                parts.into_iter().filter_map(|p| p.trim().strip_prefix("data: ")).collect()
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
                s.create_task_from_message("win", "live-target", msg).unwrap();
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

        /// 무파라미터 구독은 현행 그대로 라이브 전용이어야 한다(watch-results 재기동 시 과거 재통지
        /// 회귀 금지 - 설계 §4 P2 항목 5).
        #[tokio::test]
        async fn dashboard_events_without_params_sends_no_snapshot() {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind");
            let port = listener.local_addr().unwrap().port();

            let store = Arc::new(std::sync::Mutex::new(
                crate::store::sqlite::SqliteStore::open_memory()
                    .expect("in-memory sqlite")
                    .with_task_events(),
            ));
            {
                let s = store.lock().unwrap();
                let mut done =
                    crate::store::a2a::Task::new("done-task", None, "win", "mac", "2026-07-11 09:00:00");
                done.state = TaskState::Completed;
                s.create_task(&done).unwrap();
            }

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let store_for_server = store.clone();
            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(
                    listener, retriever, None, None, None, None, store_for_server,
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
                s.create_task_from_message("win", "live-target", msg).unwrap();
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

        #[test]
        fn core_local_url_maps_wildcards_to_loopback() {
            // 와일드카드 host는 loopback으로, 일반 host는 그대로.
            assert_eq!(core_local_url("0.0.0.0:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("[::]:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("127.0.0.1:8771"), "http://127.0.0.1:8771/mcp");
            assert_eq!(core_local_url("192.0.2.20:9000"), "http://192.0.2.20:9000/mcp");
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
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let token = Some("secret-tok".to_string());

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, None, None, token, test_a2a_store()).await;
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
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, None, None, None, test_a2a_store()).await;
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

        let task_a = Task::new("task-a", None, "win-claude", "mac-claude", "2026-07-06 10:00:00");
        let mut task_b = Task::new("task-b", None, "win-claude", "mac-codex", "2026-07-06 10:01:00");
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
            assert_eq!(frame["event"], expected, "state={state:?}의 envelope 매핑이 §5-2와 다름");
            assert_eq!(frame["task"]["id"], "t1");
        }
    }

    #[cfg(feature = "serve")]
    #[test]
    fn parse_dashboard_events_query_defaults_and_each_param() {
        // 무파라미터 = 기본(replay 0, since/dispatcher 없음) = 현행 라이브 전용.
        assert_eq!(parse_dashboard_events_query(""), DashboardEventsQuery::default());
        // replay 단독.
        assert_eq!(parse_dashboard_events_query("replay=50").replay, 50);
        // 파싱 불가 replay는 0(무시), 상한 초과는 상한으로 클램프.
        assert_eq!(parse_dashboard_events_query("replay=abc").replay, 0);
        assert_eq!(parse_dashboard_events_query("replay=999999").replay, DASHBOARD_REPLAY_MAX);
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
        assert_eq!(parse_dashboard_events_query("foo=bar"), DashboardEventsQuery::default());
    }

    #[cfg(feature = "serve")]
    #[test]
    fn percent_decode_handles_plus_hex_and_malformed_sequences() {
        assert_eq!(percent_decode("2026-07-11%2009%3A00%3A00"), "2026-07-11 09:00:00");
        assert_eq!(percent_decode("a+b"), "a b");
        assert_eq!(percent_decode("plain"), "plain");
        // 불완전/비-hex %시퀀스는 그대로 통과(패닉·소실 없음).
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
        // UTF-8 멀티바이트(한글) 복원.
        assert_eq!(percent_decode("%ED%94%BC%EB%93%9C"), "피드");
    }
}
