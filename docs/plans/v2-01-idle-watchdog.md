---
title: "tunaRound v2 Plan 01: idle watchdog (INV-4)"
type: plan
status: done
priority: P0
updated_at: 2026-06-29
owner: shared
summary: 동기 러너의 read_to_string 무한 블로킹(에이전트 행)을 라인 단위 읽기 + 활동 타이머 + watchdog 스레드로 막는다. 공유 헬퍼 src/runner/exec.rs로 추출(DRY, watchdog 단일 출처), RunError::Timeout 추가, stderr 동시 배수로 pipe-buffer 데드락도 제거. 기본 600s(INV-4), 테스트용 주입 가능. 신규 의존성 0(std만).
---

# tunaRound v2 Plan 01: idle watchdog Implementation Plan

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v2-idle-watchdog` -> main). 43 테스트 green(기존 33 + exec 신규 3 + 러너 타임아웃 통합 2 + 통합 5), `cargo build`/`clippy` 경고 0. Opus 리뷰: 계획서와 정확히 일치, 큰 문제 없음.

- Task 1: `src/runner/exec.rs` 공유 watchdog 헬퍼 + `RunError::Timeout` (커밋 `3414cf2`).
- Task 2: 양 러너를 `run_with_watchdog`로 배선, `idle_timeout` 필드(기본 600s) + `with_idle_timeout` (커밋 `78dd033`).
- 러너 타임아웃 통합 테스트 2개 안정적 PASS(`#[ignore]` 불필요). stderr 동시 배수로 pipe-buffer 데드락도 제거.
- 사소(비차단): 타임아웃 테스트가 temp sleep 스크립트를 남김(무해).

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`). TDD red->green.

**Goal:** 에이전트 CLI가 멈춰도(무출력 행) REPL 전체가 영구 정지하지 않게 한다. 무출력이 idle_timeout(기본 600s = INV-4)을 넘으면 watchdog가 자식을 종료하고 구분된 `RunError::Timeout`을 반환한다.

**Architecture:** 현재 `ClaudeRunner::run`/`CodexRunner::run`은 `pipe.read_to_string(&mut stdout)`로 EOF까지 블로킹한다(행 지점). 양 러너의 spawn->read->wait 로직은 사실상 동일하므로 공유 헬퍼 `src/runner/exec.rs::run_with_watchdog`로 추출한다. watchdog는 한 곳에만 둔다. 차이(argv 조립/stdin 주입/파서)만 각 러너에 남긴다. 오케스트레이터/REPL은 무변경(에러 전파만, `Timeout`도 자동 전파 - exhaustive match 없음 확인됨).

**Tech Stack:** Rust 2024, std만(`std::sync`{Arc,Mutex,AtomicBool} + `std::thread` + `std::time`). 신규 의존성 0. 선행: v1 완료(Plan 01~06 done).

> 출처: tunaFlow `src-tauri/src/agents/claude.rs` L429~629(검증된 watchdog 패턴 + 2026-04-29 trailing-kill race 수정). parking_lot은 도입 안 함(std::sync로 충분).
> 규율: docs/reference/development-guidelines.md. #5 한국어 마침표, #6 파일 헤더 한 줄 주석, TDD.

---

## 범위

- **포함:** `src/runner/exec.rs` 신규(공유 watchdog 실행 헬퍼) + `RunError::Timeout` 변형 + 양 러너를 헬퍼로 배선 + `idle_timeout` 필드(기본 600s, 테스트 주입). stderr 동시 배수.
- **비포함(후속):** 프로세스-그룹 kill(고아 grandchild가 pipe를 잡는 드문 경우 - tunaFlow도 단일 PID kill, 본 plan도 단일 PID + 테스트는 `exec`로 단일 프로세스), 스트리밍 seam(라이브 TUI용), rate_limit 라인 파싱(insightStabilityPlan T1). 위험 섹션 참조.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/runner/exec.rs` | (신규) `ExecSpec` + `run_with_watchdog` + `WatchdogGuard` + `kill_pid`/`poll_interval`. spawn->stdin주입->stderr배수->stdout 라인읽기(타이머 리셋)->watchdog->에러 분류. |
| `src/runner/mod.rs` | (수정) `pub(crate) mod exec;` + `RunError::Timeout(String)` 추가. |
| `src/runner/claude.rs` | (수정) `run`을 `run_with_watchdog` 호출로, `idle_timeout` 필드 + `with_idle_timeout`. |
| `src/runner/codex.rs` | (수정) 동일. stdin=Some(prompt). |

