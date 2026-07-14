# codex CLI를 가로채 세션 threadId<->래퍼PID 생존 마커만 기록하는 PATH shim 래퍼(이슈 #119).
"""tunaRound codex 자동무장 마커 래퍼.

v2-43(60e8d8c)의 구 래퍼는 poll 무장(ensure_codex_armed)까지 겸했으나, v2-44에서 presence가
머신 스캐너(`tunaround presence-scan`)로 이관되며 무장 코드가 유령 poll·중복 주입의 근원으로
밝혀져 전량 삭제됐다(tuna_arm.py 모듈 docstring 참고). 이 래퍼는 그 실패를 반복하지 않는다:
**역할은 마커 기록뿐이다.** poll을 띄우지도, disarm을 수행하지도 않는다.

마커(~/.tunaround/autoarm/<threadId>.ctx)는 presence 스캐너·codex-relay가 codex 세션의
threadId<->래퍼PID 생존을 판정하는 hybrid 게이트 입력이다(이슈 #119, presence_scan::MarkerState
규약과 동일: PID 기록 = 생존판정 대상, "dead" = tombstone 즉시 제외).

threadId 바인딩은 두 경로다:
  ① argv에 `resume <uuid>` 패턴이 있으면 그 uuid를 그대로 쓴다(resume은 새 rollout을 안 만들
     수 있어 세션 생성 대기로는 못 잡는다).
  ② 없으면 백그라운드 스레드가 `~/.codex/sessions`를 2초 간격 폴링해, 래퍼 시작 이후 새로 나타난
     rollout-*.jsonl 중 이 cwd에서 시작된 메인(non-subagent) session_meta의 session_id를 찾는다.
     **파일명에 박힌 uuid는 쓰지 않는다** - 서브에이전트 rollout은 파일명 uuid != session_id가
     실측됐다(파일명 신뢰 시 엉뚱한 스레드에 마커를 붙이게 된다).

stdlib만 쓴다(외부 훅 모듈 임포트 없음 - 임포트 실패·경로 드리프트에 이 래퍼가 엮이지 않게).
"""
import glob
import json
import os
import re
import shutil
import subprocess
import sys
import threading

# 세션 id를 마커 파일명으로 쓸 때 경로 이탈(../, 구분자)을 막는 허용 문자 집합.
# tuna_arm.sanitize_session_id와 동일 규칙(코드 공유 없이 자체 구현 - stdlib만 쓰는 이 래퍼의
# 제약, 임포트로 훅 경로에 엮이지 않는다).
_SAFE_MARKER_RE = re.compile(r"[^A-Za-z0-9._-]")

# threadId 미확정(resume 아님) 케이스의 rollout 폴링 간격(초).
SESSION_POLL_INTERVAL_SECS = 2.0


# ---------------------------------------------------------------------------
# 순수부 (doctest로 자체 검증)
# ---------------------------------------------------------------------------


def sanitize_marker_id(session_id):
    """세션 id를 안전한 파일명 조각으로 정규화한다(허용 외 문자->'_', 경로 이탈 차단).

    >>> sanitize_marker_id("abc-123")
    'abc-123'
    >>> sanitize_marker_id("../../etc/passwd")
    '_.._etc_passwd'
    >>> sanitize_marker_id("")
    ''
    """
    s = _SAFE_MARKER_RE.sub("_", str(session_id or "").strip())
    return s.strip(".")


def parse_resume_thread_id(args):
    """argv에서 'resume <uuid>' 패턴의 uuid를 뽑는다(위치 무관). resume 바로 다음 토큰이
    '-'로 시작하는 옵션이면(그 자리에 uuid가 아님) 매칭하지 않는다. 없으면 None.

    >>> parse_resume_thread_id(["resume", "abc-123", "--remote", "ws://x"])
    'abc-123'
    >>> parse_resume_thread_id(["exec", "resume", "abc-123"])
    'abc-123'
    >>> parse_resume_thread_id(["resume", "--last"]) is None
    True
    >>> parse_resume_thread_id(["--help"]) is None
    True
    """
    for i, tok in enumerate(args):
        if tok == "resume" and i + 1 < len(args):
            nxt = args[i + 1]
            if not nxt.startswith("-"):
                return nxt
    return None


def _norm_path(p):
    """경로 비교용 정규화(구분자·대소문자 흡수). None은 그대로 통과."""
    if p is None:
        return None
    return os.path.normcase(os.path.normpath(str(p)))


def session_meta_matches(payload, cwd):
    """session_meta payload가 cwd에서 시작된 메인(non-subagent) 세션인지 판정한다(순수부).

    >>> session_meta_matches({"cwd": "/x/y", "session_id": "a"}, "/x/y")
    True
    >>> session_meta_matches({"cwd": "/x/y", "session_id": "a", "thread_source": "subagent"}, "/x/y")
    False
    >>> session_meta_matches({"cwd": "/other", "session_id": "a"}, "/x/y")
    False
    >>> session_meta_matches({}, "/x/y")
    False
    """
    if not isinstance(payload, dict):
        return False
    if _norm_path(payload.get("cwd")) != _norm_path(cwd):
        return False
    if payload.get("thread_source") == "subagent":
        return False
    return bool(payload.get("session_id") or payload.get("id"))


