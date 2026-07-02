# A2A 스트리밍 (SSE) 설계 - Phase 2

> 2026-07-03 세션8. semi-a2a 파트너 위임 Phase 1(unary SendMessage/GetTask/CancelTask + Agent Card + broker/polling worker)을 라이브 검증한 뒤, **A2A spec v1.0 스트리밍 표면**(`SendStreamingMessage`/`SubscribeToTask` + SSE 이벤트)을 코어에 얹는 설계. 이전 결정([partner-delegation §65](v2-a2a-partner-delegation_2026-07-02.md))에서 "후속"으로 유예했던 것을 **호기심·스펙준수·이기종 interop**을 근거로 재개한다. 초안 = 동구님 리뷰 대기.

## 0. 최종 목표 (north star)

**tunaRound 코어가 A2A spec 스트리밍을 구현해, Agent Card에 `capabilities.streaming: true`를 정직하게 광고하고, 임의의 스펙 준수 A2A 클라이언트(다른 tunaRound가 아니어도)가 task를 던진 뒤 그 진행(submitted -> working -> completed+artifact)을 SSE로 실시간 구독할 수 있는, 표준 A2A 서버가 되는 것.**

이 목표가 서비스하는 큰 그림: tunaRound는 "같은 레포 위 2-에이전트 설계 토론"에서 출발했고, A2A 레이어로 "이기종 파트너에게 서브작업을 위임하고 실시간으로 지켜보는" 표준 기반 협업 코어로 확장한다. 궁극적으로 Agent Card의 skills 광고 -> dispatcher가 best-fit 파트너 선택 -> 실시간 스트리밍 위임("모듈 제작 위임" 시나리오, [partner-delegation §56](v2-a2a-partner-delegation_2026-07-02.md)).

### 비목표 (오해 방지)

- **자율성(AutoLoop)이 아니다.** 스트리밍은 transport/interop 완성도의 목표이지 제어 평면 자율화가 아니다. 사람 주도(HITL)는 유지된다. 자율 turn-triggering은 여전히 Stage 4로 별도 보류.
- **우리 자신의 워커를 이벤트 구동으로 바꾸지 않는다.** 워커(대화형 CLI 에이전트)는 계속 poll_tasks/claim/complete로 폴링한다(브로커 토폴로지 유지). 스트리밍은 **dispatcher-facing 실시간 읽기**와 **외부 A2A 클라이언트 interop**만 담당한다. "요청 안 한 party(워커)에게 서버가 push"하는 어려운 방향은 이번 스코프가 아니다.

### 정직한 가치 평가

우리 자신의 에이전트-투-에이전트 UX에는 스트리밍의 실익이 modest하다(우리 dispatcher도 에이전트라, SSE를 백그라운드 프로세스가 소비해 에이전트를 깨우는 형태 = 폴링과 UX 동일). **스트리밍의 진짜 가치는 (1) 스펙 준수로 외부 A2A 에이전트와의 interop, (2) 코어 이벤트 흐름 역량 확보(학습), (3) 공개 레포의 신뢰 신호다.** 복붙 UX 제거는 폴링 watcher로도 되므로, 이 작업은 "UX 고치기"가 아니라 "표준 A2A 시민 되기"로 프레이밍한다.

## 1. 정찰 결과 (스펙 표면 + 현 코드)

### A2A spec v1.0 스트리밍 표면 (2026-07-03 확인)

