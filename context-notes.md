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

## Plan 03 완료 (2026-06-29)

- 오케스트레이터 완료. 브랜치 `feat/v1-orchestrator`(3a13954~c9af140) -> main. 24 테스트 green, build/clippy 클린.
- `src/orchestrator/`: roles(역할 지시문) + prompt(build_round_prompt 순차-인지) + mod(Participant/Utterance/RunnerRegistry/MapRegistry/run_round). Runner trait 경계만 의존(concrete 러너 미임포트).
- run_round는 사람 메시지=라운드. 모든 턴 ReadOnly(쓰기 지목 mode 분기는 Plan 05 REPL). consensus 자동추출은 주석 seam만.
- 사용자 지시 "플랜3까지". 여기서 정지. 남은: Plan 04(영속 트리-ready), Plan 05(thin REPL), Hardening(idle watchdog + consensus + 실 CLI 스모크).

## Plan 05 완료 (2026-06-29) — 돌아가는 앱

- "계속 진행해" 지시로 Plan 05(REPL)를 Plan 04보다 먼저(돌아가는 앱 우선). 브랜치 `feat/v1-repl`(e35683d~10dda04) -> main. `cargo run` 구동, 비대화형 스모크(배너/help/save/quit) 통과, 29 테스트 green.
- `src/repl/`: Command·parse_command·render·StepOutcome·Session. main.rs가 실 CodexRunner/ClaudeRunner를 MapRegistry로 묶음. 기본 2자리 claude=proposer, codex=reviewer. v1 에이전트 읽기 전용, 결과 문서는 /save가 전사에서 저장(에이전트 파일쓰기=v2).
- **현재 상태: 토론 코어(runner+orchestrator) + 돌아가는 REPL 완성.** 남은: Plan 04(전사 영속 트리-ready, resume), Hardening(idle watchdog + consensus 합성/conclude + 자리/쓰기 지목 + 실 CLI 통합 스모크).

## 실 에이전트 스모크 통과 (2026-06-29) — 핵심 가설 실증

- `cargo run`에 메시지 한 줄 -> 실 claude(제안자)+codex(리뷰어)가 정상 응답, exit 0, 출력 안 깨짐. fake로 못 본 실 CLI 통합 검증됨.
- 역할 주입·순차-인지·읽기전용 레포 접근(claude가 실제 README 인용) 전부 실증. **v1 핵심 가설(Claude↔Codex 구조 토론이 가치 있나)이 실 에이전트로 증명됨.**
- 주의: claude는 read-only 모드에서 레포를 자율 탐색함(읽기만). 토론 턴 후 `git status` 깨끗(레포 미변경) 확인.
