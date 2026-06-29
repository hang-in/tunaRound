# tunaRound 구현 체크리스트

> 규율 #7. task 완료 시 체크. plan 전문은 docs/plans/.

## Plan 01: 스캐폴드 + Codex 러너 (docs/plans/v1-01-agent-runner.md)

- [x] Task 1: 스캐폴드 + 도메인 타입(RunInput·RunOutput·RunMode·RunError) + Runner trait (5330063)
- [x] Task 2: dedup 순수함수 (c9628e8)
- [x] Task 3: Codex JSONL 파서 순수함수 (f2872b4)
- [x] Task 4: Codex argv 빌더 (c1a5a41; 실측 교정: --full-auto 없음 -> Write=`--sandbox workspace-write`, ReadOnly=`--sandbox read-only`)
- [x] Task 5: CodexRunner 통합 (가짜 CLI fixture) (e7949f9) — Plan 01 완료, 10 테스트 green, build/clippy 클린

## Plan 02: Claude 러너 (docs/plans/v1-02-claude-runner.md) — feat/v1-claude-runner

- [x] Task 1: claude argv 빌더 (80ca2cb; --help 실측: 가정 플래그 전부 확인)
- [x] Task 2: stream-json 파서 + RunError::Agent (032e550; 전체 스위트 green, codex 무영향)
- [x] Task 3: ClaudeRunner 통합 (2b18382) — Plan 02 완료, 17 테스트 green, build/clippy 클린

## 다음 plan (미작성)

- [ ] Plan 03: 토론 오케스트레이터
- [ ] Plan 04: 전사·영속 (트리-ready)
- [ ] Plan 05: thin REPL 프론트
- [ ] Hardening: 양 러너 idle watchdog(INV-4) + 실 CLI 스모크
