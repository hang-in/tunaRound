// run_round가 자리들을 구동하고 라운드 응답을 반환하는지 FakeRunner로 검증(실 CLI 없음).
// transcript append는 호출자(Session::append_round) 책임이므로 여기서는 검증하지 않는다.
use tunaround::orchestrator::{
    ContextMode, MapRegistry, Participant, RoundInput, Utterance, run_round,
};
use tunaround::runner::{RunError, RunInput, RunMode, RunOutput, Runner};

/// 고정 응답을 내는 가짜 러너.
struct FakeRunner {
    reply: String,
}
impl Runner for FakeRunner {
    fn run(&self, _input: &RunInput) -> Result<RunOutput, RunError> {
        Ok(RunOutput {
            content: self.reply.clone(),
            input_tokens: 0,
            output_tokens: 0,
        })
    }
}

#[test]
fn run_round_drives_seats_and_returns_utterances() {
    let mut reg = MapRegistry::new();
    reg.insert(
        "claude",
        Box::new(FakeRunner {
            reply: "아키텍트 의견".into(),
        }),
    );
    reg.insert(
        "codex",
        Box::new(FakeRunner {
            reply: "리뷰어 의견".into(),
        }),
    );

    let participants = vec![
        Participant {
            engine: "claude".into(),
            role: Some("architect".into()),
            instruction: String::new(),
        },
        Participant {
            engine: "codex".into(),
            role: Some("reviewer".into()),
            instruction: String::new(),
        },
    ];

    let prior: Vec<Utterance> = Vec::new();
    let input = RoundInput {
        prior: &prior,
        retrieved: &[],
        carried: "",
        ctx_mode: ContextMode::Push,
        transcript_len: 0,
    };
    let (round, err) = run_round(
        &participants,
        "이 설계 어떤가요?",
        &reg,
        RunMode::ReadOnly,
        input,
    );

    assert!(err.is_none(), "정상 완료라 에러 없어야 함: {err:?}");
    assert_eq!(round.len(), 2);
    assert_eq!(round[0].content, "아키텍트 의견");
    assert_eq!(round[1].content, "리뷰어 의견");
}
// 순차-인지(2번째 자리가 1번째 응답을 봄)는 Task 2 prompt.rs 단위테스트가 증명한다.
