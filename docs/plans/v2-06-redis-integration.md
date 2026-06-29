---
title: "tunaRound v2 Plan 06: Redis 멀티세션 통합 (미러 + observe + resume)"
type: plan
status: done
priority: P1
updated_at: 2026-06-29
owner: shared
summary: session_bus(Plan 04)·트리모델(Plan 05) 위에 멀티프로세스 동시 세션을 얹는다. driver가 매 라운드를 Redis로 미러(publish_event + hot_snapshot, owner lease 유지), 둘째 프로세스가 --observe <id>로 라이브 읽기전용 추종, --session <id>로 Redis snapshot에서 라이브 재개. write path는 sync(fire-and-forget), read path(observe/resume)만 tokio block_on. 자동 테스트=fake bus write-path + 파싱, 라이브=수동/#[ignore].
---

# tunaRound v2 Plan 06: Redis 멀티세션 통합 Implementation Plan

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v2-redis-integration` -> main). 66 테스트(63 pass + 3 ignored 라이브 Redis), `cargo build`/`clippy` 경고 0. Redis 없이 `cargo run` /quit 정상(미러 no-op, bus=None 동작 불변 확인). Opus 리뷰: write path sync/payload 재사용 정확, main 중복 핸들 정리.

- Task 1: session_bus snapshot(set/get + snapshot_json fire-and-forget + Snapshot 메시지) (커밋 `e72c867`).
- Task 2: Session 미러(Option<bus>+session_id, append_round 후 event+snapshot 발행, new_with_bus/seed_from). FakeBus 단위테스트 (커밋 `c46121c`).
- Task 3: main.rs tokio 런타임 + `--observe <id>`(라이브 구독 루프) + `--session <id>`(snapshot seed + owner lease + refresh 태스크) (커밋 `eb470b8`).
- 리뷰 정리: `--session` 재개 시 중복 RedisBusHandle spawn 제거(bus_boxed 재사용) (커밋 `389fe09`).
- **검증 한계(정직):** observe/resume 라이브는 라이브 Redis + 2 터미널 필요라 자동검증 불가. 코드 경로는 컴파일·타입 OK, 자동 테스트는 write-path(FakeBus)+파싱만. 실제 라이브 동작은 사람이 2 터미널로 1회 확인 필요.

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`).
> 결정: 사용자 확정 "둘 다 한 플랜"(미러 + observe + resume). 설계문서 L108(동시 세션/관찰/재개)·L146(presence/snapshot 신규). 아키텍처 재론 금지.

**Goal:** 멀티세션의 마지막 조각. 한 프로세스(driver)의 토론을 Redis로 미러해, 다른 프로세스가 라이브로 관찰(`--observe`)하거나 이어받기(`--session`)할 수 있게 한다.

**Architecture:**
- **write path는 sync.** `SessionBus`는 fire-and-forget(mpsc -> 백그라운드 tokio 태스크)라 REPL이 동기 호출한다. 매 라운드 후 Session이 `publish_event_json`(이번 라운드 새 메시지) + `snapshot_json`(전체 트리)를 보낸다. block_on 불필요.
- **read path만 async.** `--observe`/`--session`은 main이 tokio 런타임에서 block_on으로 일회성 GET(snapshot) + subscribe 루프를 돈다.
- **payload는 기존 store 타입 재사용:** snapshot = `StoredSession{messages,head}` JSON, event = 이번 라운드 `Vec<StoredMessage>` JSON. Utterance에 Serialize 추가 불필요.
- **owner lease:** driver가 `try_acquire_owner`(worker_id=process id) + 백그라운드 `refresh_owner`. 이미 owner 있으면 경고(이중 driver 방지, 강제 차단은 아님).
- **격리/무영향:** Redis 미설정(TUNAROUND_REDIS_URL 없음)이면 bus=None, 기존 단일 프로세스 동작 그대로(미러 no-op).

**Tech Stack:** Rust 2024. 신규 의존성 0(tokio/redis/futures는 Plan 04에서 추가됨). 선행: v2 Plan 04·05 done.

> 규율: #5 한국어 마침표, TDD(write path는 fake bus 단위테스트). **검증 한계 명시:** observe/resume 라이브는 라이브 Redis + 2 프로세스 필요 -> 수동/#[ignore]. 자동 테스트가 "라이브 동작함"을 보장하지 않는다.

---

## 범위

