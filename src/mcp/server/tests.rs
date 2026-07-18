// server 모듈 테스트(#138 B 분해로 파일 이동, 내용 순수 이동): 게이트·핸들러 단위 + HTTP e2e.

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
        let ip: std::net::IpAddr = "203.0.113.5".parse().unwrap();
        let h = headers_with_auth(Some("Bearer tok"));
        assert!(dashboard_write_allowed(ip, &h, &Some("tok".into())));
    }

    #[test]
    fn remote_with_wrong_or_missing_bearer_denied() {
        let ip: std::net::IpAddr = "203.0.113.5".parse().unwrap();
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
        let ip: std::net::IpAddr = "203.0.113.5".parse().unwrap();
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

        let empty_targets =
            axum::body::Bytes::from(serde_json::json!({"text": "목표", "targets": []}).to_string());
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

    #[tokio::test]
    async fn duplicate_targets_create_task_once() {
        // targets에 같은 uuid가 두 번 들어오면(체크박스 더블클릭 등) task를 두 번 만들지 않는다.
        // 처음 본 것만 생성하고, 재등장은 created 없이 errors에 "중복" 사유로 기록된다.
        let addr: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        let store = test_store();
        let body = axum::body::Bytes::from(
            serde_json::json!({"text": "목표", "targets": ["dup-uuid", "dup-uuid"]}).to_string(),
        );
        let resp = dashboard_goal_handler(
            axum::extract::ConnectInfo(addr),
            axum::extract::State(store),
            axum::http::HeaderMap::new(),
            body,
        )
        .await;
        let (status, body) = read_body(resp).await;
        assert_eq!(
            status,
            axum::http::StatusCode::OK,
            "요청 자체는 성공: {body}"
        );
        let parsed: serde_json::Value = serde_json::from_str(&body).expect("JSON 응답");
        let created = parsed["created"].as_array().expect("created 배열");
        let errors = parsed["errors"].as_array().expect("errors 배열");
        assert_eq!(created.len(), 1, "task는 1건만 생성돼야 함: {body}");
        assert_eq!(errors.len(), 1, "중복분은 errors에 1건 기록돼야 함: {body}");
        assert!(
            errors[0].as_str().unwrap_or_default().contains("중복"),
            "errors 사유에 '중복'이 있어야 함: {body}"
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
        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
        let reader = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptReader>);
        let writer = Some(Arc::new(log.clone()) as Arc<dyn crate::orchestrator::TranscriptWriter>);
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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
                active_at: None,
            }
        };
        {
            let s = store.lock().unwrap();
            // 시각=실시계 now(#133: sync_presence가 매 호출 gc_presence_events(now-30d)를 돌아
            // 고정 리터럴은 보존창을 지나는 날짜에 방금 넣은 이벤트가 삭제되는 시한폭탄).
            // 세 호출이 같은 초를 공유해도 최신순 단언은 id DESC 타이브레이크로 결정적이다.
            let now = s.now().expect("now");
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
                &now,
            );
            s.mark_human_input("s1", &now);
            s.sync_presence(
                "win",
                &[up(
                    "s1",
                    "claude",
                    Some("tunaRound"),
                    Some("win-claude-tunaRound"),
                )],
                &now,
            );
        }

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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
        // 최신순(at DESC, 동시각은 id DESC): 마지막에 기록된 disappear(s2)가 배열 맨 앞.
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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

        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;

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
    let q =
        parse_dashboard_events_query("since=2026-07-11%2009%3A00%3A00&dispatcher=win-opus-boss");
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
        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
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
        let body = axum::body::Bytes::from(serde_json::json!({"agent": "ghost-uuid"}).to_string());
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

    /// turn-ping(이슈 #123): start가 로스터 turn_active_at을 세우고 end가 클리어한다.
    /// 미등록 uuid는 no-op 200, phase 오타는 400.
    #[tokio::test]
    async fn turn_ping_start_sets_and_end_clears_turn_signal() {
        let store = test_store();
        let loopback: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        let now = {
            let s = store.lock().unwrap();
            let now = s.now().unwrap();
            s.register_agent("sess-1", BTreeMap::new(), None, &now);
            now
        };
        let ping = |phase: &str, agent: &str| {
            let body = axum::body::Bytes::from(
                serde_json::json!({"agent": agent, "phase": phase}).to_string(),
            );
            dashboard_turn_ping_handler(
                axum::extract::ConnectInfo(loopback),
                axum::extract::State(store.clone()),
                axum::Extension(Arc::new(None::<String>)),
                axum::http::HeaderMap::new(),
                body,
            )
        };
        assert_eq!(
            ping("start", "sess-1").await.status(),
            axum::http::StatusCode::OK
        );
        let turn_at = |uuid: &str| {
            store
                .lock()
                .unwrap()
                .list_agents(&BTreeMap::new(), &now, i64::MAX)
                .into_iter()
                .find(|a| a.uuid == uuid)
                .and_then(|a| a.turn_active_at)
        };
        assert!(
            turn_at("sess-1").is_some(),
            "start가 turn_active_at을 세워야 함"
        );
        assert_eq!(
            ping("end", "sess-1").await.status(),
            axum::http::StatusCode::OK
        );
        assert!(
            turn_at("sess-1").is_none(),
            "end가 turn_active_at을 클리어해야 함"
        );
        // 미등록 uuid = no-op 200(인메모리 전용이라 선기록 없음).
        assert_eq!(
            ping("start", "ghost").await.status(),
            axum::http::StatusCode::OK
        );
        // phase 오타 = 400.
        assert_eq!(
            ping("pause", "sess-1").await.status(),
            axum::http::StatusCode::BAD_REQUEST
        );
    }

    /// deregister: 미등록 uuid는 404(멱등 - 이미 없거나 애초에 없던 세션도 훅은 실패 취급 안 함).
    #[tokio::test]
    async fn deregister_unregistered_uuid_is_404_idempotent() {
        let store = test_store();
        let loopback: std::net::SocketAddr = "127.0.0.1:9".parse().unwrap();
        let body =
            axum::body::Bytes::from(serde_json::json!({"agent": "never-registered"}).to_string());
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
