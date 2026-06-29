---
title: "tunaRound v1 Plan 01: 프로젝트 스캐폴드 + 에이전트 러너 (Codex)"
type: plan
status: draft
priority: P0
updated_at: 2026-06-29
owner: shared
summary: Rust 프로젝트 스캐폴드 + Runner trait 경계 + Codex 러너(exec --json 스폰·JSONL 파싱·dedup·read/write 모드 분리). 순수함수 우선 TDD. 후속 plan(Claude 러너, 오케스트레이터, 영속, REPL)의 기초.
---

# tunaRound v1 Plan 01: 프로젝트 스캐폴드 + 에이전트 러너 (Codex) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Codex CLI를 한 턴 구동해 텍스트·토큰을 돌려주는 러너를, 가짜 CLI fixture로 검증 가능한 형태로 만든다.

**Architecture:** 코어는 framework-independent. `Runner` trait가 엔진 경계이고, `CodexRunner`가 첫 구현이다. CLI 출력 파싱은 spawn과 분리된 순수함수(`parse_codex_stream`, `push_agent_text_dedup`, `build_codex_args`)라 spawn 없이 단위테스트한다. 동기 `std::process`(tokio 미사용 - v1은 순차).

**Tech Stack:** Rust 2021, `serde`/`serde_json`. 외부 프로세스 = `codex exec --json`.

> 규율: docs/reference/development-guidelines.md (TDD red->green, 선제 설계 5규칙, 품질 셀프리뷰). 설계 근거: docs/design/tunaRound-v1-design_2026-06-29.md. 포팅 출처: tunaFlow `src-tauri/src/agents/codex.rs`(실측 완료).

---

## 범위

- **포함:** cargo 스캐폴드 / 도메인 타입(RunInput·RunOutput·RunMode·RunError) / Runner trait / Codex argv·파싱·dedup 순수함수 / CodexRunner 통합(가짜 CLI fixture 테스트).
- **비포함(후속 plan):** Claude 러너(Plan 02, stream-json NDJSON) / idle watchdog hardening / 오케스트레이터·영속·REPL.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | 크레이트 메타 + deps(serde, serde_json) |
| `src/main.rs` | 진입점(v1 동안 스모크용 최소) |
| `src/runner/mod.rs` | 도메인 타입 + `Runner` trait (엔진 경계) |
| `src/runner/codex.rs` | Codex argv·dedup·파싱 순수함수 + `CodexRunner` |
| `tests/fixtures/fake-codex.sh` | stdin 무시, 고정 JSONL 출력하는 가짜 CLI |

> 선제 설계: 도메인 개념(RunMode 등)을 처음부터 타입으로. 파싱은 순수함수로 추출. 코어는 프론트/transport import 0.

---

### Task 1: 프로젝트 스캐폴드 + 도메인 타입 + Runner trait

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `src/runner/mod.rs`

- [ ] **Step 1: cargo 프로젝트 초기화**

Run: `cargo init --name tunaround`
그 다음 `Cargo.toml`의 `[dependencies]`를 아래로 채운다.

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

- [ ] **Step 2: 도메인 타입 + trait 작성 (`src/runner/mod.rs`)**

```rust
// 에이전트 CLI를 한 턴 구동하고 결과를 돌려주는 러너 레이어의 경계.

pub mod codex;

/// 한 턴 입력. v1은 매 턴 전사를 prompt에 주입하므로 resume 토큰은 두지 않는다.
#[derive(Debug, Clone)]
pub struct RunInput {
    pub prompt: String,
    pub model: Option<String>,
    pub project_path: Option<String>,
    pub mode: RunMode,
}

/// 말하기 턴(읽기 전용) vs 사람이 지목한 쓰기 턴. spec §5 쓰기 하드 분리.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    ReadOnly,
    Write,
}

/// 한 턴 출력.
#[derive(Debug, Clone, PartialEq)]
pub struct RunOutput {
    pub content: String,
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum RunError {
    Spawn(String),
    Io(String),
    Empty(String),
}

/// 엔진 경계. 오케스트레이터는 concrete 엔진이 아니라 이 trait에 의존한다.
pub trait Runner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError>;
}
```

