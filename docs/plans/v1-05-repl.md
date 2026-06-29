---
title: "tunaRound v1 Plan 05: thin REPL (돌아가는 앱)"
type: plan
status: done
priority: P0
updated_at: 2026-06-29
owner: shared
summary: 사용자 입력 -> run_round -> 렌더의 터미널 REPL. 명령 파싱(순수) + Session.step(fake registry로 테스트) + main.rs가 실 CodexRunner/ClaudeRunner를 묶는 돌아가는 앱. 결과 문서는 도구가 전사에서 저장(/save). 전사 영속·resume은 Plan 04.
---

# tunaRound v1 Plan 05: thin REPL Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** 터미널에서 사용자가 메시지를 치면 두 에이전트(실 CLI)가 응답하는, 실제로 돌아가는 앱을 만든다.

**Architecture:** 코어(orchestrator/runner)는 그대로. 새 `repl` 모듈은 명령 파싱(순수)과 `Session`(참가자+전사+레지스트리 보유, `step`으로 한 입력 처리)을 제공한다. I/O(stdin 루프·파일 쓰기·실 러너 spawn)는 `main.rs`에 격리해 단위테스트는 fake registry로, 실 CLI 구동은 수동 스모크로 검증한다.

**Tech Stack:** Rust 2024, std only. 선행: Plan 01/02/03(done).

> 규율: docs/reference/development-guidelines.md. 설계 §5(핵심 루프), §4(읽기 전용 화자). v1 에이전트는 읽기 전용 - 결과 문서는 도구가 전사에서 저장(/save). 에이전트 파일 쓰기(RunMode::Write 행사)는 v2.

---

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v1-repl` -> main). **돌아가는 앱.** 비대화형 스모크로 구동 확인: 배너 출력 -> `/help` 도움말 -> `/save`로 결과 md 기록(`저장됨: ...`) -> `/quit` 종료. 전체 테스트 green(26 unit + 3 integration), `cargo build`/`clippy` 클린.

- Message(실 CLI) 경로는 fake 러너로 단위 검증. 실 claude/codex 구동은 사용자가 `cargo run`으로 스모크.
- 작성 시 Task 2 테스트 import 충돌(Participant/Utterance 중복)을 실행 전 정리.
- 커밋: e35683d -> d5e3dfc -> 10dda04.

## 범위

- **포함:** `src/repl/mod.rs` - `Command` 파싱(순수) + `render`(순수) + `Session::step`(fake registry 테스트). `src/main.rs` - 실 러너 레지스트리 + 기본 참가자 + stdin 루프 + /save 파일 쓰기(돌아가는 앱).
- **비포함(후속):** 전사 영속·resume·트리-ready 모델 → Plan 04. 자리 지목(@engine)·에이전트 쓰기 지목(RunMode::Write) → 후속. consensus 합성 /conclude → 후속. ratatui TUI → v1.x.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/lib.rs` | (수정) `pub mod repl;` |
| `src/repl/mod.rs` | (신규) `Command`·`parse_command`·`render`·`StepOutcome`·`Session` |
| `src/main.rs` | (수정) 실 러너 묶기 + stdin 루프 + /save (돌아가는 앱) |

> 선제 설계: 파싱·렌더 순수함수, Session은 RunnerRegistry 경계에만 의존(테스트 fake 주입), I/O는 main에 격리.

---

### Task 1: 명령 파싱 (순수)

**Files:**
- Modify: `src/lib.rs` (`pub mod repl;`)
- Create: `src/repl/mod.rs`

- [ ] **Step 1: lib.rs + 실패 테스트**
`src/lib.rs`에 `pub mod repl;` 추가.
`src/repl/mod.rs` 생성, 첫 줄 `// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.` 그 아래:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("/quit"), Command::Quit);
        assert_eq!(parse_command("/help"), Command::Help);
        assert_eq!(parse_command("/save notes.md"), Command::Save(Some("notes.md".into())));
        assert_eq!(parse_command("/save"), Command::Save(None));
        assert_eq!(parse_command("이 설계 어떤가요?"), Command::Message("이 설계 어떤가요?".into()));
    }

    #[test]
    fn blank_is_noop() {
        assert_eq!(parse_command("   "), Command::Noop);
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl::tests::parses` → FAIL.

- [ ] **Step 3: 구현 (mod.rs, 테스트 위)**
```rust
/// REPL 한 줄 입력의 해석 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Message(String),
    Save(Option<String>),
    Help,
    Quit,
    Noop,
}

