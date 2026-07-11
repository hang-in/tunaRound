# 핸드오프: 세션16 (2026-07-07) - 리팩토링 대청소 + 봇리뷰 사이클 + mac 협업 + 거버넌스 개정

> WIN 핸드오프. 라이브 접속값은 gitignored `docs/reference/backend-private.md` 하단(세션16 블록). 토큰=User env `TUNA_BROKER_TOKEN`(setx), argv/파일 평문 없음. 레포 PUBLIC=평문 금지. main=`fca18fb`.

## 이 세션(16)의 큰 줄기

재부팅 후 클린 재기동 → 위임 규약 문서화 → armed-overlay 버그 수정 → **mac 2자에 리팩토링 검토 위임** → **검토 결과 기반 리팩토링 대청소**(god파일 분할 포함) → **봇리뷰(CodeRabbit/gemini) 전수 확인·반영** → mac 거버넌스 제안 채택.

### A. 인프라/디버깅 (세션 전반)
- **재부팅 클린 재기동**: 재부팅으로 detached 메시 전부 소멸 → cargo build(overlay 포함) → broker 8770 → watcher/discover/watch-results → app-server **8790 복귀**(재부팅으로 옛 소켓 풀림). 라이브값=backend-private 세션16 블록.
- **codex 스테일 토큰 401**: 로테이션 전 기동한 codex app-server(win 34176·mac 옛것)가 옛 토큰 env → tuna-broker MCP 401. win은 8790 소켓 고아→8791 우회 후 재부팅으로 8790 복귀, mac은 자가 재기동. **node_repl config 경로버그**(`/mnt/c` WSL 경로 → Windows 경로) 수정.
- **§7 codex 라이브 관전 미해결 실증**: `codex --remote ws://`는 글루 thread(codex-inject 대상)에 안 붙고 **새 thread를 만든다**(실측: loaded threads 2개). `.thread`를 사용자 thread에 맞춰도 TUI 조용 = codex app-server가 injector 턴을 다른 붙은 클라에 브로드캐스트 안 함(설계 §7 열린질문2). **맥에선 되는데 왜?** = 미규명(managed remote-control이 Unix전용이라 맥은 정상경로, 윈은 raw ws 우회라 추정 - 확인 필요). **codex 감독 단순화(work 데몬 통합) 결정 대기.**

### B. 리팩토링 대청소 (검토→위임→반영, 11 PR 머지)
mac-claude-sup 검토(task c1a93ce, 우수) + mac-codex-sup(도구승인 한계로 빈 답=감독 fragile 재확인) 기반, 삼분류로 처리:

| PR | 내용 | 비고 |
|---|---|---|
| #21 | 한글 task 파싱 char 경계 패닉(mac 작성) | **오늘 로스터 불안정 근원** |
| #22 | armed-overlay 이중표시(고정이름 감독) | session 태그 매칭 |
| #23 | 위임 행동 규약 문서(a2a-usage §12·13) | |
| #25 | config → config/node.rs SRP 분리 | |
| #26 | Utterance → crate::types (store→orchestrator 역결합 제거) | |
| #27 | **sqlite god파일 분할** 2786→ core403+messages1044+registry234+tasks1139 | 서브에이전트 이동+Opus검증 |
| #28 | **mcp god파일** HTTP/대시보드 → mcp/server.rs | |
| #29 | **SSRF 보안**: ws_target_is_loopback IpAddr파싱(userinfo·127-prefix·대소문자·FQDN끝점 우회 차단) | 봇이 발견 |
| #30 | mcp format/params 추가 분할 → core~1104 | |
| #31 | **거버넌스**: 관리자=진단·리뷰·스펙 중심(mac 제안 채택) | |

