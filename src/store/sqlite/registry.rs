// 에이전트 로스터(인메모리 RefCell 풀, 영속 아님).

use std::collections::BTreeMap;

use super::*;

impl SqliteStore {
    /// 에이전트를 로스터에 등록(있으면 교체). now는 last_heartbeat 초기값.
    /// 재등록(재기동) 시 기존 human_input_at(총감독 신호)은 보존한다(설계 v2-42).
    pub fn register_agent(
        &self,
        uuid: &str,
        tags: BTreeMap<String, String>,
        display_name: Option<String>,
        now: &str,
    ) {
        let mut roster = self.agent_roster.borrow_mut();
        let human_input_at = roster.get(uuid).and_then(|e| e.human_input_at.clone());
        roster.insert(
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
        for uuid in stale {
            roster.remove(&uuid);
        }
        for s in sessions {
            let human_input_at = roster.get(&s.uuid).and_then(|e| e.human_input_at.clone());
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
    /// 미등록 uuid면 false(무장=등록 선행 필요).
    pub fn mark_human_input(&self, uuid: &str, now: &str) -> bool {
        match self.agent_roster.borrow_mut().get_mut(uuid) {
            Some(entry) => {
                entry.human_input_at = Some(now.to_string());
                true
            }
            None => false,
        }
    }

    /// 로스터에서 에이전트를 즉시 제거(세션 종료 시 disarm이 호출, 설계 v2-43 잔존구간 제거).
    /// 존재했으면 true, 미등록이면 false. TTL(90초) 자연소멸을 기다리지 않고 닫힌 세션을 바로 없앤다.
    pub fn deregister_agent(&self, uuid: &str) -> bool {
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
        }
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
