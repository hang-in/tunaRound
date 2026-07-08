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
        .route("/dashboard/candidates", get(dashboard_candidates_handler))
        .route("/dashboard/goal", axum::routing::post(dashboard_goal_handler))
        .route("/dashboard/human-ping", axum::routing::post(dashboard_human_ping_handler))
        .route("/dashboard/control", axum::routing::post(dashboard_control_handler));
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
    let app = dashboard.with_state(a2a_store_for_dash).merge(authed);

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
                    let (kind, task) = match &ev {
                        TaskEvent::Status(t) => ("status", t),
                        TaskEvent::Completed(t) => ("completed", t),
                    };
                    let envelope = serde_json::json!({ "event": kind, "task": task });
                    let data = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                    return Some((data, rx));
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            }
        }
    })
}

/// 위 JSON 문자열을 axum SSE Event로 감싼다(HTTP 핸들러 전용 얇은 래퍼).
#[cfg(feature = "serve")]
fn dashboard_event_stream(
    rx: tokio::sync::broadcast::Receiver<crate::store::a2a::TaskEvent>,
) -> impl futures_util::Stream<Item = Result<axum::response::sse::Event, std::convert::Infallible>> {
    use futures_util::StreamExt;
    dashboard_event_json_stream(rx).map(|data| Ok(axum::response::sse::Event::default().data(data)))
}

/// GET /dashboard/events: 전역 task 이벤트 SSE(대시보드 라이브 피드). 브라우저 EventSource가 구독한다.
#[cfg(feature = "serve")]
async fn dashboard_events_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    let sender = {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        store.task_event_sender()
    };
    let Some(sender) = sender else {
        return (axum::http::StatusCode::SERVICE_UNAVAILABLE, "task event bus 미활성").into_response();
    };
    let rx = sender.subscribe();
    let stream = dashboard_event_stream(rx);
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

/// GET /dashboard/candidates: 발견된(미무장) 세션 후보 JSON. 브라우저가 주기 폴(S3 "발견된 세션" 패널).
/// armed는 저장값이 아니라 online roster 소속으로 계산한 overlay다(무장되면 자동 armed=true로 승격 표시).
#[cfg(feature = "serve")]
async fn dashboard_candidates_handler(
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    #[derive(serde::Serialize)]
    struct DashCandidate {
        uuid: String,
        runner: String,
        project: Option<String>,
        machine: Option<String>,
        source: String,
        age_secs: i64,
        reported_at: String,
        armed: bool,
    }
    let candidates: Vec<DashCandidate> = tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        let now = store.now().unwrap_or_default();
        // armed overlay: online roster의 uuid 또는 session 태그에 있으면 이미 무장된 것으로 표시.
        let armed = store.armed_session_ids(&now, AGENT_TTL_SECS);
        store
            .list_candidates(&now, CANDIDATE_TTL_SECS)
            .into_iter()
            .map(|c| {
                let is_armed = armed.contains(&c.uuid);
                DashCandidate {
                    uuid: c.uuid,
                    runner: c.runner,
                    project: c.project,
                    machine: c.machine,
                    source: c.source,
                    age_secs: c.age_secs,
                    reported_at: c.reported_at,
                    armed: is_armed,
                }
            })
            .collect()
    })
    .await
    .unwrap_or_default();
    let body = serde_json::to_vec(&candidates).unwrap_or_else(|_| b"[]".to_vec());
    (
        axum::http::StatusCode::OK,
        [(axum::http::header::CONTENT_TYPE, "application/json")],
        body,
    )
        .into_response()
}

/// 로컬 write 엔드포인트(goal/control)의 local CSRF 방어. 브라우저가 붙이는 `Sec-Fetch-Site`가
/// `cross-site`면 다른 사이트가 유도한 요청이므로 거부한다. 헤더가 없으면(curl 등 비브라우저) 허용.
#[cfg(feature = "serve")]
fn is_cross_site(headers: &axum::http::HeaderMap) -> bool {
    matches!(
        headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        Some("cross-site")
    )
}

