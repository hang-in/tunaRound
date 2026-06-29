---
title: "tunaRound v1 Plan 06: Hardening (consensus 합성 + 자리 지목)"
type: plan
status: draft
priority: P1
updated_at: 2026-06-29
owner: shared
summary: v1 사용자 경험 완성용 hardening 2종. /conclude(synthesizer 역할로 토론 종합 = 결과 문서 품질) + @engine(자리 지목, 특정 자리만 응답). 둘 다 기존 run_round 재사용, REPL에 additive. idle watchdog과 에이전트 쓰기 지목(RunMode::Write 행사)은 v2.
---

# tunaRound v1 Plan 06: Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** 결과 문서 품질(`/conclude` 종합)과 사람 주도 UX(`@engine` 자리 지목)를 더해 v1 사용자 경험을 완성한다.

**Architecture:** 둘 다 새 `Command` variant + `Session::step` 분기로 추가하고, 실행은 기존 `run_round`를 그대로 재사용한다(synthesizer 역할 1자리 / 필터된 1자리). 기존 `Command::Message` 등은 건드리지 않는다(additive).

**Tech Stack:** Rust 2024. 선행: Plan 01~05(done).

> 규율: docs/reference/development-guidelines.md. 설계 §4(자리/쓰기 지목), roles.rs의 `synthesizer`. 비포함(v2): idle watchdog(INV-4, 동기 러너 refactor), 에이전트 쓰기 지목(RunMode::Write 실제 행사 = 협업 코딩).

---

## 범위

- **포함:** `/conclude [engine]`(synthesizer 역할 1자리로 토론 종합, 전사에 추가) + `@engine <message>`(그 자리만 응답). REPL 명령 파싱 + `Session::step` 분기. 기존 run_round 재사용.
- **비포함(v2):** idle watchdog / 에이전트가 직접 파일 쓰기 / consensus 자동(매 라운드) 주입 / N좌석 동적 구성.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/repl/mod.rs` | (수정) `Command::Conclude`/`Command::Only` + `parse_command` 분기 + `Session::step` 분기 |

> 선제 설계: 둘 다 additive variant + run_round 재사용. concrete 러너·새 모듈 없음.

---

### Task 1: /conclude (synthesizer 종합)

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 추가 (`src/repl/mod.rs`의 mod tests)**
```rust
    #[test]
    fn parses_conclude() {
        assert_eq!(parse_command("/conclude"), Command::Conclude(None));
        assert_eq!(parse_command("/conclude claude"), Command::Conclude(Some("claude".into())));
    }

    #[test]
    fn step_conclude_runs_synthesizer_and_grows_transcript() {
        let mut s = session_with_two_seats(); // claude=제안, codex=리뷰 (FakeRunner reply "제안"/"리뷰")
        // 먼저 한 라운드로 전사를 채운다
        let _ = s.step(Command::Message("주제?".into()));
        let before = s.transcript_len();
        match s.step(Command::Conclude(None)) {
            StepOutcome::Print(text) => assert!(text.contains("제안")), // 기본 엔진=claude의 FakeRunner reply
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), before + 1); // 종합 1발언 추가
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl::tests::parses_conclude repl::tests::step_conclude` → FAIL.

- [ ] **Step 3: 구현**
`Command` enum에 variant 추가: `Conclude(Option<String>),`
`parse_command`의 명령 match에 추가:
```rust
            "conclude" => Command::Conclude(arg.filter(|s| !s.is_empty())),
