# UserPromptSubmit 훅: 사람입력 핑(총감독 ★ 지정, 설계 v2-42). 무장은 presence 스캐너 소관(v2-44).
"""사람이 프롬프트를 넣을 때마다 브로커에 '이 세션이 방금 사람 입력 받음' 핑 → 총감독 = 최신 사람입력 세션.

v2-44에서 무장(ensure_armed) 책임이 제거됐다: 등록은 머신 스캐너가 하고 이 훅은 핑만 보낸다.
opt-in(TUNA_AUTOARM=1) 전제. 실패는 조용히 통과(세션을 절대 막지 않음). 출력 없음(컨텍스트 비오염).
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

    # 하네스는 백그라운드 이벤트(Monitor wake·Agent 완료 등)로 세션을 깨울 때도 UserPromptSubmit을
    # 발화한다. 그건 사람 입력이 아니므로 human-ping(총감독 판정)을 보내지 않는다 - A2A 수신만 한
    # 세션이 ★를 가져가는 오염 실측(2026-07-11, mac-claude-tunaRound). 판별 = 하네스 고정 래퍼의
    # 시작 prefix만(실측 2종). 본문 부분 문자열 매칭은 그 문구를 인용하는 진짜 프롬프트까지 제외하므로 안 쓴다.
    prompt = str(payload.get("prompt") or "").lstrip()
    if prompt.startswith(("<task-notification>", "[SYSTEM NOTIFICATION")):
        return 0

    # 마커 자가치유: 마커가 없거나 PID 미기록이면 채운다(훅 배포 전에 뜬 세션의 전환 경로).
    # 있으면 no-op(stat만) - 조상 체인 조회(무거움)를 매 프롬프트 반복하지 않는다.
    mp = tuna_arm.marker_path(session_id)
    try:
        if mp is not None and (not mp.exists() or not mp.read_text(encoding="utf-8").strip().isdigit()):
            tuna_arm.write_marker(session_id)
    except Exception:
        pass

    # 핑: {core-base}/dashboard/human-ping {agent=세션 id}. core는 .../mcp라 base로 절단.
    c = tuna_arm.broker_core().rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    if not base.startswith(("http://", "https://")):
        return 0  # loopback HTTP 전용(file: 등 비정상 스킴 차단)
    url = base + "/dashboard/human-ping"
    token = tuna_arm.cfg("TUNA_BROKER_TOKEN", "")
    body = json.dumps({"agent": session_id}).encode()
    req = urllib.request.Request(url, data=body, method="POST")
    req.add_header("Content-Type", "application/json")
    if token:
        req.add_header("Authorization", "Bearer " + token)
    # 갓 뜬 세션은 스캐너 등록(≤15초)이 아직일 수 있어 404가 난다. 짧게 1회 재시도하고,
    # 그래도 안 되면 다음 프롬프트 핑에 맡긴다(훅은 매 프롬프트 동기 블로킹 = 총 지연 ~2초 이하 유지).
    for attempt in range(2):
        try:
            urllib.request.urlopen(req, timeout=0.75).read()
            break
        except urllib.error.HTTPError as e:
            if e.code != 404 or attempt == 1:
                break
            time.sleep(0.4)  # 스캐너 등록 반영 대기 후 재시도.
        except Exception:
            break  # 브로커 다운·네트워크 드롭은 재시도 없이 통과(체감 지연 방지).
    return 0


if __name__ == "__main__":
    sys.exit(main())
