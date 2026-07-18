# 세션 훅 공유 유틸(설정파일 조회·세션 id 정규화·deregister·구식 poll 정리). v2-44에서 무장 로직 제거.
"""tunaRound 세션 훅 공유 모듈.

v2-43까지는 세션별 detached poll을 띄우는 무장(ensure_armed) 코어였으나, v2-44에서 presence가
머신 스캐너(`tunaround presence-scan`)로 이관되며 무장·리핑·경합 락 코드가 전부 제거됐다
(그 코드가 유령 poll·중복 주입의 근원이었다, W1). 남은 것 = 설정 조회(cfg), 안전한 파일명
정규화, deregister 핑, 그리고 전환기용 구식 poll 정리(disarm_session)뿐이다.
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

    UUID(hex+하이픈)는 그대로 통과하므로 실사용에선 no-op이다. 훅들이 같은 마커/pidfile을
    가리키도록 이 함수를 공유해야 한다.
    """
    s = _SAFE_SESSION_RE.sub("_", str(session_id or "").strip())
    return s.strip(".") or ""


def state_dir() -> Path:
    d = Path.home() / ".tunaround" / "autoarm"
    d.mkdir(parents=True, exist_ok=True)
    return d


# tombstone(.ctx="dead") GC 임계값(분): 시간창 게이트(240분)+여유. 이보다 오래된 것만 지운다 - 같은
# id의 resume은 SessionStart의 write_marker가 .ctx를 재생성하므로 삭제해도 부활에 지장 없다.
_STALE_MARKER_MINUTES = 360


def gc_stale_markers(max_age_minutes: int = _STALE_MARKER_MINUTES) -> None:
    """오래된 tombstone(.ctx="dead") + 고아 .rx(대응 .ctx 없음)를 정리한다.

    SessionEnd(tuna-disarm)마다 .ctx에 "dead"를 남기지만 이를 지우는 경로가 없어 무한 축적된다
    (실측 2일 155개, 스캐너가 매 주기 전 .ctx를 read_to_string해 파일 수에 비례해 IO 증가). 이 함수는
    stale 창을 넘긴 tombstone만 지운다(살아있는 Pid/Unknown 마커는 스캐너 판정에 맡기고 건드리지
    않는다). best-effort: 개별 파일 오류는 무시하고 계속한다(정리 실패가 세션을 막으면 안 됨).
    """
    try:
        d = state_dir()
        cutoff = time.time() - max_age_minutes * 60
        # 1) 오래된 tombstone(dead) .ctx 삭제.
        for p in d.glob("*.ctx"):
            try:
                if p.stat().st_mtime >= cutoff:
                    continue  # 아직 신선 - 재생성(resume) 유예 기간 안이라 유지.
                if p.read_text(encoding="utf-8").strip() != "dead":
                    continue  # tombstone만 대상(Pid/Unknown 마커는 스캐너가 별도 판정).
                p.unlink()
            except Exception:
                continue
        # 2) 대응 .ctx가 없는(위 삭제 반영 후) 오래된 고아 .rx 삭제(수신 지시 1회 마커의 잔재 청소).
        ctx_stems = {p.stem for p in d.glob("*.ctx")}
        for p in d.glob("*.rx"):
            try:
                if p.stem in ctx_stems:
                    continue
                if p.stat().st_mtime >= cutoff:
                    continue
                p.unlink()
            except Exception:
                continue
    except Exception:
        pass


def is_temp_cwd(cwd) -> bool:
    """cwd가 시스템 temp 아래인지. 자동화가 %TEMP%에서 돌리는 headless 세션은 로스터 노이즈라
    안내·핑 대상에서 제외한다(2026-07-10 실측)."""
    try:
        if not cwd:
            return False
        p = Path(cwd).resolve()
        t = Path(tempfile.gettempdir()).resolve()
        return p == t or t in p.parents
    except Exception:
        return False


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


def pid_alive(pid: int) -> bool:
    try:
        if pid <= 0:  # 손상된 pidfile(-1 등)이 os.kill(-1,0) 특수동작으로 살아있다 오판되는 것 차단.
            return False
        if os.name == "nt":
            out = subprocess.run(
                ["tasklist", "/FI", f"PID eq {pid}", "/NH"],
                capture_output=True, text=True, timeout=5, check=False,
            )
            return str(pid) in out.stdout.split()
        os.kill(pid, 0)
        return True
    except Exception:
        return False


