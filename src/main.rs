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
