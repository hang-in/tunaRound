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
    from tuna_arm import (
        broker_core,
        cfg,
        derive_seat_address,
        is_temp_cwd,
        sanitize_session_id,
        state_dir,
        write_marker,
    )
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
    # temp cwd(자동화 headless 세션)는 안내를 항상 생략한다(컨텍스트 비오염). 과거엔 TUNA_AUTOARM_PROJECT가
    # 설정돼 있으면 이 억제를 우회했는데, 그 키는 원래 "표시 이름·역할 재정의" 용도로 문서화됐던 것이라
    # 실제 동작(temp-cwd 억제 우회)과 의미가 어긋났다(#5). 우회 자체를 제거한다 - %TEMP% 자동화 세션이
    # 로스터에 노이즈로 뜨는 것을 막는 원래 의도가 더 중요하고, 특별 사유로 안내가 필요한 case는 없었다.
    if is_temp_cwd(cwd):
        return 0

    safe_id = sanitize_session_id(session_id)
    if not safe_id:
        return 0
    # 마커 내용 = owner claude PID(스캐너의 per-session 생존 판정 = 유령 즉시 제거, v2-44 §10).
    marker = state_dir() / f"{safe_id}.ctx"
    # 안내 1회 보장(W1)은 O_EXCL 원자 생성으로 판정한다(다중 발화 경합에도 안내는 한 번).
    try:
        fd = os.open(str(marker), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
        os.close(fd)
        first_start = True
    except FileExistsError:
        first_start = False
    except Exception:
        first_start = True  # 판정 불가면 안내를 내는 쪽(안내 유실 < 중복).
    # PID는 항상 갱신: resume은 같은 session_id에 새 claude 프로세스라, 옛 pid를 두면
    # 스캐너가 산 세션을 유령 판정한다(봇리뷰 Major).
    write_marker(session_id)
    if not first_start:
        return 0

    core = broker_core()
    c = core.rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    # Monitor 커맨드는 Git Bash로 실행돼 백슬래시 경로가 증발한다(exit 127 실측 2026-07-11)
    # - 슬래시 정규화 + 공백 경로 대비 작은따옴표.
    tuna_bin = "'" + cfg("TUNA_BIN", "tunaround").strip("'\"").replace("\\", "/") + "'"
    # 이슈 #118: 마커 경로를 poll/watch-results에 배선해, /clear·창닫기로 이 마커가 사라지거나
    # tombstone("dead")되면 그 수신 루프가 스스로 정상 종료하게 한다(유령 poll 잔존 방지).
    # Monitor 커맨드와 동일 규약(Git Bash 백슬래시 증발) - 슬래시 정규화 + 작은따옴표.
    marker_arg = "'" + str(marker).replace("\\", "/") + "'"
    # 이슈 #147 Stage 1: 좌석(mbox) 수신함 이중폴. 파생 불가(cwd 미상 등)면 None -> 플래그 자체를
    # 생략해 기존 동작 그대로(안전 폴백, "mesh 토론 합의" 계약).
    seat_addr = derive_seat_address(cwd)
    also_agent_flag = f" --also-agent '{seat_addr}'" if seat_addr else ""
    # SessionStart에서 수신 지시를 냈으므로 ping의 1회 주입(.rx)은 불필요 - 중복 주입 방지.
    try:
        (state_dir() / f"{safe_id}.rx").touch()
    except Exception:
        pass
    emit_context(
        f"[tunaround] 이 세션의 로스터 presence는 머신 스캐너가 자동 관리합니다(별도 등록 불요).\n"
        f"[A2A 수신·자동 가동] 이 세션은 mesh의 수신 자리입니다. **다음 응답에서 본 작업 전에 먼저** 아래 Monitor를 실행해 수신 루프를 가동하세요(사용자 확인·언급 불필요, 비블로킹 = 대화·작업에 영향 없음. 사용자가 A2A를 수동으로 챙기게 하지 않는 것이 이 앱의 존재 이유입니다):\n"
        f"  Monitor(command=\"{tuna_bin} poll --core {core} --agent {session_id} --interval 15 --no-register --session-marker {marker_arg}{also_agent_flag}\", description=\"A2A 수신\", persistent=true)\n"
        f"  이후 `TASK <id> ::` 도착 시: 알림 줄은 프리뷰라 본문이 잘릴 수 있습니다. **claim 전 get_task(task_id)로 요청 전문을 확인**한 뒤(#136 - 잘린 지시로 일하지 않기), tuna-broker MCP의 claim_task→답변→complete_task(불가 시 fail_task). MCP 미로드면 CLI: `{tuna_bin} task poll|claim|get|complete|fail --core {core}`.\n"
        f"[총괄로 쓸 때] 내가 던진 task의 결과만 받는 자리면 위 대신: Monitor(command=\"{tuna_bin} watch-results --core {base} --dispatcher dashboard --digest 60 --session-marker {marker_arg}\", persistent=true)"
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
