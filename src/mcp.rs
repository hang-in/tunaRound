// 토론 맥락 검색 MCP 서버: rmcp stdio 서버로 search_context 툴 하나를 노출한다.

use std::sync::Arc;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::{CallToolResult, Content, ServerCapabilities, ServerInfo},
    tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler, ServiceExt,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::orchestrator::{ContextRetriever, TranscriptReader, Utterance};

/// search_context 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct SearchParams {
    /// 검색 질의.
    pub query: String,
    /// 최대 결과(기본 10).
    pub limit: Option<usize>,
}

/// read_transcript 툴 파라미터.
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscriptParams {
    /// 세션 id(기본 "default").
    pub session_id: Option<String>,
    /// 마지막 N턴만(생략=전체).
    pub max_turns: Option<usize>,
}

/// rmcp MCP 서버 핸들러. ContextRetriever를 감싸 search_context/read_transcript 툴을 노출한다.
#[derive(Clone)]
pub struct TunaSearchServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    /// session_id 파라미터 생략 시 기본으로 사용할 세션 id.
    default_session: String,
}

impl TunaSearchServer {
    /// retriever Arc를 받아 새 서버 인스턴스를 반환한다(reader=None, default_session="default", 기존 시그니처 유지).
    pub fn new(retriever: Arc<dyn ContextRetriever>) -> Self {
        Self { tool_router: Self::tool_router(), retriever, reader: None, default_session: "default".to_string() }
    }

    /// 전사 리더를 연결한 빌더 메서드(기존 new 시그니처 무영향).
    pub fn with_transcript_reader(mut self, reader: Arc<dyn TranscriptReader>) -> Self {
        self.reader = Some(reader);
        self
    }

    /// session_id 파라미터 생략 시 사용할 기본 세션 id를 설정한다.
    pub fn with_default_session(mut self, session: String) -> Self {
        self.default_session = session;
        self
    }
}

