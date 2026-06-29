---
title: "tunaRound v2 Plan 03: 에이전트 쓰기 지목 (협업 코딩)"
type: plan
status: done
priority: P1
updated_at: 2026-06-29
owner: shared
summary: 사람이 `@engine! <msg>`로 한 자리를 쓰기 턴으로 지목하면 그 에이전트가 RunMode::Write로 실제 레포(cwd)를 편집한다. 토론 도구 -> 협업 코딩 도약. run_round에 mode 파라미터 추가(기존 호출은 ReadOnly = 무변경), Command::Write + @engine! 파싱 + step 분기. 쓰기 인자/샌드박스는 v1에 이미 구현됨. 결정 확정: claude 현행 권한 / cwd / 확인 프롬프트 없음.
---

# tunaRound v2 Plan 03: 에이전트 쓰기 지목 Implementation Plan

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v2-write-delegation` -> main). 52 테스트 green(기존 48 + 신규 4), `cargo build`/`clippy` 경고 0. Opus 리뷰: 계획서 정확히 일치, 큰 문제 없음.

- Task 1: `run_round`에 `mode: RunMode` 파라미터, 4개 호출부 ReadOnly로 갱신(동작 보존) (커밋 `9c55b97`).
- Task 2: `Command::Write` + `@engine!` 파싱 + `Session::step` Write 분기(RunMode::Write) + /help 갱신 + ModeEchoRunner 테스트 4개 (커밋 `1ae8b49`).
- 쓰기 인프라(러너 인자: claude `--dangerously-skip-permissions` / codex `--sandbox workspace-write`)는 v1 구현 재사용(무변경).
- 사소(비차단, 후속): 쓰기 후 git diff 요약 없음, 실 쓰기 스모크는 수동(`@engine!` 1회 + git status), 오타 보호 없음(확인 프롬프트 미채택).

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`). TDD red->green.
> 선행: v2 Plan 01·02 done. 설계·결정: docs/design/v2-write-delegation-design_2026-06-29.md (결정 3건 확정).

**Goal:** 사람이 특정 자리에 쓰기 턴을 지목해(`@engine! <msg>`) 그 에이전트가 실제로 레포를 편집하게 한다. v1은 모든 턴 읽기 전용이었다.

**Architecture:** 쓰기 인프라(RunMode::Write + 러너별 쓰기 인자: claude `--dangerously-skip-permissions`, codex `--sandbox workspace-write`)는 v1부터 구현돼 있다. 막힌 건 (1) `run_round`이 mode를 ReadOnly로 하드코딩, (2) REPL에 쓰기 지목 경로 없음 - 이 둘만 연다. `run_round`에 `mode: RunMode` 파라미터를 추가(기존 호출부는 ReadOnly 전달 = 동작 불변)하고, `@engine!`로 쓰기 턴을 지목하는 `Command::Write`를 추가한다. 기존 `@engine`(읽기 Only)와 평행.

**Tech Stack:** Rust 2024, 신규 의존성 0. 선행: v2 Plan 01·02(done).

> 결정 확정(설계안): claude 쓰기 권한 현행 유지(러너 인자 변경 없음), 쓰기 대상 = cwd(run_round의 project_path: None 그대로), 실행 전 확인 프롬프트 없음(역할 분리로 동시 같은 파일 경합 없음).
> 규율: #5 한국어 마침표, #6 파일 헤더(신규 파일 없음), TDD.

---

## 범위

