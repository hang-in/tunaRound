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
                capture_output=True, text=True, timeout=10,
            ).stdout
            for line in out.splitlines():
                parts = line.strip().split(",", 2)
                if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
                    m[int(parts[0])] = (int(parts[1]), parts[2].lower())
        else:
            out = subprocess.run(
                ["ps", "-eo", "pid=,ppid=,comm="],
                capture_output=True, text=True, timeout=10,
            ).stdout
            for line in out.splitlines():
                f = line.split(None, 2)
                if len(f) >= 3 and f[0].isdigit() and f[1].isdigit():
                    m[int(f[0])] = (int(f[1]), f[2].lower())
    except Exception:
        return {}
    return m


def find_owner_pid(pmap: dict = None) -> int:
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
            # orphan: poll kill → deregister → pidfile 제거
            pollpid = info.get("pid")
            if pollpid and int(pollpid) in pmap:
                try:
                    if os.name == "nt":
                        subprocess.run(["taskkill", "/PID", str(pollpid), "/F"],
                                       capture_output=True, timeout=5)
                    else:
                        os.kill(int(pollpid), 9)
                except Exception:
                    pass
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
            flags = 0x00000008 | 0x00000200  # DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP
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


def ensure_armed(session_id: str, cwd: str):
    """이 세션을 무장(idempotent)한다. 반환=(agent, core) 또는 None.

    opt-in(TUNA_AUTOARM=1) + 토큰 필요. 이미 무장(pidfile 살아있음)이면 재기동 없이 (agent, core) 반환.
    """
    if cfg("TUNA_AUTOARM") != "1":
        return None
    session_id = str(session_id or "").strip()
    if not session_id:
        return None
    if not cfg("TUNA_BROKER_TOKEN"):
        return None

    core = broker_core()
    tuna_bin = cfg("TUNA_BIN", "tunaround")
    host = os.environ.get("COMPUTERNAME") or os.environ.get("HOSTNAME") or "host"
    user = os.environ.get("USERNAME") or os.environ.get("USER") or "user"
    machine = cfg("TUNA_MACHINE") or ("win" if os.name == "nt" else "unix")
    project = cfg("TUNA_AUTOARM_PROJECT") or Path(cwd).name or "unknown"
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
        pid = launch_detached(cmd, log_path, env=child_env())
    except Exception:
        return None

    # owner_pid = 이 세션의 claude 프로세스. 실제 launch할 때만 조회(핑 no-op 경로엔 지연 없음).
    # 창 X·크래시로 SessionEnd가 안 돌아도 autoarm 리퍼가 owner 죽음을 보고 이 poll을 청소한다.
    owner_pid = find_owner_pid()
    pidfile.write_text(json.dumps({
        "pid": pid, "agent": agent, "display_name": display, "core": core,
        "tags": tags, "log": str(log_path), "session_id": session_id,
        "owner_pid": owner_pid,
    }), encoding="utf-8")
    return (agent, core)
