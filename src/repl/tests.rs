// repl 모듈 최상위 단위 테스트.

use super::*;
use crate::orchestrator::MapRegistry;
use crate::runner::{RunError, RunInput, RunOutput, Runner};
use crate::store::{StoredMessage, StoredSession}; // 테스트 fixture(FakeCoreSync·seed_from)가 조립에 사용.

struct FakeRunner {
    reply: String,
}
impl Runner for FakeRunner {
    fn run(&self, _i: &RunInput) -> Result<RunOutput, RunError> {
        Ok(RunOutput {
            content: self.reply.clone(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

fn session_with_two_seats() -> Session {
    let mut reg = MapRegistry::new();
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: "제안".into(),
        }),
    );
    reg.insert(
        "codex",
        Box::new(FakeRunner {
            reply: "리뷰".into(),
        }),
    );
    let participants = vec![
        Participant {
            engine: "claude".into(),
            role: Some("proposer".into()),
            instruction: String::new(),
        },
        Participant {
            engine: "codex".into(),
            role: Some("reviewer".into()),
            instruction: String::new(),
        },
    ];
    Session::new(participants, Box::new(reg))
}

/// 공유 트리를 흉내내는 가짜 CoreSync(외부 post_turn 시뮬레이션용). DB id 권위를 모사.
#[derive(Clone)]
struct FakeCoreSync {
    db: std::sync::Arc<std::sync::Mutex<StoredSession>>,
}
impl FakeCoreSync {
    fn new() -> Self {
        Self {
            db: std::sync::Arc::new(std::sync::Mutex::new(StoredSession {
                messages: vec![],
                head: None,
            })),
        }
    }
    fn append_inner(&self, speaker: &str, content: &str) -> u64 {
        let mut db = self.db.lock().unwrap();
        let new_id = db.messages.iter().map(|m| m.id).max().unwrap_or(0) + 1;
        let parent = db.head;
        db.messages.push(StoredMessage {
            id: new_id,
            parent_id: parent,
            speaker: speaker.into(),
            content: content.into(),
        });
        db.head = Some(new_id);
        new_id
    }
    /// 다른 프론트/에이전트의 post_turn을 흉내낸다(REPL 밖에서 DB에 직접 추가).
    fn external_post(&self, speaker: &str, content: &str) -> u64 {
        self.append_inner(speaker, content)
    }
    fn len(&self) -> usize {
        self.db.lock().unwrap().messages.len()
    }
}
impl crate::orchestrator::CoreSync for FakeCoreSync {
    fn load_session(&self, _sid: &str) -> Option<crate::types::ConversationSnapshot> {
        let db = self.db.lock().unwrap();
        if db.messages.is_empty() {
            None
        } else {
            Some(db.clone().into())
        }
    }
    fn append_turn(&self, _sid: &str, speaker: &str, content: &str) -> Result<u64, String> {
        Ok(self.append_inner(speaker, content))
    }
}

/// core-sync append 실패를 흉내내는 가짜(결함 #3 테스트용). 지정한 순번(1-based)에서 실패한다.
struct FailingCoreSync {
    fail_at: usize,
    calls: std::sync::Mutex<usize>,
}
impl crate::orchestrator::CoreSync for FailingCoreSync {
    fn load_session(&self, _sid: &str) -> Option<crate::types::ConversationSnapshot> {
        None
    }
    fn append_turn(&self, _sid: &str, _speaker: &str, _content: &str) -> Result<u64, String> {
        let mut c = self.calls.lock().unwrap();
        *c += 1;
        if *c == self.fail_at {
            Err("의도된 실패".into())
        } else {
            Ok(*c as u64)
        }
    }
}

fn core_sync_session(cs: FakeCoreSync) -> Session {
    let mut reg = MapRegistry::new();
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: "제안".into(),
        }),
    );
    reg.insert(
        "codex",
        Box::new(FakeRunner {
            reply: "리뷰".into(),
        }),
    );
    let participants = vec![
        Participant {
            engine: "claude".into(),
            role: Some("proposer".into()),
            instruction: String::new(),
        },
        Participant {
            engine: "codex".into(),
            role: Some("reviewer".into()),
            instruction: String::new(),
        },
    ];
    Session::new(participants, Box::new(reg)).with_core_sync(Some(Box::new(cs)))
}

#[test]
fn parses_validity_commands() {
    assert_eq!(
        parse_command("/supersede 3"),
        Command::Supersede { id: 3, by: None }
    );
    assert_eq!(
        parse_command("/supersede 3 7"),
        Command::Supersede { id: 3, by: Some(7) }
    );
    assert_eq!(parse_command("/reject 4"), Command::Reject(4));
    assert_eq!(
        parse_command("/explain 검색 질의"),
        Command::Explain("검색 질의".into())
    );
    assert_eq!(
        parse_command("/explain"),
        Command::Message("/explain".into())
    );
    // 인자 없으면 일반 메시지로 폴스루.
    assert_eq!(
        parse_command("/supersede"),
        Command::Message("/supersede".into())
    );
    assert_eq!(
        parse_command("/reject x"),
        Command::Message("/reject x".into())
    );
}

