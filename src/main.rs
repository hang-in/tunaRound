// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

fn main() {
    // 인자: [--roster <path>] [<state.json>]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut roster_path: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--roster" => {
                roster_path = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                if state_path.is_none() {
                    state_path = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    // 로스터 파일이 있으면 동적 좌석, 없으면 기본 2자리(claude proposer + codex reviewer).
    let (participants, registry): (Vec<Participant>, MapRegistry) = match &roster_path {
        Some(p) => {
            let roster = match tunaround::roster::load_roster(p) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            let parts = match tunaround::roster::build_participants_checked(&roster) {
                Ok(v) => v,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            let reg = match tunaround::roster::build_registry(&roster) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            (parts, reg)
        }
        None => {
            let mut reg = MapRegistry::new();
            reg.insert("claude", Box::new(ClaudeRunner::new()));
            reg.insert("codex", Box::new(CodexRunner::new()));
            let parts = vec![
                Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
                Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
            ];
            (parts, reg)
        }
    };

    // 선택 상태파일: `cargo run -- <state.json>` 있으면 시작 시 resume, 종료 시 저장.
    let resume_existing = state_path
        .as_deref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);
    let mut session = if resume_existing {
        let p = state_path.as_deref().unwrap();
        match Session::resume(participants, Box::new(registry), p) {
            Ok(s) => {
                println!("(이어받음: {p})");
                s
            }
            Err(e) => {
                eprintln!("[resume 실패: {e}] 종료합니다.");
                std::process::exit(1);
            }
        }
    } else {
        Session::new(participants, Box::new(registry))
    };

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
    if let Some(p) = &state_path {
        match session.save_state(p) {
            Ok(()) => println!("세션 저장됨: {p}"),
            Err(e) => println!("[세션 저장 실패] {e}"),
        }
    }
}
