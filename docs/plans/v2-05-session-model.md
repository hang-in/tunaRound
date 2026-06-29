---
title: "tunaRound v2 Plan 05: 세션 모델 (브랜치=세션, in-store 논리 트리)"
type: plan
status: done
priority: P1
updated_at: 2026-06-29
owner: shared
summary: 선형 전사를 in-store 논리 트리로. Session이 messages(Vec<StoredMessage>)+head를 들고, 라운드마다 active path(root->head)를 run_round에 넘긴 뒤 결과를 head 분기로 append. /branches(트리 목록)+/checkout <id>(head 이동)로 분기 탐색. parent_id 실사용. run_round 무변경(트리 로직은 store 순수함수+Session 격리). Redis는 Plan 06.
---

# tunaRound v2 Plan 05: 세션 모델 Implementation Plan

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v2-session-model` -> main). 61 테스트(59 pass + 2 ignored 라이브 Redis), `cargo build`/`clippy` 경고 0. Opus 리뷰: 설계 충실, 이중 append 없음 확인(run_round은 임시 path mutate, Session은 반환 round만 트리에 1회 append).

- Task 1: store 트리 순수함수(path_to_root/next_id/tree_summary) + StoredSession + save_session/load_session(레거시 폴백) (커밋 `7ded26d`).
- Task 2: Session 선형 transcript -> 트리(messages+head), active_path/append_round 헬퍼, 4개 라운드 분기 통일, 영속 배선 (커밋 `c9510fe`).
- Task 3: /branches(tree_summary) + /checkout <id>(head 이동) + 파싱 + /help (커밋 `5b25827`).
- 일탈(정당): Branches/Checkout variant를 Task 2에서 선구현(step exhaustive match). integration `store_roundtrip.rs`의 resume 검증을 load_session().messages.len()으로 갱신(save_state가 StoredSession 포맷으로 변경됨, 예고된 위험).

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`). TDD red->green.
> 결정: 사용자 확정 "in-store 논리 트리"(옵션 A). 설계문서 L63·L100·L108(브랜치=세션, 트리-ready, 분기 UI=v2). 아키텍처 재론 금지.

**Goal:** "브랜치=세션"을 단일 프로세스에서 실제로 구현한다. 토론 트리의 임의 지점에서 분기하고(`/checkout`), 트리를 보고(`/branches`), 각 분기를 독립 대화선으로 이어간다. parent_id를 실사용한다(현재는 선형만).

**Architecture:** `Session`이 선형 `transcript: Vec<Utterance>` 대신 **트리**(`messages: Vec<StoredMessage>` + `head: Option<u64>`)를 보유한다. 라운드 실행은: active path(root->head)를 계산해 run_round에 넘기고, 반환된 round를 head에서 시작하는 체인으로 트리에 append하며 head를 이동한다. `run_round`/`Participant`/러너는 **무변경**(트리 로직은 store 순수함수 + Session에 격리). `/checkout <id>`로 head를 임의 노드로 옮기면 그 다음 라운드가 자연히 분기(sibling)를 만든다.

**Tech Stack:** Rust 2024, 신규 의존성 0. 선행: v2 Plan 01~04 done.

> 규율: #5 한국어 마침표, #6 새 파일 없음, TDD(트리 함수는 순수 -> 단위테스트 용이). Redis/멀티프로세스/presence는 Plan 06(이 plan은 단일 프로세스 트리).

---

## 범위

