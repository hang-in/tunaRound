# agentgateway에서 tunaRound가 선별 도입할 요소 (검토 노트)

> 2026-07-04. agentgateway(MCP/A2A/LLM/API 트래픽을 route/secure/observe/govern하는 AI-native gateway)의 인프라 설계 요소 중 tunaRound에 맞는 것만 선별한다. tunaRound는 범용 gateway가 아니라 **개인 규모, 사용자 주도, 신뢰 네트워크 전제**의 코딩 에이전트 라운드테이블 + 작업 라우터다. 관련: [브로커 거버넌스](v2-broker-governance_2026-07-03.md) · [에이전트 레지스트리](v2-agent-registry-uuid-tags_2026-07-04.md) · [파트너 위임](v2-a2a-partner-delegation_2026-07-02.md) · [a2a-usage](../reference/a2a-usage.md).

## 0. 한 줄 결론

이 제안의 8할은 "이미 있는 것의 재명명"이고, 진짜 net-new는 2개(정책 게이트, 태스크 트레이스)뿐이다. 그중 정책 게이트(규칙 엔진)가 tunaRound를 gateway로 변질시킬 유일한 실질 위험이다. **취할 것 = capability 태그(이미 함) + 값싼 trace/denylist. 나머지는 gateway 정체성이라 외부 인프라 뒤로 outbound 위임하는 현재 경계가 정답.**

## 1. 반드시 가져올 3개 / 가져오면 안 되는 3개

**가져올 3개**
1. **capability 태그 기반 agent registry** = Plan v2-34로 이미 구현(UUID+태그). `capabilities=[...]`는 우리 태그로 흡수, capability routing = `to_selector`.
2. **flat task trace 컬럼**(runner/session_id/started_at/completed_at) = `tasks` additive 스키마. 관측성 이득 대비 최저 비용.
3. **쓰기 경로 민감 path denylist**(`.env`/`secrets/**`) = 규칙 엔진 아닌 고정 denylist. `write_lane_disrupts_node` 가드와 동류.

**가져오면 안 되는 3개**
1. **정책 규칙 엔진(DSL/OPA류 `[[policy.rule]]`)** = behavioral read-only 결정([readonly-soft-enforcement])과 정면충돌 + gateway 변질.
2. **`[backend.*]`를 agent/roster와 별개 레지스트리로** = 레지스트리 3중화. runner abstraction이 이미 backend.
3. **artifact lineage DAG** = YAGNI. flat trace로 충분.

**가장 먼저 손댈 곳**: Plan v2-34 완료(done) + `capabilities`를 태그 키로 표준화 → `tasks` trace 컬럼 additive(스키마 v8).

## 2. 질문별 판정 (확정/추론/권고)

### Q1. Core = "Agent Work Router" 재정의
- **확정(6/8 이미 있음)**: transcript SoT, search index, task queue, agent registry(v2-34), A2A/MCP routing point.
- **아직 없음(2/8)**: policy gate(전무), backend registry("레지스트리"는 아니고 factory), artifact lineage store.
- **권고**: 정의어에 없는 2개(policy/lineage)를 넣지 말 것. 문서가 코드를 앞서면 다음 세션이 있는 줄 오인한다.

### Q2. Agent Registry (config `[agent.*]`)
- **함정**: roster가 이미 2개다 - `RosterSeat`/`SeatConfig`(토론 좌석, 정적) + `agent_roster`(A2A 라우팅, 런타임). `[agent.*]`는 세 번째.
- **권고(수렴)**: `capabilities` → 런타임 태그. `can_modify_files` → 기존 `RunMode`. 정적 정의가 필요하면 `[agent.*]` 신설 말고 **roster/SeatConfig에 tags 필드 추가**해 워커 기동 시 seed. 세 번째 registry 금지.

### Q3. Policy Gate
- **충돌 2개**: (a) behavioral read-only 결정과 배치. (b) **워커는 헤드리스 데몬이라 "ask"할 사람이 없다** - `default_file_write="ask"`는 대화형 dispatcher에서만 의미, 자율 루프에선 allow/deny로 붕괴.
- **권고(최소·비엔진)**: 쓰기 민감 path 고정 denylist + `allow_unattended_debate=false`(turn cap은 이미 `/debate` 최대 10) + 외부 토큰 env 확인(이미 함). **`[[policy.rule]]` glob 엔진 채택 금지.** 자리 = 러너 spawn 직전 순수 가드(`write_lane_disrupts_node`가 사는 곳).

