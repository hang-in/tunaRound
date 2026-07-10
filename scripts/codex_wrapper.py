#!/usr/bin/env python3
# codex CLI를 가로채 세션을 자동 무장(poll 등록)·해제하는 PATH shim 래퍼 (v2-43 §5-3 codex arming).
import json
import os
import sys
import uuid
import shutil
import subprocess

# .claude/hooks 디렉토리를 path에 추가하여 tuna_arm.py를 가져올 수 있게 함
hooks_dir = os.path.abspath(os.path.join(os.path.dirname(__file__), "..", ".claude", "hooks"))
if os.path.exists(hooks_dir):
    sys.path.insert(0, hooks_dir)

try:
    import tuna_arm
except ImportError:
    # hooks 경로에서 못 찾을 시 fallback 시도 (동일 디렉토리 등)
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    try:
        import tuna_arm
    except ImportError:
        tuna_arm = None


def find_real_codex():
    """래퍼 자신을 제외한 시스템 PATH 상의 진짜 codex 명령어의 경로를 반환합니다."""
    wrapper_dir = os.path.dirname(os.path.abspath(__file__))
    paths = os.environ.get("PATH", "").split(os.pathsep)
    filtered_paths = [p for p in paths if os.path.abspath(p) != os.path.abspath(wrapper_dir)]

    # 윈도우의 경우 codex.cmd, codex.bat, codex.exe 탐색
    # Unix의 경우 codex 탐색
    bin_names = ["codex.cmd", "codex.bat", "codex"] if os.name == "nt" else ["codex"]

    for path in filtered_paths:
        for name in bin_names:
            full_path = os.path.join(path, name)
            if os.path.isfile(full_path) and os.access(full_path, os.X_OK):
                return full_path

    # fallback: shutil.which
    which_res = shutil.which("codex")
    if which_res and os.path.dirname(os.path.abspath(which_res)) != wrapper_dir:
        return which_res

    return None


def main():
    args = sys.argv[1:]

    # 무장 조건 체크 (tuna_arm 모듈 로드 성공 + TUNA_AUTOARM=1 설정)
    is_autoarm = False
    if tuna_arm:
        try:
            is_autoarm = (tuna_arm.cfg("TUNA_AUTOARM") == "1") and bool(tuna_arm.cfg("TUNA_BROKER_TOKEN"))
        except Exception:
            pass

    # 무장 비대상일 경우 원본 codex만 조용히 구동하고 통과
    if not is_autoarm:
        real_codex = find_real_codex()
        if not real_codex:
            print("[codex-wrapper] 원본 'codex' 명령어를 찾을 수 없습니다.", file=sys.stderr)
            return 127
        return subprocess.run([real_codex] + args, check=False).returncode

    # 무장 대상인 경우: 세션 기동
    machine = tuna_arm.cfg("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    project = tuna_arm.cfg("TUNA_AUTOARM_PROJECT") or os.path.basename(os.getcwd()) or "unknown"
    
    # 세션 ID 생성: TUNA_AUTOARM_AGENT 우선, 없으면 고유 식별자 생성
    # 이 ID가 세션 uuid이자 pidfile 키가 됨
    agent_id = tuna_arm.cfg("TUNA_AUTOARM_AGENT")
    if not agent_id:
        random_hex = uuid.uuid4().hex[:8]
        agent_id = f"{machine}-codex-{project}-{random_hex}"

    display = f"{machine}-codex-{project}"
    owner_pid = os.getpid()

    # tunaround poll 기동
    armed = None
    try:
        armed = tuna_arm.ensure_codex_armed(agent_id, os.getcwd(), display, project, owner_pid)
    except Exception as e:
        print(f"[codex-wrapper] 세션 무장 실패: {e}", file=sys.stderr)

    if armed:
        print(f"[codex-wrapper] Codex 세션이 무장되었습니다 (ID: {agent_id}, Core: {armed[1]})")

    # 원본 codex 실행
    real_codex = find_real_codex()
    if not real_codex:
        print("[codex-wrapper] 원본 'codex' 명령어를 찾을 수 없습니다.", file=sys.stderr)
        # 만약 원본을 못 찾았다면 poll 정리 후 종료
        if armed:
            disarm_session(agent_id)
        return 127

    try:
        result = subprocess.run([real_codex] + args, check=False)
        returncode = result.returncode
    except KeyboardInterrupt:
        returncode = 130
    except Exception as e:
        print(f"[codex-wrapper] codex 실행 에러: {e}", file=sys.stderr)
        returncode = 1

    # codex 종료 후 disarm 및 즉시 등록해제(deregister)
    if armed:
        disarm_session(agent_id)

    return returncode


def disarm_session(agent_id: str):
    """지정된 세션 ID의 poll을 강제 종료하고 브로커 등록 해제 요청을 수행합니다."""
    try:
        safe_id = tuna_arm.sanitize_session_id(agent_id)
        pidfile = tuna_arm.state_dir() / f"{safe_id}.json"
        if pidfile.exists():
            info = json.loads(pidfile.read_text(encoding="utf-8"))
            pollpid = info.get("pid")
            if pollpid:
                try:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(pollpid), "/F"],
                                       capture_output=True, timeout=5, check=False)
                    else:
                        os.kill(int(pollpid), 9)
                except Exception:
                    pass
            tuna_arm._deregister(info.get("agent"), info.get("core") or tuna_arm.broker_core(), tuna_arm.cfg("TUNA_BROKER_TOKEN"))
            try:
                pidfile.unlink()
            except Exception:
                pass
            print("[codex-wrapper] Codex 세션 무장이 해제되었습니다.")
    except Exception as e:
        print(f"[codex-wrapper] 무장 해제 중 에러 발생: {e}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