#[test]
fn parses_annotate() {
    // 둘 다 지정(따옴표 안 공백·콤마 보존).
    assert_eq!(
        parse_command("/annotate 3 --abstraction \"핵심 결정 텍스트\" --anchors \"검색,랭킹\""),
        Command::Annotate {
            id: 3,
            abstraction: Some("핵심 결정 텍스트".into()),
            anchors: Some("검색,랭킹".into()),
        }
    );
    // abstraction만.
    assert_eq!(
        parse_command("/annotate 5 --abstraction \"요약만\""),
        Command::Annotate {
            id: 5,
            abstraction: Some("요약만".into()),
            anchors: None
        }
    );
    // anchors만.
    assert_eq!(
        parse_command("/annotate 7 --anchors \"a,b\""),
        Command::Annotate {
            id: 7,
            abstraction: None,
            anchors: Some("a,b".into())
        }
    );
    // 따옴표 없는 단일 토큰 값도 허용.
    assert_eq!(
        parse_command("/annotate 9 --anchors kiwi"),
        Command::Annotate {
            id: 9,
            abstraction: None,
            anchors: Some("kiwi".into())
        }
    );
    // id 없음 / 플래그 없음 / 빈 값은 일반 메시지로 폴스루.
    assert_eq!(
        parse_command("/annotate"),
        Command::Message("/annotate".into())
    );
    assert_eq!(
        parse_command("/annotate 3"),
        Command::Message("/annotate 3".into())
    );
    assert_eq!(
        parse_command("/annotate x --abstraction \"y\""),
        Command::Message("/annotate x --abstraction \"y\"".into())
    );
    assert_eq!(
        parse_command("/annotate 3 --abstraction \"\""),
        Command::Message("/annotate 3 --abstraction \"\"".into())
    );
    // 값 없는 --abstraction 뒤에 --anchors가 바로 오면, --anchors를 값으로 삼키지 않고 정상 파싱.
    assert_eq!(
        parse_command("/annotate 3 --abstraction --anchors \"x\""),
        Command::Annotate {
            id: 3,
            abstraction: None,
            anchors: Some("x".into())
        }
    );
    // 양쪽 다 값 없으면 일반 메시지로 폴스루(삼킴 없음).
    assert_eq!(
        parse_command("/annotate 3 --abstraction --anchors"),
        Command::Message("/annotate 3 --abstraction --anchors".into())
    );
}

/// set_validity 캡처 튜플: (session_id, msg_id, state, by).
type ValidityCapture = (String, u64, String, Option<u64>);

/// set_validity 호출을 캡처하는 가짜 sink.
struct CapturingSink {
    last: std::sync::Mutex<Option<ValidityCapture>>,
}
impl crate::orchestrator::ValiditySink for CapturingSink {
    fn set_validity(
        &self,
        sid: &str,
        msg_id: u64,
        state: &str,
        by: Option<u64>,
    ) -> Result<(), String> {
        *self.last.lock().unwrap() = Some((sid.to_string(), msg_id, state.to_string(), by));
        Ok(())
    }
}

#[test]
fn supersede_command_calls_sink_for_existing_message() {
    let sink = std::sync::Arc::new(CapturingSink {
        last: std::sync::Mutex::new(None),
    });
    // 메시지 2건 있는 세션 구성(sink 배선). by(#2)도 존재해야 검증(결함 #7)을 통과한다.
    let mut s =
        session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
    s.seed_from(
        StoredSession {
            messages: vec![
                StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "claude".into(),
                    content: "x".into(),
                },
                StoredMessage {
                    id: 2,
                    parent_id: Some(1),
                    speaker: "codex".into(),
                    content: "y".into(),
                },
            ],
            head: Some(2),
        }
        .into(),
    );
    let out = s.step(Command::Supersede { id: 1, by: Some(2) });
    assert!(matches!(out, StepOutcome::Print(_)));
    let cap = sink.last.lock().unwrap().clone();
    assert_eq!(
        cap,
        Some(("default".into(), 1, "superseded".into(), Some(2)))
    );
}

#[test]
fn supersede_missing_message_does_not_call_sink() {
    let sink = std::sync::Arc::new(CapturingSink {
        last: std::sync::Mutex::new(None),
    });
    let mut s =
        session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
    let _ = s.step(Command::Reject(99)); // 없는 id.
    assert_eq!(
        sink.last.lock().unwrap().clone(),
        None,
        "없는 발언은 sink 미호출"
    );
}

#[test]
fn supersede_missing_by_is_rejected_and_does_not_call_sink() {
    // by(대체 발언 id)가 존재하지 않으면 대상 id와 같은 방식으로 거부돼야 한다(결함 #7).
    let sink = std::sync::Arc::new(CapturingSink {
        last: std::sync::Mutex::new(None),
    });
    let mut s =
        session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
    s.seed_from(
        StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "claude".into(),
                content: "x".into(),
            }],
            head: Some(1),
        }
        .into(),
    );
    match s.step(Command::Supersede {
        id: 1,
        by: Some(99),
    }) {
        StepOutcome::Print(t) => assert!(t.contains("99"), "없는 by id 안내 불일치: {t}"),
        other => panic!("expected Print, got {other:?}"),
    }
    assert_eq!(
        sink.last.lock().unwrap().clone(),
        None,
        "존재하지 않는 by는 sink 미호출"
    );
}

