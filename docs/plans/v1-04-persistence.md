---
title: "tunaRound v1 Plan 04: 전사 영속 (tree-ready) + resume"
type: plan
status: done
priority: P1
updated_at: 2026-06-29
owner: shared
summary: 전사를 트리-ready 메시지(id/parent)로 JSON 영속. src/store 모듈(StoredMessage serde + to_stored/from_stored 선형 트리 + save/load) + Session.save_state/resume + main의 상태파일 CLI 인자(시작 시 resume, 종료 시 저장). SQLite는 멀티세션 필요 시점(v2).
---

# tunaRound v1 Plan 04: 전사 영속 (tree-ready) + resume Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** 토론 전사를 트리-ready 메시지(id/parent)로 파일에 저장하고, 다음 실행에서 이어받을 수 있게 한다.

**Architecture:** 새 `store` 모듈은 직렬화 형식(`StoredMessage{id,parent_id,speaker,content}`)과 변환(전사 <-> stored)·JSON save/load만 담당하는 경계다. v1은 선형 체인(parent = 직전 id)으로 쓰되 parent 포인터가 있어 v2 분기에 rewrite 없이 확장된다. REPL `Session`은 `save_state`/`resume`로 store를 쓰고, `main`이 상태파일 경로(CLI 인자)로 시작 시 resume·종료 시 저장한다.

**Tech Stack:** Rust 2024, `serde`/`serde_json`(이미 의존). v1 영속 = JSON 파일. SQLite/rusqlite는 멀티세션·쿼리가 필요한 v2(Redis 동반)로.

> 규율: docs/reference/development-guidelines.md. 설계 §3(전사·영속, 트리-ready), §7(트리-ready 스키마 리스크). 선행: Plan 01/02/03/05(done).

---

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v1-store` -> main). 전체 33 테스트 green, `cargo build`/`clippy` 클린. **resume 스모크 확인**: `cargo run -- state.json` 1회차 "세션 저장됨" -> 2회차 "(이어받음)".

- StoredMessage(id/parent)로 트리-ready(§7 리스크 해소), v1은 선형 체인. SQLite 대신 JSON(YAGNI, SQLite는 v2 멀티세션).
- Session.save_state/resume + main 상태파일 인자(시작 resume, 종료 save). 작성 시 main 의사코드의 깨진 블록(move 충돌)을 실행 전 정리.
- 커밋: 21dbfc5 -> a5456fd -> 1cc75bf.

## 범위

- **포함:** `src/store/mod.rs` - `StoredMessage`(serde) + `to_stored`/`from_stored`(트리-ready 선형) + `save`/`load`(JSON). `Session::save_state`/`Session::resume`. `main`의 상태파일 인자(resume on start, save on exit).
- **비포함(후속):** SQLite, 다중 브랜치(분기) 실제 사용, 멀티세션, 쿼리/인덱스 → v2(Redis). 자동 저장 타이밍 정교화(매 턴 저장 등) → 후속.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/lib.rs` | (수정) `pub mod store;` |
| `src/store/mod.rs` | (신규) `StoredMessage` + 변환 + JSON save/load |
| `src/repl/mod.rs` | (수정) `Session::save_state`/`Session::resume` |
| `src/main.rs` | (수정) 상태파일 CLI 인자: 시작 resume, 종료 save |

> 선제 설계: 직렬화 타입을 처음부터 id/parent로(트리-ready), 변환은 순수함수, I/O(파일)는 save/load·main에 격리.

---

### Task 1: store 타입 + 트리-ready 변환 (순수)

**Files:**
- Modify: `src/lib.rs` (`pub mod store;`)
- Create: `src/store/mod.rs`

- [ ] **Step 1: lib.rs + 실패 테스트**
`src/lib.rs`에 `pub mod store;` 추가.
`src/store/mod.rs` 생성, 첫 줄 `// 전사 영속의 직렬화 형식과 변환. 트리-ready(id/parent), v1은 선형 체인.`
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::Utterance;

    fn utts() -> Vec<Utterance> {
        vec![
            Utterance { speaker: "claude/proposer".into(), content: "제안".into() },
            Utterance { speaker: "codex/reviewer".into(), content: "리뷰".into() },
        ]
    }

    #[test]
    fn to_stored_assigns_linear_ids_and_parents() {
        let s = to_stored(&utts());
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].id, 1);
        assert_eq!(s[0].parent_id, None);
        assert_eq!(s[1].id, 2);
        assert_eq!(s[1].parent_id, Some(1)); // 트리-ready: 직전이 parent
        assert_eq!(s[1].speaker, "codex/reviewer");
    }

    #[test]
    fn roundtrip_stored_to_transcript() {
        let original = utts();
        let back = from_stored(&to_stored(&original));
        assert_eq!(back, original);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib store::tests` → FAIL.

- [ ] **Step 3: 구현 (`src/store/mod.rs`, 테스트 위)**
```rust
use serde::{Deserialize, Serialize};

use crate::orchestrator::Utterance;

/// 영속 메시지. 트리-ready: parent_id로 체인/분기 표현(v1은 선형, parent=직전).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredMessage {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub speaker: String,
    pub content: String,
}

