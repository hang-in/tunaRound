# autoarm(SessionStart)·session-ping(UserPromptSubmit) 공유: 세션 무장(idempotent) 코어.
"""tunaRound 세션 무장 공유 모듈(설계 v2-42).

ensure_armed(session_id, cwd) = 이 세션의 detached poll(register+heartbeat)이 없으면 띄운다.
이미 살아있으면 no-op. 반환=(agent_uuid, core) 또는 None(무장 못 함/비대상).
opt-in(TUNA_AUTOARM=1)·토큰 존재를 전제로만 무장한다. 실패는 조용히 None(세션을 절대 막지 않음).
"""
import json
import os
import re
import subprocess
import tempfile
import time
import urllib.request
from pathlib import Path

# session_id를 파일명으로 쓸 때 경로 이탈(../, 절대경로, 구분자)을 막는 허용 문자 집합.
_SAFE_SESSION_RE = re.compile(r"[^A-Za-z0-9._-]")

_CONFIG_CACHE = None  # 훅은 이벤트마다 새 프로세스라 프로세스 내 캐시로 충분(스테일 없음).


def config_path() -> Path:
    """무장 설정파일 경로. env TUNA_CONFIG로 재정의 가능(기본=~/.tunaround/config)."""
    override = os.environ.get("TUNA_CONFIG")
    if override:
        return Path(override)
    return Path.home() / ".tunaround" / "config"


def load_config() -> dict:
    """~/.tunaround/config를 KEY=VALUE(dotenv 유사)로 읽는다. 없거나 오류면 빈 dict.

    빈 줄·`#` 주석·`=` 없는 줄은 무시. 값의 앞뒤 따옴표는 벗긴다.
    """
    out = {}
    try:
        # utf-8-sig: Windows 편집기가 넣는 BOM을 벗겨 첫 키가 '﻿TUNA_...'로 깨지지 않게 한다.
        for raw in config_path().read_text(encoding="utf-8-sig").splitlines():
            line = raw.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, v = line.split("=", 1)
            v = v.strip()
            if len(v) >= 2 and v[0] == v[-1] and v[0] in ("'", '"'):
                v = v[1:-1]  # 따옴표 값은 리터럴(인라인 주석·공백 그대로 보존).
            else:
                # 따옴표 없는 값: " #" 이후를 인라인 주석으로 제거(TOKEN=abc # 메모 → abc).
                hidx = v.find(" #")
                if hidx != -1:
                    v = v[:hidx].rstrip()
            out[k.strip()] = v
    except Exception:
        pass
    return out


def cfg(key: str, default=None):
    """설정값 조회: ~/.tunaround/config(파일) 우선 → 환경변수 → default.

    파일 우선인 이유(설계 v2-43 §5-1): env는 터미널 launch 시점에 고정돼, setx/토큰
    로테이션이 이미 열린 터미널에 반영되지 않아 훅이 no-op한다. 파일은 런타임에 읽어 신선도 무관.
    """
    global _CONFIG_CACHE
    if _CONFIG_CACHE is None:
        _CONFIG_CACHE = load_config()
    if key in _CONFIG_CACHE and _CONFIG_CACHE[key] != "":
        return _CONFIG_CACHE[key]
    return os.environ.get(key, default)


def sanitize_session_id(session_id: str) -> str:
    """session_id를 안전한 파일명 조각으로 정규화한다(허용 외 문자→'_', 경로 이탈 차단).

    UUID(hex+하이픈)는 그대로 통과하므로 실사용에선 no-op이다. autoarm/disarm/ping이
    같은 pidfile을 가리키도록 세 훅이 이 함수를 공유해야 한다.
    """
    s = _SAFE_SESSION_RE.sub("_", str(session_id or "").strip())
    return s.strip(".") or ""


def state_dir() -> Path:
    d = Path.home() / ".tunaround" / "autoarm"
    d.mkdir(parents=True, exist_ok=True)
    return d


