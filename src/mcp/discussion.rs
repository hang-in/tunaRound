// mesh 토론 MCP 툴(start_discussion/stop_discussion): v2-56 driver의 세션 표면. 시작·중단은 총괄
// 세션의 MCP 도구로만 한다(대시보드=관제탑 원칙, 웹 시작 폼은 비범위).

use std::collections::BTreeMap;

use rmcp::{
    ErrorData as McpError,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, Content},
    tool, tool_router,
};

use super::TunaSearchServer;
use super::params::{
    ContinueDiscussionParams, DiscussionSeatParams, StartDiscussionParams, StopDiscussionParams,
};
use crate::discussion::{DiscussionSeat, DiscussionSpec, DriverConfig, debate_ns, run_discussion};
use crate::store::agents::{AGENT_TTL_SECS, AgentEntry};

/// 좌석 파라미터를 로스터 online 목록과 대조해 확정 좌석으로 해석한다(순수 로직, 단위테스트 대상).
/// 실패 = 좌석 수 범위 밖 / 중복 좌석 / offline 에이전트 / 라이브 세션 live 미동의(v2-56 §8-3).
pub(crate) fn resolve_seats(
    params: &[DiscussionSeatParams],
    online: &[AgentEntry],
) -> Result<Vec<DiscussionSeat>, String> {
    if !(2..=6).contains(&params.len()) {
        return Err(format!(
            "좌석은 2~6석이어야 합니다(현재 {}석)",
            params.len()
        ));
    }
    let mut seen = std::collections::HashSet::new();
    let mut seen_labels = std::collections::HashSet::new();
    let mut seats = Vec::with_capacity(params.len());
    for p in params {
        if !seen.insert(p.agent.clone()) {
            return Err(format!("중복 좌석: {}", p.agent));
        }
        let Some(entry) = online.iter().find(|e| e.uuid == p.agent) else {
            return Err(format!(
                "로스터에 online이 아닌 에이전트: {} (list_agents로 확인)",
                p.agent
            ));
        };
        let is_session = entry.tags.get("role").is_some_and(|r| r == "session");
        let live = p.live.unwrap_or(false);
        if is_session && !live {
            return Err(format!(
                "{}는 라이브 세션입니다. 그 세션의 컨텍스트를 소모해도 좋다면 live:true를 명시하세요",
                entry
                    .display_name
                    .clone()
                    .unwrap_or_else(|| p.agent.clone())
            ));
        }
        // 라벨은 전사 speaker(`debate/<label>`)에 들어가므로 '/'는 '-'로 치환(네임스페이스 보존).
        let label = p
            .label
            .clone()
            .or_else(|| entry.display_name.clone())
            .unwrap_or_else(|| p.agent.chars().take(8).collect())
            .replace('/', "-");
        // 라벨 중복 = 서로 다른 좌석의 발언이 한 화자로 귀속(순차-인지·종합 왜곡)되므로 거부.
        if !seen_labels.insert(label.clone()) {
            return Err(format!(
                "좌석 라벨 중복: {label} (label을 명시해 좌석을 구분하세요)"
            ));
        }
        seats.push(DiscussionSeat {
            agent: p.agent.clone(),
            label,
            role: p.role.clone(),
            instruction: p.instruction.clone().unwrap_or_default(),
            live,
        });
    }
    Ok(seats)
}