> 선제 설계: 공유 헬퍼로 watchdog 단일 출처(중복 race 수정 방지). 파서/argv는 러너에 그대로 둔다. concrete 엔진 미의존 경계 유지.

---

### Task 1: 공유 watchdog 헬퍼 `src/runner/exec.rs` + RunError::Timeout

**Files:**
- Create: `src/runner/exec.rs`
- Modify: `src/runner/mod.rs`

- [ ] **Step 1: `RunError::Timeout` 추가 + 모듈 선언 (`src/runner/mod.rs`)**
  - `RunError` enum에 변형 추가: `Timeout(String),`
  - 파일 상단 모듈 선언에 추가: `pub(crate) mod exec;`

- [ ] **Step 2: 실패 테스트 먼저 (`src/runner/exec.rs`의 `mod tests`)**
  - 헤더 한 줄 주석(#6): `// 에이전트 자식 프로세스를 idle watchdog와 함께 구동하고 stdout를 수집하는 공유 실행 헬퍼.`
  - 아래 테스트를 먼저 작성한다(아직 `run_with_watchdog` 미구현 -> 컴파일/실패).
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn spec(args: &[&str], idle_ms: u64) -> ExecSpec {
        ExecSpec {
            bin: "sh".into(),
            args: ["-c"].iter().chain(args.iter()).map(|s| s.to_string()).collect(),
            cwd: None,
            stdin: None,
            idle_timeout: Duration::from_millis(idle_ms),
            label: "test".into(),
        }
    }

    #[test]
    fn idle_no_output_triggers_timeout() {
        // exec로 단일 프로세스(sh가 sleep로 치환) -> 단일 PID kill로 확실히 종료.
        let out = run_with_watchdog(&spec(&["exec sleep 5"], 150));
        match out {
            Err(RunError::Timeout(_)) => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn output_then_exit_succeeds_no_false_timeout() {
        // 즉시 출력 후 종료 -> 타이머 리셋되어 타임아웃 없이 stdout 수집.
        let out = run_with_watchdog(&spec(&["printf 'line1\\nline2\\n'"], 2000)).expect("ok");
        assert!(out.contains("line1"));
        assert!(out.contains("line2"));
    }

    #[test]
    fn nonzero_exit_is_spawn_error_not_timeout() {
        // 무출력이지만 즉시 비정상 종료 -> Timeout 아님(Spawn).
        let out = run_with_watchdog(&spec(&["exit 3"], 2000));
        assert!(matches!(out, Err(RunError::Spawn(_))));
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test --lib runner::exec` -> 컴파일 에러 또는 FAIL.

- [ ] **Step 4: 구현 (`src/runner/exec.rs`, tests 위에)**
```rust
use super::RunError;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 한 자식 프로세스 실행 명세. argv·stdin·작업디렉토리·idle 타임아웃·로그 라벨.
pub(crate) struct ExecSpec {
    pub bin: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub stdin: Option<String>,
    pub idle_timeout: Duration,
    pub label: String,
}

/// watchdog에 함수 scope 종료를 알려 trailing-kill race(이미 reap된 PID에 kill 송출)를 막는 RAII 가드.
struct WatchdogGuard(Arc<AtomicBool>);
impl Drop for WatchdogGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// 자식을 spawn해 idle watchdog로 감시하며 stdout를 라인 단위로 수집한다.
/// 무출력이 idle_timeout을 넘으면 자식을 kill하고 `RunError::Timeout`. 성공 시 stdout 수집본을 돌려준다.
pub(crate) fn run_with_watchdog(spec: &ExecSpec) -> Result<String, RunError> {
    let mut cmd = Command::new(&spec.bin);
    cmd.args(&spec.args);
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }
    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = cmd
        .spawn()
        .map_err(|e| RunError::Spawn(format!("{} spawn 실패 ({}): {e}", spec.label, spec.bin)))?;

    // 프롬프트 stdin 주입(별 스레드 - 큰 입력 pipe 데드락 회피).
    if let Some(input) = &spec.stdin
        && let Some(mut stdin) = child.stdin.take()
    {
        let bytes = input.clone().into_bytes();
        std::thread::spawn(move || {
            let _ = stdin.write_all(&bytes);
        });
    }

    // stderr 동시 배수(pipe-buffer 데드락 회피).
    let stderr_handle = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut s = String::new();
            let _ = pipe.read_to_string(&mut s);
            s
        })
    });

    // idle watchdog: 활동 타이머 + 폴링 스레드.
    let last_activity = Arc::new(Mutex::new(Instant::now()));
    let timed_out = Arc::new(AtomicBool::new(false));
    let watchdog_done = Arc::new(AtomicBool::new(false));
    let pid = child.id();
    let idle_timeout = spec.idle_timeout;
    let tick = poll_interval(idle_timeout);
    {
        let last_act = Arc::clone(&last_activity);
        let timed_out_w = Arc::clone(&timed_out);
        let done = Arc::clone(&watchdog_done);
        std::thread::spawn(move || {
            loop {
                std::thread::sleep(tick);
                if done.load(Ordering::SeqCst) {
                    return;
                }
                let elapsed = last_act.lock().map(|g| g.elapsed()).unwrap_or_default();
                if elapsed > idle_timeout {
                    timed_out_w.store(true, Ordering::SeqCst);
                    kill_pid(pid);
                    return;
                }
            }
        });
    }
    let _guard = WatchdogGuard(Arc::clone(&watchdog_done));

    // stdout 라인 단위 읽기, 매 라인마다 활동 타이머 리셋.
    let mut collected = String::new();
    if let Some(pipe) = child.stdout.take() {
        let reader = BufReader::new(pipe);
        for line in reader.lines() {
            let line =
                line.map_err(|e| RunError::Io(format!("{} stdout 읽기 실패: {e}", spec.label)))?;
            if let Ok(mut g) = last_activity.lock() {
                *g = Instant::now();
            }
            collected.push_str(&line);
            collected.push('\n');
        }
    }

    let stderr = stderr_handle
        .map(|h| h.join().unwrap_or_default())
        .unwrap_or_default();

    let status = child
        .wait()
        .map_err(|e| RunError::Io(format!("{} wait 실패: {e}", spec.label)))?;

    // 타임아웃을 종료 코드 검사보다 먼저 본다(kill된 자식은 비정상 종료라 Spawn으로 오분류될 수 있음).
    if timed_out.load(Ordering::SeqCst) {
        return Err(RunError::Timeout(format!(
            "{} 타임아웃: {}s 무출력으로 watchdog가 종료했습니다.",
            spec.label,
            idle_timeout.as_secs()
        )));
    }
    if !status.success() {
        let detail = if stderr.trim().is_empty() {
            format!("exit {:?}", status.code())
        } else {
            stderr.trim().to_string()
        };
        return Err(RunError::Spawn(format!("{} 실패: {detail}", spec.label)));
    }
    Ok(collected)
}

