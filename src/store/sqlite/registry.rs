// 에이전트 로스터 + 발견 후보 풀(인메모리 RefCell 풀, 영속 아님).

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

    /// 발견 후보를 풀에 보고(upsert). uuid 단위로 교체하며 reported_at은 브로커 수신 시각(now)으로
    /// 덮어쓴다(리포터 시계 불신). 재보고 없는 후보는 list_candidates의 TTL로 자연 제외된다.
    pub fn report_candidates(&self, candidates: Vec<CandidateEntry>, now: &str) {
        let mut pool = self.candidate_pool.borrow_mut();
        for mut c in candidates {
            c.reported_at = now.to_string();
            pool.insert(c.uuid.clone(), c);
        }
    }

    /// fresh(reported_at이 ttl_secs 이내)인 후보를 uuid 오름차순으로 반환(clone).
    pub fn list_candidates(&self, now: &str, ttl_secs: i64) -> Vec<CandidateEntry> {
        let mut out: Vec<CandidateEntry> = self
            .candidate_pool
            .borrow()
            .values()
            .filter(|c| crate::store::candidates::is_fresh(&c.reported_at, now, ttl_secs))
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

    /// online 에이전트의 "무장 식별자" 집합: 각 에이전트의 uuid + `session` 태그값(있으면).
    /// 후보 overlay가 candidate uuid(=jsonl 세션 id)를 여기에 대조해, 이미 무장된 세션을 armed로 표시한다.
    /// uuid=세션id로 무장한 세션(autoarm·arm 프롬프트)은 uuid로 매칭되고, 고정 이름으로 무장한 감독
    /// (로스터 uuid=친숙명, 예: mac-claude-sup)은 `session` 태그에 자기 세션 id를 실어야 매칭돼 후보에서
    /// 정확히 제외된다. session id는 uuid 공간이라 무관 에이전트와 충돌하지 않는다.
    pub fn armed_session_ids(&self, now: &str, ttl_secs: i64) -> std::collections::HashSet<String> {
        let mut set = std::collections::HashSet::new();
        for entry in self.list_agents(&BTreeMap::new(), now, ttl_secs) {
            if let Some(sid) = entry.tags.get("session")
                && !sid.is_empty()
            {
                set.insert(sid.clone());
            }
            set.insert(entry.uuid);
        }
        set
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

    #[test]
    fn armed_session_ids_includes_uuid_and_session_tag() {
        let db = SqliteStore::open_memory().unwrap();
        // (a) uuid=세션 id로 무장(autoarm·arm 프롬프트 경로).
        db.register_agent("sess-uuid-1", tags(&[("runner", "claude")]), None, "2026-07-04 10:00:00");
        // (b) 고정 이름으로 무장하되 session 태그에 세션 id를 실음(레거시 감독 마이그레이션 경로).
        db.register_agent(
            "mac-claude-sup",
            tags(&[("runner", "claude"), ("session", "e0502b88")]),
            None,
            "2026-07-04 10:00:00",
        );
        // (c) offline 에이전트는 무시돼야 함.
        db.register_agent("stale", tags(&[("session", "zzz")]), None, "2026-07-04 09:00:00");

        let armed = db.armed_session_ids("2026-07-04 10:00:10", 90);
        assert!(armed.contains("sess-uuid-1"), "uuid로 무장한 세션은 uuid로 매칭");
        assert!(armed.contains("mac-claude-sup"), "고정 이름 uuid도 포함");
        assert!(armed.contains("e0502b88"), "session 태그의 세션 id로도 매칭(핵심 수정)");
        assert!(!armed.contains("zzz"), "offline 에이전트의 session 태그는 제외");
    }

    fn candidate(uuid: &str) -> CandidateEntry {
        CandidateEntry {
            uuid: uuid.to_string(),
            runner: "claude".to_string(),
            project: Some("tunaround".to_string()),
            machine: Some("win".to_string()),
            source: "claude-jsonl".to_string(),
            age_secs: 5,
            reported_at: String::new(), // report_candidates가 now로 덮어씀
        }
    }

    #[test]
    fn report_then_list_candidates_roundtrip_and_upsert() {
        let db = SqliteStore::open_memory().unwrap();
        db.report_candidates(vec![candidate("s1"), candidate("s2")], "2026-07-06 10:00:00");
        let found = db.list_candidates("2026-07-06 10:00:10", 180);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].uuid, "s1");
        assert_eq!(found[0].reported_at, "2026-07-06 10:00:00"); // 브로커 now로 채워짐
        // 같은 uuid 재보고는 upsert(교체), 개수 불변.
        db.report_candidates(vec![candidate("s1")], "2026-07-06 10:01:00");
        let again = db.list_candidates("2026-07-06 10:01:05", 180);
        assert_eq!(again.len(), 2);
        let s1 = again.iter().find(|c| c.uuid == "s1").unwrap();
        assert_eq!(s1.reported_at, "2026-07-06 10:01:00");
    }

    #[test]
    fn list_candidates_excludes_stale() {
        let db = SqliteStore::open_memory().unwrap();
        db.report_candidates(vec![candidate("s1")], "2026-07-06 09:00:00");
        // now 기준 1시간 경과, ttl 180초 -> stale이라 제외되어야 함.
        let found = db.list_candidates("2026-07-06 10:00:00", 180);
        assert!(found.is_empty(), "stale 후보는 list_candidates에서 제외되어야 함");
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
