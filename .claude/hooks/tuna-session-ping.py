# UserPromptSubmit 훅: 사람입력 핑(총감독 ★ 지정, 설계 v2-42). 무장은 presence 스캐너 소관(v2-44).
"""사람이 프롬프트를 넣을 때마다 브로커에 '이 세션이 방금 사람 입력 받음' 핑 → 총감독 = 최신 사람입력 세션.

v2-44에서 무장(ensure_armed) 책임이 제거됐다: 등록은 머신 스캐너가 하고 이 훅은 핑만 보낸다.
opt-in(TUNA_AUTOARM=1) 전제. 실패는 조용히 통과(세션을 절대 막지 않음). 출력 없음(컨텍스트 비오염).
"""
import ipaddress
import json
import os
import socket
import sys
import threading
import time
import urllib.error
import urllib.parse
import urllib.request

try:
    # __file__은 zipapp/임베디드 등에서 미정의(NameError)일 수 있어 sys.path 조작도 try 안에 둔다.
    sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
    import tuna_arm
except Exception:
    sys.exit(0)  # 모듈 없으면 조용히 통과.


def _host_resolves_quickly(host: str, timeout: float = 0.75) -> bool:
    """host가 이미 IP 리터럴이면 즉시 True(DNS 불필요). 아니면 getaddrinfo를 별도 스레드로 시도해
    timeout 안에 끝나면 그 성패를, 못 끝나면 False(=핑 스킵)를 반환한다.

    urlopen(timeout=...)의 timeout은 소켓 connect/read만 덮고 getaddrinfo(DNS/mDNS)는 포함하지
    않는다. 이 훅은 매 프롬프트 동기 실행이라, 코어가 호스트네임(.local 등)이고 대상이 오프라인이면
    리졸버 타임아웃(수 초~수십 초)만큼 매 프롬프트가 지연될 수 있다. 이 헬퍼로 그 지연을 훅의 기존
    타임아웃 예산(0.75초) 안에 가둔다 - 훅은 세션을 절대 막으면 안 되는 규약(조용히 통과)이라, 제
    시간에 안 끝나면 실패로 보고 이번 프롬프트의 핑은 건너뛴다(다음 프롬프트가 재시도).
    """
    try:
        ipaddress.ip_address(host)
        return True
    except (ValueError, TypeError):
        pass
    result = {"ok": False}

    def _resolve():
        try:
            socket.getaddrinfo(host, None)
            result["ok"] = True
        except Exception:
            result["ok"] = False

    t = threading.Thread(target=_resolve, daemon=True)
    t.start()
    t.join(timeout)
    return result["ok"]


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

    # 마커 자가치유: 마커가 없거나 내용이 비었으면 1회 채운다(훅 배포 전에 뜬 세션의 전환 경로).
    # tombstone("dead", v2-46)도 되살린다 - 사용자가 이 세션에서 입력 중 = 살아있음의 확정 증거라,
    # 잘못 남은 dead가 산 세션을 로스터에서 숨기지 않게 한다.
    # sentinel "unknown"(owner 탐색 실패)은 재시도하지 않는다 - 무거운 프로세스 조회가
    # 매 프롬프트 반복되는 것 방지(봇리뷰 critical). 숫자·unknown이면 stat+read만으로 no-op.
    mp = tuna_arm.marker_path(session_id)
    try:
        if mp is not None and (not mp.exists() or mp.read_text(encoding="utf-8").strip() in ("", "dead")):
            tuna_arm.write_marker(session_id)
    except Exception:
        pass

    # 수신 자동 가동(세션당 1회): SessionStart 지시문을 못 받은 기존 세션도 다음 프롬프트에서
    # 지시를 주입해 스스로 수신 루프를 걸게 한다. 사용자가 A2A를 수동으로 챙기지 않는 것이
    # 제품의 존재 이유(no-shuttle). .rx 마커 = 주입 1회 보장(O_EXCL, 다중 발화 안전).
    cwd = payload.get("cwd") or ""
    # 비대화형(스크립트/워크플로 스폰) 세션은 사람 입력이 아니므로 ★(총감독) 판정과 수신 루프 주입을
    # 오염시키면 안 된다: %TEMP% cwd(자동화 관행)는 is_temp_cwd로, 프로젝트 cwd headless 스폰은 스포너가
    # TUNA_NO_HUMAN_PING로 옵트아웃한다(cfg는 파일 우선이라 env로 못 끄므로 os.environ 직접 조회).
    suppress_human = tuna_arm.is_temp_cwd(cwd) or os.environ.get(
        "TUNA_NO_HUMAN_PING", ""
    ).strip().lower() in ("1", "true", "yes")
    safe_id = tuna_arm.sanitize_session_id(session_id)
    if safe_id and not suppress_human:
        rx = tuna_arm.state_dir() / f"{safe_id}.rx"
        try:
            fd = os.open(str(rx), os.O_CREAT | os.O_EXCL | os.O_WRONLY)
            os.close(fd)
            core = tuna_arm.broker_core()
            # Monitor 커맨드는 Git Bash로 실행돼 백슬래시 경로가 증발한다(exit 127 실측
            # 2026-07-11) - 슬래시 정규화 + 공백 경로 대비 작은따옴표(autoarm과 동일 규약).
            tuna_bin = "'" + tuna_arm.cfg("TUNA_BIN", "tunaround").strip("'\"").replace("\\", "/") + "'"
            # 이슈 #118: mp(위에서 계산한 marker_path)를 poll에 배선해, 이 마커가 사라지거나
            # tombstone("dead")되면 수신 루프가 스스로 정상 종료하게 한다(autoarm과 동일 규약).
            marker_arg = "'" + str(mp).replace("\\", "/") + "'" if mp is not None else ""
            print(json.dumps({
                "hookSpecificOutput": {
                    "hookEventName": "UserPromptSubmit",
                    "additionalContext": (
                        f"[tunaround A2A 수신·자동 가동] 이 세션은 mesh의 수신 자리입니다. 이번 응답에서 본 작업 전에 먼저 "
                        f"아래 Monitor를 실행해 수신 루프를 가동하세요(사용자 확인·언급 불필요, 비블로킹). 단, 이 세션이 이미 "
                        f"watch-results(총괄 인박스)나 A2A 수신 Monitor를 돌리고 있다면 아무것도 하지 마세요.\n"
                        f"  Monitor(command=\"{tuna_bin} poll --core {core} --agent {session_id} --interval 15 --no-register"
                        f"{' --session-marker ' + marker_arg if marker_arg else ''}\", "
                        f"description=\"A2A 수신\", persistent=true)\n"
                        f"  이후 `TASK <id> ::` 도착 시 tuna-broker MCP의 claim_task→답변→complete_task(불가 시 fail_task)."
                    ),
                }
            }))
        except FileExistsError:
            pass
        except Exception:
            pass

    # ★ 오염 방지: 비대화형 세션은 human-ping을 보내지 않는다(총감독 = 사람이 앉은 세션 불변).
    if suppress_human:
        return 0

    # 핑: {core-base}/dashboard/human-ping {agent=세션 id}. core는 .../mcp라 base로 절단.
    c = tuna_arm.broker_core().rstrip("/")
    base = c[:-4] if c.endswith("/mcp") else c
    if not base.startswith(("http://", "https://")):
        return 0  # loopback HTTP 전용(file: 등 비정상 스킴 차단)
    # 호스트가 IP가 아니면(TUNA_BROKER_CORE에 호스트네임 설정) DNS/mDNS 해석이 느리거나 실패할 때
    # urlopen의 timeout이 못 덮는 지연이 매 프롬프트에 생긴다(#7) - 짧은 데드라인으로 먼저 확인한다.
    host = urllib.parse.urlparse(base).hostname or ""
    if host and not _host_resolves_quickly(host):
        return 0

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