def parse_session_meta_payload(line):
    """rollout 파일 한 줄(JSON)에서 session_meta 이벤트의 payload(dict)를 뽑는다. session_meta가
    아니거나 파싱 실패면 None.

    >>> parse_session_meta_payload('{"type":"session_meta","payload":{"session_id":"a"}}')
    {'session_id': 'a'}
    >>> parse_session_meta_payload('{"type":"turn","payload":{}}') is None
    True
    >>> parse_session_meta_payload("not json") is None
    True
    """
    try:
        obj = json.loads(line)
    except (ValueError, TypeError):
        return None
    if not isinstance(obj, dict) or obj.get("type") != "session_meta":
        return None
    payload = obj.get("payload")
    return payload if isinstance(payload, dict) else None


def read_session_id_if_matching(path, cwd):
    """rollout 파일 첫 줄을 읽어 (완결 여부, session_id)를 반환한다.

    완결 여부(complete)는 "이 파일의 판정이 끝났는가"다: 첫 줄이 유효한 JSON이면 True(우리
    세션이든 아니든 확정), 파일이 갓 생성돼 첫 줄이 비었거나 불완전한 JSON이면 False - 호출부가
    known에 넣지 않고 다음 폴링 주기에 재시도한다(생성과 기록 사이 시간차로 바인딩을 영구히
    놓치는 레이스 방지, gemini 리뷰). IO 실패도 False(재시도).
    session_id는 이 cwd의 메인 세션일 때만 값이 있다."""
    try:
        # path는 glob(_list_rollout_files)이 고정 sessions_dir 하위에서 연 것 - 외부 입력 아님.
        with open(path, "r", encoding="utf-8") as f:  # skipcq: PTC-W6004
            first_line = f.readline()
    except OSError:
        return (False, None)
    try:
        obj = json.loads(first_line)
    except (ValueError, TypeError):
        return (False, None)  # 빈 줄·기록 중 잘린 JSON = 미완결, 재시도.
    payload = None
    if isinstance(obj, dict) and obj.get("type") == "session_meta":
        p = obj.get("payload")
        payload = p if isinstance(p, dict) else None
    if payload is None or not session_meta_matches(payload, cwd):
        return (True, None)  # 유효 JSON = 판정 확정(우리 세션 아님).
    return (True, payload.get("session_id") or payload.get("id"))


# ---------------------------------------------------------------------------
# IO부
# ---------------------------------------------------------------------------


def default_codex_sessions_dir():
    """codex rollout 디렉토리(`~/.codex/sessions`). presence_scan::default_codex_sessions_dir와
    같은 규약."""
    return os.path.join(os.path.expanduser("~"), ".codex", "sessions")


def _list_rollout_files(sessions_dir):
    """sessions_dir 하위 rollout-*.jsonl 전체 경로(재귀). 디렉토리 없음/IO 실패는 빈 리스트."""
    pattern = os.path.join(sessions_dir, "**", "rollout-*.jsonl")
    try:
        return glob.glob(pattern, recursive=True)
    except OSError:
        return []


def find_bound_thread_id(sessions_dir, cwd, stop_event, poll_interval=SESSION_POLL_INTERVAL_SECS):
    """래퍼 시작 이후 새로 나타난 rollout 파일 중 이 cwd의 메인 session_meta를 찾을 때까지
    poll_interval 간격으로 폴링한다(파일명 uuid는 신뢰하지 않고 매번 내용을 파싱한다).
    stop_event가 set되면(자식 종료) 다음 대기 직전에 멈춘다. 찾으면 session_id, 못 찾으면 None.

    '래퍼 시작 이후 생성'의 기준은 시각(mtime) 비교가 아니라 **최초 스냅샷 이후 신규 등장**이다
    (클록 스큐·파일시스템 타임스탬프 해상도 차이에 흔들리지 않는다 - 시작 시점에 이미 있던 파일은
    known에 담겨 다시는 후보가 되지 않는다).
    """
    known = set(_list_rollout_files(sessions_dir))
    while True:
        for path in _list_rollout_files(sessions_dir):
            if path in known:
                continue
            complete, sid = read_session_id_if_matching(path, cwd)
            if complete:
                # 판정이 확정된 파일만 known에 담는다. 미완결(갓 생성돼 첫 줄이 아직 없음)은
                # 다음 주기에 재시도 - 여기서 known에 넣으면 바인딩을 영구히 놓친다(gemini).
                known.add(path)
            if sid:
                return sid
        if stop_event.is_set():
            return None
        stop_event.wait(poll_interval)
        if stop_event.is_set():
            return None


