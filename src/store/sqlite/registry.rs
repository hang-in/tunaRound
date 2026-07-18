// 에이전트 로스터(인메모리 RefCell 풀). human_input_at(총감독 ★)만 agent_human_input 테이블로 영속한다.

use std::collections::BTreeMap;

use rusqlite::OptionalExtension;

use super::*;

/// human_input_at 영속 행 보존기간(일). deregister(명시적 종료 신호)를 안 탄 고아 행(#8: stale
/// 제거된 세션도 여기 포함 - 진짜 종료인지 스캔 누락인지 즉시 구분할 수 없어 이 기간까지 유예한다)을
/// 이 기간 뒤 GC한다.
const HUMAN_INPUT_RETAIN_DAYS: u32 = 7;

/// presence 이벤트 이력 보존기간(일, v2-50). 이력이라 human_input 최신값(7일)보다 길게 잡는다.
const PRESENCE_EVENTS_RETAIN_DAYS: u32 = 30;

impl SqliteStore {
    // ---- 총감독 ★ 신호(human_input_at) 영속(v2-45 P4, agent_human_input 테이블) ----

    /// 영속 테이블에서 uuid의 human_input_at을 읽는다(재기동 후 인메모리 로스터가 빈 상태에서 ★ 복원
    /// 폴백). 행이 없으면 None. DB 에러도 None으로 흡수한다(★ 복원은 best-effort, 로스터를 막지 않음).
    fn load_human_input(&self, uuid: &str) -> Option<String> {
        self.conn
            .query_row(
                "SELECT at FROM agent_human_input WHERE uuid = ?1",
                [uuid],
                |r| r.get::<_, String>(0),
            )
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

    /// 영속 human_input_at 행을 제거한다. best-effort. **진짜 종료 신호(deregister_agent)에서만
    /// 즉시 호출한다(#8).** sync_presence의 stale 제거(스캐너가 한 주기 세션을 놓친 것 - 진짜 종료와
    /// 구분 불가)는 더 이상 이 함수를 호출하지 않는다: 즉시 삭제하면 스캐너가 살아있는 세션을 한
    /// 주기(15초) 빠뜨리기만 해도 카드는 다음 주기에 부활하지만 ★는 이미 지워져 복원 불가였다. stale로
    /// 사라진 세션의 ★는 gc_human_input의 보존기간(7일)에 맡긴다.
    fn delete_human_input(&self, uuid: &str) {
        let _ = self
            .conn
            .execute("DELETE FROM agent_human_input WHERE uuid = ?1", [uuid]);
    }

    /// 보존기간(기본 7일) 초과 human_input_at 행을 GC한다. deregister를 안 타고 남은 고아 행(#8:
    /// stale 제거된 세션 포함 - 스캐너가 다시 보고하지 않는 uuid)을 정리한다. sync_presence가 매 스캔
    /// 주기에 호출 = 자연 주기 훅.
    fn gc_human_input(&self) {
        let _ = self.conn.execute(
            "DELETE FROM agent_human_input WHERE at < datetime('now', ?1)",
            [format!("-{HUMAN_INPUT_RETAIN_DAYS} days")],
        );
    }

    // ---- presence 이벤트 이력(v2-50, presence_events 테이블) ----

    /// presence edge(appear/disappear/human_input)를 presence_events 테이블에 best-effort 기록한다.
    /// INSERT 실패가 로스터/통지를 막지 않는다(persist_human_input 규약 답습, 순수 append INSERT).
    /// machine/runner/project는 tags에서 뽑고 display_name은 별도로 받아 컬럼을 채운다(순수 raw 기록,
    /// ★-도출 로직은 넣지 않음). `at`은 이벤트 시각(appear/disappear=now, human_input=입력 시각).
    fn log_presence_event(
        &self,
        event_type: &str,
        uuid: &str,
        tags: &BTreeMap<String, String>,
        display_name: Option<&str>,
        detail: Option<&str>,
        at: &str,
    ) {
        let _ = self.conn.execute(
            "INSERT INTO presence_events\
             (at, event_type, agent_uuid, machine, runner, project, display_name, detail) \
             VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                at,
                event_type,
                uuid,
                tags.get("machine"),
                tags.get("runner"),
                tags.get("project"),
                display_name,
                detail,
            ],
        );
    }

    /// 보존기간(기본 30일) 초과 presence 이벤트 행을 GC한다. sync_presence가 매 스캔 주기에 호출
    /// (gc_human_input 옆) = 자연 주기 훅. 이력 테이블이라 시간 인덱스(idx_presence_events_at)로 삭제된다.
    fn gc_presence_events(&self) {
        let _ = self.conn.execute(
            "DELETE FROM presence_events WHERE at < datetime('now', ?1)",
            [format!("-{PRESENCE_EVENTS_RETAIN_DAYS} days")],
        );
    }

