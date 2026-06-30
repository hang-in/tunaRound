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

## Plan 04 완료 (2026-06-29) — v1 본체 완성

- 전사 영속 완료. 브랜치 `feat/v1-store`(21dbfc5~1cc75bf) -> main. 33 테스트 green, resume 스모크 통과(저장 -> 이어받기).
- `src/store/`: StoredMessage(id/parent 트리-ready) + JSON save/load. Session.save_state/resume + main `cargo run -- state.json`(시작 resume, 종료 save). v1은 JSON, SQLite는 v2.
- **v1 본체 완성: 러너(Codex+Claude) + 오케스트레이터 + REPL + 영속.** 돌아가고, 저장/재개되고, 실 에이전트로 검증됨. 남은 건 hardening(idle watchdog, consensus /conclude, 자리/쓰기 지목).

## Plan 06 Hardening 완료 (2026-06-29) — v1 완료

- `/conclude`(synthesizer 종합) + `@engine`(자리 지목). 브랜치 `feat/v1-hardening`(464bf37, 0c4b282) -> main. 38 테스트 green. 둘 다 run_round 재사용, additive.
- **v1 완료.** 본체 + hardening. idle watchdog · 에이전트 쓰기 지목(RunMode::Write 행사) · Redis 멀티세션=git-tree 분기 · N좌석 로스터 · ratatui/web 는 v2.
- 다음 세션 = v2. 핸드오프는 docs/prompts/.

## v2 착수 (2026-06-29) — brainstorming으로 우선순위 확정

- 사용자가 "v2 끝까지 자율 진행"(특별한 결정만 확인) 지시. brainstorming으로 v2 첫 수 = **idle watchdog**(신뢰성 먼저) 확정.
- v2 우선순위: (1) idle watchdog [P0, 진행중] (2) 에이전트 쓰기 지목=협업코딩 (3) N좌석 로스터 (4) Redis 멀티세션=git-tree [신규 인프라, 착수 전 결정 필요 - 자율 진행에서 제외] (5) 리치 프론트.
- 근거: 나머지 4개는 "앱을 더 많이/오래 쓴다"는 전제 -> 신뢰성이 토대. idle watchdog은 작고 자기완결적.

## v2 Plan 01 idle watchdog 설계 결정 (2026-06-29)

- **공유 헬퍼 `src/runner/exec.rs`**로 추출(양 러너의 spawn->read->wait 동일, watchdog 단일 출처). 범위 결정 = watchdog + stderr 동시 배수(pipe-buffer 데드락도 제거).
- 출처 = tunaFlow `claude.rs` L429~629 검증 패턴. **race 수정**: watchdog_done AtomicBool + RAII WatchdogGuard(trailing-kill 차단, tunaFlow 2026-04-29 버그 반영). timed_out을 종료코드 검사보다 먼저 확인.
- **신규 의존성 0**: parking_lot 안 씀, std::sync로 충분. tokio도 불필요(동기 러너).
- kill = **단일 PID**(tunaFlow와 동일). 고아 grandchild+pipe 드문 경우는 후속 프로세스-그룹 kill로(위험 섹션). 테스트는 `exec sleep`로 단일 프로세스 보장.
- 기본 idle_timeout=600s(INV-4), 러너 필드 + `with_idle_timeout`로 테스트 주입. RunError::Timeout 추가(additive, exhaustive match 없음 확인).

## v2 Plan 01 idle watchdog 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-idle-watchdog`(a142c11 docs, 3414cf2, 78dd033) -> main. 43 테스트 green, build/clippy 클린.
- Sonnet 서브에이전트 구현 + Opus 리뷰. 계획서와 정확히 일치(벗어남 없음). 러너 타임아웃 테스트 안정적이라 `#[ignore]` 불필요.
- `src/runner/exec.rs`: run_with_watchdog(공유) = spawn -> stdin주입 -> stderr 동시배수 -> stdout 라인읽기(타이머리셋) -> watchdog 스레드 -> timed_out 먼저검사 -> 분류. WatchdogGuard(RAII)로 trailing-kill race 차단.
- 다음: v2 Plan 02(설정 구동 N좌석 로스터, docs/plans/v2-02-roster.md 작성됨). 오케스트레이터 N-ready라 main.rs + 신규 roster 로더만. 신규 의존성 0.

