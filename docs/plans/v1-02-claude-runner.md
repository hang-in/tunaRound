---
title: "tunaRound v1 Plan 02: Claude 러너 (stream-json)"
type: plan
status: done
priority: P0
updated_at: 2026-06-29
owner: shared
summary: Claude Code를 `claude -p --output-format stream-json`로 구동하는 ClaudeRunner. argv(read/write 모드) + NDJSON 파서(result 라인 content + INV-3 토큰 fallback + is_error) + 가짜 CLI fixture 통합. Plan 01의 Runner trait·도메인 타입 재사용. idle watchdog은 후속 hardening plan.
---

# tunaRound v1 Plan 02: Claude 러너 (stream-json) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Claude Code를 한 턴 구동해 텍스트·토큰을 돌려주는 러너를, 가짜 CLI fixture로 검증 가능하게 만든다.

**Architecture:** Plan 01의 `Runner` trait + 도메인 타입을 그대로 재사용한다. `ClaudeRunner`가 두 번째 구현이다. Codex와 달리 프롬프트는 `-p <prompt>` argv로 전달(stdin 아님), 출력은 stream-json NDJSON이며 최종 답은 `result` 라인에 있다. 파싱은 spawn과 분리된 순수함수.

**Tech Stack:** Rust 2024, `serde_json`(Value 기반 파싱). 외부 프로세스 = `claude -p --output-format stream-json`.

> 규율: docs/reference/development-guidelines.md. 설계: docs/design/tunaRound-v1-design_2026-06-29.md §10(tunaFlow `claude.rs` 실측). 선행: Plan 01(done) - `src/runner/mod.rs`의 RunInput/RunOutput/RunMode/RunError/Runner, `src/runner/codex.rs` 패턴.

---

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v1-claude-runner` -> main 머지). 전체 17 테스트 green(15 unit + 2 integration), `cargo build`/`clippy` 클린.

- **claude 플래그(Task 1, #10).** `claude --help` 실측 결과 plan 가정이 전부 확인됨. Write=`--dangerously-skip-permissions`, ReadOnly=`--disallowedTools Write,Edit,Bash`, `--output-format stream-json`(+`-p`/`--verbose`). 교정 불필요.
- **`RunError::Agent` 추가(Task 2).** claude in-band 에러(result 라인 is_error) 모델. Codex 무영향.
- 커밋: 80ca2cb -> 032e550 -> 2b18382.

## 범위

- **포함:** `src/runner/claude.rs` - claude argv(read/write 모드) / stream-json NDJSON 파서(result 라인 → content + INV-3 토큰 fallback + is_error 처리) / `ClaudeRunner`(spawn + 파싱) / 가짜 CLI fixture 통합테스트. `RunError`에 in-band agent 에러용 변형 추가.
- **비포함:** idle watchdog(INV-4) → 후속 hardening plan(Codex 러너와 함께). 스트리밍 progress 콜백(v1 비스트리밍 동기). resume/system-prompt(v1 전사 주입).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/runner/mod.rs` | (수정) `pub mod claude;` 추가 + `RunError::Agent(String)` 변형 추가 |
| `src/runner/claude.rs` | (신규) claude argv·NDJSON 파서·`ClaudeRunner` |
| `tests/fixtures/fake-claude.sh` | (신규) 고정 stream-json NDJSON 출력 가짜 CLI |
| `tests/claude_runner.rs` | (신규) ClaudeRunner spawn/파싱 통합테스트 |

> 선제 설계: 파서는 순수함수, in-band 에러를 타입(`RunError::Agent`)으로 모델, Codex와 같은 패턴 답습.

---

### Task 1: claude argv 빌더 (read/write 모드)

**Files:**
- Create: `src/runner/claude.rs`
- Modify: `src/runner/mod.rs` (`pub mod claude;`)

