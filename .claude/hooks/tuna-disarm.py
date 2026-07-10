#!/usr/bin/env python3
# SessionEnd 훅: 자동무장으로 띄운 poll 프로세스를 정리한다(로스터는 TTL로 소멸).
"""tunaRound v2-40 S1 자동무장 해제 훅.

tuna-autoarm.py가 남긴 pidfile을 읽어 detached poll 프로세스를 종료한다.
deregister MCP 도구가 없으므로 heartbeat가 끊기면 브로커 로스터가 TTL(90초)로
자동 offline 처리한다. opt-in이 아니면 no-op.
"""
import json
import os
import re
import signal
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

# 설정파일(config-first) 게이트는 tuna_arm 단일 소스에서. import 실패 시 env-only로 안전 강등.
try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from tuna_arm import cfg
except Exception:
    def cfg(key, default=None):
        return os.environ.get(key, default)

# tuna_arm.sanitize_session_id와 동일 규칙: autoarm이 쓴 pidfile 이름과 반드시 일치해야 한다.
_SAFE_SESSION_RE = re.compile(r"[^A-Za-z0-9._-]")


def sanitize_session_id(session_id: str) -> str:
    s = _SAFE_SESSION_RE.sub("_", str(session_id or "").strip())
    return s.strip(".") or ""


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
    if cfg("TUNA_AUTOARM") != "1":  # 설정파일 우선(env 신선도 무관, 설계 v2-43 §5-1).
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

    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return 0
    pidfile = Path.home() / ".tunaround" / "autoarm" / f"{safe_id}.json"
    if not pidfile.exists():
        return 0

    try:
        info = json.loads(pidfile.read_text(encoding="utf-8"))
    except Exception:
        info = None  # 손상 pidfile = 죽일 대상 불명 → 파일만 정리.

    poll_dead = True
    if info:
        try:
            pid = int(info.get("pid", -1))
        except Exception:
            pid = -1
        if pid > 0:
            kill_pid(pid)
            if is_tunaround_pid(pid):
                time.sleep(0.3)  # taskkill 반영 지연 재확인.
                poll_dead = not is_tunaround_pid(pid)
        if poll_dead:
            # 브로커에 즉시 등록해제 통보 → TTL(90초) 자연소멸 대기 없이 로스터에서 바로 제거(설계 v2-43).
            deregister(info.get("agent"), info.get("core"))

    # 사망 미확인이면 pidfile 보존: pidfile만 지우면 살아남은 poll이 heartbeat "미등록"
    # 응답에 자가 재등록해 유령이 된다(2026-07-10 실측). 다음 리핑/disarm에서 재시도.
    if poll_dead:
        try:
            pidfile.unlink()
        except Exception:
            pass
    return 0


def deregister(agent, core) -> None:
    """SessionEnd 시 브로커 로스터에서 이 세션을 즉시 제거한다(loopback POST). 실패는 조용히 통과.

    등록해제가 안 돼도 heartbeat 끊김으로 90초 내 자연소멸하므로 best-effort로만 시도한다.
    """
    if not agent or not core:
        return
    c = str(core).rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c  # ".../mcp"를 base로 절단.
    if not base.startswith(("http://", "https://")):
        return  # loopback HTTP 전용(file: 등 비정상 스킴 차단)
    token = cfg("TUNA_BROKER_TOKEN", "")
    body = json.dumps({"agent": agent}).encode()
    req = urllib.request.Request(base + "/dashboard/deregister", data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        urllib.request.urlopen(req, timeout=0.75).read()
    except Exception:
        pass


if __name__ == "__main__":
    sys.exit(main())