- 메서드(PascalCase): **`SendStreamingMessage`**(§3.1.2), **`SubscribeToTask`**(§3.1.6). 구 슬래시 표기(`message/stream`/`tasks/resubscribe`)는 폐기.
- SSE 프레이밍: 각 이벤트는 **StreamResponse 래퍼**(§3.2.3) = `task` | `message` | `statusUpdate` | `artifactUpdate` 중 **정확히 하나**. JSON-RPC 응답(같은 req id)을 SSE `data:` 프레임으로 스트리밍.
- 이벤트 스키마:
  - **TaskStatusUpdateEvent**(§4.2.1): `taskId`(req), `contextId`(opt), `status`(TaskStatus, req), `final`(bool, 종료 이벤트 표시), `metadata`(opt).
  - **TaskArtifactUpdateEvent**(§4.2.2): `taskId`(req), `contextId`(opt), `artifact`(Artifact, req), `append`(bool, opt), `lastChunk`(bool, opt), `metadata`(opt).
  - **TaskStatus**: `state`(TaskState), `message`(opt), `timestamp`(opt).
  - **Artifact**: `artifactId`(req), `name`(opt), `parts`(req), `metadata`(opt).
- capability 게이트: `AgentCard.capabilities.streaming`이 false/부재면 `SendStreamingMessage`/`SubscribeToTask`는 **`UnsupportedOperationError` 반환 MUST**(§3.3.2).

### 현 코드 위치

- `src/a2a_server.rs`: `/a2a` JSON-RPC(SendMessage/GetTask/CancelTask, PascalCase §9.1) + `/.well-known/agent-card.json`. `AgentCapabilities { streaming: false, push_notifications: false }`(하드코딩).
- `src/mcp.rs`: worker inbox 툴(poll_tasks/claim_task/complete_task) + dispatcher 툴(send_task/get_task).
- `src/store/a2a.rs`: `TaskState`(6-state 채택), `Task`/`Message`/`Artifact` 타입, 스키마 v6.
- **crux**: 모든 task 상태변이가 `SqliteStore`의 세 메서드를 통과한다.
  - `create_task_from_message` (submitted) <- handle_send(a2a) + send_task(mcp).
  - `update_task_state` (working via claim / canceled via cancel) <- claim_task(mcp) + handle_cancel(a2a).
  - `complete_task` (completed + artifacts) <- complete_task(mcp).

## 2. 아키텍처

### 2.1 이벤트 버스 = store 계층 (척추)

`SqliteStore`가 선택적 **`TaskEventBus`**(= `tokio::sync::broadcast::Sender<TaskEvent>`)를 보유한다. 세 변이 메서드가 **커밋 성공 직후** `TaskEvent`를 publish한다. broadcast::send는 논블로킹 sync라 rusqlite 동기 경로에서 그대로 호출 가능하다.

- 장점: emit이 store 단일 지점이라 `/a2a`와 MCP **두 경로가 자동으로 다 커버**된다. 호출자마다 emit 배선할 필요 없음.
- `TaskEvent`(내부 표현) = `{ task_id, context_id, kind: Status|Artifact, snapshot }`. SSE 직렬화 시 StreamResponse 래퍼로 변환.
- 버스가 None이면(스트리밍 미사용 구성) emit은 no-op. 기존 unary 경로 무영향.

### 2.2 두 SSE 엔드포인트 (dispatcher-facing)

`/a2a` 라우트에서 method로 분기:

- **`SendStreamingMessage`**: task를 생성(submitted, SendMessage와 동일)하고 **axum SSE 응답**(`Content-Type: text/event-stream`)을 연다. 해당 task_id로 버스를 구독해:
  1. 즉시 초기 `task`(submitted) 프레임 emit,
  2. 워커가 claim하면 `statusUpdate`(working),
  3. 워커가 complete하면 `artifactUpdate`(결과) + `statusUpdate`(completed, `final: true`),
  4. `final: true`에서 스트림 종료.
- **`SubscribeToTask`**: **이미 존재하는** in-flight task의 스트림에 (재)구독. 현재 스냅샷을 초기 이벤트로 보내고 이후 버스 이벤트를 이어붙인다. 재연결/늦은 구독용.

각 SSE `data:` 프레임 = `{"jsonrpc":"2.0","id":<req id>,"result":<StreamResponse>}`.

### 2.3 capability 게이트

