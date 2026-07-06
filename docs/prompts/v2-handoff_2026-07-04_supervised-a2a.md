# 핸드오프: 감독(supervised) A2A 자율 수신 테스트 (2026-07-04/05)

> WIN 핸드오프(감독 A2A 테스트 전용). **라이브 접속값(브로커 IP·토큰·PID·task id·config 경로)은 gitignored `docs/reference/backend-private.md` 하단 "세션12 감독 A2A 라이브 상태"를 먼저 읽어라.** 레포 PUBLIC이라 여기엔 평문 금지.

## ⛔ 먼저: 지난 세션 실수(반복 금지)

Opus가 감독 A2A 테스트 중 **curl↔MCP를 오가며 "되니 안되니" flip-flop**해서 사용자를 지치게 했다. 바로잡는다:

- **MCP config = 고도화(upgrade)다. curl로 후퇴하지 마라.** curl은 부트스트랩이었고(세션7-8 win-claude가 raw curl로 poll/claim/complete 실증), 이번에 레지스트리+태그+네이티브 MCP 도구로 올렸다. 테스트 단계 = 올린 경로를 작동시키는 것이지 curl로 되돌리는 게 아니다.
- **매 task 사람이 poll/claim/complete 수동 = 목적 위반.** 이걸 없애려고 만든 것이다. 테스트에 한두 번 사람이 복붙하는 건 되지만 그게 목표가 아니다.
- **원래 목적(메모리 [[core-purpose-no-shuttle]])**: 사용자가 Win↔Mac 왔다갔다 안 하고, 붙어 있는 TUI(총감독)에서 던지면 다른 머신 에이전트가 **자동 수신·응답**. 이미 세션7-8 크로스머신·이번 mac-codex 자율 왕복으로 됨. **후퇴시키지 마라.**
- **codex config에 `[mcp_servers.X.env]` 넣지 마라** → "env is not supported for streamable_http"로 config 로딩 전체가 깨진다(검증됨). 인라인 `bearer_token`도 불가. 토큰은 codex 프로세스 env로만.
- **`codex exec`를 네 셸에서 스폰하지 마라** — 그건 사용자의 감독 세션이 아니라 별개 프로세스다. 감독에게 "물어보기" = 브로커로 send_task 라우팅.

## 현재 라이브 상태 (값은 backend-private.md)

- 브로커(serve) Windows 상주(백그라운드), 안정 db, 토큰 고정. 맥 워커 `mac-claude`/`mac-codex` 등록·자율(mac-codex 크로스머신 왕복 실증).
- 윈도우 codex에 tuna-broker MCP 설치 완료(고도화, 유지). 토큰만 프로세스 env로 주면 브로커 도구 전부 로드(검증됨).
- 미소비 테스트 task 1건이 win-codex-sup 앞에 submitted(감독 미등록·미감시라 대기).
- main 머지: 레지스트리 #5·doctor #6·트레이스 #7·node태그 #8(전부 origin/main, 스키마 v8).

## 목표: 감독 자율 수신 (1회 세팅 → 이후 던지면 끝)

각 감독(TUI 세션)이 **자기 inbox를 감시하는 루프를 1회 켜두면**, 총감독이 던질 때마다 스스로 깨어나 **자기 맥락 안에서** claim→답변→complete. 사람은 총감독만 상대. **매 task 수동 아님.**

- **claude 감독(깔끔)**: 백그라운드 `tunaround poll --agent <id> --core <url> --token <tok>`를 세션 하네스 **Monitor가 감지→세션 wake**(이 세션이 쓰는 그 메커니즘) + 브로커 MCP 도구 allowlist(승인 없이 자동). 맥락 유지.
- **codex 감독(한 겹 더)**: 브로커 MCP는 이미 로드(config 완료). 관건 = 도구 승인 자동화(`-a` 정책 또는 bypass) + 감시(poll --on-task 또는 세션 self-watch). 맥락은 `resume --last`(감독엔 resume가 맥락 수단, 워커 resume 유예와 별개).

## 다음 세션 순서(사용자 확정)

1. **① 윈도우 codex 감독** 자율 수신 세팅·테스트(사용자가 codex를 `TUNA_BROKER_TOKEN=<tok> codex`로 새로 띄움 → register_agent(win-codex-sup, role=supervised) → 감시 루프 1회 → 총감독이 던지면 자동 처리).
2. **② 되면 LAN 맥 claude/codex 감독**도 같은 방식(맥은 core=[사설IP], 실값은 gitignored backend-private.md).
3. **③ 그 다음 워커**(감독 테스트 통과 후).

## 첫 행동

1. `docs/reference/backend-private.md` "세션12 감독 A2A 라이브 상태" 읽어 브로커/토큰/URL/task id 확보. 브로커 살아있나 확인(agent-card 프로브). 죽었으면 **같은 토큰**으로 재기동(로테이트 금지).
2. 감독 자율 수신 = **1회 세팅 후 자동**이 되게 설계. curl 후퇴·매task 수동 금지. claude 감독으로 루프 먼저 증명 후 codex/맥 복제(권장), 사용자가 codex부터 원하면 bypass+watch로.
3. 규율: 구현 Sonnet+Opus 리뷰, GitHub Flow+3-OS CI+CodeRabbit, cargo는 Bash 툴. 레포 PUBLIC=평문 금지.