## v2 Plan 02 N좌석 로스터 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-roster`(af69db9, bb23e22) -> main. 48 테스트 green, build/clippy 클린, 스모크 3종 통과.
- `src/roster.rs`: JSON 로스터(Roster/SeatConfig serde) -> build_participants(_checked) + build_registry(엔진별 1러너, claude/codex만, 미지 엔진 에러). `src/main.rs` `--roster <path>` 수동 파싱(positional state backward compat 유지). `examples/roster.json`.
- 같은 엔진 다중 좌석 OK(registry 엔진별 1러너 공유, run_round이 자리별 프롬프트 분기). per-seat model·신규 엔진 러너는 후속.

## v2 자율 세션 종료 지점 (2026-06-29) — 결정 대기

- idle watchdog + N좌석 로스터까지 자율 완료(전부 main, 미푸시). 남은 v2는 전부 "특별한 결정" 필요라 자율 진행 멈춤:
  - **협업 코딩(쓰기 지목):** 설계안 docs/design/v2-write-delegation-design_2026-06-29.md. 결정 3건 = (1)claude 쓰기 권한 수위(--dangerously-skip vs --permission-mode acceptEdits) (2)쓰기 대상 디렉토리 (3)실행 전 확인 프롬프트.
  - **Redis 멀티세션=git-tree:** 신규 인프라(Redis) 결정 필요.
  - **리치 프론트(ratatui/web):** 신규 의존성 결정 필요.
- 사용자가 돌아오면 위 결정부터 받고 이어간다.

## v2 Plan 03 협업 코딩 착수 (2026-06-29) — 결정 확정

- 사용자 결정: (1) claude 쓰기 권한 **현행 `--dangerously-skip-permissions` 유지**(수개월 무사고) (2) 쓰기 대상 **cwd 레포** (3) 실행 전 확인 프롬프트 **없음**(역할 분리로 동시 같은 파일 경합 없음, 한 번에 한 자리만 쓰기).
- 설계: `@engine!`로 쓰기 턴 지목. run_round에 mode 파라미터 추가(기존 호출 ReadOnly=동작보존), Command::Write + step 분기. 쓰기 인프라(러너 인자)는 v1 구현 재사용.
- main 푸시 시작함(이 시점 origin 동기화, 8bc3bea..240cd83). 이후 논리 단위로 푸시.

## v2 Plan 03 협업 코딩 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-write-delegation`(ee96a53 docs, 9c55b97, 1ae8b49) -> main. 52 테스트 green, build/clippy 클린.
- `@engine! <msg>`로 한 자리를 쓰기 턴 지목 -> 그 자리만 RunMode::Write로 cwd 레포 편집. run_round에 mode 파라미터(기존 호출 ReadOnly=동작보존). 쓰기 인프라는 v1 재사용.
- **이제 tunaRound는 토론 + 실제 협업 코딩 도구.** 남은 v2(전부 인프라/의존성 결정 필요): Redis 멀티세션=git-tree / 리치 프론트 ratatui·web / 신규 엔진 러너(tunaLlama·opencode 좌석).
- 후속(쓰기 관련): git diff 자동 요약, 자동 커밋, 쓰기 결과 리뷰 라운드 - 필요 시.

## v2 멀티세션 착수 + 교정 (2026-06-29)

- **교정:** 내가 "Redis가 정말 필요한가"로 멀티세션 아키텍처를 재론해 사용자가 제지("이미 다 결정했는데 뒤집지 마라, claude-mem 활용해라"). 설계문서 L33·L108·L144-145가 이미 확정: **Redis 멀티세션=git-tree 분기, tunaSalon session_bus 포팅, 브랜치=세션, presence/snapshot 신규**. 메모리 [[no-relitigating-decisions]] 추가. 앞으로 v2 착수 전 design 문서 v2 섹션 + claude-mem 먼저.
- 분해 3플랜으로 진행(사용자 GO): **Plan 04 session_bus 포팅(격리 토대)** -> Plan 05 세션모델(브랜치=세션) -> Plan 06 REPL통합+presence/snapshot.
- async 경계 결정(내가 정함): tokio/async는 bus 레이어에만, 동기 코어 유지, block_on 브리지는 Plan 06. 신규 의존성 tokio/redis 0.32/futures-util(설계문서 L145 승인).

