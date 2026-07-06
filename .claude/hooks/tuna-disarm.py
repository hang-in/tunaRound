#!/usr/bin/env python3
# SessionEnd 훅: 자동무장으로 띄운 poll 프로세스를 정리한다(로스터는 TTL로 소멸).
"""tunaRound v2-40 S1 자동무장 해제 훅.

tuna-autoarm.py가 남긴 pidfile을 읽어 detached poll 프로세스를 종료한다.
deregister MCP 도구가 없으므로 heartbeat가 끊기면 브로커 로스터가 TTL(90초)로
자동 offline 처리한다. opt-in이 아니면 no-op.
"""
import json
import os
import signal
import subprocess
import sys
from pathlib import Path


def is_tunaround_pid(pid: int) -> bool:
    """해당 PID가 실제 tunaround 프로세스인지 확인한다(PID 재사용으로 엉뚱한 프로세스를 죽이지 않게)."""
    try:
        if os.name == "nt":
            out = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                capture_output=True, text=True, timeout=5,
            )
            return "tunaround" in out.stdout.lower()
        # POSIX: /proc 또는 ps로 명령줄에 tunaround가 있는지.
        try:
            with open(f"/proc/{pid}/cmdline", "rb") as f:
                return b"tunaround" in f.read().lower()
        except OSError:
            out = subprocess.run(
                ["ps", "-p", str(pid), "-o", "command="],
                capture_output=True, text=True, timeout=5,
            )
            return "tunaround" in out.stdout.lower()
    except Exception:
        return False


def kill_pid(pid: int) -> bool:
    """tunaround로 확인된 PID만 종료한다. 확인 실패(재사용/이미 종료)면 kill하지 않는다."""
    if not is_tunaround_pid(pid):
        return False
    try:
        if os.name == "nt":
            subprocess.run(
                ["taskkill", "/PID", str(pid), "/F", "/T"],
                capture_output=True, text=True, timeout=10,
            )
        else:
            os.kill(pid, signal.SIGTERM)
        return True
    except Exception:
        return False


def main() -> int:
    if os.environ.get("TUNA_AUTOARM") != "1":
        return 0

    # 대화형 터미널에서 stdin 없이 실행하면 json.load가 무한 대기하므로 isatty로 가드.
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
    except Exception:
        payload = {}
    # session_id가 없으면 무장하지도 않았으므로(autoarm이 skip) 정리할 것도 없다. 공유 unknown.json을 건드리지 않는다.
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        return 0

    pidfile = Path.home() / ".tunaround" / "autoarm" / f"{session_id}.json"
    if not pidfile.exists():
        return 0

    try:
        info = json.loads(pidfile.read_text(encoding="utf-8"))
        pid = int(info.get("pid", -1))
        if pid > 0:
            kill_pid(pid)
    except Exception:
        pass
    finally:
        try:
            pidfile.unlink()
        except Exception:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
