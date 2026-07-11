// 에이전트 로스터(인메모리 RefCell 풀). human_input_at(총감독 ★)만 agent_human_input 테이블로 영속한다.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use super::*;

/// human_input_at 영속 행 보존기간(일). deregister/stale를 안 탄 고아 행을 이 기간 뒤 GC한다.
const HUMAN_INPUT_RETAIN_DAYS: u32 = 7;

impl SqliteStore {
    // ---- 총감독 ★ 신호(human_input_at) 영속(v2-45 P4, agent_human_input 테이블) ----

    /// 영속 테이블에서 uuid의 human_input_at을 읽는다(재기동 후 인메모리 로스터가 빈 상태에서 ★ 복원
    /// 폴백). 행이 없으면 None. DB 에러도 None으로 흡수한다(★ 복원은 best-effort, 로스터를 막지 않음).
    fn load_human_input(&self, uuid: &str) -> Option<String> {
        self.conn
            .query_row("SELECT at FROM agent_human_input WHERE uuid = ?1", [uuid], |r| r.get::<_, String>(0))
            .optional()
            .unwrap_or(None)
    }

    /// human_input_at을 영속 테이블에 **단조** write-through(UPSERT, 더 새 값만). best-effort
    /// (영속 실패가 로스터/통지를 막지 않는다). 단조라 과거 값으로 되감기지 않아 merge 승자 재기록도 안전.
    fn persist_human_input(&self, uuid: &str, at: &str) {
        let _ = self.conn.execute(
            "INSERT INTO agent_human_input(uuid, at) VALUES(?1, ?2) \
             ON CONFLICT(uuid) DO UPDATE SET at = excluded.at WHERE excluded.at > agent_human_input.at",
            rusqlite::params![uuid, at],
        );
    }

    /// 영속 human_input_at 행을 제거(세션 소멸 GC = deregister·sync_presence stale). best-effort.
    fn delete_human_input(&self, uuid: &str) {
        let _ = self.conn.execute("DELETE FROM agent_human_input WHERE uuid = ?1", [uuid]);
    }

    /// 보존기간(기본 7일) 초과 human_input_at 행을 GC한다. deregister/stale를 안 타고 남은 고아 행
    /// (스캐너가 다시 보고하지 않는 uuid)을 정리한다. sync_presence가 매 스캔 주기에 호출 = 자연 주기 훅.
    fn gc_human_input(&self) {
        let _ = self.conn.execute(
            "DELETE FROM agent_human_input WHERE at < datetime('now', ?1)",
            [format!("-{HUMAN_INPUT_RETAIN_DAYS} days")],
        );
    }

    /// 에이전트를 로스터에 등록(있으면 교체). now는 last_heartbeat 초기값.
    /// 재등록(재기동) 시 human_input_at(총감독 ★)은 인메모리 → 영속 테이블 순으로 복원한다(v2-45 P4:
    /// 브로커 재기동 직후 인메모리가 비어도 register/스캐너 첫 보고 때 테이블에서 ★를 되살린다).
    pub fn register_agent(
        &self,
        uuid: &str,
        tags: BTreeMap<String, String>,
        display_name: Option<String>,
        now: &str,
    ) {
        let human_input_at = self
            .agent_roster
            .borrow()
            .get(uuid)
            .and_then(|e| e.human_input_at.clone())
            .or_else(|| self.load_human_input(uuid));
        self.agent_roster.borrow_mut().insert(
            uuid.to_string(),
            AgentEntry {
                uuid: uuid.to_string(),
                tags,
                display_name,
                last_heartbeat: now.to_string(),
                human_input_at,
            },
        );
    }

