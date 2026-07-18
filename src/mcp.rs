// tunaRound MCP 서버: 검색·전사·A2A task·에이전트 레지스트리 툴을 노출한다.
// 툴 정의는 토픽별 자식 서브모듈(mcp/{search,tasks,registry}.rs)에 named tool_router로 나뉘고,
// 여기서 tool_router()가 그 라우터들을 합성한다(v2-52 분리). 종결 task 색인은 mcp/indexing.rs.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use rmcp::{
    ServerHandler, ServiceExt,
    handler::server::router::tool::ToolRouter,
    model::{ServerCapabilities, ServerInfo},
    tool_handler,
};

use crate::orchestrator::{ContextRetriever, RosterSeat, TranscriptReader, TranscriptWriter};
use crate::store::a2a::TaskState;
use crate::store::agents::{AGENT_TTL_SECS, parse_tags};
use crate::store::sqlite::SqliteStore;

mod indexing;
// server.rs가 `crate::mcp::backfill_unindexed_terminal_tasks` 경로로 호출하므로 재노출(경로 유지).
pub(crate) use indexing::backfill_unindexed_terminal_tasks;
// #[tool] 메서드를 토픽별 서브모듈로 분리하고 named tool_router를 합성한다(v2-52 분리. 개수는
// 서브모듈들이 진실이라 여기 세지 않는다 - 수치 주석은 드리프트만 낳았다).
mod discussion;
mod registry;
mod search;
mod tasks;

#[cfg(feature = "serve")]
mod server;
#[cfg(feature = "serve")]
pub use server::{serve_http_mcp_on_listener, start_http_mcp_server};

mod params;
pub use params::*;
mod format;
pub use format::*;

/// rmcp MCP 서버 핸들러. ContextRetriever를 감싸 search_context/read_transcript 툴을 노출한다.
#[derive(Clone)]
pub struct TunaSearchServer {
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
    retriever: Arc<dyn ContextRetriever>,
    reader: Option<Arc<dyn TranscriptReader>>,
    writer: Option<Arc<dyn TranscriptWriter>>,
    roster: Option<Vec<RosterSeat>>,
    /// session_id 파라미터 생략 시 기본으로 사용할 세션 id.
    default_session: String,
    /// A2A task 저장소(inbox 툴 poll_tasks/claim_task/complete_task 전용). None이면 세 툴 모두 비활성
    /// 안내 텍스트를 반환한다(stdio mcp-search 경로처럼 A2A가 배선되지 않은 경우).
    a2a_store: Option<Arc<Mutex<SqliteStore>>>,
    /// mesh 토론 레지스트리(v2-56 start/stop_discussion 전용, 브로커 serve 경로만 배선). None이면
    /// 두 툴 모두 비활성 안내 텍스트를 반환한다.
    discussions: Option<Arc<crate::discussion::DiscussionRegistry>>,
}

impl TunaSearchServer {
    /// retriever Arc를 받아 새 서버 인스턴스를 반환한다(reader/writer/a2a_store=None, default_session="default", 기존 시그니처 유지).
    pub fn new(retriever: Arc<dyn ContextRetriever>) -> Self {
        Self {
            tool_router: Self::tool_router(),
            retriever,
            reader: None,
            writer: None,
            roster: None,
            default_session: "default".to_string(),
            a2a_store: None,
            discussions: None,
        }
    }

    /// 전사 리더를 연결한 빌더 메서드(기존 new 시그니처 무영향).
    pub fn with_transcript_reader(mut self, reader: Arc<dyn TranscriptReader>) -> Self {
        self.reader = Some(reader);
        self
    }

    /// 전사 writer를 연결한 빌더 메서드(post_turn 활성화).
    pub fn with_transcript_writer(mut self, writer: Arc<dyn TranscriptWriter>) -> Self {
        self.writer = Some(writer);
        self
    }

    /// 로스터 스냅샷을 연결한 빌더 메서드(get_roster 활성화).
    pub fn with_roster(mut self, roster: Vec<RosterSeat>) -> Self {
        self.roster = Some(roster);
        self
    }

    /// session_id 파라미터 생략 시 사용할 기본 세션 id를 설정한다.
    pub fn with_default_session(mut self, session: String) -> Self {
        self.default_session = session;
        self
    }

