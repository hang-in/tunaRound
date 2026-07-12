// A2A task 재생(replay)·mesh 색인 스탬프·retention 슬림화(과거 종결 task 조회·정리).

use super::super::{SqliteStore, TASK_COLUMNS};
use super::{ReplayLimit, task_row_from_sql};
use crate::store::a2a::{Task, TaskRow};

impl SqliteStore {
    /// 과거 task를 재생(replay)용으로 조회하는 공용 질의(v2-45 P2, 설계 §3). 피드 스냅샷(?replay=N)과
    /// watch-results 재생(?since=TS)이 이 하나로 수렴한다(질의 중복 설계 방지).
    ///
    /// - `from_agent`: Some이면 발신자 필터(watch-results의 dispatcher 의미. None=전체).
    /// - `since`: Some이면 `updated_at >= ?` 필터. 포맷은 DB `datetime('now')` 그대로
    ///   ("YYYY-MM-DD HH:MM:SS" UTC, 사전순 비교 가능). ISO8601 변환 금지(§5-3 고정 계약:
    ///   'T' > ' ' 사전순 왜곡).
    /// - `states`: 빈 배열이면 전 상태, 아니면 `state IN (...)` 필터.
    /// - `limit`: [`ReplayLimit`] 참조(Newest=최근 N건, Oldest=오래된 것부터 N건, All=전량).
    ///
    /// 반환은 항상 updated_at 오름차순(소비자 = SSE 선행 프레임이 시간순으로 흘러야 함). 같은 초의
    /// tie는 rowid를 2차 키로 안정화한다.
    ///
    /// **Oldest 상한의 전제**(watch-results catch-up 연쇄, v2-45 P3): 클라이언트 워터마크가 초 단위
    /// (updated_at)라, 단일 초에 종결 task가 상한(N)을 초과하면 Oldest(N)이 매번 같은 rowid 앞 N건만
    /// 돌려주고 워터마크가 그 초를 넘지 못해 나머지가 재생에서 누락된다(초 내 페이지네이션 불가).
    /// 운영 처리율(주간 약 100건)에선 비도달이며, 근본 해소는 (updated_at, rowid) 복합 커서로의
    /// 확장이다(현재는 단순성 우선 비채택 - list_tasks_replay_oldest_wedges_within_same_second로 고정).
    pub fn list_tasks_replay(
        &self,
        from_agent: Option<&str>,
        since: Option<&str>,
        states: &[&str],
        limit: ReplayLimit,
    ) -> Result<Vec<Task>, String> {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<String> = Vec::new();
        if let Some(agent) = from_agent {
            params.push(agent.to_string());
            clauses.push(format!("from_agent=?{}", params.len()));
        }
        if let Some(ts) = since {
            params.push(ts.to_string());
            clauses.push(format!("updated_at >= ?{}", params.len()));
        }
        if !states.is_empty() {
            let placeholders: Vec<String> = states
                .iter()
                .map(|s| {
                    params.push(s.to_string());
                    format!("?{}", params.len())
                })
                .collect();
            clauses.push(format!("state IN ({})", placeholders.join(", ")));
        }
        let where_sql = if clauses.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", clauses.join(" AND "))
        };
        // Newest는 "최근 N건"이므로 DESC로 끊은 뒤 Rust에서 뒤집어 오름차순 계약을 지킨다.
        // Oldest는 ASC LIMIT가 곧 "앞에서부터 N건"이라 그대로 반환한다.
        let order_sql = match limit {
            ReplayLimit::Newest(n) => format!(" ORDER BY updated_at DESC, rowid DESC LIMIT {n}"),
            ReplayLimit::Oldest(n) => format!(" ORDER BY updated_at ASC, rowid ASC LIMIT {n}"),
            ReplayLimit::All => " ORDER BY updated_at ASC, rowid ASC".to_string(),
        };
        let sql = format!("SELECT {TASK_COLUMNS} FROM tasks{where_sql}{order_sql}");
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map(rusqlite::params_from_iter(params.iter()), task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        let mut tasks: Vec<Task> = rows
            .into_iter()
            .map(TaskRow::into_task)
            .collect::<Result<Vec<_>, _>>()?;
        if matches!(limit, ReplayLimit::Newest(_)) {
            tasks.reverse();
        }
        Ok(tasks)
    }

