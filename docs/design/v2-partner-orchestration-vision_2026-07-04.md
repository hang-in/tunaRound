# tunaRound v2: 협력업체(Partner) 오케스트레이션 비전 (방향 노트)

> 2026-07-04. 4인 크로스머신 티키타카 도그푸딩 중 동구님이 방향을 재프레이밍했다. "에이전트 개발팀"보다 **협력업체(on-demand partners)** 모델. 코드에 안 드러나는 제품 방향이라 노트로 고정한다. 배경: [파트너 위임](v2-a2a-partner-delegation_2026-07-02.md) · [워커 데몬](v2-a2a-worker-daemon_2026-07-03.md) · usecase [agent-dev-team](../reference/agent-dev-team.md).

## 0. 한 줄

에이전트를 **필요할 때 능동적으로 부르는 협력업체**로 다룬다. 원격/로컬의 다른 에이전트를 감독(터미널)·워커(헤드리스) 모드로 소환해 서로 협력하고, 감독이 워커를 부리고, 필요하면 tunallama 서브에이전트까지 소환하는 유연한 오케스트레이션이 목표. A2A(에이전트=서비스) 모델과 정확히 맞는다.

## 1. 조직 모델 (책임자 / 실무 계층)

협력업체 비유로 두 티어다.

- **감독 = 각 업체 책임자**(supervised, 터미널, HITL, 보임): 대화형 Claude/Codex 세션. 협의·결정하고, 무거운 실무는 아래로 내려보낸다. task 도착 wake는 하네스별로 갈린다: **claude는 `tunaround poll`+Monitor로 0토큰 wake** 가능. **codex는 백그라운드 완료로 호출 에이전트를 깨우는 수단이 없다**(openai/codex#15723: 블로킹 폴 또는 사람 넛지뿐) - codex 책임자는 상시 대기가 어려우므로 "사람이 알리면 책임자가 실무 소환" 흐름으로 운용.
- **실무담당자 = 워커 + 서브에이전트**(headless, 안 보임): 책임자가 **필요할 때 소환**하는 하위 티어.
  - 워커: `tunaround work`/node auto 레인 = `run_worker_loop`이 claim->러너 실행->complete를 사람 없이. `--runner claude|codex|http|a2a`.
  - 서브에이전트: tunallama(tuna-developer 등) 로컬 LLM 위임.

핵심: **책임자끼리 협의**하고, 각 책임자가 **자기 실무(워커/서브에이전트)를 부린다**. 둘 다 원격/로컬 혼재(크로스머신 A2A, 2머신 실증). 온디맨드 소환 = `send_task(to=<agent>)`. 발송은 대칭이라 책임자도 dispatcher다.

## 2. 상호작용 (핵심)

- **감독이 워커를 소환**: 감독 세션도 `send_task`를 부르면 그 순간 dispatcher다. 즉 누구나(도구만 있으면) 발송자이자 수신자 = 대칭. 감독 에이전트가 필요할 때 워커를 띄워 하위 작업을 분배.
- **tunallama 서브에이전트 소환**: 감독/워커가 tunallama MCP(tuna-developer 등)로 로컬 LLM 서브에이전트에 위임. (도구는 있음, "소환 가능한 파트너"로 매끄럽게 배선하는 것은 정리 대상.)
- **자료수집 -> 정리 -> 토론**: 각 파트너가 먼저 자료를 모으고(레포/코드/외부), 누군가에게 정리를 시켜, 그 결과를 토론 시드/기본자료로 쓴다. 수집=send_task, 산출=artifact, 취합=get_task로 primitive는 이미 있다.

## 3. 이미 있는 primitive vs 새로 필요한 레이어

**있음(오늘 실증 포함):** 온디맨드 소환(send_task) · 감독/워커 두 모드 · 원격/로컬 · 이기종 러너(claude/codex/http/a2a) · 대칭 발송(감독도 dispatcher) · tunallama MCP · 미배달/고착 가시성(governance `tasks`/`⚠no-consumer?`).

**net-new(primitive 위 오케스트레이션):**
1. **자료수집->정리->토론 파이프라인**: 누가 수집·정리하고 그 artifact를 토론에 먹이는 흐름 레이어.
2. **/debate가 A2A 수집물을 시드로**: 현재 `/debate`는 로컬 2좌석 라운드 기반. A2A로 모은 artifact를 토론 기본자료로 주입하는 확장이 별도로 필요.
3. **능력 기반 자동 라우팅**: dispatcher가 Agent Card 보고 최적 파트너 자동 선택(거버넌스 §6 후속).
4. **소환 UX**: 감독 세션이 "이 일 누구한테 시킬까"를 매끄럽게 고르고 던지는 인터페이스(현재는 raw send_task).

## 4. 비범위(지금)

- full-auto 무한 토론 루프(사람 없는) = 의도적 보류(semi-a2a HITL 유지).
- 능력 best-fit 자동선택 = Phase 2 이후.

## 5. 릴리스 접점

v0.2.1에서 릴리스 바이너리에 `worker`/`engines`/`a2a-out` 포함 -> `brew install` 하나로 각 머신이 `tunaround node`(감독/자동 레인)를 띄울 수 있게 됨 = 이 협력업체 오케스트레이션이 "설치 가능"해지는 최소 인프라. 이 위에 §3의 오케스트레이션 레이어를 얹는 것이 다음 방향.
