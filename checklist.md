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

## Plan 03: 토론 오케스트레이터 (docs/plans/v1-03-orchestrator.md) — feat/v1-orchestrator

- [x] Task 1: 역할 지시문 (roles) (3a13954)
- [x] Task 2: 라운드 프롬프트 조립 (순차-인지) + Participant/Utterance (123ee5d)
- [x] Task 3: run_round + RunnerRegistry (c9af140) — Plan 03 완료, 24 테스트 green, build/clippy 클린

## Plan 05: thin REPL (docs/plans/v1-05-repl.md) — feat/v1-repl (돌아가는 앱, Plan 04보다 먼저)

- [x] Task 1: 명령 파싱 (e35683d)
- [x] Task 2: render + Session.step (d5e3dfc; 5 repl 테스트 green)
- [x] Task 3: main.rs 실 러너 REPL (10dda04) — Plan 05 완료, 돌아가는 앱, 스모크 통과, 29 테스트 green

## Plan 04: 전사 영속 (docs/plans/v1-04-persistence.md) — feat/v1-store

- [x] Task 1: store 타입 + 트리-ready 변환 (21dbfc5)
- [x] Task 2: JSON save/load 라운드트립 (a5456fd; 32 테스트 green)
- [x] Task 3: Session resume + main 상태파일 인자 (1cc75bf) — Plan 04 완료, 33 테스트 green, resume 스모크 통과

## Plan 06: Hardening (docs/plans/v1-06-hardening.md) — feat/v1-hardening

- [x] Task 1: /conclude synthesizer 종합 (464bf37)
- [x] Task 2: @engine 자리 지목 (0c4b282) — Plan 06 완료, v1 완료, 38 테스트 green

## v2 Plan 01: idle watchdog (docs/plans/v2-01-idle-watchdog.md) — feat/v2-idle-watchdog -> main

- [x] Task 1: 공유 watchdog 헬퍼 src/runner/exec.rs + RunError::Timeout (3414cf2)
- [x] Task 2: 양 러너를 watchdog 헬퍼로 배선 (idle_timeout 필드, 기본 600s) (78dd033) — Plan 01 완료, 43 테스트 green, build/clippy 클린

## v2 백로그

- [ ] 에이전트 쓰기 지목(RunMode::Write 행사) = 협업 코딩
- [ ] N좌석 로스터(역할×엔진 동적 구성, tunaLlama·opencode 좌석)
- [ ] Redis 멀티세션 = git-tree 분기 (신규 인프라 - 착수 전 결정 필요)
- [ ] 리치 프론트(ratatui/web)
