// 대시보드 SSE 피드: envelope·lagged 신호·이벤트 스트림·쿼리 파싱·/dashboard/events 핸들러.

use super::*;

/// task 스냅샷 하나를 대시보드 SSE envelope JSON 문자열로 만든다(라이브·재생 공용, v2-45 P2 §3).
/// 매핑 = state가 completed일 때만 event="completed", 그 외(failed/canceled 포함) 전부 "status"
/// (§5-2 고정 계약). completed 상태는 try_complete/complete_task 전이(=Completed 이벤트)로만
/// 도달하므로 state 기준 재구성이 라이브 버스의 variant 기준 매핑과 일치한다. 라이브 스트림과
/// 재생 스냅샷이 이 한 함수를 공유해 두 경로의 매핑이 갈라지지 않게 한다.
#[cfg(feature = "serve")]
pub(super) fn dashboard_envelope_json(task: &crate::store::a2a::Task) -> String {
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
pub(super) fn lagged_signal_json(skipped: u64) -> String {
    serde_json::json!({ "event": "lagged", "skipped": skipped }).to_string()
}

/// 전역 task 이벤트를 JSON data 문자열로 흘리는 순수 스트림(단위테스트 대상). task_id 필터 없이 모든
/// TaskEvent를 내보낸다. Lagged는 조용히 스킵하지 않고 `lagged_signal_json` 프레임으로 알린 뒤 계속,
/// Closed면 종료한다(#2: 조용히 skip하면 워터마크 소비자가 갭을 인지 못해 completed/failed가 재생에서
/// 영구 누락될 수 있다 - 서버측 최소 개선안).
#[cfg(feature = "serve")]
pub(super) fn dashboard_event_json_stream(
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
pub(super) struct DashboardEventsQuery {
    /// 최근 N건 스냅샷 선행(전 상태 포함, 피드 전용). 0=현행 유지(라이브만).
    pub(super) replay: usize,
    /// 이 시각(updated_at, DB datetime 포맷) 이후의 completed/failed만 선행(watch-results 재생 전용).
    pub(super) since: Option<String>,
    /// since와 조합해 from_agent 필터(빈 값=전체, watch-results 의미와 일치).
    pub(super) dispatcher: Option<String>,
}

/// 재생 상한(replay·since 두 경로 공통). 재생은 피드 창(최근 N건, 현재 200)용 표면이라 전 테이블 덤프
/// 수준의 N을 막는다(원격 관전자도 무인증으로 붙는 엔드포인트라 방어적 상한). since 경로는 이 상한에서
/// 잘리면 스냅샷만 보내고 스트림을 정상 종료한다(catch-up 연쇄, 핸들러 주석 참조).
#[cfg(feature = "serve")]
pub(super) const DASHBOARD_REPLAY_MAX: usize = 500;

/// application/x-www-form-urlencoded 값 디코딩('+' -> 공백, %XX -> 바이트). since의
/// "YYYY-MM-DD HH:MM:SS"가 %20/+로 인코딩되어 오는 것을 원복한다. 불완전한 %시퀀스는 그대로 둔다.
#[cfg(feature = "serve")]
pub(super) fn percent_decode(s: &str) -> String {
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
pub(super) fn parse_dashboard_events_query(query: &str) -> DashboardEventsQuery {
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
pub(super) async fn dashboard_events_handler(
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