/// 전사를 stored로. id는 1부터, parent는 직전 id(첫 메시지는 None).
pub fn to_stored(transcript: &[Utterance]) -> Vec<StoredMessage> {
    let mut out = Vec::with_capacity(transcript.len());
    let mut prev: Option<u64> = None;
    for (i, u) in transcript.iter().enumerate() {
        let id = (i as u64) + 1;
        out.push(StoredMessage {
            id,
            parent_id: prev,
            speaker: u.speaker.clone(),
            content: u.content.clone(),
        });
        prev = Some(id);
    }
    out
}

/// stored를 전사로(메타 버리고 speaker/content만). v1 선형 가정.
pub fn from_stored(messages: &[StoredMessage]) -> Vec<Utterance> {
    messages
        .iter()
        .map(|m| Utterance { speaker: m.speaker.clone(), content: m.content.clone() })
        .collect()
}
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib store::tests` PASS(2개).
`git add src/lib.rs src/store/mod.rs && git commit -m "feat(store): 트리-ready 메시지 + 전사 변환"` (push 금지).

> Utterance가 PartialEq라야 roundtrip 단언이 컴파일됨 - Plan 03에서 이미 derive됨(확인). 아니면 보고.

---

### Task 2: JSON save/load (파일 라운드트립)

**Files:**
- Modify: `src/store/mod.rs`
- Create: `tests/store_roundtrip.rs`

- [ ] **Step 1: 실패 통합테스트 (`tests/store_roundtrip.rs`)**
```rust
// store가 stored 메시지를 JSON 파일로 저장/로드 라운드트립하는지 검증.
use tunaround::orchestrator::Utterance;
use tunaround::store::{from_stored, load, save, to_stored};