- **포함:** session_bus에 snapshot(fire-and-forget `snapshot_json` + `RedisBus::set_snapshot`/`get_snapshot`) + Session 미러 통합(Option<bus> + session_id, append_round 미러) + main.rs(tokio 런타임 + `--observe <id>` 관찰 모드 + `--session <id>` Redis 재개 + owner lease/refresh).
- **비포함(후속):** 분기별 session_id(현재는 세션=프로세스 1개 session_id, snapshot=전체 트리), 강제 owner 차단(현재 경고만), web 게이트웨이, presence UI(owner lease가 최소 presence). 분기 단위 관찰은 후속.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/session_bus.rs` | (수정) `SessionBus`에 `snapshot_json` + RedisBusMessage::Snapshot + `RedisBus::set_snapshot`/`get_snapshot`. |
| `src/repl/mod.rs` | (수정) Session에 `bus: Option<Box<dyn SessionBus>>` + `session_id`. append_round 후 미러. 생성자 `new_with_bus`. FakeBus 테스트. |
| `src/main.rs` | (수정) tokio 런타임 + 인자 `--observe <id>`/`--session <id>` + 관찰 모드(block_on subscribe) + 재개(snapshot seed + owner lease + refresh 태스크) + REPL 미러 배선. |

> 선제 설계: write path sync(fire-and-forget)로 REPL/Session은 async 무지. read path만 main이 block_on. payload는 store 타입 재사용. bus=None이면 기존 동작 불변.

---

### Task 1: session_bus snapshot 지원

**Files:**
- Modify: `src/session_bus.rs`

- [ ] **Step 1: `RedisBus`에 snapshot set/get 추가**
```rust
    /// hot_snapshot 키에 세션 상태(JSON)를 저장한다.
    pub async fn set_snapshot(&self, session_id: &str, payload: &str) -> redis::RedisResult<()> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.set(keys.hot_snapshot, payload).await
    }

    /// hot_snapshot 키에서 세션 상태(JSON)를 읽는다.
    pub async fn get_snapshot(&self, session_id: &str) -> redis::RedisResult<Option<String>> {
        let mut con = self.client.get_multiplexed_async_connection().await?;
        let keys = Self::keys(session_id);
        con.get(keys.hot_snapshot).await
    }
```

- [ ] **Step 2: 트레이트 + 핸들에 snapshot fire-and-forget 추가**
  - `SessionBus` trait에 메서드 추가: `fn snapshot_json(&self, session_id: &str, payload: &str);`
  - `enum RedisBusMessage`에 변형 추가: `Snapshot { session_id: String, payload: String },`
  - `RedisBusHandle::spawn`의 match에 추가:
```rust
                    RedisBusMessage::Snapshot { session_id, payload } => {
                        bus.set_snapshot(&session_id, &payload).await.map(|_| ())
                    }
```
  - `impl SessionBus for RedisBusHandle`에 추가:
```rust
    fn snapshot_json(&self, session_id: &str, payload: &str) {
        let _ = self.tx.send(RedisBusMessage::Snapshot {
            session_id: session_id.to_string(),
            payload: payload.to_string(),
        });
    }
```

- [ ] **Step 3: 라이브 snapshot 왕복 테스트 (#[ignore])**
```rust
    #[tokio::test]
    #[ignore]
    async fn snapshot_set_get_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-snapshot";
        bus.set_snapshot(sid, "{\"messages\":[],\"head\":null}").await.expect("set");
        let got = bus.get_snapshot(sid).await.expect("get");
        assert_eq!(got.as_deref(), Some("{\"messages\":[],\"head\":null}"));
    }
```

- [ ] **Step 4: 검증 + 커밋** — `cargo test`(전체 green, 신규 ignored 스킵), build/clippy 클린.
  `git add src/session_bus.rs && git commit -m "feat(session-bus): hot_snapshot set/get + snapshot_json fire-and-forget"` (push 금지).

---

### Task 2: Session 미러 통합

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저 (FakeBus가 호출을 기록) (`mod tests`)**
```rust
    use std::cell::RefCell;
    use std::rc::Rc;

    #[derive(Default)]
    struct BusCalls { events: usize, snapshots: usize, last_session: String }
    struct FakeBus(Rc<RefCell<BusCalls>>);
    impl crate::session_bus::SessionBus for FakeBus {
        fn submit_command_json(&self, _s: &str, _p: &str) {}
        fn publish_event_json(&self, s: &str, _p: &str) {
            let mut c = self.0.borrow_mut(); c.events += 1; c.last_session = s.to_string();
        }
        fn snapshot_json(&self, _s: &str, _p: &str) { self.0.borrow_mut().snapshots += 1; }
    }

    #[test]
    fn round_mirrors_event_and_snapshot_when_bus_present() {
        let calls = Rc::new(RefCell::new(BusCalls::default()));
        let mut reg = MapRegistry::new();
        reg.insert("claude", Box::new(FakeRunner { reply: "제안".into() }));
        let participants = vec![Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() }];
        let mut s = Session::new_with_bus(participants, Box::new(reg), "sess-1".into(), Some(Box::new(FakeBus(Rc::clone(&calls)))));
        let _ = s.step(Command::Message("주제".into()));
        let c = calls.borrow();
        assert_eq!(c.events, 1);      // 라운드 1회 -> 이벤트 1
        assert_eq!(c.snapshots, 1);   // 스냅샷 1
        assert_eq!(c.last_session, "sess-1");
    }

    #[test]
    fn no_bus_means_no_mirror_and_normal_behavior() {
        let mut s = session_with_two_seats(); // bus 없음
        let _ = s.step(Command::Message("주제".into()));
        assert_eq!(s.transcript_len(), 2); // 기존 동작 불변
    }
