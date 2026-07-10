#!/usr/bin/env python3
# codex CLI를 가로채 세션을 자동 무장(poll 등록)·해제하는 PATH shim 래퍼 (v2-43 §5-3 codex arming).
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


def _is_wrapper_dir(path: str, wrapper_dir: str) -> bool:
    """PATH 항목이 래퍼 자신의 디렉터리인지(디바이스+아이노드 비교 = 표기·대소문자 무관).

    normcase는 macOS에서 no-op이라(POSIX) 대소문자 무구분 FS의 자기재귀를 못 막는다.
    samefile이면 심볼릭 링크·표기 차이까지 커버된다. 존재하지 않는 PATH 항목은 False.
    """
    try:
        return os.path.samefile(path, wrapper_dir)
    except OSError:
        return False


def find_real_codex():
    """래퍼 자신을 제외한 시스템 PATH 상의 진짜 codex 명령어의 경로를 반환합니다."""
    wrapper_dir = os.path.dirname(os.path.abspath(__file__))
    paths = os.environ.get("PATH", "").split(os.pathsep)
    filtered_paths = [p for p in paths if p and not _is_wrapper_dir(p, wrapper_dir)]

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
    if which_res and not _is_wrapper_dir(os.path.dirname(os.path.abspath(which_res)), wrapper_dir):
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

    # 시스템 temp에서의 codex 실행(자동화 headless 추정)은 로스터 노이즈라 무장 없이 투명 통과
    # (claude 훅의 temp 제외와 동일 정책, TUNA_AUTOARM_PROJECT 명시 시 예외).
    if is_autoarm:
        is_temp = getattr(tuna_arm, "is_temp_cwd", lambda _c: False)
        if is_temp(os.getcwd()) and not tuna_arm.cfg("TUNA_AUTOARM_PROJECT"):
            is_autoarm = False

    # 무장 비대상일 경우 원본 codex만 조용히 구동하고 통과
    if not is_autoarm:
        real_codex = find_real_codex()
        if not real_codex:
            print("[codex-wrapper] 원본 'codex' 명령어를 찾을 수 없습니다.", file=sys.stderr)
            return 127
        return subprocess.run([real_codex] + args, check=False).returncode

    # 무장 대상인 경우: 세션 기동
    machine = tuna_arm.cfg("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    # home에서 띄운 codex는 개인 폴더명 대신 'home'으로(codex는 프로젝트 개념이 없어 cwd 폴더명이 유일 단서).
    project = tuna_arm.cfg("TUNA_AUTOARM_PROJECT") or tuna_arm.project_from_cwd(os.getcwd())
    
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
    """지정된 세션 ID의 poll을 종료하고 브로커 등록을 해제한다(tuna_arm 공용 로직 위임)."""
    try:
        if tuna_arm.disarm_session(agent_id) == "DISARMED":
            print("[codex-wrapper] Codex 세션 무장이 해제되었습니다.")
    except Exception as e:
        print(f"[codex-wrapper] 무장 해제 중 에러 발생: {e}", file=sys.stderr)


if __name__ == "__main__":
    sys.exit(main())
