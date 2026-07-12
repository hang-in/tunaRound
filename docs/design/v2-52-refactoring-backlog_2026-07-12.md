# v2-52: 리팩토링 백로그 (Codex 전수조사 기반, 2026-07-12)

> 출처: Codex 전수조사(2026-07-12), presence 타임라인(v2-50)·큐레이션 기억(v2-51) 머지 후 v0.5.0 릴리즈 전 검토. 이 문서는 즉시 처리분과 defer분을 분리해 기록합니다. 착수 원칙은 세션16식 god파일 분할(한 번에 하나, worktree 격리, 공유 파일은 mac 조율, 북극성상 급진 재작성 지양)입니다.

## 0. 즉시 처리 (v0.5.0에 포함)

브랜치 `fix/quality-gates-pre-0.5.0`에서 처리합니다. 기능 회귀가 아니라 기존 품질 게이트 정리입니다.

- **clippy `--all-features --all-targets` 3 errors**: `src/runner/claude.rs:415`(항상 참 assertion 단순화) + "very complex type" 2건(`type` 별칭 factoring, 하나는 `src/repl/mod.rs:697` 부근). canonical CI가 `--all-targets`를 안 돌려(대부분 테스트 코드) 놓쳤습니다.
- **exec.rs 테스트 이식성**: `src/runner/exec.rs`의 `spec()`(약 215행)가 `bin: "sh"`를 하드코딩해 sh 없는 clean Windows에서 `idle_no_output_triggers_timeout`·`output_then_exit_succeeds_no_false_timeout`·`nonzero_exit_is_spawn_error_not_timeout` 3건이 실패합니다. Git Bash sh가 있는 우리 머신·CI에서는 통과해 잠복해 있었습니다. OS 인지형 `spec()`(Unix=sh -c, Windows=cmd/powershell 등가) 또는 `#[cfg(unix)]` 게이트 + Windows 등가로 이식성을 확보합니다.
- **프론트 `Feed.tsx` react-hooks 경고 4건**: `machineOf`·`runnerOf` 누락 dependency 추가, `workerMeta` 불필요 dependency 2건 제거.

## 1. Defer: 포맷·CI 강화 (mac 조율 필요)

- **`cargo fmt` 전역 드리프트**: `cargo fmt --all -- --check`가 약 1008 hunk 드리프트를 보고합니다(레포가 rustfmt를 강제한 적 없음). 순수 cosmetic이나 전역 재포맷은 거대 diff라 git blame 오염·mac 인플라이트 작업 충돌 위험이 있습니다. **mac과 시점 조율 후 일괄 `cargo fmt --all` 1회 + CI에 `cargo fmt --all -- --check` 게이트 추가**를 별도 청소로 진행합니다.
- **CI 강화**: 위 fmt 게이트와 함께 canonical clippy를 `--all-features --all-targets`로 확대(테스트 코드까지 커버)해 위 즉시-처리분 같은 잠복 이슈를 앞으로 CI가 잡게 합니다.

## 2. Defer: 구조 리팩토링 (세션16식 전용 청소)

정당한 지적이나 크고 비-blocking입니다. 각 항목은 전용 세션에서 worktree 격리로 점진 수행하고, 착수 전 계약(공개 API·테스트)을 고정합니다.

### P1. `main.rs` 명령 dispatch 분리
`src/main.rs`(약 1,143줄)가 CLI 정규화·profile 병합·DB 구성·runner 구성·REPL 실행·core/serve 분기를 모두 담습니다. command별 실행 함수 추출이 안전합니다: `run_chat`·`run_join`·`run_core`·`run_reindex`·`build_search_backend`·`build_runner_registry`. 전면 아키텍처 변경이 아니라 함수 추출.

### P1. `mcp.rs` 책임 분리
`src/mcp.rs`(약 1,353줄)에 검색·transcript read/write·A2A task 조작·agent registry·HTTP MCP 서버 조립·인증/세션 기본값이 섞여 있습니다. 이미 있는 `src/mcp/format.rs`·`src/mcp/server.rs` 방향으로 `mcp/search.rs`·`mcp/transcript.rs`·`mcp/tasks.rs` 경계를 점진 확장합니다.

### P2. Task wire format을 구조화
`src/mcp/format.rs:63`·`src/worker.rs:82`가 `[id] from=... state=... ctx=... msg=...` 문자열을 직접 생성·파싱합니다. 자유 형식 메시지를 문자열 프로토콜로 나르는 구조가 취약합니다. 순서: ① JSON/JSONL 응답 추가 → ② worker가 구조화 응답 우선 → ③ 문자열 형식은 하위호환 유지 → ④ 이후 문자열 parser 제거.

### P2. Store DTO ↔ 도메인 모델 경계
`orchestrator`·`repl`·`store`가 `StoredSession`·`StoredMessage`·`Utterance`를 직접 공유합니다. 중립 도메인 snapshot 타입(`ConversationSnapshot`·`MessageNode`·`BranchHead`)을 두면 SQLite 스키마 변경이 REPL·prompt 로직까지 전파되는 문제를 줄입니다. 다음 큰 기능 전에 두는 편이 좋습니다.

### P2. 대형 SQLite 모듈 분리
`src/store/sqlite/tasks.rs`(약 1,542줄)에 schema/migration·task CRUD·lease·state transition·event history·replay·tests가 집중돼 있습니다. `tasks/state.rs`·`tasks/lease.rs`·`tasks/replay.rs`로 나누면 상태 머신 변경 시 영향 범위가 줄어듭니다. 상태 전이 테스트가 충실하므로 리팩토링 안전망이 있습니다.

## 3. Codex가 정상 확인 (회귀 없음 근거)

1차 검토에서 지적됐던 다음은 현재 코드에서 보완돼 있습니다: `--context-map` fail-fast 파싱, A2A task 조건부 상태 전이, first-completer-wins 보호, MCP `isError` 전달, 세션 ID MCP 기본값 배선, 세션 축소 시 orphan vector/validity 정리, Unix process group 및 Windows `taskkill /T`, `cargo check --no-default-features`·`--all-features`, frontend production build.

## 4. 우선순위 요약

| 우선순위 | 항목 | 처리 |
|---|---|---|
| P0 | Windows exec 테스트 이식성 | 즉시(0.5.0) |
| P0 | clippy `--all-targets` 3건 | 즉시(0.5.0) |
| P0 | frontend hook 경고 | 즉시(0.5.0) |
| P1 | `cargo fmt` 전역 + CI 게이트 | defer(mac 조율) |
| P1 | `main.rs` command dispatch 분리 | defer |
| P1 | `mcp.rs` 책임 분리 | defer |
| P2 | task 문자열 프로토콜 → JSON | defer |
| P2 | store DTO ↔ 도메인 모델 분리 | defer |
| P2 | `tasks.rs` 분리 | defer |