```
  - 주의: `SessionBus`는 `&self`만 받으므로 `Box<dyn SessionBus>`는 Send 불필요(REPL은 단일 스레드). RefCell/Rc 테스트 OK. 단 Session 필드 타입은 `Option<Box<dyn SessionBus>>`.

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl` -> FAIL(new_with_bus 미존재).

- [ ] **Step 3: 구현**
  - `use crate::session_bus::SessionBus;` 추가.
  - Session 필드 추가:
```rust
pub struct Session {
    participants: Vec<Participant>,
    messages: Vec<StoredMessage>,
    head: Option<u64>,
    registry: Box<dyn RunnerRegistry>,
    bus: Option<Box<dyn SessionBus>>,
    session_id: String,
}
```
  - `new`: `bus: None, session_id: "default".to_string()`(기존 시그니처 유지, 내부 기본값). resume도 동일 기본값.
  - 새 생성자:
```rust
    pub fn new_with_bus(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        session_id: String,
        bus: Option<Box<dyn SessionBus>>,
    ) -> Self {
        Self { participants, messages: Vec::new(), head: None, registry, bus, session_id }
    }
```
  - `append_round` 끝에 미러 추가(append 후 새 메시지 + 트리 스냅샷):
```rust
    fn append_round(&mut self, round: &[Utterance]) {
        let start = self.messages.len();
        for u in round {
            let id = crate::store::next_id(&self.messages);
            self.messages.push(StoredMessage { id, parent_id: self.head, speaker: u.speaker.clone(), content: u.content.clone() });
            self.head = Some(id);
        }
        if let Some(bus) = &self.bus {
            let new_msgs = &self.messages[start..];
            if let Ok(ev) = serde_json::to_string(new_msgs) {
                bus.publish_event_json(&self.session_id, &ev);
            }
            let snap = StoredSession { messages: self.messages.clone(), head: self.head };
            if let Ok(s) = serde_json::to_string(&snap) {
                bus.snapshot_json(&self.session_id, &s);
            }
        }
    }
```
  - resume에 옵션: main이 Redis snapshot을 seed하려면 messages/head를 직접 주입하는 경로가 필요. `Session::from_snapshot(participants, registry, session_id, bus, ss: StoredSession)` 헬퍼 추가(또는 new_with_bus + 필드 세팅). 간단히:
```rust
    pub fn seed_from(&mut self, ss: StoredSession) {
        self.messages = ss.messages;
        self.head = ss.head;
    }
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test`(전체) PASS, build/clippy 클린.
  `git add src/repl/mod.rs && git commit -m "feat(repl): Session Redis 미러(이벤트+스냅샷) + new_with_bus"` (push 금지).

---

### Task 3: main.rs 런타임 + observe/resume

**Files:**
- Modify: `src/main.rs`

> 이 Task는 라이브 Redis가 있어야 완전 검증된다. 자동 테스트 불가 영역(main 바이너리). 검증은 빌드 + 수동 스모크.

- [ ] **Step 1: 인자 파싱 확장** — 기존 `--roster`/positional state에 더해 `--observe <id>`, `--session <id>` 추가(수동 파서에 케이스 추가).

- [ ] **Step 2: tokio 런타임 + 관찰 모드**
  - `let rt = tokio::runtime::Runtime::new().expect("tokio runtime");`
  - `--observe <id>`면 REPL 대신 관찰 루프(block_on):
