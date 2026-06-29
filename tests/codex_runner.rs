// CodexRunner가 실제 프로세스를 spawn해 stdout JSONL을 파싱하는지 가짜 CLI로 검증.
use tunaround::runner::codex::CodexRunner;
use tunaround::runner::{RunInput, RunMode, Runner};

#[test]
fn codex_runner_spawns_and_parses_fixture() {
    let runner = CodexRunner::with_bin("tests/fixtures/fake-codex.sh");
    let input = RunInput {
        prompt: "이 설계 어떤가요?".into(),
        model: None,
        project_path: None,
        mode: RunMode::ReadOnly,
    };
    let out = runner.run(&input).expect("run ok");
    assert_eq!(out.content, "fixture 응답");
    assert_eq!(out.input_tokens, 5);
    assert_eq!(out.output_tokens, 7);
}