#[tool_router]
impl TunaSearchServer {
    #[tool(description = "토론 맥락 검색: 과거·다른 분기의 관련 발언을 찾는다")]
    async fn search_context(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let hits: Vec<Utterance> =
            self.retriever.retrieve(&p.query, p.limit.unwrap_or(10).min(50));
        let text = if hits.is_empty() {
            "검색 결과 없음".to_string()
        } else {
            hits.iter()
                .map(|u| format!("[{}] {}", u.speaker, u.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(description = "현재 토론 전사를 읽는다(활성 경로). 검색이 아니라 통째 맥락이 필요할 때.")]
    async fn read_transcript(
        &self,
        Parameters(p): Parameters<TranscriptParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(reader) = &self.reader else {
            return Ok(CallToolResult::success(vec![Content::text(
                "전사 리더 미연결".to_string(),
            )]));
        };
        let sid = p.session_id.unwrap_or_else(|| self.default_session.clone());
        let utts = reader.read_transcript(&sid, p.max_turns);
        let text = if utts.is_empty() {
            "전사 없음".to_string()
        } else {
            utts.iter()
                .map(|u| format!("[{}] {}", u.speaker, u.content))
                .collect::<Vec<_>>()
                .join("\n\n")
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}

#[tool_handler]
impl ServerHandler for TunaSearchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "토론 맥락을 검색하려면 search_context(query)를, 전사 통째를 읽으려면 read_transcript(session_id?, max_turns?)를 호출하세요.".to_string(),
            )
    }
}

/// HTTP MCP 서버를 기동한다. serve 피처 전용.
#[cfg(feature = "serve")]
pub async fn start_http_mcp_server(
    addr: &str,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let listener = tokio::net::TcpListener::bind(addr).await?;
    serve_http_mcp_on_listener(listener, retriever, reader, token).await
}

/// 이미 바인드된 TcpListener로 HTTP MCP 서버를 서빙한다(테스트에서도 재사용).
#[cfg(feature = "serve")]
pub async fn serve_http_mcp_on_listener(
    listener: tokio::net::TcpListener,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    token: Option<String>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use axum::{
        Router,
        extract::Request,
        http::{StatusCode, header::AUTHORIZATION},
        middleware::{self, Next},
        response::IntoResponse,
    };
    use rmcp::transport::streamable_http_server::{
        StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
    };

    let retriever2 = retriever.clone();
    let reader2 = reader.clone();
    // service_factory: 요청마다 새 TunaSearchServer 인스턴스를 생성한다(Clone 불필요, Arc 공유).
    let service: StreamableHttpService<TunaSearchServer, LocalSessionManager> =
        StreamableHttpService::new(
            move || {
                let mut s = TunaSearchServer::new(retriever2.clone());
                if let Some(r) = &reader2 {
                    s = s.with_transcript_reader(r.clone());
                }
                Ok(s)
            },
            Default::default(), // Arc::new(LocalSessionManager::default())
            // 원격 에이전트 접속을 위해 호스트 제한을 해제하고 bearer 토큰으로 인증한다.
            StreamableHttpServerConfig::default().disable_allowed_hosts(),
        );

    let router: Router = if let Some(tok) = token {
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
        Router::new().nest_service("/mcp", service).layer(bearer)
    } else {
        Router::new().nest_service("/mcp", service)
    };

    let bound_addr = listener.local_addr()?;
    eprintln!("[serve-mcp] HTTP MCP 서버 기동: {bound_addr}");
    axum::serve(listener, router).await?;
    Ok(())
}

/// stdin/stdout을 전송으로 사용하는 stdio MCP 서버를 기동한다.
pub async fn start_mcp_server(
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    default_session: String,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut server = TunaSearchServer::new(retriever).with_default_session(default_session);
    if let Some(r) = reader {
        server = server.with_transcript_reader(r);
    }
    let (stdin, stdout) = rmcp::transport::io::stdio();
    let service = server.serve((stdin, stdout)).await?;
    service.waiting().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmcp::handler::server::wrapper::Parameters;

    // HTTP MCP 서버 통합 테스트: serve 피처 전용.
    #[cfg(feature = "serve")]
    mod http_serve {
        use super::super::*;

        struct NullRetriever;
        impl crate::orchestrator::ContextRetriever for NullRetriever {
            fn retrieve(&self, _q: &str, _limit: usize) -> Vec<crate::orchestrator::Utterance> {
                vec![]
            }
        }

        /// initialize 요청 본문(MCP 2025-03-26 프로토콜).
        const INIT_BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

        #[tokio::test]
        async fn http_mcp_bearer_auth() {
            // 포트 :0 으로 바인드해 OS가 빈 포트를 할당하도록 한다(포트 경합 없음).
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            let token = Some("secret-tok".to_string());

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, token).await;
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
        }

        #[tokio::test]
        async fn http_mcp_no_token_allows_all() {
            // token=None이면 미들웨어 없이 모든 요청 통과.
            let listener =
                tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind 실패");
            let port = listener.local_addr().unwrap().port();

            let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;

            tokio::spawn(async move {
                let _ = serve_http_mcp_on_listener(listener, retriever, None, None).await;
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
        }
    }

    struct FakeRetriever(Vec<Utterance>);

    impl crate::orchestrator::ContextRetriever for FakeRetriever {
        fn retrieve(&self, _query: &str, _limit: usize) -> Vec<Utterance> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn search_context_delegates_and_returns_ok() {
        let hits = vec![Utterance {
            speaker: "claude/proposer".into(),
            content: "검색 시스템 설계".into(),
        }];
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(hits)));
        let result = server
            .search_context(Parameters(SearchParams {
                query: "검색".into(),
                limit: Some(5),
            }))
            .await;
        assert!(result.is_ok(), "검색이 Ok여야 함: {result:?}");
    }

    #[tokio::test]
    async fn search_context_empty_retriever_returns_ok() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .search_context(Parameters(SearchParams {
                query: "없는내용".into(),
                limit: None,
            }))
            .await;
        assert!(result.is_ok());
    }

    /// 고정 Utterance를 반환하는 가짜 전사 리더.
    struct FakeTranscriptReader(Vec<Utterance>);

    impl crate::orchestrator::TranscriptReader for FakeTranscriptReader {
        fn read_transcript(&self, _session_id: &str, _max_turns: Option<usize>) -> Vec<Utterance> {
            self.0.clone()
        }
    }

    #[tokio::test]
    async fn read_transcript_with_reader_returns_content() {
        let utts = vec![
            Utterance { speaker: "claude/proposer".into(), content: "첫 번째 발언".into() },
            Utterance { speaker: "codex/reviewer".into(), content: "두 번째 발언".into() },
        ];
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(Arc::new(FakeTranscriptReader(utts)));
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: Some("test-session".into()),
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok(), "read_transcript가 Ok여야 함: {result:?}");
        let call_result = result.unwrap();
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("첫 번째 발언"), "전사 내용이 포함되어야 함: {text}");
        assert!(text.contains("두 번째 발언"), "전사 내용이 포함되어야 함: {text}");
    }

    #[tokio::test]
    async fn read_transcript_without_reader_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: None,
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("전사 리더 미연결"), "reader=None 안내 불일치: {text}");
    }

    /// session_id를 캡처해 검증하는 전사 리더.
    struct CapturingTranscriptReader {
        captured: std::sync::Mutex<Option<String>>,
        utts: Vec<Utterance>,
    }

    impl CapturingTranscriptReader {
        fn new(utts: Vec<Utterance>) -> Self {
            Self { captured: std::sync::Mutex::new(None), utts }
        }
        fn last_session_id(&self) -> Option<String> {
            self.captured.lock().unwrap().clone()
        }
    }

    impl crate::orchestrator::TranscriptReader for CapturingTranscriptReader {
        fn read_transcript(&self, session_id: &str, _max_turns: Option<usize>) -> Vec<Utterance> {
            *self.captured.lock().unwrap() = Some(session_id.to_string());
            self.utts.clone()
        }
    }

    #[tokio::test]
    async fn read_transcript_without_session_id_uses_default_session() {
        // session_id 파라미터 생략 시 default_session이 TranscriptReader에 전달된다.
        let capturing = Arc::new(CapturingTranscriptReader::new(vec![
            Utterance { speaker: "claude".into(), content: "안녕".into() },
        ]));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>)
            .with_default_session("session-xyz".to_string());
        let result = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: None, // 생략 → default_session 사용.
                max_turns: None,
            }))
            .await;
        assert!(result.is_ok(), "Ok여야 함: {result:?}");
        assert_eq!(
            capturing.last_session_id().as_deref(),
            Some("session-xyz"),
            "default_session이 TranscriptReader에 전달되어야 함"
        );
    }

    #[tokio::test]
    async fn read_transcript_explicit_session_id_overrides_default() {
        // session_id 명시 시 default_session이 아닌 명시 id가 사용된다.
        let capturing = Arc::new(CapturingTranscriptReader::new(vec![]));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>)
            .with_default_session("should-not-appear".to_string());
        let _ = server
            .read_transcript(Parameters(TranscriptParams {
                session_id: Some("explicit-session".into()),
                max_turns: None,
            }))
            .await;
        assert_eq!(
            capturing.last_session_id().as_deref(),
            Some("explicit-session"),
            "명시 session_id가 우선되어야 함"
        );
    }
}