- **포함:** store 트리 순수함수(path_to_root/next_id/트리 요약) + 저장 포맷 `StoredSession{messages, head}`(레거시 bare-array 폴백) + Session 트리 리팩토링(messages+head, 라운드 append-to-tree) + `/branches`·`/checkout <id>` 명령 + /help 갱신.
- **비포함(Plan 06):** Redis 연동(session_id per 분기), presence/snapshot, 멀티프로세스 동시 세션, block_on 브리지. 트리 시각화는 단순 목록(ASCII 아트 트리는 후속).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/store/mod.rs` | (수정) `path_to_root`/`next_id`/`tree_summary` 순수함수 + `StoredSession` + `save_session`/`load_session`(레거시 폴백). 기존 to_stored/from_stored/save/load 유지. |
| `src/repl/mod.rs` | (수정) Session: transcript -> messages+head. 라운드 append-to-tree 헬퍼. `Command::Branches`/`Checkout(u64)` + parse + step 분기 + /help. transcript_len/markdown은 active path 기반. |
| `src/main.rs` | (수정) resume/save가 save_session/load_session 쓰도록(시그니처 변화 흡수). |

> 선제 설계: 트리 로직 순수함수로 추출(테스트 용이). run_round 무변경(트리는 Session 책임). 저장 포맷 변경은 레거시 폴백으로 backward compat.

---

### Task 1: store 트리 순수함수 + 저장 포맷

**Files:**
- Modify: `src/store/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저 (`mod tests`)**
```rust
    #[test]
    fn path_to_root_walks_parents() {
        // 트리: 1 -> 2 -> 3, 그리고 2 -> 4 (분기)
        let msgs = vec![
            StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
            StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
            StoredMessage { id: 3, parent_id: Some(2), speaker: "c".into(), content: "3".into() },
            StoredMessage { id: 4, parent_id: Some(2), speaker: "d".into(), content: "4".into() },
        ];
        let path = path_to_root(&msgs, Some(3));
        assert_eq!(path.iter().map(|u| u.content.clone()).collect::<Vec<_>>(), vec!["1","2","3"]);
        let branch = path_to_root(&msgs, Some(4));
        assert_eq!(branch.iter().map(|u| u.content.clone()).collect::<Vec<_>>(), vec!["1","2","4"]);
        assert!(path_to_root(&msgs, None).is_empty());
    }

    #[test]
    fn next_id_is_max_plus_one() {
        assert_eq!(next_id(&[]), 1);
        let msgs = vec![StoredMessage { id: 5, parent_id: None, speaker: "a".into(), content: "x".into() }];
        assert_eq!(next_id(&msgs), 6);
    }

    #[test]
    fn session_roundtrip_preserves_tree_and_head() {
        let ss = StoredSession {
            messages: vec![
                StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
                StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
            ],
            head: Some(2),
        };
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_session_rt.json");
        save_session(&ss, path.to_str().unwrap()).unwrap();
        let back = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(back.messages, ss.messages);
        assert_eq!(back.head, Some(2));
    }

    #[test]
    fn load_session_falls_back_to_legacy_bare_array() {
        // 레거시 v1: bare [StoredMessage] (head 없음) -> head = 마지막 id
        let legacy = vec![
            StoredMessage { id: 1, parent_id: None, speaker: "a".into(), content: "1".into() },
            StoredMessage { id: 2, parent_id: Some(1), speaker: "b".into(), content: "2".into() },
        ];
        let dir = std::env::temp_dir();
        let path = dir.join("tuna_legacy.json");
        save(&legacy, path.to_str().unwrap()).unwrap(); // 기존 bare-array 저장
        let ss = load_session(path.to_str().unwrap()).unwrap();
        assert_eq!(ss.messages.len(), 2);
        assert_eq!(ss.head, Some(2)); // 마지막 id
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib store` -> FAIL.

