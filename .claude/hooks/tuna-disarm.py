#!/usr/bin/env python3
# SessionEnd 훅: 브로커에 즉시 deregister 핑(+구식 detached poll 잔재가 있으면 정리).
"""tunaRound v2-44 세션 종료 훅.

presence는 스캐너가 다음 주기(±15초)에 exit를 반영하지만, 깨끗한 종료는 이 핑으로 즉시
로스터에서 내린다(설계 v2-44 §7 부수 결정: poll-kill·pidfile·리핑은 제거, deregister 1줄만 잔존).

전환기 보호: v2-43 이전 autoarm이 남긴 detached poll pidfile이 있으면 그 poll을 죽이고
정리한다(안 죽이면 heartbeat가 영원히 유령을 유지한다). 새 세션엔 pidfile이 없어 no-op.
"""
import json
import os
import sys

try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import tuna_arm
except Exception:
    sys.exit(0)


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

    # 전환기: 구식 detached poll이 남아 있으면 kill+deregister+pidfile 정리(내부에서 처리).
    # NOT_FOUND(=pidfile 없음, v2-44 정상 경로)면 deregister 핑만 직접 보낸다.
    if tuna_arm.disarm_session(session_id) == "NOT_FOUND":
        tuna_arm.deregister(session_id, tuna_arm.broker_core(), tuna_arm.cfg("TUNA_BROKER_TOKEN"))

    # 안내 1회 마커(autoarm)가 남아 있으면 정리(세션 종료 = 다음 같은 id resume 시 재안내 허용).
    safe_id = tuna_arm.sanitize_session_id(session_id)
    if safe_id:
        try:
            (tuna_arm.state_dir() / f"{safe_id}.ctx").unlink()
        except Exception:
            pass
    return 0


if __name__ == "__main__":
    sys.exit(main())
