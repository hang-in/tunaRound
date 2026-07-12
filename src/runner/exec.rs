// 에이전트 자식 프로세스를 idle watchdog와 함께 구동하고 stdout를 수집하는 공유 실행 헬퍼.

use super::RunError;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[cfg(unix)]
use std::os::unix::process::CommandExt;

/// 한 자식 프로세스 실행 명세. argv·stdin·작업디렉토리·idle 타임아웃·로그 라벨.
pub(crate) struct ExecSpec {
    pub bin: String,
    pub args: Vec<String>,
    pub cwd: Option<String>,
    pub stdin: Option<String>,
    pub idle_timeout: Duration,
    pub label: String,
    /// 자식에 추가로 주입할 환경변수(key, value). 비면 부모 환경만 상속.
    /// codex bearer_token_env_var처럼 인자에 비밀을 노출하지 않고 토큰을 넘길 때 사용.
    pub env: Vec<(String, String)>,
}

/// watchdog에 함수 scope 종료를 알려 trailing-kill race(이미 reap된 PID에 kill 송출)를 막는 RAII 가드.
struct WatchdogGuard(Arc<AtomicBool>);
impl Drop for WatchdogGuard {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}

/// Windows에서 확장자 없는 bin을 PATH 디렉토리들에서 .exe/.cmd/.bat/.com 순으로 찾아 풀경로 반환.
/// 이미 확장자/경로가 있거나 못 찾으면 None(원본 유지). 순수 함수(테스트용).
#[cfg(windows)]
fn resolve_bin_in(bin: &str, dirs: &[std::path::PathBuf]) -> Option<String> {
    use std::path::Path;
    if bin.contains('/') || bin.contains('\\') || Path::new(bin).extension().is_some() {
        return None;
    }
    const EXTS: [&str; 4] = ["exe", "cmd", "bat", "com"];
    for dir in dirs {
        for ext in EXTS {
            let cand = dir.join(format!("{bin}.{ext}"));
            if cand.is_file() {
                return cand.to_str().map(|s| s.to_string());
            }
        }
    }
    None
}

/// spawn용 bin 해석. Windows에서 PATH를 뒤져 풀경로화(.cmd 래핑 가능케). 그 외/실패 시 원본.
fn resolve_bin(bin: &str) -> String {
    #[cfg(windows)]
    {
        if let Ok(path) = std::env::var("PATH") {
            let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&path).collect();
            if let Some(found) = resolve_bin_in(bin, &dirs) {
                return found;
            }
        }
        bin.to_string()
    }
    #[cfg(not(windows))]
    {
        bin.to_string()
    }
}