- [ ] **Step 3: 구현 (`src/store/mod.rs`)**
```rust
/// 세션 저장 단위: 메시지 트리 + 현재 head(활성 분기 끝).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StoredSession {
    pub messages: Vec<StoredMessage>,
    pub head: Option<u64>,
}

/// head에서 parent_id를 따라 root까지 거슬러 올라간 경로(루트->head 순)를 전사로.
pub fn path_to_root(messages: &[StoredMessage], head: Option<u64>) -> Vec<Utterance> {
    let mut chain: Vec<&StoredMessage> = Vec::new();
    let mut cur = head;
    while let Some(id) = cur {
        match messages.iter().find(|m| m.id == id) {
            Some(m) => {
                chain.push(m);
                cur = m.parent_id;
            }
            None => break,
        }
    }
    chain.reverse();
    chain.iter().map(|m| Utterance { speaker: m.speaker.clone(), content: m.content.clone() }).collect()
}

/// 다음 메시지 id(max+1, 비어있으면 1).
pub fn next_id(messages: &[StoredMessage]) -> u64 {
    messages.iter().map(|m| m.id).max().map(|m| m + 1).unwrap_or(1)
}

/// 트리 요약 줄(id, parent, speaker, 본문 일부). /branches 표시용.
pub fn tree_summary(messages: &[StoredMessage], head: Option<u64>) -> String {
    if messages.is_empty() {
        return "(빈 트리)".to_string();
    }
    let mut out = String::new();
    for m in messages {
        let marker = if Some(m.id) == head { "*" } else { " " };
        let parent = m.parent_id.map(|p| p.to_string()).unwrap_or_else(|| "-".into());
        let snippet: String = m.content.chars().take(30).collect();
        out.push_str(&format!("{marker} #{} (<-{parent}) {}: {}\n", m.id, m.speaker, snippet));
    }
    out
}

/// StoredSession을 JSON으로 저장.
pub fn save_session(s: &StoredSession, path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(path, json)
}

/// StoredSession 로드. 레거시 bare-array(head 없음)면 head=마지막 id로 폴백.
pub fn load_session(path: &str) -> std::io::Result<StoredSession> {
    let s = std::fs::read_to_string(path)?;
    if let Ok(ss) = serde_json::from_str::<StoredSession>(&s) {
        return Ok(ss);
    }
    // 레거시 v1: bare [StoredMessage]
    let messages: Vec<StoredMessage> = serde_json::from_str(&s)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let head = messages.iter().map(|m| m.id).max();
    Ok(StoredSession { messages, head })
}
```
  - `Utterance` import는 이미 있음(파일 상단). 기존 to_stored/from_stored/save/load는 그대로 둔다(테스트·레거시).

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib store` PASS, clippy 클린.
  `git add src/store/mod.rs && git commit -m "feat(store): 트리 순수함수(path_to_root/next_id) + StoredSession 저장 포맷"` (push 금지).

---

### Task 2: Session 트리 리팩토링

**Files:**
- Modify: `src/repl/mod.rs`, `src/main.rs`

- [ ] **Step 1: Session 필드 교체 (`src/repl/mod.rs`)**
  - `use crate::store::{StoredMessage, StoredSession};`(또는 store 경로) 추가. `transcript: Vec<Utterance>` -> `messages: Vec<StoredMessage>, head: Option<u64>`.
```rust
pub struct Session {
    participants: Vec<Participant>,
    messages: Vec<StoredMessage>,
    head: Option<u64>,
    registry: Box<dyn RunnerRegistry>,
}
```
  - `new`: `messages: Vec::new(), head: None`.
  - active path 헬퍼 + round append 헬퍼(private):
```rust
    fn active_path(&self) -> Vec<Utterance> {
        crate::store::path_to_root(&self.messages, self.head)
    }
    /// round 발언들을 head에서 시작하는 체인으로 트리에 append하고 head를 옮긴다.
    fn append_round(&mut self, round: &[Utterance]) {
        for u in round {
            let id = crate::store::next_id(&self.messages);
            self.messages.push(StoredMessage {
                id,
                parent_id: self.head,
                speaker: u.speaker.clone(),
                content: u.content.clone(),
            });
            self.head = Some(id);
        }
    }
