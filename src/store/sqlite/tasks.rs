// A2A task 큐: 상태머신 도메인별 자식 모듈(state/lease/replay)로 분리. 공용 타입 ReplayLimit과 행 매핑 헬퍼만 이 파일에 둔다.

use crate::store::a2a::TaskRow;

mod lease;
mod replay;
mod state;

/// list_tasks_replay의 상한 방향(v2-45 P3). 잘림 의미가 소비자마다 달라 방향을 명시한다:
/// 피드 스냅샷(?replay=N)은 "최근 N건" 창 뷰이고, watch-results catch-up(?since=TS)은
/// "오래된 것부터 N건"이어야 클라이언트 워터마크가 앞에서부터 전진해 재접속 연쇄가 갭 없이 따라잡는다
/// (최근 N건으로 자르면 since와 창 시작 사이가 영영 건너뛰어진다).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplayLimit {
    /// 상한 없음(전량).
    All,
    /// updated_at 기준 최근 N건(DESC LIMIT로 끊고 뒤집어 오름차순 반환).
    Newest(usize),
    /// updated_at 기준 가장 오래된 N건(ASC LIMIT 그대로).
    Oldest(usize),
}

/// tasks SELECT 행 -> TaskRow. query_row/query_map 양쪽에서 재사용(fn 포인터는 Fn/FnMut 모두 충족).
fn task_row_from_sql(row: &rusqlite::Row) -> rusqlite::Result<TaskRow> {
    Ok(TaskRow {
        id: row.get(0)?,
        context_id: row.get(1)?,
        from_agent: row.get(2)?,
        to_agent: row.get(3)?,
        state: row.get(4)?,
        message_json: row.get(5)?,
        artifacts_json: row.get(6)?,
        history_json: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
        runner: row.get(10)?,
    })
}

#[cfg(test)]
use super::*;

/// 테스트 전용 크레이트 내부 헬퍼(cfg(test)라 테스트 빌드에만 존재). `conn`은 이 모듈 밖(예:
/// crate::mcp의 테스트)에서 접근 불가한 private 필드라, lease 만료를 raw SQL로 강제 시뮬레이션해야
/// 하는 크로스모듈 테스트가 이 pub(crate) 통로로 우회한다(운영 코드 경로에는 영향 없음).
#[cfg(test)]
impl SqliteStore {
    pub(crate) fn test_force_lease_expired(&self, task_id: &str) {
        self.conn
            .execute(
                "UPDATE tasks SET lease_expires_at=datetime('now','-1 hour') WHERE task_id=?1",
                [task_id],
            )
            .unwrap();
    }

    /// task의 updated_at을 `minutes_ago`분 전으로 강제한다(대시보드 busy 신선도 게이트 등 시간경과
    /// 시나리오 테스트용). test_force_lease_expired와 같은 raw SQL 우회 패턴, 대상 컬럼만 다르다.
    pub(crate) fn test_force_task_stale(&self, task_id: &str, minutes_ago: i64) {
        self.conn
            .execute(
                "UPDATE tasks SET updated_at=datetime('now', ?1) WHERE task_id=?2",
                rusqlite::params![format!("-{minutes_ago} minutes"), task_id],
            )
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod a2a_tests {
        use super::*;
        use crate::store::a2a::{Artifact, Message, Part, Task, TaskState};

        fn sample_message(id: &str) -> Message {
            Message {
                message_id: id.into(),
                role: "user".into(),
                parts: vec![Part {
                    text: Some("내용".into()),
                    ..Default::default()
                }],
                task_id: Some("t1".into()),
                context_id: None,
            }
        }

        #[test]
        fn now_returns_nonempty_datetime_string() {
            let db = SqliteStore::open_memory().unwrap();
            let ts = db.now().unwrap();
            // "YYYY-MM-DD HH:MM:SS" 형식(datetime('now') 기본 포맷). 정확한 파싱보다 형태만 가드.
            assert_eq!(ts.len(), 19, "datetime('now') 포맷 불일치: {ts}");
            assert!(
                ts.contains('-') && ts.contains(':'),
                "datetime('now') 포맷 불일치: {ts}"
            );
        }

        #[test]
        fn new_task_id_is_unique_and_hex() {
            let db = SqliteStore::open_memory().unwrap();
            let a = db.new_task_id().unwrap();
            let b = db.new_task_id().unwrap();
            assert_ne!(a, b, "연속 생성 id가 겹치면 안 됨");
            assert_eq!(a.len(), 32, "randomblob(16) hex는 32자여야 함: {a}");
            assert!(a.chars().all(|c| c.is_ascii_hexdigit()), "hex가 아님: {a}");
            assert_eq!(a, a.to_lowercase(), "lower(hex(...))인데 대문자 포함: {a}");
        }

        #[test]
        fn create_get_roundtrip_preserves_all_fields() {
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let mut task = Task::new(
                "t1",
                Some("ctx1".into()),
                "win-claude",
                "mac-claude",
                "2026-07-02 10:00:00",
            );
            task.status_message = Some(msg.clone());
            task.history = vec![msg.clone()];
            db.create_task(&task).unwrap();

            let back = db.get_task("t1").unwrap().expect("존재해야 함");
            assert_eq!(back.id, "t1");
            assert_eq!(back.context_id.as_deref(), Some("ctx1"));
            assert_eq!(back.from_agent, "win-claude");
            assert_eq!(back.to_agent, "mac-claude");
            assert_eq!(back.state, TaskState::Submitted);
            assert_eq!(back.status_message, Some(msg.clone()));
            assert_eq!(back.history, vec![msg]);
            assert!(back.artifacts.is_empty());
            assert_eq!(back.created_at, "2026-07-02 10:00:00");
            assert_eq!(back.updated_at, "2026-07-02 10:00:00");
        }

        #[test]
        fn get_task_missing_is_none() {
            let db = SqliteStore::open_memory().unwrap();
            assert!(db.get_task("nope").unwrap().is_none());
        }

        #[test]
        fn create_task_from_message_creates_submitted_task_and_persists_message() {
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let task = db
                .create_task_from_message("win-claude", "mac-claude", msg.clone())
                .unwrap();

            assert_eq!(task.state, TaskState::Submitted);
            assert_eq!(
                task.id.len(),
                32,
                "task_id는 randomblob(16) hex 32자여야 함: {}",
                task.id
            );
            assert_eq!(task.from_agent, "win-claude");
            assert_eq!(task.to_agent, "mac-claude");
            assert_eq!(task.status_message, Some(msg.clone()));
            assert_eq!(task.history, vec![msg]);

            // store에도 실제로 영속되었는지 확인(round-trip).
            let persisted = db.get_task(&task.id).unwrap().expect("영속되어야 함");
            assert_eq!(persisted, task);
        }

        #[test]
        fn create_task_from_message_preserves_context_id_from_message() {
            let db = SqliteStore::open_memory().unwrap();
            let mut msg = sample_message("m1");
            msg.context_id = Some("ctx1".into());
            let task = db.create_task_from_message("a", "b", msg).unwrap();
            assert_eq!(task.context_id.as_deref(), Some("ctx1"));
        }

        #[test]
        fn create_task_from_message_two_calls_produce_distinct_task_ids() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = db
                .create_task_from_message("a", "b", sample_message("m1"))
                .unwrap();
            let t2 = db
                .create_task_from_message("a", "b", sample_message("m2"))
                .unwrap();
            assert_ne!(t1.id, t2.id);
        }

        #[test]
        fn list_open_tasks_for_filters_agent_and_completed() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00"); // open, mac
            let mut t2 = Task::new("t2", None, "win", "mac", "2026-07-02 09:05:00"); // completed, mac
            t2.state = TaskState::Completed;
            let t3 = Task::new("t3", None, "win", "other", "2026-07-02 09:10:00"); // open, other
            db.create_task(&t1).unwrap();
            db.create_task(&t2).unwrap();
            db.create_task(&t3).unwrap();

            let open = db.list_open_tasks_for("mac").unwrap();
            assert_eq!(open.len(), 1, "completed 제외 + 다른 to_agent 제외");
            assert_eq!(open[0].id, "t1");
        }