/// codex 제어(turn/start) 대상 ws가 loopback인지 검사한다(SSRF 방어: 브로커가 임의 원격 ws에
/// 접속하지 않게). ws://127.0.0.1[:port] / localhost / [::1] / ::1 만 허용한다.
#[cfg(feature = "serve")]
fn ws_target_is_loopback(ws: &str) -> bool {
    // 스킴 제거 후 authority(host[:port])만 취한다(경로/쿼리/프래그먼트 제거).
    let after = ws
        .strip_prefix("ws://")
        .or_else(|| ws.strip_prefix("wss://"))
        .unwrap_or(ws);
    let authority = after.split(['/', '?', '#']).next().unwrap_or(after);
    // userinfo(user:pass@) 제거: 마지막 @ 이후가 실제 host[:port]다. `ws://127.0.0.1:80@evil.com`처럼
    // @ 앞을 host로 오인하면 loopback을 통과시키고 실제로는 evil.com에 접속해 SSRF 우회가 된다(CodeRabbit 지적).
    let hostport = authority.rsplit('@').next().unwrap_or(authority);
    // IPv6 대괄호 형태 [::1]:port 처리, 아니면 host:port에서 host.
    let host = if let Some(rest) = hostport.strip_prefix('[') {
        rest.split(']').next().unwrap_or(rest)
    } else {
        hostport.split(':').next().unwrap_or(hostport)
    };
    // FQDN 끝점(`localhost.`, `127.0.0.1.`) 제거 후 판정. 문자열 prefix(`starts_with("127.")`)는
    // `127.0.0.1.evil.com` 같은 외부 도메인을 허용해 쓰지 않는다(gemini). localhost는 IP가 아니라 별도
    // 허용(호스트명은 대소문자 무시). IpAddr::is_loopback은 IPv4 127.0.0.0/8 + IPv6 ::1을 모두 커버한다.
    let host = host.trim_end_matches('.');
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(false)
}