```
  - `transcript_len` -> active path 길이: `self.active_path().len()`.
  - `transcript_markdown` -> `render(&self.active_path())` 기반(기존과 동일하게 active 분기 렌더).

- [ ] **Step 2: 라운드 실행부 4곳을 트리로** — Message/Only/Conclude/Write 분기에서 패턴 통일:
```rust
            Command::Message(text) => {
                let mut path = self.active_path();
                match run_round(&self.participants, &mut path, &text, self.registry.as_ref(), RunMode::ReadOnly) {
                    Ok(round) => { self.append_round(&round); StepOutcome::Print(render(&round)) }
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
```
  - Only/Write/Conclude도 동일 패턴(seats/synth 구성은 그대로, run_round 후 `self.append_round(&round)`). run_round에 넘기는 transcript = `&mut path`(active_path 복사본). round 반환값을 append.
  - 주의: 기존엔 run_round가 self.transcript를 직접 mutate했지만 이제는 임시 path를 받고, append는 Session이 round로 수행한다(중복 없이).

- [ ] **Step 3: 영속 배선 (`src/repl/mod.rs` + `src/main.rs`)**
  - `save_state`: `crate::store::save_session(&StoredSession { messages: self.messages.clone(), head: self.head }, path)`.
  - `resume`: `let ss = crate::store::load_session(path)?; Ok(Self { participants, messages: ss.messages, head: ss.head, registry })`.
  - `main.rs`는 save_state/resume 시그니처가 같으면 무변경. (resume이 std::io::Result 유지하면 main 분기 그대로.)

- [ ] **Step 4: 기존 테스트 통과 확인** — 기존 repl 테스트(step_message transcript_len==2 등)는 선형 사용이라 트리에서도 동일 결과여야 한다. `cargo test` 전체 GREEN. 깨지면 원인 보고(특히 transcript_len 계산).

- [ ] **Step 5: 커밋** — `cargo build`/`clippy` 클린.
  `git add src/repl/mod.rs src/main.rs && git commit -m "refactor(repl): Session 선형 전사 -> in-store 트리(messages+head)"` (push 금지).

---

### Task 3: /branches + /checkout 분기 탐색

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저 (`mod tests`)**
```rust
    #[test]
    fn parses_branches_and_checkout() {
        assert_eq!(parse_command("/branches"), Command::Branches);
        assert_eq!(parse_command("/checkout 3"), Command::Checkout(3));
        assert_eq!(parse_command("/checkout"), Command::Message("/checkout".into())); // 인자 없으면 일반 메시지
    }

    #[test]
    fn checkout_then_message_creates_branch() {
        let mut s = session_with_two_seats(); // claude=제안, codex=리뷰
        let _ = s.step(Command::Message("주제".into()));   // msg 1,2 (head=2)
        // head를 1로 옮기고 새 메시지 -> 분기(2의 sibling)
        match s.step(Command::Checkout(1)) {
            StepOutcome::Print(t) => assert!(t.contains("1")),
            other => panic!("got {other:?}"),
        }
        let _ = s.step(Command::Message("다른 방향".into())); // msg 3,4 parent=1.. (분기)
        // 트리에 4개 메시지(2개 분기), active path는 1->3->4 (길이 3)
        assert_eq!(s.message_count(), 4);
        assert_eq!(s.transcript_len(), 3);
    }

    #[test]
    fn checkout_unknown_id_errors() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제".into()));
        match s.step(Command::Checkout(99)) {
            StepOutcome::Print(t) => assert!(t.contains("없")),
            other => panic!("got {other:?}"),
        }
    }
```
  - `message_count()` 헬퍼(트리 전체 메시지 수) 추가 필요(테스트용 pub).

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl` -> FAIL.

- [ ] **Step 3: 구현**
  - `Command`에 추가: `Branches,` 와 `Checkout(u64),`.
  - parse_command의 `/` match에 추가:
```rust
            "branches" | "tree" => Command::Branches,
            "checkout" | "co" => match arg.and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Checkout(id),
                None => Command::Message(line.to_string()),
            },
```
  - Session에 `pub fn message_count(&self) -> usize { self.messages.len() }`.
  - step에 분기 추가:
```rust
            Command::Branches => StepOutcome::Print(crate::store::tree_summary(&self.messages, self.head)),
            Command::Checkout(id) => {
                if self.messages.iter().any(|m| m.id == id) {
                    self.head = Some(id);
                    StepOutcome::Print(format!("checkout #{id} (현재 분기 전환). 이어서 메시지를 보내면 분기됩니다."))
                } else {
                    StepOutcome::Print(format!("그런 메시지가 없습니다: #{id}"))
                }
            }
```
  - /help 텍스트에 `/branches`·`/checkout <id>` 추가.

- [ ] **Step 4: 통과 + 전체 검증 + 커밋**
  - `cargo test`(전체) PASS. `cargo build`/`clippy` 클린. (선택) `printf '주제\n/branches\n/quit\n' | cargo run` 스모크는 실 에이전트 호출이라 생략 가능, fake 테스트로 충분.
  - `git add src/repl/mod.rs && git commit -m "feat(repl): /branches + /checkout 분기 탐색"` (push 금지).

---

## Self-Review (작성자 체크)

- **결정 준수:** in-store 논리 트리(사용자 확정). 브랜치=세션 틀 위에서 parent_id 실사용. Redis는 Plan 06.
- **placeholder:** 없음.
- **격리/경계:** 트리 로직은 store 순수함수(테스트 용이) + Session 책임. run_round/Participant/러너 무변경. 저장 포맷 변경은 레거시 폴백으로 backward compat.
- **타입 일관성:** Session.transcript -> messages+head. transcript_len/markdown은 active path 기반(기존 의미 보존, 선형 사용 시 동일). 기존 repl 테스트 불변 통과 기대.
- **TDD:** 트리 함수·파싱·분기 동작 모두 fake/순수 테스트.

## 위험 / 한계 (문서화된 후속)

- **저장 포맷 변경:** bare-array -> {messages, head}. load_session이 레거시 폴백하지만, 새 저장은 신포맷. 구버전 바이너리로 신포맷 파일은 못 읽음(개인 도구라 허용).
- **트리 시각화 단순:** tree_summary는 평면 목록(id/parent/marker). ASCII 트리 그래프는 후속.
- **Plan 06 연계:** 각 분기에 session_id 부여 + Redis 동시성/presence는 Plan 06. 이 plan은 단일 프로세스.
- **head=None 빈 세션:** active_path 빈 Vec, 첫 라운드가 root 생성(parent None). 정상.