    /// presence 스캐너 일괄 동기화(설계 v2-44 §6): 보고된 세션은 upsert(human_input_at 보존),
    /// 같은 machine의 스캐너 소유(`src=scan`) 항목 중 보고에 없는 것은 제거한다(유령 원천 차단).
    /// 소유 태그로 격리하므로 수동 register(워커·infra·수신 poll) 항목은 건드리지 않는다.
    /// 반환=(upsert 수, 제거 수).
    pub fn sync_presence(
        &self,
        machine: &str,
        sessions: &[crate::store::agents::PresenceUpsert],
        now: &str,
    ) -> (usize, usize) {
        let mut roster = self.agent_roster.borrow_mut();
        let reported: std::collections::HashSet<&str> =
            sessions.iter().map(|s| s.uuid.as_str()).collect();
        let stale: Vec<String> = roster
            .values()
            .filter(|e| {
                e.tags.get("machine").map(String::as_str) == Some(machine)
                    && e.tags.get("src").map(String::as_str) == Some("scan")
                    && !reported.contains(e.uuid.as_str())
            })
            .map(|e| e.uuid.clone())
            .collect();
        let removed = stale.len();
        for uuid in &stale {
            roster.remove(uuid);
            // 세션 소멸의 대부분은 deregister를 안 타므로(조사 확정) stale 제거 시 영속 행도 GC한다.
            self.delete_human_input(uuid);
        }
        for s in sessions {
            // §5-8 최종형: human_input_at = max(인메모리, 스캐너 보고값, 영속 테이블).
            // base = 인메모리(있으면 직전 write-through로 테이블과 동기) 또는 재기동 복원(테이블 SELECT).
            // 보고값(codex 입력 신호)이 base보다 새로우면(merged != base) 승자를 테이블에 단조
            // write-through한다. 보고값 없음(claude)·불변이면 write 생략(P4의 N+1 회피 유지).
            let mem = roster.get(&s.uuid).and_then(|e| e.human_input_at.clone());
            let base = mem.or_else(|| self.load_human_input(&s.uuid));
            // Option<String>은 None < Some 순서(파생 Ord)라 std::cmp::max가 곧 max(base, 보고값)이다
            // (DB datetime 포맷은 사전순=시간순, gemini 리뷰). 커스텀 헬퍼 대신 stdlib.
            let human_input_at = std::cmp::max(base.clone(), s.human_input_at.clone());
            if human_input_at != base
                && let Some(at) = &human_input_at
            {
                self.persist_human_input(&s.uuid, at);
            }
            let mut tags = BTreeMap::new();
            tags.insert("machine".to_string(), machine.to_string());
            tags.insert("runner".to_string(), s.runner.clone());
            tags.insert("role".to_string(), "session".to_string());
            tags.insert("session".to_string(), s.uuid.clone());
            tags.insert("src".to_string(), "scan".to_string());
            if let Some(p) = &s.project {
                tags.insert("project".to_string(), p.clone());
            }
            roster.insert(
                s.uuid.clone(),
                AgentEntry {
                    uuid: s.uuid.clone(),
                    tags,
                    display_name: s.display_name.clone(),
                    last_heartbeat: now.to_string(),
                    human_input_at,
                },
            );
        }
        self.gc_human_input(); // 매 스캔 주기 = 7일 초과 고아 행 정리 훅(테이블이 작아 부담 없음)
        (sessions.len(), removed)
    }

    /// heartbeat: 존재하면 last_heartbeat 갱신 후 true, 미등록 uuid면 false(등록 선행 필요).
    pub fn heartbeat_agent(&self, uuid: &str, now: &str) -> bool {
        match self.agent_roster.borrow_mut().get_mut(uuid) {
            Some(entry) => {
                entry.last_heartbeat = now.to_string();
                true
            }
            None => false,
        }
    }

    /// 사람 프롬프트 핑: 해당 agent의 human_input_at을 now로 갱신(총감독=이 값 최신 세션, 설계 v2-42).
    /// **미등록이어도 영속 테이블에 선기록**한다(v2-45 P4: 무장 전 핑이 404로 유실되던 창 제거 +
    /// 재기동/스캐너 첫 보고 때 register/sync_presence가 테이블에서 ★를 복원한다). 로스터에 있으면
    /// 인메모리도 즉시 갱신한다. 항상 기록되므로 true를 반환한다(핸들러는 200으로 응답).
    pub fn mark_human_input(&self, uuid: &str, now: &str) -> bool {
        self.persist_human_input(uuid, now); // DB 선기록(등록 여부 무관, now는 항상 최신이라 단조 통과)
        if let Some(entry) = self.agent_roster.borrow_mut().get_mut(uuid) {
            entry.human_input_at = Some(now.to_string());
        }
        true
    }

