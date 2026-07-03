# A2A outbound 러너 (표준 에이전트에게 위임) 설계 - 세션8

> 2026-07-03 세션8. A2A interop 방향 결정의 결과물. **inbound(제3자가 우리한테 표준으로 던지기 = A+B, 브로커→per-agent 재편)은 폐기**(오픈소스라 필요하면 레포를 가져가면 됨, 소비자 없음). 대신 **outbound(우리가 외부 표준 A2A 에이전트에게 던지기)** = 기반을 만든다. 이래야 semi 수준이라도 정당하게 "A2A"다(우리끼리만 말하는 게 아니라 표준으로 나갈 수 있음). 우리 브로커 모델은 안 건드린다. interop findings=[context-notes 2026-07-03].

## 0. 최종 목표

**`A2ARunner`(신규 `Runner` 구현) = 외부 표준 A2A 에이전트를 워커 파트너로.** 워커가 task를 로컬 CLI/LLM으로 실행하는 대신, 지정한 외부 표준 A2A 에이전트에게 **표준 SendMessage로 위임**하고(a2a-client 크레이트 사용) 결과 artifact를 받아 반환한다. 기존 `--runner` 이기종 모델에 "원격 표준 A2A 에이전트"가 네 번째 파트너 타입으로 얹힌다(Claude/Codex/로컬LLM/**A2A-원격**).

```bash
tunaround work --agent bridge-worker --runner a2a --a2a-card https://some-agent.example/
```

## 1. 결정

- **[결정] a2a-client 크레이트 채택**(손구현 아님). "표준으로 던진다"의 표준성(method 이름·카드 발견·payload)을 우리가 떠안지 않고 검증된 크레이트에 위임한다. 공식 JSON-RPC 명명(슬래시 vs PascalCase) 미해결분도 크레이트 생태계 호환으로 흡수. 의존: `a2a-client 0.2` + `a2a-types 0.2`(optional, feature `a2a-out`).
- **[결정] 우리 브로커 모델 불변.** A2ARunner는 우리 코어의 워커 러너일 뿐, 우리 `/a2a` 서버(다대일 브로커)는 그대로. inbound 표준화(per-agent 재편) 안 함.
- **[결정] sync-over-async.** `Runner::run`은 sync, a2a-client는 async. 워커는 러너를 순수 std::thread에서 돌리므로(reqwest::blocking 이슈로 이미 그렇게 함), A2ARunner.run()이 자기 tokio 런타임을 만들어 block_on해도 안전(ambient 런타임 없음).

## 2. 설계

### 2.1 A2ARunner
- 필드: `card_url: String`(외부 에이전트 카드 URL), `auth_token: Option<String>`, (선택) `to_agent`/`timeout`/`poll_interval`.
- `run(input)`:
  1. `A2AClient::from_card_url(card_url).await?`(카드 발견) `.with_auth_token(token)`.
  2. `send_message(SendMessageRequest{ message: input.prompt를 A2A Message로, .. })`.
  3. 응답이 완료 task(artifacts 有)면 artifact 텍스트 추출. submitted/working이면 `get_task`를 완료까지 폴링(또는 streaming 구독). failed면 RunError.
  4. `RunOutput{ content, tokens }`로 매핑.
  - a2a-types의 정확한 타입(SendMessageRequest/Message/Task/Artifact)은 docs.rs/a2a-types로 확인하며 빌드-에러 기반 교정(interop 스모크에서 일부 파악: SendMessageRequest = tenant/message/configuration/metadata, Message에 role/parts, AgentCard=supported_interfaces).
  - block_on: `tokio::runtime::Builder::new_current_thread().enable_all().build()?.block_on(async { .. })`.
- 순수 매핑(테스트 대상): A2A Task/Artifact -> RunOutput 변환 함수를 분리해 단위테스트.

### 2.2 배선 (main.rs)
- `WorkRunner`에 `A2a` 추가. `--a2a-card <url>`(필수 for a2a), (선택) `--a2a-agent`, `--a2a-token`(외부 에이전트 인증, --token=코어와 분리). factory에서 `#[cfg(feature="a2a-out")] WorkRunner::A2a => Arc::new(A2ARunner::new(card, token))`.
- `a2a-out` 피처 없으면 `--runner a2a`는 친절한 에러.

### 2.3 feature
- Cargo.toml: `a2a-client`/`a2a-types` optional dep + `a2a-out = ["dep:a2a-client", "dep:a2a-types"]`. 기본 빌드 불변(a2a-out off).

## 3. 검증 (대칭: inbound 스모크의 짝)
- **outbound 표준 interop 스모크**: `a2a-rs`의 예제 서버(또는 a2a-client 생태계의 표준 A2A 서버)를 **진짜 독립 표준 A2A 에이전트**로 띄우고, 우리 A2ARunner가 거기에 표준으로 SendMessage -> 결과 수신을 실증. 자기주장 아닌 외부검증. (a2a-rs 서버는 a2a-types로 표준 카드를 서빙하므로 a2a-client from_card_url이 붙을 것.)
- 로컬 데모: 코어 + `tunaround work --runner a2a --a2a-card <a2a-rs 서버>` + dispatcher가 bridge-worker에 task -> A2ARunner가 외부 A2A 에이전트에 표준 위임 -> 결과가 우리 코어 task artifact로.

## 4. 태스크 분해
- **WA1**: `A2ARunner`(a2a-client 통합, sync-over-async block_on, from_card_url->send_message->poll->RunOutput) + `a2a-out` feature + Task/Artifact->RunOutput 순수 매핑 단위테스트.
- **WA2**: `--runner a2a` + `--a2a-card`/`--a2a-token` WorkArgs + main.rs factory 배선.
- **WA3**: outbound 표준 interop 스모크(a2a-rs 예제 서버 상대) + 로컬 데모(우리 코어 경유).

## 5. 스코프·비목표
- **outbound만.** inbound 표준화(제3자가 우리한테)·per-agent 브로커 재편은 안 함(폐기).
- 표준 method 명명 논쟁은 a2a-client에 위임(그 생태계와 호환되면 충분).
- 스트리밍 구독(SubscribeToTask outbound)은 폴링(GetTask)으로 시작, 필요 시 후속.