## v2 Plan 04 session_bus 포팅 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-session-bus`(c0ee2bb docs, 0783179, 86aa482, 11e1f52 주석정리) -> main. 56 테스트(49 pass + 2 ignored 라이브 Redis + 5 통합), build/clippy 클린.
- `src/session_bus.rs`: tunaSalon 포팅. SessionBus trait + RedisBus(6함수 async) + RedisBusHandle + RedisSessionKeys/RedisStreamMessage. 키 prefix `session:{id}:...`, env `TUNAROUND_REDIS_URL`. redis 0.32 API 출처와 동일(조정 불필요).
- 완전 격리: 기존 동기 코드 미접촉, main.rs 런타임 미도입. 평소 cargo test는 Redis 없이 green.
- 다음: Plan 05 세션 모델(브랜치=세션, store parent_id 실사용). 착수 전 design 문서 + claude-mem에서 분기/세션 결정 확인할 것([[no-relitigating-decisions]]).

## v2 Plan 05 세션 모델 착수 (2026-06-29)

- 설계문서가 "분기 UI는 v2(Q8)"로 남긴 미결 항목 -> 사용자에게 구체 모델만 확인(재론 아님). **확정: in-store 논리 트리(옵션 A).** git 브랜치 백업/세션파일 복사는 기각.
- 설계: Session이 선형 transcript -> 트리(messages: Vec<StoredMessage> + head). 라운드마다 active path(root->head)를 run_round에 넘기고 반환 round를 head 분기로 append. `/branches`(tree_summary)+`/checkout <id>`(head 이동). run_round/러너 무변경(트리 로직 = store 순수함수 + Session 격리).
- 저장 포맷 StoredSession{messages, head}, load_session은 레거시 bare-array 폴백(head=마지막 id). Redis/presence/멀티프로세스는 Plan 06.

## v2 Plan 05 세션 모델 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-session-model`(7ded26d, c9510fe, 5b25827) -> main. 63 테스트(61 pass + 2 ignored), build/clippy 클린.
- Session: 선형 transcript -> 트리(messages+head). active_path(root->head)를 run_round에 넘기고 반환 round를 head 분기로 append(이중 append 없음 검증). `/branches`(tree_summary)+`/checkout <id>`. 저장 StoredSession+레거시 폴백.
- **단일 프로세스 분기 토론 동작.** 멀티세션 04 토대+05 트리모델 done. 남은 Plan 06 = Redis 통합(각 분기 session_id)+presence/snapshot 신규+block_on 브리지(멀티프로세스 동시 세션). Plan 06은 async<->sync 브리지·net-new presence라 가장 무거움, 착수 전 설계 필요.

## v2 Plan 06 Redis 통합 착수 (2026-06-29)

- 사용자 확정: 미러 + observe + resume **전부 한 플랜**(read 쪽 첫 동작 질문에 "둘 다").
- 설계 핵심: write path는 sync(SessionBus fire-and-forget mpsc -> 백그라운드 tokio), read path(observe/resume 일회성 GET·subscribe 루프)만 main에서 block_on. payload=store 타입 재사용(snapshot=StoredSession, event=이번 라운드 Vec<StoredMessage>). owner lease=process id, 경고만(강제차단 아님). bus=None이면 기존 동작 불변.
- **검증 한계(정직):** observe/resume 라이브는 라이브 Redis + 2 터미널 필요 -> 수동/#[ignore]. 자동 테스트는 fake bus write-path + 파싱만. 자동 green이 라이브 동작 보장 안 함.
- Task: 1 session_bus snapshot / 2 Session 미러(fake bus 테스트) / 3 main.rs 런타임+observe+resume(수동 스모크).

## v2 Plan 06 Redis 통합 완료 (2026-06-29) — 멀티세션 완성

- 구현 완료. 브랜치 `feat/v2-redis-integration`(e72c867, c46121c, eb470b8, 389fe09 정리) -> main. 66 테스트(63 pass + 3 ignored), build/clippy 클린. Redis 없이 cargo run /quit 정상(bus=None 불변).
- session_bus snapshot(set/get + fire-and-forget) + Session 미러(append_round 후 event=새메시지/snapshot=전체트리) + main.rs tokio 런타임 + `--observe`(구독 루프)/`--session`(snapshot seed+owner lease+refresh).
- 리뷰 정리: --session 재개의 중복 RedisBusHandle spawn 제거(bus_boxed 재사용, 389fe09).
- **검증 한계:** observe/resume 라이브는 라이브 Redis+2터미널 필요라 자동검증 불가(수동 1회 확인 필요). 자동 테스트는 FakeBus write-path+파싱만.
- **멀티세션 완성(04 토대+05 트리+06 통합).** v2 설계문서 멀티세션 로드맵 끝. 남은 v2 백로그(결정 필요): 리치 프론트 ratatui·web / 신규 엔진 러너 좌석(tunaLlama·opencode).

