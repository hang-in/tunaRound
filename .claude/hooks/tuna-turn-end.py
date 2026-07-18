# Stop 훅: 대화 턴 종료 신호(이슈 #123). 브로커 turn-ping(end)로 로스터 스피너를 즉시 끈다.
"""응답 생성이 끝날 때마다 turn-ping(end)를 보낸다. 크래시로 미발화해도 서버 신선도 창(10분)이
스피너를 자연 소등한다. opt-in(TUNA_AUTOARM=1) 전제, 실패는 조용히 통과(세션을 절대 막지 않음)."""
import json
import os
import sys

try:
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import tuna_arm
except Exception:
    sys.exit(0)


def main() -> int:
    if tuna_arm.cfg("TUNA_AUTOARM") != "1":
        return 0
    try:
        payload = json.load(sys.stdin) if not sys.stdin.isatty() else {}
        if not isinstance(payload, dict):
            payload = {}  # 리스트/문자열 JSON이 오면 .get에서 AttributeError(gemini 리뷰).
    except Exception:
        payload = {}
    session_id = str(payload.get("session_id") or "").strip()
    if not session_id:
        return 0
    # 미등록 세션(headless 등)은 서버가 no-op 200 - 여기서 따로 거르지 않는다(단순 유지).
    tuna_arm.post_dashboard("/dashboard/turn-ping", {"agent": session_id, "phase": "end"})
    return 0


if __name__ == "__main__":
    sys.exit(main())
