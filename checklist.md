# tunaRound 구현 체크리스트

> 규율 #7. task 완료 시 체크. plan 전문은 docs/plans/.

## Plan 01: 스캐폴드 + Codex 러너 (docs/plans/v1-01-agent-runner.md)

- [ ] Task 1: 스캐폴드 + 도메인 타입(RunInput·RunOutput·RunMode·RunError) + Runner trait
- [ ] Task 2: dedup 순수함수
- [ ] Task 3: Codex JSONL 파서 순수함수
- [ ] Task 4: Codex argv 빌더 (read/write 모드, `codex exec --help`로 샌드박스 플래그 확인)
- [ ] Task 5: CodexRunner 통합 (가짜 CLI fixture)

## 다음 plan (미작성)

- [ ] Plan 02: Claude 러너 (stream-json NDJSON) + idle watchdog
- [ ] Plan 03: 토론 오케스트레이터
- [ ] Plan 04: 전사·영속 (트리-ready)
- [ ] Plan 05: thin REPL 프론트
