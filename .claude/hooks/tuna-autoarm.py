#!/usr/bin/env python3
# SessionStart 훅: opt-in 세션을 브로커 로스터에 자동 무장(detached tunaround poll 기동)한다.
"""tunaRound v2-40 S1 자동무장 훅.

TUNA_AUTOARM=1일 때만 동작한다. Claude Code 세션이 시작되면:
  1. orphan 리핑(창 X·크래시로 SessionEnd가 못 정리한 poll 청소).
  2. `tuna_arm.ensure_armed`로 detached poll 기동(register_agent + heartbeat).
     실제 무장 로직은 session-ping 훅과 tuna_arm 단일 소스를 공유한다(중복 구현이
     무장 경합→유령 poll의 근원이었음, 2026-07-10 실측 후 일원화).
  3. additionalContext로 세션에 "무장됨 + 수신법"을 주입한다.

opt-in이 아니거나 토큰이 없으면 조용히 no-op(exit 0)한다 - 세션 시작을 절대 막지 않는다.
tuna_arm 모듈이 없으면(설치 손상) 무장 없이 조용히 통과한다.
"""
import json
import os
import sys

try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    from tuna_arm import (
        cfg,
        ensure_armed,
        is_temp_cwd,
        pid_alive,
        proc_map,
        reap_orphans,
        sanitize_session_id,
        state_dir,
    )
except Exception:
    sys.exit(0)  # 무장 코어가 없으면 세션을 막지 않고 조용히 통과.


def emit_context(text: str) -> None:
    """SessionStart 훅 출력 계약: hookSpecificOutput.additionalContext로 세션에 문자열 주입."""
    print(json.dumps({
        "hookSpecificOutput": {
            "hookEventName": "SessionStart",
            "additionalContext": text,
        }
    }))


