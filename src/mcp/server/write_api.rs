// 대시보드 쓰기 엔드포인트와 게이트: human/turn-ping·deregister·goal + CSRF·상수시간 토큰 검사.

use super::*;

/// 로컬 write 엔드포인트(goal 등)의 local CSRF 방어. 브라우저가 붙이는 `Sec-Fetch-Site`가
/// `cross-site`면 다른 사이트가 유도한 요청이므로 거부한다. 헤더가 없으면(curl 등 비브라우저) 허용.
#[cfg(feature = "serve")]
pub(super) fn is_cross_site(headers: &axum::http::HeaderMap) -> bool {
    matches!(
        headers.get("sec-fetch-site").and_then(|v| v.to_str().ok()),
        Some("cross-site")
    )
}

/// bearer 토큰을 상수시간으로 비교한다(타이밍 사이드채널 방지). 길이 노출은 허용(토큰 길이는
/// 비밀 아님) - 길이가 다르면 즉시 false, 길이가 같으면 전체를 XOR 누적해 조기반환 없이 비교한다.
#[cfg(feature = "serve")]
pub(super) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
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
pub(super) fn dashboard_write_allowed(
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
pub(super) async fn dashboard_human_ping_handler(
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
            // 이슈 #123: 사람 프롬프트 = 대화 턴 시작이기도 하다. 훅이 별도 turn-ping을 한 번 더
            // 보내지 않도록 여기 동승(훅은 매 프롬프트 동기 블로킹 = 왕복 1회 유지). 미등록이면 no-op.
            store.record_turn_start(&req.agent, &now);
        }
    })
    .await
    .ok();
    (axum::http::StatusCode::OK, "ok").into_response()
}

/// POST /dashboard/turn-ping: 세션 훅이 대화 턴 경계를 보고한다(이슈 #123, 대시보드 스피너 소프트
/// 신호). body = {"agent": "<세션 uuid>", "phase": "start"|"end"}. start=UserPromptSubmit(백그라운드
/// wake 포함 - human-ping과 달리 ★ 판정이 아니라 "지금 생성 중" 표시라 wake 턴도 대상), end=Stop 훅.
/// 게이트는 human-ping과 동일(loopback 무조건 + 원격 Bearer). 미등록 uuid는 no-op 200(턴 신호는
/// 인메모리 전용이라 선기록 가치가 없고, 다음 스캔 등록 후 턴부터 잡힌다 - FN 수용).
#[cfg(feature = "serve")]
pub(super) async fn dashboard_turn_ping_handler(
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
    struct TurnPingReq {
        agent: String,
        phase: String,
    }
    let req: TurnPingReq = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                axum::http::StatusCode::BAD_REQUEST,
                format!("잘못된 요청: {e}"),
            )
                .into_response();
        }
    };
    if req.phase != "start" && req.phase != "end" {
        return (
            axum::http::StatusCode::BAD_REQUEST,
            "phase는 start|end만 허용".to_string(),
        )
            .into_response();
    }
    tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        if req.phase == "end" {
            store.record_turn_end(&req.agent);
        } else if let Ok(now) = store.now() {
            // now() 실패 시 기록 생략(human-ping과 동일 방침: 빈 타임스탬프 오염 방지).
            store.record_turn_start(&req.agent, &now);
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
pub(super) async fn dashboard_deregister_handler(
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
    // spawn_blocking JoinError(내부 패닉/취소)를 "미등록(404)"로 위장하지 않고 500으로 표면화한다
    // (fail-visible: health·roster 핸들러와 동일 원칙). 정상 Ok(bool) 경로만 등록 여부로 200/404를 가른다.
    match tokio::task::spawn_blocking(move || {
        let store = store.lock().unwrap_or_else(|e| e.into_inner());
        store.deregister_agent(&req.agent)
    })
    .await
    {
        Ok(true) => (axum::http::StatusCode::OK, "ok").into_response(),
        Ok(false) => {
            // 미등록(이미 제거됐거나 무장 안 됨). 훅은 성패에 무관하게 통과한다.
            (
                axum::http::StatusCode::NOT_FOUND,
                "미등록 세션(이미 제거됨)",
            )
                .into_response()
        }
        Err(e) => {
            eprintln!("[dashboard/deregister] spawn_blocking 실패: {e}");
            (
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "등록해제 처리에 실패했습니다.",
            )
                .into_response()
        }
    }
}

/// POST /dashboard/goal: 로컬(loopback) 총감독이 선택한 감독들에게 목표를 던진다(대상마다 1 task).
/// 원격(비-loopback)은 read-only 관전이라 403. 무인증이지만 loopback 신뢰라 로컬 한정 write.
/// body = {"text": "...", "targets": ["uuid", ...]}. 응답 = {"created":[{taskId,toAgent}], "errors":[...]}.
#[cfg(feature = "serve")]
pub(super) async fn dashboard_goal_handler(
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
        // 같은 uuid가 targets에 중복되면 task를 여러 번 만들지 않는다(체크박스 더블클릭 등 프론트
        // 중복 제출 방어). 처음 본 순서만 처리하고, 재등장은 task 생성 없이 errors에 기록한다.
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for uuid in &req.targets {
            if !seen.insert(uuid.as_str()) {
                errors.push(format!("{uuid}: 대상이 중복되었습니다."));
                continue;
            }
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