    /// A2A task 저장소를 연결한 빌더 메서드(poll_tasks/claim_task/complete_task 활성화). 호출자가 이미
    /// 들고 있는 Arc를 그대로 넘겨 같은 mutex를 공유한다(새 SQLite 커넥션을 열지 않음).
    pub fn with_a2a_store(mut self, store: Arc<Mutex<SqliteStore>>) -> Self {
        self.a2a_store = Some(store);
        self
    }

    /// mesh 토론 레지스트리를 연결한 빌더 메서드(start/stop_discussion 활성화, v2-56). driver가
    /// a2a_store·writer를 함께 쓰므로 둘이 배선된 serve 경로에서만 의미가 있다.
    pub fn with_discussions(
        mut self,
        registry: Arc<crate::discussion::DiscussionRegistry>,
    ) -> Self {
        self.discussions = Some(registry);
        self
    }
}

impl TunaSearchServer {
    /// 네 서브모듈(search/tasks/registry/discussion)의 named tool_router를 `+`로 합성해 전체 툴
    /// 라우터를 만든다. #[tool_handler] impl ServerHandler가 기본 라우터로 이 연관 함수를 호출한다
    /// (rmcp 1.8 규약).
    pub(crate) fn tool_router() -> ToolRouter<Self> {
        Self::search_router()
            + Self::tasks_router()
            + Self::registry_router()
            + Self::discussion_router()
    }
}

#[tool_handler]
impl ServerHandler for TunaSearchServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_instructions(
                "토론 맥락을 검색하려면 search_context(query)를, 전사 통째를 읽으려면 read_transcript(session_id?, max_turns?)를 호출하세요. \
                 작업을 맡기는 쪽(dispatcher)은 send_task(from_agent, to_agent 또는 to_selector, text, context_id?)로 위임하고 get_task(task_id)로 결과를 확인하세요. \
                 작업을 받는 쪽(worker)은 poll_tasks(agent)로 확인하고 claim_task(task_id)로 착수, complete_task(task_id, result)로 완료를 보고하세요. \
                 워커/세션은 register_agent(uuid, tags?, display_name?)로 로스터에 등록하고 heartbeat(uuid)로 주기 갱신하며, \
                 dispatcher는 list_agents(selector?)로 online 에이전트를 발견합니다. \
                 머신당 presence 스캐너는 report_presence(machine, sessions)로 라이브 세션 전집합을 일괄 동기화합니다(v2-44). \
                 브로커 운영자는 tasks()로 전체 열린 task를 미배달(no-consumer?)/고착(stuck?) 주석과 함께 조망할 수 있습니다. \
                 mesh 토론(v2-56)은 start_discussion(topic, seats, rounds?)으로 시작하고 stop_discussion(discussion_id)으로 중단합니다(전사=debate:<id> 세션)."
                    .to_string(),
            )
    }
}

/// bind 주소 문자열(`host:port`)에서 와일드카드 host(0.0.0.0 / :: / [::])를 loopback(127.0.0.1)으로
/// 치환한 base URL("http://host:port", 경로 접미사 없음)을 만든다. core_local_url(/mcp)과
/// a2a_server의 Agent Card(/a2a) 양쪽이 이 매핑을 공유한다.
#[cfg(feature = "serve")]
fn local_base_url(addr: &str) -> String {
    // 마지막 ':'로 host/port 분리. IPv6 "[::]:8771"도 마지막 ':'가 port 앞이라 host="[::]"가 된다.
    let (host, port) = match addr.rsplit_once(':') {
        Some((h, p)) => (h, p),
        None => return format!("http://{addr}"), // 포트 없음(비정상): 그대로 감싼다.
    };
    let host = match host {
        "0.0.0.0" | "::" | "[::]" => "127.0.0.1",
        other => other,
    };
    format!("http://{host}:{port}")
}

/// 코어 bind 주소를 로컬 좌석이 접속할 HTTP MCP URL로 변환한다(`/mcp` 접미사).
#[cfg(feature = "serve")]
pub fn core_local_url(addr: &str) -> String {
    format!("{}/mcp", local_base_url(addr))
}

