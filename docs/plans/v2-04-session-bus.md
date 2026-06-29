---
title: "tunaRound v2 Plan 04: Redis session_bus 포팅 (격리 토대)"
type: plan
status: in_progress
priority: P1
updated_at: 2026-06-29
owner: shared
summary: 멀티세션(브랜치=세션)의 토대. tunaSalon src/session_bus.rs를 src/session_bus.rs로 포팅(room->session 용어, env TUNAROUND_REDIS_URL). SessionBus trait + RedisBus(6함수 async) + RedisBusHandle. tokio/redis 0.32/futures-util 신규 의존(설계문서 L145 승인). 완전 격리: 기존 동기 코드 무변경. 라이브 Redis 테스트는 #[ignore].
---

# tunaRound v2 Plan 04: Redis session_bus 포팅 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`).
> 결정 출처: docs/design/tunaRound-v1-design_2026-06-29.md L33·L108·L144-145(Redis 멀티세션=git-tree 분기, tunaSalon session_bus 포팅 확정). 아키텍처 재론 금지 - 구현 분해만.

**Goal:** 멀티세션(브랜치=세션)의 런타임 조율 토대를 깐다. tunaSalon의 검증된 Redis session_bus를 포팅하되, 기존 동기 앱을 안 건드리는 격리된 모듈로 둔다.

**Architecture:** 이 모듈은 런타임 조율 레이어(commands/events를 Redis streams로 미러)다. tunaSalon `~/privateProject/tunaSalon/src/session_bus.rs`(300줄)를 거의 그대로 포팅하고, tunaRound 도메인에 맞게 용어만 바꾼다(room->session). **async 경계 결정(구현 디테일):** tokio 런타임/async는 이 bus 레이어에만. 동기 코어(러너/오케스트레이터/REPL)는 무변경. REPL 통합 시 block_on 브리지는 Plan 06.

**Tech Stack:** Rust 2024. 신규 의존성(설계문서 L145 승인): `tokio` 1(rt-multi-thread/macros/sync/time), `redis` 0.32(default-features=false, tokio-comp/streams), `futures-util` 0.3. 선행: v2 Plan 01~03 done.

> 규율: #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, TDD. tunaSalon의 비-Redis 테스트 2개는 그대로 포팅(라이브 Redis 불필요), Redis 왕복은 #[ignore].

---

## 범위

- **포함:** Cargo 의존성 3개 추가 + `src/session_bus.rs` 포팅(SessionBus trait, RedisBus 6+함수, RedisBusHandle, RedisSessionKeys, RedisStreamMessage) + `src/lib.rs` 모듈 선언 + 테스트(pure 2개 + 라이브 Redis 왕복 #[ignore]).
- **비포함(후속 Plan):** 세션 모델/브랜치=세션 매핑(Plan 05), REPL 통합·presence/snapshot 신규 구현(Plan 06), block_on 브리지(Plan 06). 이 Plan은 bus 모듈만, 어디서도 호출 안 함(격리).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) tokio/redis/futures-util 의존성 추가. |
| `src/session_bus.rs` | (신규) tunaSalon session_bus 포팅. SessionBus + RedisBus + RedisBusHandle + 키/메시지 타입. |
| `src/lib.rs` | (수정) `pub mod session_bus;`. |

> 선제 설계: 격리 모듈(기존 코드 미접촉). SessionBus는 trait 경계라 후속에서 in-process/test double 대체 가능. async는 이 레이어에 가둔다.

---

### Task 1: 의존성 + session_bus 포팅

**Files:**
- Modify: `Cargo.toml`, `src/lib.rs`
- Create: `src/session_bus.rs`

- [ ] **Step 1: Cargo 의존성 추가 (`[dependencies]`)**
```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "sync", "time"] }
futures-util = "0.3"
redis = { version = "0.32", default-features = false, features = ["tokio-comp", "streams"] }
```
  - `cargo build`로 의존성 해석 확인(첫 빌드는 크레이트 다수 받음, 정상).