- [ ] **Step 1: 실측 — claude 권한/도구 플래그 확인 (#10, 추측 금지)**

Run `claude --help` (and if present `claude -p --help`). 본 plan 가정:
- Write 모드 = `--dangerously-skip-permissions` (모든 도구 허용).
- ReadOnly 모드 = 도구를 읽기 전용으로 제한. 가정 플래그 `--disallowedTools "Write,Edit,Bash"`. **실제 `--help`에서 도구 제한 플래그명이 다르면(예: `--allowedTools`, `--permission-mode`) 실제 값으로 코드·테스트를 맞춘 뒤 진행.** claude 미설치/`--help` 실패면 가정대로 두되 Status DONE_WITH_CONCERNS로 "읽기전용 도구 제한 플래그 미검증" 명시.

- [ ] **Step 2: `src/runner/claude.rs` 생성 (#6 헤더) + 실패 테스트**

파일 첫 줄: `// Claude Code를 stream-json으로 구동하는 러너. argv·NDJSON 파서·ClaudeRunner.`
그 아래 import + 테스트:
```rust
use super::{RunInput, RunMode};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunMode;

    #[test]
    fn args_have_stream_json_and_prompt() {
        let input = RunInput { prompt: "이 설계 어떤가요?".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
        let args = build_claude_args(&input);
        let joined = args.join(" ");
        assert!(joined.contains("-p 이 설계 어떤가요?"));
        assert!(joined.contains("--output-format stream-json"));
    }

    #[test]
    fn args_write_mode_skips_permissions() {
        let input = RunInput { prompt: "p".into(), model: Some("claude-x".into()), project_path: None, mode: RunMode::Write };
        let joined = build_claude_args(&input).join(" ");
        assert!(joined.contains("--dangerously-skip-permissions"));
        assert!(joined.contains("--model claude-x"));
    }
}
```

- [ ] **Step 3: `pub mod claude;`를 `src/runner/mod.rs`에 추가** (헤더 아래, 타입 위). codex 선언 옆.

- [ ] **Step 4: 테스트 실패 확인** — `cargo test --lib claude::tests::args` → FAIL(`build_claude_args` 미정의).

- [ ] **Step 5: 구현 (claude.rs, 테스트 위)**
```rust
/// `claude -p` argv 조립. 프롬프트는 `-p <arg>`로 전달(stdin 아님).
/// 모드에 따라 도구 권한을 분리한다(쓰기 하드 분리). 실측 플래그는 Step 1 참조.
fn build_claude_args(input: &RunInput) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "-p".into(),
        input.prompt.clone(),
        "--output-format".into(),
        "stream-json".into(),
        "--verbose".into(),
    ];
    match input.mode {
        RunMode::Write => args.push("--dangerously-skip-permissions".into()),
        RunMode::ReadOnly => {
            args.push("--disallowedTools".into());
            args.push("Write,Edit,Bash".into());
        }
    }
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args
}
```
(Step 1에서 실제 플래그가 달랐으면 그 값으로.)

- [ ] **Step 6: 테스트 통과 확인 + 커밋** — `cargo test --lib claude::tests::args` PASS.
`git add src/runner/claude.rs src/runner/mod.rs && git commit -m "feat(runner): Claude argv 빌더 (read/write 모드)"` (push 금지).

> EXPECTED/OK: 비테스트 빌드에서 `build_claude_args` dead_code 경고 transient(다음 task에서 ClaudeRunner가 사용). `#[allow]` 금지.

---

### Task 2: claude stream-json NDJSON 파서 (+ RunError::Agent)

**Files:**
- Modify: `src/runner/mod.rs` (`RunError`에 `Agent(String)` 추가)
- Modify: `src/runner/claude.rs`

- [ ] **Step 1: `RunError`에 변형 추가 (`src/runner/mod.rs`)**
in-band 에이전트 에러(claude `result` 라인의 `is_error`)를 모델하기 위해 `RunError` enum에 `Agent(String)` 변형을 추가한다:
```rust
#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Spawn(String),
    Io(String),
    Empty(String),
    Agent(String),
}
```

- [ ] **Step 2: 실패 테스트 추가 (claude.rs의 `mod tests`)**
```rust
    #[test]
    fn parse_takes_result_line_content_and_tokens() {
        let stdout = concat!(
            r#"{"type":"system"}"#, "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"중간"}]}}"#, "\n",
            r#"{"type":"result","result":"최종 결론입니다.","total_input_tokens":10,"total_output_tokens":20}"#, "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.content, "최종 결론입니다.");
        assert_eq!(out.input_tokens, 10);
        assert_eq!(out.output_tokens, 20);
    }

    #[test]
    fn parse_token_fallback_to_nested_usage() {
        // INV-3: top-level total_*_tokens 부재 → nested usage.*_tokens
        let stdout = concat!(
            r#"{"type":"result","result":"답","usage":{"input_tokens":3,"output_tokens":4}}"#, "\n",
        );
        let out = parse_claude_stream(stdout).expect("ok");
        assert_eq!(out.input_tokens, 3);
        assert_eq!(out.output_tokens, 4);
    }

    #[test]
    fn parse_is_error_returns_agent_err() {
        let stdout = concat!(
            r#"{"type":"result","is_error":true,"result":"rate limit"}"#, "\n",
        );
        let err = parse_claude_stream(stdout).unwrap_err();
        assert_eq!(err, RunError::Agent("rate limit".into()));
    }

    #[test]
    fn parse_no_result_line_returns_empty_err() {
        let stdout = r#"{"type":"system"}"#;
        assert!(matches!(parse_claude_stream(stdout), Err(RunError::Empty(_))));
    }
```

- [ ] **Step 3: 테스트 실패 확인** — `cargo test --lib claude::tests::parse` → FAIL.

- [ ] **Step 4: 구현 (claude.rs)** — import에 `RunError`, `RunOutput` 추가(`use super::{RunError, RunInput, RunMode, RunOutput};`):
```rust
/// claude stream-json NDJSON에서 최종 결과를 뽑는다.
/// `result` 라인의 content + 토큰(INV-3: top-level total → nested usage fallback).
/// is_error → Err(Agent), result 라인 없음 → Err(Empty). 비-JSON 라인은 무시.
pub(crate) fn parse_claude_stream(stdout: &str) -> Result<RunOutput, RunError> {
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(ev) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if ev.get("type").and_then(|v| v.as_str()) != Some("result") {
            continue;
        }
        let result_text = ev.get("result").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if ev.get("is_error").and_then(|v| v.as_bool()).unwrap_or(false) {
            return Err(RunError::Agent(result_text));
        }
        let usage = ev.get("usage");
        let pick = |top: &str, nested: &str| -> i64 {
            ev.get(top).and_then(|v| v.as_i64())
                .or_else(|| usage.and_then(|u| u.get(nested)).and_then(|v| v.as_i64()))
                .unwrap_or(0)
        };
        return Ok(RunOutput {
            content: result_text,
            input_tokens: pick("total_input_tokens", "input_tokens"),
            output_tokens: pick("total_output_tokens", "output_tokens"),
        });
    }
    Err(RunError::Empty("claude result 라인 없음".into()))
}
```

- [ ] **Step 5: 통과 확인 + 커밋** — `cargo test --lib claude::tests` 전부 PASS.
`git add src/runner/mod.rs src/runner/claude.rs && git commit -m "feat(runner): Claude stream-json 파서 + RunError::Agent"` (push 금지).

---

### Task 3: ClaudeRunner 통합 (가짜 CLI fixture)

**Files:**
- Modify: `src/runner/claude.rs`
- Create: `tests/fixtures/fake-claude.sh`
- Create: `tests/claude_runner.rs`

- [ ] **Step 1: 가짜 CLI fixture (`tests/fixtures/fake-claude.sh`)**
```bash
#!/usr/bin/env bash
# 인자(프롬프트 포함)를 무시하고 고정 stream-json NDJSON을 출력. 러너 spawn/파싱 검증용.
printf '%s\n' '{"type":"system"}'
printf '%s\n' '{"type":"result","result":"fixture 결론","total_input_tokens":11,"total_output_tokens":22}'
```
그다음 `chmod +x tests/fixtures/fake-claude.sh`.

- [ ] **Step 2: 실패 통합테스트 (`tests/claude_runner.rs`)**
```rust
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
```

- [ ] **Step 3: 실패 확인** — `cargo test --test claude_runner` → FAIL(`ClaudeRunner` 미정의).

- [ ] **Step 4: 구현 (claude.rs)** — import에 `Runner` 추가. claude는 프롬프트가 argv라 stdin 불필요:
```rust
use std::io::Read;
use std::process::{Command, Stdio};

/// Claude Code 러너. `bin`은 실행 파일 경로(테스트는 가짜 스크립트).
pub struct ClaudeRunner {
    bin: String,
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self { bin: "claude".to_string() }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string() }
    }
}

impl Default for ClaudeRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for ClaudeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(build_claude_args(input));
        if let Some(dir) = &input.project_path {
            cmd.current_dir(dir);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| RunError::Spawn(format!("claude spawn 실패 ({}): {e}", self.bin)))?;

        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .map_err(|e| RunError::Io(format!("claude stdout 읽기 실패: {e}")))?;
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let status = child.wait().map_err(|e| RunError::Io(format!("claude wait 실패: {e}")))?;
        if !status.success() {
            let detail = if stderr.trim().is_empty() { format!("exit {:?}", status.code()) } else { stderr.trim().to_string() };
            return Err(RunError::Spawn(format!("claude 실패: {detail}")));
        }
        parse_claude_stream(&stdout)
    }
}
```
(import 통합: `use super::{RunError, RunInput, RunMode, RunOutput, Runner};`)

- [ ] **Step 5: 통과 + 전체 검증 (#8 §3)** — `cargo test --test claude_runner` PASS. 이어서 `cargo test`(전체: Codex + Claude), `cargo build`(경고 0, dead_code 해소), `cargo clippy`(클린).

- [ ] **Step 6: 커밋**
`git add src/runner/claude.rs tests/fixtures/fake-claude.sh tests/claude_runner.rs && git commit -m "feat(runner): ClaudeRunner spawn + 가짜 CLI fixture 통합테스트"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** Claude 러너(spec §10 tunaFlow claude.rs) + stream-json 파싱 견고화(비-JSON 라인 무시, INV-3 토큰 fallback) + 쓰기 하드 분리(RunMode). idle watchdog은 명시적 비포함(후속).
- **placeholder:** 없음. Task 1 Step 1은 `claude --help` 실측 단계(추측 금지).
- **타입 일관성:** Plan 01의 RunInput/RunOutput/RunMode/Runner 재사용. `RunError::Agent` 추가는 Task 2 Step 1에서 정의 후 파서에서 사용. `build_claude_args`/`parse_claude_stream`/`ClaudeRunner::{new,with_bin}` 일관.
- **선제 설계:** 파서 순수함수, in-band 에러를 타입으로, Codex 패턴 답습, 코어 경계 유지.

## 다음 plan

- **Plan 03:** 토론 오케스트레이터(RoundtableParticipant·build_round_prompt 순수함수·드라이빙 루프·consensus·자리/쓰기 지목). 두 러너를 `Runner` trait로 주입.
- **Hardening plan:** 양 러너 idle watchdog(INV-4) + 실 CLI 통합 스모크.
