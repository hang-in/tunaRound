// A2A task 워커·dispatcher·운영자 MCP 툴(poll/claim/complete/fail/extend/cancel/send/get/tasks) 라우터.

use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};

use super::TunaSearchServer;
use super::format::{
    cancel_task_text, claim_task_text, complete_task_text, extend_lease_text, fail_task_text,
    get_task_text, list_all_tasks_text, poll_tasks_text, send_task_routed,
};
use super::indexing::{build_terminal_index_payload, index_terminal_task};
use super::params::{
    CancelTaskParams, ClaimTaskParams, CompleteTaskParams, ExtendLeaseParams, FailTaskParams,
    GetTaskParams, ListTasksParams, PollTasksParams, SendTaskParams,
};

#[tool_router(router = tasks_router, vis = "pub(crate)")]
impl TunaSearchServer {
    #[tool(
        description = "내 앞으로 온 A2A task 목록을 조회한다(열린 상태: submitted/working/input_required)."
    )]
    pub(crate) async fn poll_tasks(
        &self,
        Parameters(p): Parameters<PollTasksParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(poll_tasks 비활성)".to_string(),
            )]));
        };
        // SQLite 락 호출이라 blocking이다. a2a_store는 A2A JSON-RPC 엔드포인트(a2a_server::a2a_handler)와
        // 동시에 경합할 수 있어 async executor 스레드를 막지 않도록 spawn_blocking으로 넘긴다(같은 관례).
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            poll_tasks_text(&store, &agent)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(DB 장애 등)를 success로 위장하지 않는다(claim_task와 동일 사유). 형제 write
        // 툴과 달리 이 조회 툴만 success 본문 텍스트로 감춰서, 워커(McpHttpClient::parse_jsonrpc_sse는
        // isError=true만 Err로 매핑)가 DB 장애를 "빈 큐"로 오인해 무로그로 조용히 재폴링하던 결함 수정.
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "poll_tasks 실패: {e}"
            ))])),
        }
    }

    #[tool(description = "task에 착수했음을 표시한다(submitted/input_required -> working).")]
    pub(crate) async fn claim_task(
        &self,
        Parameters(p): Parameters<ClaimTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(claim_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let agent = p.agent;
        let runner = p.runner;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            claim_task_text(&store, &task_id, agent.as_deref(), runner.as_deref())
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다. isError=true라야 클라(McpHttpClient::
        // parse_jsonrpc_sse)가 Err로 인식하고, 워커(run_one_pass)가 claim 실패로 보고 러너를 안 돌린다.
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "착수 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "task 결과를 보고하고 완료 처리한다(-> completed, 결과는 텍스트 Artifact로 저장)."
    )]
    pub(crate) async fn complete_task(
        &self,
        Parameters(p): Parameters<CompleteTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(complete_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let result = p.result;
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let text = complete_task_text(&store, &task_id, &result, agent.as_deref())?;
            // v2-45 P6a: 종결 성공 후 색인 payload를 같은 락 안에서 구성(요청=history[0], 결과=artifact).
            let payload = store
                .get_task(&task_id)
                .ok()
                .flatten()
                .as_ref()
                .and_then(build_terminal_index_payload);
            Ok::<_, String>((text, payload))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok((t, payload)) => {
                // a2a_store 락 해제 후 writer로 mesh 기억 색인(best-effort, 종결 응답과 독립).
                if let (Some(writer), Some(a2a), Some(payload)) =
                    (self.writer.clone(), self.a2a_store.clone(), payload)
                {
                    // best-effort·종결 응답과 독립: 색인을 백그라운드로 던지고 응답을 막지 않는다
                    // (gemini 리뷰). 크래시로 미완료 시 재기동 백필이 멱등 재색인한다(delete-then-append).
                    tokio::task::spawn_blocking(move || {
                        index_terminal_task(&writer, &a2a, &payload)
                    });
                }
                Ok(CallToolResult::success(vec![Content::text(t)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "완료 처리 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "task 실행이 실패했음을 보고한다(-> failed, 사유는 상태 메시지로 저장). completed와 구분되어 dispatcher가 실패를 인지한다."
    )]
    pub(crate) async fn fail_task(
        &self,
        Parameters(p): Parameters<FailTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(fail_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let reason = p.reason;
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let text = fail_task_text(&store, &task_id, &reason, agent.as_deref())?;
            // v2-45 P6a: 종결 성공 후 색인 payload 구성(요청=history[0], 결과=실패 사유 status_message).
            let payload = store
                .get_task(&task_id)
                .ok()
                .flatten()
                .as_ref()
                .and_then(build_terminal_index_payload);
            Ok::<_, String>((text, payload))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // R1: 내부 실패(전이충돌 포함)를 success로 위장하지 않는다(claim_task와 동일 사유).
        match outcome {
            Ok((t, payload)) => {
                if let (Some(writer), Some(a2a), Some(payload)) =
                    (self.writer.clone(), self.a2a_store.clone(), payload)
                {
                    // best-effort·종결 응답과 독립: 색인을 백그라운드로 던지고 응답을 막지 않는다
                    // (gemini 리뷰). 크래시로 미완료 시 재기동 백필이 멱등 재색인한다(delete-then-append).
                    tokio::task::spawn_blocking(move || {
                        index_terminal_task(&writer, &a2a, &payload)
                    });
                }
                Ok(CallToolResult::success(vec![Content::text(t)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "실패 처리 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "claim한 task의 lease를 연장한다(장시간 실행 중 requeue 방지, 워커가 주기 호출)."
    )]
    pub(crate) async fn extend_task_lease(
        &self,
        Parameters(p): Parameters<ExtendLeaseParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(extend_task_lease 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let agent = p.agent;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            extend_lease_text(&store, &task_id, &agent)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        // 대상이 아니면(종료·재claim) isError=true라야 워커가 Err로 인지한다(claim_task와 동일 계약).
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "lease 연장 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "열린 task를 취소한다(-> canceled). 잘못 보냈거나 더 필요 없는 task 정리용. 이미 종료된 task는 거부한다."
    )]
    pub(crate) async fn cancel_task(
        &self,
        Parameters(p): Parameters<CancelTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(cancel_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        let reason = p.reason;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            cancel_task_text(&store, &task_id, reason.as_deref())
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        match outcome {
            Ok(t) => Ok(CallToolResult::success(vec![Content::text(t)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "취소 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "다른 에이전트에게 새 A2A task를 위임한다(생성 즉시 submitted 상태, dispatcher용)."
    )]
    pub(crate) async fn send_task(
        &self,
        Parameters(p): Parameters<SendTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(send_task 비활성)".to_string(),
            )]));
        };
        let SendTaskParams {
            from_agent,
            to_agent,
            text,
            context_id,
            to_selector,
        } = p;
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            send_task_routed(
                &store,
                &from_agent,
                to_agent.as_deref(),
                to_selector.as_deref(),
                &text,
                context_id,
            )
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("전송 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "위임한 A2A task의 상태를 조회한다(completed면 결과 텍스트도 함께 반환, dispatcher용). wait_secs(1~120)를 주면 terminal까지 그 시간만큼 서버가 대기(long-poll)해 폴링 관리가 불필요하다."
    )]
    pub(crate) async fn get_task(
        &self,
        Parameters(p): Parameters<GetTaskParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(get_task 비활성)".to_string(),
            )]));
        };
        let task_id = p.task_id;
        // long-poll(v2-54 G7): terminal이 아니면 1초 간격 재확인, wait 소진 시 그 시점 상태 반환.
        // 미존재·조회 오류는 대기 없이 즉시 반환한다(id 오타를 wait 내내 붙잡지 않고, 일시 store
        // 오류도 조기 종료 후 아래 최종 조회가 표면화한다). wait=0/생략은 루프 자체를 건너뛰어 기존
        // 경로와 동일하다(store 왕복 추가 없음). lease 만료 sweep은 poll 경로 트리거라 여기 대기가
        // requeue를 만들지는 않는다(관찰만 한다). 락은 매 반복 spawn_blocking 안에서만 잡는다.
        let wait = p.wait_secs.unwrap_or(0).min(120);
        if wait > 0 {
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(wait);
            loop {
                let store2 = store.clone();
                let tid = task_id.clone();
                let state = tokio::task::spawn_blocking(move || {
                    let s = store2.lock().unwrap_or_else(|e| e.into_inner());
                    s.get_task(&tid).map(|o| o.map(|t| t.state))
                })
                .await
                .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
                let open = matches!(state, Ok(Some(st)) if st.is_open());
                if !open || std::time::Instant::now() >= deadline {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        }
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            get_task_text(&store, &task_id)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("조회 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }

    #[tool(
        description = "브로커 전역에서 열려 있는 A2A task를 to_agent 무관하게 전부 조회한다(운영자 조망용, 미배달/고착 의심 주석 포함)."
    )]
    pub(crate) async fn tasks(
        &self,
        Parameters(_p): Parameters<ListTasksParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(store) = self.a2a_store.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "A2A task 저장소 미구성(tasks 비활성)".to_string(),
            )]));
        };
        let outcome = tokio::task::spawn_blocking(move || {
            let store = store.lock().unwrap_or_else(|e| e.into_inner());
            let now = store.now()?;
            list_all_tasks_text(&store, &now)
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let text = match outcome {
            Ok(t) => t,
            Err(e) => format!("조회 실패: {e}"),
        };
        Ok(CallToolResult::success(vec![Content::text(text)]))
    }
}
