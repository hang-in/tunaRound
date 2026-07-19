// 에이전트 로스터 등록·하트비트·발견·presence 동기화 MCP 툴 라우터.

use std::collections::BTreeMap;

use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_router,
};

use super::TunaSearchServer;
use super::format::format_agents;
use super::params::{HeartbeatParams, ListAgentsParams, RegisterAgentParams, ReportPresenceParams};
use crate::store::agents::{AGENT_TTL_SECS, parse_tags};

#[tool_router(router = registry_router, vis = "pub(crate)")]
impl TunaSearchServer {
    #[tool(
        description = "이 에이전트를 브로커 로스터에 등록한다(uuid+태그, 워커/세션 자기 등록용)."
    )]
    pub(crate) async fn register_agent(
        &self,
        Parameters(p): Parameters<RegisterAgentParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "A2A task 저장소 미구성(register_agent 비활성)".to_string(),
            )]));
        };
        let RegisterAgentParams {
            uuid,
            tags,
            display_name,
        } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let tags = match tags {
                Some(s) => parse_tags(&s)?,
                None => BTreeMap::new(),
            };
            let tags_len = tags.len();
            store.register_agent(&uuid, tags, display_name, &now);
            Ok::<String, String>(format!("등록됨: uuid={uuid} tags={tags_len}개"))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 등록 실패(now/parse_tags 오류)를 success로 위장하지 않는다(클라가 감지하게 isError).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![ContentBlock::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "등록 실패: {e}"
            ))])),
        }
    }

    #[tool(description = "로스터에 자기 존재를 갱신한다(online 유지, 주기 호출).")]
    pub(crate) async fn heartbeat(
        &self,
        Parameters(p): Parameters<HeartbeatParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "A2A task 저장소 미구성(heartbeat 비활성)".to_string(),
            )]));
        };
        let uuid = p.uuid;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let ok = store.heartbeat_agent(&uuid, &now);
            Ok::<String, String>(if ok {
                format!("heartbeat 갱신: {uuid}")
            } else {
                format!("미등록 uuid={uuid}(register_agent 먼저 호출하세요)")
            })
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 실제 실패(now 오류)만 isError. "미등록..."은 클로저에서 Ok라 success로 남아 워커의
        // 재등록 로직(needs_reregister)이 그 텍스트를 받는다(정상 흐름, 실패 아님).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![ContentBlock::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "heartbeat 실패: {e}"
            ))])),
        }
    }

    #[tool(description = "online 에이전트를 발견한다(selector 태그로 필터, dispatcher 라우팅용).")]
    pub(crate) async fn list_agents(
        &self,
        Parameters(p): Parameters<ListAgentsParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "A2A task 저장소 미구성(list_agents 비활성)".to_string(),
            )]));
        };
        let selector = p.selector;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let sel = match selector {
                Some(s) => parse_tags(&s)?,
                None => BTreeMap::new(),
            };
            let agents = store.list_agents(&sel, &now, AGENT_TTL_SECS);
            Ok::<String, String>(format_agents(&agents))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 조회 실패(now/parse_tags 오류)를 success로 위장하지 않는다(클라가 감지하게 isError).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![ContentBlock::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "조회 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "머신당 presence 스캐너가 라이브 세션 전집합을 일괄 보고한다(upsert+소유분 제거, v2-44)."
    )]
    pub(crate) async fn report_presence(
        &self,
        Parameters(p): Parameters<ReportPresenceParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![ContentBlock::text(
                "A2A task 저장소 미구성(report_presence 비활성)".to_string(),
            )]));
        };
        let ReportPresenceParams { machine, sessions } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            let entries: Vec<crate::store::agents::PresenceUpsert> = sessions
                .into_iter()
                .map(|s| crate::store::agents::PresenceUpsert {
                    uuid: s.uuid,
                    runner: s.runner,
                    project: s.project,
                    display_name: s.display_name,
                    human_input_at: s.human_input_at,
                    active_at: s.active_at,
                })
                .collect();
            let (upserted, removed) = store.sync_presence(&machine, &entries, &now);
            Ok::<String, String>(format!(
                "presence 동기화(machine={machine}): upsert {upserted}건, 제거 {removed}건"
            ))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // 동기화 실패(now 오류)를 success로 위장하지 않는다(R1 계약과 동일).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![ContentBlock::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "presence 동기화 실패: {e}"
            ))])),
        }
    }
}