def find_real_codex():
    """래퍼 자신의 디렉토리를 제외한 PATH 상에서 진짜 codex 실행파일 경로를 찾는다. 못 찾으면 None.

    비교는 realpath로 한다(gemini): 래퍼가 다른 경로에 심볼릭 링크돼 있으면 abspath 비교로는
    그 링크 디렉토리가 걸러지지 않아 래퍼가 자기 자신을 재실행하는 무한 루프가 된다. 후보 파일
    자체의 realpath가 래퍼 디렉토리 안을 가리키는 경우(개별 파일 symlink)도 제외한다."""
    wrapper_dir = os.path.realpath(os.path.dirname(os.path.abspath(__file__)))
    paths = os.environ.get("PATH", "").split(os.pathsep)
    filtered = [p for p in paths if os.path.realpath(p) != wrapper_dir]

    def _is_self(candidate):
        real = os.path.realpath(candidate)
        return real == wrapper_dir or os.path.dirname(real) == wrapper_dir

    bin_names = ["codex.cmd", "codex.bat", "codex"] if os.name == "nt" else ["codex"]
    for p in filtered:
        for name in bin_names:
            candidate = os.path.join(p, name)
            if os.path.isfile(candidate) and os.access(candidate, os.X_OK) and not _is_self(candidate):
                return candidate

    # 필터링된 PATH로 shutil.which를 재시도한다(우리 자신을 다시 찾는 것을 원천 차단).
    found = shutil.which("codex", path=os.pathsep.join(filtered))
    if found and _is_self(found):
        return None
    return found


def marker_dir():
    d = os.path.join(os.path.expanduser("~"), ".tunaround", "autoarm")
    try:
        os.makedirs(d, exist_ok=True)
    except OSError:
        pass
    return d


def marker_path(thread_id):
    safe = sanitize_marker_id(thread_id)
    if not safe:
        return None
    return os.path.join(marker_dir(), f"{safe}.ctx")


def write_live_marker(thread_id, pid):
    """마커에 래퍼 자신의 PID를 기록한다(presence_scan::MarkerState::Pid 생존판정 근거).
    실패는 조용히 무시한다(마커는 best-effort 힌트일 뿐, 실패가 codex 실행을 막으면 안 된다)."""
    path = marker_path(thread_id)
    if path is None:
        return
    try:
        with open(path, "w", encoding="utf-8") as f:
            f.write(str(pid))
    except OSError:
        pass


def write_dead_marker(thread_id):
    """종료 확정 tombstone(claude SessionEnd 훅과 같은 컨벤션: 내용="dead", presence_scan
    ::MarkerState::Dead가 즉시 제외 판정)."""
    path = marker_path(thread_id)
    if path is None:
        return
    try:
        with open(path, "w", encoding="utf-8") as f:
            f.write("dead")
    except OSError:
        pass


def main():
    args = sys.argv[1:]

    real_codex = find_real_codex()
    if not real_codex:
        print("[codex-wrapper] 원본 'codex' 명령어를 찾을 수 없습니다.", file=sys.stderr)
        return 127

    cwd = os.getcwd()
    resume_thread_id = parse_resume_thread_id(args)

    bound = {"thread_id": resume_thread_id}
    stop_event = threading.Event()
    binder = None

    if resume_thread_id:
        write_live_marker(resume_thread_id, os.getpid())
    else:
        sessions_dir = default_codex_sessions_dir()
        if os.path.isdir(sessions_dir):

            def _bind():
                sid = find_bound_thread_id(sessions_dir, cwd, stop_event)
                if sid:
                    bound["thread_id"] = sid
                    write_live_marker(sid, os.getpid())

            binder = threading.Thread(target=_bind, daemon=True)
            binder.start()

    returncode = 1
    try:
        result = subprocess.run([real_codex] + args, check=False)
        returncode = result.returncode
    except KeyboardInterrupt:
        returncode = 130
    except Exception as e:  # noqa: BLE001 - 예상 밖 오류도 아래 finally의 마커 정리를 타야 한다.
        print(f"[codex-wrapper] codex 실행 에러: {e}", file=sys.stderr)
        returncode = 1
    finally:
        # 폴링 스레드에 중단을 알리고, 진행 중이던 한 주기가 끝날 시간을 짧게 준다(바인딩이
        # 막 성사됐는데 죽은 채로 넘어가는 경합을 줄인다 - 완전 차단은 아니고 완화).
        stop_event.set()
        if binder is not None:
            binder.join(timeout=SESSION_POLL_INTERVAL_SECS + 0.5)
        thread_id = bound["thread_id"]
        if thread_id:
            write_dead_marker(thread_id)

    return returncode


if __name__ == "__main__":
    sys.exit(main())