- [ ] **Step 3: 컴파일 확인(스모크)**

Run: `cargo build`
Expected: PASS (경고는 허용, 에러 0).

- [ ] **Step 4: 커밋**

```bash
git add Cargo.toml Cargo.lock src/main.rs src/runner/mod.rs
git commit -m "feat(runner): 프로젝트 스캐폴드 + 도메인 타입 + Runner trait"
```

---

### Task 2: dedup 순수함수 (Codex agent_message 중복 제거)

**Files:**
- Create: `src/runner/codex.rs`
- Test: `src/runner/codex.rs` (모듈 내 `#[cfg(test)]`)

- [ ] **Step 1: 실패 테스트 작성 (`src/runner/codex.rs` 하단)**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_skips_exact_duplicate() {
        let mut v = vec!["hello".to_string()];
        push_agent_text_dedup(&mut v, "hello");
        assert_eq!(v, vec!["hello"]);
    }

    #[test]
    fn dedup_replaces_when_incoming_extends_prefix() {
        let mut v = vec!["hello".to_string()];
        push_agent_text_dedup(&mut v, "hello world");
        assert_eq!(v, vec!["hello world"]);
    }

    #[test]
    fn dedup_replaces_when_long_last_is_contained() {
        let long = "x".repeat(40);
        let mut v = vec![long.clone()];
        push_agent_text_dedup(&mut v, &format!("prefix {long}"));
        assert_eq!(v, vec![format!("prefix {long}")]);
    }

    #[test]
    fn dedup_appends_distinct() {
        let mut v = vec!["a".to_string()];
        push_agent_text_dedup(&mut v, "b");
        assert_eq!(v, vec!["a", "b"]);
    }
}
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib codex::tests::dedup`
Expected: FAIL (컴파일 에러 — `push_agent_text_dedup` 미정의).

- [ ] **Step 3: 최소 구현 (`src/runner/codex.rs` 상단)**

```rust
// Codex exec --json argv·파싱·dedup 순수함수 + CodexRunner.

use super::{RunError, RunInput, RunMode, RunOutput, Runner};

/// Codex는 한 턴에 agent_message를 여러 번 emit한다(reasoning 후 재방출).
/// 정확 중복은 skip, prefix 확장이면 교체, 긴(>=40) 직전이 포함되면 교체, 그 외 append.
fn push_agent_text_dedup(texts: &mut Vec<String>, incoming: &str) {
    let trimmed = incoming.trim();
    if trimmed.is_empty() {
        return;
    }
    if let Some(last) = texts.last() {
        let last_tr = last.trim().to_string();
        if last_tr == trimmed {
            return;
        }
        if trimmed.starts_with(&last_tr) && trimmed.len() > last_tr.len() {
            *texts.last_mut().unwrap() = incoming.to_string();
            return;
        }
        if last_tr.len() >= 40 && trimmed.contains(&last_tr) {
            *texts.last_mut().unwrap() = incoming.to_string();
            return;
        }
    }
    texts.push(incoming.to_string());
}
```

또한 `src/runner/codex.rs`가 모듈로 잡히도록 `src/runner/mod.rs`에 `pub mod codex;`가 있는지 확인(Task 1 Step 2에 포함됨).

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib codex::tests::dedup`
Expected: PASS (4개).

- [ ] **Step 5: 커밋**

```bash
git add src/runner/codex.rs
git commit -m "feat(runner): Codex agent_message dedup 순수함수"
```

---

### Task 3: Codex JSONL 파서 순수함수

**Files:**
- Modify: `src/runner/codex.rs`

- [ ] **Step 1: 실패 테스트 추가 (`codex.rs`의 `mod tests`)**

