#!/usr/bin/env python3
# SessionStart 훅: A2A 수신법 짧은 안내를 세션당 1회 주입한다(무장은 presence 스캐너 소관, v2-44).
"""tunaRound v2-44 세션 안내 훅.

presence(로스터 등록·해제)는 머신당 스캐너 데몬(`tunaround presence-scan`)이 관리하므로
이 훅은 아무것도 기동하지 않는다. 하는 일은 하나: 이 세션이 task를 수신·처리하는 법을
~5줄로 안내한다(전문 레시피는 tuna-broker MCP instructions가 이미 상시 제공 = 중복 금지, W2).

훅 다중 발화(전역·프로젝트 이중 등록, matcher 중복)에도 주입이 한 번이 되도록
세션별 마커 파일로 1회를 보장한다(W1). opt-in이 아니면 조용히 no-op.
"""
import json
import os
import sys

try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from tuna_arm import broker_core, cfg, is_temp_cwd, sanitize_session_id, state_dir
except Exception:
    sys.exit(0)  # 공유 모듈이 없으면 세션을 막지 않고 조용히 통과.


def emit_context(text: str) -> None:
    """SessionStart 훅 출력 계약: hookSpecificOutput.additionalContext로 세션에 문자열 주입."""
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    }))


def main() -> int:
    if cfg("TUNA_AUTOARM") != "1":
        return 0
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
    except Exception:
        payload = {}
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        return 0
    cwd = payload.get("cwd") or os.getcwd()
    if is_temp_cwd(cwd) and not cfg("TUNA_AUTOARM_PROJECT"):
        return 0  # 자동화 headless 세션엔 안내도 생략(컨텍스트 비오염).

    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return 0
    # 주입 1회 보장 마커(W1): 훅이 몇 번 발화해도 안내는 세션당 한 번만.
    marker = state_dir() / f"{safe_id}.ctx"
    try:
        fd = os.open(str(marker), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.close(fd)
    except FileExistsError:
        return 0
    except Exception:
        pass  # 마커 실패 시에도 안내는 낸다(중복 가능성 < 안내 유실).

    core = broker_core()
    c = core.rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    tuna_bin = cfg("TUNA_BIN", "tunaround")
    emit_context(
        f"[tunaround] 이 세션의 로스터 presence는 머신 스캐너가 자동 관리합니다(별도 등록 불요).\n"
        f"[A2A 수신] task를 받으려면: Monitor(command=\"{tuna_bin} poll --core {core} --agent {session_id} --interval 15\", description=\"A2A 수신\", persistent=true)\n"
        f"  `TASK <id> ::` 도착 시 tuna-broker MCP의 claim_task→답변→complete_task(불가 시 fail_task)로 처리합니다.\n"
        f"  tuna-broker MCP가 안 붙었으면 CLI로: `{tuna_bin} task poll|claim|get|complete|fail --core {core}`.\n"
        f"[총괄로 쓸 때] 던진 task 결과 수신: Monitor(command=\"{tuna_bin} watch-results --core {base} --dispatcher dashboard --digest 60\", persistent=true)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
