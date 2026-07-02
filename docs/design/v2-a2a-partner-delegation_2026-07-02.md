# tunaRound v2: semi-a2a 파트너 위임 (설계 초안)

> 2026-07-02 세션6. 크로스머신 에이전트 위임을 표준 A2A 프로토콜(Google/Linux Foundation, spec v1.0 2026)로 코어에 얹는 설계. 단일머신 토론과 **별개의 신규 능력**. **초안 = 동구님 리뷰 대기.** 방향 정본 [v2-A2A-core-backend](v2-A2A-core-backend_2026-06-30.md) 확장.

## 0. 한 줄

윈도우 에이전트가 맥 에이전트(또는 Ollama Cloud 등 이기종 파트너)에게 작업을 위임하고, 파트너가 수행·보고하면 검토·재지시하는 **semi-autonomous 에이전트 위임**을, 표준 A2A 프로토콜로 구현한다.

## 1. 용어: half -> semi

- **semi-a2a** = HITL 있는 앱-투-앱 A2A. A2A(에이전트↔에이전트)는 진짜로 성립하고, 자율만 "semi"(사람이 감독·승인).
- 자율 스펙트럼: 수동 relay < **semi-a2a(여기)** < full-auto(AutoLoop=Stage 4, 보류).
- "half"는 "반쪽/미완성"으로 오독되어 폐기. README·CLAUDE.md의 "half-a2a" 표기 정정은 후속 작업.

## 2. 왜 bespoke가 아니라 A2A 표준

- 파트너가 **이기종**이다(맥 Claude Code, Ollama Cloud 모델, 향후 타 프레임워크 에이전트). 이기종 interop이 A2A의 존재 이유. bespoke 관례는 tunaRound 인스턴스끼리만 통함.
- A2A는 MCP를 **보완**한다: MCP(에이전트↔도구, 이미 보유=검색/맥락 pull) + A2A(에이전트↔에이전트, 신규=작업 위임).

## 3. 상호작용 패턴 (토론과 다름 = 중요)

- 토론(기존) = **공유 append-log**(post_turn/read_transcript), N자리 순차-인지 discussion.
- 위임(신규) = **point-to-point Task + 수명주기**(A가 B에 던지고 B가 artifacts 반환).
- 그래서 위임은 전사에 관례를 얹는 게 아니라 **코어에 A2A Task 엔드포인트를 신설**한다.

## 4. 아키텍처