        #[test]
        fn list_open_tasks_for_orders_by_created_at() {
            let db = SqliteStore::open_memory().unwrap();
            let t_later = Task::new("later", None, "win", "mac", "2026-07-02 09:10:00");
            let t_earlier = Task::new("earlier", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&t_later).unwrap();
            db.create_task(&t_earlier).unwrap();
            let open = db.list_open_tasks_for("mac").unwrap();
            assert_eq!(
                open.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["earlier", "later"]
            );
        }

        #[test]
        fn list_all_open_tasks_returns_every_agent_and_excludes_completed() {
            let db = SqliteStore::open_memory().unwrap();
            let t1 = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00"); // open, mac
            let t2 = Task::new("t2", None, "win", "other", "2026-07-02 09:05:00"); // open, other
            let mut t3 = Task::new("t3", None, "win", "mac", "2026-07-02 09:10:00"); // completed, mac
            t3.state = TaskState::Completed;
            db.create_task(&t1).unwrap();
            db.create_task(&t2).unwrap();
            db.create_task(&t3).unwrap();

            let all_open = db.list_all_open_tasks().unwrap();
            assert_eq!(
                all_open.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["t1", "t2"],
                "to_agent 필터 없이 열린 task 전부(agent 무관) + completed 제외"
            );
        }

        #[test]
        fn count_by_state_groups_all_states() {
            let db = SqliteStore::open_memory().unwrap();
            // submitted 2, working 1, completed 3, failed 1(입력 상태는 0 → 키 부재로 확인).
            let seed = [
                ("a", TaskState::Submitted),
                ("b", TaskState::Submitted),
                ("c", TaskState::Working),
                ("d", TaskState::Completed),
                ("e", TaskState::Completed),
                ("f", TaskState::Completed),
                ("g", TaskState::Failed),
            ];
            for (id, state) in seed {
                let mut t = Task::new(id, None, "win", "mac", "2026-07-12 10:00:00");
                t.state = state;
                db.create_task(&t).unwrap();
            }

            let counts = db.count_by_state().unwrap();
            assert_eq!(counts.get("submitted").copied(), Some(2));
            assert_eq!(counts.get("working").copied(), Some(1));
            assert_eq!(counts.get("completed").copied(), Some(3));
            assert_eq!(counts.get("failed").copied(), Some(1));
            // 존재하지 않는 상태는 키가 없어야 한다(호출부가 unwrap_or(0)로 흡수).
            assert_eq!(counts.get("input_required").copied(), None);
            assert_eq!(counts.get("canceled").copied(), None);
        }

        #[test]
        fn count_by_state_empty_table_is_empty_map() {
            let db = SqliteStore::open_memory().unwrap();
            assert!(db.count_by_state().unwrap().is_empty(), "빈 테이블은 빈 맵");
        }

        // --- v2-45 P2: list_tasks_replay(재생 공용 질의) 단위테스트 ---

        /// updated_at이 서로 다른 재생용 task 4건을 심는다(상태·발신자 혼합).
        /// t1=completed(win) < t2=failed(win) < t3=canceled(other) < t4=submitted(win).
        fn seed_replay_tasks(db: &SqliteStore) {
            let mut t1 = Task::new("t1", None, "win", "mac", "2026-07-11 09:00:00");
            t1.state = TaskState::Completed;
            let mut t2 = Task::new("t2", None, "win", "mac", "2026-07-11 09:01:00");
            t2.state = TaskState::Failed;
            let mut t3 = Task::new("t3", None, "other", "mac", "2026-07-11 09:02:00");
            t3.state = TaskState::Canceled;
            let t4 = Task::new("t4", None, "win", "mac", "2026-07-11 09:03:00");
            for t in [&t1, &t2, &t3, &t4] {
                db.create_task(t).unwrap();
            }
        }

