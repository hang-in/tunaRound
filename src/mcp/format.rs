// A2A task/에이전트 목록을 사람이 읽는 텍스트로 조립하는 순수 포맷 헬퍼 모음.

use super::*;
use crate::store::a2a::{Artifact, Message, Part};
use crate::store::agents::{format_ambiguous_candidates, validate_send_target, AgentEntry, SendTarget};

/// working이 이 초과 갱신정지면 stuck? 표시(claim 후 사망 의심).
pub(crate) const STUCK_WORKING_SECS: i64 = 15 * 60;
/// submitted가 이 초과 미claim이면 no-consumer? 표시(폴러 없음 의심).
pub(crate) const NO_CONSUMER_SUBMITTED_SECS: i64 = 5 * 60;

/// task 건강 분류(표시·집계 공용 단일 소스). 임계 이내·다른 상태·now 파싱 실패는 Ok.
pub(crate) enum TaskHealth {
    Ok,
    /// submitted가 NO_CONSUMER_SUBMITTED_SECS 초과 미claim(폴러 없음 의심). 값=경과 초.
    NoConsumer(i64),
    /// working이 STUCK_WORKING_SECS 초과 갱신정지(claim 후 사망 의심). 값=경과 초.
    Stuck(i64),
}

/// task를 건강 상태로 분류한다(health_annotation 표시와 헬스 패널 집계가 같은 임계를 쓰게 하는 단일 소스).
pub(crate) fn classify_task_health(task: &crate::store::a2a::Task, now: &str) -> TaskHealth {
    use crate::store::a2a::age_secs;
    match task.state {
        TaskState::Working => match age_secs(now, &task.updated_at) {
            Some(secs) if secs > STUCK_WORKING_SECS => TaskHealth::Stuck(secs),
            _ => TaskHealth::Ok,
        },
        TaskState::Submitted => match age_secs(now, &task.created_at) {
            Some(secs) if secs > NO_CONSUMER_SUBMITTED_SECS => TaskHealth::NoConsumer(secs),
            _ => TaskHealth::Ok,
        },
        _ => TaskHealth::Ok,
    }
}

/// task의 미배달/고착 의심 주석을 만든다(표시 전용, 상태 전이·저장 없음). 임계·판정은 classify_task_health
/// 단일 소스를 재사용한다(그 외는 빈 문자열).
pub(crate) fn health_annotation(task: &crate::store::a2a::Task, now: &str) -> String {
    match classify_task_health(task, now) {
        TaskHealth::Stuck(secs) => format!(" ⚠stuck?({}m)", secs / 60),
        TaskHealth::NoConsumer(secs) => format!(" ⚠no-consumer?({}m)", secs / 60),
        TaskHealth::Ok => String::new(),
    }
}

/// poll_tasks 순수 로직: agent 앞으로 열린(submitted/working/input_required) task 목록을 사람이 읽기
/// 쉬운 텍스트로 조립한다. SQLite 호출은 하되 MCP/async 계층과 무관해 in-memory store로 단위테스트 가능.
/// 조회 전에 lease 만료 지연 sweep(expire_stale_claims)을 먼저 돌려, 죽은 워커가 물고 있던 task를
/// poll에 반영되기 전에 회수한다(별도 타이머 없이 poll 경로에 얹는 설계).
pub(crate) fn poll_tasks_text(store: &SqliteStore, agent: &str) -> Result<String, String> {
    // sweep 실패는 poll 자체를 막지 않는다(목록 조회는 sweep 여부와 무관하게 계속 유용하므로 로그만).
    if let Err(e) = store.expire_stale_claims() {
        eprintln!("[poll_tasks] expire_stale_claims 실패(무시하고 계속): {e}");
    }
    let tasks = store.list_open_tasks_for(agent)?;
    let now = store.now()?;
    Ok(format_open_tasks(agent, &tasks, &now))
}

