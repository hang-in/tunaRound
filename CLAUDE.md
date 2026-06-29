# tunaRound - Claude Code Handoff

> 이 파일은 다음 세션이 이어가기 위한 핸드오프입니다. 제품/설계 전모는 [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md)(현행 spec).

## 표기 / 작업 규칙 (tuna 생태계 공통)

- 사용자 응답·문서는 **한국어 존댓말**. **em-dash 사용금지**(일반 대시 `-` 또는 콜론 `:`). ANSI 박스 드로잉 자제.
- 도메인 도착 URL/도메인은 비노출(소스공개, 서비스 비공개).
- 구현 위임은 **Sonnet 서브에이전트**(codex 비사용), Opus가 스펙·리뷰·검증.
- 한 세션 한 목적. 검증(build/test)과 commit/push는 분리.

## 개발 행동 규율 (이 프로젝트 실험 적용, 2026-06-29)

> 전역 규칙 아님. 이 레포 실험 적용. **전문·근거·예시·위임 라우팅은 [docs/reference/development-guidelines.md](docs/reference/development-guidelines.md)**.
> 10개 중 #1·#2·#3·#4·#8·#9·#10은 전역 COMMON.md가 이미 always-on으로 강제하므로 여기 중복하지 않는다. 아래는 이 프로젝트 신규 3개만 둔다.

- **#5 한국어 문장 끝은 마침표.** 리스트/예시 앞이라도 `:`로 끝내지 않는다. 콜론은 라벨·key-value·문장 중간만.
- **#6 새 소스 파일 첫 줄 = 역할 한국어 한 줄 주석.** Rust 예: `// 토론 라운드 프롬프트를 조립하는 순수 함수`. config 파일 제외.
- **#7 비trivial 작업 전 plan + `checklist.md` + `context-notes.md`.** plan만 주고 코딩 요청 시 멈추고 checklist·notes 먼저 만들지 묻는다.

## 현재 상태 (2026-06-29)

- **v1 완료 + v2 멀티세션 완성.** v1 본체 + hardening. v2 done: **01 idle watchdog · 02 N좌석 로스터 · 03 협업 코딩(`@engine!`) · 04 session_bus 토대 · 05 세션 모델(in-store 트리, `/branches`·`/checkout`) · 06 Redis 통합(`--observe`/`--session`, 미러+관찰+재개).** 66 테스트(63 pass + 라이브 Redis 3 #[ignore]), build/clippy 클린. 토론 + 협업 코딩 + 분기 토론 트리 + 멀티프로세스 동시 세션.
- 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md). 진행 현황은 [docs/plans/index.md](docs/plans/index.md)(v1 Plan 01~06, v2 Plan 01~06 done).
- 변경은 origin/main 동기화(푸시됨). **멀티세션 v2 완성 + `/debate`(바운드 자동 교환, Plan 07) 추가.** 69 테스트.
- **북극성(2026-06-30):** 계층형 공유 맥락 + 능동 검색 - 에이전트가 단기(세션)~프로젝트 모든 층 맥락을 능동 기억·검색. 핵심 전환 "전사 통째 재주입 -> RAG(검색 슬라이스 주입)". 설계 [docs/design/v2-context-memory-direction_2026-06-30.md](docs/design/v2-context-memory-direction_2026-06-30.md). 첫 스텝 조율 중(재주입 감소: handle+windowing vs SQLite+FTS 백본). 백로그: 분리터미널 A2A 협업(turn-triggering) / 리치 프론트 / 신규 엔진 러너.
- **라이브 검증 완료(2026-06-30, 로컬 Redis):** bus #[ignore] 3 / resume / observe / 실 3라운드 컨텍스트 유지 전부 통과. 실 라운드로 버그 1건 발견·수정(종료 시 마지막 snapshot 유실 -> 동기 flush, fix/v2-06-snapshot-flush). 검증법: `TUNAROUND_REDIS_URL=redis://127.0.0.1/ cargo run -- --session demo` / 다른 터미널 `... --observe demo`. redis-server 끄기: `redis-cli shutdown nosave`.

## 무엇을 만드나 (요약)

터미널에서 **사용자가 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론** 도구. 같은 레포 위에서 사람 주도로 토론하고, 결론을 **결과 문서로 자동 기록**해 구현으로 넘긴다.

**핵심 결정(brainstorming 2026-06-29):** 사람 주도 대화형 / 공유 컨텍스트 = 같은 레포+공유 문서(컨텍스트팩 없음) / 읽기 전용 화자 + 사람이 쓰기 지목 / 순차-인지 턴 / 자리마다 역할 주입 / v1=2자리 고정 / consensus carry-forward(종료는 사람) / 스택 Rust+tokio.

**레이어(출처):** 에이전트 러너(tunaFlow `claude.rs`/`codex.rs` 포팅) + 토론 오케스트레이터(tunapi `core/roundtable/` 청사진 -> Rust 재구현) + 전사·영속(파일/rusqlite, 트리-ready) + 프론트(thin REPL).

**v1 비목표 -> v2:** Redis 멀티세션 = git-tree 다중 브랜치 / N>2 좌석 로스터(로컬LLM·opencode) / 리치 TUI(ratatui)·웹 / 협업 코딩.

## 출처 레포 (포팅 시 읽기)

- **tunapi**(전전신, Python): `~/privateProject/tunapi/src/tunapi/core/roundtable/` - 토론 오케스트레이터 청사진(`orchestrator.py`/`prompt.py`/`rt_participant.py`/`session.py`). 역할·순차-인지·follow-up·consensus.
- **tunaFlow**(Rust): `~/privateProject/tunaFlow/src-tauri/src/agents/{claude,codex}.rs` - CLI 러너(`stream_run`) + hardening.
- **tunaSalon**(Rust, v2용): `src/session_bus.rs`(Redis), `src/chat.rs`의 `render_chat`(ratatui), `src/flow.rs`(FlowMeter, 선택).

## 다음 세션 첫 행동

1. `cargo run`으로 앱 동작 확인(claude/codex CLI 필요). 진행 현황은 [docs/plans/index.md](docs/plans/index.md), 결정 로그는 `context-notes.md`.
2. 다음 = **남은 v2 백로그 중 택1**(각각 결정 필요): 리치 프론트 ratatui·web(신규 의존성, tunaSalon `render_chat` 포팅) / 신규 엔진 러너 좌석(tunaLlama·opencode, 외부 CLI - 로스터는 이미 N-ready라 러너만 추가). 또는 Plan 06 observe/resume 라이브 수동 검증부터. 착수 전 design 문서 v2 섹션 + claude-mem으로 기결정 확인(재론 금지).
3. 작업 추적은 `checklist.md`·`context-notes.md`(규율 #7). 위임은 Sonnet 서브에이전트 + Opus 리뷰(subagent-driven).
