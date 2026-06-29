# Plans — 진행 현황

> 현재 진행 중인 plan (active + partial). 완료된 plan 은 `../archive/plans/completed/` 로 이동.

## 진행 중

| 문서 | 우선순위 | 상태 | 요약 |
|---|---|---|---|
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