def main() -> int:
    # opt-in이 아니면 즉시 no-op(출력 없음 = 컨텍스트 오염 없음). 설정파일 우선(env 신선도 무관).
    if cfg("TUNA_AUTOARM") != "1":
        return 0

    # 대화형 터미널에서 stdin 없이 실행하면 json.load가 EOF를 무한 대기하므로 isatty로 가드.
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
    except Exception:
        payload = {}
    # session_id가 없으면 여러 세션이 unknown.json을 공유해 SessionEnd가 남의 poll을 죽일 수 있다.
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        emit_context("[tuna-autoarm] session_id가 없어 무장하지 않았습니다(세션별 pidfile 충돌 방지).")
        return 0
    cwd = payload.get("cwd") or os.getcwd()

    if not cfg("TUNA_BROKER_TOKEN"):
        emit_context(
            "[tuna-autoarm] TUNA_AUTOARM=1이나 TUNA_BROKER_TOKEN 미설정이라 무장하지 않았습니다. "
            "~/.tunaround/config 또는 env에 토큰을 설정하면 이 세션이 브로커 로스터에 자동 등록됩니다."
        )
        return 0

    # 프로세스 스냅샷 1회(orphan 리핑 + ensure_armed의 owner 탐색에 재사용). SessionStart에서만.
    pmap = proc_map()
    reap_orphans(pmap, session_id)

    # 시스템 temp의 자동화 headless 세션은 조용히 제외(로스터 노이즈·컨텍스트 주입 모두 생략).
    if is_temp_cwd(cwd) and not cfg("TUNA_AUTOARM_PROJECT"):
        return 0

    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        emit_context("[tuna-autoarm] session_id가 안전한 파일명으로 정규화되지 않아 무장하지 않았습니다.")
        return 0
    pidfile = state_dir() / f"{safe_id}.json"

    # 이미 무장돼 있었는지(메시지 분기용). 실제 무장·경합 직렬화는 ensure_armed가 담당한다.
    already = False
    try:
        prev = json.loads(pidfile.read_text(encoding="utf-8"))
        already = pid_alive(int(prev.get("pid", -1)))
    except Exception:
        pass

    res = ensure_armed(session_id, cwd, pmap=pmap)
    if not res:
        tuna_bin = cfg("TUNA_BIN", "tunaround")
        emit_context(
            f"[tuna-autoarm] 무장 실패(poll 기동 실패). '{tuna_bin}' 경로(TUNA_BIN)를 확인하세요. "
            "다음 프롬프트에서 자동 재시도됩니다."
        )
        return 0
    agent, core = res

    # 표시 정보는 최종 pidfile에서 읽는다(경합 시 다른 훅이 무장했어도 실제 상태 반영).
    try:
        info = json.loads(pidfile.read_text(encoding="utf-8"))
    except Exception:
        info = {}
    display = info.get("display_name") or agent
    pid = info.get("pid", "?")
    log_path = info.get("log") or str(state_dir() / f"{safe_id}.log")

    if already:
        emit_context(
            f"[tuna-autoarm] 이미 무장됨: {display} (poll pid={pid}). 로스터에서 online 상태입니다."
        )
        return 0

    # 받아오기(pull) 수신 명령: 이 세션이 자기 detached poll의 로그를 Monitor로 감시한다.
    # poll이 `TASK <id> :: ..`를 로그에 쓰면 Monitor가 세션을 깨운다. claude엔 codex 같은 외부
    # 주입 소켓이 없어(설계상 codex=넣기/claude=받아오기), 세션이 스스로 당겨야 한다(v2-43 §수신).
    # Monitor는 Git Bash에서 돈다: 경로는 forward-slash로, 작은따옴표로 감싼다(백슬래시·중첩 큰따옴표 회피).
    tuna_bin = cfg("TUNA_BIN", "tunaround")
    log_posix = str(log_path).replace("\\", "/")
    recv_cmd = f"tail -f -n 0 '{log_posix}' | grep --line-buffered '^TASK '"
    c_clean = core.rstrip("/")
    base_core = c_clean[:-4] if c_clean.endswith("/mcp") else c_clean  # watch-results는 base URL.
    emit_context(
        f"[tuna-autoarm] 이 세션이 브로커 로스터에 자동 등록되었습니다(online).\n"
        f"  uuid={agent}(세션 id)  display={display}  core={core}  poll pid={pid}\n"
        f"\n"
        f"[A2A 감독 수신 - claude=받아오기(pull), 설계 v2-43]\n"
        f"이 세션은 A2A 감독(받는 자리)입니다. 아래 Monitor를 실행해 수신 루프에 진입하세요"
        f"(비블로킹 = 세션은 계속 대화·작업 가능, task 도착 시에만 깨어남):\n"
        f"\n"
        f"  ① 지금 대기 중인 task부터 처리: `poll_tasks`(tuna-broker MCP) 호출 → 있으면 각각 claim→답변→complete.\n"
        f"  ② 이후 자동 수신: Monitor(command=\"{recv_cmd}\", description=\"A2A 수신 {display}\", persistent=true)\n"
        f"\n"
        f"poll이 `TASK <id> :: <미리보기>`를 로그에 쓰면 Monitor가 이 세션을 깨웁니다. 그때:\n"
        f"  1) `claim_task`(task_id=<id>)  2) `get_task`로 요청 확인  3) 요청에 직접 답 생성\n"
        f"  4) `complete_task`(task_id=<id>, result=<답변 텍스트>)  (처리 불가 시 `fail_task`)\n"
        f"→ claim_task/get_task/complete_task/poll_tasks 는 tuna-broker MCP 네이티브 도구입니다.\n"
        f"\n"
        f"[총괄(사람 자리)로 쓸 때] 내가 던진 task의 결과만 받으려면:\n"
        f"  Monitor(command=\"{tuna_bin} watch-results --core {base_core} --dispatcher dashboard\", persistent=true)\n"
        f"세션 종료 시 SessionEnd 훅이 poll·등록을 정리합니다."
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
