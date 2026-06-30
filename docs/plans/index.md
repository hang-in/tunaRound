# Plans — 진행 현황

> 현재 진행 중인 plan (active + partial). 완료된 plan 은 `../archive/plans/completed/` 로 이동.

## 진행 중

| 문서 | 우선순위 | 상태 | 요약 |
|---|---|---|---|
| [v2-08-ko-tokenizer.md](v2-08-ko-tokenizer.md) | P1 | in_progress | 한국어 형태소 토크나이저 포팅(secall): Kiwi 메인 + lindera 폴백, POS keep-tags(SL 포함). morphology feature. 능동검색/한국어FTS 토대. 설계 v2-context-memory-direction |
| [v2-07-bounded-debate.md](v2-07-bounded-debate.md) | P1 | done | v2 바운드 자동 교환: `/debate <n> <주제>`로 사람 발화 1회 -> 에이전트 N턴 자동 교환 -> 복귀. run_round N회 재사용, 최대 10 clamp. 69 테스트, main 머지됨 |
| [v2-06-redis-integration.md](v2-06-redis-integration.md) | P1 | done | v2 멀티세션 통합: Redis 미러(이벤트+스냅샷) + `--observe` 라이브 관찰 + `--session` 재개 + owner lease. 66 테스트, main 머지됨. observe/resume 라이브는 수동 검증 필요. 멀티세션 3플랜(04+05+06) 완성 |
| [v2-05-session-model.md](v2-05-session-model.md) | P1 | done | v2 세션 모델: in-store 논리 트리(Session messages+head, parent_id 실사용), /branches·/checkout 분기 탐색. 저장 포맷 StoredSession(레거시 폴백). 61 테스트, main 머지됨. 단일 프로세스 분기 토론 동작 |
| [v2-04-session-bus.md](v2-04-session-bus.md) | P1 | done | v2 멀티세션 토대: tunaSalon Redis session_bus 포팅(room->session), tokio/redis/futures 신규 의존. 격리 모듈, 라이브 Redis 테스트 #[ignore]. 56 테스트, main 머지됨. 멀티세션 3플랜의 1단계(다음 05 세션모델/06 통합) |
| [v2-03-write-delegation.md](v2-03-write-delegation.md) | P1 | done | v2 협업 코딩: `@engine!` 쓰기 지목, run_round mode 파라미터, Session::step Write 분기. 쓰기 인프라(러너 인자)는 v1 구현 재사용. 52 테스트, main 머지됨 |
| [v2-02-roster.md](v2-02-roster.md) | P1 | done | v2 설정 구동 N좌석 로스터: JSON 로스터 -> participants+registry, main.rs --roster 플래그. 오케스트레이터 N-ready 활용, 48 테스트, main 머지됨 |
| [v2-01-idle-watchdog.md](v2-01-idle-watchdog.md) | P0 | done | v2 idle watchdog(INV-4): 공유 헬퍼 exec.rs + RunError::Timeout + 양 러너 배선. 무출력 행 방지, stderr 동시 배수. 43 테스트, main 머지됨 |
| [v1-01-agent-runner.md](v1-01-agent-runner.md) | P0 | done | 스캐폴드 + Codex 러너(argv·JSONL 파싱·dedup·read/write 모드), 순수함수 TDD. main 머지됨 |
| [v1-02-claude-runner.md](v1-02-claude-runner.md) | P0 | done | Claude 러너(stream-json NDJSON, result 라인 content + INV-3 토큰 fallback, RunError::Agent). main 머지됨 |
| [v1-03-orchestrator.md](v1-03-orchestrator.md) | P0 | done | 토론 오케스트레이터(roles + build_round_prompt 순차-인지 + run_round/RunnerRegistry, FakeRunner). main 머지됨 |
| [v1-05-repl.md](v1-05-repl.md) | P0 | done | thin REPL(명령 파싱 + Session.step + main.rs 실 러너). 돌아가는 앱(`cargo run`). main 머지됨 |
| [v1-04-persistence.md](v1-04-persistence.md) | P1 | done | 전사 영속(StoredMessage id/parent 트리-ready + JSON save/load) + Session resume + main 상태파일 인자. main 머지됨 |
| [v1-06-hardening.md](v1-06-hardening.md) | P1 | done | Hardening: /conclude(synthesizer 종합) + @engine(자리 지목). run_round 재사용 additive. main 머지됨 |

## 부분 완료 / 보류

| 문서 | 사유 |
|---|---|

## 완료

(`../archive/plans/completed/` 참조)
