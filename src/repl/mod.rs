// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.
use crate::orchestrator::{run_round, Participant, RunnerRegistry, Utterance};

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

#[cfg(test)]
mod tests {
    use super::*;
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
    fn step_quit_help_save() {
        let mut s = session_with_two_seats();
        assert!(matches!(s.step(Command::Quit), StepOutcome::Exit));
        assert!(matches!(s.step(Command::Help), StepOutcome::Print(_)));
        assert!(matches!(s.step(Command::Noop), StepOutcome::Noop));
        match s.step(Command::Save(Some("x.md".into()))) {
            StepOutcome::Save { path, .. } => assert_eq!(path, "x.md"),
            other => panic!("expected Save, got {other:?}"),
        }
    }
}