    /// 로스터에서 에이전트를 즉시 제거(세션 종료 시 disarm이 호출, 설계 v2-43 잔존구간 제거).
    /// 존재했으면 true, 미등록이면 false. TTL(90초) 자연소멸을 기다리지 않고 닫힌 세션을 바로 없앤다.
    /// 세션 종료이므로 영속 ★ 행도 함께 GC한다(v2-45 P4).
    pub fn deregister_agent(&self, uuid: &str) -> bool {
        self.delete_human_input(uuid);
        self.agent_roster.borrow_mut().remove(uuid).is_some()
    }

    /// selector에 매칭되며 online인 에이전트를 uuid 오름차순으로 반환(clone).
    pub fn list_agents(
        &self,
        selector: &BTreeMap<String, String>,
        now: &str,
        ttl_secs: i64,
    ) -> Vec<AgentEntry> {
        let mut out: Vec<AgentEntry> = self
            .agent_roster
            .borrow()
            .values()
            .filter(|entry| {
                crate::store::agents::selector_matches(&entry.tags, selector)
                    && crate::store::agents::is_online(&entry.last_heartbeat, now, ttl_secs)
            })
            .cloned()
            .collect();
        out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
        out
    }

    /// selector 매칭 online 에이전트의 uuid만 정렬해 반환(라우팅 해석용).
    pub fn resolve_selector(
        &self,
        selector: &BTreeMap<String, String>,
        now: &str,
        ttl_secs: i64,
    ) -> Vec<String> {
        self.list_agents(selector, now, ttl_secs).into_iter().map(|entry| entry.uuid).collect()
    }

}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn register_then_list_agents_roundtrip() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", tags(&[("machine", "win")]), Some("win-claude".into()), "2026-07-04 10:00:00");
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:10", 90);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
        assert_eq!(found[0].display_name.as_deref(), Some("win-claude"));
    }

    #[test]
    fn heartbeat_agent_updates_existing_and_rejects_unknown() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 10:00:00");
        assert!(db.heartbeat_agent("u1", "2026-07-04 10:01:00"));
        assert!(!db.heartbeat_agent("unknown", "2026-07-04 10:01:00"));
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:01:05", 90);
        assert_eq!(found[0].last_heartbeat, "2026-07-04 10:01:00");
    }

    #[test]
    fn list_agents_excludes_offline() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 09:00:00");
        // now 기준 1시간 경과, ttl 90초 -> offline이라 제외되어야 함.
        let found = db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:00", 90);
        assert!(found.is_empty(), "offline 에이전트는 list_agents에서 제외되어야 함");
    }

    #[test]
    fn deregister_agent_removes_and_reports_presence() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 10:00:00");
        // 등록된 세션 제거 = true, 이후 online 목록에서 즉시 사라짐(TTL 대기 없이).
        assert!(db.deregister_agent("u1"));
        assert!(db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:05", 90).is_empty());
        // 미등록/이미 제거된 uuid는 false(멱등).
        assert!(!db.deregister_agent("u1"));
        assert!(!db.deregister_agent("unknown"));
    }

    #[test]
    fn resolve_selector_matches_none_one_or_many() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", tags(&[("machine", "win"), ("runner", "claude")]), None, "2026-07-04 10:00:00");
        db.register_agent("u2", tags(&[("machine", "mac"), ("runner", "claude")]), None, "2026-07-04 10:00:00");
        let now = "2026-07-04 10:00:10";

        let none = db.resolve_selector(&tags(&[("machine", "linux")]), now, 90);
        assert!(none.is_empty());

        let one = db.resolve_selector(&tags(&[("machine", "mac")]), now, 90);
        assert_eq!(one, vec!["u2".to_string()]);

        let many = db.resolve_selector(&tags(&[("runner", "claude")]), now, 90);
        assert_eq!(many, vec!["u1".to_string(), "u2".to_string()]);
    }

    fn presence(uuid: &str, runner: &str, project: Option<&str>) -> crate::store::agents::PresenceUpsert {
        crate::store::agents::PresenceUpsert {
            uuid: uuid.to_string(),
            runner: runner.to_string(),
            project: project.map(str::to_string),
            display_name: project.map(|p| format!("win-{runner}-{p}")),
            human_input_at: None,
        }
    }

    /// 스캐너 보고값(codex 입력 신호)이 있는 presence 항목(v2-45 P5).
    fn presence_with_input(uuid: &str, at: &str) -> crate::store::agents::PresenceUpsert {
        crate::store::agents::PresenceUpsert { human_input_at: Some(at.to_string()), ..presence(uuid, "codex", None) }
    }

    #[test]
    fn sync_presence_upserts_and_removes_only_scan_owned() {
        let db = SqliteStore::open_memory().unwrap();
        let t0 = "2026-07-11 10:00:00";
        // 수동 등록 항목(스캐너 소유 아님): infra watcher + 타 머신 세션.
        db.register_agent("win-codex-sup", tags(&[("machine", "win"), ("role", "infra")]), None, t0);
        db.register_agent("mac-sess", tags(&[("machine", "mac"), ("role", "session"), ("src", "scan")]), None, t0);
        // 1차 스캔 보고: s1, s2.
        let (up, rm) = db.sync_presence("win", &[presence("s1", "claude", Some("tunaRound")), presence("s2", "codex", None)], t0);
        assert_eq!((up, rm), (2, 0));
        let all = db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:05", 90);
        assert_eq!(all.len(), 4);
        let s1 = all.iter().find(|e| e.uuid == "s1").unwrap();
        assert_eq!(s1.tags.get("role").map(String::as_str), Some("session"));
        assert_eq!(s1.tags.get("src").map(String::as_str), Some("scan"));
        assert_eq!(s1.tags.get("project").map(String::as_str), Some("tunaRound"));
        assert_eq!(s1.display_name.as_deref(), Some("win-claude-tunaRound"));
        // 2차 스캔: s2가 사라짐(exit) → 제거. 수동 등록(win-codex-sup)·타 머신(mac-sess)은 불변.
        let (up2, rm2) = db.sync_presence("win", &[presence("s1", "claude", Some("tunaRound"))], "2026-07-11 10:00:15");
        assert_eq!((up2, rm2), (1, 1));
        let after: Vec<String> = db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90).into_iter().map(|e| e.uuid).collect();
        assert_eq!(after, vec!["mac-sess".to_string(), "s1".to_string(), "win-codex-sup".to_string()]);
    }

    #[test]
    fn sync_presence_preserves_human_input_at() {
        let db = SqliteStore::open_memory().unwrap();
        let t0 = "2026-07-11 10:00:00";
        db.sync_presence("win", &[presence("s1", "claude", None)], t0);
        assert!(db.mark_human_input("s1", "2026-07-11 10:00:03"));
        // 다음 스캔 upsert가 총감독 신호를 지우면 ★가 튄다(v2-42 계약과 동일하게 보존).
        db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:00:15");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:03"));
        assert_eq!(e.last_heartbeat, "2026-07-11 10:00:15");
    }

    // --- v2-45 P4: human_input_at 영속(agent_human_input 테이블) ---

    /// 파일 DB를 새 경로로 열고 마무리에 지우는 테스트 도우미(재기동 시뮬레이션용).
    fn temp_db_path(tag: &str) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("tuna-p4-{tag}-{}.db", std::process::id()));
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
        }
        p
    }
    fn cleanup_db(p: &std::path::Path) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", p.display()));
        }
    }

    #[test]
    fn mark_human_input_records_even_when_unregistered() {
        let db = SqliteStore::open_memory().unwrap();
        // 무장 전(로스터에 없음) 핑도 선기록되어 true(404 유실 창 제거).
        assert!(db.mark_human_input("u1", "2026-07-11 10:00:03"));
        // 이후 register가 영속 테이블에서 ★를 복원한다.
        db.register_agent("u1", tags(&[("machine", "win")]), None, "2026-07-11 10:00:10");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:03"), "미등록 선기록이 register에서 복원");
    }

    #[test]
    fn human_input_persists_across_broker_restart() {
        let path = temp_db_path("restart");
        let p = path.to_str().unwrap();
        {
            let db = SqliteStore::open(p).unwrap();
            db.register_agent("s1", tags(&[("machine", "win")]), None, "2026-07-11 10:00:00");
            assert!(db.mark_human_input("s1", "2026-07-11 10:00:05"));
        }
        // 재기동 = 새 SqliteStore(인메모리 로스터 비어 있음). register가 테이블에서 ★ 복원.
        {
            let db = SqliteStore::open(p).unwrap();
            db.register_agent("s1", tags(&[("machine", "win")]), None, "2026-07-11 10:05:00");
            let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:05:10", 90)[0];
            assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:05"), "재기동 후 ★ 영속 복원");
        }
        cleanup_db(&path);
    }

    #[test]
    fn sync_presence_restores_human_input_from_table_after_restart() {
        let path = temp_db_path("sync-restore");
        let p = path.to_str().unwrap();
        {
            let db = SqliteStore::open(p).unwrap();
            db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:00:00");
            assert!(db.mark_human_input("s1", "2026-07-11 10:00:07"));
        }
        {
            // 재기동 후 스캐너 첫 보고(sync_presence)가 테이블에서 ★를 복원해야 한다(≤15초 자동 복원).
            let db = SqliteStore::open(p).unwrap();
            db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:05:00");
            let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:05:10", 90)[0];
            assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:07"), "sync가 테이블에서 ★ 복원");
        }
        cleanup_db(&path);
    }

    #[test]
    fn deregister_deletes_persisted_human_input() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("s1", tags(&[("machine", "win")]), None, "2026-07-11 10:00:00");
        db.mark_human_input("s1", "2026-07-11 10:00:05");
        assert!(db.deregister_agent("s1"));
        assert_eq!(db.load_human_input("s1"), None, "deregister가 영속 ★ 행도 제거");
        // 재등록해도 복원할 값이 없다.
        db.register_agent("s1", tags(&[("machine", "win")]), None, "2026-07-11 10:10:00");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:10:05", 90)[0];
        assert_eq!(e.human_input_at, None, "제거된 ★는 재등록 시 복원 안 됨");
    }

    #[test]
    fn sync_presence_stale_deletes_persisted_human_input() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:00:00");
        db.mark_human_input("s1", "2026-07-11 10:00:05");
        // s1이 다음 스캔 보고에서 사라짐(exit) → stale 제거 + 영속 행 GC.
        db.sync_presence("win", &[], "2026-07-11 10:00:20");
        assert_eq!(db.load_human_input("s1"), None, "stale 제거가 영속 ★ 행도 GC");
    }

    #[test]
    fn sync_presence_preserves_persisted_signal_across_upsert() {
        let path = temp_db_path("wt");
        let p = path.to_str().unwrap();
        {
            // mark로 테이블에 기록된 ★가 이후 sync upsert를 거쳐도 소실되지 않아야 한다(sync는 인메모리
            // 값을 그대로 유지하고 테이블을 지우지 않음 - 재기동 후에도 영속 보존).
            let db = SqliteStore::open(p).unwrap();
            db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:00:00");
            db.mark_human_input("s1", "2026-07-11 10:00:05");
            db.sync_presence("win", &[presence("s1", "claude", None)], "2026-07-11 10:00:15");
        }
        {
            let db = SqliteStore::open(p).unwrap();
            assert_eq!(db.load_human_input("s1").as_deref(), Some("2026-07-11 10:00:05"), "sync upsert가 영속 ★를 보존");
        }
        cleanup_db(&path);
    }

    #[test]
    fn gc_human_input_removes_only_stale_rows() {
        let db = SqliteStore::open_memory().unwrap();
        // 7일보다 훨씬 과거/미래 행을 직접 심는다(빈 테이블이라 단조 UPSERT가 그대로 삽입).
        db.persist_human_input("old", "2020-01-01 00:00:00");
        db.persist_human_input("fresh", "2099-01-01 00:00:00");
        db.gc_human_input();
        assert_eq!(db.load_human_input("old"), None, "보존기간 초과 행은 GC");
        assert_eq!(db.load_human_input("fresh").as_deref(), Some("2099-01-01 00:00:00"), "신선 행은 보존");
    }

    #[test]
    fn persist_human_input_is_monotonic() {
        let db = SqliteStore::open_memory().unwrap();
        db.persist_human_input("s1", "2026-07-11 10:00:05");
        db.persist_human_input("s1", "2026-07-11 09:00:00"); // 과거 = 무시(단조)
        assert_eq!(db.load_human_input("s1").as_deref(), Some("2026-07-11 10:00:05"), "과거 값으로 되감기 없음");
        db.persist_human_input("s1", "2026-07-11 11:00:00"); // 미래 = 전진
        assert_eq!(db.load_human_input("s1").as_deref(), Some("2026-07-11 11:00:00"), "새 값은 전진");
    }

    // --- v2-45 P5: 스캐너 보고값(codex 입력 신호) merge(§5-8 최종형) ---

    #[test]
    fn sync_presence_reported_input_advances_and_persists() {
        let db = SqliteStore::open_memory().unwrap();
        // codex 세션이 첫 등장 + 보고값(사람 입력 시각) → 인메모리·영속 양쪽에 반영.
        db.sync_presence("win", &[presence_with_input("c1", "2026-07-11 10:00:05")], "2026-07-11 10:00:10");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:15", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:05"), "보고값이 로스터에 반영");
        assert_eq!(db.load_human_input("c1").as_deref(), Some("2026-07-11 10:00:05"), "보고값이 영속에 write-through");
    }

    #[test]
    fn sync_presence_reported_input_takes_max_not_regress() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence("win", &[presence_with_input("c1", "2026-07-11 10:00:30")], "2026-07-11 10:00:31");
        // 다음 보고가 더 과거 값이어도(rollout 캐시 지연 등) 후퇴하지 않는다(max-merge).
        db.sync_presence("win", &[presence_with_input("c1", "2026-07-11 10:00:10")], "2026-07-11 10:00:45");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:50", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:30"), "과거 보고로 후퇴 안 함");
        // 더 새 보고는 전진.
        db.sync_presence("win", &[presence_with_input("c1", "2026-07-11 10:01:00")], "2026-07-11 10:01:01");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:01:05", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:01:00"), "새 보고는 전진");
    }

    #[test]
    fn sync_presence_no_report_preserves_existing_star() {
        let db = SqliteStore::open_memory().unwrap();
        // 훅으로 기록된 ★(claude) 후 스캐너가 보고값 없이(None) upsert해도 보존(max-merge).
        db.sync_presence("win", &[presence("c1", "codex", None)], "2026-07-11 10:00:00");
        db.mark_human_input("c1", "2026-07-11 10:00:05");
        db.sync_presence("win", &[presence("c1", "codex", None)], "2026-07-11 10:00:15");
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)[0];
        assert_eq!(e.human_input_at.as_deref(), Some("2026-07-11 10:00:05"), "무보고 upsert가 기존 ★ 보존");
    }

    #[test]
    fn list_agents_filters_by_selector_subset() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "u1",
            tags(&[("machine", "win"), ("runner", "claude"), ("role", "worker")]),
            None,
            "2026-07-04 10:00:00",
        );
        db.register_agent("u2", tags(&[("machine", "win"), ("runner", "codex")]), None, "2026-07-04 10:00:00");
        let now = "2026-07-04 10:00:10";
        let found = db.list_agents(&tags(&[("machine", "win"), ("runner", "claude")]), now, 90);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
    }
}
