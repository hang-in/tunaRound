# tunaRound 컨텍스트 노트

> 작업 중 결정과 근거. 계속 append. (규율 #7) 다음 세션이 결정을 재유도하지 않게.

## 2026-06-29 실행 준비

- 스택 Rust+tokio 확정. 단 Plan 01 러너는 **동기 `std::process`**(v1 순차)라 tokio 미사용. tokio는 concurrency가 실제로 필요할 때 도입(YAGNI).
- **Codex 러너 먼저**(Plan 01), Claude 러너는 Plan 02. `codex exec --json` 파싱이 claude stream-json보다 단순.
- 러너는 `Runner` trait 경계. 오케스트레이터가 concrete 엔진에 안 묶이게(선제 설계 #2).
- `RunMode{ReadOnly,Write}`를 처음부터 타입으로(선제 설계 #1). spec §5 쓰기 하드 분리.
- **미확인:** codex 샌드박스 read-only 플래그. 본 plan은 `--sandbox read-only`(read) / `--full-auto`(write) 가정. Task 4 Step 1에서 `codex exec --help`로 확인 후 진행(규율 #10).
- 실행 방식: subagent-driven (Sonnet per task, Opus 리뷰). tunaRound 관례("구현=Sonnet, Opus 리뷰·검증").
- push는 천천히(개인 프로젝트). 커밋은 논리 단위로 진행.

## 실행 중 교정

- **Plan 01 Task 1 컴파일 순서 버그 교정.** plan 원안은 Task 1 `runner/mod.rs`에 `pub mod codex;`를 두고 lib.rs를 Task 5에 도입했는데, codex.rs가 없는 Task 1에서 `cargo build`가 깨진다. 교정: **lib.rs를 Task 1부터** 두고(통합테스트가 `tunaround::` 접근), `pub mod codex;` 선언은 codex.rs가 생기는 **Task 2로** 미룸. plan 문서는 실행 후 동기.
- 구현은 feature 브랜치 `feat/v1-agent-runner`에서 진행(main 직접 구현 금지).
- **Codex 샌드박스 플래그 실측 교정(Task 4, #10).** `codex exec --help` 결과 plan 가정 `--full-auto`는 **실재하지 않음**. 실제는 `-s/--sandbox <read-only|workspace-write|danger-full-access>`. 채택: **Write=`--sandbox workspace-write`**(레포 쓰기 허용), **ReadOnly=`--sandbox read-only`**(말하기 턴). plan 문서의 `--full-auto`는 Plan 01 종료 시 동기 필요.
- 미확인: `--color=never`(=형) vs `--color never`(공백형). codex가 = 형도 통상 허용. 실제 통합 실행 시 확인.

## Plan 01 완료 (2026-06-29)

- 러너 레이어 완료. 브랜치 `feat/v1-agent-runner`, 커밋 5330063~e7949f9. 전체 10 테스트 green, `cargo build`/`clippy` 클린.
- parse의 중첩 if를 let-chain으로 정리(edition 2024). dead_code 경고 전부 해소.
- 다음: Plan 02(Claude 러너, stream-json NDJSON, StreamLine 파싱, INV-3 토큰 fallback, idle watchdog). 그 전에 브랜치 마감(merge/PR) 결정 필요.

## Plan 02 완료 (2026-06-29)

- Claude 러너 완료. 브랜치 `feat/v1-claude-runner`(80ca2cb~2b18382) -> main 머지. 전체 17 테스트 green, build/clippy 클린.
- `claude --help` 실측으로 가정 플래그 전부 확인(교정 불필요). `RunError::Agent` 변형 추가(in-band 에러).
- 러너 레이어 완결(Codex + Claude, 둘 다 `Runner` trait). 다음: Plan 03 토론 오케스트레이터(두 러너를 trait로 주입, build_round_prompt 순수함수, 드라이빙 루프, consensus, 자리/쓰기 지목). idle watchdog은 hardening plan.
