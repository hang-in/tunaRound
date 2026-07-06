#!/usr/bin/env python3
# SessionEnd 훅: 자동무장으로 띄운 poll 프로세스를 정리한다(로스터는 TTL로 소멸).
"""tunaRound v2-40 S1 자동무장 해제 훅.

tuna-autoarm.py가 남긴 pidfile을 읽어 detached poll 프로세스를 종료한다.
deregister MCP 도구가 없으므로 heartbeat가 끊기면 브로커 로스터가 TTL(90초)로
자동 offline 처리한다. opt-in이 아니면 no-op.
"""
import json
import os
import subprocess
import sys
from pathlib import Path


def kill_pid(pid: int) -> bool:
    try:
        if os.name == "nt":
            subprocess.run(
                ["taskkill", "/PID", str(pid), "/F", "/T"],
                capture_output=True, text=True, timeout=10,
            )
        else:
            os.kill(pid, 15)
        return True
    except Exception:
        return False


def main() -> int:
    if os.environ.get("TUNA_AUTOARM") != "1":
        return 0

    try:
        payload = json.load(sys.stdin)
    except Exception:
        payload = {}
    session_id = str(payload.get("session_id") or "unknown")

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
