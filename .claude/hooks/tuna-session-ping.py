# UserPromptSubmit 훅: 이 세션을 무장 보장(resume 포함) + 사람입력 핑(총감독 지정, 설계 v2-42).
"""사람이 프롬프트를 넣을 때마다: (a) 이 세션이 미무장이면 무장(SessionStart를 못 잡는 resume 세션 보강),
(b) 브로커에 '이 세션이 방금 사람 입력 받음' 핑 → 총감독 = 최신 사람입력 세션.

opt-in(TUNA_AUTOARM=1)·토큰 전제. 실패는 조용히 통과(세션을 절대 막지 않음). 출력 없음(컨텍스트 비오염).
"""
import json
import os
import sys
import time
import urllib.error
import urllib.request

try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import tuna_arm
except Exception:
    sys.exit(0)  # 모듈 없으면 조용히 통과.


def main() -> int:
    if tuna_arm.cfg("TUNA_AUTOARM") != "1":  # 설정파일 우선(env 신선도 무관, 설계 v2-43 §5-1).
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

    # 하네스는 백그라운드 이벤트(Monitor wake·Agent 완료 등)로 세션을 깨울 때도 UserPromptSubmit을
    # 발화한다. 그건 사람 입력이 아니므로 human-ping(총감독 판정)을 보내지 않는다 - A2A 수신만 한
    # 세션이 ★를 가져가는 오염 실측(2026-07-11, mac-claude-tunaRound). 무장 보장은 위에서 이미 했다.
    prompt = str(payload.get("prompt") or "").lstrip()
    if prompt.startswith(("<task-notification>", "[SYSTEM NOTIFICATION")) or (
        "This is an automated background-task event" in prompt[:600]
    ):
        return 0

    # 핑: {core-base}/dashboard/human-ping {agent}. core는 .../mcp라 base로 절단.
    # 후행 슬래시를 먼저 제거해 "http://x/mcp/"처럼 끝나도 /mcp가 정확히 잘리게 한다.
    c = core.rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    if not base.startswith(("http://", "https://")):
        return 0  # loopback HTTP 전용(file: 등 비정상 스킴 차단)
    url = base + "/dashboard/human-ping"
    token = tuna_arm.cfg("TUNA_BROKER_TOKEN", "")
    body = json.dumps({"agent": agent}).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    # 방금 무장한 세션은 poll의 브로커 등록이 아직이라 404가 난다. 짧게 1회 재시도해
    # "첫 프롬프트 핑 유실 → 총감독 ★가 즉시 안 따라옴"을 줄인다(2026-07-10 실측).
    # 이 훅은 매 프롬프트를 동기 블로킹하므로 총 지연을 ~2초 아래로 유지한다.
    for attempt in range(2):
        try:
            urllib.request.urlopen(req, timeout=0.75).read()
            break
        except urllib.error.HTTPError as e:
            if e.code != 404 or attempt == 1:
                break
            time.sleep(0.4)  # poll register 반영 대기 후 재시도.
        except Exception:
            break  # 브로커 다운·네트워크 드롭은 재시도 없이 통과(체감 지연 방지).
    return 0


if __name__ == "__main__":
    sys.exit(main())
