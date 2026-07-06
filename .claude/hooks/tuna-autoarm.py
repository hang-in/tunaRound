#!/usr/bin/env python3
# SessionStart 훅: opt-in 세션을 브로커 로스터에 자동 무장(detached tunaround poll 기동)한다.
"""tunaRound v2-40 S1 자동무장 훅.

TUNA_AUTOARM=1일 때만 동작한다. Claude Code 세션이 시작되면:
  1. `tunaround poll`을 detached로 기동(내부적으로 register_agent + heartbeat).
  2. pidfile을 남겨 SessionEnd 훅(tuna-disarm.py)이 정리할 수 있게 한다.
  3. additionalContext로 세션에 "무장됨 + 수신법"을 주입한다.

deregister MCP 도구는 없으므로 로스터 정리는 heartbeat 중단 후 TTL(90초) 소멸에 맡긴다.
opt-in이 아니거나 토큰이 없으면 조용히 no-op(exit 0)한다 - 세션 시작을 절대 막지 않는다.
"""
import json
import os
import subprocess
import sys
from pathlib import Path


def emit_context(text: str) -> None:
    """SessionStart 훅 출력 계약: hookSpecificOutput.additionalContext로 세션에 문자열 주입."""
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    }))


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
            # 부분 문자열이 아니라 공백 분리 토큰으로 정확히 매칭(메모리 열의 숫자 오탐 방지).
            return str(pid) in out.stdout.split()
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def launch_detached(cmd: list, log_path: Path) -> int:
    """세션·하네스 수명과 무관하게 상주하도록 완전 분리된 프로세스로 기동한다."""
    # with로 부모의 파일 핸들을 즉시 닫는다(자식은 자기 복제본을 유지 = FD 누수 방지).
    with open(log_path, "ab") as log:
        if os.name == "nt":
            # DETACHED_PROCESS(0x08) | CREATE_NEW_PROCESS_GROUP(0x200): 콘솔·그룹 분리.
            flags = 0x00000008 | 0x00000200
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


def main() -> int:
    # opt-in이 아니면 즉시 no-op(출력 없음 = 컨텍스트 오염 없음).
    if os.environ.get("TUNA_AUTOARM") != "1":
        return 0

    # 대화형 터미널에서 stdin 없이 실행하면 json.load가 EOF를 무한 대기하므로 isatty로 가드.
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
    except Exception:
        payload = {}
    # session_id가 없으면 여러 세션이 unknown.json을 공유해 SessionEnd가 남의 poll을 죽일 수 있다.
    # pidfile 키가 세션별로 유일해야 안전하므로, 없으면 무장하지 않는다.
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        emit_context("[tuna-autoarm] session_id가 없어 무장하지 않았습니다(세션별 pidfile 충돌 방지).")
        return 0
    cwd = payload.get("cwd") or os.getcwd()

    token = os.environ.get("TUNA_BROKER_TOKEN")
    if not token:
        emit_context(
            "[tuna-autoarm] TUNA_AUTOARM=1이나 TUNA_BROKER_TOKEN 미설정이라 무장하지 않았습니다. "
            "토큰 env를 설정하면 이 세션이 브로커 로스터에 자동 등록됩니다."
        )
        return 0

    core = os.environ.get("TUNA_BROKER_CORE", "http://127.0.0.1:8770/mcp")
    tuna_bin = os.environ.get("TUNA_BIN", "tunaround")
    host = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
    user = os.environ.get("USERNAME") or os.environ.get("USER") or "user"
    machine = os.environ.get("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    project = os.environ.get("TUNA_AUTOARM_PROJECT") or Path(cwd).name or "unknown"
    role = os.environ.get("TUNA_AUTOARM_ROLE", "session")
    # uuid는 라우팅·발견 overlay 키라 세션 id를 쓴다(설계 §2.1: uuid=세션 id). 그래야 discover가
    # 낸 후보(uuid=세션 id)와 로스터가 매칭돼 armed overlay·중복제거가 성립한다. 사람이 읽는 이름은
    # display_name으로 분리한다(총감독은 TUNA_AUTOARM_AGENT로 win-opus-boss 등 지정).
    agent = session_id
    display = os.environ.get("TUNA_AUTOARM_AGENT") or f"{host}-claude-{session_id[:8]}"
    interval = os.environ.get("TUNA_AUTOARM_INTERVAL", "15")

    tags = f"machine={machine},runner=claude,role={role},project={project},user={user},host={host}"

    sdir = state_dir()
    pidfile = sdir / f"{session_id}.json"
    log_path = sdir / f"{session_id}.log"

    # 중복 무장 가드: 같은 세션의 poll이 이미 살아있으면 재기동하지 않는다.
    if pidfile.exists():
        try:
            prev = json.loads(pidfile.read_text(encoding="utf-8"))
            if pid_alive(int(prev.get("pid", -1))):
                emit_context(
                    f"[tuna-autoarm] 이미 무장됨: {prev.get('display_name') or prev.get('agent', agent)} "
                    f"(poll pid={prev.get('pid')}). 로스터에서 online 상태입니다."
                )
                return 0
        except Exception:
            pass  # 손상된 pidfile은 무시하고 새로 기동.

    # 토큰은 --token(argv) 대신 자식이 상속하는 TUNA_BROKER_TOKEN env로 전달한다(프로세스 목록 노출 방지).
    # poll이 --token 없으면 이 env를 폴백으로 읽는다. token 존재는 위에서 이미 확인했다.
    cmd = [
        tuna_bin, "poll",
        "--core", core,
        "--agent", agent,
        "--display-name", display,
        "--tags", tags,
        "--interval", str(interval),
    ]

    try:
        pid = launch_detached(cmd, log_path)
    except FileNotFoundError:
        emit_context(
            f"[tuna-autoarm] '{tuna_bin}' 실행 실패(PATH에 없음). TUNA_BIN으로 tunaround 경로를 지정하세요."
        )
        return 0
    except Exception as e:
        emit_context(f"[tuna-autoarm] 무장 실패: {e}")
        return 0

    pidfile.write_text(json.dumps({
        "pid": pid,
        "agent": agent,
        "display_name": display,
        "core": core,
        "tags": tags,
        "log": str(log_path),
        "session_id": session_id,
    }), encoding="utf-8")

    emit_context(
        f"[tuna-autoarm] 이 세션이 브로커 로스터에 자동 등록되었습니다.\n"
        f"  uuid={agent}(세션 id)  display={display}  tags={tags}\n"
        f"  core={core}  poll pid={pid}  log={log_path}\n"
        f"이제 총감독 대시보드(/dashboard/roster)에 online으로 나타납니다. "
        f"이 세션 앞으로 온 A2A task를 받으려면 poll 로그를 Monitor로 감시하거나 "
        f"`poll_tasks`/`claim_task`/`complete_task`로 처리하세요. "
        f"세션 종료 시 SessionEnd 훅이 poll을 정리하고 로스터는 TTL로 소멸합니다."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