#[test]
fn save_then_load_roundtrips() {
    let transcript = vec![
        Utterance { speaker: "claude/proposer".into(), content: "제안".into() },
        Utterance { speaker: "codex/reviewer".into(), content: "리뷰".into() },
    ];
    let stored = to_stored(&transcript);

    let dir = std::env::temp_dir();
    let path = dir.join(format!("tunaround_store_test_{}.json", std::process::id()));
    let path = path.to_str().unwrap();

    save(&stored, path).expect("save ok");
    let loaded = load(path).expect("load ok");
    assert_eq!(loaded, stored);
    assert_eq!(from_stored(&loaded), transcript);

    let _ = std::fs::remove_file(path);
}
```

- [ ] **Step 2: 실패 확인** — `cargo test --test store_roundtrip` → FAIL(`save`/`load` 미정의).

- [ ] **Step 3: 구현 (`src/store/mod.rs`)**
```rust
/// stored 메시지를 JSON 파일로 저장.
pub fn save(messages: &[StoredMessage], path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(messages)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// JSON 파일에서 stored 메시지를 로드.
pub fn load(path: &str) -> std::io::Result<Vec<StoredMessage>> {
    let s = std::fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test --test store_roundtrip` PASS.
`git add src/store/mod.rs tests/store_roundtrip.rs && git commit -m "feat(store): JSON save/load 라운드트립"` (push 금지).

---

### Task 3: Session resume + main 상태파일 인자

**Files:**
- Modify: `src/repl/mod.rs` (`Session::save_state`/`resume`)
- Modify: `src/main.rs` (상태파일 인자)

- [ ] **Step 1: 실패 통합테스트 (`tests/store_roundtrip.rs`에 추가)**
```rust
use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::repl::Session;

#[test]
fn session_save_state_then_resume() {
    // 전사를 가진 세션을 만들기 위해 resume 경로로 구성한다.
    let transcript = vec![
        Utterance { speaker: "claude/proposer".into(), content: "이전 결론".into() },
    ];
    let dir = std::env::temp_dir();
    let path = dir.join(format!("tunaround_session_test_{}.json", std::process::id()));
    let path = path.to_str().unwrap();
    save(&to_stored(&transcript), path).expect("seed save");

    let participants = vec![
        Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
    ];
    let resumed = Session::resume(participants, Box::new(MapRegistry::new()), path).expect("resume ok");
    assert_eq!(resumed.transcript_len(), 1);

    // 다시 저장 후 로드해도 1개 유지
    resumed.save_state(path).expect("save_state ok");
    assert_eq!(load(path).expect("reload").len(), 1);

    let _ = std::fs::remove_file(path);
}
```

- [ ] **Step 2: 실패 확인** — `cargo test --test store_roundtrip` → FAIL(`Session::resume`/`save_state` 미정의).

- [ ] **Step 3: 구현 (`src/repl/mod.rs`의 `impl Session`에 추가)**
```rust
    /// 현재 전사를 상태 파일(JSON)로 저장한다.
    pub fn save_state(&self, path: &str) -> std::io::Result<()> {
        crate::store::save(&crate::store::to_stored(&self.transcript), path)
    }

    /// 상태 파일에서 전사를 로드해 세션을 복원한다.
    pub fn resume(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        path: &str,
    ) -> std::io::Result<Self> {
        let messages = crate::store::load(path)?;
        Ok(Self {
            participants,
            transcript: crate::store::from_stored(&messages),
            registry,
        })
    }
```

- [ ] **Step 4: 통과 확인** — `cargo test --test store_roundtrip` PASS(2개).

- [ ] **Step 5: main 상태파일 인자 (`src/main.rs`)**
`main` 안에서 participants/registry 구성 후, Session 생성을 상태파일 인자 기반으로 바꾼다. resume 실패 시 move 충돌을 피하려고, 파일 존재 여부를 먼저 확인한 뒤 한 경로에서만 participants/registry를 소비한다.
```rust
    let state_path: Option<String> = std::env::args().nth(1);
    let resume_existing = state_path.as_deref().map(|p| std::path::Path::new(p).exists()).unwrap_or(false);
    let mut session = if resume_existing {
        let p = state_path.as_deref().unwrap();
        match Session::resume(participants, Box::new(registry), p) {
            Ok(s) => { println!("(이어받음: {p})"); s }
            Err(e) => { eprintln!("[resume 실패: {e}] 새 세션으로 시작합니다."); std::process::exit(1); }
        }
    } else {
        Session::new(participants, Box::new(registry))
    };
```
그리고 stdin 루프가 끝난 뒤(`break` 이후)에:
```rust
    if let Some(p) = &state_path {
        match session.save_state(p) {
            Ok(()) => println!("세션 저장됨: {p}"),
            Err(e) => println!("[세션 저장 실패] {e}"),
        }
    }
```
import에 `use tunaround::repl::Session;`는 이미 있음(Plan 05). `MapRegistry`도 이미 import됨.

- [ ] **Step 6: 전체 검증 + 커밋**
- `cargo test`(전체) 모두 통과.
- `cargo build` 경고 0, `cargo clippy --all-targets` 클린.
- (선택) 비대화형 스모크: `printf '/quit\n' | cargo run -q -- /tmp/tr_state.json` 후 `cargo run -q -- /tmp/tr_state.json` 재실행 시 "(이어받음...)" 출력 확인(Message 없으니 실 CLI 미호출).
- `git add src/repl/mod.rs src/main.rs && git commit -m "feat(store): Session resume + main 상태파일 인자"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** 전사 영속(§3) + 트리-ready 메시지 id/parent(§7 리스크 해소: parent 포인터로 v2 분기 확장 대비) + resume. SQLite·다중 브랜치·멀티세션은 명시적 후속(v2).
- **placeholder:** 없음. Task 3 Step 5는 의사코드의 move 충돌을 명시적으로 교정한 실코드 제공(추측 금지).
- **타입 일관성:** StoredMessage/to_stored/from_stored/save/load를 store에서 정의, Session.save_state/resume·main에서 동일 사용. Utterance/Participant/MapRegistry/Session 재사용.
- **선제 설계:** 직렬화 타입 처음부터 id/parent, 변환 순수함수, I/O 격리, JSON으로 시작(SQLite는 YAGNI까지 보류).

## 다음 (v1 마무리 후)

- **Hardening:** 양 러너 idle watchdog(INV-4), consensus 합성(`/conclude`), 자리 지목(`@engine`)·에이전트 쓰기 지목(RunMode::Write).
- **v2:** Redis 멀티세션 = git-tree 다중 브랜치(StoredMessage.parent_id가 기반), N좌석 로스터, ratatui/web.
