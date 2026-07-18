// 종결(completed/failed) A2A task를 mesh 기억(messages/FTS)에 색인하는 순수 로직·기동 백필 헬퍼.

use std::sync::{Arc, Mutex};

use crate::orchestrator::TranscriptWriter;
use crate::store::a2a::{Task, TaskState};
use crate::store::sqlite::SqliteStore;

// ---------------------------------------------------------------------------
// v2-45 P6a: mesh 기억화 = 종결 task의 요청문+결과를 messages/FTS에 색인(search_context로 위임 이력 검색).
// ---------------------------------------------------------------------------

/// 종결 task 색인에 필요한 최소 정보(락 밖에서 writer로 색인하기 위해 락 안에서 미리 뽑는다).
pub(crate) struct TerminalIndexPayload {
    pub(crate) task_id: String,
    pub(crate) from_agent: String,
    pub(crate) to_agent: String,
    pub(crate) runner: Option<String>,
    /// 원 요청문(history[0]). 없으면 결과만 색인.
    pub(crate) request_text: Option<String>,
    /// 결과: completed=artifact 텍스트, failed=상태 메시지 텍스트. 없으면 요청만 색인.
    pub(crate) result_text: Option<String>,
}

/// 종결(completed/failed) task에서 색인 payload를 뽑는다(§5-7 네임스페이스용). 요청=history[0],
/// 결과=completed면 artifact·failed면 status_message. **비종결(canceled·열린)만 None**이다.
/// 결과 텍스트가 없어도 요청문만 있으면 색인한다: 결과 없다고 None을 주면 백필이 색인 없이 indexed_at을
/// 스탬프하고, P6b prune이 그걸 "mesh에 있음"으로 신뢰해 요청(history)을 영구 삭제해버리는 손실이 생긴다
/// (적대 리뷰 confirmed). "indexed_at ⟹ 텍스트 내용이 mesh에(또는 애초에 없음)" 불변식을 지킨다.
pub(crate) fn build_terminal_index_payload(task: &Task) -> Option<TerminalIndexPayload> {
    if !matches!(task.state, TaskState::Completed | TaskState::Failed) {
        return None; // canceled·열린 task는 색인 비대상(§4 P6a).
    }
    // v2-56: 토론 라운드 task의 요청문은 색인하지 않는다. 라운드 프롬프트는 prior 발언 전량 재조립이라
    // task마다 같은 발언이 O(좌석×라운드)배로 FTS에 중복 축적되고, speaker `a2a/debate:<id>`가
    // 대시보드 검색 스코프(a2a/*)를 통과해 §8-2 비노출 결정도 깨진다(적대 리뷰 major). 라운드 맥락의
    // 정본은 debate:<id> 전사(§6-4)이고, 결과(발언 자체)만 1배 중복으로 색인한다.
    let request_text = if task
        .from_agent
        .starts_with(crate::discussion::DEBATE_NS_PREFIX)
    {
        None
    } else {
        task.history
            .first()
            .and_then(|m| m.parts.first())
            .and_then(|p| p.text.clone())
    };
    let result_text = match task.state {
        TaskState::Completed => task
            .artifacts
            .first()
            .and_then(|a| a.parts.first())
            .and_then(|p| p.text.clone()),
        _ => task
            .status_message
            .as_ref()
            .and_then(|m| m.parts.first())
            .and_then(|p| p.text.clone()),
    };
    Some(TerminalIndexPayload {
        task_id: task.id.clone(),
        from_agent: task.from_agent.clone(),
        to_agent: task.to_agent.clone(),
        runner: task.runner.clone(),
        request_text,
        result_text,
    })
}