/// idle_timeout에 맞춘 watchdog 폴링 간격. 짧은 타임아웃(테스트)에도 제때 발화하도록 비례 + 캡(20ms~30s).
fn poll_interval(idle_timeout: Duration) -> Duration {
    (idle_timeout / 5).clamp(Duration::from_millis(20), Duration::from_secs(30))
}

/// 자식 PID를 best-effort로 강제 종료한다(Unix kill -9 / Windows taskkill).
fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill")
            .args(["-9", &pid.to_string()])
            .status();
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/PID", &pid.to_string()])
            .status();
    }
}
```
  - 주의: let-chain(`if let ... && let ...`)은 edition 2024라 허용(codex.rs와 동일 스타일). clippy 경고 0 유지.

- [ ] **Step 5: 통과 + 커밋** — `cargo test --lib runner::exec` 전부 PASS, `cargo clippy --all-targets` 클린.
  `git add src/runner/exec.rs src/runner/mod.rs && git commit -m "feat(runner): idle watchdog 공유 실행 헬퍼 + RunError::Timeout"` (push 금지).

---

### Task 2: 양 러너를 watchdog 헬퍼로 배선

**Files:**
- Modify: `src/runner/claude.rs`, `src/runner/codex.rs`

- [ ] **Step 1: 기존 테스트가 통과하는지 기준선 확인** — `cargo test` 전체 GREEN(파서/argv 테스트는 불변이어야 함).

- [ ] **Step 2: `ClaudeRunner` 수정 (`src/runner/claude.rs`)**
  - `use std::io::Read;` / `use std::process::{Command, Stdio};` 제거(헬퍼로 이동). 대신 `use super::exec::{run_with_watchdog, ExecSpec};` `use std::time::Duration;` 추가.
  - 구조체에 필드 추가, 생성자에서 기본 600s:
```rust
pub struct ClaudeRunner {
    bin: String,
    idle_timeout: Duration,
}