#[test]
fn validity_command_without_sink_guides() {
    let mut s = session_with_two_seats(); // sink 미배선.
    match s.step(Command::Reject(1)) {
        StepOutcome::Print(t) => assert!(t.contains("--db"), "안내 불일치: {t}"),
        _ => panic!("Print 기대"),
    }
}

/// Arc<CapturingSink>를 Box<dyn ValiditySink>로 넘기기 위한 얇은 래퍼.
struct SinkHandle(std::sync::Arc<CapturingSink>);
impl crate::orchestrator::ValiditySink for SinkHandle {
    fn set_validity(
        &self,
        sid: &str,
        msg_id: u64,
        state: &str,
        by: Option<u64>,
    ) -> Result<(), String> {
        self.0.set_validity(sid, msg_id, state, by)
    }
}

/// set_annotation 캡처 튜플: (session_id, msg_id, abstraction, anchors).
type AnnotationCapture = (String, u64, Option<String>, Option<String>);

/// set_annotation 호출을 캡처하는 가짜 sink(session_id, msg_id, abstraction, anchors).
struct CapturingAnnotationSink {
    last: std::sync::Mutex<Option<AnnotationCapture>>,
}
impl crate::orchestrator::AnnotationSink for CapturingAnnotationSink {
    fn set_annotation(
        &self,
        sid: &str,
        msg_id: u64,
        abstraction: Option<&str>,
        anchors: Option<&str>,
    ) -> Result<(), String> {
        *self.last.lock().unwrap() = Some((
            sid.to_string(),
            msg_id,
            abstraction.map(str::to_string),
            anchors.map(str::to_string),
        ));
        Ok(())
    }
}
/// Arc<CapturingAnnotationSink>를 Box<dyn AnnotationSink>로 넘기기 위한 얇은 래퍼.
struct AnnotationSinkHandle(std::sync::Arc<CapturingAnnotationSink>);
impl crate::orchestrator::AnnotationSink for AnnotationSinkHandle {
    fn set_annotation(
        &self,
        sid: &str,
        msg_id: u64,
        abstraction: Option<&str>,
        anchors: Option<&str>,
    ) -> Result<(), String> {
        self.0.set_annotation(sid, msg_id, abstraction, anchors)
    }
}

#[test]
fn annotate_command_calls_sink_for_existing_message() {
    let sink = std::sync::Arc::new(CapturingAnnotationSink {
        last: std::sync::Mutex::new(None),
    });
    let mut s = session_with_two_seats()
        .with_annotation_sink(Some(Box::new(AnnotationSinkHandle(sink.clone()))));
    s.seed_from(
        StoredSession {
            messages: vec![StoredMessage {
                id: 1,
                parent_id: None,
                speaker: "claude".into(),
                content: "x".into(),
            }],
            head: Some(1),
        }
        .into(),
    );
    let out = s.step(Command::Annotate {
        id: 1,
        abstraction: Some("요약".into()),
        anchors: Some("검색,랭킹".into()),
    });
    assert!(matches!(out, StepOutcome::Print(_)));
    let cap = sink.last.lock().unwrap().clone();
    assert_eq!(
        cap,
        Some((
            "default".into(),
            1,
            Some("요약".into()),
            Some("검색,랭킹".into())
        ))
    );
}

#[test]
fn annotate_missing_message_does_not_call_sink() {
    let sink = std::sync::Arc::new(CapturingAnnotationSink {
        last: std::sync::Mutex::new(None),
    });
    let mut s = session_with_two_seats()
        .with_annotation_sink(Some(Box::new(AnnotationSinkHandle(sink.clone()))));
    let _ = s.step(Command::Annotate {
        id: 99,
        abstraction: Some("요약".into()),
        anchors: None,
    });
    assert_eq!(
        sink.last.lock().unwrap().clone(),
        None,
        "없는 발언은 sink 미호출"
    );
}

#[test]
fn annotate_command_without_sink_guides() {
    let mut s = session_with_two_seats(); // sink 미배선.
    match s.step(Command::Annotate {
        id: 1,
        abstraction: Some("요약".into()),
        anchors: None,
    }) {
        StepOutcome::Print(t) => assert!(t.contains("--db"), "안내 불일치: {t}"),
        _ => panic!("Print 기대"),
    }
}

/// 긴 발언 여러 개를 반환하는 가짜 retriever(길이 cap 테스트용).
struct LongRetriever;
impl crate::orchestrator::ContextRetriever for LongRetriever {
    fn retrieve(&self, _q: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
        Ok((0..3)
            .map(|i| Utterance::new(format!("s{i}"), "가".repeat(1200)))
            .collect())
    }
}

#[test]
fn retrieved_injection_is_capped_by_chars() {
    // 1200자 발언 3개(총 3600 > MAX_RETRIEVED_CHARS 2000) → 누적 초과 전까지만(1건).
    let s = session_with_two_seats().with_retriever(Some(Box::new(LongRetriever)));
    let got = s.retrieve_for("주제");
    assert_eq!(got.len(), 1, "글자수 cap으로 초과 발언 드롭(최소 1건 보장)");
}

