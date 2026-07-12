// ClaudeRunner가 claude를 spawn해 stream-json을 파싱하는지 가짜 CLI로 검증.
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::{RunInput, RunMode, Runner};

// 가짜 claude CLI 경로를 OS별로 돌려준다. Unix는 커밋된 .sh, Windows는 동일 NDJSON을 흘리는 .cmd를 tmp에 생성.
#[cfg(unix)]
fn fixture_bin() -> String {
    "tests/fixtures/fake-claude.sh".to_string()
}
#[cfg(windows)]
fn fixture_bin() -> String {
    let dir = std::env::temp_dir();
    let ndjson = dir.join("tuna_fake_claude.ndjson");
    std::fs::write(
        &ndjson,
        "{\"type\":\"system\"}\n{\"type\":\"result\",\"result\":\"fixture 결론\",\"total_input_tokens\":11,\"total_output_tokens\":22}\n",
    )
    .unwrap();
    // .cmd엔 ASCII만 담고 NDJSON은 %~dp0(cmd 런타임 확장)로 참조해 한글 tmp 경로 인코딩 문제를 피한다.
    let cmd = dir.join("tuna_fake_claude.cmd");
    std::fs::write(&cmd, "@type \"%~dp0tuna_fake_claude.ndjson\"\r\n").unwrap();
    cmd.to_str().unwrap().to_string()
}

#[test]
fn claude_runner_spawns_and_parses_fixture() {
    let runner = ClaudeRunner::with_bin(&fixture_bin());
    let input = RunInput {
        prompt: "이 설계 어떤가요?".into(),
        mode: RunMode::ReadOnly,
        ..Default::default()
    };
    let out = runner.run(&input).expect("run ok");
    assert_eq!(out.content, "fixture 결론");
    assert_eq!(out.input_tokens, 11);
    assert_eq!(out.output_tokens, 22);
}