- [ ] **Step 2: 포팅 (`src/session_bus.rs`)** — 출처 `~/privateProject/tunaSalon/src/session_bus.rs`를 읽어 그대로 옮기되 아래 적응을 적용한다:
  - **헤더(#6):** 첫 줄 `// Redis 기반 세션 버스 프리미티브(멀티세션 런타임 조율). memory.db 대체 아님, hot 미러.`
  - **용어 room->session:**
    - `RedisRoomKeys` -> `RedisSessionKeys`(메서드 `new`/`keys` 동일), 키 prefix `room:{id}` -> `session:{id}`.
    - 함수 파라미터 `room_id` -> `session_id`(시그니처/본문 일괄).
    - `RedisBusMessage`/`RedisBusHandle`의 room 필드도 session으로.
  - **env:** `SALON_REDIS_URL` -> `TUNAROUND_REDIS_URL`.
  - **로그:** `eprintln!("[tunaSalon] ...")` -> `[tunaRound]`.
  - **나머지(로직)는 그대로:** `SessionBus` trait(submit_command_json/publish_event_json), `RedisBus`(open/open_from_env/with_limits/keys/submit_command/publish_event/read_commands/command_cursor/mark_command_consumed/subscribe_events/try_acquire_owner/refresh_owner), `RedisStreamMessage`, `RedisBusHandle`(spawn/spawn_from_env + SessionBus impl), 상수 DEFAULT_*.
  - import 그대로: `futures_util::StreamExt`, `redis::streams::{StreamReadOptions, StreamReadReply}`, `redis::AsyncCommands`, `tokio::sync::mpsc`.
  - `src/lib.rs`에 `pub mod session_bus;` 추가.

- [ ] **Step 3: pure 테스트 포팅 (`mod tests`)** — 라이브 Redis 불필요:
```rust
    #[test]
    fn session_keys_are_stable() {
        let keys = RedisSessionKeys::new("debate-alpha");
        assert_eq!(keys.owner, "session:debate-alpha:owner");
        assert_eq!(keys.commands, "session:debate-alpha:cmd");
        assert_eq!(keys.command_cursor, "session:debate-alpha:cmd:cursor");
        assert_eq!(keys.events, "session:debate-alpha:events");
        assert_eq!(keys.event_channel, "session:debate-alpha:events:pubsub");
        assert_eq!(keys.presence, "session:debate-alpha:presence");
        assert_eq!(keys.hot_snapshot, "session:debate-alpha:hot_snapshot");
    }

    #[test]
    fn empty_env_disables_handle() {
        unsafe { std::env::set_var("TUNAROUND_REDIS_URL", ""); }
        assert!(RedisBusHandle::spawn_from_env().is_none());
        unsafe { std::env::remove_var("TUNAROUND_REDIS_URL"); }
    }
```
  - 주의: edition 2024는 `std::env::set_var`/`remove_var`가 `unsafe`. 위처럼 `unsafe {}`로 감싼다(clippy/컴파일 깨지면 이 방식). 안전 단정 주석 1줄 권장.

- [ ] **Step 4: 검증 + 커밋**
  - `cargo build` 경고 0. `cargo clippy --all-targets` 경고 0. `cargo test` 전체 GREEN(기존 52 + pure 2). 라이브 Redis 테스트는 아직 없음.
  - `git add Cargo.toml Cargo.lock src/session_bus.rs src/lib.rs && git commit -m "feat(session-bus): tunaSalon Redis session_bus 포팅 (격리 토대)"` (push 금지).

---

### Task 2: 라이브 Redis 왕복 통합 테스트 (#[ignore])

**Files:**
- Modify: `src/session_bus.rs` (mod tests)

라이브 Redis가 있을 때만 수동 실행(`TUNAROUND_REDIS_URL=redis://127.0.0.1/ cargo test -- --ignored`). 평소 `cargo test`에서는 스킵.

- [ ] **Step 1: 왕복 테스트 추가**
```rust
    // 라이브 Redis 필요: TUNAROUND_REDIS_URL 설정 후 `cargo test -- --ignored`.
    #[tokio::test]
    #[ignore]
    async fn command_roundtrip_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-roundtrip";
        let id = bus.submit_command(sid, "{\"cmd\":\"hi\"}").await.expect("submit");
        assert!(!id.is_empty());
        let msgs = bus.read_commands(sid, "0", 100, 10).await.expect("read");
        assert!(msgs.iter().any(|m| m.payload.contains("hi")));
    }

    #[tokio::test]
    #[ignore]
    async fn owner_acquire_then_refresh_live() {
        let url = std::env::var("TUNAROUND_REDIS_URL").expect("set TUNAROUND_REDIS_URL");
        let bus = RedisBus::open(&url).expect("open");
        let sid = "test-owner";
        let _ = bus.try_acquire_owner(sid, "w1", 30).await.expect("acquire");
        // 같은 worker는 refresh 성공
        assert!(bus.refresh_owner(sid, "w1", 30).await.expect("refresh"));
        // 다른 worker는 refresh 실패
        assert!(!bus.refresh_owner(sid, "w2", 30).await.expect("refresh2"));
    }
```
  - 주의: `command_roundtrip_live`는 이전 잔여 데이터로 플레이키할 수 있음. last_id "0"으로 전체 읽기라 보통 OK. 불안정하면 고유 sid(타임스탬프 인자)로. `owner_*`는 키 잔존 영향 받을 수 있어 acquire 결과는 단정 안 함(refresh 동작만 검증).

- [ ] **Step 2: 검증 + 커밋**
  - `cargo test` 전체 GREEN(ignored 2개는 스킵됨). `cargo build`/`clippy` 클린.
  - (선택) 로컬 Redis 있으면 `TUNAROUND_REDIS_URL=redis://127.0.0.1/ cargo test -- --ignored`로 왕복 확인.
  - `git add src/session_bus.rs && git commit -m "test(session-bus): 라이브 Redis 왕복 통합 테스트 (#[ignore])"` (push 금지).

---

## Self-Review (작성자 체크)

- **결정 준수:** Redis session_bus 포팅(설계문서 확정)을 구현. 아키텍처 재론 없음. room->session 용어/env만 적응.
- **placeholder:** 없음. 포팅 출처 명시(tunaSalon session_bus.rs).
- **격리:** 신규 모듈, 기존 동기 코드 미접촉. SessionBus trait 경계 유지(후속 대체 가능). async는 bus 레이어에 가둠.
- **타입 일관성:** 의존성 feature는 tunaSalon과 동일(redis 0.32 tokio-comp/streams). lib.rs 모듈 추가. main.rs 무변경(런타임 미도입).
- **테스트:** pure 2개(평소 green) + 라이브 Redis 왕복 2개(#[ignore], 수동). CI/일반 `cargo test`는 Redis 없이 green 유지.

## 위험 / 한계 (문서화된 후속)

- **첫 빌드 시간 증가:** tokio/redis/futures 다수 크레이트. 1회성, 정상.
- **미배선:** 이 Plan은 bus 모듈만. 실제 멀티세션 동작은 Plan 05(세션 모델)·Plan 06(REPL 통합+presence/snapshot 신규). 그 전까지 사용자 가시 변화 없음(토대).
- **env set_var unsafe(edition 2024):** 테스트에서 unsafe 블록 필요. 단일 스레드 테스트라 안전.
- **presence/snapshot 키는 정의만:** RedisSessionKeys에 presence/hot_snapshot 키는 있으나 채우는 로직은 Plan 06(tunaSalon도 미구현).