```
`Session::step`의 match에 추가(Message 분기 옆):
```rust
            Command::Conclude(engine) => {
                let eng = engine.or_else(|| self.participants.first().map(|p| p.engine.clone()));
                let Some(eng) = eng else {
                    return StepOutcome::Print("종합할 참가자가 없습니다.".into());
                };
                let synth = vec![Participant {
                    engine: eng,
                    role: Some("synthesizer".into()),
                    instruction: String::new(),
                }];
                match run_round(&synth, &mut self.transcript, "지금까지의 토론을 종합해 결론을 정리해줘.", self.registry.as_ref()) {
                    Ok(round) => StepOutcome::Print(render(&round)),
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib repl::tests` 전부 PASS.
`git add src/repl/mod.rs && git commit -m "feat(repl): /conclude synthesizer 종합"` (push 금지).

---

### Task 2: @engine 자리 지목

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 추가 (`mod tests`)**
```rust
    #[test]
    fn parses_at_engine_target() {
        assert_eq!(parse_command("@codex 이거 봐줘"), Command::Only { engine: "codex".into(), text: "이거 봐줘".into() });
        // @만 있고 메시지 없으면 일반 메시지로 취급
        assert_eq!(parse_command("@codex"), Command::Message("@codex".into()));
    }

    #[test]
    fn step_only_targets_single_seat() {
        let mut s = session_with_two_seats();
        match s.step(Command::Only { engine: "codex".into(), text: "리뷰만".into() }) {
            StepOutcome::Print(text) => {
                assert!(text.contains("리뷰"));      // codex FakeRunner reply
                assert!(!text.contains("제안"));     // claude는 응답 안 함
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 1); // 한 자리만
    }

    #[test]
    fn step_only_unknown_engine_errors() {
        let mut s = session_with_two_seats();
        match s.step(Command::Only { engine: "gemini".into(), text: "?".into() }) {
            StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
            other => panic!("expected Print, got {other:?}"),
        }
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl::tests::parses_at repl::tests::step_only` → FAIL.

- [ ] **Step 3: 구현**
`Command` enum에 variant 추가: `Only { engine: String, text: String },`
`parse_command`에서 `/` 분기보다 먼저(혹은 그 아래에) `@` 분기 추가. 빈 줄/Noop 처리 뒤, `/` 처리 앞에:
```rust
    if let Some(rest) = line.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let engine = it.next().unwrap_or("").to_string();
        let text = it.next().map(|s| s.trim().to_string()).unwrap_or_default();
        if !engine.is_empty() && !text.is_empty() {
            return Command::Only { engine, text };
        }
        return Command::Message(line.to_string()); // "@codex"만 있으면 일반 메시지
    }
```
`Session::step`의 match에 추가:
```rust
            Command::Only { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                match run_round(&seats, &mut self.transcript, &text, self.registry.as_ref()) {
                    Ok(round) => StepOutcome::Print(render(&round)),
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
```

- [ ] **Step 4: 통과 + 전체 검증 + 커밋**
- `cargo test`(전체) PASS. `cargo build` 경고 0. `cargo clippy --all-targets` 클린.
- (선택) /help 텍스트에 `@engine`/`/conclude` 추가하면 사용성↑(Session::step의 Help 문자열). 범위 내 한 줄 수정 허용.
- `git add src/repl/mod.rs && git commit -m "feat(repl): @engine 자리 지목"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** 자리 지목(§4 "코덱스만") + consensus 합성(결과 문서 품질, roles.synthesizer 활용). idle watchdog·에이전트 쓰기 지목은 명시적 v2.
- **placeholder:** 없음. 모든 단계 실코드.
- **타입 일관성:** Command에 Conclude/Only 추가(기존 Message 등 불변, additive). parse_command·Session::step 동일 사용. run_round/render/Participant 재사용.
- **선제 설계:** additive variant + run_round 재사용(새 경로 발명 안 함), concrete 러너 미의존.

## 다음 (핸드오프 후 v2)

- idle watchdog(INV-4): 동기 러너를 line-by-line read + 활동 타이머 + watchdog 스레드로(tunaFlow claude.rs 참고).
- 에이전트 쓰기 지목(RunMode::Write 실제 행사) = 협업 코딩 시작.
- Redis 멀티세션 = git-tree 다중 브랜치(StoredMessage.parent_id 기반), N좌석 로스터, ratatui/web.
