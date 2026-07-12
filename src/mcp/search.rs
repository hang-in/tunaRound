// 토론 맥락 검색·전사·로스터 MCP 툴(search_context/read_transcript/post_turn/get_roster) 라우터.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};

use super::TunaSearchServer;
use super::params::{PostTurnParams, RosterParams, SearchParams, TranscriptParams};
use crate::orchestrator::Utterance;

#[tool_router(router = search_router, vis = "pub(crate)")]
impl TunaSearchServer {
    #[tool(description = "토론 맥락 검색: 과거·다른 분기의 관련 발언을 찾는다")]
    pub(crate) async fn search_context(
        &self,
        Parameters(p): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        // retrieve는 SQLite 락 + (semantic 시) 동기 임베딩 HTTP 호출이라 blocking이다.
        // async executor 스레드를 막지 않도록 spawn_blocking으로 넘긴다.
        let retriever = Arc::clone(&self.retriever);
        let query = p.query;
        let limit = p.limit.unwrap_or(10).min(50);
        // retrieve Err(1차 검색 경로 DB 장애, R7) = success로 위장하지 않는다. R1 계약(isError=true)으로 반환해
        // 클라(McpHttpClient::parse_jsonrpc_sse)가 "결과 없음"과 "검색 실패"를 구분하게 한다.
        let outcome: Result<Vec<Utterance>, String> =
            tokio::task::spawn_blocking(move || retriever.retrieve(&query, limit))
                .await
                .unwrap_or_else(|e| Err(format!("검색 태스크 실패: {e}")));
        let hits = match outcome {
            Ok(h) => h,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "검색 실패: {e}"
                ))]));
            }
        };
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

    #[tool(
        description = "현재 토론 전사를 읽는다(활성 경로). 검색이 아니라 통째 맥락이 필요할 때."
    )]
    pub(crate) async fn read_transcript(
        &self,
        Parameters(p): Parameters<TranscriptParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(reader) = &self.reader else {
            return Ok(CallToolResult::success(vec![Content::text(
                "전사 리더 미연결".to_string(),
            )]));
        };
        let sid = p.session_id.unwrap_or_else(|| self.default_session.clone());
        // read_transcript Err(세션 로드 DB 장애, R7) = "전사 없음"으로 위장하지 않고 R1 계약으로 반환.
        let utts = match reader.read_transcript(&sid, p.max_turns) {
            Ok(u) => u,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "전사 읽기 실패: {e}"
                ))]));
            }
        };
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

    #[tool(description = "토론에 발언을 추가한다(원격 참가자가 코어 전사에 자기 턴을 씀).")]
    pub(crate) async fn post_turn(
        &self,
        Parameters(p): Parameters<PostTurnParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(writer) = &self.writer else {
            return Ok(CallToolResult::success(vec![Content::text(
                "전사 writer 미연결(post_turn 비활성)".to_string(),
            )]));
        };
        let sid = p.session_id.unwrap_or_else(|| self.default_session.clone());
        match writer.append_turn(&sid, &p.speaker, &p.content) {
            Ok(id) => Ok(CallToolResult::success(vec![Content::text(format!(
                "추가됨: session={sid} msg_id={id}"
            ))])),
            Err(e) => Ok(CallToolResult::success(vec![Content::text(format!(
                "추가 실패: {e}"
            ))])),
        }
    }

    #[tool(description = "현재 토론 참가자(좌석) 구성을 조회한다.")]
    pub(crate) async fn get_roster(
        &self,
        Parameters(_p): Parameters<RosterParams>,
    ) -> Result<CallToolResult, McpError> {
        let text = match &self.roster {
            None => "로스터 미연결".to_string(),
            Some(seats) if seats.is_empty() => "참가자 없음".to_string(),
            Some(seats) => seats
                .iter()
                .map(|s| match &s.role {
                    Some(r) => format!("{} ({})", s.engine, r),
                    None => s.engine.clone(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}
