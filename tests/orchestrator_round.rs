// run_round가 자리들을 구동하고 전사를 누적하는지 FakeRunner로 검증(실 CLI 없음).
use tunaround::orchestrator::{run_round, MapRegistry, Participant, Utterance};
use tunaround::runner::{RunError, RunInput, RunOutput, Runner};

/// 고정 응답을 내는 가짜 러너.
struct FakeRunner {
    reply: String,
}
impl Runner for FakeRunner {
    fn run(&self, _input: &RunInput) -> Result<RunOutput, RunError> {
        Ok(RunOutput { content: self.reply.clone(), input_tokens: 0, output_tokens: 0 })
    }
}

#[test]
fn run_round_drives_seats_and_accumulates_transcript() {
    let mut reg = MapRegistry::new();
    reg.insert("claude", Box::new(FakeRunner { reply: "아키텍트 의견".into() }));
    reg.insert("codex", Box::new(FakeRunner { reply: "리뷰어 의견".into() }));

    let participants = vec![
        Participant { engine: "claude".into(), role: Some("architect".into()), instruction: String::new() },
        Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
    ];
    let mut transcript: Vec<Utterance> = Vec::new();

    let round = run_round(&participants, &mut transcript, "이 설계 어떤가요?", &reg).expect("ok");

    assert_eq!(round.len(), 2);
    assert_eq!(round[0].content, "아키텍트 의견");
    assert_eq!(round[1].content, "리뷰어 의견");
    assert_eq!(transcript.len(), 2);
}
// 순차-인지(2번째 자리가 1번째 응답을 봄)는 Task 2 prompt.rs 단위테스트가 증명한다.