/// 자식을 spawn해 idle watchdog로 감시하며 stdout를 라인 단위로 수집한다.
/// 무출력이 idle_timeout을 넘으면 자식을 kill하고 `RunError::Timeout`. 성공 시 stdout 수집본을 돌려준다.
pub(crate) fn run_with_watchdog(spec: &ExecSpec) -> Result<String, RunError> {
    let mut cmd = Command::new(resolve_bin(&spec.bin));
    cmd.args(&spec.args);
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    if let Some(dir) = &spec.cwd {
        cmd.current_dir(dir);
    }
    if spec.stdin.is_some() {
        cmd.stdin(Stdio::piped());
    }
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

    // Unix에서는 CLI와 그 후손만 묶인 별도 process group을 만든다.
    // process_group(0)은 spawn된 자식의 PID를 PGID로 사용하므로 watchdog가 그룹 전체를 종료할 수 있다.
    #[cfg(unix)]
    cmd.process_group(0);

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

/// 자식 PID를 루트로 하는 프로세스 트리를 best-effort로 강제 종료한다.
fn kill_pid(pid: u32) {
    #[cfg(unix)]
    {
        // run_with_watchdog가 process_group(0)으로 자식 PID를 PGID로 만들었으므로,
        // 음수 PID(그룹 전체)에 SIGKILL을 syscall로 직접 보낸다. 외부 `kill -9 -PID`는
        // util-linux 등에서 음수 인자가 옵션으로 파싱돼 그룹을 못 죽이는 이식성 함정이 있다.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    #[cfg(windows)]
    {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .status();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// 주어진 bin·argv로 ExecSpec을 만든다(테스트 공용 빌더).
    fn spec_from(bin: &str, args: &[&str], idle_ms: u64) -> ExecSpec {
        ExecSpec {
            bin: bin.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            cwd: None,
            stdin: None,
            idle_timeout: Duration::from_millis(idle_ms),
            label: "test".into(),
            env: Vec::new(),
        }
    }

    // 아래 세 시나리오 헬퍼는 OS 인지형이다. sh 는 clean Windows에 없으므로,
    // 같은 watchdog 거동(무출력 idle / 즉시 출력 / 비정상 종료)을 Unix=sh -c, Windows=cmd /C 로 각각 재현한다.

    /// 무출력으로 수 초 도는 자식(idle 타임아웃 발화 검증용). Windows는 ping의 무출력 대기로 등가 재현.
    fn spec_idle_no_output(idle_ms: u64) -> ExecSpec {
        #[cfg(not(windows))]
        let (bin, args): (&str, &[&str]) = ("sh", &["-c", "exec sleep 5"]);
        // ping -n 6 = 약 5초 대기, 출력은 nul로 버려 stdout 무출력을 보장한다.
        #[cfg(windows)]
        let (bin, args): (&str, &[&str]) = ("cmd", &["/C", "ping -n 6 127.0.0.1 >nul"]);
        spec_from(bin, args, idle_ms)
    }

    /// 즉시 두 줄 출력 후 정상 종료(출력이 타이머를 리셋해 오탐 타임아웃이 없음을 검증).
    fn spec_two_lines(idle_ms: u64) -> ExecSpec {
        #[cfg(not(windows))]
        let (bin, args): (&str, &[&str]) = ("sh", &["-c", "printf 'line1\\nline2\\n'"]);
        #[cfg(windows)]
        let (bin, args): (&str, &[&str]) = ("cmd", &["/C", "echo line1&echo line2"]);
        spec_from(bin, args, idle_ms)
    }

    /// 무출력으로 즉시 비정상 종료(Timeout 아닌 Spawn 오류로 분류되는지 검증).
    fn spec_nonzero_exit(idle_ms: u64) -> ExecSpec {
        #[cfg(not(windows))]
        let (bin, args): (&str, &[&str]) = ("sh", &["-c", "exit 3"]);
        #[cfg(windows)]
        let (bin, args): (&str, &[&str]) = ("cmd", &["/C", "exit 3"]);
        spec_from(bin, args, idle_ms)
    }

    #[test]
    fn idle_no_output_triggers_timeout() {
        let out = run_with_watchdog(&spec_idle_no_output(150));
        match out {
            Err(RunError::Timeout(_)) => {}
            other => panic!("expected Timeout, got {other:?}"),
        }
    }

    #[test]
    fn output_then_exit_succeeds_no_false_timeout() {
        // 즉시 출력 후 종료 -> 타이머 리셋되어 타임아웃 없이 stdout 수집.
        let out = run_with_watchdog(&spec_two_lines(2000)).expect("ok");
        assert!(out.contains("line1"));
        assert!(out.contains("line2"));
    }

    #[test]
    fn nonzero_exit_is_spawn_error_not_timeout() {
        // 무출력이지만 즉시 비정상 종료 -> Timeout 아님(Spawn).
        let out = run_with_watchdog(&spec_nonzero_exit(2000));
        assert!(matches!(out, Err(RunError::Spawn(_))));
    }

    #[cfg(unix)]
    #[test]
    fn timeout_kills_spawned_process_tree() {
        let pid_file = std::env::temp_dir().join(format!(
            "tunaround-process-tree-{}-{}.pid",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("system time after epoch")
                .as_nanos()
        ));
        let command = format!(
            "sleep 30 & child=$!; printf '%s' \"$child\" > '{}'; wait \"$child\"",
            pid_file.display()
        );

        let started = Instant::now();
        let out = run_with_watchdog(&spec_from("sh", &["-c", &command], 200));
        assert!(matches!(out, Err(RunError::Timeout(_))));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "process tree was not terminated promptly"
        );

        let child_pid = std::fs::read_to_string(&pid_file)
            .expect("spawned child pid file")
            .trim()
            .to_string();
        let _ = std::fs::remove_file(&pid_file);

        let mut still_alive = true;
        for _ in 0..20 {
            still_alive = Command::new("kill")
                .args(["-0", &child_pid])
                .status()
                .is_ok_and(|status| status.success());
            if !still_alive {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!still_alive, "spawned child {child_pid} survived timeout");
    }

    #[cfg(windows)]
    #[test]
    fn resolve_finds_cmd_in_dir() {
        let dir = std::env::temp_dir().join("tuna_resolve_test");
        let _ = std::fs::create_dir_all(&dir);
        let cmd = dir.join("mytool.cmd");
        std::fs::write(&cmd, "@echo off\r\n").unwrap();
        // 확장자 없는 "mytool" -> dir의 mytool.cmd 풀경로.
        let got = resolve_bin_in("mytool", std::slice::from_ref(&dir));
        assert_eq!(got.as_deref(), Some(cmd.to_str().unwrap()));
        let _ = std::fs::remove_file(&cmd);
    }

    #[cfg(windows)]
    #[test]
    fn resolve_keeps_bin_with_extension() {
        // 이미 .cmd면 탐색 안 하고 None(=원본 유지 신호).
        assert!(resolve_bin_in("foo.cmd", &[]).is_none());
    }

    #[test]
    fn resolve_bin_noop_for_pathed() {
        // 경로/확장자 있으면 원본 그대로(크로스플랫폼 불변 가지).
        let p = "some/dir/tool.sh";
        assert_eq!(resolve_bin(p), p);
    }
}