#[test]
fn core_sync_round_writes_through_to_db() {
    // core-sync 모드: 라운드 발언이 DB(CoreSync)에 append되고 인메모리도 그걸 채택.
    let cs = FakeCoreSync::new();
    let mut s = core_sync_session(cs.clone());
    let _ = s.step(Command::Message("설계 논의".into()));
    // 2좌석 응답 2건이 DB에 기록.
    assert_eq!(cs.len(), 2, "라운드 발언이 DB에 써져야 함");
    assert_eq!(s.message_count(), 2, "인메모리도 DB를 채택");
}

#[test]
fn core_sync_adopts_external_post_and_does_not_clobber() {
    // 외부 post_turn(다른 프론트)이 들어와도 REPL이 다음 step에서 흡수하고, REPL 턴이 덮지 않는다.
    let cs = FakeCoreSync::new();
    let mut s = core_sync_session(cs.clone());

    // 1라운드: REPL 발언 2건(DB id 1,2).
    let _ = s.step(Command::Message("첫 주제".into()));
    assert_eq!(cs.len(), 2);

    // 외부 참가자가 post_turn으로 발언 추가(DB id 3).
    cs.external_post("remote/agent", "외부에서 추가한 발언");
    assert_eq!(cs.len(), 3);

    // 2라운드: step 시작에 adopt → 외부 발언이 prior에 들어오고, REPL 2건이 더해짐(id 4,5).
    let _ = s.step(Command::Message("이어서".into()));
    assert_eq!(cs.len(), 5, "외부 발언 보존 + REPL 2건 추가(클로버 없음)");
    // 인메모리 트리에 외부 발언이 포함되어야 한다.
    let path = s.active_path();
    assert!(
        path.iter().any(|u| u.content == "외부에서 추가한 발언"),
        "외부 post_turn이 활성 경로에 흡수되어야 함: {:?}",
        path.iter().map(|u| u.content.as_str()).collect::<Vec<_>>()
    );
}

#[test]
fn core_sync_append_failure_halts_chain_and_surfaces_warning() {
    // 2번째 append_turn 호출(codex 좌석)에서 실패 -> 1번(claude) 발언은 화면에 남고, 실패가
    // StepOutcome에 표면화되며, 3번째 이후 append는 시도되지 않는다(체인 무결성, 결함 #3).
    let cs = FailingCoreSync {
        fail_at: 2,
        calls: std::sync::Mutex::new(0),
    };
    let mut reg = MapRegistry::new();
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: "제안".into(),
        }),
    );
    reg.insert(
        "codex",
        Box::new(FakeRunner {
            reply: "리뷰".into(),
        }),
    );
    let participants = vec![
        Participant {
            engine: "claude".into(),
            role: Some("proposer".into()),
            instruction: String::new(),
        },
        Participant {
            engine: "codex".into(),
            role: Some("reviewer".into()),
            instruction: String::new(),
        },
    ];
    let mut s = Session::new(participants, Box::new(reg)).with_core_sync(Some(Box::new(cs)));
    match s.step(Command::Message("주제".into())) {
        StepOutcome::Print(t) => {
            assert!(
                t.contains("제안"),
                "먼저 완료된 발언은 화면에 남아야 함: {t}"
            );
            assert!(
                t.contains("실패"),
                "실패가 사용자 출력에 표면화돼야 함: {t}"
            );
        }
        other => panic!("expected Print, got {other:?}"),
    }
}

#[test]
fn parses_commands() {
    assert_eq!(parse_command("/quit"), Command::Quit);
    assert_eq!(parse_command("/help"), Command::Help);
    assert_eq!(
        parse_command("/save notes.md"),
        Command::Save(Some("notes.md".into()))
    );
    assert_eq!(parse_command("/save"), Command::Save(None));
    assert_eq!(
        parse_command("이 설계 어떤가요?"),
        Command::Message("이 설계 어떤가요?".into())
    );
}

#[test]
fn blank_is_noop() {
    assert_eq!(parse_command("   "), Command::Noop);
}

#[test]
fn parses_debate() {
    assert_eq!(
        parse_command("/debate 3 이 설계 괜찮나"),
        Command::Debate {
            turns: 3,
            topic: "이 설계 괜찮나".into()
        }
    );
    // 숫자 생략 -> 기본 3턴
    assert_eq!(
        parse_command("/debate 주제만"),
        Command::Debate {
            turns: 3,
            topic: "주제만".into()
        }
    );
    // 상한 clamp(최대 10)
    assert_eq!(
        parse_command("/debate 50 큰주제"),
        Command::Debate {
            turns: 10,
            topic: "큰주제".into()
        }
    );
    // 주제 없음 -> 일반 메시지로 폴스루
    assert_eq!(parse_command("/debate"), Command::Message("/debate".into()));
    assert_eq!(
        parse_command("/debate 3"),
        Command::Message("/debate 3".into())
    ); // 숫자만, 주제 없음
}