    /// presence 이벤트 이력을 최신순(at DESC, id DESC)으로 조회한다. since(Some)면 at >= since 필터.
    /// limit로 상한을 건다(무인증 원격 관전 방어). DB 오류는 Err로 표면화한다(엔드포인트가 500으로).
    pub fn list_presence_events(
        &self,
        since: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::store::agents::PresenceEvent>, String> {
        use crate::store::agents::PresenceEvent;
        fn map_row(r: &rusqlite::Row) -> rusqlite::Result<PresenceEvent> {
            Ok(PresenceEvent {
                id: r.get(0)?,
                at: r.get(1)?,
                event_type: r.get(2)?,
                agent_uuid: r.get(3)?,
                machine: r.get(4)?,
                runner: r.get(5)?,
                project: r.get(6)?,
                display_name: r.get(7)?,
                detail: r.get(8)?,
            })
        }
        let cols = "id, at, event_type, agent_uuid, machine, runner, project, display_name, detail";
        let limit = limit as i64;
        let rows = match since {
            Some(ts) => {
                let mut stmt = self
                    .conn
                    .prepare(&format!(
                        "SELECT {cols} FROM presence_events WHERE at >= ?1 \
                         ORDER BY at DESC, id DESC LIMIT ?2"
                    ))
                    .map_err(|e| format!("sqlite: {e}"))?;
                stmt.query_map(rusqlite::params![ts, limit], map_row)
                    .map_err(|e| format!("sqlite: {e}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("sqlite: {e}"))?
            }
            None => {
                let mut stmt = self
                    .conn
                    .prepare(&format!(
                        "SELECT {cols} FROM presence_events ORDER BY at DESC, id DESC LIMIT ?1"
                    ))
                    .map_err(|e| format!("sqlite: {e}"))?;
                stmt.query_map(rusqlite::params![limit], map_row)
                    .map_err(|e| format!("sqlite: {e}"))?
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|e| format!("sqlite: {e}"))?
            }
        };
        Ok(rows)
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
        // roster 부재→존재 첫 진입이면 경로 무관 appear 1회(v2-50 대칭화). deregister가 disappear를
        // 남기듯, register(워커·infra 경로)도 신규 uuid면 appear를 남긴다 = 짝 없는 disappear 방지.
        // 이미 있으면 재등록(재기동/재무장)이라 생략 = 전이당 appear 1회. sync_presence의 appear는
        // `!roster.contains_key`로 이미 여기서 들어온 uuid를 억제하므로 이중 로깅되지 않는다.
        // insert가 tags/display_name을 소비하므로 그 전에 로깅한다.
        let is_new = !self.agent_roster.borrow().contains_key(uuid);
        if is_new {
            self.log_presence_event("appear", uuid, &tags, display_name.as_deref(), None, now);
        }
        self.agent_roster.borrow_mut().insert(
            uuid.to_string(),
            AgentEntry {
                uuid: uuid.to_string(),
                tags,
                display_name,
                last_heartbeat: now.to_string(),
                human_input_at,
                // 재등록(재기동/재무장)은 새 턴 신호가 아니므로 기존 인메모리 값을 승계하지 않고
                // 비운다(어차피 다음 turn-ping/스캔이 채운다).
                turn_active_at: None,
            },
        );
    }

    /// 이슈 #123: 대화 턴 시작 신호(claude turn-ping start / human-ping 동승). 로스터에 있으면
    /// turn_active_at을 갱신하고 true, 미등록 uuid면 false(스캐너 등록 전 신호는 유실 = 다음 턴이 채움).
    pub fn record_turn_start(&self, uuid: &str, now: &str) -> bool {
        match self.agent_roster.borrow_mut().get_mut(uuid) {
            Some(e) => {
                e.turn_active_at = Some(now.to_string());
                true
            }
            None => false,
        }
    }

    /// 이슈 #123: 대화 턴 종료 신호(claude Stop 훅). turn_active_at을 클리어한다(스피너 즉시 소등).
    pub fn record_turn_end(&self, uuid: &str) -> bool {
        match self.agent_roster.borrow_mut().get_mut(uuid) {
            Some(e) => {
                e.turn_active_at = None;
                true
            }
            None => false,
        }
    }

    /// presence 스캐너 일괄 동기화(설계 v2-44 §6): 보고된 세션은 upsert(human_input_at 보존),
    /// 같은 machine의 스캐너 소유(`src=scan`) 항목 중 보고에 없는 것은 제거한다(유령 원천 차단).
    /// 소유 태그로 격리하므로 수동 register(워커·infra·수신 poll) 항목은 건드리지 않는다: stale
    /// 제거뿐 아니라 upsert도 대상 uuid가 이미 로스터에 있고 소유 태그(`src`)가 `scan`이 아니면
    /// 건너뛴다(#7 - 라이브 세션이 자기 uuid로 커스텀 태그를 register한 뒤 다음 스캔에 스캐너 기본
    /// 태그로 덮여 to_selector 라우팅이 플랩하는 것 방지).
    /// 반환=(upsert 수, 제거 수). uuid가 수동 소유라 스킵된 건도 upsert 시도 수에는 포함한다(호출자
    /// 로그는 "몇 건을 보고받았나"이지 "몇 건을 실제로 갱신했나"가 아니므로 반환값 계약은 불변).
    pub fn sync_presence(
        &self,
        machine: &str,
        sessions: &[crate::store::agents::PresenceUpsert],
        now: &str,
    ) -> (usize, usize) {
        let mut roster = self.agent_roster.borrow_mut();
        let reported: std::collections::HashSet<&str> =
            sessions.iter().map(|s| s.uuid.as_str()).collect();
        let stale: Vec<AgentEntry> = roster
            .values()
            .filter(|e| {
                e.tags.get("machine").map(String::as_str) == Some(machine)
                    && e.tags.get("src").map(String::as_str) == Some("scan")
                    && !reported.contains(e.uuid.as_str())
            })
            .cloned()
            .collect();
        let removed = stale.len();
        for e in &stale {
            roster.remove(&e.uuid);
            // 소멸(disappear, 사유=stale) raw edge 기록(best-effort, v2-50).
            self.log_presence_event(
                "disappear",
                &e.uuid,
                &e.tags,
                e.display_name.as_deref(),
                Some("stale"),
                now,
            );
            // #8: stale 제거는 즉시 delete_human_input을 호출하지 않는다. 스캐너가 살아있는 세션을
            // 한 주기(15초) 빠뜨리기만 해도 로스터 카드는 다음 주기에 부활하지만, 영속 ★(진짜 종료
            // 신호가 아닌데) 이 자리에서 지워버리면 그 사이 복원 불가로 증발한다. 진짜 종료 신호는
            // deregister_agent(명시적 disarm)뿐이라 그 경로만 즉시 삭제하고, stale은 gc_human_input의
            // 7일 보존기간에 맡긴다(진짜로 사라진 세션은 그 안에 자연 GC됨, 재등장 세션은 ★ 보존).
        }
        for s in sessions {
            // #7 소유권 가드: 이 uuid가 이미 로스터에 있고 소유 태그(src)가 scan이 아니면(수동
            // register 소유 - 예: 워커가 role=worker로 자기 uuid를 등록) 스캐너 upsert가 건드리지
            // 않는다. 지금까지 격리는 stale 제거(위 for e in &stale)에만 있었고 upsert엔 없어, 같은
            // uuid를 스캐너가 재보고하면 태그가 스캐너 기본값(role=session 등)으로 조용히 교체돼
            // to_selector 라우팅이 플랩했다.
            if let Some(existing) = roster.get(&s.uuid)
                && existing.tags.get("src").map(String::as_str) != Some("scan")
            {
                continue;
            }
            // §5-8 최종형: human_input_at = max(인메모리, 스캐너 보고값, 영속 테이블).
            // base = 인메모리(있으면 직전 write-through로 테이블과 동기) 또는 재기동 복원(테이블 SELECT).
            // 보고값(codex 입력 신호)이 base보다 새로우면(merged != base) 승자를 테이블에 단조
            // write-through한다. 보고값 없음(claude)·불변이면 write 생략(P4의 N+1 회피 유지).
            let mem = roster.get(&s.uuid).and_then(|e| e.human_input_at.clone());
            let base = mem.or_else(|| self.load_human_input(&s.uuid));
            // Option<String>은 None < Some 순서(파생 Ord)라 std::cmp::max가 곧 max(base, 보고값)이다
            // (DB datetime 포맷은 사전순=시간순, gemini 리뷰). 커스텀 헬퍼 대신 stdlib.
            let human_input_at = std::cmp::max(base.clone(), s.human_input_at.clone());
            // 이슈 #123: 턴 활동 신호도 인메모리와 max-merge한다. 스캐너 upsert가 엔트리를 재구성하므로
            // 병합 없이는 claude의 turn-ping(인메모리 전용)이 매 스캔(15초)마다 증발한다. claude의
            // turn-end(None 클리어)는 보고값도 None(claude는 active_at 미보고)이라 그대로 유지된다.
            let turn_active_at = std::cmp::max(
                roster.get(&s.uuid).and_then(|e| e.turn_active_at.clone()),
                s.active_at.clone(),
            );
            let mut tags = BTreeMap::new();
            tags.insert("machine".to_string(), machine.to_string());
            tags.insert("runner".to_string(), s.runner.clone());
            tags.insert("role".to_string(), "session".to_string());
            tags.insert("session".to_string(), s.uuid.clone());
            tags.insert("src".to_string(), "scan".to_string());
            if let Some(p) = &s.project {
                tags.insert("project".to_string(), p.clone());
            }
            // 등장(appear) = 직전 roster에 없던 uuid(insert 전 검사). raw edge 기록(best-effort, v2-50).
            if !roster.contains_key(&s.uuid) {
                self.log_presence_event(
                    "appear",
                    &s.uuid,
                    &tags,
                    s.display_name.as_deref(),
                    None,
                    now,
                );
            }
            // human_input_at 전진(codex 보고값 경로)만 영속 write-through + raw 이벤트 기록. 보고값
            // 없음(claude)·불변이면 write/log 생략 = 매 heartbeat 로깅 방지(claude ★는 mark_human_input에서).
            if human_input_at != base
                && let Some(at) = &human_input_at
            {
                self.persist_human_input(&s.uuid, at);
                self.log_presence_event(
                    "human_input",
                    &s.uuid,
                    &tags,
                    s.display_name.as_deref(),
                    None,
                    at,
                );
            }
            roster.insert(
                s.uuid.clone(),
                AgentEntry {
                    uuid: s.uuid.clone(),
                    tags,
                    display_name: s.display_name.clone(),
                    last_heartbeat: now.to_string(),
                    human_input_at,
                    turn_active_at,
                },
            );
        }
        self.gc_human_input(); // 매 스캔 주기 = 7일 초과 고아 행 정리 훅(테이블이 작아 부담 없음)
        self.gc_presence_events(); // 매 스캔 주기 = 30일 초과 이력 정리 훅(v2-50)
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
        // 전진 판정: 직전 값(인메모리 또는 영속)보다 now가 새로우면 raw human_input 이벤트를 기록한다
        // (매 핑이 아니라 실제 전진 시에만 - 같은 초 중복 핑은 스킵). ★-도출은 프론트 몫(v2-50).
        // register_agent와 동일한 인메모리→영속 폴백으로 prior를 읽는다(RefCell/conn 상이 필드라 안전).
        let prior = self
            .agent_roster
            .borrow()
            .get(uuid)
            .and_then(|e| e.human_input_at.clone())
            .or_else(|| self.load_human_input(uuid));
        let advanced = prior.as_deref().is_none_or(|p| now > p);
        self.persist_human_input(uuid, now); // DB 선기록(등록 여부 무관, now는 항상 최신이라 단조 통과)
        if advanced {
            // tags/display_name은 로스터에 있으면 채우고, 무장 전 핑이면 비어 있다(컬럼 NULL).
            let (tags, display_name) = self
                .agent_roster
                .borrow()
                .get(uuid)
                .map(|e| (e.tags.clone(), e.display_name.clone()))
                .unwrap_or_default();
            self.log_presence_event(
                "human_input",
                uuid,
                &tags,
                display_name.as_deref(),
                None,
                now,
            );
        }
        // 인메모리 갱신도 전진(advanced)일 때만 한다(CodeRabbit MAJOR). advanced와 무관하게 항상
        // 대입하면 과거·동일 시각 핑이 인메모리 ★를 과거로 되감아(★ 회귀) sync_presence의 max-merge와
        // 어긋난다. persist_human_input은 이미 단조(WHERE >)라 DB는 안전하지만 인메모리만 뚫려 있었다.
        if advanced && let Some(entry) = self.agent_roster.borrow_mut().get_mut(uuid) {
            entry.human_input_at = Some(now.to_string());
        }
        true
    }

    /// 로스터에서 에이전트를 즉시 제거(세션 종료 시 disarm이 호출, 설계 v2-43 잔존구간 제거).
    /// 존재했으면 true, 미등록이면 false. TTL(90초) 자연소멸을 기다리지 않고 닫힌 세션을 바로 없앤다.
    /// 세션 종료이므로 영속 ★ 행도 함께 GC한다(v2-45 P4).
    pub fn deregister_agent(&self, uuid: &str) -> bool {
        // 소멸(disappear, 사유=deregister) raw edge 기록(best-effort, v2-50). 제거 전 로스터에서
        // tags/display_name을 확보해 이벤트 컬럼을 채운다. now는 여기서 조회(시그니처 불변 유지).
        if let Some((tags, display_name)) = self
            .agent_roster
            .borrow()
            .get(uuid)
            .map(|e| (e.tags.clone(), e.display_name.clone()))
        {
            let now = self.now().unwrap_or_default();
            self.log_presence_event(
                "disappear",
                uuid,
                &tags,
                display_name.as_deref(),
                Some("deregister"),
                &now,
            );
        }
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
        self.list_agents(selector, now, ttl_secs)
            .into_iter()
            .map(|entry| entry.uuid)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn register_then_list_agents_roundtrip() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "u1",
            tags(&[("machine", "win")]),
            Some("win-claude".into()),
            "2026-07-04 10:00:00",
        );
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
        assert!(
            found.is_empty(),
            "offline 에이전트는 list_agents에서 제외되어야 함"
        );
    }

    #[test]
    fn deregister_agent_removes_and_reports_presence() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent("u1", BTreeMap::new(), None, "2026-07-04 10:00:00");
        // 등록된 세션 제거 = true, 이후 online 목록에서 즉시 사라짐(TTL 대기 없이).
        assert!(db.deregister_agent("u1"));
        assert!(
            db.list_agents(&BTreeMap::new(), "2026-07-04 10:00:05", 90)
                .is_empty()
        );
        // 미등록/이미 제거된 uuid는 false(멱등).
        assert!(!db.deregister_agent("u1"));
        assert!(!db.deregister_agent("unknown"));
    }

