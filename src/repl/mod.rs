// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.
use crate::orchestrator::{run_round, Participant, RunnerRegistry, Utterance};
use crate::runner::RunMode;
use crate::session_bus::SessionBus;
use crate::store::{StoredMessage, StoredSession};

/// REPL 한 줄 입력의 해석 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Message(String),
    Save(Option<String>),
    Conclude(Option<String>),
    Only { engine: String, text: String },
    Write { engine: String, text: String },
    Branches,
    Checkout(u64),
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
            "conclude" => Command::Conclude(arg.filter(|s| !s.is_empty())),
            "branches" | "tree" => Command::Branches,
            "checkout" | "co" => match arg.as_deref().and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Checkout(id),
                None => Command::Message(line.to_string()),
            },
            _ => Command::Message(line.to_string()),
        };
    }
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

/// 한 토론 세션. 참가자 + in-store 트리(messages+head) + 러너 레지스트리를 보유한다.
pub struct Session {
    participants: Vec<Participant>,
    messages: Vec<StoredMessage>,
    head: Option<u64>,
    registry: Box<dyn RunnerRegistry>,
    bus: Option<Box<dyn SessionBus>>,
    session_id: String,
}

impl Session {
    pub fn new(participants: Vec<Participant>, registry: Box<dyn RunnerRegistry>) -> Self {
        Self { participants, messages: Vec::new(), head: None, registry, bus: None, session_id: "default".to_string() }
    }

    /// bus + session_id 있는 생성자. 매 라운드 후 Redis 미러를 활성화한다.
    pub fn new_with_bus(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        session_id: String,
        bus: Option<Box<dyn SessionBus>>,
    ) -> Self {
        Self { participants, messages: Vec::new(), head: None, registry, bus, session_id }
    }

    /// Redis snapshot에서 트리 상태를 주입한다. main이 --session 재개 시 호출.
    pub fn seed_from(&mut self, ss: StoredSession) {
        self.messages = ss.messages;
        self.head = ss.head;
    }

    /// 활성 경로(root->head) 전사를 반환한다.
    fn active_path(&self) -> Vec<Utterance> {
        crate::store::path_to_root(&self.messages, self.head)
    }

