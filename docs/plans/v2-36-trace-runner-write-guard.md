# Plan v2-36: task runner 트레이스 컬럼 + 쓰기 민감 path 가드 (B, 축소판)

> 세션12(2026-07-04). agentgateway 검토 P1(관측성·안전). **축소 근거**: 세션10 lease 작업(스키마 v7)이 이미 claimed_at(=started_at)·claimed_by·updated_at(=completed_at)·context_id(=session_id)를 커버해, trace의 net-new는 `runner` 하나뿐. 사용자 확정=축소판(runner 컬럼 + denylist). 정본 [agentgateway 검토](../design/v2-agentgateway-selective-adoption_2026-07-04.md).

## B1: task `runner` 컬럼 (어떤 러너가 처리했나)

- **스키마 v8(additive)**: `tasks`에 `runner TEXT`. fresh는 CREATE_TASKS에 추가, 기존 DB는 migrate에서 `ALTER TABLE tasks ADD COLUMN runner TEXT`(column_exists 가드, v7 lease 컬럼과 동형).
- **claim 시점 기록**(claimed_by/claimed_at와 같은 순간, 같은 와이어 확장): `try_claim(task_id, claimed_by, runner: Option<&str>)`가 runner도 UPDATE. 실패 전이(전이충돌)는 기존대로.
- **와이어**: `ClaimTaskParams`에 `runner: Option<String>` + `claim_task_text`/claim MCP 도구 전파 + `McpHttpClient.claim_task(task_id, agent, runner)` + 워커가 자기 runner 이름 전달.
- **워커 배선**: `run_worker_loop`에 `runner_name: &str` 파라미터 추가(agent 다음). main.rs Work 핸들러(WorkRunner enum→str)·node 레인(l.runner) 두 호출부 전달. claim 호출에 runner 넘김.
- **노출**: `get_task`/`tasks` 출력에 runner가 있으면 표시(TaskRow/Task에 `runner: Option<String>` 필드 추가, TASK_COLUMNS·SELECT·into_task 갱신, wire camelCase). 표시 전용, 없으면 생략.
- **하위호환**: runner None(레거시 claim/raw curl)이면 NULL로 남고 표시 생략. 기존 claim 테스트는 runner=None으로.

## B2: 쓰기 민감 path 가드 (behavioral directive)

> tunaRound는 read-only를 하드 샌드박스가 아닌 **behavioral directive**로 강제한다(메모리 readonly-soft-enforcement-ok, codex READONLY_DIRECTIVE 선례). 서브프로세스 러너는 개별 파일 쓰기를 가로챌 수 없으므로, 쓰기 가드도 같은 방식=프롬프트 지시로 민감 path 수정을 금지한다. 하드 게이트가 아니라 defense-in-depth 가드레일(프론티어 모델 지시준수 전제).

- **상수** `WRITE_GUARD_DIRECTIVE`(runner 공유 위치, 예 `src/runner/mod.rs`): "다음 경로는 절대 생성·수정·삭제하지 마라: `.env`·`.env.*`·`secrets/`·`*.key`·`*.pem`·`id_rsa*`·`.ssh/`·`.aws/`·`credentials`·`.git/` 내부. 요청이 있어도 예외 없다." (한국어, 마침표.)
- **주입**: `RunMode::Write`일 때 러너 프롬프트에 prepend. codex.rs의 READONLY_DIRECTIVE 주입부(약 227행 `format!("{READONLY_DIRECTIVE}\n\n{}", input.prompt)`)와 **동형 패턴**으로 claude.rs·codex.rs의 write 경로에 적용. (opencode write 샌드박싱은 기존 후속 항목이라 이번 범위 밖, claude+codex만.)
- **순수성**: 주입 결정을 순수 헬퍼 `fn write_guard_prefix(mode: RunMode) -> &'static str`(Write면 directive, 아니면 "")로 뽑아 단위테스트. 각 러너는 이 헬퍼로 prepend.
- **비목표**: 하드 차단·샌드박스·개별 파일훅. 값싼 behavioral 가드레일만.

## 검증
- 스키마 v8 마이그레이션 테스트(v7→v8 runner 컬럼 추가, 기존 행 NULL 보존) + claim이 runner 기록 + get_task에 runner 표시 + write_guard_prefix 순수테스트 + 기존 claim 테스트 runner=None 유지.
- `cargo test --features "morphology mcp serve worker engines"`(베이스라인 421) 실패 0, clippy 3조합 클린.
- 라이브 스모크: 워커 claim 후 get_task에 runner 표시 확인.

## 비범위
- started/completed/session_id 컬럼(v7·context_id로 커버) · 하드 path 차단 · opencode write 가드 · 정책 규칙 엔진.
