# autoarm(SessionStart)·session-ping(UserPromptSubmit) 공유: 세션 무장(idempotent) 코어.
"""tunaRound 세션 무장 공유 모듈(설계 v2-42).

ensure_armed(session_id, cwd) = 이 세션의 detached poll(register+heartbeat)이 없으면 띄운다.
이미 살아있으면 no-op. 반환=(agent_uuid, core) 또는 None(무장 못 함/비대상).
opt-in(TUNA_AUTOARM=1)·토큰 존재를 전제로만 무장한다. 실패는 조용히 None(세션을 절대 막지 않음).
"""
import json
import os
import subprocess
from pathlib import Path


def state_dir() -> Path:
    d = Path.home() / ".tunaround" / "autoarm"
    d.mkdir(parents=True, exist_ok=True)
    return d


def pid_alive(pid: int) -> bool:
    try:
        if os.name == "nt":
            out = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                capture_output=True, text=True, timeout=5,
            )
            return str(pid) in out.stdout.split()
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def launch_detached(cmd: list, log_path: Path) -> int:
    """세션·하네스 수명과 무관하게 상주하도록 완전 분리된 프로세스로 기동한다."""
    with open(log_path, "ab") as log:
        if os.name == "nt":
            flags = 0x00000008 | 0x00000200  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
            proc = subprocess.Popen(
                cmd, stdout=log, stderr=log, stdin=subprocess.DEVNULL,
                creationflags=flags, close_fds=True,
            )
        else:
            proc = subprocess.Popen(
                cmd, stdout=log, stderr=log, stdin=subprocess.DEVNULL,
                start_new_session=True, close_fds=True,
            )
    return proc.pid


def broker_core() -> str:
    """브로커 MCP 코어 URL(핑용 base 도출에도 씀)."""
    return os.environ.get("TUNA_BROKER_CORE", "http://127.0.0.1:8770/mcp")


def ensure_armed(session_id: str, cwd: str):
    """이 세션을 무장(idempotent)한다. 반환=(agent, core) 또는 None.

    opt-in(TUNA_AUTOARM=1) + 토큰 필요. 이미 무장(pidfile 살아있음)이면 재기동 없이 (agent, core) 반환.
    """
    if os.environ.get("TUNA_AUTOARM") != "1":
        return None
    session_id = str(session_id or "").strip()
    if not session_id:
        return None
    if not os.environ.get("TUNA_BROKER_TOKEN"):
        return None

    core = broker_core()
    tuna_bin = os.environ.get("TUNA_BIN", "tunaround")
    host = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
    user = os.environ.get("USERNAME") or os.environ.get("USER") or "user"
    machine = os.environ.get("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    project = os.environ.get("TUNA_AUTOARM_PROJECT") or Path(cwd).name or "unknown"
    role = os.environ.get("TUNA_AUTOARM_ROLE", "session")
    agent = session_id  # uuid=세션 id(라우팅·discover overlay 키, 설계 §2.1)
    display = os.environ.get("TUNA_AUTOARM_AGENT") or f"{machine}-claude-{project}"
    interval = os.environ.get("TUNA_AUTOARM_INTERVAL", "15")
    tags = (
        f"machine={machine},runner=claude,role={role},project={project},"
        f"user={user},host={host},session={session_id}"
    )

    sdir = state_dir()
    pidfile = sdir / f"{session_id}.json"
    log_path = sdir / f"{session_id}.log"

    # 이미 무장(살아있음)이면 재기동 없이 반환.
    if pidfile.exists():
        try:
            prev = json.loads(pidfile.read_text(encoding="utf-8"))
            if pid_alive(int(prev.get("pid", -1))):
                return (agent, core)
        except Exception:
            pass  # 손상된 pidfile은 무시하고 새로 기동.

    cmd = [
        tuna_bin, "poll", "--core", core, "--agent", agent,
        "--display-name", display, "--tags", tags, "--interval", str(interval),
    ]
    try:
        pid = launch_detached(cmd, log_path)
    except Exception:
        return None

    pidfile.write_text(json.dumps({
        "pid": pid, "agent": agent, "display_name": display, "core": core,
        "tags": tags, "log": str(log_path), "session_id": session_id,
    }), encoding="utf-8")
    return (agent, core)
