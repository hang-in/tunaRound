// 대시보드 read-only JSON API: roster·health·presence-timeline·search 핸들러.

use super::*;

/// GET /dashboard/roster: online 감독 roster(list_agents, 빈 selector = 전체) JSON. 브라우저가 주기 폴.
/// axum "json" 피처(신규 의존) 없이 serde_json(기존 의존)만으로 application/json 응답을 만든다.
#[cfg(feature = "serve")]
pub(super) async fn dashboard_roster_handler(
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
        // health 핸들러와 동일 원칙: 시각 조회 실패를 기본값(UNIX epoch 등)으로 위장해 정상 200
        // 로스터처럼 보이게 하지 않고 500으로 표면화한다(fail-visible).
        let now = store.now()?;
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
                // busy = A2A task 처리 중(fresh working, #94) OR 대화 턴 처리 중(turn 신호, 이슈 #123).
                // 같은 스피너로 합친다: 사용자 관점 의미는 "지금 응답 생성 중" 하나다.
                let busy = busy.contains(&a.uuid) || crate::store::agents::is_turn_active(&a, &now);
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
pub(super) async fn dashboard_health_handler(
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
pub(super) const PRESENCE_TIMELINE_MAX: usize = 500;

/// GET /dashboard/presence-timeline?limit=&since=: presence 이벤트 이력(read-only, v2-50). 세션 등장
/// (appear)·소멸(disappear, 사유 stale|deregister)·사람입력(human_input)의 raw edge를 최신순으로 돌려준다.
/// health 핸들러 패턴(spawn_blocking + serde_json + fail-visible 500). 조회 실패를 정상 빈 배열로
/// 위장하지 않는다(관제 오판 방지). limit 기본 100·상한 PRESENCE_TIMELINE_MAX, since는 옵션(at >= since).
/// 백엔드는 raw 이벤트만 돌려주고 ★-도출(총감독 판정)은 프론트 activity.ts가 단일 소스로 유지한다.
#[cfg(feature = "serve")]
pub(super) async fn dashboard_presence_timeline_handler(
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
pub(super) async fn dashboard_search_handler(
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