#[test]
fn render_formats_speaker_and_content() {
    let utts = vec![Utterance {
        speaker: "claude/proposer".into(),
        content: "제안".into(),
        abstraction: None,
    }];
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
    assert_eq!(
        parse_command("/conclude claude"),
        Command::Conclude(Some("claude".into()))
    );
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
    assert_eq!(
        parse_command("@codex 이거 봐줘"),
        Command::Only {
            engine: "codex".into(),
            text: "이거 봐줘".into()
        }
    );
    // @만 있고 메시지 없으면 일반 메시지로 취급
    assert_eq!(parse_command("@codex"), Command::Message("@codex".into()));
}

#[test]
fn step_only_targets_single_seat() {
    let mut s = session_with_two_seats();
    match s.step(Command::Only {
        engine: "codex".into(),
        text: "리뷰만".into(),
    }) {
        StepOutcome::Print(text) => {
            assert!(text.contains("리뷰")); // codex FakeRunner reply
            assert!(!text.contains("제안")); // claude는 응답 안 함
        }
        other => panic!("expected Print, got {other:?}"),
    }
    assert_eq!(s.transcript_len(), 1);
}

#[test]
fn step_only_unknown_engine_errors() {
    let mut s = session_with_two_seats();
    match s.step(Command::Only {
        engine: "gemini".into(),
        text: "?".into(),
    }) {
        StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
        other => panic!("expected Print, got {other:?}"),
    }
}

