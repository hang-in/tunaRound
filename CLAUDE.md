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

## 현재 상태 (2026-06-30, Windows 세션 3)

- **세션 3: (A) 코어-백엔드 half-a2a Stage 0~3a + 코드리뷰/리팩토링.** Stage 0(검색 OR 개선·요약이월·eval 확대 → 벡터측정으로 쿼리확장/리랭커 도입취소) · Stage 1(read_transcript·세션id) · **Stage 2 push→pull 라이브검증**(토큰 80~95%↓·grounding 유지, pull=claude전용·codex=push폴백) · Stage 3 설계(Plan 25)+3a-1(`--serve-mcp` HTTP MCP 상주)+3a-2(러너 URL 접속). Batch A/B(리뷰 버그수정·리팩토링). 기본 ~128/전체 ~135 pass.
- **v1 완료 + v2 Plan 01~19 완성.** 01~08(Mac): watchdog·로스터·협업코딩·session_bus·세션모델·Redis통합·/debate·토크나이저. 09~20(Windows): 09 SQLite+FTS5 · 10 라이브 색인 · 11 검색주입(RAG) · 12 /search · 13 벡터/하이브리드 · 14 에이전트 MCP(search_context) · 15 gotcha#4(Windows .cmd spawn) · 16 재주입축소(--recent-turns) · 17 HTTP 엔진 러너(ollama/lmstudio/openai) · 18 FTS 리콜 보강 · 19 Windows Kiwi 활성화 · 20 opencode CLI 러너. 검증: 기본 105 / `--features "semantic morphology mcp engines"` 112 pass, clippy 클린. 전부 origin/main 푸시.
- 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md). 진행: [docs/plans/index.md](docs/plans/index.md)(v1 01~06, v2 01~08 done).
- **>>> 최신 핸드오프: [docs/prompts/v2-handoff_2026-06-30_session3.md](docs/prompts/v2-handoff_2026-06-30_session3.md) 먼저 읽기 <<<** (half-a2a Stage 0~3a + 코드리뷰 + 남은 항목). 정본 방향: [docs/design/v2-A2A-core-backend_2026-06-30.md](docs/design/v2-A2A-core-backend_2026-06-30.md). 이전: [session2](docs/prompts/v2-handoff_2026-06-30_session2.md)
- **북극성(2026-06-30):** 계층형 공유 맥락 + 능동 검색. **1차 완결**: 형태소 FTS(Kiwi v0.22.2 수동/lindera 폴백) + RAG 주입 + /search + BGE-M3 벡터/하이브리드 + 에이전트 MCP 도구 + 재주입 축소. 설계 [docs/design/v2-context-memory-direction_2026-06-30.md](docs/design/v2-context-memory-direction_2026-06-30.md). **남은 항목:** 검색 품질 추가 개선(현실 코퍼스) · 요약 carry-forward · 예시 로스터 확장 · 리치 프론트(보류). **검토할 방향:** 코어-백엔드 + 에이전트-클라이언트(A2A) - 핸드오프 ⑧-A. 신규 엔진(HTTP+opencode)·Kiwi·검색스택은 done.
- **검증/주의:** 멀티세션 라이브 검증 통과(맥, 로컬 Redis). 임베딩=원격 Ollama(SSH `-p [사설포트]` 터널, bge-m3 dim 1024, 검증됨). **⚠️ Kiwi 런타임 버그**(libkiwi 404)->현재 lindera 실효; **Windows는 Kiwi cfg 제외=lindera만**이라 무관. 맥 정리: redis 내림·SSH터널 종료(brew redis 설치는 남음).

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

1. **[docs/prompts/v2-handoff_2026-06-30_session2.md](docs/prompts/v2-handoff_2026-06-30_session2.md) 먼저 읽기** + `context-notes.md` + `docs/plans/index.md`. `cargo test`(기본) + `cargo test --features "semantic morphology mcp"`로 상태 확인(**cargo는 Bash 툴로**).
2. 검색/맥락 북극성은 1차 완결(Plan 09~19). 다음 = 핸드오프 ⑤의 남은 항목(opencode CLI 참가자 / 검색 품질 추가 개선 / ctx-handle 요약 / 리치 프론트 보류) 중 사용자 지정으로 착수. Kiwi는 v0.22.2 수동 설치(`scripts/install-kiwi-windows.sh`), 미설치 시 lindera 폴백. Ollama 터널 11435(SSH -p [사설포트]), Redis 6379.
3. 작업 추적 `checklist.md`·`context-notes.md`(규율 #7). 위임 Sonnet + Opus 리뷰. 굵직한 결정 재론 금지. 서브에이전트 진행 중 git add -A 레이스 주의.