/// POST /dashboard/human-ping: 세션의 UserPromptSubmit 훅이 "이 세션이 방금 사람 프롬프트를 받았다"를
/// 보고한다(총감독=human_input_at 최신 세션, 설계 v2-42). body = {"agent": "<세션 uuid>"}. loopback만 허용
/// (원격은 read-only). 무인증이지만 loopback 신뢰. 미등록 uuid(무장 전)면 404.
#[cfg(feature = "serve")]
async fn dashboard_human_ping_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    axum::extract::State(store): axum::extract::State<Arc<Mutex<crate::store::sqlite::SqliteStore>>>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    if !peer.ip().is_loopback() {
        return (axum::http::StatusCode::FORBIDDEN, "원격 관전 모드: 핑은 로컬(loopback)에서만.").into_response();
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


/// POST /dashboard/control: 로컬(loopback) 총감독이 codex app-server 세션에 turn/start를 직접 주입한다
/// (v2-40 S4). body = {"ws":"ws://127.0.0.1:8790","text":"지시","agent"?:"...","timeout"?:300}.
/// 응답 = {"answer":"codex 최종답"} 또는 에러. 원격(비-loopback)은 403(관전 전용). 실제 주입은 worker 피처.
#[cfg(feature = "serve")]
async fn dashboard_control_handler(
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> axum::response::Response {
    use axum::response::IntoResponse;
    // loopback만 제어 허용. 원격은 관전 전용(goal과 동일 신뢰 경계).
    if !peer.ip().is_loopback() {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "원격 관전 모드: codex 제어는 로컬(loopback)에서만 가능합니다.",
        )
            .into_response();
    }
    // local CSRF 방어: 다른 사이트가 유도한 cross-site POST 거부.
    if is_cross_site(&headers) {
        return (axum::http::StatusCode::FORBIDDEN, "cross-site 요청 거부(local CSRF 방어).").into_response();
    }
    // agent·timeout은 worker 피처 빌드에서만 읽힌다(worker 없이 빌드하면 501). 조건부 DTO라 dead_code 허용.
    #[derive(serde::Deserialize)]
    #[cfg_attr(not(feature = "worker"), allow(dead_code))]
    struct ControlReq {
        ws: String,
        text: String,
        agent: Option<String>,
        timeout: Option<u64>,
    }
    let req: ControlReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => return (axum::http::StatusCode::BAD_REQUEST, format!("잘못된 요청: {e}")).into_response(),
    };
    if req.ws.trim().is_empty() || req.text.trim().is_empty() {
        return (axum::http::StatusCode::BAD_REQUEST, "ws와 text가 필요합니다.").into_response();
    }
    // SSRF 방어: 제어 대상 ws는 loopback만(브로커가 임의 원격 ws에 접속하지 않게).
    if !ws_target_is_loopback(req.ws.trim()) {
        return (
            axum::http::StatusCode::FORBIDDEN,
            "제어 대상 ws는 loopback(127.0.0.1/localhost/[::1])만 허용합니다.",
        )
            .into_response();
    }
    #[cfg(feature = "worker")]
    {
        let agent = req.agent.unwrap_or_else(|| "dashboard-control".to_string());
        let timeout = req.timeout.unwrap_or(300);
        // 제어 주입은 tuna-broker MCP 호출 자동승인(never) + workspace-write(감독 레시피와 동일).
        let approval = crate::codex_appserver::ApprovalPolicy::Never;
        let sandbox = crate::codex_appserver::SandboxMode::WorkspaceWrite;
        match crate::codex_inject::run(&req.ws, &agent, &req.text, approval, sandbox, timeout, false).await {
            Ok(answer) => {
                #[derive(serde::Serialize)]
                struct Resp {
                    answer: String,
                }
                let bodyv = serde_json::to_vec(&Resp { answer }).unwrap_or_else(|_| b"{}".to_vec());
                (
                    axum::http::StatusCode::OK,
                    [(axum::http::header::CONTENT_TYPE, "application/json")],
                    bodyv,
                )
                    .into_response()
            }
            Err(e) => (axum::http::StatusCode::BAD_GATEWAY, format!("codex 제어 실패: {e}")).into_response(),
        }
    }
    #[cfg(not(feature = "worker"))]
    {
        let _ = req;
        (
            axum::http::StatusCode::NOT_IMPLEMENTED,
            "worker 피처 없이 빌드됨: codex 제어(codex-inject) 비활성입니다.",
        )
            .into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[cfg(feature = "serve")]
    #[test]
    fn ws_target_is_loopback_accepts_only_local() {
        assert!(ws_target_is_loopback("ws://127.0.0.1:8790"));
        assert!(ws_target_is_loopback("ws://localhost:8790"));
        assert!(ws_target_is_loopback("ws://[::1]:8790"));
        assert!(ws_target_is_loopback("ws://127.0.0.5:9000/path"));
        assert!(!ws_target_is_loopback("ws://192.168.1.50:8790"));
        assert!(!ws_target_is_loopback("ws://evil.example.com:8790"));
        assert!(!ws_target_is_loopback("ws://10.0.0.1"));
        // SSRF 우회 방지: 127.로 시작하는 외부 도메인은 거부(IpAddr 파싱 실패).
        assert!(!ws_target_is_loopback("ws://127.0.0.1.evil.com:8790"));
        assert!(!ws_target_is_loopback("ws://127.0.0.1x:8790"));
        // userinfo 우회 방지: @ 앞을 host로 오인하면 안 된다(실제 host는 @ 뒤).
        assert!(!ws_target_is_loopback("ws://127.0.0.1:80@evil.com"));
        assert!(!ws_target_is_loopback("ws://127.0.0.1@evil.com:8790"));
        // 정상: userinfo가 붙어도 실제 host가 loopback이면 허용.
        assert!(ws_target_is_loopback("ws://user@127.0.0.1:8790"));
        // 대소문자 무시 + FQDN 끝점(.)도 loopback으로 인정.
        assert!(ws_target_is_loopback("ws://LocalHost:8790"));
        assert!(ws_target_is_loopback("ws://localhost.:8790"));
        assert!(ws_target_is_loopback("ws://127.0.0.1.:8790"));
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
}
