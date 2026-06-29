// 에이전트 자식 프로세스를 idle watchdog와 함께 구동하고 stdout를 수집하는 공유 실행 헬퍼.

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