impl ClaudeRunner {
    pub fn new() -> Self {
        Self { bin: "claude".to_string(), idle_timeout: Duration::from_secs(600) }
    }
    pub fn with_bin(bin: &str) -> Self {
        Self { bin: bin.to_string(), idle_timeout: Duration::from_secs(600) }
    }
    /// 테스트/설정용 idle 타임아웃 주입.
    pub fn with_idle_timeout(mut self, d: Duration) -> Self {
        self.idle_timeout = d;
        self
    }
}
```
  - `run`을 헬퍼 호출로 교체:
```rust
impl Runner for ClaudeRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_claude_args(input),
            cwd: input.project_path.clone(),
            stdin: None,
            idle_timeout: self.idle_timeout,
            label: "claude".to_string(),
        };
        let stdout = run_with_watchdog(&spec)?;
        parse_claude_stream(&stdout)
    }
}
```

- [ ] **Step 3: `CodexRunner` 수정 (`src/runner/codex.rs`)** — 동일 패턴, stdin=Some(prompt).
  - import 정리(`use std::io::{Read, Write};` `use std::process::{Command, Stdio};` 제거 -> `use super::exec::{run_with_watchdog, ExecSpec};` `use std::time::Duration;`).
  - 필드/생성자에 `idle_timeout`(기본 600s) + `with_idle_timeout` 추가(claude와 동형).
  - `run` 교체:
```rust
impl Runner for CodexRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let spec = ExecSpec {
            bin: self.bin.clone(),
            args: build_codex_args(input),
            cwd: input.project_path.clone(),
            stdin: Some(input.prompt.clone()),
            idle_timeout: self.idle_timeout,
            label: "codex".to_string(),
        };
        let stdout = run_with_watchdog(&spec)?;
        let out = parse_codex_stream(&stdout);
        if out.content.is_empty() {
            return Err(RunError::Empty("codex 응답 없음".into()));
        }
        Ok(out)
    }
}
```

- [ ] **Step 4: 러너-수준 타임아웃 통합 테스트(각 러너 `mod tests`에 1개씩)**
  - `with_bin`이 가짜 bin을 받으므로, 타임아웃을 직접 검증하려면 sleep하는 가짜 스크립트가 필요하다. 간단히 `sh`를 bin으로 주입하되 argv가 claude/codex 형식이라 부적합 -> 대신 **exec.rs Task 1 테스트가 watchdog 동작을 이미 커버**하므로 러너 수준은 "배선이 헬퍼를 타는지"만 가볍게 확인한다. 다음 테스트를 codex.rs에 추가(claude도 동형 1개):
```rust
    #[test]
    fn runner_propagates_timeout_via_helper() {
        // codex 형식 argv로는 sh가 못 돌므로, with_bin에 무출력 sleep 스크립트를 주입한다.
        // 가짜 스크립트: 인자 무시하고 stdin 안 읽고 sleep. tmp에 작성.
        let dir = std::env::temp_dir();
        let script = dir.join("tuna_fake_sleep.sh");
        std::fs::write(&script, "#!/bin/sh\nexec sleep 5\n").unwrap();
        let _ = std::process::Command::new("chmod").args(["+x", script.to_str().unwrap()]).status();
        let r = CodexRunner::with_bin(script.to_str().unwrap())
            .with_idle_timeout(std::time::Duration::from_millis(150));
        let input = RunInput { prompt: "x".into(), model: None, project_path: None, mode: RunMode::ReadOnly };
        assert!(matches!(r.run(&input), Err(RunError::Timeout(_))));
    }
