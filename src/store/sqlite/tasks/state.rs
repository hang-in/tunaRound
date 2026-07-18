// A2A task 생성·조회·상태 전이(상태머신 도메인: submitted/working/completed/failed/canceled).

use super::super::{SqliteStore, TASK_COLUMNS};
use super::task_row_from_sql;
use crate::store::a2a::{
    Artifact, Message, Task, TaskEvent, TaskRow, TaskState, append_history_json,
};

impl SqliteStore {
    /// 신규 A2A task_id를 생성한다. SQLite 내장 randomblob(16)을 hex로 인코딩(32자)해 신규 crate 의존
    /// 없이 고유 식별자를 얻는다(uuid crate 등 도입 회피).
    pub fn new_task_id(&self) -> Result<String, String> {
        self.conn
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// A2A task를 신규 생성한다(INSERT). created_at/updated_at은 Task 값을 그대로 쓴다(SQL 기본값 없음).
    /// 호출자(dispatcher)가 시각을 stamping해 전달하는 것을 전제한다(round-trip 필드 보존 우선).
    pub fn create_task(&self, task: &Task) -> Result<(), String> {
        let message_json = match &task.status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        let artifacts_json =
            serde_json::to_string(&task.artifacts).map_err(|e| format!("json: {e}"))?;
        let history_json =
            serde_json::to_string(&task.history).map_err(|e| format!("json: {e}"))?;
        self.conn
            .execute(
                &format!(
                    "INSERT INTO tasks({TASK_COLUMNS}) VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)"
                ),
                rusqlite::params![
                    task.id,
                    task.context_id,
                    task.from_agent,
                    task.to_agent,
                    task.state.as_str(),
                    message_json,
                    artifacts_json,
                    history_json,
                    task.created_at,
                    task.updated_at,
                    task.runner,
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }

    /// 메시지 하나로 submitted task를 만들어 영속한다(A2A SendMessage와 MCP send_task 툴이 공유하는
    /// 헬퍼). task_id/시각은 이 함수가 발급하고, message는 status_message이자 history의 첫 항목으로
    /// 그대로 보존한다. a2a_server::handle_send와 mcp::send_task 양쪽이 이 함수로 수렴해 "메시지로
    /// task를 튼다"는 로직이 store 레이어 한 곳에만 존재하게 한다(serve<->mcp 크로스피처 의존 회피).
    pub fn create_task_from_message(
        &self,
        from_agent: &str,
        to_agent: &str,
        message: Message,
    ) -> Result<Task, String> {
        let id = self.new_task_id()?;
        let now = self.now()?;
        let context_id = message.context_id.clone();
        let mut task = Task::new(id, context_id, from_agent, to_agent, now);
        task.status_message = Some(message.clone());
        task.history = vec![message];
        self.create_task(&task)?;
        self.emit_task_event(TaskEvent::Status(task.clone()));
        Ok(task)
    }

    /// task_id로 단건 조회한다. 없으면 Ok(None)(load_session 폴리시 답습: QueryReturnedNoRows만 None).
    pub fn get_task(&self, task_id: &str) -> Result<Option<Task>, String> {
        let row: TaskRow = match self.conn.query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE task_id=?1"),
            [task_id],
            task_row_from_sql,
        ) {
            Ok(r) => r,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(format!("sqlite: {e}")),
        };
        row.into_task().map(Some)
    }

    /// 특정 에이전트(to_agent) 앞으로 열려 있는(submitted/working/input_required) task를
    /// created_at 오름차순으로 반환한다. 상태 리터럴은 TaskState::is_open과 의미를 동기 유지한다.
    pub fn list_open_tasks_for(&self, agent: &str) -> Result<Vec<Task>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks \
                 WHERE to_agent=?1 AND state IN ('submitted','working','input_required') \
                 ORDER BY created_at"
            ))
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map([agent], task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        rows.into_iter().map(TaskRow::into_task).collect()
    }