/// 한 줄을 명령으로 파싱한다. `/`로 시작하면 명령, 아니면 메시지, 공백이면 Noop.
pub fn parse_command(line: &str) -> Command {
    let line = line.trim();
    if line.is_empty() {
        return Command::Noop;
    }
    if let Some(rest) = line.strip_prefix('/') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let name = it.next().unwrap_or("");
        let arg = it.next().map(|s| s.trim().to_string());
        return match name {
            "quit" | "exit" | "q" => Command::Quit,
            "help" | "h" => Command::Help,
            "save" => Command::Save(arg.filter(|s| !s.is_empty())),
            _ => Command::Message(line.to_string()),
        };
    }
    Command::Message(line.to_string())
}
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib repl::tests` PASS.
`git add src/lib.rs src/repl/mod.rs && git commit -m "feat(repl): 명령 파싱"` (push 금지).

---

### Task 2: 렌더 + Session.step (fake registry 테스트)

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 추가 (`src/repl/mod.rs`의 mod tests)**
```rust
    // Participant/Utterance/Command/StepOutcome/Session/render는 `use super::*`로 들어온다.
    // 여기선 super에 없는 것만 명시 import.
    use crate::orchestrator::MapRegistry;
    use crate::runner::{RunError, RunInput, RunOutput, Runner};

    struct FakeRunner { reply: String }
    impl Runner for FakeRunner {
        fn run(&self, _i: &RunInput) -> Result<RunOutput, RunError> {
            Ok(RunOutput { content: self.reply.clone(), input_tokens: 0, output_tokens: 0 })
        }
    }

    fn session_with_two_seats() -> Session {
        let mut reg = MapRegistry::new();
        reg.insert("claude", Box::new(FakeRunner { reply: "제안".into() }));
        reg.insert("codex", Box::new(FakeRunner { reply: "리뷰".into() }));
        let participants = vec![
            Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
            Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
        ];
        Session::new(participants, Box::new(reg))
    }

    #[test]
    fn render_formats_speaker_and_content() {
        let utts = vec![Utterance { speaker: "claude/proposer".into(), content: "제안".into() }];
        let out = render(&utts);
        assert!(out.contains("claude/proposer"));
        assert!(out.contains("제안"));
    }

    #[test]
    fn step_message_runs_round_and_prints() {
        let mut s = session_with_two_seats();
        match s.step(Command::Message("이 설계?".into())) {
            StepOutcome::Print(text) => {
                assert!(text.contains("제안"));
                assert!(text.contains("리뷰"));
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 2);
    }

    #[test]
    fn step_quit_and_help_and_save() {
        let mut s = session_with_two_seats();
        assert!(matches!(s.step(Command::Quit), StepOutcome::Exit));
        assert!(matches!(s.step(Command::Help), StepOutcome::Print(_)));
        assert!(matches!(s.step(Command::Noop), StepOutcome::Print(_) | StepOutcome::Noop));
        // 빈 전사 저장도 동작(헤더만)
        match s.step(Command::Save(Some("x.md".into()))) {
            StepOutcome::Save { path, .. } => assert_eq!(path, "x.md"),
            other => panic!("expected Save, got {other:?}"),
        }
    }
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib repl::tests` → FAIL(`render`/`Session`/`StepOutcome` 미정의).

- [ ] **Step 3: 구현 (mod.rs, Command 아래)**
```rust
use crate::orchestrator::{run_round, Participant, RunnerRegistry, Utterance};

/// step 결과. I/O(출력·파일쓰기·종료)는 main이 수행한다.
#[derive(Debug)]
pub enum StepOutcome {
    Print(String),
    Save { path: String, markdown: String },
    Exit,
    Noop,
}

/// 한 발언 목록을 터미널 표시용 문자열로.
pub fn render(round: &[Utterance]) -> String {
    round
        .iter()
        .map(|u| format!("## {}\n{}", u.speaker, u.content))
        .collect::<Vec<_>>()
        .join("\n\n")
}

const DEFAULT_SAVE_PATH: &str = "tunaround-discussion.md";

/// 한 토론 세션. 참가자 + 전사 + 러너 레지스트리를 보유한다.
pub struct Session {
    participants: Vec<Participant>,
    transcript: Vec<Utterance>,
    registry: Box<dyn RunnerRegistry>,
}

impl Session {
    pub fn new(participants: Vec<Participant>, registry: Box<dyn RunnerRegistry>) -> Self {
        Self { participants, transcript: Vec::new(), registry }
    }

    pub fn transcript_len(&self) -> usize {
        self.transcript.len()
    }

    /// 전사를 마크다운 결과 문서로 직렬화(도구가 저장 - 에이전트 파일쓰기는 v2).
    pub fn transcript_markdown(&self) -> String {
        let mut out = String::from("# tunaRound 토론 기록\n\n");
        out.push_str(&render(&self.transcript));
        out.push('\n');
        out
    }

    /// 한 입력을 처리한다. run_round 호출 등 로직만; 실제 I/O는 호출자(main).
    pub fn step(&mut self, cmd: Command) -> StepOutcome {
        match cmd {
            Command::Quit => StepOutcome::Exit,
            Command::Noop => StepOutcome::Noop,
            Command::Help => StepOutcome::Print(
                "메시지를 입력하면 두 에이전트가 응답합니다. /save [경로] 결과 저장, /quit 종료.".into(),
            ),
            Command::Save(path) => StepOutcome::Save {
                path: path.unwrap_or_else(|| DEFAULT_SAVE_PATH.to_string()),
                markdown: self.transcript_markdown(),
            },
            Command::Message(text) => {
                match run_round(&self.participants, &mut self.transcript, &text, self.registry.as_ref()) {
                    Ok(round) => StepOutcome::Print(render(&round)),
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
        }
    }
}
```

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib repl::tests` PASS(4개). `cargo build` 경고 0(Session/render는 main이 쓰지만, 통합테스트/타입 사용으로 dead_code면 main 구현 후 해소 - Task 3 전엔 transient 허용, suppress 금지).
`git add src/repl/mod.rs && git commit -m "feat(repl): render + Session.step"` (push 금지).

---

### Task 3: main.rs - 실 러너 묶기 + stdin 루프 (돌아가는 앱)

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 구현 (`src/main.rs` 교체)**
첫 줄 #6 헤더 유지/갱신. 실 러너를 묶고 stdin 루프를 돈다(이 파일은 I/O 격리부라 단위테스트 없음 - 수동 스모크).
```rust
// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

fn main() {
    // 기본 2자리: claude=제안자, codex=리뷰어(역할명은 roles.rs canonical).
    let mut registry = MapRegistry::new();
    registry.insert("claude", Box::new(ClaudeRunner::new()));
    registry.insert("codex", Box::new(CodexRunner::new()));
    let participants = vec![
        Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
        Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
    ];
    let mut session = Session::new(participants, Box::new(registry));

    println!("tunaRound - 메시지를 입력하세요. /help, /save, /quit.");
    let stdin = io::stdin();
    loop {
        print!("\n> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        match session.step(parse_command(&line)) {
            StepOutcome::Print(text) => println!("{text}"),
            StepOutcome::Noop => {}
            StepOutcome::Save { path, markdown } => match std::fs::write(&path, markdown) {
                Ok(()) => println!("저장됨: {path}"),
                Err(e) => println!("[저장 실패] {e}"),
            },
            StepOutcome::Exit => break,
        }
    }
}
```

- [ ] **Step 2: 빌드 + 전체 검증 (#8 §3)**
- `cargo build` → 경고 0(이제 Session/render/parse_command가 main에서 쓰여 dead_code 해소).
- `cargo test`(전체) → 기존 + repl 단위테스트 모두 통과.
- `cargo clippy --all-targets` → 클린.

- [ ] **Step 3: 수동 스모크(실 CLI, 선택이지만 권장)**
실제 claude/codex가 설치돼 있으면: `cargo run` 후 한 줄 입력 -> 두 응답이 출력되는지, `/save`로 파일이 써지는지, `/quit`로 종료되는지 확인. (CI 불가 - 실 CLI·대화형이라 수동.) 설치 안 됐으면 이 단계는 건너뛰고 보고에 명시.

- [ ] **Step 4: 커밋**
`git add src/main.rs && git commit -m "feat(repl): main.rs 실 러너 REPL (돌아가는 앱)"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** 사람 주도 핵심 루프(§5) - 입력 -> run_round -> 렌더. 결과 문서 저장(/save, 도구가 전사에서). v1 에이전트 읽기 전용(§4) 유지(RunMode::Write 미행사). 자리/쓰기 지목·전사 영속·consensus는 명시적 후속.
- **placeholder:** 없음. Task 3 Step 3은 실 CLI 수동 스모크(환경 의존, 조건부) - placeholder 아님.
- **타입 일관성:** Command/StepOutcome/Session을 repl/mod.rs에서 정의, main이 동일 사용. Participant/Utterance/run_round/MapRegistry/RunnerRegistry는 Plan 03, Codex/ClaudeRunner는 Plan 01/02 재사용.
- **선제 설계:** parse_command·render 순수함수, Session은 RunnerRegistry 경계 의존(테스트 fake), I/O는 main 격리.

## 다음 plan

- **Plan 04: 전사·영속** (트리-ready 메시지 id/parent + resume). Session 전사를 영속 모델로.
- **Hardening:** 양 러너 idle watchdog, consensus 합성(/conclude), 자리/쓰기 지목, 실 CLI 통합 스모크.