### Q4. Task Trace / Artifact Lineage
- **확정(있음)**: task_id·context_id·from_agent·to_agent·state·created/updated_at·error(status_message).
- **추가(값쌈, additive)**: runner·session_id·started_at·completed_at(현재 updated_at 하나로 claim↔complete 뭉갬).
- **보류(YAGNI)**: turn_id(A2A task≠turn 1:1), input/output_artifact_ids 계보, policy_decision(정책 안 넣으면 불필요), lineage DAG.

### Q5. Backend Connector 모델
- **확정**: runner abstraction이 이미 backend connector(claude/codex/opencode/http + `--runner a2a`, base_url/model/api_key_env, card/token).
- **충돌**: `--runner a2a` vs `[backend type=a2a]` = 같은 것 두 방식 → UX 분기.
- **권고**: `[backend.*]` 신설 금지. 기존 SeatConfig 필드 사용 + profile(tunaround.toml)로 은닉. "backend = named seat".

### Q6. agentgateway와의 경계 (전적 동의)
- **안 가져옴**: 범용 HTTP/gRPC gateway, k8s/xDS, OPA/enterprise RBAC 전면, LLM provider gateway 전면화, 임의 트래픽 중계, multi-tenant public gateway.
- **경계선**: 개인 2-3머신·신뢰망. 인증=bearer 하나, 라우팅=태그 셀렉터, 관측=flat trace+`tasks` 조망. 그 이상은 외부 인프라에 맡기고 tunaRound는 그 뒤 agent에 outbound A2A 위임(이미 `--runner a2a`).

### Q7. README
- **권고**: 제안 blurb 한 문단만(전사/검색/큐/라우팅/추적 + "범용 gateway 아님, 필요시 외부 A2A 위임"). 아키텍처·정책·registry는 design 문서로만. 아스피레이션을 README에 올리면 "있는 기능" 오독.

## 3. 도입 우선순위

| 우선순위 | 항목 | 이유 | 변경 범위 |
| --- | --- | --- | --- |
| P0(done) | agent registry + capability=태그 | 이미 구현(v2-34), capability routing=to_selector | 완료 + 태그 관례 문서화 |
| P1 | flat task trace 컬럼 | 저비용·고관측성, additive | tasks v8 ALTER + create/claim/complete 배선 |
| P1 | 쓰기 민감 path denylist | 값싼 안전망 | worker.rs 순수 함수 1개 |
| P2 | roster/SeatConfig에 tags(config→런타임 seed) | 정적 정의를 런타임 registry로 흡수(3중화 방지) | roster.rs + 자동등록 배선 |
| P3(후속) | backend를 named seat로 profile 은닉 | 새 네임스페이스 없이 UX 단순화 | 문서 + profile 예시 |
| 보류 | policy 규칙 엔진 / artifact lineage DAG / 별도 backend registry | gateway 변질 / YAGNI / 3중화 | 채택 안 함 |

## 4. 위험

- **복잡도**: 정책 규칙 엔진이 최대 리스크(유지보수·테스트 표면이 검색 스택급).
- **CLI UX**: `[agent.*]`+`[backend.*]`+`--runner`+roster 4중 표현 → "어디 정의?" 혼란. 단일 seat 개념 + profile 은닉으로만 방어.
- **README 과밀**: 아스피레이션 노출 = 오독.
- **gateway 변질**: policy DSL + backend registry + trace 다 넣으면 미니 agentgateway. 개인 도구의 단순함(차별점) 상실.
- **A2A 표준 호환 오해**: capabilities/backend를 표준 Agent Card `skills`처럼 광고 금지. a2a-usage §0의 "구조 차용, 완전 호환 비목표" 정직성 유지.

## 5. 최종 권고

- **v1 전**: agent registry(done) + capability=태그 관례 문서화 + flat trace 컬럼 + 쓰기 민감 path denylist + README 한 문단.
- **v1 이후**: config→런타임 태그 seed + backend를 named seat로 profile 은닉.
- **채택 안 함**: policy 규칙 엔진(behavioral 유지) · artifact lineage DAG · 별도 backend registry · 범용 gateway 전 영역.