/// task 목록을 `[id] from=... state=... msg=...` 줄들로 조립하는 순수 함수(SQLite 없이 테스트 가능).
/// now는 health_annotation(표시 전용 stuck?/no-consumer? 주석)에 쓰인다.
pub(crate) fn format_open_tasks(agent: &str, tasks: &[crate::store::a2a::Task], now: &str) -> String {
    if tasks.is_empty() {
        return format!("{agent} 앞 열린 task 없음");
    }
    tasks
        .iter()
        .map(|t| {
            let msg = t
                .status_message
                .as_ref()
                .and_then(|m| m.parts.first())
                .and_then(|p| p.text.as_deref())
                .unwrap_or("(본문 없음)");
            // ctx=<context_id>는 워커가 프로젝트별 라우팅(--context-map)에 쓴다. 없으면 "-".
            let ctx = t.context_id.as_deref().unwrap_or("-");
            let annotation = health_annotation(t, now);
            format!(
                "[{}] from={} state={}{} ctx={} msg={}",
                t.id,
                t.from_agent,
                t.state.as_str(),
                annotation,
                ctx,
                msg
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// claim_task 순수 로직: task를 working으로 전이하고 확인 텍스트를 만든다. 대상 task가 없거나 이미
/// working 이상으로 전이돼 있으면(다른 워커가 먼저 claim) try_claim이 Err를 반환하고 그대로 위로
/// 전파한다(레이스 컨디션 방지, R2). agent는 lease 소유자(claimed_by)로 기록되어 first-completer-wins
/// 판별에 쓰인다(None이면 하위호환 - claimed_by NULL). runner는 처리하는 러너 종류(트레이스용, v8).
pub(crate) fn claim_task_text(
    store: &SqliteStore,
    task_id: &str,
    agent: Option<&str>,
    runner: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    store.try_claim(task_id, agent, runner)?;
    Ok(format!("착수됨: task_id={task_id} state=working"))
}

/// complete_task 순수 로직: result 텍스트를 단일 Artifact로 감싸 completed로 마감한다. 대상 task가
/// 없으면 Err. artifact_id는 store.new_task_id()로 발급받아 신규 crate 의존 없이 고유성을 확보한다.
/// working 상태가 아니면(예: 아직 claim 안 됨, 또는 이미 completed/canceled로 종료) try_complete가
/// Err를 반환하고 그대로 위로 전파한다(레이스 컨디션 방지, R2). agent는 first-completer-wins 완료자
/// 검증에 쓰인다(claimed_by와 불일치하면 거부, None이면 하위호환 - 가드 무력화).
pub(crate) fn complete_task_text(
    store: &SqliteStore,
    task_id: &str,
    result: &str,
    agent: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    let artifact_id = store.new_task_id()?;
    let artifacts =
        vec![Artifact { artifact_id, name: None, parts: vec![Part { text: Some(result.to_string()), ..Default::default() }] }];
    store.try_complete(task_id, &artifacts, agent)?;
    Ok(format!("완료됨: task_id={task_id} state=completed"))
}

/// fail_task 순수 로직: task를 failed로 전이하고 사유를 상태 메시지로 남긴다. 대상 task가 없으면 Err.
/// 러너 실행이 실패했을 때 completed로 위장하지 않고 failed로 구분해 dispatcher가 성패를 알 수 있게 한다.
/// 이미 completed/canceled로 종료된 task면 try_fail이 Err를 반환하고 그대로 위로 전파한다(레이스
/// 컨디션 방지, R2 - 종료 상태를 failed로 덮어쓰지 못함).
pub(crate) fn fail_task_text(
    store: &SqliteStore,
    task_id: &str,
    reason: &str,
    agent: Option<&str>,
) -> Result<String, String> {
    if store.get_task(task_id)?.is_none() {
        return Err(format!("task 없음: task_id={task_id}"));
    }
    let message_id = store.new_task_id()?;
    let message = Message {
        message_id,
        role: "agent".to_string(),
        parts: vec![Part { text: Some(reason.to_string()), ..Default::default() }],
        task_id: None,
        context_id: None,
    };
    store.try_fail(task_id, Some(&message), agent)?;
    Ok(format!("실패 처리됨: task_id={task_id} state=failed"))
}

/// send_task 순수 로직: text 하나를 A2A Message로 감싸 store::create_task_from_message에 위임한다.
/// message_id는 store.new_task_id()로 발급(신규 crate 의존 없이 고유성 확보, complete_task_text의
/// artifact_id 발급과 같은 관례).
pub(crate) fn send_task_text(
    store: &SqliteStore,
    from_agent: &str,
    to_agent: &str,
    text: &str,
    context_id: Option<String>,
) -> Result<String, String> {
    let message_id = store.new_task_id()?;
    let message = Message {
        message_id,
        role: "user".to_string(),
        parts: vec![Part { text: Some(text.to_string()), ..Default::default() }],
        task_id: None,
        context_id,
    };
    let task = store.create_task_from_message(from_agent, to_agent, message)?;
    Ok(format!("생성됨: task_id={} state={}", task.id, task.state.as_str()))
}

/// AgentEntry 목록을 사람이 읽는 텍스트로 조립한다(비면 "online 에이전트 없음").
pub fn format_agents(agents: &[AgentEntry]) -> String {
    if agents.is_empty() {
        return "online 에이전트 없음".to_string();
    }
    agents
        .iter()
        .map(|a| {
            let name = a.display_name.as_deref().unwrap_or("-");
            let tags = a
                .tags
                .iter()
                .map(|(k, v)| format!("{k}={v}"))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{}] {} tags: {} (heartbeat={})", a.uuid, name, tags, a.last_heartbeat)
        })
        .collect::<Vec<_>>()
        .join("\n")
}


/// send_task 셀렉터 인지 버전. Agent면 concrete 발송(send_task_text 위임), Selector면 resolve 후
/// 0개=no-consumer 안내(생성 안 함), 1개=그 uuid로 발송, 2개+=후보 목록(생성 안 함).
pub(crate) fn send_task_routed(
    store: &SqliteStore,
    from_agent: &str,
    to_agent: Option<&str>,
    to_selector: Option<&str>,
    text: &str,
    context_id: Option<String>,
) -> Result<String, String> {
    match validate_send_target(to_agent, to_selector)? {
        SendTarget::Agent(agent) => send_task_text(store, from_agent, &agent, text, context_id),
        SendTarget::Selector(selector) => {
            let sel = parse_tags(&selector)?;
            let now = store.now()?;
            let uuids = store.resolve_selector(&sel, &now, AGENT_TTL_SECS);
            match uuids.len() {
                0 => Ok(format!(
                    "대상 없음(no-consumer): 셀렉터 '{selector}'에 매칭되는 online 에이전트가 없습니다. list_agents로 확인하세요."
                )),
                1 => send_task_text(store, from_agent, &uuids[0], text, context_id),
                _ => Ok(format_ambiguous_candidates(&selector, &uuids)),
            }
        }
    }
}

/// get_task 순수 로직: task를 조회해 상태를 요약한다. completed면 artifact 텍스트들을 이어붙인다.
/// 대상 task가 없어도 Err가 아니라 안내 문구를 Ok로 반환한다(poll_tasks의 빈 목록 관례와 동일 - "없음"은
/// 실패가 아니라 정상적인 조회 결과이므로).
pub(crate) fn get_task_text(store: &SqliteStore, task_id: &str) -> Result<String, String> {
    match store.get_task(task_id)? {
        None => Ok(format!("task 없음: task_id={task_id}")),
        Some(task) => {
            let now = store.now()?;
            Ok(format_task_status(&task, &now))
        }
    }
}

/// task 상태를 `[id] state=...` 한 줄로 조립하고, completed면 artifact 텍스트를, 열린 상태
/// (submitted/working/input_required)면 원 요청 본문을 이어붙이는 순수 함수(SQLite 없이 테스트 가능).
/// 열린 상태 본문은 claim 후 본문 재조회 경로가 없어 수신자가 브로커 DB를 직독하던 마찰을 없앤다
/// (세션20 실측). now는 health_annotation(표시 전용 stuck?/no-consumer? 주석)에 쓰인다.
/// runner가 기록돼 있으면(v8, claim한 워커의 러너 종류) ` runner=<x>`를 덧붙인다. 표시 전용, 없으면 생략.
pub(crate) fn format_task_status(task: &crate::store::a2a::Task, now: &str) -> String {
    let mut out = format!("[{}] state={}{}", task.id, task.state.as_str(), health_annotation(task, now));
    if let Some(runner) = task.runner.as_deref() {
        out.push_str(&format!(" runner={runner}"));
    }
    if task.state == TaskState::Completed {
        let texts: Vec<&str> =
            task.artifacts.iter().flat_map(|a| a.parts.iter()).filter_map(|p| p.text.as_deref()).collect();
        if !texts.is_empty() {
            out.push('\n');
            out.push('\n');
            out.push_str(&texts.join("\n\n"));
        }
    } else if matches!(task.state, TaskState::Submitted | TaskState::Working | TaskState::InputRequired) {
        // 텍스트가 하나도 없는 status_message는 건너뛰고 history로 폴백한다(봇리뷰: Some이지만
        // parts.text 전부 None이면 본문이 있는 history[0]가 막히던 것).
        let texts: Vec<&str> = task
            .status_message
            .as_ref()
            .filter(|m| m.parts.iter().any(|p| p.text.is_some()))
            .or_else(|| task.history.first())
            .map(|m| m.parts.iter().filter_map(|p| p.text.as_deref()).collect())
            .unwrap_or_default();
        if !texts.is_empty() {
            out.push_str("\n\n[요청]\n");
            out.push_str(&texts.join("\n"));
        }
    }
    out
}

/// tasks 순수 로직: 브로커 전역에서 열려 있는 task를 to_agent 무관하게 전부 조회해 사람이 읽는 텍스트로
/// 조립한다(운영자 조망용, poll_tasks의 agent 필터판과 대비). health_annotation의 stuck?/no-consumer?
/// 표시가 그대로 붙어 미배달/고착 의심 task를 한눈에 볼 수 있다. poll_tasks_text와 동일하게 조회 전
/// lease 만료 지연 sweep을 먼저 돌린다(운영자 조망에도 최신 회수 결과가 반영되도록).
pub(crate) fn list_all_tasks_text(store: &SqliteStore, now: &str) -> Result<String, String> {
    if let Err(e) = store.expire_stale_claims() {
        eprintln!("[list_all_tasks] expire_stale_claims 실패(무시하고 계속): {e}");
    }
    let tasks = store.list_all_open_tasks()?;
    if tasks.is_empty() {
        return Ok("열린 task 없음".to_string());
    }
    Ok(tasks
        .iter()
        .map(|t| {
            let msg = t
                .status_message
                .as_ref()
                .and_then(|m| m.parts.first())
                .and_then(|p| p.text.as_deref())
                .unwrap_or("(본문 없음)");
            let ctx = t.context_id.as_deref().unwrap_or("-");
            let annotation = health_annotation(t, now);
            format!(
                "[{}] from={} to={} state={}{} ctx={} msg={}",
                t.id,
                t.from_agent,
                t.to_agent,
                t.state.as_str(),
                annotation,
                ctx,
                msg
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// task 하나를 심고 store에 영속한다(inbox 테스트 공용 헬퍼).
    fn seed_task(store: &SqliteStore, id: &str, from: &str, to: &str, created_at: &str) {
        let task = crate::store::a2a::Task::new(id, None, from, to, created_at);
        store.create_task(&task).unwrap();
    }

    #[test]
    fn format_open_tasks_empty_says_no_open_tasks() {
        let text = format_open_tasks("mac-claude", &[], "2026-07-02 09:00:00");
        assert!(text.contains("mac-claude"), "agent 언급 없음: {text}");
        assert!(text.contains("없음"), "빈 목록 안내가 아님: {text}");
    }

    #[test]
    fn format_open_tasks_lists_task_id_from_agent_state_and_message() {
        let mut task =
            crate::store::a2a::Task::new("t1", None, "win-claude", "mac-claude", "2026-07-02 09:00:00");
        task.status_message = Some(crate::store::a2a::Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part { text: Some("리뷰 부탁".into()), ..Default::default() }],
            task_id: Some("t1".into()),
            context_id: None,
        });
        // now를 created_at과 같게 둬 stuck?/no-consumer? 주석이 안 붙게 한다(이 테스트는 그 표시를 검증하지 않음).
        let text = format_open_tasks("mac-claude", &[task], "2026-07-02 09:00:00");
        assert!(text.contains("t1"), "task id 누락: {text}");
        assert!(text.contains("win-claude"), "from_agent 누락: {text}");
        assert!(text.contains("submitted"), "state 누락: {text}");
        assert!(text.contains("리뷰 부탁"), "메시지 본문 누락: {text}");
    }

    #[test]
    fn format_task_status_open_states_include_request_body() {
        // claim 후(working)에도 원 요청 본문이 보여야 수신자가 DB 직독 없이 재조회한다(세션20 실측).
        let mut task =
            crate::store::a2a::Task::new("t9", None, "boss", "worker", "2026-07-11 09:00:00");
        task.status_message = Some(crate::store::a2a::Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part { text: Some("31*13을 계산해줘".into()), ..Default::default() }],
            task_id: Some("t9".into()),
            context_id: None,
        });
        for state in [TaskState::Submitted, TaskState::Working, TaskState::InputRequired] {
            task.state = state;
            let text = format_task_status(&task, "2026-07-11 09:00:01");
            assert!(text.contains("[요청]"), "본문 라벨 누락({:?}): {text}", task.state);
            assert!(text.contains("31*13"), "본문 누락({:?}): {text}", task.state);
        }
        // completed는 기존대로 artifact만(요청 본문 미표시).
        task.state = TaskState::Completed;
        let text = format_task_status(&task, "2026-07-11 09:00:01");
        assert!(!text.contains("[요청]"), "completed에 본문이 붙으면 안 됨: {text}");
    }

    #[test]
    fn poll_tasks_text_filters_agent_and_excludes_completed() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00"); // open, mac 앞.
        let mut t2 = crate::store::a2a::Task::new("t2", None, "win", "mac", "2026-07-02 09:05:00");
        t2.state = TaskState::Completed;
        store.create_task(&t2).unwrap(); // completed, mac 앞 → 제외돼야 함.
        seed_task(&store, "t3", "win", "other", "2026-07-02 09:10:00"); // open, other 앞 → 제외돼야 함.

        let text = poll_tasks_text(&store, "mac").unwrap();
        assert!(text.contains("t1"), "열린 task 누락: {text}");
        assert!(!text.contains("t2"), "completed가 섞여 들어옴: {text}");
        assert!(!text.contains("t3"), "다른 agent 앞 task가 섞여 들어옴: {text}");
    }

    #[test]
    fn poll_tasks_text_sweeps_expired_lease_before_listing() {
        // poll_tasks_text 호출 자체가 지연 sweep을 트리거해, 죽은 워커가 물고 있던 task가 다시
        // submitted로 노출되어야 한다(별도 타이머 없이 poll 경로에 얹는 설계의 핵심).
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        store.try_claim("t1", Some("worker-a"), None).unwrap();
        // lease를 과거로 강제 심어 만료를 시뮬레이션(테스트 전용 pub(crate) 헬퍼, conn은 private).
        store.test_force_lease_expired("t1");

        let text = poll_tasks_text(&store, "mac").unwrap();
        assert!(text.contains("state=submitted"), "sweep 후 submitted로 복귀 안 됨: {text}");
    }

    #[test]
    fn claim_task_text_transitions_to_working() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = claim_task_text(&store, "t1", None, None).unwrap();
        assert!(text.contains("state=working"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Working);
    }

    #[test]
    fn claim_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = claim_task_text(&store, "nope", None, None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    #[test]
    fn complete_task_text_sets_completed_with_artifact() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        claim_task_text(&store, "t1", None, None).unwrap();
        let text = complete_task_text(&store, "t1", "작업 결과", None).unwrap();
        assert!(text.contains("state=completed"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Completed);
        assert_eq!(reloaded.artifacts.len(), 1);
        assert_eq!(reloaded.artifacts[0].parts[0].text.as_deref(), Some("작업 결과"));
    }

    #[test]
    fn complete_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = complete_task_text(&store, "nope", "결과", None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    #[test]
    fn fail_task_text_sets_failed_with_reason() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = fail_task_text(&store, "t1", "러너 타임아웃", None).unwrap();
        assert!(text.contains("state=failed"), "응답 불일치: {text}");
        let reloaded = store.get_task("t1").unwrap().unwrap();
        assert_eq!(reloaded.state, TaskState::Failed);
        // 사유는 상태 메시지로 남아 dispatcher가 읽을 수 있다.
        assert_eq!(
            reloaded.status_message.and_then(|m| m.parts[0].text.clone()).as_deref(),
            Some("러너 타임아웃")
        );
    }

    #[test]
    fn fail_task_text_missing_task_is_err() {
        let store = SqliteStore::open_memory().unwrap();
        let err = fail_task_text(&store, "nope", "사유", None).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지에 task_id 없음: {err}");
    }

    // --- health_annotation(표시 전용 stuck?/no-consumer? 주석): 순수 함수 단위테스트 ---

    #[test]
    fn health_annotation_working_stuck_past_threshold() {
        let mut task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        task.state = TaskState::Working;
        task.updated_at = "2026-07-02 09:00:00".into(); // claim 시각.
        // STUCK_WORKING_SECS(15분) 초과: 09:00:00 -> 09:20:00 = 20분.
        let annotation = health_annotation(&task, "2026-07-02 09:20:00");
        assert!(annotation.contains("stuck?"), "stuck 표시 누락: {annotation}");
        assert!(annotation.contains("20m"), "경과분 표시 불일치: {annotation}");
    }

    #[test]
    fn health_annotation_submitted_no_consumer_past_threshold() {
        let task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        // NO_CONSUMER_SUBMITTED_SECS(5분) 초과: 09:00:00 -> 09:10:00 = 10분.
        let annotation = health_annotation(&task, "2026-07-02 09:10:00");
        assert!(annotation.contains("no-consumer?"), "no-consumer 표시 누락: {annotation}");
        assert!(annotation.contains("10m"), "경과분 표시 불일치: {annotation}");
    }

    #[test]
    fn health_annotation_recent_task_is_empty() {
        let task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        // 임계(5분) 이내: 09:00:00 -> 09:01:00 = 1분.
        let annotation = health_annotation(&task, "2026-07-02 09:01:00");
        assert_eq!(annotation, "", "임계 이내인데 주석이 붙음: {annotation}");
    }

    #[test]
    fn health_annotation_terminal_state_is_always_empty() {
        let mut task = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        task.state = TaskState::Completed;
        task.updated_at = "2026-07-02 09:00:00".into();
        // 아주 오래 지났어도 종료 상태(completed)는 주석을 붙이지 않는다.
        let annotation = health_annotation(&task, "2026-07-03 09:00:00");
        assert_eq!(annotation, "", "종료 상태인데 주석이 붙음: {annotation}");
    }

    #[test]
    fn classify_task_health_returns_variants_for_health_panel_counts() {
        // 헬스 패널(/dashboard/health)이 no-consumer/stuck 집계에 의존하는 enum 계약을 잠근다.
        let submitted = crate::store::a2a::Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
        assert!(
            matches!(classify_task_health(&submitted, "2026-07-02 09:10:00"), TaskHealth::NoConsumer(secs) if secs == 600),
            "submitted 10분 = NoConsumer(600) 여야 함",
        );

        let mut working = crate::store::a2a::Task::new("t2", None, "win", "mac", "2026-07-02 09:00:00");
        working.state = TaskState::Working;
        working.updated_at = "2026-07-02 09:00:00".into();
        assert!(
            matches!(classify_task_health(&working, "2026-07-02 09:20:00"), TaskHealth::Stuck(secs) if secs == 1200),
            "working 20분 = Stuck(1200) 여야 함",
        );

        // 임계 이내는 Ok(집계 제외).
        assert!(matches!(classify_task_health(&submitted, "2026-07-02 09:01:00"), TaskHealth::Ok));
    }

    // --- tasks 툴(list_all_tasks_text): 순수 함수 단위테스트 ---

    #[test]
    fn list_all_tasks_text_mixes_multiple_to_agents() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        seed_task(&store, "t2", "win", "codex", "2026-07-02 09:05:00");
        let text = list_all_tasks_text(&store, "2026-07-02 09:06:00").unwrap();
        assert!(text.contains("t1"), "t1 누락: {text}");
        assert!(text.contains("to=mac"), "to=mac 누락: {text}");
        assert!(text.contains("t2"), "t2 누락: {text}");
        assert!(text.contains("to=codex"), "to=codex 누락: {text}");
    }

    #[test]
    fn list_all_tasks_text_shows_no_consumer_annotation_for_stale_submitted() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // now를 미래로 둬 NO_CONSUMER_SUBMITTED_SECS(5분)을 넘긴다.
        let text = list_all_tasks_text(&store, "2026-07-02 09:30:00").unwrap();
        assert!(text.contains("no-consumer?"), "no-consumer 주석 누락: {text}");
    }

    #[test]
    fn list_all_tasks_text_empty_says_no_open_tasks() {
        let store = SqliteStore::open_memory().unwrap();
        let text = list_all_tasks_text(&store, "2026-07-02 09:00:00").unwrap();
        assert!(text.contains("없음"), "빈 목록 안내가 아님: {text}");
    }

    // --- A2A dispatcher 툴(send_task/get_task): 순수 함수 단위테스트 ---

    #[test]
    fn send_task_text_creates_submitted_task_and_preserves_text() {
        let store = SqliteStore::open_memory().unwrap();
        let text =
            send_task_text(&store, "win-claude", "mac-claude", "리뷰 부탁", Some("ctx1".into())).unwrap();
        assert!(text.contains("state=submitted"), "응답 불일치: {text}");

        // store에 실제로 submitted task가 생겼는지, 메시지 본문이 보존됐는지 확인(round-trip).
        let tasks = store.list_open_tasks_for("mac-claude").unwrap();
        assert_eq!(tasks.len(), 1, "mac-claude 앞 task 하나가 생겨야 함");
        let task = &tasks[0];
        assert_eq!(task.from_agent, "win-claude");
        assert_eq!(task.context_id.as_deref(), Some("ctx1"));
        assert_eq!(
            task.status_message.as_ref().and_then(|m| m.parts.first()).and_then(|p| p.text.as_deref()),
            Some("리뷰 부탁")
        );
    }

    #[test]
    fn get_task_text_missing_task_says_not_found() {
        let store = SqliteStore::open_memory().unwrap();
        let text = get_task_text(&store, "nope").unwrap();
        assert!(text.contains("없음"), "미존재 안내 불일치: {text}");
        assert!(text.contains("nope"), "task_id 언급 없음: {text}");
    }

    // --- 레지스트리 라우팅: 순수 함수 단위테스트는 store::agents로 이동(Plan v2-34 T3) ---

    #[test]
    fn format_agents_empty_says_none_online() {
        assert_eq!(format_agents(&[]), "online 에이전트 없음");
    }

    #[test]
    fn format_agents_formats_uuid_name_tags_heartbeat() {
        let mut tags = BTreeMap::new();
        tags.insert("machine".to_string(), "win".to_string());
        let agents = vec![AgentEntry {
            uuid: "uuid-1".to_string(),
            tags,
            display_name: Some("win-claude".to_string()),
            last_heartbeat: "2026-07-04 10:00:00".to_string(),
            human_input_at: None,
        }];
        let text = format_agents(&agents);
        assert!(text.contains("uuid-1"));
        assert!(text.contains("win-claude"));
        assert!(text.contains("machine=win"));
        assert!(text.contains("2026-07-04 10:00:00"));
    }


    #[test]
    fn send_task_routed_selector_zero_matches_is_no_consumer_and_no_task_created() {
        let store = SqliteStore::open_memory().unwrap();
        let text = send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None)
            .unwrap();
        assert!(text.contains("no-consumer") || text.contains("대상 없음"), "안내 불일치: {text}");
        assert!(store.list_all_open_tasks().unwrap().is_empty(), "task가 생성되면 안 됨");
    }

    #[test]
    fn send_task_routed_selector_one_match_creates_task_to_that_uuid() {
        let store = SqliteStore::open_memory().unwrap();
        let now = store.now().unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("runner".to_string(), "claude".to_string());
        store.register_agent("uuid-only", tags, None, &now);

        let text =
            send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None).unwrap();
        assert!(text.contains("state=submitted"), "응답 불일치: {text}");

        let tasks = store.list_open_tasks_for("uuid-only").unwrap();
        assert_eq!(tasks.len(), 1, "uuid-only 앞으로 정확히 하나 생성돼야 함");
    }

    #[test]
    fn send_task_routed_selector_multiple_matches_lists_candidates_and_no_task_created() {
        let store = SqliteStore::open_memory().unwrap();
        let now = store.now().unwrap();
        let mut tags = BTreeMap::new();
        tags.insert("runner".to_string(), "claude".to_string());
        store.register_agent("uuid-a", tags.clone(), None, &now);
        store.register_agent("uuid-b", tags, None, &now);

        let text =
            send_task_routed(&store, "win-claude", None, Some("runner=claude"), "부탁", None).unwrap();
        assert!(text.contains("uuid-a"), "후보 목록 불일치: {text}");
        assert!(text.contains("uuid-b"), "후보 목록 불일치: {text}");
        assert!(store.list_all_open_tasks().unwrap().is_empty(), "task가 생성되면 안 됨");
    }

    #[test]
    fn get_task_text_open_task_shows_state_without_artifacts() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        let text = get_task_text(&store, "t1").unwrap();
        assert!(text.contains("state=submitted"), "state 누락: {text}");
    }

    #[test]
    fn get_task_text_completed_task_appends_artifact_text() {
        let store = SqliteStore::open_memory().unwrap();
        seed_task(&store, "t1", "win", "mac", "2026-07-02 09:00:00");
        // R2: try_complete는 working 상태에서만 성공하므로, 먼저 claim해 착수 상태로 만든다.
        claim_task_text(&store, "t1", None, None).unwrap();
        complete_task_text(&store, "t1", "작업 결과 요약", None).unwrap();
        let text = get_task_text(&store, "t1").unwrap();
        assert!(text.contains("state=completed"), "state 누락: {text}");
        assert!(text.contains("작업 결과 요약"), "artifact 텍스트 누락: {text}");
    }
}