struct ModeEchoRunner;
impl Runner for ModeEchoRunner {
    fn run(&self, i: &RunInput) -> Result<RunOutput, RunError> {
        Ok(RunOutput {
            content: format!("mode={:?}", i.mode),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

fn session_with_mode_echo() -> Session {
    let mut reg = MapRegistry::new();
    reg.insert("codex", Box::new(ModeEchoRunner));
    let participants = vec![Participant {
        engine: "codex".into(),
        role: Some("coder".into()),
        instruction: String::new(),
    }];
    Session::new(participants, Box::new(reg))
}

#[test]
fn parses_at_engine_bang_as_write() {
    assert_eq!(
        parse_command("@codex! 이 함수 고쳐줘"),
        Command::Write {
            engine: "codex".into(),
            text: "이 함수 고쳐줘".into()
        }
    );
    // 읽기 지목은 그대로
    assert_eq!(
        parse_command("@codex 봐줘"),
        Command::Only {
            engine: "codex".into(),
            text: "봐줘".into()
        }
    );
    // bang만 있고 메시지 없으면 일반 메시지
    assert_eq!(parse_command("@codex!"), Command::Message("@codex!".into()));
}

#[test]
fn step_write_uses_write_mode_on_single_seat() {
    let mut s = session_with_mode_echo();
    match s.step(Command::Write {
        engine: "codex".into(),
        text: "고쳐줘".into(),
    }) {
        StepOutcome::Print(text) => assert!(text.contains("Write"), "got: {text}"),
        other => panic!("expected Print, got {other:?}"),
    }
    assert_eq!(s.transcript_len(), 1);
}

#[test]
fn step_only_stays_readonly() {
    let mut s = session_with_mode_echo();
    match s.step(Command::Only {
        engine: "codex".into(),
        text: "봐줘".into(),
    }) {
        StepOutcome::Print(text) => assert!(text.contains("ReadOnly"), "got: {text}"),
        other => panic!("expected Print, got {other:?}"),
    }
}

#[test]
fn step_write_unknown_engine_errors() {
    let mut s = session_with_mode_echo();
    match s.step(Command::Write {
        engine: "gemini".into(),
        text: "x".into(),
    }) {
        StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
        other => panic!("expected Print, got {other:?}"),
    }
}

#[test]
fn parses_branches_and_checkout() {
    assert_eq!(parse_command("/branches"), Command::Branches);
    assert_eq!(parse_command("/checkout 3"), Command::Checkout(3));
    assert_eq!(
        parse_command("/checkout"),
        Command::Message("/checkout".into())
    ); // 인자 없으면 일반 메시지
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
fn step_debate_runs_n_rounds_and_grows_tree() {
    let mut s = session_with_two_seats(); // claude="제안", codex="리뷰" (FakeRunner)
    match s.step(Command::Debate {
        turns: 2,
        topic: "주제".into(),
    }) {
        StepOutcome::Print(text) => {
            assert!(text.contains("라운드 1"));
            assert!(text.contains("라운드 2"));
            assert!(text.contains("제안") && text.contains("리뷰"));
        }
        other => panic!("expected Print, got {other:?}"),
    }
    // 2턴 x 2자리 = 메시지 4개(트리), active path 길이 4
    assert_eq!(s.message_count(), 4);
    assert_eq!(s.transcript_len(), 4);
}

/// 검색 질의를 캡처하는 가짜 retriever(결함 #5 테스트용).
struct CapturingRetriever {
    queries: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
}
impl crate::orchestrator::ContextRetriever for CapturingRetriever {
    fn retrieve(&self, q: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
        self.queries.lock().unwrap().push(q.to_string());
        Ok(Vec::new())
    }
}

#[test]
fn debate_uses_original_topic_for_retrieval_not_round_directive() {
    // 2라운드 이상에서도 검색 질의는 진행용 고정 지시문(round_topic)이 아니라 원래 topic이어야
    // 한다(결함 #5: 지시문이 FTS/시맨틱 질의로 새어 들어가 무관한 히트를 끌어오는 것 방지).
    let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut s = session_with_two_seats().with_retriever(Some(Box::new(CapturingRetriever {
        queries: queries.clone(),
    })));
    let _ = s.step(Command::Debate {
        turns: 3,
        topic: "원래 주제".into(),
    });
    let qs = queries.lock().unwrap();
    assert_eq!(qs.len(), 3, "라운드마다 검색 호출: {qs:?}");
    for q in qs.iter() {
        assert_eq!(
            q, "원래 주제",
            "매 라운드 검색 질의는 원래 topic 고정: {qs:?}"
        );
    }
}

#[test]
fn step_debate_stops_on_error() {
    // 첫 라운드는 OK, 이후 에러나는 시나리오는 FakeRunner로 만들기 번거로우니
    // 최소: turns=1도 정상 동작(라운드 1만)
    let mut s = session_with_two_seats();
    match s.step(Command::Debate {
        turns: 1,
        topic: "주제".into(),
    }) {
        StepOutcome::Print(text) => {
            assert!(text.contains("라운드 1") && !text.contains("라운드 2"))
        }
        other => panic!("expected Print, got {other:?}"),
    }
    assert_eq!(s.message_count(), 2);
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

#[test]
fn checkout_is_refused_in_core_sync_mode() {
    // core-sync 모드에서는 adopt_from_core가 매 명령마다 DB head로 스냅샷을 통째 교체하므로
    // checkout이 실제로는 무력하다. 조용히 "분기됩니다"라고 안내하는 대신 명시적으로 거부해야
    // 한다(결함 #2). head도 옮기지 않아야 한다.
    let cs = FakeCoreSync::new();
    let mut s = core_sync_session(cs.clone());
    let _ = s.step(Command::Message("주제".into())); // DB에 발언 2건(claude, codex).
    let path_before = s.active_path();

    match s.step(Command::Checkout(1)) {
        StepOutcome::Print(t) => {
            assert!(
                t.contains("지원하지 않"),
                "core 모드 미지원 안내가 있어야 함: {t}"
            );
            assert!(
                !t.contains("분기 전환"),
                "실제 분기 전환처럼 보이면 안 됨: {t}"
            );
        }
        other => panic!("expected Print, got {other:?}"),
    }

    // head가 그대로 유지되어 다음 메시지가 checkout(1) 기준이 아니라 기존 head 기준으로 이어진다.
    let _ = s.step(Command::Message("이어서".into()));
    let path_after = s.active_path();
    assert_eq!(
        path_after.len(),
        path_before.len() + 2,
        "checkout이 무시되고 기존 head에서 정상 진행돼야 함: {path_after:?}"
    );
}

struct FakeRetriever {
    results: Vec<Utterance>,
}
impl crate::orchestrator::ContextRetriever for FakeRetriever {
    fn retrieve(&self, _query: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
        Ok(self.results.clone())
    }
}

#[test]
fn retrieve_for_deduplicates_active_path_content() {
    let mut s = session_with_two_seats(); // claude="제안", codex="리뷰"
    let _ = s.step(Command::Message("초기 주제".into()));
    // 활성 경로에 "제안", "리뷰" 두 발언이 있다.
    let active = s.active_path();
    let dup_content = active[0].content.clone(); // "제안" - 활성경로 중복

    let retriever = FakeRetriever {
        results: vec![
            Utterance {
                speaker: "past/speaker".into(),
                content: dup_content,
                abstraction: None,
            },
            Utterance {
                speaker: "past/other".into(),
                content: "고유 맥락 발언".into(),
                abstraction: None,
            },
        ],
    };
    let s = s.with_retriever(Some(Box::new(retriever)));

    let retrieved = s.retrieve_for("테스트 쿼리");
    // 활성경로 중복("제안")은 제외하고 신규("고유 맥락 발언")만 남아야 한다.
    assert_eq!(retrieved.len(), 1, "dedup 후 1개여야 함: {:?}", retrieved);
    assert_eq!(retrieved[0].content, "고유 맥락 발언");
}

#[test]
fn retrieve_for_returns_empty_without_retriever() {
    let s = session_with_two_seats(); // retriever 없음
    let result = s.retrieve_for("어떤 주제");
    assert!(result.is_empty(), "retriever 없으면 빈 결과");
}

/// 큐레이션(v2-51) 회귀 방지: annotation(abstraction)이 달린 현재-세션 active-path 발언이
/// 검색 히트로 돌아와도, content(raw)가 활성 경로와 일치하면 dedup으로 제외돼 **이중 주입되지 않아야** 한다.
/// (표면화를 retriever content에 하면 content가 변형돼 dedup이 깨졌던 실회귀를 못박는다.)
#[test]
fn annotated_active_path_hit_is_deduped_not_double_injected() {
    struct AnnotatedRetriever {
        dup: String,
    }
    impl crate::orchestrator::ContextRetriever for AnnotatedRetriever {
        fn retrieve(&self, _q: &str, _l: usize) -> Result<Vec<Utterance>, String> {
            // finish가 실어 보내는 것과 동형: content=raw(활성 경로와 동일), abstraction=Some.
            Ok(vec![Utterance {
                speaker: "past/speaker".into(),
                content: self.dup.clone(),
                abstraction: Some("증류 요약".into()),
            }])
        }
    }
    let mut s = session_with_two_seats(); // claude="제안", codex="리뷰"
    let _ = s.step(Command::Message("초기 주제".into())); // 활성 경로에 "제안","리뷰"
    let active = s.active_path();
    let dup_content = active[0].content.clone(); // "제안"(활성 경로 발언)
    let s = s.with_retriever(Some(Box::new(AnnotatedRetriever { dup: dup_content })));
    let retrieved = s.retrieve_for("테스트 쿼리");
    assert!(
        retrieved.is_empty(),
        "annotation 달린 active-path 발언이 dedup되지 않아 이중 주입됨: {retrieved:?}"
    );
}

#[derive(Default)]
struct IdxCalls {
    persists: usize,
    last_session: String,
    last_len: usize,
}
struct FakeIndexer(std::sync::Arc<std::sync::Mutex<IdxCalls>>);
impl crate::store::indexer::MessageIndexer for FakeIndexer {
    fn persist(&self, session_id: &str, snap: &crate::types::ConversationSnapshot) {
        let mut c = self.0.lock().unwrap();
        c.persists += 1;
        c.last_session = session_id.to_string();
        c.last_len = snap.node_count();
    }
}

#[test]
fn round_persists_to_indexer_when_present() {
    let calls = std::sync::Arc::new(std::sync::Mutex::new(IdxCalls::default()));
    let mut reg = MapRegistry::new();
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: "제안".into(),
        }),
    );
    let participants = vec![Participant {
        engine: "claude".into(),
        role: Some("proposer".into()),
        instruction: String::new(),
    }];
    let mut s = Session::new_with_indexer(
        participants,
        Box::new(reg),
        "sess-i".into(),
        Some(Box::new(FakeIndexer(std::sync::Arc::clone(&calls)))),
    );
    let _ = s.step(Command::Message("주제".into()));
    let c = calls.lock().unwrap();
    assert_eq!(c.persists, 1);
    assert_eq!(c.last_session, "sess-i");
    assert_eq!(c.last_len, 1); // 1자리 1발언
}

#[test]
fn no_indexer_means_normal_behavior() {
    let mut s = session_with_two_seats(); // indexer 없음
    let _ = s.step(Command::Message("주제".into()));
    assert_eq!(s.transcript_len(), 2); // 기존 동작 불변
}

#[test]
fn parses_search() {
    assert_eq!(
        parse_command("/search 검색 시스템"),
        Command::Search("검색 시스템".into())
    );
    // 인자 없으면 일반 메시지로 폴스루(기존 명령 패턴)
    assert_eq!(parse_command("/search"), Command::Message("/search".into()));
}

#[test]
fn step_search_without_retriever_explains() {
    let mut s = session_with_two_seats(); // retriever 없음
    match s.step(Command::Search("아무거나".into())) {
        StepOutcome::Print(t) => assert!(t.contains("검색") && t.contains("--db")),
        other => panic!("got {other:?}"),
    }
}

#[test]
fn step_search_with_retriever_renders_hits() {
    // FakeRetriever(고정 Utterance 반환)로 검색 결과 렌더 확인.
    struct FakeRetriever(Vec<Utterance>);
    impl crate::orchestrator::ContextRetriever for FakeRetriever {
        fn retrieve(&self, _q: &str, _l: usize) -> Result<Vec<Utterance>, String> {
            Ok(self.0.clone())
        }
    }
    let hits = vec![Utterance {
        speaker: "claude/proposer".into(),
        content: "검색 시스템 설계".into(),
        abstraction: None,
    }];
    let mut s = session_with_two_seats().with_retriever(Some(Box::new(FakeRetriever(hits))));
    match s.step(Command::Search("검색".into())) {
        StepOutcome::Print(t) => {
            assert!(t.contains("검색 시스템 설계"));
            assert!(t.contains("claude/proposer"));
        }
        other => panic!("got {other:?}"),
    }
}

#[test]
fn prior_for_prompt_uncapped_by_default() {
    let mut s = session_with_two_seats();
    let _ = s.step(Command::Message("주제1".into())); // 발언 2개
    let _ = s.step(Command::Message("주제2".into())); // 총 4개
    // 기본(None) = prior_for_prompt가 활성 경로 전체와 길이 동일.
    assert_eq!(s.prior_for_prompt().len(), s.transcript_len());
}

#[test]
fn prior_for_prompt_caps_to_recent_n() {
    let mut s = session_with_two_seats().with_recent_turns(Some(2));
    let _ = s.step(Command::Message("주제1".into()));
    let _ = s.step(Command::Message("주제2".into())); // 활성 경로 4턴
    let prior = s.prior_for_prompt();
    assert_eq!(prior.len(), 2); // 최근 2턴만 재주입
    // 마지막 발언이 활성 경로 전체의 마지막 발언과 동일해야 한다.
    let full = s.active_path_pub_for_test();
    assert_eq!(
        prior.last().map(|u| &u.content),
        full.last().map(|u| &u.content)
    );
}

// --- carry_forward_digest 테스트 ---

#[test]
fn carry_forward_digest_empty_when_no_cap() {
    // recent_turns None(기본) -> 드롭 없음 -> 빈 문자열.
    let s = session_with_two_seats();
    assert_eq!(s.carry_forward_digest(), "");
}

#[test]
fn carry_forward_digest_empty_when_path_not_exceeded() {
    // recent_turns=Some(4), 발언 2개(path 2) -> path<=n -> 빈 문자열.
    let mut s = session_with_two_seats().with_recent_turns(Some(4));
    let _ = s.step(Command::Message("주제".into())); // path 길이 2
    assert_eq!(s.carry_forward_digest(), "");
}

#[test]
fn carry_forward_digest_includes_dropped_speaker_and_gist() {
    // recent_turns=Some(2), 두 번 Message -> path=4, 드롭=2(path[..2]).
    // 드롭된 발언의 speaker와 gist가 다이제스트에 포함돼야 한다.
    let mut s = session_with_two_seats().with_recent_turns(Some(2));
    let _ = s.step(Command::Message("주제1".into())); // path 2
    let _ = s.step(Command::Message("주제2".into())); // path 4, 드롭 2
    let digest = s.carry_forward_digest();
    assert!(!digest.is_empty(), "드롭 존재 -> 비어있으면 안 됨");
    // claude/proposer="제안", codex/reviewer="리뷰" 중 하나는 포함돼야 한다.
    assert!(
        digest.contains("claude/proposer") || digest.contains("codex/reviewer"),
        "speaker 없음: {digest}"
    );
    assert!(
        digest.contains("제안") || digest.contains("리뷰"),
        "gist 없음: {digest}"
    );
}

#[test]
fn with_context_mode_pull_does_not_break_step() {
    // with_context_mode(Pull) 후 step이 정상 동작하는지(스모크). FakeRunner 엔진이므로 동작 동일.
    let mut s = session_with_two_seats().with_context_mode(crate::orchestrator::ContextMode::Pull);
    match s.step(Command::Message("테스트".into())) {
        StepOutcome::Print(text) => {
            assert!(
                text.contains("제안") || text.contains("리뷰"),
                "출력 없음: {text}"
            );
        }
        other => panic!("expected Print, got {other:?}"),
    }
    assert_eq!(s.transcript_len(), 2);
}

#[test]
fn default_context_mode_is_push() {
    // 기본(미설정) context_mode는 Push여야 한다.
    let s = session_with_two_seats();
    assert_eq!(s.context_mode, crate::orchestrator::ContextMode::Push);
}

#[test]
fn carry_forward_digest_caps_at_max_carry() {
    // 긴 응답을 내는 러너로 캡 초과 시나리오 구성.
    // recent_turns=Some(1), 10번 Message -> path=20, 드롭=19 -> 각 라인 ~100자 합계 ~1900 > 1500.
    let mut reg = MapRegistry::new();
    let long_reply = "A".repeat(200);
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: long_reply.clone(),
        }),
    );
    reg.insert("codex", Box::new(FakeRunner { reply: long_reply }));
    let parts = vec![
        Participant {
            engine: "claude".into(),
            role: Some("proposer".into()),
            instruction: String::new(),
        },
        Participant {
            engine: "codex".into(),
            role: Some("reviewer".into()),
            instruction: String::new(),
        },
    ];
    let mut s = Session::new(parts, Box::new(reg)).with_recent_turns(Some(1));
    for _ in 0..10 {
        let _ = s.step(Command::Message("주제".into()));
    }
    let digest = s.carry_forward_digest();
    assert!(digest.contains("이전"), "생략 표기 없음: {digest}");
    assert!(
        digest.len() <= super::MAX_CARRY,
        "MAX_CARRY 초과: {} > {}",
        digest.len(),
        super::MAX_CARRY
    );
}

#[test]
fn carry_forward_digest_pull_summarizes_whole_path() {
    // Pull 모드: recent_turns 없이도 전사 전체를 요약(안전망). Push 기본과 대비.
    use crate::orchestrator::ContextMode;
    let mut s = session_with_two_seats().with_context_mode(ContextMode::Pull);
    let _ = s.step(Command::Message("주제1".into())); // path 2
    let digest = s.carry_forward_digest();
    assert!(!digest.is_empty(), "pull 모드는 전사 전체를 요약해야 함");
    assert!(
        digest.contains("claude/proposer") || digest.contains("codex/reviewer"),
        "speaker 없음: {digest}"
    );
    // 같은 전사라도 Push(미캡)면 빈 문자열이어야 한다(대조).
    let mut s2 = session_with_two_seats();
    let _ = s2.step(Command::Message("주제1".into()));
    assert_eq!(s2.carry_forward_digest(), "", "push 미캡은 빈 요약");
}