## v2 Plan 06 라이브 검증 + 버그 수정 (2026-06-30)

- 로컬 Redis(brew 8.8.0) 설치 후 실 검증. bus 3 #[ignore] / resume(--session) / observe(--observe) / 실 3라운드 컨텍스트 유지 전부 통과. git 청결(ReadOnly).
- **실 라운드로만 잡힌 버그:** mirror가 fire-and-forget이라 /quit 시 마지막 라운드 snapshot 유실(events=3, snapshot=2). resume 정확성 결함.
- **수정:** 종료 직전 `session.snapshot_json()`을 동기 `set_snapshot`으로 1회 기록(Session::snapshot_json 추가). 1라운드 재검증 통과(snapshot이 라운드 보존). 브랜치 `fix/v2-06-snapshot-flush`(50edea4). 잔여: per-round 이벤트는 best-effort(관찰자는 라운드 중 수신), resume는 종료 flush로 보장.
- redis-server는 검증 후에도 기동 상태(brew 설치). 끄려면 `redis-cli shutdown nosave`.

## 설계 방향 리뷰 (2026-06-30) — 결정 보류

- 사용자가 "로컬 멀티세션 broker(PTY로 claude/codex 라이브 세션 + Redis stream + 수동 라우팅)" 설계 대화를 검토 요청. 내 결론: **이건 tunaRound 현재 one-shot 재주입 모델과 다른 패러다임**(PTY 라이브 세션 = 컨텍스트가 에이전트 내부). PTY는 tunaRound가 의도적으로 피한 파싱 복잡도(턴 종료 판정 등)를 되살림. local-first·manual-first·ctx-handle 순서는 옳음. **추천 = 하이브리드**(one-shot 유지 + ctx-handle + side-by-side UI + draft→approve relay, PTY 없이). "tunaRound 피벗 vs 별도 tunaSalon v0" 결정 필요. 아직 미결.

## 설계 방향 수렴 (2026-06-30)

- 사용자 비전: 다른 터미널의 claude/codex가 A2A로 토론·대화·협업. **PTY 아니라 A2A(MCP 도구로 서로 메시지, 버스=Redis)가 맞는 길**로 합의(터미널 스크래핑 회피).
- 핵심 난제 = **turn-triggering**: 인터랙티브 CLI는 메시지 와도 스스로 안 깨어남(데몬 모드 없음). 그래서 자율 두-터미널 대화는 큰 과제.
- **트리거 UX 통찰:** 토론은 분리 터미널이 트리거를 *더* 애매하게 만듦(수동 핑퐁). 단일 REPL 오케스트레이션이 이미 깔끔(한 곳 입력 -> 둘 순차). "사람 발화 1회 -> 둘이 N턴 자동 교환 -> 복귀"로 바운드하면 트리거 명확 + 폭주 방지.
- **결정: 두 갈래.** (1) 토론/대화 = 오케스트레이션 + 바운드 자동 교환(Plan 07, 지금) (2) 프로젝트 협업 = 분리 터미널 A2A(MCP+버스, 자율 핸드오프, turn-triggering 해결 필요 = 백로그). #2의 "codex 리뷰 -> claude에 전달 -> claude 응답" 예시 상당 부분은 #1 토대(방향 지정 라운드)로도 표현됨. **#1부터.**

## v2 Plan 07 바운드 자동 교환 착수 (2026-06-30)

- `/debate <n> <주제>`: 사람 발화 1개 -> run_round을 N회 반복(라운드1=주제, 라운드2~N=연속 프롬프트 반박/심화/수렴) -> 누적 출력. 기본 3턴, 최대 10 clamp(비용 폭주 방지). 각 라운드는 기존 append_round(트리·Redis 미러 그대로). 새 인프라 0. fake 러너 TDD.
- **완료(2026-06-30):** 브랜치 `feat/v2-bounded-debate`(c5b9339, 01b8860) -> main. 69 테스트(66+3 ignored), build/clippy 클린.