        fn ids(tasks: &[Task]) -> Vec<&str> {
            tasks.iter().map(|t| t.id.as_str()).collect()
        }

        #[test]
        fn list_tasks_replay_no_filters_returns_all_states_ascending() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            let all = db
                .list_tasks_replay(None, None, &[], ReplayLimit::All)
                .unwrap();
            assert_eq!(
                ids(&all),
                vec!["t1", "t2", "t3", "t4"],
                "전 상태 + updated_at 오름차순"
            );
        }

        #[test]
        fn list_tasks_replay_limit_takes_most_recent_and_returns_ascending() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            let recent = db
                .list_tasks_replay(None, None, &[], ReplayLimit::Newest(2))
                .unwrap();
            assert_eq!(
                ids(&recent),
                vec!["t3", "t4"],
                "최근 2건을 오름차순으로 반환(DESC LIMIT 후 뒤집기)"
            );
        }

        #[test]
        fn list_tasks_replay_oldest_limit_takes_from_front() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            // catch-up 연쇄(v2-45 P3): 잘려도 오래된 것부터 이어받아야 워터마크가 갭 없이 전진한다.
            let oldest = db
                .list_tasks_replay(None, None, &[], ReplayLimit::Oldest(2))
                .unwrap();
            assert_eq!(
                ids(&oldest),
                vec!["t1", "t2"],
                "오래된 것부터 2건(ASC LIMIT)"
            );
        }

        #[test]
        fn list_tasks_replay_oldest_wedges_within_same_second() {
            // 동일-초 wedge 문서화(v2-45 P3 리뷰, 비도달 nit이라 회귀 가드로만 고정): 한 초에 상한
            // 초과 종결이 몰리면 Oldest(N)은 매번 rowid 앞 N건만 주고, 클라이언트 워터마크가 그 초를
            // 못 넘어(초 단위) N+1번째 이후는 재생에서 누락된다. since>= 재조회도 같은 prefix 반복.
            let db = SqliteStore::open_memory().unwrap();
            let ts = "2026-07-11 09:00:00";
            for i in 0..5 {
                let mut t = Task::new(format!("s{i}"), None, "win", "mac", ts);
                t.state = TaskState::Completed;
                db.create_task(&t).unwrap();
            }
            let first = db
                .list_tasks_replay(
                    Some("win"),
                    Some(ts),
                    &["completed"],
                    ReplayLimit::Oldest(3),
                )
                .unwrap();
            assert_eq!(
                ids(&first),
                vec!["s0", "s1", "s2"],
                "동일 초에선 rowid 앞 N건만(상한=3)"
            );
            // 워터마크가 그 초(=ts)로만 전진 가능 → since>= 재조회도 같은 prefix = s3·s4 도달 불가.
            let again = db
                .list_tasks_replay(
                    Some("win"),
                    Some(ts),
                    &["completed"],
                    ReplayLimit::Oldest(3),
                )
                .unwrap();
            assert_eq!(
                ids(&again),
                vec!["s0", "s1", "s2"],
                "since>= 재조회도 전진 불가(same prefix)"
            );
        }

        #[test]
        fn list_tasks_replay_since_is_inclusive_gte() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            let from = db
                .list_tasks_replay(None, Some("2026-07-11 09:01:00"), &[], ReplayLimit::All)
                .unwrap();
            assert_eq!(
                ids(&from),
                vec!["t2", "t3", "t4"],
                "since는 >= (경계 포함, seen dedup은 소비자 몫)"
            );
        }

        #[test]
        fn list_tasks_replay_filters_states_and_from_agent() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            // watch-results 의미론: completed/failed만 + dispatcher(from_agent) 필터.
            let terminal = db
                .list_tasks_replay(
                    Some("win"),
                    None,
                    &["completed", "failed"],
                    ReplayLimit::All,
                )
                .unwrap();
            assert_eq!(
                ids(&terminal),
                vec!["t1", "t2"],
                "canceled(t3)·submitted(t4)·타 발신자 제외"
            );
        }

        #[test]
        fn list_tasks_replay_combined_since_states_dispatcher_limit() {
            let db = SqliteStore::open_memory().unwrap();
            seed_replay_tasks(&db);
            let hit = db
                .list_tasks_replay(
                    Some("win"),
                    Some("2026-07-11 09:01:00"),
                    &["completed", "failed"],
                    ReplayLimit::Newest(10),
                )
                .unwrap();
            assert_eq!(ids(&hit), vec!["t2"], "필터 4종 동시 적용");
        }

        #[test]
        fn list_tasks_replay_empty_db_is_empty() {
            let db = SqliteStore::open_memory().unwrap();
            assert!(
                db.list_tasks_replay(None, None, &[], ReplayLimit::Newest(50))
                    .unwrap()
                    .is_empty()
            );
        }

        #[test]
        fn unindexed_terminal_tasks_lists_completed_failed_and_mark_excludes() {
            let db = SqliteStore::open_memory().unwrap();
            for (id, st, ts) in [
                ("t1", TaskState::Completed, "2026-07-11 09:00:00"),
                ("t2", TaskState::Failed, "2026-07-11 09:01:00"),
                ("t3", TaskState::Canceled, "2026-07-11 09:02:00"),
                ("t4", TaskState::Submitted, "2026-07-11 09:03:00"),
            ] {
                let mut t = Task::new(id, None, "win", "mac", ts);
                t.state = st;
                db.create_task(&t).unwrap();
            }
            // 미색인 종결 = completed·failed만(canceled·submitted 제외), updated_at 오름차순.
            let un = db.list_unindexed_terminal_tasks().unwrap();
            assert_eq!(
                un.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["t1", "t2"]
            );
            // 색인 스탬프 후엔 목록에서 빠진다.
            db.mark_task_indexed("t1").unwrap();
            let un2 = db.list_unindexed_terminal_tasks().unwrap();
            assert_eq!(
                un2.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
                vec!["t2"]
            );
            // 존재하지 않는 task_id 스탬프도 Ok(0행, best-effort).
            assert!(db.mark_task_indexed("nope").is_ok());
        }

        #[test]
        fn prune_slims_old_indexed_terminals_preserving_artifacts_and_fail_reason() {
            let db = SqliteStore::open_memory().unwrap();
            let msg = |t: &str| Message {
                message_id: "m".into(),
                role: "user".into(),
                parts: vec![Part {
                    text: Some(t.into()),
                    ..Default::default()
                }],
                task_id: None,
                context_id: None,
            };
            let old = "2020-01-01 00:00:00";
            // t1: 오래된 completed·색인됨(history+요청 message+artifact).
            let mut t1 = Task::new("t1", None, "win", "mac", old);
            t1.state = TaskState::Completed;
            t1.updated_at = old.into();
            t1.history = vec![msg("요청1")];
            t1.status_message = Some(msg("요청1"));
            t1.artifacts = vec![Artifact {
                artifact_id: "a".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과1".into()),
                    ..Default::default()
                }],
            }];
            db.create_task(&t1).unwrap();
            db.mark_task_indexed("t1").unwrap();
            // t2: 오래된 failed·색인됨(message_json=실패 사유).
            let mut t2 = Task::new("t2", None, "win", "mac", old);
            t2.state = TaskState::Failed;
            t2.updated_at = old.into();
            t2.history = vec![msg("요청2")];
            t2.status_message = Some(msg("BLOCKED: 사유"));
            db.create_task(&t2).unwrap();
            db.mark_task_indexed("t2").unwrap();
            // t3: 최근 completed·색인됨(보존기간 내 → 불변).
            let mut t3 = Task::new("t3", None, "win", "mac", old);
            t3.state = TaskState::Completed;
            t3.updated_at = db.now().unwrap();
            t3.history = vec![msg("요청3")];
            db.create_task(&t3).unwrap();
            db.mark_task_indexed("t3").unwrap();
            // t4: 오래된 completed·미색인(indexed_at NULL → 불변).
            let mut t4 = Task::new("t4", None, "win", "mac", old);
            t4.state = TaskState::Completed;
            t4.updated_at = old.into();
            t4.history = vec![msg("요청4")];
            db.create_task(&t4).unwrap();

            assert_eq!(
                db.prune_terminal_tasks(30).unwrap(),
                2,
                "오래되고 색인된 종결(t1·t2)만 슬림화"
            );
            let g1 = db.get_task("t1").unwrap().unwrap();
            assert!(g1.history.is_empty(), "completed history 비움");
            assert!(
                g1.status_message.is_none(),
                "completed 요청(message_json) 비움"
            );
            assert_eq!(
                g1.artifacts[0].parts[0].text.as_deref(),
                Some("결과1"),
                "artifacts 보존(§5-5)"
            );
            let g2 = db.get_task("t2").unwrap().unwrap();
            assert!(g2.history.is_empty(), "failed history 비움");
            assert_eq!(
                g2.status_message.as_ref().unwrap().parts[0].text.as_deref(),
                Some("BLOCKED: 사유"),
                "failed 실패 사유 보존(§5-5)"
            );
            assert!(
                !db.get_task("t3").unwrap().unwrap().history.is_empty(),
                "보존기간 내 task 불변"
            );
            assert!(
                !db.get_task("t4").unwrap().unwrap().history.is_empty(),
                "미색인 task 불변"
            );
            // 재실행은 0건(이미 슬림, 멱등).
            assert_eq!(
                db.prune_terminal_tasks(30).unwrap(),
                0,
                "재실행은 0건(멱등)"
            );
            // WAL 체크포인트 호출도 성공(인메모리는 no-op이나 에러 없음).
            assert!(db.wal_checkpoint().is_ok());
        }

        #[test]
        fn state_transition_submitted_to_working_to_completed_sets_artifacts() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Submitted
            );

            let working_msg = sample_message("wm1");
            db.update_task_state("t1", TaskState::Working, Some(&working_msg))
                .unwrap();
            let mid = db.get_task("t1").unwrap().unwrap();
            assert_eq!(mid.state, TaskState::Working);
            assert_eq!(mid.status_message, Some(working_msg));

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part {
                    text: Some("완료 보고".into()),
                    ..Default::default()
                }],
            }];
            db.complete_task("t1", &artifacts).unwrap();
            let done = db.get_task("t1").unwrap().unwrap();
            assert_eq!(done.state, TaskState::Completed);
            assert_eq!(done.artifacts, artifacts);
        }

        #[test]
        fn append_history_grows_in_order() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            let m1 = sample_message("h1");
            let m2 = sample_message("h2");
            db.append_history("t1", &m1).unwrap();
            db.append_history("t1", &m2).unwrap();

            let back = db.get_task("t1").unwrap().unwrap();
            assert_eq!(back.history, vec![m1, m2]);
        }

        #[test]
        fn append_history_on_missing_task_is_err() {
            let db = SqliteStore::open_memory().unwrap();
            let m1 = sample_message("h1");
            assert!(db.append_history("nope", &m1).is_err());
        }

        #[test]
        fn task_events_emit_status_then_status_then_completed_in_order() {
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db
                .task_event_sender()
                .expect("with_task_events 후엔 버스 활성화")
                .subscribe();

            let msg = sample_message("m1");
            let task = db
                .create_task_from_message("win-claude", "mac-claude", msg)
                .unwrap();

            let working_msg = sample_message("wm1");
            db.update_task_state(&task.id, TaskState::Working, Some(&working_msg))
                .unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part {
                    text: Some("완료 보고".into()),
                    ..Default::default()
                }],
            }];
            db.complete_task(&task.id, &artifacts).unwrap();

            // 1) create_task_from_message -> Status(submitted).
            match rx.try_recv().expect("첫 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Submitted),
                other => panic!("Status(submitted) 기대, 실제: {other:?}"),
            }
            // 2) update_task_state(Working) -> Status(working).
            match rx.try_recv().expect("둘째 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Working),
                other => panic!("Status(working) 기대, 실제: {other:?}"),
            }
            // 3) complete_task -> Completed(completed, artifacts 포함).
            match rx.try_recv().expect("셋째 이벤트 없음") {
                TaskEvent::Completed(t) => {
                    assert_eq!(t.state, TaskState::Completed);
                    assert_eq!(t.artifacts, artifacts);
                }
                other => panic!("Completed 기대, 실제: {other:?}"),
            }
            assert!(rx.try_recv().is_err(), "이벤트가 3건보다 많음");
        }

        #[test]
        fn task_events_no_bus_is_noop() {
            // with_task_events()를 호출하지 않으면 emit이 조용히 무시된다(기존 unary 경로 무영향).
            let db = SqliteStore::open_memory().unwrap();
            assert!(db.task_event_sender().is_none());
            let msg = sample_message("m1");
            let task = db
                .create_task_from_message("win-claude", "mac-claude", msg)
                .unwrap();
            assert_eq!(task.state, TaskState::Submitted);
        }

        // --- R2: 조건부 전이(try_claim/try_complete/try_fail/try_cancel) 단위테스트 ---

        #[test]
        fn try_claim_twice_second_call_is_transition_conflict() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            // 첫 claim: submitted -> working 성공.
            db.try_claim("t1", None, None).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working
            );

            // 둘째 claim(동시 착수 경쟁 시뮬레이션): 이미 working이라 전이 대상 아님 -> Err.
            let err = db.try_claim("t1", None, None).unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            // 실패한 전이가 상태를 건드리지 않았는지 확인(여전히 working, 다른 상태로 안 튐).
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working
            );
        }

        #[test]
        fn try_complete_on_non_working_task_is_transition_conflict() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap(); // submitted 상태(아직 claim 안 됨).

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과".into()),
                    ..Default::default()
                }],
            }];
            let err = db.try_complete("t1", &artifacts, None).unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            // submitted로 남아있어야 함(완료로 잘못 전이되지 않음).
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Submitted
            );
        }

        #[test]
        fn try_cancel_on_completed_task_is_blocked_and_state_preserved() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", None, None).unwrap();
            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과".into()),
                    ..Default::default()
                }],
            }];
            db.try_complete("t1", &artifacts, None).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Completed
            );

            // 이미 completed(종료 상태)인 task를 canceled로 덮어쓰려 하면 차단돼야 한다(R2 핵심 회귀).
            let err = db.try_cancel("t1").unwrap_err();
            assert!(err.contains("t1"), "에러 메시지에 task_id 없음: {err}");
            let after = db.get_task("t1").unwrap().unwrap();
            assert_eq!(
                after.state,
                TaskState::Completed,
                "completed가 canceled로 덮어써짐(R2 회귀)"
            );
            assert_eq!(after.artifacts, artifacts, "완료 산출물이 유지돼야 함");
        }

        #[test]
        fn try_claim_then_try_complete_emit_status_then_completed() {
            // 기존 update_task_state/complete_task 경로를 검증하던
            // task_events_emit_status_then_status_then_completed_in_order와 동일한 이벤트버스 계약을
            // try_* 조건부 전이 경로에서도 유지하는지 확인한다(R2: emit 보존이 핵심 요구사항).
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db
                .task_event_sender()
                .expect("with_task_events 후엔 버스 활성화")
                .subscribe();

            let msg = sample_message("m1");
            let task = db
                .create_task_from_message("win-claude", "mac-claude", msg)
                .unwrap();

            db.try_claim(&task.id, None, None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: Some("결과물".into()),
                parts: vec![Part {
                    text: Some("완료 보고".into()),
                    ..Default::default()
                }],
            }];
            db.try_complete(&task.id, &artifacts, None).unwrap();

            // 1) create_task_from_message -> Status(submitted).
            match rx.try_recv().expect("첫 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Submitted),
                other => panic!("Status(submitted) 기대, 실제: {other:?}"),
            }
            // 2) try_claim -> Status(working).
            match rx.try_recv().expect("둘째 이벤트 없음") {
                TaskEvent::Status(t) => assert_eq!(t.state, TaskState::Working),
                other => panic!("Status(working) 기대, 실제: {other:?}"),
            }
            // 3) try_complete -> Completed(completed, artifacts 포함).
            match rx.try_recv().expect("셋째 이벤트 없음") {
                TaskEvent::Completed(t) => {
                    assert_eq!(t.state, TaskState::Completed);
                    assert_eq!(t.artifacts, artifacts);
                }
                other => panic!("Completed 기대, 실제: {other:?}"),
            }
            assert!(rx.try_recv().is_err(), "이벤트가 3건보다 많음");
        }

        #[test]
        fn try_transition_on_missing_task_is_err_and_emits_nothing() {
            // 대상 task 자체가 없으면 rows_affected=0으로 같은 에러 경로를 타야 한다(스펙 요구사항).
            // 전이가 없었으니 이벤트도 없어야 한다.
            let db = SqliteStore::open_memory().unwrap().with_task_events();
            let mut rx = db
                .task_event_sender()
                .expect("with_task_events 후엔 버스 활성화")
                .subscribe();
            assert!(db.try_claim("nope", None, None).is_err());
            assert!(db.try_fail("nope", None, None).is_err());
            assert!(db.try_cancel("nope").is_err());
            assert!(
                rx.try_recv().is_err(),
                "존재하지 않는 task에 대해 이벤트가 emit됨"
            );
        }

        // --- lease 기반 claim-후-워커사망 자동 requeue 단위테스트 ---

        /// tasks의 DB 내부 컬럼(claimed_at/lease_expires_at/claimed_by/attempt_count)을 직접 조회한다.
        /// Task wire 구조체에는 노출되지 않는 컬럼이라 raw SQL로만 검증 가능.
        fn raw_claim_fields(
            db: &SqliteStore,
            task_id: &str,
        ) -> (Option<String>, Option<String>, Option<String>, i64) {
            db.conn
                .query_row(
                    "SELECT claimed_at, lease_expires_at, claimed_by, attempt_count \
                     FROM tasks WHERE task_id=?1",
                    [task_id],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
                )
                .unwrap()
        }

        #[test]
        fn try_claim_sets_lease_claimed_by_and_increments_attempt_count() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let (claimed_at, lease_expires_at, claimed_by, attempt_count) =
                raw_claim_fields(&db, "t1");
            assert!(claimed_at.is_some(), "claimed_at이 세팅되어야 함");
            assert!(
                lease_expires_at.is_some(),
                "lease_expires_at이 세팅되어야 함"
            );
            assert_eq!(claimed_by.as_deref(), Some("worker-a"));
            assert_eq!(attempt_count, 1, "최초 claim은 attempt_count=1");
        }

        #[test]
        fn try_claim_records_runner_and_get_task_exposes_it() {
            // v8: claim 시 runner를 기록하고, get_task로 조회한 Task에도 그대로 노출되어야 한다.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), Some("codex")).unwrap();

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(
                reloaded.runner.as_deref(),
                Some("codex"),
                "claim한 runner가 노출되어야 함"
            );
        }

        #[test]
        fn try_claim_without_runner_leaves_runner_null_backward_compat() {
            // 하위호환: runner 인자 없이 claim해도(레거시 워커·raw curl 등) 정상 동작, runner만 NULL.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.runner, None, "runner 없이 claim하면 NULL이어야 함");
        }

        #[test]
        fn extend_lease_refreshes_lease_and_prevents_requeue() {
            // v2-49 #6: 살아 있는 워커가 lease를 연장하면 만료로 인한 requeue가 일어나지 않아야 한다.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();
            // lease를 강제 만료(워커 사망 시나리오와 동일한 상태로) 시킨 뒤,
            db.test_force_lease_expired("t1");
            // 워커가 살아 있어 연장하면 lease_expires_at이 미래로 밀린다.
            db.extend_lease("t1", "worker-a").unwrap();
            let requeued = db.expire_stale_claims().unwrap();
            assert_eq!(requeued, 0, "lease 연장 후에는 requeue되지 않아야 함");
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working,
                "연장 후에도 여전히 working"
            );
        }

        #[test]
        fn extend_lease_rejects_non_working_and_wrong_claimer() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            // claim 전(submitted)에는 working이 아니라 연장 불가.
            assert!(
                db.extend_lease("t1", "worker-a").is_err(),
                "claim 전에는 연장 불가"
            );
            db.try_claim("t1", Some("worker-a"), None).unwrap();
            // 다른 워커는 claimed_by 불일치라 연장 불가(소유권 없는 연장 차단).
            assert!(
                db.extend_lease("t1", "worker-b").is_err(),
                "claimed_by 불일치 연장 불가"
            );
            // 소유 워커는 성공.
            assert!(
                db.extend_lease("t1", "worker-a").is_ok(),
                "소유 워커는 연장 성공"
            );
        }

        #[test]
        fn try_claim_without_agent_leaves_claimed_by_null_backward_compat() {
            // 하위호환: agent 인자 없이 claim해도(raw curl 등) 정상 동작, claimed_by만 NULL.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();

            db.try_claim("t1", None, None).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working
            );

            let (_, _, claimed_by, attempt_count) = raw_claim_fields(&db, "t1");
            assert_eq!(claimed_by, None, "agent 없이 claim하면 claimed_by는 NULL");
            assert_eq!(attempt_count, 1);
        }

        #[test]
        fn expire_stale_claims_requeues_expired_lease_under_max_attempts() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), Some("codex")).unwrap(); // attempt_count=1, runner 기록.

            // lease를 과거로 강제 심어 만료를 시뮬레이션한다(raw SQL, wire에 없는 내부 컬럼).
            db.conn
                .execute(
                    "UPDATE tasks SET lease_expires_at=datetime('now','-1 hour') WHERE task_id='t1'",
                    [],
                )
                .unwrap();

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1, "만료된 claim 1건이 회수되어야 함");

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(
                reloaded.state,
                TaskState::Submitted,
                "만료된 working은 submitted로 복귀"
            );
            assert!(
                reloaded.runner.is_none(),
                "runner는 회수(submitted 복귀) 시 클리어되어야 함(claimed_by와 동형)"
            );

            let (claimed_at, lease_expires_at, claimed_by, attempt_count) =
                raw_claim_fields(&db, "t1");
            assert!(claimed_at.is_none(), "claimed_at은 클리어되어야 함");
            assert!(
                lease_expires_at.is_none(),
                "lease_expires_at은 클리어되어야 함"
            );
            assert!(claimed_by.is_none(), "claimed_by는 클리어되어야 함");
            assert_eq!(
                attempt_count, 1,
                "attempt_count는 유지(다음 claim에서 다시 증가)"
            );
        }

        #[test]
        fn expire_stale_claims_preserves_task_instruction_for_redelivery() {
            // requeue된 task는 새 워커가 poll에서 지시문(status_message)을 다시 읽어 실행하므로,
            // claim·requeue 모두 status_message를 지우면 안 된다(재배달 시 빈 프롬프트 방지).
            let db = SqliteStore::open_memory().unwrap();
            let msg = sample_message("m1");
            let task = db
                .create_task_from_message("win", "mac", msg.clone())
                .unwrap();
            db.try_claim(&task.id, Some("worker-a"), None).unwrap();
            db.test_force_lease_expired(&task.id);

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1);

            let reloaded = db.get_task(&task.id).unwrap().unwrap();
            assert_eq!(
                reloaded.state,
                TaskState::Submitted,
                "만료 claim은 submitted로 복귀"
            );
            assert_eq!(
                reloaded.status_message,
                Some(msg),
                "requeue 후 지시문(status_message)이 보존되어야 재배달 워커가 프롬프트를 얻는다"
            );
        }

        #[test]
        fn expire_stale_claims_fails_task_when_attempt_count_at_max() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            // attempt_count를 상한(MAX_CLAIM_ATTEMPTS=3)까지 도달한 상태로 강제 세팅한다(raw SQL).
            db.conn
                .execute(
                    "UPDATE tasks SET attempt_count=3, \
                     lease_expires_at=datetime('now','-1 hour') WHERE task_id='t1'",
                    [],
                )
                .unwrap();

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1, "상한 도달 claim 1건이 격리되어야 함");

            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(
                reloaded.state,
                TaskState::Failed,
                "상한 초과는 submitted가 아니라 failed로 격리"
            );
        }

        #[test]
        fn expire_stale_claims_leaves_unexpired_working_untouched() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap(); // lease는 기본 30분 후(미래).

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 0, "lease가 아직 안 지났으면 아무것도 회수되지 않아야 함");
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working
            );
        }

        #[test]
        fn expire_stale_claims_ignores_non_working_tasks() {
            // submitted/completed 등 working이 아닌 task는 sweep 대상이 아니다(설사 lease 컬럼이 남아있어도).
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap(); // submitted, lease 없음.

            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 0);
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Submitted
            );
        }

        #[test]
        fn try_complete_first_completer_wins_rejects_mismatched_completer() {
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과".into()),
                    ..Default::default()
                }],
            }];
            // stale(되살아난) worker-b가 completer 불일치로 거부되어야 한다(레이스 방지 핵심).
            let err = db
                .try_complete("t1", &artifacts, Some("worker-b"))
                .unwrap_err();
            assert!(err.contains("t1"));
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working,
                "거부 후 상태 불변"
            );

            // claim한 본인(worker-a)이 completer면 성공.
            db.try_complete("t1", &artifacts, Some("worker-a")).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Completed
            );
        }

        #[test]
        fn try_fail_first_completer_wins_rejects_mismatched_failer() {
            // try_complete와 대칭: 되살아난 stale worker-b가 worker-a claim task를 failed로 덮어쓰지 못한다
            // (gemini/coderabbit 리뷰). failer=None이면 하위호환으로 통과.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            // stale worker-b의 fail은 거부 -> 상태 불변(working).
            let err = db.try_fail("t1", None, Some("worker-b")).unwrap_err();
            assert!(err.contains("t1"));
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working,
                "거부 후 상태 불변"
            );

            // claim 본인(worker-a)이면 성공.
            db.try_fail("t1", None, Some("worker-a")).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Failed);

            // 하위호환: failer=None이면 가드 무력화(다른 task로 확인).
            let t2 = Task::new("t2", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&t2).unwrap();
            db.try_claim("t2", Some("worker-a"), None).unwrap();
            db.try_fail("t2", None, None).unwrap();
            assert_eq!(db.get_task("t2").unwrap().unwrap().state, TaskState::Failed);
        }

        #[test]
        fn try_fail_rejects_stale_worker_on_requeued_submitted_task() {
            // #4 회귀: expire_stale_claims의 requeue(state=submitted, claimed_by=NULL)된 뒤, 되살아난
            // stale 워커(failer=이전 claimed_by)의 try_fail이 예약된 재시도를 무단 종결시키면 안 된다.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();
            db.test_force_lease_expired("t1");
            let n = db.expire_stale_claims().unwrap();
            assert_eq!(n, 1, "만료된 claim이 회수(requeue)되어야 함");
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Submitted,
                "requeue 후 submitted"
            );

            // 되살아난 stale worker-a(직전 소유자였던 uuid)의 fail은 거부 -> 다음 워커의 재시도 기회 보존.
            let err = db.try_fail("t1", None, Some("worker-a")).unwrap_err();
            assert!(err.contains("t1"));
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Submitted,
                "거부 후 상태 불변(재시도 예약 보존)"
            );

            // 하위호환: failer=None(브로커/dispatcher 직접 경로)이면 submitted도 fail 가능(불변 유지).
            db.try_fail("t1", None, None).unwrap();
            assert_eq!(db.get_task("t1").unwrap().unwrap().state, TaskState::Failed);
        }

        #[test]
        fn try_fail_succeeds_on_working_task_with_matching_failer() {
            // 정상 경로(working 상태에서 claim한 워커 본인의 fail)는 새 가드로 영향받지 않는다.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t2", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t2", Some("worker-a"), None).unwrap();
            db.try_fail("t2", None, Some("worker-a")).unwrap();
            assert_eq!(db.get_task("t2").unwrap().unwrap().state, TaskState::Failed);
        }

        #[test]
        fn try_complete_completer_none_bypasses_guard_backward_compat() {
            // 하위호환: completer=None이면 claimed_by 불일치와 무관하게(가드 무력화) 기존 동작대로 성공.
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", Some("worker-a"), None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과".into()),
                    ..Default::default()
                }],
            }];
            db.try_complete("t1", &artifacts, None).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Completed
            );
        }

        #[test]
        fn try_complete_succeeds_when_claimed_by_is_null() {
            // agent 인자 없이 claim된(claimed_by NULL) task는 completer가 있어도 성공해야 한다
            // (claimed_by IS NULL 분기, 혼재 호출 시나리오 하위호환).
            let db = SqliteStore::open_memory().unwrap();
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            db.try_claim("t1", None, None).unwrap();

            let artifacts = vec![Artifact {
                artifact_id: "a1".into(),
                name: None,
                parts: vec![Part {
                    text: Some("결과".into()),
                    ..Default::default()
                }],
            }];
            db.try_complete("t1", &artifacts, Some("worker-a")).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Completed
            );
        }

        #[test]
        fn migration_v6_to_v7_adds_lease_columns_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v6v7.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v6 스키마 수동 구성: tasks에 lease 컬럼 없음 + schema_version=6 + 기존 task 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     CREATE TABLE tasks(task_id TEXT PRIMARY KEY, context_id TEXT, \
                         from_agent TEXT NOT NULL, to_agent TEXT NOT NULL, state TEXT NOT NULL, \
                         message_json TEXT, artifacts_json TEXT, history_json TEXT, \
                         created_at TEXT NOT NULL, updated_at TEXT NOT NULL);
                     INSERT INTO tasks(task_id, context_id, from_agent, to_agent, state, \
                         created_at, updated_at) \
                         VALUES('t1', NULL, 'win', 'mac', 'submitted', \
                         '2026-07-02 09:00:00', '2026-07-02 09:00:00');
                     INSERT INTO config(key,value) VALUES('schema_version','6');",
                )
                .unwrap();
            }
            // open → migrate v6→v7(lease 컬럼 4종 신설).
            let db = SqliteStore::open(p).unwrap();
            for col in [
                "claimed_at",
                "lease_expires_at",
                "claimed_by",
                "attempt_count",
            ] {
                assert!(
                    db.column_exists("tasks", col),
                    "마이그레이션이 {col} 컬럼을 추가해야 함"
                );
            }
            // 기존 task 보존 + attempt_count 기본값 0.
            let preserved = db.get_task("t1").unwrap().expect("기존 task 보존");
            assert_eq!(preserved.state, TaskState::Submitted);
            let (_, _, _, attempt_count) = raw_claim_fields(&db, "t1");
            assert_eq!(attempt_count, 0, "기존 행의 attempt_count는 기본값 0");
            // 마이그레이션된 스키마에서 claim이 바로 동작해야 한다(신규 컬럼이 실사용 가능한지 확인).
            db.try_claim("t1", Some("worker-a"), None).unwrap();
            assert_eq!(
                db.get_task("t1").unwrap().unwrap().state,
                TaskState::Working
            );
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn migration_v7_to_v8_adds_runner_column_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v7v8.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v7 스키마 수동 구성: tasks에 lease 컬럼은 있으나 runner 없음 + schema_version=7 + 기존 task 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     CREATE TABLE tasks(task_id TEXT PRIMARY KEY, context_id TEXT, \
                         from_agent TEXT NOT NULL, to_agent TEXT NOT NULL, state TEXT NOT NULL, \
                         message_json TEXT, artifacts_json TEXT, history_json TEXT, \
                         created_at TEXT NOT NULL, updated_at TEXT NOT NULL, \
                         claimed_at TEXT, lease_expires_at TEXT, claimed_by TEXT, \
                         attempt_count INTEGER NOT NULL DEFAULT 0);
                     INSERT INTO tasks(task_id, context_id, from_agent, to_agent, state, \
                         created_at, updated_at) \
                         VALUES('t1', NULL, 'win', 'mac', 'submitted', \
                         '2026-07-02 09:00:00', '2026-07-02 09:00:00');
                     INSERT INTO config(key,value) VALUES('schema_version','7');",
                )
                .unwrap();
            }
            // open → migrate v7→v8(runner 컬럼 신설).
            let db = SqliteStore::open(p).unwrap();
            assert!(
                db.column_exists("tasks", "runner"),
                "마이그레이션이 runner 컬럼을 추가해야 함"
            );
            // 기존 task 보존 + runner는 NULL(마이그레이션 이전엔 없던 컬럼).
            let preserved = db.get_task("t1").unwrap().expect("기존 task 보존");
            assert_eq!(preserved.state, TaskState::Submitted);
            assert_eq!(
                preserved.runner, None,
                "마이그레이션 이전 행의 runner는 NULL이어야 함"
            );
            // 마이그레이션된 스키마에서 runner를 포함한 claim이 바로 동작해야 한다.
            db.try_claim("t1", Some("worker-a"), Some("claude"))
                .unwrap();
            let reloaded = db.get_task("t1").unwrap().unwrap();
            assert_eq!(reloaded.state, TaskState::Working);
            assert_eq!(reloaded.runner.as_deref(), Some("claude"));
            let _ = std::fs::remove_file(&path);
        }

        #[test]
        fn migration_v5_to_v6_adds_tasks_table_and_preserves_data() {
            let dir = std::env::temp_dir();
            let path = dir.join("tuna_mig_v5v6.db");
            let _ = std::fs::remove_file(&path);
            let p = path.to_str().unwrap();
            // v5 스키마 수동 구성: tasks 테이블 없음 + schema_version=5 + 기존 메시지 1건.
            {
                let conn = rusqlite::Connection::open(p).unwrap();
                conn.execute_batch(
                    "CREATE TABLE config(key TEXT PRIMARY KEY, value TEXT);
                     CREATE TABLE sessions(id TEXT PRIMARY KEY, head_id INTEGER, \
                         created_at TEXT, updated_at TEXT);
                     CREATE TABLE messages(rowid INTEGER PRIMARY KEY AUTOINCREMENT, \
                         session_id TEXT NOT NULL, msg_id INTEGER NOT NULL, parent_id INTEGER, \
                         speaker TEXT NOT NULL, content TEXT NOT NULL, created_at TEXT, \
                         UNIQUE(session_id, msg_id));
                     INSERT INTO sessions(id, head_id) VALUES('s', 1);
                     INSERT INTO messages(session_id,msg_id,parent_id,speaker,content) \
                         VALUES('s',1,NULL,'a','hi');
                     INSERT INTO config(key,value) VALUES('schema_version','5');",
                )
                .unwrap();
            }
            // open → migrate v5→v6(tasks 테이블 신설).
            let db = SqliteStore::open(p).unwrap();
            let table_count: i64 = db
                .conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='tasks'",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(table_count, 1, "마이그레이션이 tasks 테이블 생성");
            // 기존 메시지 보존.
            let content: String = db
                .conn
                .query_row(
                    "SELECT content FROM messages WHERE session_id='s' AND msg_id=1",
                    [],
                    |r| r.get(0),
                )
                .unwrap();
            assert_eq!(content, "hi", "기존 메시지 보존");
            // tasks 테이블 실제 사용 가능 확인(신규 마이그레이션 스키마에 바로 INSERT 가능해야 함).
            let task = Task::new("t1", None, "win", "mac", "2026-07-02 09:00:00");
            db.create_task(&task).unwrap();
            assert!(db.get_task("t1").unwrap().is_some());
            let _ = std::fs::remove_file(&path);
        }
    }
}