- **포함:** `run_round`에 `mode: RunMode` 파라미터(기존 4개 호출부 ReadOnly로 갱신) + `Command::Write { engine, text }` + `parse_command`의 `@engine!` 분기 + `Session::step` Write 분기(해당 1자리만 RunMode::Write) + /help 텍스트 갱신.
- **비포함(후속):** 쓰기 후 `git diff --stat` 자동 요약(추적성), 자동 커밋, 쓰기 결과 자동 리뷰 라운드, 여러 자리 동시 쓰기, 롤백 명령, `--project <path>` 대상 분리.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/orchestrator/mod.rs` | (수정) `run_round`에 `mode: RunMode` 파라미터. RunInput.mode를 그 값으로. 주석 갱신. |
| `src/repl/mod.rs` | (수정) `use crate::runner::RunMode;`, `Command::Write` + `@engine!` 파싱 + step Write 분기. Message/Only/Conclude 호출은 ReadOnly 전달. /help 갱신. |
| `tests/orchestrator_round.rs` | (수정) run_round 호출에 `RunMode::ReadOnly` 인자 추가. |

> 선제 설계: mode를 호출자가 지정(additive 파라미터, 기존 동작 보존). `@engine!`는 기존 `@engine` 파서에 trailing `!` 감지만 추가. 새 모듈/concrete 러너 의존 없음.

---

### Task 1: run_round에 mode 파라미터 (behavior-preserving 리팩토링)

**Files:**
- Modify: `src/orchestrator/mod.rs`, `src/repl/mod.rs`, `tests/orchestrator_round.rs`

이 Task는 동작을 바꾸지 않는다(모든 호출부 ReadOnly 전달). 전체 테스트가 계속 green이어야 한다.

- [ ] **Step 1: `run_round` 시그니처에 mode 추가 (`src/orchestrator/mod.rs`)**
  - 시그니처에 파라미터 추가(맨 끝):
```rust
pub fn run_round(
    participants: &[Participant],
    transcript: &mut Vec<Utterance>,
    topic: &str,
    registry: &dyn RunnerRegistry,
    mode: RunMode,
) -> Result<Vec<Utterance>, RunError> {
```
  - 본문 RunInput의 `mode: RunMode::ReadOnly`를 `mode`로 교체:
```rust
        let input = RunInput {
            prompt,
            model: None,
            project_path: None,
            mode,
        };
```
  - 함수 위 주석 "v1 토론 턴은 읽기 전용..." 줄을 갱신: `// mode는 호출자가 지정(말하기=ReadOnly, 사람이 지목한 쓰기 턴=Write).`
  - `RunMode`는 이미 `use crate::runner::{RunError, RunInput, RunMode, Runner};`로 import됨(확인). 미import면 추가.

- [ ] **Step 2: 호출부 갱신 (전부 ReadOnly)**
  - `src/repl/mod.rs` 상단에 `use crate::runner::RunMode;` 추가.
  - L122(Message), L133(Only), L148(Conclude)의 run_round 호출 끝에 `, RunMode::ReadOnly` 추가:
```rust
    run_round(&self.participants, &mut self.transcript, &text, self.registry.as_ref(), RunMode::ReadOnly)
    // Only:
    run_round(&seats, &mut self.transcript, &text, self.registry.as_ref(), RunMode::ReadOnly)
    // Conclude:
    run_round(&synth, &mut self.transcript, "지금까지의 토론을 종합해 결론을 정리해줘.", self.registry.as_ref(), RunMode::ReadOnly)
```
  - `tests/orchestrator_round.rs` L27: `use tunaround::runner::RunMode;`(필요시) + 호출에 `RunMode::ReadOnly` 추가:
```rust
    let round = run_round(&participants, &mut transcript, "이 설계 어떤가요?", &reg, RunMode::ReadOnly).expect("ok");
```

- [ ] **Step 3: 전체 검증** — `cargo test`(전체 48) PASS, `cargo build` 경고 0, `cargo clippy --all-targets` 클린. 동작 불변.

- [ ] **Step 4: 커밋** — `git add src/orchestrator/mod.rs src/repl/mod.rs tests/orchestrator_round.rs && git commit -m "refactor(orchestrator): run_round에 mode 파라미터 (쓰기 턴 준비)"` (push 금지).

---

### Task 2: @engine! 쓰기 지목 명령

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저 (`src/repl/mod.rs`의 `mod tests`)**
  - mode 전파를 검증할 테스트용 러너를 tests 안에 추가(mode를 출력에 echo):
```rust
    struct ModeEchoRunner;
    impl Runner for ModeEchoRunner {
        fn run(&self, i: &RunInput) -> Result<RunOutput, RunError> {
            Ok(RunOutput { content: format!("mode={:?}", i.mode), input_tokens: 0, output_tokens: 0 })
        }
    }

    fn session_with_mode_echo() -> Session {
        let mut reg = MapRegistry::new();
        reg.insert("codex", Box::new(ModeEchoRunner));
        let participants = vec![
            Participant { engine: "codex".into(), role: Some("coder".into()), instruction: String::new() },
        ];
        Session::new(participants, Box::new(reg))
    }
```
  - 파싱/step 테스트:
```rust
    #[test]
    fn parses_at_engine_bang_as_write() {
        assert_eq!(parse_command("@codex! 이 함수 고쳐줘"), Command::Write { engine: "codex".into(), text: "이 함수 고쳐줘".into() });
        // 읽기 지목은 그대로
        assert_eq!(parse_command("@codex 봐줘"), Command::Only { engine: "codex".into(), text: "봐줘".into() });
        // bang만 있고 메시지 없으면 일반 메시지
        assert_eq!(parse_command("@codex!"), Command::Message("@codex!".into()));
    }

    #[test]
    fn step_write_uses_write_mode_on_single_seat() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Write { engine: "codex".into(), text: "고쳐줘".into() }) {
            StepOutcome::Print(text) => assert!(text.contains("Write"), "got: {text}"),
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 1);
    }

    #[test]
    fn step_only_stays_readonly() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Only { engine: "codex".into(), text: "봐줘".into() }) {
            StepOutcome::Print(text) => assert!(text.contains("ReadOnly"), "got: {text}"),
            other => panic!("expected Print, got {other:?}"),
        }
    }

    #[test]
    fn step_write_unknown_engine_errors() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Write { engine: "gemini".into(), text: "x".into() }) {
            StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
            other => panic!("expected Print, got {other:?}"),
        }
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl` -> 컴파일 에러/FAIL(Command::Write 미존재).

- [ ] **Step 3: 구현 (`src/repl/mod.rs`)**
  - `Command` enum에 variant 추가: `Write { engine: String, text: String },`
  - `parse_command`의 `@` 분기를 trailing `!` 감지로 확장:
```rust
    if let Some(rest) = line.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let mut engine = it.next().unwrap_or("").to_string();
        let text = it.next().map(|s| s.trim().to_string()).unwrap_or_default();
        let write = engine.ends_with('!');
        if write {
            engine.pop(); // trailing '!' 제거
        }
        if !engine.is_empty() && !text.is_empty() {
            return if write {
                Command::Write { engine, text }
            } else {
                Command::Only { engine, text }
            };
        }
        return Command::Message(line.to_string()); // "@codex"·"@codex!"만이면 일반 메시지
    }
```
  - `Session::step`에 Write 분기 추가(Only 분기 옆):
```rust
            Command::Write { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                match run_round(&seats, &mut self.transcript, &text, self.registry.as_ref(), RunMode::Write) {
                    Ok(round) => StepOutcome::Print(render(&round)),
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
```
  - /help 텍스트에 `@engine!` 추가(Help 분기 문자열):
```rust
    "메시지를 입력하면 두 에이전트가 응답합니다. @engine 메시지로 한 자리만 지목(읽기), @engine! 메시지로 쓰기 턴(에이전트가 레포 편집), /conclude [engine] 종합, /save [경로] 결과 저장, /quit 종료.".into(),
```

- [ ] **Step 4: 통과 + 전체 검증 + 커밋**
  - `cargo test`(전체) PASS. `cargo build` 경고 0. `cargo clippy --all-targets` 클린.
  - `git add src/repl/mod.rs && git commit -m "feat(repl): @engine! 쓰기 지목 (협업 코딩)"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** 쓰기 지목(설계 §"제안 접근")의 핵심 = 사람이 한 자리에 쓰기 턴 지목. 결정 3건 반영(권한 현행/cwd/확인없음). 쓰기 인프라 재사용(러너 인자 v1 구현).
- **placeholder:** 없음.
- **타입 일관성:** `run_round`에 mode 파라미터(additive, 기존 호출 ReadOnly로 동작 보존). `Command::Write`는 기존 `Only`와 동형(추가 variant). RunMode import repl에 추가.
- **behavior preservation:** Task 1은 순수 리팩토링(전 호출 ReadOnly), 테스트 green 유지. Task 2만 새 동작.
- **선제 설계:** 신규 의존성·모듈 없음. `@engine!`는 기존 파서에 `!` 감지만. concrete 러너 미의존.

## 위험 / 한계 (문서화된 후속)

- **무샌드박스 쓰기(claude):** 결정상 `--dangerously-skip-permissions` 유지. 쓰기 턴은 무샌드박스 자율 편집/bash. 한 번에 한 자리만(역할 분리). 추적성(git diff 요약)·자동 커밋은 후속.
- **실 쓰기 스모크는 수동:** 자동 테스트는 mode 전파만 검증(ModeEchoRunner). 실제 claude/codex가 파일을 쓰는 검증은 사람이 `@engine!`로 1회 + `git status` 확인(이 plan 자동 테스트 범위 밖).
- **오타 보호 없음:** 확인 프롬프트 없음(결정). `@engine!` 오타 시 즉시 쓰기 턴. 역할 분리·단일 자리로 위험 한정.