```rust
    #[test]
    fn parse_extracts_agent_message_and_tokens() {
        let stdout = concat!(
            r#"{"type":"thread.started"}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"설계 의견입니다."}}"#, "\n",
            r#"{"type":"turn.completed","usage":{"input_tokens":12,"output_tokens":34}}"#, "\n",
        );
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "설계 의견입니다.");
        assert_eq!(out.input_tokens, 12);
        assert_eq!(out.output_tokens, 34);
    }

    #[test]
    fn parse_falls_back_on_non_json_line() {
        let stdout = "그냥 텍스트 한 줄\n";
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "그냥 텍스트 한 줄");
    }

    #[test]
    fn parse_dedups_repeated_agent_message() {
        let stdout = concat!(
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"답"}}"#, "\n",
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"답 확장"}}"#, "\n",
        );
        let out = parse_codex_stream(stdout);
        assert_eq!(out.content, "답 확장");
    }
```

- [ ] **Step 2: 테스트 실패 확인**

Run: `cargo test --lib codex::tests::parse`
Expected: FAIL (`parse_codex_stream` 미정의).

- [ ] **Step 3: 최소 구현 (`codex.rs`, dedup 함수 아래)**

```rust
/// Codex `exec --json` JSONL에서 (본문, 토큰)을 추출한다.
/// item.completed+agent_message → 본문(dedup), turn.completed → 토큰 누적,
/// 비-JSON 라인은 plain text fallback. 그 외 이벤트는 무시.
pub(crate) fn parse_codex_stream(stdout: &str) -> RunOutput {
    let mut texts: Vec<String> = Vec::new();
    let mut input_tokens: i64 = 0;
    let mut output_tokens: i64 = 0;

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(event) = serde_json::from_str::<serde_json::Value>(line) else {
            push_agent_text_dedup(&mut texts, line);
            continue;
        };
        match event.get("type").and_then(|v| v.as_str()).unwrap_or("") {
            "item.completed" => {
                if let Some(item) = event.get("item") {
                    if item.get("type").and_then(|v| v.as_str()) == Some("agent_message") {
                        if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                            if !text.is_empty() {
                                push_agent_text_dedup(&mut texts, text);
                            }
                        }
                    }
                }
            }
            "turn.completed" => {
                if let Some(usage) = event.get("usage") {
                    input_tokens += usage.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                    output_tokens += usage.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }
            _ => {}
        }
    }

    RunOutput {
        content: texts.join("\n\n").trim().to_string(),
        input_tokens,
        output_tokens,
    }
}
```

- [ ] **Step 4: 테스트 통과 확인**

Run: `cargo test --lib codex::tests::parse`
Expected: PASS (3개).

- [ ] **Step 5: 커밋**

```bash
git add src/runner/codex.rs
git commit -m "feat(runner): Codex JSONL 파서 순수함수"
```

---

### Task 4: Codex argv 빌더 (read/write 모드 분리)

**Files:**
- Modify: `src/runner/codex.rs`

- [ ] **Step 1: codex 샌드박스 플래그 검증(실측)**