def project_from_cwd(cwd) -> str:
    """cwd 폴더명 → project 태그. home에서 띄운 세션은 개인 폴더명(사용자명) 대신 'home'.

    cwd가 비면 'unknown'(빈 문자열이 Path 해석으로 훅 프로세스의 CWD로 둔갑하는 것 방지)."""
    try:
        if not cwd:
            return "unknown"
        p = Path(cwd).resolve()
        if p == Path.home().resolve():
            return "home"
        return p.name or "unknown"
    except Exception:
        return "unknown"


def is_temp_cwd(cwd) -> bool:
    """cwd가 시스템 temp 아래인지. 자동화가 %TEMP%에서 돌리는 headless 세션은 로스터 노이즈라
    무장에서 제외한다(v2-40 discover의 노이즈 필터 역할을 presence 모델에서 재현, 2026-07-10 실측)."""
    try:
        if not cwd:
            return False
        p = Path(cwd).resolve()
        t = Path(tempfile.gettempdir()).resolve()
        return p == t or t in p.parents
    except Exception:
        return False


def _acquire_arm_lock(sdir: Path, safe_id: str):
    """세션별 무장 락을 원자적으로 생성한다. 반환=락 Path 또는 None(다른 훅이 무장 중).

    SessionStart(autoarm)와 첫 프롬프트(session-ping)가 동시에 무장을 시도하면 poll이 2개 떠서
    한쪽이 pidfile 없는 유령이 된다(2026-07-10 실측: Temp·luckyCAD). 크래시 잔재(stale) 락은
    15초 지나면 치우되 이번 호출은 양보한다(다음 이벤트가 무장)."""
    lock = sdir / f"{safe_id}.lock"
    try:
        fd = os.open(str(lock), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.close(fd)
        return lock
    except FileExistsError:
        try:
            if time.time() - lock.stat().st_mtime > 15:
                lock.unlink()
                # stale(크래시 잔재) 정리 후 즉시 1회 재시도 - 양보하면 이번 세션이 무장 없이 남는다.
                fd = os.open(str(lock), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
                os.close(fd)
                return lock
        except (FileExistsError, OSError):
            pass
        return None
    except Exception:
        return None


def is_tunaround_pid(pid: int) -> bool:
    """해당 PID가 실제 tunaround 프로세스인지 확인한다(PID 재사용으로 엉뚱한 프로세스를 죽이지 않게)."""
    try:
        if os.name == "nt":
            out = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                capture_output=True, text=True, timeout=5, check=False,
            )
            return "tunaround" in out.stdout.lower()
        try:
            with open(f"/proc/{pid}/cmdline", "rb") as f:
                return b"tunaround" in f.read().lower()
        except OSError:
            out = subprocess.run(
                ["ps", "-p", str(pid), "-o", "command="],
                capture_output=True, text=True, timeout=5, check=False,
            )
            return "tunaround" in out.stdout.lower()
    except Exception:
        return False


def kill_poll(pollpid) -> bool:
    """poll 프로세스를 종료하고 실제로 죽었는지 확인한다. 반환=사망 확인 여부.

    kill이 실패했는데 pidfile만 지우면, 살아남은 poll이 heartbeat "미등록" 응답에
    자가 재등록해 유령이 된다(2026-07-10 실측: win-claude-Temp·luckyCAD 중복).
    호출부는 False면 pidfile을 보존해 다음 disarm/리핑에서 재시도해야 한다.
    """
    try:
        pollpid = int(pollpid)
    except (TypeError, ValueError):
        return True  # pid 기록이 없거나 손상 = 죽일 대상 없음(사망 취급).
    if pollpid <= 0:
        return True  # 음수/0은 Unix os.kill에서 프로세스 그룹으로 번지므로 시도하지 않는다.
    if not is_tunaround_pid(pollpid):
        return True  # 이미 죽었거나 PID 재사용(다른 프로세스) = poll 없음. 남의 프로세스를 죽이지 않는다.
    try:
        if os.name == "nt":
            # /T: poll이 --on-task로 낳은 자식(codex-inject 등)까지 트리로 정리(고아 방지).
            subprocess.run(["taskkill", "/PID", str(pollpid), "/F", "/T"],
                           capture_output=True, timeout=5, check=False)
        else:
            os.kill(pollpid, 9)
    except Exception:
        pass  # 이미 죽은 프로세스(ProcessLookupError 등)면 아래 확인에서 사망으로 판정된다.
    if not pid_alive(pollpid):
        return True
    time.sleep(0.3)  # taskkill 반영 지연 재확인.
    return not pid_alive(pollpid)


def pid_alive(pid: int) -> bool:
    try:
        if pid <= 0:  # 손상된 pidfile(-1 등)이 os.kill(-1,0) 특수동작으로 살아있다 오판되는 것 차단.
            return False
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


def proc_map() -> dict:
    """{pid: (ppid, name_lower)} 스냅샷. 한 번만 떠서 owner 탐색·리핑에 재사용한다.
    실패하면 빈 dict(호출부가 폴백). 프로세스 조회는 SessionStart에서만(핑 지연 경로 아님)."""
    m = {}
    try:
        if os.name == "nt":
            # PowerShell CIM: Win11에서 wmic가 제거될 수 있어 항상 있는 CIM을 쓴다.
            out = subprocess.run(
                ["powershell", "-NoProfile", "-Command",
                 "Get-CimInstance Win32_Process | ForEach-Object "
                 "{ \"$($_.ProcessId),$($_.ParentProcessId),$($_.Name)\" }"],
                capture_output=True, text=True, timeout=10, check=False,
            ).stdout
            for line in out.splitlines():
                parts = line.strip().split(",", 2)
                if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
                    m[int(parts[0])] = (int(parts[1]), parts[2].lower())
        else:
            out = subprocess.run(
                ["ps", "-eo", "pid=,ppid=,comm="],
                capture_output=True, text=True, timeout=10, check=False,
            ).stdout
            for line in out.splitlines():
                f = line.split(None, 2)
                if len(f) >= 3 and f[0].isdigit() and f[1].isdigit():
                    m[int(f[0])] = (int(f[1]), f[2].lower())
    except Exception:
        return {}
    return m


def find_owner_pid(pmap=None) -> int:
    """이 훅을 낳은 세션(claude 프로세스)의 PID. getpid부터 조상을 올라가며 이름에 'claude'가
    든 첫 프로세스를 owner로 본다. 못 찾으면 getppid 폴백(0이면 미지정)."""
    m = pmap if pmap is not None else proc_map()
    if not m:
        return os.getppid()
    pid = os.getpid()
    for _ in range(16):  # 조상 체인 상한(순환 방지)
        entry = m.get(pid)
        if not entry:
            break
        ppid, name = entry
        if "claude" in name:
            return pid
        if ppid <= 0 or ppid == pid:
            break
        pid = ppid
    return os.getppid()


def _deregister(agent: str, core: str, token: str) -> None:
    """브로커 로스터에서 즉시 등록해제(loopback POST). 실패는 조용히 통과."""
    if not agent or not core:
        return
    c = str(core).rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    if not base.startswith(("http://", "https://")):
        return  # loopback HTTP 전용(file: 등 비정상 스킴 차단)
    body = json.dumps({"agent": agent}).encode()
    req = urllib.request.Request(base + "/dashboard/deregister", data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        urllib.request.urlopen(req, timeout=0.75).read()
    except Exception:
        pass


def reap_orphans(pmap: dict, current_session_id: str = "") -> int:
    """owner 세션이 죽은 orphan poll을 청소한다(창 X·크래시로 SessionEnd 미발화 대비).

    각 pidfile의 owner_pid(=세션 claude 프로세스)가 proc_map에 없으면 = 세션 죽음 →
    그 poll을 kill + deregister + pidfile 삭제. 자기 자신(current_session_id)과 owner_pid
    미기록(레거시) pidfile은 건드리지 않는다. pmap이 비면(프로세스 조회 실패) 살아있는
    세션까지 전량 오판되므로 아무것도 하지 않는다. 반환=청소 개수. 실패는 조용히 무시."""
    if not pmap:
        return 0  # 스냅샷 실패 = 판단 불가 → 보존(전량 리핑 방지)
    token = cfg("TUNA_BROKER_TOKEN")
    reaped = 0
    try:
        pidfiles = list(state_dir().glob("*.json"))  # 순회 중 unlink하므로 리스트화
    except Exception:
        return 0
    for pf in pidfiles:
        try:
            info = json.loads(pf.read_text(encoding="utf-8"))
            if info.get("session_id") == current_session_id:
                continue
            owner = info.get("owner_pid")
            if not owner:
                continue  # owner 미기록(레거시)은 판단 불가 → 보존
            if int(owner) in pmap:
                continue  # 세션(owner) 살아있음
            # orphan: poll kill(사망 확인) → deregister → pidfile 제거.
            # 사망 미확인이면 pidfile 보존(다음 리핑에 재시도) - deregister만 하면 poll이 자가 재등록해 유령이 된다.
            if not kill_poll(info.get("pid")):
                continue
            _deregister(info.get("agent"), info.get("core") or broker_core(), token)
            try:
                pf.unlink()
            except Exception:
                pass
            reaped += 1
        except Exception:
            continue  # 손상 pidfile 하나가 나머지 청소를 막지 않게 파일 단위 격리
    return reaped


def child_env() -> dict:
    """detached poll 자식에 넘길 env. 설정파일 값(토큰 등)을 env로 승격해,
    부모 터미널 env가 stale/미설정이어도 poll이 브로커 인증에 성공하게 한다.

    토큰은 --token(argv 노출) 대신 env로만 전달한다(로테이션 교훈). core는 --core로 넘기지만
    default_machine 등 다른 참조를 위해 TUNA_MACHINE도 승격한다.
    """
    env = dict(os.environ)
    for key in ("TUNA_BROKER_TOKEN", "TUNA_BROKER_CORE", "TUNA_MACHINE"):
        val = cfg(key)
        if val:
            env[key] = val
    return env


def launch_detached(cmd: list, log_path: Path, env: dict = None) -> int:
    """세션·하네스 수명과 무관하게 상주하도록 완전 분리된 프로세스로 기동한다."""
    with open(log_path, "ab") as log:
        if os.name == "nt":
            # DETACHED_PROCESS는 콘솔이 필요한 자식(.cmd → cmd.exe, codex app-server 등)이
            # 보이는 콘솔 창을 새로 할당해 터미널 창이 튀어나온다(2026-07-11 실측). NO_WINDOW는
            # 보이지 않는 콘솔을 부여해 창 없이 상주한다(부모 분리·생존 특성은 동일).
            flags = subprocess.CREATE_NO_WINDOW | subprocess.CREATE_NEW_PROCESS_GROUP
            proc = subprocess.Popen(
                cmd, stdout=log, stderr=log, stdin=subprocess.DEVNULL,
                creationflags=flags, close_fds=True, env=env,
            )
        else:
            proc = subprocess.Popen(
                cmd, stdout=log, stderr=log, stdin=subprocess.DEVNULL,
                start_new_session=True, close_fds=True, env=env,
            )
    return proc.pid


def broker_core() -> str:
    """브로커 MCP 코어 URL(핑용 base 도출에도 씀)."""
    return cfg("TUNA_BROKER_CORE", "http://127.0.0.1:8770/mcp")


def ensure_armed(session_id: str, cwd: str, pmap=None):
    """이 세션을 무장(idempotent)한다. 반환=(agent, core) 또는 None.

    opt-in(TUNA_AUTOARM=1) + 토큰 필요. 이미 무장(pidfile 살아있음)이면 재기동 없이 (agent, core) 반환.
    동시 무장은 세션별 락으로 직렬화(양보 측도 (agent, core) 반환 - 상대가 무장을 끝낸다).
    pmap=proc_map() 스냅샷 재사용(옵션, autoarm이 리퍼와 공유해 스냅샷 중복 방지).
    """
    if cfg("TUNA_AUTOARM") != "1":
        return None
    session_id = str(session_id or "").strip()
    if not session_id:
        return None
    if not cfg("TUNA_BROKER_TOKEN"):
        return None
    if is_temp_cwd(cwd) and not cfg("TUNA_AUTOARM_PROJECT"):
        return None  # 시스템 temp의 자동화 headless 세션 = 로스터 노이즈(명시 프로젝트 지정 시 예외).

    core = broker_core()
    tuna_bin = cfg("TUNA_BIN", "tunaround")
    host = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
    user = os.environ.get("USERNAME") or os.environ.get("USER") or "user"
    machine = cfg("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    project = cfg("TUNA_AUTOARM_PROJECT") or project_from_cwd(cwd)
    role = cfg("TUNA_AUTOARM_ROLE", "session")
    agent = session_id  # uuid=세션 id(라우팅·discover overlay 키, 설계 §2.1)
    display = cfg("TUNA_AUTOARM_AGENT") or f"{machine}-claude-{project}"
    interval = cfg("TUNA_AUTOARM_INTERVAL", "15")
    tags = (
        f"machine={machine},runner=claude,role={role},project={project},"
        f"user={user},host={host},session={session_id}"
    )

    sdir = state_dir()
    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return None
    pidfile = sdir / f"{safe_id}.json"
    log_path = sdir / f"{safe_id}.log"

    def _already_alive() -> bool:
        if not pidfile.exists():
            return False
        try:
            prev = json.loads(pidfile.read_text(encoding="utf-8"))
            return pid_alive(int(prev.get("pid", -1)))
        except Exception:
            return False  # 손상된 pidfile은 무시하고 새로 기동.

    # 이미 무장(살아있음)이면 재기동 없이 반환(락 불필요 빠른 경로).
    if _already_alive():
        return (agent, core)

    # 무장 경합 방지 락: 획득 실패 = 다른 훅(autoarm↔ping)이 무장 중 → 그쪽에 맡긴다.
    lock = _acquire_arm_lock(sdir, safe_id)
    if lock is None:
        return (agent, core)
    try:
        if _already_alive():  # 락 획득 사이에 상대가 무장을 끝냈을 수 있다(double-checked).
            return (agent, core)
        cmd = [
            tuna_bin, "poll", "--core", core, "--agent", agent,
            "--display-name", display, "--tags", tags, "--interval", str(interval),
        ]
        try:
            pid = launch_detached(cmd, log_path, env=child_env())
        except Exception:
            return None

        # owner_pid = 이 세션의 claude 프로세스. 실제 launch할 때만 조회(핑 no-op 경로엔 지연 없음).
        # 창 X·크래시로 SessionEnd가 안 돌아도 autoarm 리퍼가 owner 죽음을 보고 이 poll을 청소한다.
        owner_pid = find_owner_pid(pmap)
        try:
            pidfile.write_text(json.dumps({
                "pid": pid, "agent": agent, "display_name": display, "core": core,
                "tags": tags, "log": str(log_path), "session_id": session_id,
                "owner_pid": owner_pid,
            }), encoding="utf-8")
        except Exception:
            kill_poll(pid)  # pidfile 없는 poll = 리퍼 사각지대 유령 → 방금 띄운 poll을 회수.
            return None
        return (agent, core)
    finally:
        try:
            lock.unlink()
        except Exception:
            pass


def ensure_codex_armed(session_id: str, cwd: str, display_name=None, project=None, owner_pid: int = 0):
    """Codex 세션을 무장(idempotent)한다(v2-43 §5-3, scripts/codex_wrapper.py가 호출).

    codex는 claude 훅이 안 잡으므로 래퍼가 세션 id를 만들어 넘긴다. TUNA_AUTOARM=1 +
    토큰 있을 때만 동작. owner_pid=래퍼 프로세스(codex 수명과 동일 = 리퍼 orphan 판정 기준).
    """
    if cfg("TUNA_AUTOARM") != "1":
        return None
    session_id = str(session_id or "").strip()
    if not session_id:
        return None
    if not cfg("TUNA_BROKER_TOKEN"):
        return None
    if is_temp_cwd(cwd) and not (project or cfg("TUNA_AUTOARM_PROJECT")):
        return None  # 시스템 temp의 자동화 headless 세션 = 로스터 노이즈(명시 프로젝트 지정 시 예외).

    core = broker_core()
    tuna_bin = cfg("TUNA_BIN", "tunaround")
    host = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
    user = os.environ.get("USERNAME") or os.environ.get("USER") or "user"
    machine = cfg("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")

    proj = project or cfg("TUNA_AUTOARM_PROJECT") or project_from_cwd(cwd)
    role = cfg("TUNA_AUTOARM_ROLE", "supervised")
    agent = session_id
    display = display_name or cfg("TUNA_AUTOARM_AGENT") or f"{machine}-codex-{proj}"
    interval = cfg("TUNA_AUTOARM_INTERVAL", "15")
    tags = (
        f"machine={machine},runner=codex,role={role},project={proj},"
        f"user={user},host={host},session={session_id}"
    )

    sdir = state_dir()
    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return None
    pidfile = sdir / f"{safe_id}.json"
    log_path = sdir / f"{safe_id}.log"

    def _already_alive() -> bool:
        if not pidfile.exists():
            return False
        try:
            prev = json.loads(pidfile.read_text(encoding="utf-8"))
            return pid_alive(int(prev.get("pid", -1)))
        except Exception:
            return False

    if _already_alive():
        return (agent, core)

    lock = _acquire_arm_lock(sdir, safe_id)  # 무장 경합 방지(ensure_armed와 동일).
    if lock is None:
        return (agent, core)
    try:
        if _already_alive():
            return (agent, core)
        cmd = [
            tuna_bin, "poll", "--core", core, "--agent", agent,
            "--display-name", display, "--tags", tags, "--interval", str(interval),
        ]
        try:
            pid = launch_detached(cmd, log_path, env=child_env())
        except Exception:
            return None

        if not owner_pid or owner_pid <= 0:
            owner_pid = os.getppid()

        try:
            pidfile.write_text(json.dumps({
                "pid": pid, "agent": agent, "display_name": display, "core": core,
                "tags": tags, "log": str(log_path), "session_id": session_id,
                "owner_pid": owner_pid,
            }), encoding="utf-8")
        except Exception:
            kill_poll(pid)  # pidfile 없는 poll = 리퍼 사각지대 유령 → 방금 띄운 poll을 회수.
            return None
        return (agent, core)
    finally:
        try:
            lock.unlink()
        except Exception:
            pass


def disarm_session(session_id: str) -> str:
    """세션 poll 종료(사망 확인) + 즉시 deregister + pidfile 삭제.

    반환="DISARMED"|"NOT_FOUND"|"KILL_FAILED". codex 래퍼와 __main__ stop이 공유한다.
    poll이 이미 죽어 있으면 사망 확인으로 통과해 정리를 계속한다. kill이 실패해 poll이
    살아 있으면 pidfile을 보존하고 KILL_FAILED를 반환한다(유령 방지 - 다음 disarm/리핑에서 재시도).
    """
    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return "NOT_FOUND"
    pidfile = state_dir() / f"{safe_id}.json"
    if not pidfile.exists():
        return "NOT_FOUND"
    try:
        info = json.loads(pidfile.read_text(encoding="utf-8"))
    except Exception:
        info = {}
    if not kill_poll(info.get("pid")):
        return "KILL_FAILED"
    _deregister(info.get("agent"), info.get("core") or broker_core(), cfg("TUNA_BROKER_TOKEN"))
    try:
        pidfile.unlink()
    except Exception:
        pass
    return "DISARMED"


if __name__ == "__main__":
    import sys
    if len(sys.argv) >= 2:
        if sys.argv[1] == "start" and len(sys.argv) >= 3:
            try:
                res = ensure_codex_armed(
                    sys.argv[2], os.getcwd(),
                    sys.argv[3] if len(sys.argv) >= 4 else None,
                    sys.argv[4] if len(sys.argv) >= 5 else None,
                    int(sys.argv[5]) if len(sys.argv) >= 6 and sys.argv[5].isdigit() else 0,
                )
                print(f"ARMED:{res[0]}:{res[1]}" if res else "FAILED")
            except Exception as e:
                print(f"ERROR:{e}")
        elif sys.argv[1] == "stop" and len(sys.argv) >= 3:
            try:
                print(disarm_session(sys.argv[2]))
            except Exception as e:
                print(f"ERROR:{e}")