    /// task가 mesh 기억에 색인됐음을 스탬프한다(v2-45 P6a). indexed_at NULL인 종결 task만 백필·재색인
    /// 대상이므로, 색인 성공 시 이 스탬프로 제외된다. best-effort 호출부라 존재하지 않는 task_id도 Ok(0행).
    pub fn mark_task_indexed(&self, task_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE tasks SET indexed_at = datetime('now') WHERE task_id = ?1",
                [task_id],
            )
            .map(|_| ())
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// 색인된(indexed_at NOT NULL) 종결 task 중 updated_at이 보존기간을 넘긴 것을 슬림화한다(v2-45 P6b
    /// retention). history_json='[]'로 비우고, **completed는 message_json(원 요청)도 NULL**로 비운다
    /// (요청은 이미 mesh 기억에 색인됨). **artifacts_json과 failed의 message_json(실패 사유)은 보존**한다
    /// (§5-5: get_task 재조회·watch-results 전문 재조회 창구, 행 수명 내내). **행 삭제는 없다.**
    /// 이미 슬림화된 행(history_json='[]')은 건너뛰어 재작업·재카운트를 피한다. 반환=슬림화한 행 수.
    pub fn prune_terminal_tasks(&self, retain_days: u32) -> Result<usize, String> {
        let cutoff = format!("-{retain_days} days");
        // completed·failed를 한 원자적 UPDATE로 슬림화(봇 리뷰): history_json='[]'로 비우고, completed만
        // message_json(원 요청)도 NULL로(CASE). failed의 message_json(실패 사유)과 artifacts_json은
        // 보존(§5-5). WHERE의 OR 조건은 history가 이미 '[]'여도 completed에서 message_json이 남아 있으면
        // 마저 정리해 경계 조건을 없앤다(완전 슬림 행은 재매칭 안 돼 멱등).
        self.conn
            .execute(
                "UPDATE tasks \
                 SET history_json='[]', \
                     message_json=CASE WHEN state='completed' THEN NULL ELSE message_json END \
                 WHERE state IN ('completed','failed') AND indexed_at IS NOT NULL \
                   AND updated_at < datetime('now', ?1) \
                   AND (history_json != '[]' OR (state='completed' AND message_json IS NOT NULL))",
                [&cutoff],
            )
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// WAL을 체크포인트하고 파일을 잘라 공간을 회수한다(v2-45 P6b: 슬림화 sweep 동반, 수동 정리 실측 해소).
    pub fn wal_checkpoint(&self) -> Result<(), String> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("sqlite: {e}"))
    }

    /// 아직 색인되지 않은 종결(completed/failed) task를 오름차순으로 반환한다(v2-45 P6a 기동 백필).
    /// canceled·열린 task는 대상 아님("결과 있는 종결만 색인" 스코프). indexed_at은 DB 내부 컬럼이라
    /// Task wire에는 없으므로 WHERE 절로만 필터한다.
    pub fn list_unindexed_terminal_tasks(&self) -> Result<Vec<Task>, String> {
        let sql = format!(
            "SELECT {TASK_COLUMNS} FROM tasks \
             WHERE state IN ('completed','failed') AND indexed_at IS NULL \
             ORDER BY updated_at ASC, rowid ASC"
        );
        let mut stmt = self
            .conn
            .prepare(&sql)
            .map_err(|e| format!("sqlite: {e}"))?;
        let rows: Vec<TaskRow> = stmt
            .query_map([], task_row_from_sql)
            .map_err(|e| format!("sqlite: {e}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| format!("sqlite: {e}"))?;
        rows.into_iter()
            .map(TaskRow::into_task)
            .collect::<Result<Vec<_>, _>>()
    }
}