- **폐기 판정 T1-2(토큰 401 self-heal)**: 구현·PR(#24)했으나 **CodeRabbit이 근본결함 규명** = 실행 중 프로세스 env는 launch시 고정 → 외부 로테이션을 데몬의 std::env::var가 못 봄 → env 재로드 self-heal 무효. #24 닫음. **근본처리=live 토큰소스(Windows레지스트리/파일/브로커 grace) 설계 필요.**
- **불필요 판정 T2-4(바이트슬라이스)**: #21 이후 모든 슬라이스가 ASCII 마커 경계라 잔여위험 0.

### C. 봇리뷰가 실제로 값을 냄 (매 PR 전수 확인 규율 - 사용자 지시)
반영: SSRF 우회 4종·self-heal 동시성레이스·ENV_LOCK UB·em-dash·펜스언어·dispatcher설명·is_supervised doc·문장 서술어. **불채택+근거**: perf 미세최적·`//!`·Eq/Hash(YAGNI)·트랜잭션 unchecked_transaction(pure-move 범위밖)·`use super::*`(서브모듈 일관성).

### D. mac 협업
- 리팩토링 검토 위임 → mac-claude-sup 우수 검토(삼분류+codex감독 단순화 제안).
- **mac이 총괄 인박스에 보낸 것 뒤늦게 확인**(총괄이 boss uuid 4a46a380이 아니라 display name으로 폴링해 놓침 = 인박스 위생 교훈): PR#21 머지요청(완료) + 거버넌스 제안(채택). ack 전송(task 3729d148) + 스테일 인박스 3건 취소.

## 라이브 상태 (backend-private 세션16 블록 참조, 재부팅 시 소멸)
- broker `serve` PID **16128** (8770, %LOCALAPPDATA%\tunaround\broker.db) / codex app-server PID **34220** (ws 8790, 새 토큰) / discover **13884** / watch-results **15876** / win-codex-sup watcher **11088**(핸들러 target/codex-sup-handle.cmd, --ws 8790).
- ⚠ **라이브 메시는 머지 전 바이너리**로 돌 수 있음(리팩토링 PR들 머지 후 rebuild+relaunch 안 함). 최신 반영하려면 cargo build 후 재기동(exe 잠금=먼저 프로세스 종료).
- 로스터: mac-claude-sup·mac-codex-sup·win-codex-sup online.

## 다음 세션 후보 (급하지 않음)
1. **main.rs 분할** — 세 god파일 중 미분할. 바이너리 크레이트+main() 인라인 디스패치라 순수이동 어렵고 자연스러운 CLI 형태. "must-do 아니면 defer" 기준 defer 판단. 원하면 clap 인자 정의(enum Commands+Args ~435줄)만 src/cli.rs로 안전 추출(검증=CARGO_TARGET_DIR 우회, 잠긴 exe 회피).
2. **codex 감독 단순화 결정** — work --runner codex 통합(4부품→1) vs 라이브 관전 유지. 관전은 codex 한계라 대시보드 SSE로 빼는 게 실용적. **맥 관전이 왜 되는지 먼저 규명**(managed vs raw).
3. **토큰 로테이션 live-source** — env self-heal 무효 판명 후속.
4. **후속 리뷰 항목(낮음)**: sqlite messages.rs 수동 BEGIN/COMMIT → unchecked_transaction(패닉안전), 비-loopback 바인드+무토큰 가드(defense-in-depth). dashboard write auth는 의도된 loopback-trust라 비대상.
5. **라이브 메시 rebuild+relaunch** — 머지된 #21(패닉수정) 등 반영.

## 진입점
- 리팩토링 검토 정본: mac-claude-sup task c1a93ce 결과(요지 위 표+삼분류) · 지침 [development-guidelines.md](../reference/development-guidelines.md).
- 위임 규약 [a2a-usage §12·13](../reference/a2a-usage.md). 거버넌스 [CLAUDE.md 총괄/관리자/실무자 위계](../../CLAUDE.md).
- 이전 [세션15 v2-40-release-governance](v2-handoff_2026-07-06_v2-40-release-governance.md).