Run: `codex exec --help`
확인: read-only 샌드박스 지정 방식. 본 plan은 `--sandbox read-only`(read), `--full-auto`(write)를 가정한다. `--help` 출력이 다르면 아래 코드·테스트의 플래그를 실제 값으로 맞춘 뒤 진행(추측 금지, 규율 #10).

- [ ] **Step 2: 실패 테스트 추가 (`codex.rs`의 `mod tests`)**

```rust
    #[test]
    fn args_write_mode_uses_full_auto() {
        let input = RunInput {
            prompt: "p".into(),
            model: None,
            project_path: None,
            mode: RunMode::Write,
        };
        let args = build_codex_args(&input);
        assert!(args.contains(&"--full-auto".to_string()));
        assert_eq!(args.last().unwrap(), "-"); // prompt via stdin
    }

    #[test]
    fn args_readonly_mode_uses_sandbox_readonly() {
        let input = RunInput {
            prompt: "p".into(),
            model: Some("gpt-x".into()),
            project_path: None,
            mode: RunMode::ReadOnly,
        };
        let args = build_codex_args(&input);
        let joined = args.join(" ");
        assert!(joined.contains("--sandbox read-only"));
        assert!(joined.contains("--model gpt-x"));
    }
```

- [ ] **Step 3: 테스트 실패 확인**

Run: `cargo test --lib codex::tests::args`
Expected: FAIL (`build_codex_args` 미정의).

- [ ] **Step 4: 최소 구현 (`codex.rs`)**

```rust
/// `codex exec` argv 조립. 모드에 따라 샌드박스 권한을 분리한다(spec §5 쓰기 하드 분리).
/// 프롬프트는 stdin(`-`)으로 전달하므로 argv에 넣지 않는다.
fn build_codex_args(input: &RunInput) -> Vec<String> {
    let mut args: Vec<String> = vec![
        "exec".into(),
        "--json".into(),
        "--skip-git-repo-check".into(),
        "--color=never".into(),
    ];
    match input.mode {
        RunMode::Write => args.push("--full-auto".into()),
        RunMode::ReadOnly => {
            args.push("--sandbox".into());
            args.push("read-only".into());
        }
    }
    if let Some(model) = &input.model {
        args.push("--model".into());
        args.push(model.clone());
    }
    args.push("-".into());
    args
}
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test --lib codex::tests::args`
Expected: PASS (2개).

- [ ] **Step 6: 커밋**

```bash
git add src/runner/codex.rs
git commit -m "feat(runner): Codex argv 빌더 (read/write 모드 분리)"
```

---

### Task 5: CodexRunner 통합 (가짜 CLI fixture로 spawn 검증)

**Files:**
- Modify: `src/runner/codex.rs`
- Create: `tests/fixtures/fake-codex.sh`
- Create: `tests/codex_runner.rs`

- [ ] **Step 1: 가짜 CLI fixture 작성 (`tests/fixtures/fake-codex.sh`)**

```bash
#!/usr/bin/env bash
# stdin(프롬프트)을 버리고 고정 JSONL을 stdout으로 낸다. 러너 spawn/파싱 검증용.
cat > /dev/null
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"fixture 응답"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":5,"output_tokens":7}}'
```

Run: `chmod +x tests/fixtures/fake-codex.sh`

- [ ] **Step 2: 실패 테스트 작성 (`tests/codex_runner.rs`)**

```rust
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
```

- [ ] **Step 3: 테스트 실패 확인**

Run: `cargo test --test codex_runner`
Expected: FAIL (`CodexRunner`/`with_bin` 미정의, `tunaround::runner::codex` 비공개).

- [ ] **Step 4: 최소 구현 (`codex.rs`) + 모듈 공개**

`src/main.rs`가 라이브러리 심볼을 노출하도록, `src/lib.rs`를 만들어 `pub mod runner;`를 두고 `Cargo.toml`에 lib/bin을 함께 둔다(통합테스트가 `tunaround::`로 접근). `src/lib.rs`:

```rust
// tunaround 라이브러리 루트. 통합테스트·바이너리가 공유하는 모듈 공개.
pub mod runner;
```

`src/runner/mod.rs`의 `pub mod codex;`는 유지하고, `codex.rs`에 러너 구조체를 추가한다.

```rust
use std::io::{Read, Write};
use std::process::{Command, Stdio};

/// Codex CLI 러너. `bin`은 실행 파일 경로(테스트는 가짜 스크립트 주입).
pub struct CodexRunner {
    bin: String,
}

impl CodexRunner {
    pub fn new() -> Self {
        Self { bin: "codex".to_string() }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string() }
    }
}

impl Default for CodexRunner {
    fn default() -> Self {
        Self::new()
    }
}

impl Runner for CodexRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let mut cmd = Command::new(&self.bin);
        cmd.args(build_codex_args(input));
        if let Some(dir) = &input.project_path {
            cmd.current_dir(dir);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| RunError::Spawn(format!("codex spawn 실패 ({}): {e}", self.bin)))?;

        // 프롬프트를 stdin으로 흘리고 닫는다(별 스레드 - 큰 prompt 데드락 회피).
        if let Some(mut stdin) = child.stdin.take() {
            let bytes = input.prompt.clone().into_bytes();
            std::thread::spawn(move || {
                let _ = stdin.write_all(&bytes);
            });
        }

        let mut stdout = String::new();
        if let Some(mut pipe) = child.stdout.take() {
            pipe.read_to_string(&mut stdout)
                .map_err(|e| RunError::Io(format!("codex stdout 읽기 실패: {e}")))?;
        }
        let mut stderr = String::new();
        if let Some(mut pipe) = child.stderr.take() {
            let _ = pipe.read_to_string(&mut stderr);
        }
        let status = child
            .wait()
            .map_err(|e| RunError::Io(format!("codex wait 실패: {e}")))?;

        if !status.success() {
            let detail = if stderr.trim().is_empty() {
                format!("exit {:?}", status.code())
            } else {
                stderr.trim().to_string()
            };
            return Err(RunError::Spawn(format!("codex 실패: {detail}")));
        }

        let out = parse_codex_stream(&stdout);
        if out.content.is_empty() {
            return Err(RunError::Empty("codex 응답 없음".into()));
        }
        Ok(out)
    }
}
```

- [ ] **Step 5: 테스트 통과 확인**

Run: `cargo test --test codex_runner`
Expected: PASS.

- [ ] **Step 6: 전체 테스트 + 빌드 확인(규율 #8)**

Run: `cargo test`
Expected: 전체 PASS.
Run: `cargo build`
Expected: 에러 0.

- [ ] **Step 7: 커밋**

```bash
git add src/lib.rs src/runner/codex.rs tests/fixtures/fake-codex.sh tests/codex_runner.rs
git commit -m "feat(runner): CodexRunner spawn + 가짜 CLI fixture 통합테스트"
```

---

## Self-Review (작성자 체크)

- **spec 커버리지:** Plan 01은 spec §3 러너 유닛 + §9-1(stream 견고화: 비-JSON fallback·dedup)·§9-2(쓰기 하드 분리: RunMode)를 다룬다. Claude 러너(§10 tunaFlow claude.rs NDJSON)·idle watchdog은 Plan 02/후속.
- **placeholder:** 없음(모든 코드 단계에 실제 코드). 단 Task 4 Step 1은 codex 샌드박스 플래그를 `--help`로 검증하는 실행 단계(추측 금지) — placeholder 아님.
- **타입 일관성:** RunInput/RunOutput/RunMode/RunError/Runner를 Task 1에서 정의하고 Task 2~5에서 동일 시그니처로 사용. `parse_codex_stream`/`build_codex_args`/`push_agent_text_dedup`/`CodexRunner::{new,with_bin}` 이름 일관.
- **선제 설계 적용:** RunMode를 처음부터 타입으로 / 파싱을 순수함수로 추출 / Runner trait 경계 / 코어 프론트·transport import 0.

## 다음 plan (예정)

- **Plan 02:** Claude 러너(stream-json NDJSON, StreamLine 파싱, INV-3 토큰 fallback) + idle watchdog hardening.
- **Plan 03:** 토론 오케스트레이터(RoundtableParticipant·build_round_prompt 순수함수·드라이빙 루프·consensus carry-forward·자리/쓰기 지목).
- **Plan 04:** 전사·영속(트리-ready 메시지 id/parent + 결과 문서).
- **Plan 05:** thin REPL 프론트(헤드리스 코어 경계 뒤).
