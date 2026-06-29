# Plans — 진행 현황

> 현재 진행 중인 plan (active + partial). 완료된 plan 은 `../archive/plans/completed/` 로 이동.

## 진행 중

| 문서 | 우선순위 | 상태 | 요약 |
|---|---|---|---|
| [v2-04-session-bus.md](v2-04-session-bus.md) | P1 | in_progress | v2 멀티세션 토대: tunaSalon Redis session_bus 포팅(room->session), tokio/redis/futures 신규 의존. 격리 모듈, 라이브 Redis 테스트 #[ignore]. 멀티세션 3플랜(04 토대/05 세션모델/06 통합+presence)의 1단계 |
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
