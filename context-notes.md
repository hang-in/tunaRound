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

## Plan 10 SQLite 라이브 배선 완료 (2026-06-30)

- **방식:** 기존 SessionBus 미러 패턴 답습. `MessageIndexer` trait(비게이트) + `SqliteIndexer`(sqlite feature, `Mutex<SqliteStore>` + tokenize closure 주입) + Session `indexer: Option<Box<dyn ...>>` 필드 + `append_round` 훅 + main `--db <path>`. 추가적(JSON save/load·Redis 미접촉), sqlite off/--db 없음=None=기존 동작 불변.
- **커밋:** Task 1 `e21cf43`(trait+indexer+Session, Sonnet) + Task 2 `5d79a0a`(main --db 3분기 배선 + roundtrip 테스트, Sonnet). sqlite 74/sqlite+morphology 81 pass, clippy 3조합 클린, 스모크 OK. **origin 푸시됨**(README `5c31a1d`와 함께, 63fc071..5c31a1d).
- **이탈(타당):** Send+Sync 위해 Rc→Arc<Mutex> / Connection !Sync라 Mutex<SqliteStore> / 통합테스트는 indexer.rs 단위테스트로(FakeRunner cross-crate 불가 회피) / `--db` 변수 cfg(sqlite) 게이트(unused 경고 억제).
- **절차 교훈:** README를 쓰는 사이 Task 2가 커밋돼 `git push`(README만 의도)가 Task 1·2 코드까지 함께 올림. 또 Task 2를 리뷰 전 푸시 -> 사후 독립검증(빌드/테스트/clippy)으로 그린 확인. **다음부턴 서브에이전트 진행 중 푸시 자제 또는 완료·리뷰 후 푸시.**
- **다음 = Plan 11 검색 주입(RAG):** `build_round_prompt`가 통째 재주입 대신 SqliteStore.search로 관련 슬라이스만 주입. 북극성 핵심. 인덱스는 Plan 10으로 라이브로 채워짐.
- **검색 토크나이저(서브에이전트 보고):** `cargo`는 Bash 툴로 돌릴 것(Git Bash sh 있어 exec.rs sh 테스트 통과; PowerShell이면 2건 거짓 실패).

## Plan 11 검색 주입(RAG) 완료 (2026-06-30)

- **방식(추가적):** prior 통째 재주입은 그대로 두고, 활성 경로 **밖**의 관련 맥락(다른 분기·과거 세션)을 topic으로 검색해 "참고할 만한 과거 맥락(검색)" 섹션으로 **추가** 주입. 검증된 단일세션 품질 보존하면서 능동 검색 기둥만 세움. prior 캡(재주입 토큰 축소)은 품질 측정 후 별 슬라이스(설계 원칙: 검색가능->주입->측정->필요시 축소).
- **구조:** `ContextRetriever` trait(orchestrator, 비게이트) + `build_round_prompt`/`run_round`에 retrieved 슬롯(Task 1, 동작 불변) + `SqliteRetriever`(sqlite, SqliteStore 읽기 + tokenize closure) + `Session.retriever`(with_retriever 빌더) + `retrieve_for`(활성 경로 content dedup, K=5) + main `--db`로 indexer와 별개 읽기 연결(WAL 동시 reader). retriever 없으면 retrieved=&[] = 동작 불변.
- **커밋:** Task 1 `b0dd7bd`(orchestrator 슬롯) + Task 2 `4643977`(SqliteRetriever+Session+main). sqlite 76/sqlite+morphology 83 pass, clippy 3조합 클린, 스모크 OK. **cross-session 검색 단위 테스트 통과 = 능동 검색 실연.** 미푸시.
- **다음 = Plan 12 벡터/하이브리드:** 어휘(FTS)만으론 동의어·의미 약함. 원격 Ollama bge-m3(dim 1024, SSH -p [사설포트] 터널) reqwest 임베더 + MockEmbedder 폴백 + ANN(usearch 또는 cosine) + 하이브리드(BM25+벡터).

## 원격 Ollama Windows 검증 + 벡터/정렬 결정 (2026-06-30)