## 북극성: 계층형 공유 맥락 + 능동 검색 (2026-06-30)

- 사용자 핵심 요구: 에이전트가 서로 맥락을 **능동적으로 기억·검색**, 단기(세션)~프로젝트 모든 층. Redis·SQLite 적극, 필요시 vector DB. 설계문서 `docs/design/v2-context-memory-direction_2026-06-30.md`.
- 핵심 전환: "전사 통째 재주입" -> "검색해 관련 슬라이스만 주입(RAG)". 현재 build_round_prompt가 통째 재주입 = 스케일 병목.
- **재주입 vs Redis 구분(사용자 질문):** Redis(Plan 06)는 cross-process 전송/캐시 + 라이브일 뿐, 프롬프트 조립을 안 바꿈 = 재주입 자체를 안 줄임. 재주입 감소 = (a)handle(참조 전달, 온디맨드 expand: Redis 강점) + (b)관련성 검색(vanilla Redis는 전문/의미검색 없음 -> SQLite FTS / vector / RediSearch). 둘 다 아직 미구현(현 Redis는 미러/observe/resume만).
- 저장소 계층: Redis=핫+handle+pubsub / SQLite=시스템오브레코드+FTS 백본 / vector=의미검색.

## 한국어 검색 정답 = secall 포팅 (2026-06-30)

- tuna 단골 "한국어 FTS 형태소" 정답이 secall 코드에 있음. 재발명 금지, 포팅. 메모리 [[korean-search-port-secall]].
- 해법: 형태소 분석기로 선-토크나이즈 -> FTS5(unicode61)에 공백조인 저장("검색을"->"검색"). keep-tags NNG/NNP/NNB/VV/VA/SL(외국어=영어·코드 살림). + BGE-M3 벡터 + 하이브리드(BM25+ANN).
- **Kiwi 메인(품질)**, lindera 폴백. lindera 폴백은 secall 초기 Mac kiwi 컴파일 이슈 잔재(현재 mac에서 Kiwi 동작). tunaSalon은 lindera-only lift라 그것만 보였던 것.
- **임베딩 = 원격 Ollama(로컬 ORT 대체):** SSH 터널 `-L 11435:127.0.0.1:11434` -> `/api/embed` model bge-m3. Embedder=reqwest HTTP + MockEmbedder 폴백. 무거운 ONNX 의존 제거. 터널 떠 있어야 동작, 원격 bge-m3 pull 필요.
- 진화: tunaFlow(vector_search) -> secall(hybrid 정본) -> tunaSalon(lindera+BGE-M3 경량). 설계 v2-context-memory-direction_2026-06-30.md.

## v2 Plan 08 한국어 토크나이저 착수 (2026-06-30)

- secall `tokenizer.rs` 포팅(Tokenizer trait + Kiwi + lindera + factory), String 에러/eprintln 적응(anyhow/tracing 미도입), `morphology` feature 게이트(기본 빌드 무영향). Task 1=lindera(안전), Task 2=Kiwi(컴파일 risk-gate). 격리 모듈, 미배선. 다음=SQLite FTS 선-형태소화.
- **완료(2026-06-30):** 브랜치 `feat/v2-ko-tokenizer`(74f8771, 1059be8) -> main. 기본 66/morphology 72 pass, clippy 클린. kiwi-rs **컴파일 성공**(mac aarch64, 과거 이슈 해소).
- **⚠️ Kiwi 런타임 부트스트랩 실패:** 라이브 테스트에서 libkiwi.dylib 로드 실패 + auto-download 404(`kiwi_mac_arm64_v0.23.2.tgz` 없음). kiwi-rs 0.1.4가 libkiwi v0.23.2 받으려다 upstream 에셋 부재 -> **create_tokenizer("kiwi")가 lindera 폴백**. Kiwi 메인 코드는 준비됐으나 실효는 lindera. 후속: kiwi-rs 버전 핀/libkiwi 수동 설치/upstream 확인. **Windows에선 Kiwi cfg 제외 = lindera만이라 무관.**

## 맥 세션 종료 + Windows 이관 (2026-06-30)