- **코어 = A2A 서버**: JSON-RPC `SendMessage`/`GetTask`/`CancelTask` + `/.well-known/agent-card.json` + `tasks` 테이블. 기존 axum HTTP(serve/core) + bearer 인증 재사용.
- **worker = CLI 에이전트**(Codex / Claude Code 등), 백엔드 **모델은 config**(Claude/GPT/**Ollama**). inbox 폴링(/loop 또는 inbox MCP 툴)로 @me task 수령 -> 수행 -> complete + artifacts.
  - **이기종 파트너 = 다른 CLI 에이전트 / 다른 모델.** "Ollama 파트너" = Ollama 모델로 구동되는 CLI 에이전트. Codex는 OpenAI-compat(`model_provider`/`base_url`)로 네이티브, Claude Code는 Anthropic-API 프록시 필요(덜 매끄러움) -> Ollama엔 Codex가 깔끔한 host.
  - **왜 CLI 에이전트(raw 모델 호출 아님)**: agentic loop(계획·툴·파일편집·반복)를 그대로 얻는다. HTTP engine runner(Plan 17)는 단발 chat이라 그게 없어 "모듈 제작"엔 부적합 -> **HTTP engine은 토론 좌석용으로 유지, worker는 CLI 에이전트로 통일**(별도 headless 모델 어댑터 불필요 = 설계 단순화).
  - 자율: 대화형이면 HITL@worker(semi), headless CLI 루프면 autonomous@worker + HITL@dispatcher(결과 검토). 둘 다 전체적으론 semi.
  - **context 전달**: 필요 맥락을 A2A Message parts에 실어 보냄(push) -> worker가 MCP pull 없이도 작업(#24135 무관). 추가 pull은 선택(대화형=사람 승인, `contextId`로 read_transcript).
- **client(dispatcher)** = `SendMessage` 던지고 `GetTask` 폴링 -> artifacts 검토 -> 재지시/정리.
- **A2A + MCP 합성**: task `contextId` ↔ tunaRound session. worker가 `read_transcript`(MCP)로 그 맥락을 pull해서 작업. 위임(A2A) + 맥락(MCP)이 한 코어에서 결합.

## 5. A2A subset 채택 (최소)

- 메서드: `SendMessage`, `GetTask`, `CancelTask`. (스트리밍 `SubscribeToTask` = 후속)
- 모델: Task{id, contextId, status{state, message}, artifacts[], history[]} / Message{messageId, role, parts[], taskId, contextId} / Part{text|data|url, mediaType} / Artifact(=산출물).
- 8-state 중 사용: submitted, working, input_required, completed, failed, canceled. (auth_required = 후속)
- **Agent Card**: 각 파트너의 skills·transport·security 광고. **이기종 파트너 확장의 핵심 abstraction**(dispatcher가 best-fit 파트너 선택). 최소 버전이라도 포함한다(Phase 2 확장의 토대).

## 6. tasks 테이블 (스케치, 스키마 v6)

`task_id`(PK) · `context_id`(nullable, session 연결) · `from_agent` · `to_agent` · `state` · `message_json`(parts) · `artifacts_json`(nullable) · `created_at` · `updated_at`. SQLite 코어에 신설.

## 7. relay 자동화 (핵심 통합 일감)

- A2A는 프로토콜만 준다. **"대화형 Claude를 A2A worker로" 만드는 건 우리 몫** = 폴링 루프(/loop로 inbox 조회 -> task 수행 -> complete).
- **opportunistic**: 파트너 앱이 켜져 폴링할 때만 task가 진행된다(동구님 수용). 백그라운드 서비스 아님.

## 8. Phase 계획

- **Phase 1**: Claude↔Claude(맥↔윈). 코어 A2A 엔드포인트 + tasks 테이블 + 대화형 worker inbox loop. 최소 round-trip 도그푸딩. #24135 무관(대화형=사람 승인).
- **Phase 2**: 이기종 파트너 확장 = 다른 CLI 에이전트/모델을 worker로 등록(Codex-on-Ollama 등). **신규 어댑터 없이 Phase 1 worker 경로 재사용.** Agent Card로 파트너 skills·모델 광고·선택. "모듈 제작 위임" 시나리오.

## 9. 비범위 / 보류

- 풀 A2A interop: SSE 스트리밍, webhook push, discovery registry, 다중 auth 스킴. 개인 2~3머신엔 과함. 필요 신호 시 후속.
- full-auto AutoLoop(Stage 4).

## 10. 열린 질문 (리뷰 포인트)

1. **[결정 2026-07-02] worker 루프 = 중앙 브로커 토폴로지**: 코어 = A2A 서버 + task 큐. worker = `/loop` + inbox MCP 툴(`poll_tasks`/`claim_task`/`complete_task`)로 폴링·수행·완료. dispatcher = A2A `SendMessage`/`GetTask`. SSE `SubscribeToTask`는 후속. 근거: 대화형 CLI 에이전트는 per-agent A2A 서버를 못 띄우므로 브로커+폴링. dispatch 측은 A2A 호환(외부 A2A 클라이언트도 코어에 던질 수 있음).
2. 파트너 간 auth: 현 bearer로 충분한가, A2A security scheme를 어디까지 채택.
3. dispatcher "wait" UX: blocking `SendMessage(returnImmediately:false)`로 장시간 블록 vs 폴링 + 상태표시.
4. Agent Card 최소 필드(skills 표현 방식).
5. headless worker(모델)의 write 권한 범위(module 제작 = 쓰기, git 백업 전제=수용 가능).

## 11. 검증

- Phase 1 최소 e2e: 윈도우 dispatch -> 맥 worker 수행 -> artifacts -> 검토. 라이브 타이밍 함정 주의(코어 Kiwi init ~3초, 폴링 간격, FIFO flush = Stage 3d 교훈 답습).