```rust
    if let Some(sid) = observe_id {
        let Some(bus) = tunaround::session_bus::RedisBus::open_from_env() else {
            eprintln!("[observe] TUNAROUND_REDIS_URL 필요"); std::process::exit(1);
        };
        rt.block_on(async move {
            if let Ok(Some(snap)) = bus.get_snapshot(&sid).await {
                println!("=== 현재 스냅샷 ===\n{snap}\n=== 라이브 ===");
            }
            let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(256);
            let subscriber = { let bus = bus.clone(); let sid = sid.clone();
                tokio::spawn(async move { let _ = bus.subscribe_events(&sid, tx).await; }) };
            while let Ok(payload) = rx.recv().await {
                println!("{payload}");
            }
            let _ = subscriber.await;
        });
        return;
    }
```

- [ ] **Step 3: REPL 모드에 bus 미러 + resume + owner lease**
  - bus 핸들 준비: `let handle = tunaround::session_bus::RedisBusHandle::spawn_from_env();`(런타임 컨텍스트 필요 -> `rt.enter()` 가드 안에서 호출, 또는 rt.block_on 내에서 spawn). 핸들 생성은 `let _g = rt.enter(); RedisBusHandle::spawn_from_env()`.
  - session_id = `--session <id>` 값 또는 "default".
  - resume seed: `--session`이고 Redis snapshot 있으면 block_on get_snapshot -> StoredSession 파싱 -> Session.seed_from. owner lease: `rt.block_on(raw_bus.try_acquire_owner(&sid, &worker_id, 60))`; false면 경고("다른 프로세스가 driver일 수 있음"). worker_id = `std::process::id().to_string()`.
  - owner refresh 백그라운드: `rt.spawn(async move { loop { tokio::time::sleep(30s); refresh_owner(...).await; } })`(driver일 때만).
  - Session::new_with_bus(participants, registry, session_id, handle.map(|h| Box::new(h) as Box<dyn SessionBus>)).
  - 이후 기존 REPL 루프 그대로(이제 미러됨). 종료 시 기존 state save 유지(파일 영속은 병행).

- [ ] **Step 4: 빌드 + 수동 스모크**
  - `cargo build` 경고 0, `cargo clippy --all-targets` 클린, `cargo test` 전체 green(main은 테스트 없음, 기존 영향 없어야).
  - Redis 없이: `printf '/quit\n' | cargo run` -> 평소대로(미러 no-op, bus=None). 정상 종료.
  - (수동, 라이브) 터미널A `TUNAROUND_REDIS_URL=redis://127.0.0.1/ cargo run -- --session demo`, 메시지 한 줄. 터미널B `... cargo run -- --observe demo` -> 스냅샷 + 라이브 이벤트 표시 확인.
  - `git add src/main.rs && git commit -m "feat(main): tokio 런타임 + --observe/--session Redis 멀티세션"` (push 금지).

---

## Self-Review (작성자 체크)

- **결정 준수:** 미러 + observe + resume 전부(사용자 확정 "둘 다 한 플랜"). 설계문서 동시/관찰/재개. 아키텍처 재론 없음.
- **격리/무영향:** bus=None(Redis 미설정)이면 기존 단일 프로세스 동작 불변(미러 no-op). write path sync라 REPL/Session은 async 무지. read path만 main block_on.
- **타입 일관성:** payload = store 타입(StoredSession/Vec<StoredMessage>) 재사용. SessionBus trait에 snapshot_json 추가(fake/handle 둘 다 구현). Session에 Option<bus>+session_id.
- **TDD:** write path(미러)는 FakeBus 단위테스트. snapshot set/get은 #[ignore] 라이브.

## 위험 / 한계 (정직하게)

- **observe/resume 자동 검증 불가:** main 바이너리 + 라이브 Redis + 2 프로세스 필요 -> 수동 스모크/#[ignore]. **자동 테스트 green이 라이브 동작을 보장하지 않는다.** 실제 동작은 사람이 2 터미널로 1회 확인 필요.
- **단일 session_id/프로세스:** 분기별 session_id 아님(snapshot=전체 트리). "브랜치=세션" 세분화는 후속.
- **owner lease 경고만:** 이중 driver를 강제 차단하지 않음(경고). 충돌 시 마지막 쓰기 우선. 강제 read-only 강등은 후속.
- **tokio 런타임 수명/spawn 컨텍스트:** RedisBusHandle::spawn은 런타임 컨텍스트 필요(rt.enter()). refresh 태스크 누수 방지는 프로세스 종료에 위임(데몬 아님).
- **payload 크기:** snapshot=전체 트리 매 라운드. 긴 토론에서 커질 수 있음(MAXLEN은 streams만, snapshot은 SET 덮어쓰기라 1개). 압축/증분은 후속.