- 사용자: 다음 작업은 **Windows로 이관**(완전 새 세션, /clear 아님). 맥 작업 여기까지. 상세 핸드오프 작성됨(docs/prompts/).
- 정리: redis-server 내림(`redis-cli shutdown nosave`), SSH 터널(2232) 종료, observer 프로세스 종료. (brew redis 설치는 남음 - 필요시 `brew uninstall redis`.)
- **Windows 주의:** (1) Kiwi cfg 제외 -> 토크나이저 = lindera(정상). (2) Redis 라이브 검증은 Windows에 redis 필요(WSL/Memurai/Docker). (3) 원격 Ollama 터널: Windows ssh도 `-p [사설포트] -L 11435:127.0.0.1:11434`(bge-m3 검증됨 dim 1024). (4) claude/codex CLI 경로/실행이 Windows에서 다를 수 있음(러너 spawn 확인).

## Windows 첫 세션: 빌드 검증 + Plan 09 착수 (2026-06-30)

- **빌드 검증(맥 패리티 달성):** 기본 `cargo test` 66/morphology 72 pass, build/clippy 클린. 처음엔 러너 timeout/spawn 픽스처 4건 Windows 실패(`#!/bin/sh`를 bin으로 직접 spawn -> Windows 직접 실행 불가). 수정: OS별 픽스처(Unix=.sh, Windows=무출력 .cmd, Rust 1.77.2+가 .cmd를 cmd.exe 래핑). 커밋 `3f44a48`(미푸시). cfg(unix) 게이트 안 하고 양 OS 커버리지 유지.
- **⚠️ 남은 리스크(gotcha #4, 미검증):** 프로덕션 러너는 `Command::new("claude")`/`("codex")`(확장자 없음)로 spawn. Windows 실제는 `claude.cmd`(npm shim)일 수 있고 .cmd 자동 래핑은 **이름이 .cmd로 끝날 때만** -> 실 에이전트 스모크 전 러너 실행파일 해석 점검 필요(tunaFlow wrap_windows_script 참고).
- **전역 설정 gotcha #0:** Windows엔 `~/.config/agents/COMMON.md` 없음(`~/.claude/CLAUDE.md`가 `@RTK.md`만 import, COMMON 미로드). 단 Windows 자체 CLAUDE.md가 공통 계약(결론우선/findings-first/검증사다리/보안/한국어)을 자체 포함 -> 치명적 공백 아님. 일원화하려면 COMMON.md 복원 + import 틸드 경로(별 트랙).
- **Plan 09 결정(사용자 확정):** 다음 = SQLite 시스템오브레코드 + FTS5(선-형태소화). 범위 = **격리 모듈 우선**(store/sqlite.rs + 테스트만, REPL/main JSON 미접촉). 의존성 = **새 sqlite feature**(rusqlite 0.31 bundled optional). 스토어는 토크나이저 비의존(선-토크나이즈 텍스트 주입), morphology는 통합 테스트에서만 결합. 출처 답습 = secall `store/schema.rs`(FTS5 unicode61 + UNINDEXED 역참조) + `search/bm25.rs`. 출처 레포 D드라이브 확인(`D:/privateProject/seCall`, `tunaSalon`). plan = docs/plans/v2-09-sqlite-fts.md.
- **Plan 09 완료(2026-06-30):** Task 1 `c61cf11`(Sonnet 위임: 스키마/마이그레이션/save_session/load_session) + Task 2 `181f46a`(Opus: FTS 검색 테스트). **Windows rusqlite bundled 컴파일 OK(21초, MSVC C:\BuildTools 자동탐지).** sqlite 68/sqlite+morphology 75 pass, 기본 61 불변, clippy 양 조합 클린. **핵심 실증**: `morpheme_indexing_matches_inflected_form` 통과 = "검색을" 형태소 색인 -> "검색" 쿼리 매칭(Windows lindera 경로). 미푸시.
- **잠재 와트(기록):** `exec.rs` 러너 테스트는 `bin:"sh"` 의존 -> Git Bash(sh on PATH)에선 green, 순수 PowerShell(sh 미발견)에선 spawn 실패. 정본 검증은 Bash 경유. 서브에이전트가 PowerShell로 돌려 "2 fail" 오인했던 원인.
- **Plan 09 다음 슬라이스:** (a) 영속을 SQLite로 전환(시스템오브레코드, REPL/main + Redis 스냅샷 조정) (b) `build_round_prompt` RAG화(통째 재주입 -> 검색 슬라이스) (c) 벡터(원격 Ollama bge-m3 dim 1024) -> 하이브리드.