#[tool_router(router = discussion_router, vis = "pub(crate)")]
impl TunaSearchServer {
    #[tool(
        description = "mesh 토론을 시작한다(v2-56). 좌석=로스터 online 에이전트 2~6석(라이브 세션 좌석은 live:true 필수), 유한 라운드(기본 3, 최대 10, 순차-인지) 후 synthesizer(기본=첫 좌석) 종합 1회. 전사=debate:<id> 세션(read_transcript로 열람), 결과 수신=watch-results --dispatcher debate:<id>. 비동기 작업이다(좌석당 수 분)."
    )]
    pub(crate) async fn start_discussion(
        &self,
        Parameters(p): Parameters<StartDiscussionParams>,
    ) -> Result<CallToolResult, McpError> {
        let (Some(registry), Some(store), Some(writer)) = (
            self.discussions.clone(),
            self.a2a_store.clone(),
            self.writer.clone(),
        ) else {
            return Ok(CallToolResult::success(vec![Content::text(
                "mesh 토론 미구성(start_discussion 비활성: 브로커 serve 경로에서만 지원)"
                    .to_string(),
            )]));
        };
        let rounds = p.rounds.unwrap_or(3).clamp(1, 10);
        let gate = p.gate.unwrap_or(false);
        let topic = p.topic.trim().to_string();
        if topic.is_empty() {
            return Ok(CallToolResult::error(vec![Content::text(
                "주제가 비어 있습니다".to_string(),
            )]));
        }
        // 로스터 검증 + id 발급. SQLite 락 호출이라 spawn_blocking 관례(poll_tasks와 동일).
        let store2 = store.clone();
        let seats_params = p.seats;
        let outcome = tokio::task::spawn_blocking(move || {
            let s = store2.lock().unwrap_or_else(|e| e.into_inner());
            let now = s.now()?;
            let online = s.list_agents(&BTreeMap::new(), &now, AGENT_TTL_SECS);
            let seats = resolve_seats(&seats_params, &online)?;
            let id: String = s.new_task_id()?.chars().take(12).collect();
            Ok::<_, String>((seats, id))
        })
        .await
        .unwrap_or_else(|e| Err(format!("작업 실패: {e}")));
        let (seats, id) = match outcome {
            Ok(v) => v,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "토론 시작 실패: {e}"
                ))]));
            }
        };
        // 동시 1건 점유(MVP). driver 종료(FinishGuard)가 해제한다.
        let control = match registry.try_begin(&id) {
            Ok(c) => c,
            Err(e) => {
                return Ok(CallToolResult::error(vec![Content::text(format!(
                    "토론 시작 실패: {e}"
                ))]));
            }
        };
        let spec = DiscussionSpec {
            id: id.clone(),
            topic,
            seats,
            rounds,
            gate,
        };
        let seat_list = spec
            .seats
            .iter()
            .map(|s| format!("{}({})", s.label, s.role.as_deref().unwrap_or("-")))
            .collect::<Vec<_>>()
            .join(" → ");
        let ns = debate_ns(&id);
        tokio::spawn(run_discussion(
            spec,
            store,
            writer,
            registry,
            control,
            DriverConfig::default(),
        ));
        let gate_note = if gate {
            format!(
                "\n게이트: 각 라운드 완료 시 인박스로 다이제스트가 옵니다. 승인 주체는 사람입니다 - 사용자에게 보고하고 지시가 있을 때만 continue_discussion(discussion_id=\"{id}\", steer?, conclude?)를 호출하세요(자율 진행 금지). 게이트 대기 중 브로커가 재기동되면 표식 task가 failed로 인박스에 통지되고 토론은 소멸합니다(재발의 필요)."
            )
        } else {
            String::new()
        };
        Ok(CallToolResult::success(vec![Content::text(format!(
            "토론 시작: id={id}\n좌석: {seat_list}\n라운드: {rounds} + 종합 1회(첫 좌석)\n전사: read_transcript(session_id=\"{ns}\")\n결과 수신: tunaround watch-results --core <브로커 URL> --dispatcher {ns} (라운드마다 RESULT 줄, 마지막 완료=종합){gate_note}\n중단: stop_discussion(discussion_id=\"{id}\")\n비고: 비동기 작업입니다(좌석당 수 분, 좌석 타임아웃 600초). 브로커 재기동 시 진행 중 토론은 실패 처리됩니다."
        ))]))
    }

    #[tool(
        description = "게이트 대기 중인 mesh 토론을 다음 단계로 진행한다(이슈 #131, start_discussion gate:true 전용). 사람 승인 게이트이므로 사용자 지시가 있을 때만 호출할 것(자율 진행 금지). steer=조향 지시(전사에 [사용자 조향 지시]로 남고 다음 라운드에 주입), conclude=true면 남은 라운드를 건너뛰고 synthesizer 종합 직행."
    )]
    pub(crate) async fn continue_discussion(
        &self,
        Parameters(p): Parameters<ContinueDiscussionParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(registry) = self.discussions.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "mesh 토론 미구성(continue_discussion 비활성)".to_string(),
            )]));
        };
        let steer = p
            .steer
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty());
        let conclude = p.conclude.unwrap_or(false);
        let steer_echo = steer.clone();
        match registry.continue_active(&p.discussion_id, steer, conclude) {
            Ok((round, total)) => {
                let next = if conclude {
                    "종합(synthesizer) 직행".to_string()
                } else if round >= total {
                    "종합(synthesizer)".to_string()
                } else {
                    format!("라운드 {}", round + 1)
                };
                let steer_note = match steer_echo {
                    Some(s) => format!("\n조향 반영: [사용자 조향 지시] {s}"),
                    None => String::new(),
                };
                Ok(CallToolResult::success(vec![Content::text(format!(
                    "게이트 해제: 라운드 {round}/{total} → {next}.{steer_note}"
                ))]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "진행 실패: {e}"
            ))])),
        }
    }

    #[tool(
        description = "진행 중 mesh 토론의 이후 라운드 발행을 중단한다. 이미 실행 중인 좌석 러너는 중단되지 않는다(취소는 상태 전이일 뿐, 늦은 완료는 무시됨)."
    )]
    pub(crate) async fn stop_discussion(
        &self,
        Parameters(p): Parameters<StopDiscussionParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(registry) = self.discussions.clone() else {
            return Ok(CallToolResult::success(vec![Content::text(
                "mesh 토론 미구성(stop_discussion 비활성)".to_string(),
            )]));
        };
        match registry.cancel(&p.discussion_id) {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "토론 {} 중단 요청됨: 이후 라운드는 발행되지 않습니다. 이미 실행 중인 좌석 러너는 끝까지 돌 수 있으나 그 task는 failed로 마감되어 늦은 완료가 무시됩니다.",
                p.discussion_id
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(format!(
                "중단 실패: {e}"
            ))])),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(uuid: &str, display: Option<&str>, role_tag: Option<&str>) -> AgentEntry {
        let mut tags = BTreeMap::new();
        if let Some(r) = role_tag {
            tags.insert("role".to_string(), r.to_string());
        }
        AgentEntry {
            uuid: uuid.to_string(),
            tags,
            display_name: display.map(String::from),
            last_heartbeat: "2026-07-18 00:00:00".to_string(),
            human_input_at: None,
            turn_active_at: None,
        }
    }

    fn seat_param(agent: &str, live: Option<bool>) -> DiscussionSeatParams {
        DiscussionSeatParams {
            agent: agent.to_string(),
            label: None,
            role: None,
            instruction: None,
            live,
        }
    }

    #[test]
    fn resolve_seats_rejects_out_of_range_and_duplicates() {
        let online = vec![entry("a", None, None), entry("b", None, None)];
        assert!(
            resolve_seats(&[seat_param("a", None)], &online).is_err(),
            "1석 거부"
        );
        let dup = [seat_param("a", None), seat_param("a", None)];
        assert!(resolve_seats(&dup, &online).is_err(), "중복 거부");
    }

    #[test]
    fn resolve_seats_rejects_offline_agent() {
        let online = vec![entry("a", None, None)];
        let params = [seat_param("a", None), seat_param("ghost", None)];
        let err = resolve_seats(&params, &online).unwrap_err();
        assert!(err.contains("online이 아닌"), "{err}");
    }

    #[test]
    fn resolve_seats_requires_live_flag_for_session_agents() {
        let online = vec![
            entry("w", None, Some("worker")),
            entry("s", Some("mac-claude-home"), Some("session")),
        ];
        let params = [seat_param("w", None), seat_param("s", None)];
        let err = resolve_seats(&params, &online).unwrap_err();
        assert!(err.contains("live:true"), "{err}");
        // live:true 명시하면 통과 + 라벨=display_name.
        let ok = resolve_seats(
            &[seat_param("w", None), seat_param("s", Some(true))],
            &online,
        )
        .unwrap();
        assert_eq!(ok[1].label, "mac-claude-home");
        assert!(ok[1].live);
        assert!(!ok[0].live);
    }

    #[test]
    fn resolve_seats_rejects_duplicate_labels() {
        // 같은 display_name의 세션 2개(같은 머신·같은 프로젝트)를 label 생략으로 지정하면 화자
        // 오귀속이 생기므로 거부한다.
        let online = vec![
            entry("a", Some("win-claude"), None),
            entry("b", Some("win-claude"), None),
        ];
        let err =
            resolve_seats(&[seat_param("a", None), seat_param("b", None)], &online).unwrap_err();
        assert!(err.contains("라벨 중복"), "{err}");
        // label 명시로 구분하면 통과.
        let mut p1 = seat_param("a", None);
        p1.label = Some("win-claude-1".to_string());
        let ok = resolve_seats(&[p1, seat_param("b", None)], &online).unwrap();
        assert_eq!(ok[0].label, "win-claude-1");
    }

    #[test]
    fn resolve_seats_label_fallback_and_slash_sanitize() {
        let online = vec![
            entry("0123456789abcdef", None, None),
            entry("x", Some("win/claude"), None),
        ];
        let params = [seat_param("0123456789abcdef", None), seat_param("x", None)];
        let seats = resolve_seats(&params, &online).unwrap();
        assert_eq!(seats[0].label, "01234567", "display 없으면 uuid 앞 8자");
        assert_eq!(seats[1].label, "win-claude", "라벨의 '/'는 '-'로 치환");
    }
}