- 구현 완료 전까지 두 스트리밍 메서드는 `UnsupportedOperationError`(streaming:false와 일관) 반환.
- 구현 완료 후 `build_agent_card`의 `streaming: true`로 플립. 이 플립이 "T 완료"의 신호.

## 3. 태스크 분해 (Phase 2 스트리밍)

- **T1 이벤트 버스**: `TaskEvent` 타입 + `SqliteStore`에 선택적 `broadcast::Sender` 보유 + 세 변이 메서드(create/update_state/complete)에 emit. 단위테스트: 변이 -> 구독자 수신.
- **T2 스트리밍 타입**: `TaskStatusUpdateEvent`/`TaskArtifactUpdateEvent`/`StreamResponse` serde(스펙 필드명 verbatim: `taskId`/`contextId`/`status`/`final`/`artifact`/`append`/`lastChunk`). 기존 `Task`/`Artifact`/`TaskStatus` 재사용.
- **T3 `SendStreamingMessage`**: SSE 엔드포인트(생성 + 스트림, final에서 종료). axum `Sse` + broadcast 구독 -> StreamResponse 프레임.
- **T4 `SubscribeToTask`**: 기존 task 재구독(스냅샷 + 이후 이벤트).
- **T5 capability + 게이트**: 미구현 시 `UnsupportedOperationError`, 구현 후 `streaming: true` 플립. Agent Card 테스트 갱신.
- **T6 테스트/데모**: 통합테스트(tower oneshot으로 SSE 구독 -> submitted/working/completed 이벤트 시퀀스 assert). 라이브 데모(로컬: 코어 + 워커 폴링 + `SendStreamingMessage` curl로 이벤트 스트림 관찰, 복붙 0).

## 4. 테스트 전략

- 단위: 버스 emit/수신, StreamResponse 직렬화(스펙 필드명), final 종료 조건.
- 통합: **tower oneshot**(reqwest 결합 회피, session6 §46 백로그와 일치)으로 `/a2a` SSE 핸들러에 SendStreamingMessage 요청 -> 별도로 store 변이(claim/complete) 트리거 -> SSE 프레임 시퀀스 assert.
- 라이브: 로컬 단독(맥 불요) = 코어 + 워커-watcher(폴링) + dispatcher가 SendStreamingMessage로 던지고 이벤트 스트림을 실시간 수신. 이후 크로스머신은 같은 메커니즘(주소만 LAN).

## 5. 정직한 한계 / 스코프 경계

- 워커 방향 push(코어 -> 워커 inbound 알림)는 **이번 스코프 아님**(브로커 폴링 유지). 이유: 대화형 CLI 에이전트는 per-agent 서버를 못 띄운다(Phase 1 결정과 동일 근거).
- push_notifications(webhook), discovery registry, 다중 auth 스킴은 계속 후속(개인 2~3머신엔 과함).
- 우리 dispatcher-에이전트 UX 이득은 modest(§0 정직한 가치). 값은 interop/스펙/학습.

## 6. 열린 결정 (동구님 리뷰)

1. **버스 위치**: store 계층 emit(제안, 단일지점) vs 호출자별 emit. store가 sync라 broadcast sync send로 해결되므로 store 계층을 추천.
2. **feature 게이팅**: 스트리밍을 `serve` feature에 포함 vs 별도 `a2a-streaming` feature. 코드량 작으면 `serve`에 흡수 추천.
3. **`SubscribeToTask` 범위**: T3와 함께 vs 후속(재연결 수요 낮으면 T4 미루기 가능).
4. **컨텍스트 id 정책**: A2A `contextId`를 tunaRound 세션/토론 id와 어떻게 매핑할지(현재 send_task의 context_id opt).
5. **위임 방식**: 구현은 Sonnet 서브에이전트 위임 + Opus 리뷰(규율). T1~T2는 순수/결정적이라 위임 적합, T3~T4는 axum SSE 결합이라 리뷰 밀도 상향.