```
  - 주의: 이 테스트가 OS/CI에서 플레이키하면(파일권한 등) `#[ignore]`로 두고 exec.rs 테스트만 신뢰한다. 우선 실행해보고 안정적이면 유지.

- [ ] **Step 5: 전체 검증 + 커밋**
  - `cargo test`(전체) PASS. `cargo build` 경고 0. `cargo clippy --all-targets` 클린.
  - `git add src/runner/claude.rs src/runner/codex.rs && git commit -m "feat(runner): 양 러너를 idle watchdog 헬퍼로 배선"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** INV-4 idle watchdog(무출력 행 방지) 충족. 양 러너 공통 적용. stderr 동시 배수로 pipe-buffer 데드락도 제거(범위 결정 = watchdog + stderr 배수).
- **placeholder:** 없음. 모든 단계 실코드.
- **타입 일관성:** `RunError::Timeout` 추가는 additive(exhaustive match 없음 확인). `ExecSpec`/`run_with_watchdog`는 pub(crate). 러너 필드 `idle_timeout` 추가(new/with_bin 기본 600s, main.rs 무영향).
- **race 안전:** `watchdog_done` AtomicBool + RAII `WatchdogGuard`로 trailing-kill race 차단(tunaFlow 2026-04-29 수정 반영). timed_out을 종료코드 검사보다 먼저 확인(오분류 방지).
- **선제 설계:** 공유 헬퍼로 watchdog 단일 출처. 신규 의존성 0(std만). concrete 엔진 경계 유지.

## 위험 / 한계 (문서화된 후속)

- **고아 grandchild + pipe:** 단일 PID kill이라, 에이전트가 stdout를 물려준 손자 프로세스를 spawn한 채 멈추면 부모만 죽고 reader가 그 손자 종료까지 더 블로킹될 수 있다. tunaFlow도 단일 PID이고 실사용에서 충분했으므로 v2는 단일 PID 채택. 필요 시 후속에서 프로세스-그룹 kill(`process_group(0)` + `kill -<pgid>`)로 강화.
- **타임아웃 600s 고정:** 설정 노출은 후속(N좌석/설정 파일 작업 때 함께). 현재는 코드 상수 + 테스트 주입.
- **러너 타임아웃 테스트 플레이키 가능성:** 파일 권한/시간 의존. exec.rs 테스트가 핵심 검증, 러너 테스트는 배선 확인용(불안정 시 `#[ignore]`).