/// 종결 task 하나를 mesh 기억에 색인한다(v2-45 P6a). 네임스페이스(§5-7): session_id=`a2a:<task_id>`,
/// speaker=`a2a/<agent>`(요청=from, 결과=to 또는 runner). writer는 자체 store 연결이라 a2a_store 락과
/// 무관하다(락 순서: a2a_store 해제 후 호출). best-effort - 색인 실패는 종결을 되돌리지 않고 로그만 남기며
/// indexed_at을 스탬프하지 않아 다음 백필이 재시도한다. 양쪽 turn이 성공해야 스탬프한다.
pub(crate) fn index_terminal_task(
    writer: &Arc<dyn TranscriptWriter>,
    a2a_store: &Arc<Mutex<SqliteStore>>,
    p: &TerminalIndexPayload,
) {
    let sid = format!("a2a:{}", p.task_id);
    // 색인 전체(delete→append→stamp)를 a2a_store 락 하나로 index_terminal_task 호출자 간 직렬화한다
    // (적대 리뷰 confirmed race 차단. a2a:<sid>의 유일한 쓰기 진입점이 이 함수라 이 직렬화로 충분).
    // 동시 색인자 = 기동 backfill(server.rs) vs 라이브 종결의 fire-and-forget spawn_blocking(tasks.rs)이
    // 같은 sid에 delete-then-append를 인터리빙하면 중복(2×req+2×res)·유실이 생긴다(락을 매 단계 놓았다
    // 잡으면 열리는 창). 락을 잡은 채로 append하면 뒤 색인자가 delete로 앞을 덮고 재-append=단일 사본(멱등).
    // writer는 별개 연결·별개 뮤텍스라(cli_run: writer=store3, a2a_store=store4) 이 락을 잡은 채 호출해도
    // 데드락이 없다: 락 순서는 항상 a2a_store→writer이고 writer(post_turn 포함)는 a2a_store를 역방향으로
    // 잡지 않는다. backfill은 목록 수집 후 락을 놓고 이 함수를 부르므로 std Mutex 비재진입 데드락도 없다.
    // best-effort 불변: 색인 실패는 종결을 되돌리지 않고 stamp를 건너뛰어 다음 백필이 재시도한다.
    let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
    if let Err(e) = store.delete_session_messages(&sid) {
        eprintln!("[index] task {} 기존 색인 정리 실패(무시): {e}", p.task_id);
    }
    let mut ok = true;
    if let Some(req) = &p.request_text
        && let Err(e) = writer.append_turn(&sid, &format!("a2a/{}", p.from_agent), req)
    {
        eprintln!("[index] task {} 요청 색인 실패(무시): {e}", p.task_id);
        ok = false;
    }
    let result_speaker = p.runner.as_deref().unwrap_or(&p.to_agent);
    if let Some(res) = &p.result_text
        && let Err(e) = writer.append_turn(&sid, &format!("a2a/{result_speaker}"), res)
    {
        eprintln!("[index] task {} 결과 색인 실패(무시): {e}", p.task_id);
        ok = false;
    }
    if ok && let Err(e) = store.mark_task_indexed(&p.task_id) {
        eprintln!(
            "[index] task {} indexed_at 스탬프 실패(무시): {e}",
            p.task_id
        );
    }
}

/// 기동 시 미색인 종결 task를 mesh 기억에 백필한다(v2-45 P6a). 구 바이너리 시절 완료분·색인 유실
/// (expire_stale_claims 등)을 재기동 때 메운다. best-effort(개별 실패는 다음 기동이 재시도).
pub(crate) fn backfill_unindexed_terminal_tasks(
    a2a_store: &Arc<Mutex<SqliteStore>>,
    writer: &Arc<dyn TranscriptWriter>,
) {
    let tasks = {
        let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
        match store.list_unindexed_terminal_tasks() {
            Ok(t) => t,
            Err(e) => {
                eprintln!("[index] 백필 조회 실패(무시): {e}");
                return;
            }
        }
    };
    if tasks.is_empty() {
        return;
    }
    let n = tasks.len();
    for task in &tasks {
        match build_terminal_index_payload(task) {
            Some(payload) => index_terminal_task(writer, a2a_store, &payload),
            None => {
                // 결과 텍스트 없는 종결(레거시·expire→failed 등): 색인할 것이 없으니 스탬프만 해
                // 목록에서 제외한다(적대 리뷰 minor: 미스탬프 시 매 기동 무한 재스캔·비수렴).
                let store = a2a_store.lock().unwrap_or_else(|e| e.into_inner());
                let _ = store.mark_task_indexed(&task.id);
            }
        }
    }
    eprintln!("[index] 기동 백필: 미색인 종결 task {n}건 처리");
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::a2a::{Artifact, Message, Part};

    fn completed_task(from_agent: &str) -> Task {
        let mut t = Task::new(
            "t1".to_string(),
            None,
            from_agent,
            "seat-a",
            "2026-07-18 00:00:00".to_string(),
        );
        t.state = TaskState::Completed;
        t.history = vec![Message {
            message_id: "m1".to_string(),
            role: "user".to_string(),
            parts: vec![Part {
                text: Some("라운드 프롬프트(prior 전량 포함)".to_string()),
                ..Default::default()
            }],
            task_id: Some("t1".to_string()),
            context_id: None,
        }];
        t.artifacts = vec![Artifact {
            artifact_id: "a1".to_string(),
            name: None,
            parts: vec![Part {
                text: Some("발언".to_string()),
                ..Default::default()
            }],
        }];
        t
    }

    #[test]
    fn debate_tasks_index_result_only() {
        // v2-56: debate:* 발신 task는 요청문(라운드 재조립) 비색인, 결과(발언)만 색인.
        let p = build_terminal_index_payload(&completed_task("debate:abc")).unwrap();
        assert!(p.request_text.is_none(), "debate 요청문은 비색인");
        assert_eq!(p.result_text.as_deref(), Some("발언"));
        // 일반 위임 task는 기존대로 요청+결과 색인.
        let p2 = build_terminal_index_payload(&completed_task("win-boss")).unwrap();
        assert!(p2.request_text.is_some());
        assert_eq!(p2.result_text.as_deref(), Some("발언"));
    }
}
