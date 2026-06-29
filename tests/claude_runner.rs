// ClaudeRunner가 claude를 spawn해 stream-json을 파싱하는지 가짜 CLI로 검증.
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::{RunInput, RunMode, Runner};

#[test]
fn claude_runner_spawns_and_parses_fixture() {
    let runner = ClaudeRunner::with_bin("tests/fixtures/fake-claude.sh");
    let input = RunInput { prompt: "이 설계 어떤가요?".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
    let out = runner.run(&input).expect("run ok");
    assert_eq!(out.content, "fixture 결론");
    assert_eq!(out.input_tokens, 11);
    assert_eq!(out.output_tokens, 22);
}
