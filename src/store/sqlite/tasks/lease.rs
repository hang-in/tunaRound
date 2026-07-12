// A2A task lease: claim·lease 연장·만료 회수(claim-후-워커사망 자동 requeue).

use super::super::{CLAIM_LEASE_SECS, MAX_CLAIM_ATTEMPTS, SqliteStore};
use crate::store::a2a::{TaskEvent, TaskState};

impl SqliteStore {
    /// task를 조건부로 working 전이한다(claim). WHERE 절의 현재상태 가드가 상태머신을 저장소 계층에서
    /// 강제한다: submitted/input_required일 때만 성공하므로, 두 워커가 같은 task를 동시에 claim해도
    /// UPDATE 하나만 1행을 갱신하고 나머지는 0행(rows_affected!=1)으로 걸러진다(레이스 방지).
    /// status_message(=task 지시문)는 지우지 않고 그대로 둔다: requeue로 다시 submitted가 되면 새 워커가
    /// poll에서 이 지시문(msg)을 읽어 실행하므로, claim에서 지우면 재배달된 task가 빈 프롬프트가 된다.
    /// 같은 UPDATE에서 lease(claimed_at/lease_expires_at/claimed_by)를 세팅하고 attempt_count를
    /// 증가시켜, claim-후-워커사망 자동 requeue(expire_stale_claims)의 판단 근거를 남긴다. claimed_by는
    /// 하위호환을 위해 Option(호출자가 agent id를 안 넘기면 NULL). runner도 같은 순간 기록한다(v8,
    /// 어떤 러너 종류가 처리했는지 트레이스용, 호출자가 안 넘기면 NULL).
    /// 전이 성공 시에만 update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_claim(
        &self,
        task_id: &str,
        claimed_by: Option<&str>,
        runner: Option<&str>,
    ) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, updated_at=datetime('now'), \
                 claimed_at=datetime('now'), \
                 lease_expires_at=datetime('now', '+' || ?3 || ' seconds'), \
                 claimed_by=?4, runner=?5, attempt_count=attempt_count + 1 \
                 WHERE task_id=?1 AND state IN ('submitted','input_required')",
                rusqlite::params![
                    task_id,
                    TaskState::Working.as_str(),
                    CLAIM_LEASE_SECS,
                    claimed_by,
                    runner
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!(
                "전이 불가: task_id={task_id} (현재 상태가 대상 아님)"
            ));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// claim한 워커가 살아 있는 동안 자기 task의 lease를 갱신한다(장기 task가 실행 중 requeue되는 것
    /// 방지, v2-49 #6). working이고 claimed_by가 일치할 때만 lease_expires_at을 now+CLAIM_LEASE_SECS로
    /// 밀고 updated_at도 갱신한다(살아있는 워커의 task는 ⚠stuck? 표시가 뜨지 않게). 상태 전이가 아니라
    /// keepalive라 이벤트는 emit하지 않는다(SSE 노이즈 방지). 대상이 아니면(종료됐거나 다른 워커 소유)
    /// affected!=1로 Err를 돌려, 워커가 이미 requeue/재claim된 상황을 로그로 인지하게 한다.
    pub fn extend_lease(&self, task_id: &str, claimed_by: &str) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET \
                 lease_expires_at=datetime('now', '+' || ?2 || ' seconds'), \
                 updated_at=datetime('now') \
                 WHERE task_id=?1 AND state='working' AND claimed_by=?3",
                rusqlite::params![task_id, CLAIM_LEASE_SECS, claimed_by],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!(
                "lease 연장 불가: task_id={task_id} (working 아님 또는 claimed_by 불일치)"
            ));
        }
        Ok(())
    }

    /// lease 만료된 working task를 회수한다(poll 경로에서 호출되는 지연 sweep, 별도 타이머 없음).
    /// lease_expires_at이 지난 working 중 attempt_count < MAX_CLAIM_ATTEMPTS면 submitted로 되돌리고
    /// (claim 필드 클리어, attempt_count는 유지해 다음 claim에서 다시 증가), MAX 이상이면 무한 requeue를
    /// 막기 위해 failed로 격리한다. status_message(지시문)는 건드리지 않는다: 재배달된 워커가 poll에서
    /// 그 지시문을 읽어 실행해야 하기 때문(try_claim이 지시문을 보존하는 것과 짝). 두 UPDATE는 서로소
    /// 조건(전자가 먼저 실행돼도 그 행들은 이미 state!='working'으로 바뀌어 후자의 WHERE에 안 걸림)이라
    /// 실행 순서가 결과에 영향을 주지 않는다. 이벤트 emit은 의도적으로 생략한다(다건 sweep이라 poll마다
    /// 이벤트가 몰릴 수 있고, 구독자는 다음 poll_tasks/get_task로 최신 상태를 확인할 수 있어 SSE 실시간성
    /// 손실이 크지 않다고 판단). 회수(requeue)·격리(failed) 합계 행 수를 반환한다.
    pub fn expire_stale_claims(&self) -> Result<usize, String> {
        // 단일 UPDATE(암묵적 단일 트랜잭션 = 원자적·fsync 1회)로 처리한다. attempt_count로 갈라
        // 회수(attempt<MAX: submitted + claim 필드 클리어) 또는 격리(attempt>=MAX: failed, claim 필드는
        // 포렌식용으로 유지)한다. status_message(지시문)는 어느 쪽도 건드리지 않아 재배달 워커가 읽는다.
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET \
                 state = CASE WHEN attempt_count < ?2 THEN ?1 ELSE ?3 END, \
                 claimed_at = CASE WHEN attempt_count < ?2 THEN NULL ELSE claimed_at END, \
                 lease_expires_at = CASE WHEN attempt_count < ?2 THEN NULL ELSE lease_expires_at END, \
                 claimed_by = CASE WHEN attempt_count < ?2 THEN NULL ELSE claimed_by END, \
                 runner = CASE WHEN attempt_count < ?2 THEN NULL ELSE runner END, \
                 updated_at = datetime('now') \
                 WHERE state='working' AND lease_expires_at IS NOT NULL \
                 AND julianday('now') > julianday(lease_expires_at)",
                rusqlite::params![
                    TaskState::Submitted.as_str(),
                    MAX_CLAIM_ATTEMPTS,
                    TaskState::Failed.as_str()
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(affected)
    }
}