def kill_poll(pollpid) -> bool:
    """구식 detached poll을 종료하고 실제로 죽었는지 확인한다(전환기 전용). 반환=사망 확인 여부.

    kill이 실패했는데 pidfile만 지우면 살아남은 poll이 자가 재등록해 유령이 된다(2026-07-10 실측).
    호출부는 False면 pidfile을 보존해 다음 disarm에서 재시도해야 한다.
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


def broker_core() -> str:
    """브로커 MCP 코어 URL(핑·deregister base 도출에도 씀)."""
    return cfg("TUNA_BROKER_CORE", "http://127.0.0.1:8770/mcp")


def _is_claude_argv(argv: str) -> bool:
    """argv 앞 3개 토큰의 basename이 claude인지(node 래퍼 `node /path/claude` 커버, 봇리뷰).

    args 전체 부분문자열 매칭은 금물: 훅 커맨드 경로에 `.claude/hooks/...`가 들어가 부모 셸
    (일회성, 즉시 사망)이 owner로 오매칭된다 → 스캐너가 산 세션을 유령 판정. basename만 본다.
    """
    for tok in argv.split()[:3]:
        base = tok.replace("\\", "/").rstrip("/").rsplit("/", 1)[-1].strip('"').lower()
        if base in ("claude", "claude.exe"):
            return True
    return False


def _proc_map() -> dict:
    """{pid: (ppid, is_claude)} 스냅샷(owner 탐색용). 실패하면 빈 dict.

    win = CIM 이미지명(claude.exe 네이티브) / unix = args의 basename 매칭(comm은 node 래퍼를
    놓쳐 owner=0 → 마커 무력화, 봇리뷰 Major).
    """
    m = {}
    try:
        if os.name == "nt":
            out = subprocess.run(
                ["powershell", "-NoProfile", "-NonInteractive", "-Command",
                 "Get-CimInstance Win32_Process | ForEach-Object "
                 "{ \"$($_.ProcessId),$($_.ParentProcessId),$($_.Name)\" }"],
                capture_output=True, text=True, timeout=10, check=False,
            ).stdout
            for line in out.splitlines():
                parts = line.strip().split(",", 2)
                if len(parts) == 3 and parts[0].isdigit() and parts[1].isdigit():
                    m[int(parts[0])] = (int(parts[1]), "claude" in parts[2].lower())
        else:
            out = subprocess.run(
                ["ps", "-eo", "pid=,ppid=,args="],
                capture_output=True, text=True, timeout=10, check=False,
            ).stdout
            for line in out.splitlines():
                f = line.split(None, 2)
                if len(f) >= 3 and f[0].isdigit() and f[1].isdigit():
                    m[int(f[0])] = (int(f[1]), _is_claude_argv(f[2]))
    except Exception:
        return {}
    return m


def find_owner_pid() -> int:
    """이 훅을 낳은 세션(claude 프로세스)의 PID. 조상 체인에서 첫 claude 프로세스.

    못 찾으면 0(미상). getppid 폴백은 쓰지 않는다 - 훅 부모(셸)는 즉시 죽는 일회성 프로세스라
    그 pid를 마커에 적으면 스캐너가 산 세션을 죽은 것으로 오판한다(v2-44 §10 마커 안전 규칙).
    """
    m = _proc_map()
    if not m:
        return 0
    pid = os.getpid()
    for _ in range(16):  # 조상 체인 상한(순환 방지)
        entry = m.get(pid)
        if not entry:
            return 0
        ppid, is_claude = entry
        if is_claude:
            return pid
        if ppid <= 0 or ppid == pid:
            return 0
        pid = ppid
    return 0


def marker_path(session_id: str):
    """세션 마커(.ctx) 경로. 내용=owner claude PID(스캐너의 per-session 생존 판정 근거).
    guidance 1회 주입 dedupe와 같은 파일을 공유한다. sanitize 실패 시 None."""
    safe = sanitize_session_id(session_id)
    if not safe:
        return None
    return state_dir() / f"{safe}.ctx"


def write_marker(session_id: str) -> None:
    """마커에 owner PID를 기록한다. owner 미상(0)이면 sentinel "unknown"을 남긴다.

    빈 파일을 남기면 ping 자가치유 조건이 매 프롬프트 참이 되어 무거운 프로세스 조회가
    반복된다(봇리뷰 critical). "unknown"은 스캐너에서 MarkerState::Unknown=보수적 유지.
    """
    p = marker_path(session_id)
    if p is None:
        return
    try:
        owner = find_owner_pid()
        # 원자적 교체(tmp + os.replace, CodeRabbit): poll이 이 마커를 주기 판독해 종료를 판정하므로
        # (이슈 #118), 쓰기 도중의 부분 내용·공유 위반이 판독측에 노출되지 않게 한다.
        tmp = p.with_name(p.name + ".tmp")
        tmp.write_text(str(owner) if owner > 0 else "unknown", encoding="utf-8")
        os.replace(tmp, p)
    except Exception:
        pass


def deregister(agent, core, token=None) -> None:
    """브로커 로스터에서 즉시 등록해제(loopback POST). 실패는 조용히 통과.

    등록해제가 안 돼도 스캐너 다음 주기(±15초) 또는 heartbeat TTL로 자연 반영되므로 best-effort.
    """
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


def post_dashboard(path: str, body: dict, timeout: float = 0.75) -> None:
    """브로커 대시보드 쓰기 엔드포인트로 소형 POST(훅 공용, 이슈 #123 turn-ping 등).

    deregister와 같은 규약: base=broker_core에서 /mcp 절단, loopback HTTP 전용, 토큰은 cfg 폴백,
    실패는 조용히 통과(훅은 세션을 절대 막지 않는다).
    """
    c = broker_core().rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    if not base.startswith(("http://", "https://")):
        return
    req = urllib.request.Request(base + path, data=json.dumps(body).encode(), method="POST")
    req.add_header("Content-Type", "application/json")
    token = cfg("TUNA_BROKER_TOKEN", "")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        urllib.request.urlopen(req, timeout=timeout).read()
    except Exception:
        pass


def disarm_session(session_id: str) -> str:
    """구식 detached poll 정리(전환기 전용): kill(사망 확인) + deregister + pidfile 삭제.

    반환="DISARMED"|"NOT_FOUND"|"KILL_FAILED". v2-44 세션엔 pidfile이 없어 NOT_FOUND가 정상
    경로다(호출부가 deregister 핑만 직접 보낸다). kill 실패 시 pidfile 보존(유령 방지 - 재시도).
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
    deregister(info.get("agent"), info.get("core") or broker_core(), cfg("TUNA_BROKER_TOKEN"))
    try:
        pidfile.unlink()
    except Exception:
        pass
    return "DISARMED"