- **검증됨(Windows):** `ssh -p [사설포트] -o BatchMode=yes [사설계정]@[사설IP] 'curl 127.0.0.1:11434/api/...'` 작동(키 인증, 무비번). `/api/tags`=bge-m3:latest + gemma4:e2b/e4b. `/api/embed` model bge-m3 -> **dim 1024 확인.** 사용자가 안내한 **포트 22는 타임아웃, 실제 포트=2232**(핸드오프 일치). 호스트=[사설IP](이제 세션에 공개됨). 터널형: `ssh -N -p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@[사설IP]`.
- **벡터 라이브 블로커 해소.** 단 **설계 YAGNI 게이트(FTS 부족 입증 시에만, 마지막)는 여전히 유효** -> 사용자 결정=벡터 보류, 정렬 슬라이스(/search)부터.
- **Plan 12 재정의 = /search 명령**(사람이 인덱스 직접 검색, FTS 품질 관측 -> 벡터 도입 근거 수집). plan=docs/plans/v2-12-search-command.md. 기존 Session.retriever 재사용, 신규 의존성 0. 벡터(원안)는 /search로 품질 관측 후.
- **Plan 12 /search 완료(2026-06-30):** `bc2f359`(Sonnet). Command::Search 파싱 + step 핸들러(retriever 재사용, 없으면 --db 안내, 빈 결과 안내, 있으면 render). 기본 70/sqlite 79/sqlite+morphology 86 pass, clippy 3조합 클린. 미푸시.
- **벡터(Plan 원안) 재개 조건:** 라이브 블로커 해소됨(2232/bge-m3 dim 1024). 남은 게이트=YAGNI(FTS 부족 입증). 재개 시 Embedder trait + MockEmbedder + reqwest Ollama(엔드포인트 http://127.0.0.1:11435, 터널 -p [사설포트]) + message_vectors BLOB(dim 1024) + cosine/ANN + 하이브리드(BM25+벡터). semantic feature 게이트.

## Plan 13 벡터/하이브리드 완료 (2026-06-30, 사용자 요청으로 YAGNI 우회 진행)

- **사용자가 벡터 진행 지시**(Ollama 호스트 제공 + "2,3 가자"). 블로커 해소돼 원안대로 구축.
- **구조:** `semantic = ["sqlite","dep:reqwest"]`(reqwest blocking, rustls-tls). `store/embedding.rs`=Embedder trait + MockEmbedder(결정적, sqlite) + OllamaEmbedder(semantic, `{model:bge-m3,input:[..]}`->`{embeddings}`). `sqlite.rs`=message_vectors(schema v2, f32 LE BLOB, content_hash 증분 가드) + index_vectors(Embedder 주입) + vector_search(brute-force cosine) + get_message. `store/mod.rs`=reciprocal_rank_fusion(k=60, secall 답습). SqliteIndexer/SqliteRetriever에 `Option<Box<dyn Embedder>>` - 있으면 벡터색인/RRF 하이브리드, 없으면 FTS 단독(불변). main semantic 시 OllamaEmbedder 2개(indexer/retriever, env TUNAROUND_OLLAMA_URL/기본 11435).
- **커밋:** 1ad8881(embedder) + 30efa51(vectors+cosine) + 8920027(RRF+배선). sqlite 86/semantic 86 pass, clippy 클린, 스모크 OK.
- **라이브 검증:** `ollama_embed_live_dim_1024 ... ok`(로컬 11435 터널 -> 원격 bge-m3, dim 1024). reqwest 클라이언트 end-to-end 동작 확인.
- **한계:** ANN 미도입(brute-force cosine, 규모 시 usearch). 라이브 의미 품질(벡터가 recall 개선하는지)은 실사용 측정 영역. embedder 2중 생성(Arc 공유 후속). reqwest blocking은 Session.step이 block_on 밖이라 안전.
- **다음 = item 3 폴리시:** load_session .ok() 보정 + 토크나이저/embedder Arc 공유.

## Plan 14 에이전트 능동 검색 MCP (2026-06-30, 사용자 선택)

- **방식:** secall rmcp(1.3.0->1.8.0) 답습. `src/mcp.rs` TunaSearchServer = 단일 툴 `search_context(query,limit)`가 기존 SqliteRetriever(하이브리드) 호출 -> Content. `main --mcp-search --db`로 stdio MCP 서버 기동. claude 러너가 `--mcp-config`(serde_json, command=self-exe args=[--mcp-search,--db,path])로 이 서버를 물려 에이전트가 토론 중 자율 호출. `mcp = ["sqlite","dep:rmcp","dep:schemars"]` 피처.
- **커밋:** Task 1 `a65feba`(서버+stdio) + Task 2 `a5a185d`(claude 배선). mcp 89 pass, 기본 71 불변, clippy 클린. ContextRetriever에 `Send+Sync` bound 추가(기존 구현 충족). rmcp Windows 빌드 OK(10초).
- **Task 3 라이브 대기:** 실 claude가 search_context를 실제로 부르는지 = 토큰 소모, 사용자 확인 후. **codex는 gotcha #4로 막힘**(codex.exe 없음, npm shim codex.cmd만 -> Command::new("codex") spawn 실패). codex 능동검색은 gotcha #4(러너 Windows CLI 해석) 수정 후.
- **CLI MCP 설정 실측:** claude `--mcp-config <JSON>`(인라인/파일)+`--strict-mcp-config`. codex는 퍼-런 플래그 없음(`codex mcp add` 영속 or `-c` 오버라이드).
- **gotcha #4 정밀 진단:** `claude`=claude.exe(spawn OK), `codex`=codex/codex.cmd/codex.ps1만(codex.exe 없음). Rust Command::new는 .exe만 덧붙여 찾고 .cmd는 이름이 .cmd로 끝날 때만 -> codex spawn 실패. 수정=러너가 Windows에서 .cmd 해석(tunaFlow wrap_windows_script).

## Plan 15(gotcha #4) + Plan 14 Task 3 라이브 검증 완료 (2026-06-30)

- **Plan 15 `8d02088`:** `exec.rs resolve_bin` - Windows에서 확장자 없는 bin을 PATH에서 .exe/.cmd/.bat/.com 풀경로화(Rust가 .cmd를 cmd.exe 자동 래핑). `run_with_watchdog` spawn 전 호출. 비Windows·확장자/경로 있는 bin은 no-op(기존 .cmd/.sh 픽스처 테스트 무영향). 기본 74/전체 99 pass.
- **라이브 검증(실 에이전트, 토큰 사용):** `printf '...자기 역할...' | tunaround --db smoke.db`(mcp 빌드) -> **claude/proposer + codex/reviewer 둘 다 실제 응답**(gotcha #4 수정으로 codex.cmd spawn 성공 = 라이브 입증). smoke.db에 색인됨.
- **MCP 검증(무토큰):** `tunaround --mcp-search --db smoke.db`에 JSON-RPC initialize+tools/call 직접 전송 -> rmcp 1.8.0 정상, `search_context("발제자")` -> 실 색인된 claude 발언 반환. **MCP 검색 전 체인 입증.**
- **남은 것(모델 행동):** 에이전트가 토론 중 search_context를 자율 호출할지는 모델 판단. 툴 배선·서버·검색은 입증됨. 품질은 `--features "mcp morphology semantic"`(형태소 FTS + bge-m3 벡터)로 빌드 시 ↑.
- **검색 스택 전체 완성:** 형태소 FTS(Plan 8,9) + 라이브 색인(10) + RAG 주입(11) + /search(12) + 벡터/하이브리드(13) + 에이전트 MCP 도구(14) + Windows 러너(15). v2 검색/맥락 북극성 1차 완결.

## 검색 품질 측정 + Plan 17~19 + Kiwi 활성화 (2026-06-30)

- **검색 품질 실측(중요):** tests/search_quality.rs(#[ignore], 통제 코퍼스+Ollama 임베딩) 게이지로 측정. 발견: lindera가 **외래어를 문맥에서 누락**("벡터 임베딩을"→"임베딩" 탈락, "인증을"→"인증"은 정상). 형태소 굴절은 OK, 외래어가 구멍. 벡터는 소규모 코퍼스에서 노이즈 큼. => **기계 동작만 검증했지 품질은 평범**이었음을 인정하고 실측으로 전환.
- **Plan 17 `e1373f9`:** OpenAI 호환 HTTP 엔진 러너(runner/http.rs, engines feature). 한 러너로 ollama/lmstudio/openai/cloud. 로스터 base_url/model/api_key_env. 라이브: Ollama gemma4:e2b /v1/chat/completions 응답.
- **Plan 18 `45cf0c8`:** FTS 리콜 보강 - 색인=형태소+raw 토큰(fts_index), 질의=prefix-AND(fts_query). 외래어 누락 메움(재측정서 "임베딩" #3 히트). index/query 클로저 분리. 기본 feature=morphology+sqlite(4441a18).
- **Plan 19 `fe0ec71` Kiwi 활성화(중요, 재현법):** Kiwi가 Windows에서 막혔던 진짜 원인 = (1) kiwi-rs 0.1.4 auto-download 깨짐(GITHUB_TOKEN 무관, release_json/curl 실패) (2) **latest Kiwi v0.23.2는 kiwi-rs 0.1.4 바인딩과 ABI 불일치 → native ACCESS_VIOLATION**. 해법 = **Kiwi v0.22.2**(0.1.4 README가 겨냥) 수동 설치. `Kiwi::init()`이 discovery(KIWI_LIBRARY_PATH/KIWI_MODEL_PATH 또는 **%LOCALAPPDATA%\kiwi** 기본)를 bootstrap보다 먼저 봄 → 수동 배치로 깨진 다운로드 우회. **설치:** `gh release download v0.22.2 --repo bab2min/Kiwi`로 kiwi_win_x64_v0.22.2.zip(→lib/kiwi.dll) + kiwi_model_v0.22.2_base.tgz(→models/cong/base)를 %LOCALAPPDATA%\kiwi에 추출(`scripts/install-kiwi-windows.sh`). env 불필요. 미설치 시 lindera 폴백. 문서 docs/reference/kiwi-windows-setup.md. **주의: v0.23.2 쓰지 말 것(crash).** Kiwi keep-tags는 base 매칭(VA-I/VV-I 변종). Kiwi도 외래어 음절분할하나 Plan 18 raw+prefix가 FTS 커버.
- **README:** 사용자가 깃헙에서 전면 리라이트(어투 개선) → 로컬 분기와 충돌 → merge에서 사용자 리라이트 채택 + 로드맵 정정·"좌석"→"참가자"·Kiwi 안내만 재적용(`5b8cd36`). origin 동기화됨. "좌석"은 코드(SeatConfig)·일부 plan 문서엔 잔존(내부라 미변경).
- **미반영 후속:** 검색 품질 추가 개선(현실 코퍼스 측정) · 요약 carry-forward(enhancement; 온디맨드 확장은 MCP search_context가 이미 커버) · 예시 로스터 확장 · 리치 프론트(보류).
- **Plan 20 opencode CLI 러너 done(`7fedac2`):** `opencode run --format json` JSONL(text.part.text=본문, step_finish.part.tokens=토큰) 파싱 + 로스터 engine "opencode"(seat.model). 신규 의존성 0, gotcha #4 resolve_bin이 opencode.cmd spawn. **ollama cloud가 opencode에 안정**(Cerebras/짧은 타임아웃은 cold start로 hang). 모델 예: `ollama-cloud/gemma3:4b`. 신규 엔진 = HTTP(17) + opencode(20) 완성.
- **검토할 아키텍처 방향(사용자 제기 2026-06-30): 코어-백엔드 + 에이전트-클라이언트(A2A).** 현재=tunaRound가 매 라운드 에이전트 stateless spawn(-p). 제안=코어(오케스트레이션+검색/메모리) 백엔드 상주 + 에이전트는 MCP 클라이언트로 접속. **이미 씨앗=`--mcp-search`(검색/메모리를 백엔드로 노출)**. 확장=오케스트레이션 툴(read_transcript/post_turn) 추가. **난점=분산 turn-triggering(A2A 백로그 난제) + 컨텍스트 통제 약화.** 두 모델 공존·점진 권고. 큰 포크라 별도 설계 세션. 상세 핸드오프 ⑧-A.

## 2026-06-30 (A) 코어-백엔드 설계 확정 (사용자 결정)

- **A2A를 둘로 분해(설계 흔들림 방지):** (A) 아키텍처 A2A = 코어 상주 백엔드 + 에이전트 접속 클라이언트, **사람이 운전자**(= 가치, 채택). (B) 자율 A2A = 에이전트가 다음 화자 스스로 결정·서로 트리거(= 미래 명시 opt-in, 지금 X). 사용자 확정: **(A)**.
- **(B) 경제 논리(사용자 직관, 기록):** 자율 루프가 비싼 진짜 이유 = 토큰이 아니라 **탐색 공간**. 사람 마이크로매니징 = 매 턴 **가지치기** = 라운드 수↓ = 품질↑·비용↓. (B)의 경제가 뒤집히는 조건 = (1) 토큰 단가 충분히 하락 or (2) 과제가 **검증 가능**(테스트/컴파일/실측 기계 판정)해 사람 없이 수렴. 그 전엔 사람-주도가 싸고 좋다. → (B)는 조건부 옵션, 디폴트는 영원히 사람-주도.
- **핵심 솔기 = turn-policy:** "다음 턴 누가 정하나"를 코어 명시 정책으로 분리. `HumanDriven`(디폴트·유일 구현) / `AutoLoop`(미래 (B), 같은 백엔드 위 정책만 교체). 이 솔기로 (B)는 포크가 아닌 **플러그인 1개**, 켜기 전 비용 0.
- **본질 전환:** push(맥락을 prompt에 통째 밀어넣기) → pull(코어가 전사·검색·요약을 서비스로 노출, 에이전트가 필요분만 도구로 당김). `--recent-turns`(Plan 16)·`--mcp-search`(Plan 14)가 이미 그 씨앗.
- **단계:** Stage 0(검색품질+요약 carry-forward, 코어 경화) → 1(오케스트레이션 툴 read_transcript/get_roster) → 2(주입 push→pull, 재전송량 감소 **실측**=crux) → 3(코어 데몬 분리) → 4(범위 밖=(B)). Stage 1~2는 **에이전트 여전히 stateless spawn**(저위험), 영속 프로세스는 Stage 3 이후.
- **리스크:** codex MCP **도구 실호출** 미검증(Plan 14 T4는 `-c mcp_servers` 인자 수용만 확인) = Stage 1 통과 기준. Stage 2는 통제 약화 위험(포인터에 당길 범위 명시로 완화).
- **정본 문서:** docs/design/v2-A2A-core-backend_2026-06-30.md. **이번 세션 = Stage 0 + (A)설계 병렬.**

## 2026-06-30 검색 품질 트랙 결정 (Memora 참고 후)

- **Stage 0 항목1(검색품질) 완료·커밋**: `581eaa2`(하네스+FTS AND→OR), `30543fb`(precision@k). R@5 0.55→0.90, MRR 0.60→0.90, P@5 0.727. K=5 정당화. 진짜 천장 = Q6 어휘공백(재주입↔재전송) = 벡터/확장 근거점.
- **ChromaDB 비도입(확정)**: ANN=근사라 exact cosine보다 품질 동급↓, 우리 규모(수천 턴) brute-force 충분·정확. 이득은 스케일/운영뿐. 사용자 여러 프로젝트 공통 SQLite 고수(메모리 [[prefer-sqlite-over-vector-db]]).
- **GRPO 비도입(확정)**: RL 정책학습 = 라벨데이터·인프라 필요(우리 없음), 측정 불가, Memora도 experimental.
- **채택(사용자 승인, 무거워도 품질이면 OK)**: cross-encoder 리랭커(secall `model_manager`/`hybrid` 씨앗) + 쿼리 확장(secall `query_expand.rs`, Q6류 어휘공백). 단 리랭커는 임베딩/CE 모델 필요 → **Ollama 터널 의존(현재 DOWN)**.
- **품질 트랙 전략(사용자 문답): 측정-증분, 심판자 우선.** "기능 다 깔고 데이터로 수정"(A)은 귀속불가·비용낭비·락인이라 기각. 순서 = (0) **eval 코퍼스 확대 먼저**(현실 라벨 케이스, 터널 불필요·결정적 FTS로 지금 측정) → (1) 기능 한 개씩 측정·유지/폐기(동시투입 금지) → (2) 프로덕션 데이터는 기능 맹목수정 아니라 **실패 케이스를 eval에 흡수**(hard negative). 기능 "완성(배선+단위테스트)"과 "튜닝(데이터 필요)"은 다른 축. 얇은 eval(10질의)에 튜닝 = 과적합이라 eval 확대가 0번 스텝.
- **다음 품질 슬라이스**: eval 코퍼스 확대(Plan 21 코퍼스 확장판) → 리랭커(터널 복구 후) → 쿼리 확장.

## 2026-06-30 eval 확대 측정 + 리랭커/벡터 분리 (중요)

- **eval 확대 완료(미커밋→커밋예정)**: tests/search_recall.rs 코퍼스 20→40발언, 질의 10→21(어휘·의미공백 질의 추가). 측정 **R@5 0.857 / P@5 0.592 / MRR 0.833**(easy 0.90보다 낮음=변별력↑). floor r5≥0.85, p5≥0.58.
- **핵심 발견 - 두 레버는 다른 문제**:
  - **리콜 공백(FTS 0건/부분)**: Q6 재주입(0.0), Q16 원격접속인증→33 누락(신원확인=어휘 0겹침), Q17 코어백엔드호스팅→35 누락, Q21 오래기억(0.0, '기억' 부재). → **리랭커로 불가**(재정렬은 가져온 것만; recall=0이면 무력). = **벡터(Plan 13 기존)+쿼리확장**의 일.
  - **정밀도/랭킹(가져왔지만 noise)**: Q8 로컬LLM좌석 P@5 0.25, Q19 동의어질의확장 P@5 0.25("확장에"가 msg1 끌어옴). → **cross-encoder 리랭커**의 일.
- **로드맵 정밀화(측정-증분)**: "리랭커부터"가 아니라 **"기존 하이브리드 벡터가 리콜 공백을 메우나" 먼저 측정**(이미 가진 기능, Ollama 터널 필요). 회복되면 쿼리확장 YAGNI. 그 뒤 리랭커=정밀도용(직교). 리랭커 로컬 가능 확인(RTX 3060 Ti 8GB, ~3.7GB 여유; TEI/Infinity 무료; 터널 불요).

## 2026-06-30 벡터 측정 완료 → 쿼리확장·리랭커 둘 다 보류 (측정-우선의 값)

- **터널**: known_hosts에서 2232 호스트 찾아 직접 기동([사설호스트]=[사설IP], d9ng). 모델 bge-m3/gemma4 확인. 하네스: tests/search_recall.rs에 `vector_hybrid_recall`(#[ignore], semantic) 추가, QUERIES 모듈 공용화(FTS/벡터 같은 gold).
- **측정(21질의/40발언)**: FTS R@5 0.857 → **벡터 0.952 / 하이브리드 0.952**, **벡터 MRR 0.976** / 하이브리드 MRR 0.921. FTS 공백 회복: Q16 0.5→1.0, Q17 0.5→1.0, Q6 0→0.667, Q21 0→0.333.
- **결론(측정이 취소시킨 것)**: (1) **쿼리확장 YAGNI 확정** - 벡터가 어휘공백 메움. (2) **리랭커 보류** - 벡터 MRR 0.976(gold 거의 1순위)이라 재정렬 한계이득 미미. 측정 한 번이 두 기능을 안 짓게 막음.
- **단서**: 깨끗한 소코퍼스라 bge-m3에 쉬움. 프로덕션 전사(길고 노이즈·문서多)는 더 어려워 갭 재개 가능 → 그때 리랭커 재검토(로컬 GPU 가능). **하이브리드 MRR < 벡터**: RRF 어휘 arm이 가끔 gold 끌어내림(이 코퍼스선 순수 벡터가 깔끔).
- **검색 품질 트랙 = 현 eval 기준 충분.** 다음 = Stage 1(A2A 오케스트레이션 툴). 검색은 프로덕션 코퍼스 확보 후 재측정.

## 2026-06-30 Stage 2(push->pull) 라이브 측정 - 페이오프 증명 + 권한 블로커 발견

- **Task 1 done(f15911b)**: ContextMode(Push/Pull) + is_mcp_capable + build_round_prompt pull 분기(포인터, prior/retrieved 생략) + --pull-context(--db 없으면 경고+Push) + [ctx] 프롬프트 크기 계측. behavior-preserving. 기본 118/mcp+sqlite 124.
- **Task 2 라이브 측정(실 claude/codex, 3턴, --db, --recent-turns 미설정이라 carried도 빈값)**:
  - **토큰 페이오프 증명**: push는 전사 누적에 선형 증가(claude 284->5184->9770, codex 2453->7623->12489). pull은 평평(claude 433->431->429, codex 2413->2307->2417). claude 95%↓, codex 81%↓. **프롬프트가 전사 길이와 탈동조** = (A) 핵심 페이오프.
  - **블로커 발견(중요)**: pull에서 read_transcript가 **헤드리스 `claude -p` 권한모드서 차단**. claude 응답에 "read_transcript 권한이 막혀 직전 4턴 전사 대신 이전 결론 메모를 근거로" 명시. 게으른 pull 아니라 **하드 권한 블록**. 에이전트는 레포(cwd)+사전지식으로 보충 → 그럴듯하나 **전사 grounding 아님**(예 "상주코어<->접속" = 레포 설계문서에서 읽음). coherence 부분 착시.
  - **결론**: 토큰 감소 실재, 단 현 spawn 설정선 pull 무효. **Task 3 = 러너 spawn에 MCP 도구 권한 자동허용**(claude --allowedTools 또는 permission-mode로 tuna-search 승인, codex 대응) 후 재측정. 측정-우선이 조용한 품질저하를 사전 차단.