    /// round 발언들을 head에서 시작하는 체인으로 트리에 append하고 head를 옮긴다.
    fn append_round(&mut self, round: &[Utterance]) {
        let start = self.messages.len();
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

    /// 활성 경로의 발언 수를 반환한다(선형 사용 시 기존 transcript.len()과 동일).
    pub fn transcript_len(&self) -> usize {
        self.active_path().len()
    }

    /// 트리 전체 메시지 수를 반환한다(분기 포함).
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// 활성 경로를 마크다운 결과 문서로 직렬화.
    pub fn transcript_markdown(&self) -> String {
        let mut out = String::from("# tunaRound 토론 기록\n\n");
        out.push_str(&render(&self.active_path()));
        out.push('\n');
        out
    }

    /// 현재 트리를 상태 파일(JSON)로 저장한다.
    pub fn save_state(&self, path: &str) -> std::io::Result<()> {
        crate::store::save_session(&StoredSession { messages: self.messages.clone(), head: self.head }, path)
    }

    /// 현재 트리를 StoredSession JSON으로 직렬화한다(종료 시 Redis 동기 스냅샷 flush용).
    pub fn snapshot_json(&self) -> String {
        serde_json::to_string(&StoredSession { messages: self.messages.clone(), head: self.head })
            .unwrap_or_default()
    }

    /// 상태 파일에서 트리를 로드해 세션을 복원한다. 레거시 bare-array 포맷도 지원한다.
    pub fn resume(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        path: &str,
    ) -> std::io::Result<Self> {
        let ss = crate::store::load_session(path)?;
        Ok(Self { participants, messages: ss.messages, head: ss.head, registry, bus: None, session_id: "default".to_string() })
    }

    /// 한 입력을 처리한다. run_round 호출 등 로직만; 실제 I/O는 호출자(main).
    pub fn step(&mut self, cmd: Command) -> StepOutcome {
        match cmd {
            Command::Quit => StepOutcome::Exit,
            Command::Noop => StepOutcome::Noop,
            Command::Help => StepOutcome::Print(
                "메시지를 입력하면 두 에이전트가 응답합니다. @engine 메시지로 한 자리만 지목(읽기), @engine! 메시지로 쓰기 턴(에이전트가 레포 편집), /conclude [engine] 종합, /save [경로] 결과 저장, /branches 트리 목록, /checkout <id> 분기 전환, /quit 종료.".into(),
            ),
            Command::Save(path) => StepOutcome::Save {
                path: path.unwrap_or_else(|| DEFAULT_SAVE_PATH.to_string()),
                markdown: self.transcript_markdown(),
            },
            Command::Message(text) => {
                let mut path = self.active_path();
                match run_round(&self.participants, &mut path, &text, self.registry.as_ref(), RunMode::ReadOnly) {
                    Ok(round) => { self.append_round(&round); StepOutcome::Print(render(&round)) }
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
            Command::Only { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                let mut path = self.active_path();
                match run_round(&seats, &mut path, &text, self.registry.as_ref(), RunMode::ReadOnly) {
                    Ok(round) => { self.append_round(&round); StepOutcome::Print(render(&round)) }
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
            Command::Write { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                let mut path = self.active_path();
                match run_round(&seats, &mut path, &text, self.registry.as_ref(), RunMode::Write) {
                    Ok(round) => { self.append_round(&round); StepOutcome::Print(render(&round)) }
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
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
                let mut path = self.active_path();
                match run_round(&synth, &mut path, "지금까지의 토론을 종합해 결론을 정리해줘.", self.registry.as_ref(), RunMode::ReadOnly) {
                    Ok(round) => { self.append_round(&round); StepOutcome::Print(render(&round)) }
                    Err(e) => StepOutcome::Print(format!("[에러] {e:?}")),
                }
            }
            Command::Branches => StepOutcome::Print(crate::store::tree_summary(&self.messages, self.head)),
            Command::Checkout(id) => {
                if self.messages.iter().any(|m| m.id == id) {
                    self.head = Some(id);
                    StepOutcome::Print(format!("checkout #{id} (현재 분기 전환). 이어서 메시지를 보내면 분기됩니다."))
                } else {
                    StepOutcome::Print(format!("그런 메시지가 없습니다: #{id}"))
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
    fn parses_conclude() {
        assert_eq!(parse_command("/conclude"), Command::Conclude(None));
        assert_eq!(parse_command("/conclude claude"), Command::Conclude(Some("claude".into())));
    }

    #[test]
    fn step_conclude_runs_synthesizer_and_grows_transcript() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제?".into())); // 전사 2개 채움
        let before = s.transcript_len();
        match s.step(Command::Conclude(None)) {
            StepOutcome::Print(text) => assert!(text.contains("제안")), // 기본 엔진=claude FakeRunner reply
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), before + 1); // 종합 1발언 추가
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
                assert!(text.contains("리뷰"));   // codex FakeRunner reply
                assert!(!text.contains("제안"));  // claude는 응답 안 함
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 1);
    }

    #[test]
    fn step_only_unknown_engine_errors() {
        let mut s = session_with_two_seats();
        match s.step(Command::Only { engine: "gemini".into(), text: "?".into() }) {
            StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
            other => panic!("expected Print, got {other:?}"),
        }
    }

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

    #[test]
    fn parses_branches_and_checkout() {
        assert_eq!(parse_command("/branches"), Command::Branches);
        assert_eq!(parse_command("/checkout 3"), Command::Checkout(3));
        assert_eq!(parse_command("/checkout"), Command::Message("/checkout".into())); // 인자 없으면 일반 메시지
    }

    #[test]
    fn checkout_then_message_creates_branch() {
        let mut s = session_with_two_seats(); // claude=제안, codex=리뷰
        let _ = s.step(Command::Message("주제".into())); // msg 1,2 (head=2)
        // head를 1로 옮기고 새 메시지 -> 분기(2의 sibling)
        match s.step(Command::Checkout(1)) {
            StepOutcome::Print(t) => assert!(t.contains("1")),
            other => panic!("got {other:?}"),
        }
        let _ = s.step(Command::Message("다른 방향".into())); // msg 3,4 (parent=1, 분기)
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
}
