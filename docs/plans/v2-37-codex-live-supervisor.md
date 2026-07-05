# Plan v2-37: codex 라이브 감독 (app-server ws + turn/start 주입)

> 설계 정본 = [v2-codex-live-supervisor-appserver_2026-07-05](../design/v2-codex-live-supervisor-appserver_2026-07-05.md). 구현 Sonnet 위임, Opus 스펙·리뷰·검증. GitHub Flow + 3-OS CI + CodeRabbit. 커밋 분리.

## 목표

codex 감독을 헤드리스 exec에서 **라이브 app-server thread**로 전환. 신규 `tunaround codex-inject`(ws 클라이언트)가 브로커 task 도착 시 `turn/start`로 라이브 thread를 외부 wake. 사람은 `codex --remote`로 관전(HITL).

## 사전(구현 착수 전 라이브 확인 = 설계 §7)

- [x] P0: **완료(2026-07-05, stdio 실측)**. `codex app-server --listen stdio://` 파이프 구동으로 initialize->thread/start->turn/start->turn/completed 왕복 성립. 확정: thread id=`result.thread.id`, turn/start input=`[{type:"text",text}]`, 완료=`turn/completed` 알림, 최종답=`item/completed`(agentMessage, phase=final_answer). **승인=MCP 호출이 approvalPolicy=never여도 `mcpServer/elicitation/request`로 오고 injector가 `{result:{action:"accept"}}` 필수**. accept 후 tuna-broker `list_agents` native 호출 정답 반환, raw HTTP 폴백 0(토큰 env 전제). enum: approvalPolicy=untrusted/on-failure/on-request/never, sandbox=read-only/workspace-write/danger-full-access. 설계 §5.2·§7 반영. ws 고유(--remote 관전/브로드캐스트/재개)는 T2·T5 라이브.

## Tasks

- [x] T1: **JSON-RPC/프로토콜 순수부** - app-server 메시지 타입(serde): initialize/thread.start/thread.resume/turn.start 요청 + 핵심 알림(turn/started, item/agentMessage/delta, turn/completed) + 승인 ServerRequest 최소셋. 프레이밍/파싱 순수함수 + 단위테스트(프레임 픽스처). codex 미설치 무관하게 CI green.
- [x] T2: **ws 클라이언트 + `codex-inject` 서브커맨드**(worker 피처) - `tokio-tungstenite`로 접속 -> initialize -> thread 확보(start|resume, threadId 파일 영속 `~/.tunaround/codex-sup-<agent>.thread`) -> turn/start -> `turn/completed`까지 알림 수신 -> agentMessage stdout -> 종료. 인자: `--ws --agent --text --token-env --approval --timeout --new`. 타임아웃/에러 시 비-0 종료.
- [x] T3: **승인 처리** - 무인 티키타카가 승인 대기로 멈추지 않게 §5.2 정책 구현(자동 승인 대상 = codex 도구/permissions 최소셋, 그 외는 로그+거부 또는 붙은 TUI 위임). P0에서 확정한 라우팅 반영.
- [x] T4: **watcher 배선 + 기동 헬퍼** - `poll --on-task`의 감독 레인 안내를 `codex-inject`로 갱신(main.rs 감독 레인 문구). `codex app-server`를 `TUNA_BROKER_TOKEN` env로 상주시키는 헬퍼/문서(토큰 전파 필수 §5.3). node.toml 감독 레인 반영.
- [x] T5: **문서 + 라이브 스모크** - a2a-usage에 codex 감독(app-server) 레시피. dev-mac-windows에 SSH 관전 노트. 라이브 스모크(설계 §9 2~4): app-server 기동 -> codex-inject 왕복 -> **raw HTTP 폴백 0** broker.db 교차검증 -> 총감독 send_task 2~3회 티키타카 맥락유지 -> `--remote` HITL 가시.

## 상태: 완료 (2026-07-05, PR #9)

P0~T5 전부 완료. 46 신규 순수 테스트, 전체 lib 454 pass, CI조합 clippy 클린. 라이브 스모크 A(list_agents 왕복)·B(task claim/complete→broker.db completed)로 감독 라이브 thread 외부 wake + 맥락연속 실증. 코드리뷰(3앵글) + gemini/CodeRabbit findings 반영(에러 fail-fast, thread 필터, 자가치유 폴백, path traversal, connect 타임아웃). checklist.md와 동기화.

## 완료 기준

- 순수부(T1) 전 OS CI green. 라이브 스모크(T5)에서 codex가 native tuna-broker MCP로 claim/complete(세션12 raw HTTP 186k 재현 없음). 티키타카 맥락 누적 확인. 감독 라이브 TUI 외부 wake 실증.

## 규율

- 신규 소스 첫 줄 = 역할 한국어 주석(#6). cargo는 Bash 툴. 검증(build/test)과 commit/push 분리. 굵직한 결정 재론 금지(설계 정본 따름).