/// 코어 bind 주소를 A2A JSON-RPC 엔드포인트 URL로 변환한다(`/a2a` 접미사). Agent Card의 `url` 필드에 쓴다.
#[cfg(feature = "serve")]
pub fn core_a2a_url(addr: &str) -> String {
    format!("{}/a2a", local_base_url(addr))
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
    use crate::orchestrator::Utterance;
    use crate::store::a2a::{Task, TaskState};
    use rmcp::handler::server::wrapper::Parameters;

    struct FakeRetriever(Vec<Utterance>);

    impl crate::orchestrator::ContextRetriever for FakeRetriever {
        fn retrieve(&self, _query: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn search_context_delegates_and_returns_ok() {
        let hits = vec![Utterance {
            speaker: "claude/proposer".into(),
            content: "검색 시스템 설계".into(),
            abstraction: None,
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

    #[test]
    fn terminal_index_payload_extracts_request_and_result() {
        use crate::store::a2a::{Artifact, Message, Part};
        let req = Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part {
                text: Some("피드백 3건 정리해줘".into()),
                ..Default::default()
            }],
            task_id: None,
            context_id: None,
        };
        // completed: 요청=history[0], 결과=artifact, 결과 speaker=runner.
        let mut done = Task::new("t1", None, "dashboard", "mac-claude", "2026-07-11 09:00:00");
        done.state = TaskState::Completed;
        done.history = vec![req.clone()];
        done.runner = Some("claude".into());
        done.artifacts = vec![Artifact {
            artifact_id: "a1".into(),
            name: None,
            parts: vec![Part {
                text: Some("정리 결과".into()),
                ..Default::default()
            }],
        }];
        let p = indexing::build_terminal_index_payload(&done).unwrap();
        assert_eq!(p.request_text.as_deref(), Some("피드백 3건 정리해줘"));
        assert_eq!(p.result_text.as_deref(), Some("정리 결과"));
        assert_eq!(p.from_agent, "dashboard");
        assert_eq!(p.runner.as_deref(), Some("claude"));
        // failed: 결과=status_message(실패 사유).
        let mut fail = Task::new("t2", None, "dashboard", "mac-claude", "2026-07-11 09:00:00");
        fail.state = TaskState::Failed;
        fail.history = vec![req.clone()];
        fail.status_message = Some(Message {
            message_id: "m2".into(),
            role: "agent".into(),
            parts: vec![Part {
                text: Some("BLOCKED: 자료 없음".into()),
                ..Default::default()
            }],
            task_id: None,
            context_id: None,
        });
        assert_eq!(
            indexing::build_terminal_index_payload(&fail)
                .unwrap()
                .result_text
                .as_deref(),
            Some("BLOCKED: 자료 없음")
        );
        // 결과 없어도 요청만 있으면 색인 대상(적대 리뷰: prune이 미색인 요청을 지우지 않게).
        let mut req_only = Task::new("t2b", None, "d", "m", "2026-07-11 09:00:00");
        req_only.state = TaskState::Completed;
        req_only.history = vec![req.clone()]; // artifact 없음.
        let ro = indexing::build_terminal_index_payload(&req_only).unwrap();
        assert_eq!(ro.request_text.as_deref(), Some("피드백 3건 정리해줘"));
        assert_eq!(
            ro.result_text, None,
            "결과 없음이어도 payload는 Some(요청 색인용)"
        );
        // canceled·열린 task만 None(색인 비대상).
        let mut cancel = Task::new("t3", None, "d", "m", "2026-07-11 09:00:00");
        cancel.state = TaskState::Canceled;
        assert!(indexing::build_terminal_index_payload(&cancel).is_none());
    }

    #[test]
    fn backfill_stamps_result_less_terminal_to_converge() {
        // 결과·요청 텍스트가 전혀 없는 종결(레거시)도 백필이 스탬프해 매 기동 재스캔(비수렴)을 끊어야 한다.
        // (요청만 있으면 색인되어 스탬프되고, 아무것도 없으면 색인 없이 스탬프 - 둘 다 수렴.)
        struct FakeWriter;
        impl crate::orchestrator::TranscriptWriter for FakeWriter {
            fn append_turn(&self, _s: &str, _sp: &str, _c: &str) -> Result<u64, String> {
                Ok(0)
            }
        }
        let db = SqliteStore::open_memory().unwrap();
        // completed인데 artifact 없음 → build_terminal_index_payload가 None.
        let mut t = Task::new("t1", None, "win", "mac", "2026-07-11 09:00:00");
        t.state = TaskState::Completed;
        db.create_task(&t).unwrap();
        assert_eq!(db.list_unindexed_terminal_tasks().unwrap().len(), 1);
        let a2a = Arc::new(Mutex::new(db));
        let writer: Arc<dyn TranscriptWriter> = Arc::new(FakeWriter);
        backfill_unindexed_terminal_tasks(&a2a, &writer);
        assert_eq!(
            a2a.lock()
                .unwrap()
                .list_unindexed_terminal_tasks()
                .unwrap()
                .len(),
            0,
            "결과 없는 종결도 스탬프돼 재스캔 목록에서 빠짐(수렴)"
        );
        // 재백필도 no-op(수렴 유지).
        backfill_unindexed_terminal_tasks(&a2a, &writer);
        assert_eq!(
            a2a.lock()
                .unwrap()
                .list_unindexed_terminal_tasks()
                .unwrap()
                .len(),
            0
        );
    }

    /// 공유 파일 DB로 실제 writer(store3)·a2a_store(store4) 별개 연결을 재현한다(인메모리는 연결마다
    /// 사설 DB라 공유 불가). 반환 cleanup은 writer·a2a drop 후 호출해야 파일 잠금이 풀린다(Windows).
    fn shared_file_backends(
        tag: &str,
    ) -> (
        Arc<dyn TranscriptWriter>,
        Arc<Mutex<SqliteStore>>,
        Vec<String>,
    ) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static SEQ: AtomicU64 = AtomicU64::new(0);
        let uniq = format!(
            "{tag}_{}_{}",
            std::process::id(),
            SEQ.fetch_add(1, Ordering::Relaxed)
        );
        let path = std::env::temp_dir().join(format!("tuna_index_{uniq}.db"));
        let p = path.to_str().unwrap().to_string();
        let sidecars: Vec<String> = ["", "-wal", "-shm"]
            .iter()
            .map(|s| format!("{p}{s}"))
            .collect();
        for f in &sidecars {
            let _ = std::fs::remove_file(f);
        }
        let a2a = Arc::new(Mutex::new(
            SqliteStore::open(&p).unwrap().with_task_events(),
        ));
        let writer: Arc<dyn TranscriptWriter> =
            Arc::new(crate::store::retriever::SqliteTranscriptWriter::new(
                SqliteStore::open(&p).unwrap(),
                Box::new(|t: &str| t.to_string()),
            ));
        (writer, a2a, sidecars)
    }

    fn session_message_count(a2a: &Arc<Mutex<SqliteStore>>, sid: &str) -> usize {
        a2a.lock()
            .unwrap()
            .load_session(sid)
            .unwrap()
            .map(|s| s.messages.len())
            .unwrap_or(0)
    }

    fn payload(task_id: &str) -> indexing::TerminalIndexPayload {
        indexing::TerminalIndexPayload {
            task_id: task_id.to_string(),
            from_agent: "win".to_string(),
            to_agent: "mac".to_string(),
            runner: None,
            request_text: Some("요청문".to_string()),
            result_text: Some("결과문".to_string()),
        }
    }

    #[test]
    fn index_terminal_task_idempotent_on_shared_store() {
        // delete-then-append 멱등성: 같은 task를 순차 두 번 색인해도 정확히 req+res 2개만 남는다
        // (크래시·백필 재색인 안전망). 세션25 리팩토링이 이 불변식을 깨지 않았는지 회귀 방어.
        let (writer, a2a, sidecars) = shared_file_backends("idem");
        let p = payload("idem-1");
        indexing::index_terminal_task(&writer, &a2a, &p);
        indexing::index_terminal_task(&writer, &a2a, &p);
        assert_eq!(
            session_message_count(&a2a, "a2a:idem-1"),
            2,
            "재색인해도 중복 없이 정확히 req+res"
        );
        drop(writer);
        drop(a2a);
        for f in &sidecars {
            let _ = std::fs::remove_file(f);
        }
    }

    #[test]
    fn concurrent_index_same_task_no_duplicate_turns() {
        // 직렬화 검증(이번 세션 fix): backfill(기동) vs live(종결)가 같은 sid를 동시 색인해도 색인 전체가
        // a2a_store 락 하나로 직렬화되어 delete-then-append가 인터리빙되지 않는다 → 항상 정확히 req+res 2개.
        // 직렬화가 없으면 delete·delete·append·append 인터리빙으로 중복(최대 4)이 생길 수 있다. fix가 있으면
        // 결정적으로 2를 통과하고, 되돌리면 지배적 인터리빙으로 실패한다(단 fail 방향은 확률적이라 40회 반복해
        // 스케줄 다양성으로 회귀 노출을 높인다).
        let (writer, a2a, sidecars) = shared_file_backends("race");
        for i in 0..40u64 {
            let task_id = format!("race-{i}");
            let sid = format!("a2a:{task_id}");
            let p = payload(&task_id);
            std::thread::scope(|s| {
                s.spawn(|| indexing::index_terminal_task(&writer, &a2a, &p));
                s.spawn(|| indexing::index_terminal_task(&writer, &a2a, &p));
            });
            assert_eq!(
                session_message_count(&a2a, &sid),
                2,
                "iter {i}: 동시 색인이 중복을 만들면 안 됨(정확히 req+res)"
            );
        }
        drop(writer);
        drop(a2a);
        for f in &sidecars {
            let _ = std::fs::remove_file(f);
        }
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
        fn read_transcript(
            &self,
            _session_id: &str,
            _max_turns: Option<usize>,
        ) -> Result<Vec<Utterance>, String> {
            Ok(self.0.clone())
        }
    }

    #[tokio::test]
    async fn read_transcript_with_reader_returns_content() {
        let utts = vec![
            Utterance {
                speaker: "claude/proposer".into(),
                content: "첫 번째 발언".into(),
                abstraction: None,
            },
            Utterance {
                speaker: "codex/reviewer".into(),
                content: "두 번째 발언".into(),
                abstraction: None,
            },
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
        assert!(
            text.contains("첫 번째 발언"),
            "전사 내용이 포함되어야 함: {text}"
        );
        assert!(
            text.contains("두 번째 발언"),
            "전사 내용이 포함되어야 함: {text}"
        );
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
        assert!(
            text.contains("전사 리더 미연결"),
            "reader=None 안내 불일치: {text}"
        );
    }

    /// session_id를 캡처해 검증하는 전사 리더.
    struct CapturingTranscriptReader {
        captured: std::sync::Mutex<Option<String>>,
        utts: Vec<Utterance>,
    }

    impl CapturingTranscriptReader {
        fn new(utts: Vec<Utterance>) -> Self {
            Self {
                captured: std::sync::Mutex::new(None),
                utts,
            }
        }
        fn last_session_id(&self) -> Option<String> {
            self.captured.lock().unwrap().clone()
        }
    }

    impl crate::orchestrator::TranscriptReader for CapturingTranscriptReader {
        fn read_transcript(
            &self,
            session_id: &str,
            _max_turns: Option<usize>,
        ) -> Result<Vec<Utterance>, String> {
            *self.captured.lock().unwrap() = Some(session_id.to_string());
            Ok(self.utts.clone())
        }
    }

    #[tokio::test]
    async fn read_transcript_without_session_id_uses_default_session() {
        // session_id 파라미터 생략 시 default_session이 TranscriptReader에 전달된다.
        let capturing = Arc::new(CapturingTranscriptReader::new(vec![Utterance {
            speaker: "claude".into(),
            content: "안녕".into(),
            abstraction: None,
        }]));
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_reader(
                capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>
            )
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
            .with_transcript_reader(
                capturing.clone() as Arc<dyn crate::orchestrator::TranscriptReader>
            )
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

    /// append_turn 인자를 캡처하는 가짜 writer.
    struct CapturingWriter {
        captured: std::sync::Mutex<Option<(String, String, String)>>,
    }

    impl crate::orchestrator::TranscriptWriter for CapturingWriter {
        fn append_turn(
            &self,
            session_id: &str,
            speaker: &str,
            content: &str,
        ) -> Result<u64, String> {
            *self.captured.lock().unwrap() = Some((
                session_id.to_string(),
                speaker.to_string(),
                content.to_string(),
            ));
            Ok(7)
        }
    }

    #[tokio::test]
    async fn post_turn_with_writer_appends_and_uses_default_session() {
        let writer = Arc::new(CapturingWriter {
            captured: std::sync::Mutex::new(None),
        });
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
                .with_transcript_writer(
                    writer.clone() as Arc<dyn crate::orchestrator::TranscriptWriter>
                )
                .with_default_session("sess-d".to_string());
        let result = server
            .post_turn(Parameters(PostTurnParams {
                session_id: None, // 생략 → default_session.
                speaker: "claude/proposer".into(),
                content: "원격 발언".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("msg_id=7"), "새 id 안내 불일치: {text}");
        let cap = writer.captured.lock().unwrap().clone();
        assert_eq!(
            cap,
            Some((
                "sess-d".into(),
                "claude/proposer".into(),
                "원격 발언".into()
            ))
        );
    }

    #[tokio::test]
    async fn post_turn_without_writer_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .post_turn(Parameters(PostTurnParams {
                session_id: None,
                speaker: "x".into(),
                content: "y".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("writer 미연결"), "미연결 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn post_turn_writer_error_returns_is_error_true() {
        // R1 계약(이번 세션 fix): append_turn 실패는 success 위장이 아니라 isError=true라야 클라
        // (mcp_client.rs의 isError 검사)가 성공으로 오인하지 않는다(형제 툴 search_context·claim/
        // complete/fail·registry와 동일 계약). "writer 미연결"(위 테스트)은 미배선이라 success 유지.
        struct FailingWriter;
        impl crate::orchestrator::TranscriptWriter for FailingWriter {
            fn append_turn(&self, _s: &str, _sp: &str, _c: &str) -> Result<u64, String> {
                Err("DB 잠김".into())
            }
        }
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_transcript_writer(
                Arc::new(FailingWriter) as Arc<dyn crate::orchestrator::TranscriptWriter>
            )
            .with_default_session("sess-e".to_string());
        let result = server
            .post_turn(Parameters(PostTurnParams {
                session_id: None,
                speaker: "claude/proposer".into(),
                content: "쓰기 실패 유발".into(),
            }))
            .await
            .unwrap();
        assert_eq!(
            result.is_error,
            Some(true),
            "쓰기 실패인데 isError=true가 아님"
        );
        let text = format!("{:?}", result.content);
        assert!(text.contains("추가 실패"), "실패 사유 노출 불일치: {text}");
    }

    #[tokio::test]
    async fn get_roster_lists_seats() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_roster(vec![
            RosterSeat {
                engine: "claude".into(),
                role: Some("proposer".into()),
            },
            RosterSeat {
                engine: "codex".into(),
                role: None,
            },
        ]);
        let result = server
            .get_roster(Parameters(RosterParams { session_id: None }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("claude (proposer)"),
            "좌석 표기 불일치: {text}"
        );
        assert!(text.contains("codex"), "역할 없는 좌석 누락: {text}");
    }

    #[tokio::test]
    async fn get_roster_without_roster_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .get_roster(Parameters(RosterParams { session_id: None }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("로스터 미연결"), "미연결 안내 불일치: {text}");
    }

    // --- A2A inbox 툴(poll_tasks/claim_task/complete_task): 순수 함수 단위테스트 ---

    /// task 하나를 심고 store에 영속한다(inbox 테스트 공용 헬퍼).
    fn seed_task(store: &SqliteStore, id: &str, from: &str, to: &str, created_at: &str) {
        let task = crate::store::a2a::Task::new(id, None, from, to, created_at);
        store.create_task(&task).unwrap();
    }

    // --- A2A inbox 툴: MCP 계층(#[tool] 메서드) 테스트 ---

    /// in-memory store를 a2a_store로 연결한 서버를 만든다(inbox 툴 3종 테스트 공용).
    fn server_with_a2a_store(store: SqliteStore) -> TunaSearchServer {
        TunaSearchServer::new(Arc::new(FakeRetriever(vec![])))
            .with_a2a_store(Arc::new(Mutex::new(store)))
    }

    #[tokio::test]
    async fn poll_tasks_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .poll_tasks(Parameters(PollTasksParams {
                agent: "mac".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn claim_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: None,
                runner: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn complete_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "결과".into(),
                agent: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn poll_tasks_tool_returns_open_tasks_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let server = server_with_a2a_store(store);
        let result = server
            .poll_tasks(Parameters(PollTasksParams {
                agent: "mac".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("t1"), "task 목록 누락: {text}");
    }

    #[tokio::test]
    async fn claim_task_tool_transitions_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());
        let result = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: None,
                runner: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("state=working"), "응답 불일치: {text}");
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[tokio::test]
    async fn complete_task_tool_completes_via_server() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: None,
                runner: None,
            }))
            .await
            .unwrap();
        let result = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "완료 보고".into(),
                agent: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("state=completed"), "응답 불일치: {text}");
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Completed);
        assert_eq!(
            reloaded.artifacts[0].parts[0].text.as_deref(),
            Some("완료 보고")
        );
    }

    #[tokio::test]
    async fn claim_task_tool_missing_task_returns_error_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "nope".into(),
                agent: None,
                runner: None,
            }))
            .await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        // R1: 내부 실패는 isError=true라야 클라이언트가 성공으로 오인하지 않는다.
        assert_eq!(call_result.is_error, Some(true), "isError=true여야 함");
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("착수 실패"), "에러 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn complete_task_tool_missing_task_returns_error_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "nope".into(),
                result: "결과".into(),
                agent: None,
            }))
            .await;
        assert!(result.is_ok());
        let call_result = result.unwrap();
        assert_eq!(call_result.is_error, Some(true), "isError=true여야 함");
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("완료 처리 실패"), "에러 안내 불일치: {text}");
    }

    #[tokio::test]
    async fn claim_task_tool_already_working_returns_is_error_true() {
        // R2 전이충돌(이미 claim된 task를 다시 claim)이 R1 에러계약(isError=true)으로 정직하게 드러나는지
        // 확인한다(두 리팩토링이 맞물리는 지점). 클라이언트(McpHttpClient::parse_jsonrpc_sse)는 이
        // isError를 보고 Err로 매핑하고, 워커(run_one_pass)는 claim 실패로 보고 러너를 안 돌린다.
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        // 첫 claim은 성공(submitted -> working).
        let first = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: None,
                runner: None,
            }))
            .await;
        assert!(first.is_ok());
        assert_eq!(
            first.unwrap().is_error,
            Some(false),
            "첫 claim은 성공이어야 함"
        );

        // 둘째 claim(동시 착수 경쟁 시뮬레이션): 이미 working이라 전이충돌 -> isError=true.
        let second = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: None,
                runner: None,
            }))
            .await;
        assert!(
            second.is_ok(),
            "MCP 레벨에서는 항상 Ok(CallToolResult)를 반환해야 함"
        );
        let call_result = second.unwrap();
        assert_eq!(
            call_result.is_error,
            Some(true),
            "전이충돌인데 isError=true가 아님"
        );
        let text = format!("{:?}", call_result.content);
        assert!(text.contains("착수 실패"), "에러 안내 불일치: {text}");

        // 상태는 여전히 working으로 유지돼야 한다(둘째 호출이 조용히 성공한 것처럼 보이면 안 됨).
        let reloaded = a2a.lock().unwrap().get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[tokio::test]
    async fn complete_task_tool_first_completer_wins_rejects_mismatched_agent() {
        // MCP 툴 계층까지 agent 배선이 실제로 first-completer-wins를 강제하는지 확인한다(claim_task의
        // agent가 claimed_by로 기록되고, complete_task의 agent가 불일치하면 거부되어야 함).
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let a2a = Arc::new(Mutex::new(store));
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        let claim = server
            .claim_task(Parameters(ClaimTaskParams {
                task_id: "t1".into(),
                agent: Some("worker-a".into()),
                runner: None,
            }))
            .await;
        assert_eq!(
            claim.unwrap().is_error,
            Some(false),
            "claim은 성공이어야 함"
        );

        // 되살아난(stale) worker-b가 completer로 완료 보고 -> 거부(isError=true).
        let mismatched = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "worker-b의 결과".into(),
                agent: Some("worker-b".into()),
            }))
            .await;
        let mismatched = mismatched.unwrap();
        assert_eq!(
            mismatched.is_error,
            Some(true),
            "completer 불일치인데 isError=true가 아님"
        );
        assert_eq!(
            a2a.lock().unwrap().get_task("t1").unwrap().unwrap().state,
            TaskState::Working,
            "거부 후에도 여전히 working이어야 함"
        );

        // claim한 본인(worker-a)이 completer면 성공.
        let matched = server
            .complete_task(Parameters(CompleteTaskParams {
                task_id: "t1".into(),
                result: "worker-a의 결과".into(),
                agent: Some("worker-a".into()),
            }))
            .await;
        assert_eq!(
            matched.unwrap().is_error,
            Some(false),
            "본인 completer는 성공해야 함"
        );
        assert_eq!(
            a2a.lock().unwrap().get_task("t1").unwrap().unwrap().state,
            TaskState::Completed
        );
    }

    // --- A2A dispatcher 툴: MCP 계층(#[tool] 메서드) 테스트 ---

    #[tokio::test]
    async fn send_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .send_task(Parameters(SendTaskParams {
                from_agent: "win".into(),
                to_agent: Some("mac".into()),
                text: "부탁".into(),
                context_id: None,
                to_selector: None,
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn get_task_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server
            .get_task(Parameters(GetTaskParams {
                task_id: "t1".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn send_task_tool_creates_task_via_server_and_get_task_reads_it_back() {
        let store = SqliteStore::open_memory().unwrap();
        let a2a = Arc::new(Mutex::new(store));
        let server =
            TunaSearchServer::new(Arc::new(FakeRetriever(vec![]))).with_a2a_store(a2a.clone());

        let send_result = server
            .send_task(Parameters(SendTaskParams {
                from_agent: "win-claude".into(),
                to_agent: Some("mac-claude".into()),
                text: "리뷰 부탁".into(),
                context_id: None,
                to_selector: None,
            }))
            .await;
        assert!(send_result.is_ok());
        let send_text = format!("{:?}", send_result.unwrap().content);
        assert!(
            send_text.contains("state=submitted"),
            "send_task 응답 불일치: {send_text}"
        );

        // 실제로 mac-claude 앞으로 열린 task가 생겼는지 store에서 직접 확인.
        let seeded_id = {
            let s = a2a.lock().unwrap();
            let tasks = s.list_open_tasks_for("mac-claude").unwrap();
            assert_eq!(tasks.len(), 1);
            tasks[0].id.clone()
        };

        // get_task 툴로도 같은 task를 읽을 수 있어야 한다(dispatcher가 보내고 확인하는 왕복).
        let get_result = server
            .get_task(Parameters(GetTaskParams {
                task_id: seeded_id.clone(),
            }))
            .await;
        assert!(get_result.is_ok());
        let get_text = format!("{:?}", get_result.unwrap().content);
        assert!(
            get_text.contains(&seeded_id),
            "get_task 응답에 task_id 없음: {get_text}"
        );
        assert!(
            get_text.contains("state=submitted"),
            "get_task 응답 불일치: {get_text}"
        );
    }

    #[tokio::test]
    async fn get_task_tool_missing_task_returns_not_found_text() {
        let store = SqliteStore::open_memory().unwrap();
        let server = server_with_a2a_store(store);
        let result = server
            .get_task(Parameters(GetTaskParams {
                task_id: "nope".into(),
            }))
            .await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("없음"), "미존재 안내 불일치: {text}");
    }

    // --- tasks 툴(운영자 전역 조망): MCP 계층(#[tool] 메서드) 테스트 ---

    #[tokio::test]
    async fn tasks_without_store_returns_not_connected() {
        let server = TunaSearchServer::new(Arc::new(FakeRetriever(vec![])));
        let result = server.tasks(Parameters(ListTasksParams {})).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(
            text.contains("A2A task 저장소 미구성"),
            "미구성 안내 불일치: {text}"
        );
    }

    #[tokio::test]
    async fn tasks_tool_lists_tasks_across_agents() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        seed_task(&store, "t2", "win", "codex", "2026-07-02 09:05:00");
        let server = server_with_a2a_store(store);
        let result = server.tasks(Parameters(ListTasksParams {})).await;
        assert!(result.is_ok());
        let text = format!("{:?}", result.unwrap().content);
        assert!(text.contains("t1"), "t1 누락: {text}");
        assert!(text.contains("t2"), "t2 누락: {text}");
    }
}