    #[test]
    fn resolve_selector_matches_none_one_or_many() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "u1",
            tags(&[("machine", "win"), ("runner", "claude")]),
            None,
            "2026-07-04 10:00:00",
        );
        db.register_agent(
            "u2",
            tags(&[("machine", "mac"), ("runner", "claude")]),
            None,
            "2026-07-04 10:00:00",
        );
        let now = "2026-07-04 10:00:10";

        let none = db.resolve_selector(&tags(&[("machine", "linux")]), now, 90);
        assert!(none.is_empty());

        let one = db.resolve_selector(&tags(&[("machine", "mac")]), now, 90);
        assert_eq!(one, vec!["u2".to_string()]);

        let many = db.resolve_selector(&tags(&[("runner", "claude")]), now, 90);
        assert_eq!(many, vec!["u1".to_string(), "u2".to_string()]);
    }

    fn presence(
        uuid: &str,
        runner: &str,
        project: Option<&str>,
    ) -> crate::store::agents::PresenceUpsert {
        crate::store::agents::PresenceUpsert {
            uuid: uuid.to_string(),
            runner: runner.to_string(),
            project: project.map(str::to_string),
            display_name: project.map(|p| format!("win-{runner}-{p}")),
            human_input_at: None,
            active_at: None,
        }
    }

    /// 스캐너 보고값(codex 입력 신호)이 있는 presence 항목(v2-45 P5).
    fn presence_with_input(uuid: &str, at: &str) -> crate::store::agents::PresenceUpsert {
        crate::store::agents::PresenceUpsert {
            human_input_at: Some(at.to_string()),
            ..presence(uuid, "codex", None)
        }
    }

    #[test]
    fn record_turn_start_end_roundtrip_and_unregistered_noop() {
        let db = SqliteStore::open_memory().unwrap();
        let now = "2026-07-18 10:00:00";
        assert!(!db.record_turn_start("ghost", now), "미등록=false(no-op)");
        assert!(!db.record_turn_end("ghost"));
        db.register_agent("s1", tags(&[("runner", "claude")]), None, now);
        assert!(db.record_turn_start("s1", now));
        let turn = |db: &SqliteStore| {
            db.list_agents(&std::collections::BTreeMap::new(), now, i64::MAX)
                .into_iter()
                .find(|a| a.uuid == "s1")
                .and_then(|a| a.turn_active_at)
        };
        assert_eq!(turn(&db).as_deref(), Some(now));
        assert!(db.record_turn_end("s1"));
        assert_eq!(turn(&db), None);
    }

    #[test]
    fn sync_presence_merges_turn_signal_and_preserves_claude_ping() {
        let db = SqliteStore::open_memory().unwrap();
        let t0 = "2026-07-18 10:00:00";
        let t1 = "2026-07-18 10:00:10";
        // claude 세션: 스캐너 보고(active_at 없음) 후 turn-ping(start) → 다음 스캔에도 신호가 살아야
        // 한다(스캐너 upsert가 엔트리를 재구성하므로 mem 병합 없이는 15초마다 증발).
        db.sync_presence("win", &[presence("c1", "claude", Some("proj"))], t0);
        assert!(db.record_turn_start("c1", t0));
        db.sync_presence("win", &[presence("c1", "claude", Some("proj"))], t1);
        let turn = |uuid: &str| {
            db.list_agents(&std::collections::BTreeMap::new(), t1, i64::MAX)
                .into_iter()
                .find(|a| a.uuid == uuid)
                .and_then(|a| a.turn_active_at)
        };
        assert_eq!(
            turn("c1").as_deref(),
            Some(t0),
            "claude turn-ping이 스캔을 넘어 보존"
        );
        // turn-end 후 스캔이 와도 None 유지(claude는 active_at 미보고 = max(None,None)).
        assert!(db.record_turn_end("c1"));
        db.sync_presence("win", &[presence("c1", "claude", Some("proj"))], t1);
        assert_eq!(turn("c1"), None, "end 클리어가 스캔에 되살아나면 안 됨");
        // codex 세션: 스캐너 active_at이 단조 병합(max)된다 - 더 오래된 보고는 무시.
        let mut cx = presence("x1", "codex", None);
        cx.active_at = Some(t1.to_string());
        db.sync_presence("win", &[presence("c1", "claude", Some("proj")), cx], t1);
        assert_eq!(turn("x1").as_deref(), Some(t1));
        let mut cx_old = presence("x1", "codex", None);
        cx_old.active_at = Some(t0.to_string());
        db.sync_presence("win", &[presence("c1", "claude", Some("proj")), cx_old], t1);
        assert_eq!(
            turn("x1").as_deref(),
            Some(t1),
            "오래된 mtime 보고가 최신을 후퇴시키면 안 됨"
        );
    }

    #[test]
    fn sync_presence_upserts_and_removes_only_scan_owned() {
        let db = SqliteStore::open_memory().unwrap();
        let t0 = "2026-07-11 10:00:00";
        // 수동 등록 항목(스캐너 소유 아님): infra watcher + 타 머신 세션.
        db.register_agent(
            "win-codex-sup",
            tags(&[("machine", "win"), ("role", "infra")]),
            None,
            t0,
        );
        db.register_agent(
            "mac-sess",
            tags(&[("machine", "mac"), ("role", "session"), ("src", "scan")]),
            None,
            t0,
        );
        // 1차 스캔 보고: s1, s2.
        let (up, rm) = db.sync_presence(
            "win",
            &[
                presence("s1", "claude", Some("tunaRound")),
                presence("s2", "codex", None),
            ],
            t0,
        );
        assert_eq!((up, rm), (2, 0));
        let all = db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:05", 90);
        assert_eq!(all.len(), 4);
        let s1 = all.iter().find(|e| e.uuid == "s1").unwrap();
        assert_eq!(s1.tags.get("role").map(String::as_str), Some("session"));
        assert_eq!(s1.tags.get("src").map(String::as_str), Some("scan"));
        assert_eq!(
            s1.tags.get("project").map(String::as_str),
            Some("tunaRound")
        );
        assert_eq!(s1.display_name.as_deref(), Some("win-claude-tunaRound"));
        // 2차 스캔: s2가 사라짐(exit) → 제거. 수동 등록(win-codex-sup)·타 머신(mac-sess)은 불변.
        let (up2, rm2) = db.sync_presence(
            "win",
            &[presence("s1", "claude", Some("tunaRound"))],
            "2026-07-11 10:00:15",
        );
        assert_eq!((up2, rm2), (1, 1));
        let after: Vec<String> = db
            .list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)
            .into_iter()
            .map(|e| e.uuid)
            .collect();
        assert_eq!(
            after,
            vec![
                "mac-sess".to_string(),
                "s1".to_string(),
                "win-codex-sup".to_string()
            ]
        );
    }

    #[test]
    fn sync_presence_upsert_preserves_manually_owned_tags() {
        // #7: 워커가 role=worker로 자기 uuid를 수동 register한 뒤, 같은 uuid를 스캐너가 일반 세션으로
        // 재보고해도(예: 프로세스 열거가 워커 세션도 잡음) upsert가 스캐너 기본 태그(role=session,
        // src=scan)로 덮으면 안 된다(to_selector 라우팅 플랩 방지).
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "w1",
            tags(&[("machine", "win"), ("role", "worker"), ("src", "manual")]),
            Some("win-worker".into()),
            "2026-07-13 10:00:00",
        );
        db.sync_presence(
            "win",
            &[presence("w1", "claude", None)],
            "2026-07-13 10:00:15",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-13 10:00:20", 90)[0];
        assert_eq!(e.uuid, "w1");
        assert_eq!(
            e.tags.get("role").map(String::as_str),
            Some("worker"),
            "수동 소유 태그가 스캐너 upsert에 덮이지 않아야 함"
        );
        assert_eq!(
            e.tags.get("src").map(String::as_str),
            Some("manual"),
            "src 태그도 스캐너 기본값(scan)으로 교체되지 않아야 함"
        );
    }

    #[test]
    fn sync_presence_preserves_human_input_at() {
        let db = SqliteStore::open_memory().unwrap();
        let t0 = "2026-07-11 10:00:00";
        db.sync_presence("win", &[presence("s1", "claude", None)], t0);
        assert!(db.mark_human_input("s1", "2026-07-11 10:00:03"));
        // 다음 스캔 upsert가 총감독 신호를 지우면 ★가 튄다(v2-42 계약과 동일하게 보존).
        db.sync_presence(
            "win",
            &[presence("s1", "claude", None)],
            "2026-07-11 10:00:15",
        );
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

    /// 실시계 기준 상대 시각(DB datetime 포맷). gc_human_input/gc_presence_events가 실벽시계
    /// datetime('now') 기준이라 고정 날짜 리터럴은 보존기간(7일/30일)을 지나는 순간 테스트가 터지는
    /// 시한폭탄이 된다(2026-07-18 실측: 07-11 리터럴을 쓰던 영속 단언 3건이 정확히 7일째에 폭발).
    /// gc를 지나 영속 생존을 단언하는 테스트는 이 헬퍼로 now 근방 시각을 쓴다.
    fn ts(db: &SqliteStore, offset_secs: i64) -> String {
        db.conn
            .query_row(
                "SELECT datetime('now', ?1)",
                [format!("{offset_secs} seconds")],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn mark_human_input_records_even_when_unregistered() {
        let db = SqliteStore::open_memory().unwrap();
        // 무장 전(로스터에 없음) 핑도 선기록되어 true(404 유실 창 제거).
        assert!(db.mark_human_input("u1", "2026-07-11 10:00:03"));
        // 이후 register가 영속 테이블에서 ★를 복원한다.
        db.register_agent(
            "u1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-11 10:00:10",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some("2026-07-11 10:00:03"),
            "미등록 선기록이 register에서 복원"
        );
    }

    #[test]
    fn human_input_persists_across_broker_restart() {
        let path = temp_db_path("restart");
        let p = path.to_str().unwrap();
        {
            let db = SqliteStore::open(p).unwrap();
            db.register_agent(
                "s1",
                tags(&[("machine", "win")]),
                None,
                "2026-07-11 10:00:00",
            );
            assert!(db.mark_human_input("s1", "2026-07-11 10:00:05"));
        }
        // 재기동 = 새 SqliteStore(인메모리 로스터 비어 있음). register가 테이블에서 ★ 복원.
        {
            let db = SqliteStore::open(p).unwrap();
            db.register_agent(
                "s1",
                tags(&[("machine", "win")]),
                None,
                "2026-07-11 10:05:00",
            );
            let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:05:10", 90)[0];
            assert_eq!(
                e.human_input_at.as_deref(),
                Some("2026-07-11 10:00:05"),
                "재기동 후 ★ 영속 복원"
            );
        }
        cleanup_db(&path);
    }

    #[test]
    fn sync_presence_restores_human_input_from_table_after_restart() {
        let path = temp_db_path("sync-restore");
        let p = path.to_str().unwrap();
        {
            let db = SqliteStore::open(p).unwrap();
            db.sync_presence(
                "win",
                &[presence("s1", "claude", None)],
                "2026-07-11 10:00:00",
            );
            assert!(db.mark_human_input("s1", "2026-07-11 10:00:07"));
        }
        {
            // 재기동 후 스캐너 첫 보고(sync_presence)가 테이블에서 ★를 복원해야 한다(≤15초 자동 복원).
            let db = SqliteStore::open(p).unwrap();
            db.sync_presence(
                "win",
                &[presence("s1", "claude", None)],
                "2026-07-11 10:05:00",
            );
            let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:05:10", 90)[0];
            assert_eq!(
                e.human_input_at.as_deref(),
                Some("2026-07-11 10:00:07"),
                "sync가 테이블에서 ★ 복원"
            );
        }
        cleanup_db(&path);
    }

    #[test]
    fn deregister_deletes_persisted_human_input() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "s1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-11 10:00:00",
        );
        db.mark_human_input("s1", "2026-07-11 10:00:05");
        assert!(db.deregister_agent("s1"));
        assert_eq!(
            db.load_human_input("s1"),
            None,
            "deregister가 영속 ★ 행도 제거"
        );
        // 재등록해도 복원할 값이 없다.
        db.register_agent(
            "s1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-11 10:10:00",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:10:05", 90)[0];
        assert_eq!(e.human_input_at, None, "제거된 ★는 재등록 시 복원 안 됨");
    }

    #[test]
    fn sync_presence_stale_does_not_delete_persisted_human_input() {
        // #8: stale 제거(스캐너가 한 주기 세션을 놓침)는 영속 ★ 행을 즉시 삭제하지 않는다. 진짜 종료가
        // 아니라면 다음 주기에 로스터 카드는 부활하는데, ★를 그 자리에서 지우면 복원 불가로 증발한다.
        // 시각은 실시계 상대(ts): sync_presence가 매 호출 gc_human_input(now-7d)을 돌리므로 고정
        // 리터럴은 시한폭탄이다.
        let db = SqliteStore::open_memory().unwrap();
        let t_sync1 = ts(&db, -40);
        let t_mark = ts(&db, -35);
        let t_sync2 = ts(&db, -20);
        let t_sync3 = ts(&db, -10);
        let t_list = ts(&db, -5);
        db.sync_presence("win", &[presence("s1", "claude", None)], &t_sync1);
        db.mark_human_input("s1", &t_mark);
        // s1이 다음 스캔 보고에서 빠짐(한 주기 누락) → 로스터에서는 제거되지만 영속 ★는 남아야 한다.
        db.sync_presence("win", &[], &t_sync2);
        assert_eq!(
            db.load_human_input("s1").as_deref(),
            Some(t_mark.as_str()),
            "stale 제거는 영속 ★ 행을 지우지 않음(진짜 종료 신호=deregister만 즉시 삭제)"
        );
        // 다음 주기에 재등장하면 ★가 그대로 복원된다(증발하지 않았다는 방증).
        db.sync_presence("win", &[presence("s1", "claude", None)], &t_sync3);
        let e = &db.list_agents(&BTreeMap::new(), &t_list, 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some(t_mark.as_str()),
            "재등장 시 ★ 복원(누락 한 주기로 증발하지 않음)"
        );
    }

    #[test]
    fn sync_presence_preserves_persisted_signal_across_upsert() {
        let path = temp_db_path("wt");
        let p = path.to_str().unwrap();
        let mark_at;
        {
            // mark로 테이블에 기록된 ★가 이후 sync upsert를 거쳐도 소실되지 않아야 한다(sync는 인메모리
            // 값을 그대로 유지하고 테이블을 지우지 않음 - 재기동 후에도 영속 보존). 시각은 실시계
            // 상대(ts, gc 시한폭탄 회피).
            let db = SqliteStore::open(p).unwrap();
            let t_sync1 = ts(&db, -30);
            mark_at = ts(&db, -25);
            let t_sync2 = ts(&db, -15);
            db.sync_presence("win", &[presence("s1", "claude", None)], &t_sync1);
            db.mark_human_input("s1", &mark_at);
            db.sync_presence("win", &[presence("s1", "claude", None)], &t_sync2);
        }
        {
            let db = SqliteStore::open(p).unwrap();
            assert_eq!(
                db.load_human_input("s1").as_deref(),
                Some(mark_at.as_str()),
                "sync upsert가 영속 ★를 보존"
            );
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
        assert_eq!(
            db.load_human_input("fresh").as_deref(),
            Some("2099-01-01 00:00:00"),
            "신선 행은 보존"
        );
    }

    #[test]
    fn persist_human_input_is_monotonic() {
        let db = SqliteStore::open_memory().unwrap();
        db.persist_human_input("s1", "2026-07-11 10:00:05");
        db.persist_human_input("s1", "2026-07-11 09:00:00"); // 과거 = 무시(단조)
        assert_eq!(
            db.load_human_input("s1").as_deref(),
            Some("2026-07-11 10:00:05"),
            "과거 값으로 되감기 없음"
        );
        db.persist_human_input("s1", "2026-07-11 11:00:00"); // 미래 = 전진
        assert_eq!(
            db.load_human_input("s1").as_deref(),
            Some("2026-07-11 11:00:00"),
            "새 값은 전진"
        );
    }

    // --- v2-45 P5: 스캐너 보고값(codex 입력 신호) merge(§5-8 최종형) ---

    #[test]
    fn sync_presence_reported_input_advances_and_persists() {
        let db = SqliteStore::open_memory().unwrap();
        // codex 세션이 첫 등장 + 보고값(사람 입력 시각) → 인메모리·영속 양쪽에 반영.
        // 시각은 실시계 상대(ts, gc 시한폭탄 회피).
        let input_at = ts(&db, -15);
        let sync_at = ts(&db, -10);
        let list_at = ts(&db, -5);
        db.sync_presence("win", &[presence_with_input("c1", &input_at)], &sync_at);
        let e = &db.list_agents(&BTreeMap::new(), &list_at, 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some(input_at.as_str()),
            "보고값이 로스터에 반영"
        );
        assert_eq!(
            db.load_human_input("c1").as_deref(),
            Some(input_at.as_str()),
            "보고값이 영속에 write-through"
        );
    }

    #[test]
    fn sync_presence_reported_input_takes_max_not_regress() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-11 10:00:30")],
            "2026-07-11 10:00:31",
        );
        // 다음 보고가 더 과거 값이어도(rollout 캐시 지연 등) 후퇴하지 않는다(max-merge).
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-11 10:00:10")],
            "2026-07-11 10:00:45",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:50", 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some("2026-07-11 10:00:30"),
            "과거 보고로 후퇴 안 함"
        );
        // 더 새 보고는 전진.
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-11 10:01:00")],
            "2026-07-11 10:01:01",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:01:05", 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some("2026-07-11 10:01:00"),
            "새 보고는 전진"
        );
    }

    #[test]
    fn sync_presence_no_report_preserves_existing_star() {
        let db = SqliteStore::open_memory().unwrap();
        // 훅으로 기록된 ★(claude) 후 스캐너가 보고값 없이(None) upsert해도 보존(max-merge).
        db.sync_presence(
            "win",
            &[presence("c1", "codex", None)],
            "2026-07-11 10:00:00",
        );
        db.mark_human_input("c1", "2026-07-11 10:00:05");
        db.sync_presence(
            "win",
            &[presence("c1", "codex", None)],
            "2026-07-11 10:00:15",
        );
        let e = &db.list_agents(&BTreeMap::new(), "2026-07-11 10:00:20", 90)[0];
        assert_eq!(
            e.human_input_at.as_deref(),
            Some("2026-07-11 10:00:05"),
            "무보고 upsert가 기존 ★ 보존"
        );
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
        db.register_agent(
            "u2",
            tags(&[("machine", "win"), ("runner", "codex")]),
            None,
            "2026-07-04 10:00:00",
        );
        let now = "2026-07-04 10:00:10";
        let found = db.list_agents(&tags(&[("machine", "win"), ("runner", "claude")]), now, 90);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "u1");
    }

    // --- v2-50: presence 이벤트 이력(presence_events 테이블) ---

    #[test]
    fn sync_presence_logs_appear_and_disappear() {
        let db = SqliteStore::open_memory().unwrap();
        // 1차 스캔: s1, s2 등장.
        db.sync_presence(
            "win",
            &[
                presence("s1", "claude", Some("tunaRound")),
                presence("s2", "codex", None),
            ],
            "2026-07-12 10:00:00",
        );
        // 2차 스캔: s2 사라짐(stale) + s3 새 등장. s1은 연속.
        db.sync_presence(
            "win",
            &[
                presence("s1", "claude", Some("tunaRound")),
                presence("s3", "codex", None),
            ],
            "2026-07-12 10:00:15",
        );
        let events = db.list_presence_events(None, 100).unwrap();
        let appears: Vec<&str> = events
            .iter()
            .filter(|e| e.event_type == "appear")
            .map(|e| e.agent_uuid.as_str())
            .collect();
        assert_eq!(appears.len(), 3, "s1·s2·s3 등장 = 3건");
        let disappears: Vec<&crate::store::agents::PresenceEvent> = events
            .iter()
            .filter(|e| e.event_type == "disappear")
            .collect();
        assert_eq!(disappears.len(), 1);
        assert_eq!(disappears[0].agent_uuid, "s2");
        assert_eq!(disappears[0].detail.as_deref(), Some("stale"));
        assert_eq!(disappears[0].machine.as_deref(), Some("win"));
    }

    #[test]
    fn register_agent_logs_appear_once_and_sync_does_not_duplicate() {
        let db = SqliteStore::open_memory().unwrap();
        // register(워커·infra 경로) 신규 진입 = appear 1회(deregister의 disappear와 대칭).
        db.register_agent(
            "w1",
            tags(&[("machine", "win"), ("role", "worker")]),
            Some("win-worker".into()),
            "2026-07-12 10:00:00",
        );
        let appears1 = db
            .list_presence_events(None, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.event_type == "appear" && e.agent_uuid == "w1")
            .count();
        assert_eq!(appears1, 1, "register 신규 진입 = appear 1회");
        // 같은 uuid를 스캐너가 보고해도(이미 roster에 있음) 중복 appear 없음(전이당 1회).
        db.sync_presence(
            "win",
            &[presence("w1", "worker", None)],
            "2026-07-12 10:00:15",
        );
        // 재등록(재기동)도 이미 존재라 appear 추가 없음.
        db.register_agent(
            "w1",
            tags(&[("machine", "win"), ("role", "worker")]),
            None,
            "2026-07-12 10:00:30",
        );
        let appears2 = db
            .list_presence_events(None, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.event_type == "appear" && e.agent_uuid == "w1")
            .count();
        assert_eq!(
            appears2, 1,
            "register→sync 재보고·재등록에도 appear는 전이당 1회"
        );
    }

    #[test]
    fn register_after_deregister_relogs_appear() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "w1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-12 10:00:00",
        );
        assert!(db.deregister_agent("w1")); // 부재로 전이(disappear 로깅).
        // 다시 등장 = 부재→존재 전이라 appear 재로깅(대칭: disappear 후 재appear).
        db.register_agent(
            "w1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-12 10:05:00",
        );
        let events = db.list_presence_events(None, 100).unwrap();
        let appears = events
            .iter()
            .filter(|e| e.event_type == "appear" && e.agent_uuid == "w1")
            .count();
        let disappears = events
            .iter()
            .filter(|e| e.event_type == "disappear" && e.agent_uuid == "w1")
            .count();
        assert_eq!(appears, 2, "부재→존재 전이가 두 번이면 appear도 2회");
        assert_eq!(disappears, 1, "deregister 1회 = disappear 1회");
    }

    #[test]
    fn sync_presence_does_not_relog_appear_for_continuing_session() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence(
            "win",
            &[presence("s1", "claude", None)],
            "2026-07-12 10:00:00",
        );
        db.sync_presence(
            "win",
            &[presence("s1", "claude", None)],
            "2026-07-12 10:00:15",
        );
        db.sync_presence(
            "win",
            &[presence("s1", "claude", None)],
            "2026-07-12 10:00:30",
        );
        let appears = db
            .list_presence_events(None, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.event_type == "appear")
            .count();
        assert_eq!(appears, 1, "연속 세션은 등장 1회만(매 스캔 재로깅 없음)");
    }

    #[test]
    fn deregister_logs_disappear_deregister() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "s1",
            tags(&[("machine", "mac"), ("runner", "claude")]),
            Some("mac-claude".into()),
            "2026-07-12 10:00:00",
        );
        assert!(db.deregister_agent("s1"));
        let events = db.list_presence_events(None, 100).unwrap();
        let d: Vec<&crate::store::agents::PresenceEvent> = events
            .iter()
            .filter(|e| e.event_type == "disappear")
            .collect();
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].agent_uuid, "s1");
        assert_eq!(d[0].detail.as_deref(), Some("deregister"));
        assert_eq!(d[0].machine.as_deref(), Some("mac"));
        assert_eq!(d[0].display_name.as_deref(), Some("mac-claude"));
        // 미등록 deregister는 로스터에 없어 tags 확보 불가 = 이벤트 미기록.
        assert!(!db.deregister_agent("unknown"));
        let d2 = db
            .list_presence_events(None, 100)
            .unwrap()
            .into_iter()
            .filter(|e| e.event_type == "disappear")
            .count();
        assert_eq!(d2, 1, "미등록 deregister는 이벤트 미기록");
    }

    #[test]
    fn mark_human_input_logs_only_on_advance() {
        let db = SqliteStore::open_memory().unwrap();
        db.register_agent(
            "s1",
            tags(&[("machine", "win")]),
            None,
            "2026-07-12 10:00:00",
        );
        // 인메모리 ★ 값을 읽는 헬퍼(로스터 첫 항목). 온라인 판정용 now는 register보다 살짝 뒤로.
        let star = |db: &SqliteStore| -> Option<String> {
            db.list_agents(&BTreeMap::new(), "2026-07-12 10:00:20", 90)[0]
                .human_input_at
                .clone()
        };
        assert!(db.mark_human_input("s1", "2026-07-12 10:00:05")); // 전진(None→10:00:05)
        assert_eq!(
            star(&db).as_deref(),
            Some("2026-07-12 10:00:05"),
            "첫 핑=전진 반영"
        );
        assert!(db.mark_human_input("s1", "2026-07-12 10:00:05")); // 같은 시각 = 스킵
        assert_eq!(
            star(&db).as_deref(),
            Some("2026-07-12 10:00:05"),
            "동일 핑은 인메모리 ★ 불변"
        );
        assert!(db.mark_human_input("s1", "2026-07-12 10:00:03")); // 과거 = 스킵(★ 회귀 방지)
        assert_eq!(
            star(&db).as_deref(),
            Some("2026-07-12 10:00:05"),
            "과거 핑은 인메모리 ★를 되감지 않음(MAJOR 회귀 방지)"
        );
        assert!(db.mark_human_input("s1", "2026-07-12 10:00:10")); // 전진
        assert_eq!(
            star(&db).as_deref(),
            Some("2026-07-12 10:00:10"),
            "새 핑엔 전진"
        );
        let events = db.list_presence_events(None, 100).unwrap();
        let hi: Vec<&crate::store::agents::PresenceEvent> = events
            .iter()
            .filter(|e| e.event_type == "human_input")
            .collect();
        assert_eq!(hi.len(), 2, "전진(2회)에만 기록, 동일/과거 핑은 스킵");
        assert_eq!(hi[0].at, "2026-07-12 10:00:10", "최신순 = 전진 시각 그대로");
        assert_eq!(hi[1].at, "2026-07-12 10:00:05");
        assert_eq!(hi[0].machine.as_deref(), Some("win"));
    }

    #[test]
    fn sync_presence_logs_human_input_on_reported_advance() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-12 10:00:05")],
            "2026-07-12 10:00:10",
        );
        // 같은 보고값 재보고 = 전진 아님(스킵).
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-12 10:00:05")],
            "2026-07-12 10:00:20",
        );
        // 더 새 보고 = 전진.
        db.sync_presence(
            "win",
            &[presence_with_input("c1", "2026-07-12 10:00:30")],
            "2026-07-12 10:00:35",
        );
        let events = db.list_presence_events(None, 100).unwrap();
        let hi = events
            .iter()
            .filter(|e| e.event_type == "human_input")
            .count();
        assert_eq!(hi, 2, "codex 보고값 전진(2회)에만 기록");
        let appears = events.iter().filter(|e| e.event_type == "appear").count();
        assert_eq!(appears, 1, "c1 등장 1회");
    }

    #[test]
    fn list_presence_events_orders_desc_and_filters_since() {
        let db = SqliteStore::open_memory().unwrap();
        db.sync_presence(
            "win",
            &[presence("s1", "claude", None)],
            "2026-07-12 10:00:00",
        );
        db.sync_presence(
            "win",
            &[
                presence("s1", "claude", None),
                presence("s2", "codex", None),
            ],
            "2026-07-12 10:05:00",
        );
        let all = db.list_presence_events(None, 100).unwrap();
        assert_eq!(
            all.first().unwrap().agent_uuid,
            "s2",
            "최신(s2 등장)이 먼저"
        );
        let recent = db
            .list_presence_events(Some("2026-07-12 10:01:00"), 100)
            .unwrap();
        assert_eq!(recent.len(), 1, "since 이후만");
        assert_eq!(recent[0].agent_uuid, "s2");
        let capped = db.list_presence_events(None, 1).unwrap();
        assert_eq!(capped.len(), 1, "limit=1은 최신 1건만");
        assert_eq!(capped[0].agent_uuid, "s2");
    }

    #[test]
    fn gc_presence_events_removes_only_old_rows() {
        let db = SqliteStore::open_memory().unwrap();
        let t = tags(&[("machine", "win")]);
        // 30일보다 훨씬 과거/미래 이벤트를 직접 심는다.
        db.log_presence_event("appear", "old", &t, None, None, "2020-01-01 00:00:00");
        db.log_presence_event("appear", "fresh", &t, None, None, "2099-01-01 00:00:00");
        db.gc_presence_events();
        let rows = db.list_presence_events(None, 100).unwrap();
        let ids: Vec<&str> = rows.iter().map(|e| e.agent_uuid.as_str()).collect();
        assert!(!ids.contains(&"old"), "보존기간 초과 이벤트는 GC");
        assert!(ids.contains(&"fresh"), "신선 이벤트는 보존");
    }
}
