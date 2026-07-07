# UserPromptSubmit 훅: 이 세션을 무장 보장(resume 포함) + 사람입력 핑(총감독 지정, 설계 v2-42).
"""사람이 프롬프트를 넣을 때마다: (a) 이 세션이 미무장이면 무장(SessionStart를 못 잡는 resume 세션 보강),
(b) 브로커에 '이 세션이 방금 사람 입력 받음' 핑 → 총감독 = 최신 사람입력 세션.

opt-in(TUNA_AUTOARM=1)·토큰 전제. 실패는 조용히 통과(세션을 절대 막지 않음). 출력 없음(컨텍스트 비오염).
"""
import json
import os
import sys
import urllib.request

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
try:
    import tuna_arm
except Exception:
    sys.exit(0)  # 모듈 없으면 조용히 통과.


def main() -> int:
    if os.environ.get("TUNA_AUTOARM") != "1":
        return 0
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
    except Exception:
        payload = {}
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        return 0
    cwd = payload.get("cwd") or os.getcwd()

    armed = tuna_arm.ensure_armed(session_id, cwd)
    if not armed:
        return 0
    agent, core = armed

    # 핑: {core-base}/dashboard/human-ping {agent}. core는 .../mcp라 base로 절단.
    base = core[:-4] if core.endswith("/mcp") else core.rstrip("/")
    url = base.rstrip("/") + "/dashboard/human-ping"
    token = os.environ.get("TUNA_BROKER_TOKEN", "")
    body = json.dumps({"agent": agent}).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    try:
        # 방금 무장한 세션은 등록이 아직 안 됐을 수 있어(404) 다음 프롬프트에 반영된다. 조용히 통과.
        urllib.request.urlopen(req, timeout=3).read()
    except Exception:
        pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