    /// 브로커 전역에서 열려 있는(submitted/working/input_required) task를 to_agent 필터 없이
    /// created_at 오름차순으로 전부 반환한다(운영자 조망용 tasks MCP 도구 전용, list_open_tasks_for와
    /// 같은 패턴에서 to_agent 조건만 뺀 버전).
    pub fn list_all_open_tasks(&self) -> Result<Vec<Task>, String> {
        let mut stmt = self
            .conn
            .prepare(&format!(
                "SELECT {TASK_COLUMNS} FROM tasks \
                 WHERE state IN ('submitted','working','input_required') \
                 ORDER BY created_at"
            ))
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map([], task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        rows.into_iter().map(TaskRow::into_task).collect()
    }

    /// tasks 테이블을 상태(state)별로 집계한다(`SELECT state, COUNT(*) FROM tasks GROUP BY state`).
    /// 대시보드 StatTiles를 SoR 라이브 질의로 서버소스화(리로드 안정)하기 위한 순수 집계 헬퍼다.
    /// 반환은 "존재하는 상태 → 개수" 맵이다(개수 0인 상태는 키가 없음 → 호출부가 `.unwrap_or(0)`).
    pub fn count_by_state(&self) -> Result<std::collections::HashMap<String, i64>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT state, COUNT(*) FROM tasks GROUP BY state")
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
            })
            .map_err(|e| format!("sqlite: {e}"))?;
        let mut out = std::collections::HashMap::new();
        for row in rows {
            let (state, count) = row.map_err(|e| format!("sqlite: {e}"))?;
            out.insert(state, count);
        }
        Ok(out)
    }

    /// state와 동반 상태 메시지를 원자적으로 갱신한다(A2A TaskStatus 단위). status_message=None이면
    /// 이번 전이에 메시지가 없다는 뜻으로 message_json을 비운다(이전 값 보존 아님).
    /// **상태 가드 없음(무조건 UPDATE) - 테스트 전용.** WHERE에 현재 상태 조건이 없어 terminal 보호·
    /// first-completer-wins를 우회한다. 프로덕션 경로는 try_claim/try_complete/try_fail/try_cancel을
    /// 쓸 것(#6, pub(crate)+cfg(test)로 강등해 크레이트 밖·비테스트 빌드 오용을 차단).
    #[cfg(test)]
    pub(crate) fn update_task_state(
        &self,
        task_id: &str,
        state: TaskState,
        status_message: Option<&Message>,
    ) -> Result<(), String> {
        let message_json = match status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        self.conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1",
                rusqlite::params![task_id, state.as_str(), message_json],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Status(task));
        }
        Ok(())
    }

    /// task를 completed로 마감하고 산출물을 세팅한다.
    /// **상태 가드 없음(무조건 UPDATE) - 테스트 전용.** WHERE에 현재 상태 조건이 없어 terminal 보호·
    /// first-completer-wins를 우회한다. 프로덕션 경로는 try_complete를 쓸 것(#6, pub(crate)+cfg(test)로
    /// 강등해 크레이트 밖·비테스트 빌드 오용을 차단).
    #[cfg(test)]
    pub(crate) fn complete_task(
        &self,
        task_id: &str,
        artifacts: &[Artifact],
    ) -> Result<(), String> {
        let artifacts_json = serde_json::to_string(artifacts).map_err(|e| format!("json: {e}"))?;
        self.conn
            .execute(
                "UPDATE tasks SET state=?2, artifacts_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1",
                rusqlite::params![task_id, TaskState::Completed.as_str(), artifacts_json],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Completed(task));
        }
        Ok(())
    }

    /// task를 조건부로 completed 전이한다(complete). working 상태일 때만 성공한다(레이스 방지: 이미
    /// completed/canceled/failed로 종료된 task를 덮어쓰지 못함). 추가로 first-completer-wins 가드:
    /// completer가 Some이면 claimed_by가 NULL이거나 completer와 일치할 때만 성공한다. lease 만료로
    /// requeue된 뒤 뒤늦게 살아난 stale 워커가 이미 다른 워커가 완료시킨(혹은 재claim된) task를
    /// completer 불일치로 덮어쓰지 못하게 막는다. completer=None이면 이 가드가 무력화되어(claimed_by
    /// 무관하게 working이면 성공) 기존 동작(하위호환, agent 인자 없는 호출)을 그대로 유지한다.
    /// 전이 성공 시에만 complete_task와 동일하게 Completed 이벤트를 emit한다.
    pub fn try_complete(
        &self,
        task_id: &str,
        artifacts: &[Artifact],
        completer: Option<&str>,
    ) -> Result<(), String> {
        let artifacts_json = serde_json::to_string(artifacts).map_err(|e| format!("json: {e}"))?;
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, artifacts_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state='working' \
                 AND (?4 IS NULL OR claimed_by IS NULL OR claimed_by = ?4)",
                rusqlite::params![
                    task_id,
                    TaskState::Completed.as_str(),
                    artifacts_json,
                    completer
                ],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        if affected != 1 {
            return Err(format!(
                "전이 불가: task_id={task_id} (현재 상태가 대상 아님)"
            ));
        }
        if let Some(task) = self.get_task(task_id)? {
            self.emit_task_event(TaskEvent::Completed(task));
        }
        Ok(())
    }

    /// task를 조건부로 failed 전이한다(fail). submitted/working/input_required(=열린 상태)일 때만
    /// 성공한다(레이스 방지: completed/canceled로 이미 종료된 task를 덮어쓰지 못함). status_message는
    /// update_task_state의 관례를 그대로 따른다(None이면 message_json 비움). 전이 성공 시에만
    /// update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_fail(
        &self,
        task_id: &str,
        status_message: Option<&Message>,
        failer: Option<&str>,
    ) -> Result<(), String> {
        let message_json = match status_message {
            Some(m) => Some(serde_json::to_string(m).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        // try_complete와 대칭인 first-completer-wins 가드: lease 만료로 requeue된 뒤 되살아난 stale
        // 워커가 이미 다른 워커가 재claim한 task를 failed로 덮어쓰지 못하게 한다(failer 불일치 거부).
        // failer=None이면 무력화(하위호환, agent 인자 없는 호출 = 브로커/dispatcher 직접 경로).
        // 추가 가드: failer가 Some인데 state='submitted'면 거부한다. expire_stale_claims의 requeue는
        // claimed_by를 NULL로 클리어하고 state를 submitted로 되돌리므로, claimed_by 일치 검사만으로는
        // "직전 소유자 아닌 stale 워커"의 늦은 fail 보고가 통과해버린다(예약된 재시도를 무단 종결).
        // try_complete가 state='working'으로만 제한하는 것과 비대칭이던 부분을 좁힌다. working·
        // input_required는 그대로 두어(claimed_by 가드로 충분) dispatcher의 정당한 직접 fail(failer=None)
        // 경로는 영향받지 않는다.
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=?3, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state IN ('submitted','working','input_required') \
                 AND (?4 IS NULL OR claimed_by IS NULL OR claimed_by = ?4) \
                 AND (?4 IS NULL OR state != 'submitted')",
                rusqlite::params![task_id, TaskState::Failed.as_str(), message_json, failer],
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

    /// task를 조건부로 canceled 전이한다(cancel). submitted/working/input_required(=열린 상태)일 때만
    /// 성공한다(레이스 방지: completed로 이미 끝난 task를 canceled로 덮어쓰지 못함). 전이 성공 시에만
    /// update_task_state와 동일하게 Status 이벤트를 emit한다.
    pub fn try_cancel(&self, task_id: &str) -> Result<(), String> {
        let affected = self
            .conn
            .execute(
                "UPDATE tasks SET state=?2, message_json=NULL, updated_at=datetime('now') \
                 WHERE task_id=?1 AND state IN ('submitted','working','input_required')",
                rusqlite::params![task_id, TaskState::Canceled.as_str()],
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

    /// v2-56 기동 고아 sweep: 토론 driver(인메모리)가 브로커 재기동으로 소멸하면 열린 토론 라운드
    /// task(from_agent가 `debate:` 프리픽스)가 고아로 남는다. 전부 failed로 전이시킨다(사유="broker
    /// restart"). watch-results가 failed terminal을 배달하므로 이 전이가 곧 사용자 통지다(별도 결과
    /// task 없음, Phase 0 토론 합의). 멱등: 열린 debate task가 없으면 0을 반환한다.
    /// 잔여 리스크(수용): 라운드 task가 terminal이 된 직후~다음 라운드 발행 전 수 초 창에서 재기동하면
    /// 열린 task가 0건이라 sweep이 잡지 못하고 토론이 무통지로 끝난다(비동기 인박스 작업 전제라 수용,
    /// 악화 시 debate-liveness 1행으로 표적 확장. v2-56 §7-1).
    pub fn fail_orphan_debate_tasks(&self) -> Result<usize, String> {
        let ids: Vec<String> = {
            let mut stmt = self
                .conn
                .prepare(
                    "SELECT task_id FROM tasks \
                     WHERE from_agent LIKE 'debate:%' \
                     AND state IN ('submitted','working','input_required') \
                     ORDER BY created_at",
                )
                .map_err(|e| format!("sqlite: {e}"))?;
            stmt.query_map([], |row| row.get::<_, String>(0))
                .map_err(|e| format!("sqlite: {e}"))?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| format!("sqlite: {e}"))?
        };
        let mut failed = 0usize;
        for task_id in ids {
            let reason = Message {
                message_id: self.new_task_id()?,
                role: "agent".to_string(),
                parts: vec![crate::store::a2a::Part {
                    text: Some(
                        "브로커 재기동으로 토론이 중단되었습니다(broker restart). 재발의가 필요합니다."
                            .to_string(),
                    ),
                    ..Default::default()
                }],
                task_id: Some(task_id.clone()),
                context_id: None,
            };
            // failer=None = 브로커 직접 경로(모든 열린 상태에서 전이 허용). 개별 실패는 다음 기동 재시도.
            match self.try_fail(&task_id, Some(&reason), None) {
                Ok(()) => failed += 1,
                Err(e) => eprintln!("[debate-sweep] {task_id} 실패 처리 불가(무시): {e}"),
            }
        }
        Ok(failed)
    }

    /// history에 메시지를 append한다(기존 history_json을 읽어 병합 후 저장). 대상 task가 없으면 에러.
    pub fn append_history(&self, task_id: &str, msg: &Message) -> Result<(), String> {
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT history_json FROM tasks WHERE task_id=?1",
                [task_id],
                |row| row.get(0),
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        let updated = append_history_json(existing.as_deref(), msg)?;
        self.conn
            .execute(
                "UPDATE tasks SET history_json=?2, updated_at=datetime('now') WHERE task_id=?1",
                rusqlite::params![task_id, updated],
            )
            .map_err(|e| format!("sqlite: {e}"))?;
        Ok(())
    }
}
