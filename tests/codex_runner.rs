// CodexRunner가 실제 프로세스를 spawn해 stdout JSONL을 파싱하는지 가짜 CLI로 검증.
use tunaround::runner::codex::CodexRunner;
use tunaround::runner::{RunInput, RunMode, Runner};

// 가짜 codex CLI 경로를 OS별로 돌려준다. Unix는 커밋된 .sh, Windows는 동일 JSONL을 흘리는 .cmd를 tmp에 생성.
#[cfg(unix)]
fn fixture_bin() -> String {
    "tests/fixtures/fake-codex.sh".to_string()
}
#[cfg(windows)]
fn fixture_bin() -> String {
    let dir = std::env::temp_dir();
    let jsonl = dir.join("tuna_fake_codex.jsonl");
    std::fs::write(
        &jsonl,
        "{\"type\":\"item.completed\",\"item\":{\"type\":\"agent_message\",\"text\":\"fixture 응답\"}}\n{\"type\":\"turn.completed\",\"usage\":{\"input_tokens\":5,\"output_tokens\":7}}\n",
    )
    .unwrap();
    // .cmd엔 ASCII만 담고 JSONL은 %~dp0(cmd 런타임 확장)로 참조해 한글 tmp 경로 인코딩 문제를 피한다.
    let cmd = dir.join("tuna_fake_codex.cmd");
    std::fs::write(&cmd, "@type \"%~dp0tuna_fake_codex.jsonl\"\r\n").unwrap();
    cmd.to_str().unwrap().to_string()
}

#[test]
fn codex_runner_spawns_and_parses_fixture() {
    let runner = CodexRunner::with_bin(&fixture_bin());
    let input = RunInput {
        prompt: "이 설계 어떤가요?".into(),
        mode: RunMode::ReadOnly,
        ..Default::default()
    };
    let out = runner.run(&input).expect("run ok");
    assert_eq!(out.content, "fixture 응답");
    assert_eq!(out.input_tokens, 5);
    assert_eq!(out.output_tokens, 7);
}
