---
title: tunaRound v2 Windows 이관 핸드오프 (맥 세션 종료 2026-06-30)
type: prompt
status: active
priority: P0
updated_at: 2026-06-30
owner: shared
summary: 맥에서 v2 Plan 01~08 완성(idle watchdog/로스터/협업코딩/Redis 멀티세션/debate/한국어 토크나이저), 전부 main+origin. 다음은 Windows 새 세션. 북극성=계층형 맥락+능동 검색(secall 포팅). 다음 콘크리트=SQLite FTS 선-형태소화. Kiwi 런타임 버그·Ollama 검증·Windows gotcha 포함.
---

# tunaRound v2 Windows 이관 핸드오프 (2026-06-30)

> 다음 세션은 **Windows에서 완전 새로 시작**(/clear 아님, 새 머신). 이 문서 + `docs/design/v2-context-memory-direction_2026-06-30.md` + `context-notes.md` + claude-mem(`mem-search`) 읽고 시작.

## ① 현 상태 (한 줄)

v1 완료 + **v2 Plan 01~08 완성, 전부 origin/main 푸시됨.** 기본 `cargo test` 66 pass + 3 ignored, `--features morphology` 72 pass + 4 ignored. build/clippy 클린. 워킹트리 클린(`.omc/`만 untracked). 피처 브랜치 전부 머지·삭제.

## ② 이번 세션(맥) 한 일 — v2 Plan 01~08

| Plan | 내용 | 비고 |
|---|---|---|
| 01 idle watchdog | `src/runner/exec.rs` 공유 헬퍼 + `RunError::Timeout` + 양 러너 배선. 무출력 행 방지(600s) + stderr 동시 배수 | tunaFlow 패턴 + trailing-kill race 수정 |
| 02 N좌석 로스터 | `src/roster.rs` JSON 로스터 -> participants+registry, `--roster` 플래그 | 오케스트레이터 N-ready 활용 |
| 03 협업 코딩 | `@engine!` 쓰기 지목 -> RunMode::Write로 레포 편집. run_round에 mode 파라미터 | 결정: claude 현행 권한 / cwd / 확인 없음 |
| 04 session_bus | tunaSalon Redis session_bus 포팅(room->session) | tokio/redis/futures 신규 의존 |
| 05 세션 모델 | in-store 트리(Session messages+head), `/branches`·`/checkout` | parent_id 실사용, 저장 StoredSession |
| 06 Redis 통합 | `--observe`(관찰)·`--session`(재개) 멀티프로세스, 미러+owner lease | 라이브 검증됨. 종료 시 스냅샷 동기 flush 버그 수정 |
| 07 /debate | 사람 발화 1회 -> 에이전트 N턴 자동 교환 -> 복귀(기본 3, 최대 10) | run_round N회 재사용 |
| 08 한국어 토크나이저 | secall `tokenizer.rs` 포팅(Kiwi 메인+lindera 폴백, `morphology` feature) | **Kiwi 런타임 버그(아래 ④)** |

**라이브 검증(맥, 로컬 Redis):** 멀티세션 bus/observe/resume + 실 3라운드 컨텍스트 유지 전부 통과. 실 라운드로 버그 1건(종료 스냅샷 유실) 잡아 수정.

## ③ 북극성 (제품 방향)

**계층형 공유 맥락 + 능동 검색:** 에이전트가 서로 맥락을 능동적으로 기억·검색, 단기(세션)~프로젝트 모든 층. 핵심 전환 = **"전사 통째 재주입" -> "검색해 관련 슬라이스만 주입(RAG)"**(현재 `build_round_prompt`가 통째 재주입 = 스케일 병목).

**한국어 검색 정답 = secall 포팅**(재발명 금지). 형태소 선-토크나이즈 -> FTS5(unicode61)에 공백조인 저장("검색을"->"검색"), keep-tags NNG/NNP/NNB/VV/VA/SL(외국어). + BGE-M3 벡터 + 하이브리드(BM25+ANN). 출처: `~/privateProject/secall/crates/secall-core/src/search/`. 상세: `docs/design/v2-context-memory-direction_2026-06-30.md`, claude-mem `korean-search-port-secall`.

## ④ 검증된 사실 / 주의

- **임베딩 = 원격 Ollama(로컬 ORT 대체, 검증됨):** SSH `-p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@<host>` -> `POST localhost:11435/api/embed` `{"model":"bge-m3","input":...}` -> `{"embeddings":[[...]]}`. **bge-m3:latest 존재, 차원 1024.** Embedder=reqwest HTTP + MockEmbedder 폴백, 엔드포인트 설정값. (SSH 포트 **2232**, 22 아님. IP 비공개.)
- **⚠️ Kiwi 런타임 부트스트랩 버그:** kiwi-rs 0.1.4 **컴파일은 성공**하나 런타임에 libkiwi.dylib 로드 실패 + auto-download 404(`kiwi_mac_arm64_v0.23.2.tgz` 없음) -> **현재 lindera로 폴백 실효.** 사용자 선호는 Kiwi 메인(품질). 후속: kiwi-rs 버전 핀/libkiwi 수동 설치/upstream 확인.

## ⑤ Windows 이관 gotcha (새 머신)

0. **전역 설정 점검(1순위):** 레포엔 프로젝트 CLAUDE.md만 들어온다. **전역 `~/.claude/CLAUDE.md` + `@import`된 `~/.config/agents/COMMON.md`가 Windows엔 없거나 다를 수 있다.** 특히 맥 CLAUDE.md의 `@import`가 **절대 Mac 경로**라 Windows에선 COMMON.md(공통 행동 계약 전체)가 안 불러와진다. 스냅샷 [docs/reference/global-claude-config-snapshot_2026-06-30.md](../reference/global-claude-config-snapshot_2026-06-30.md)와 Windows 자기 설정을 비교·보완할 것.
1. **토크나이저:** Kiwi는 cfg로 **Windows 제외** -> Windows에선 **lindera만**(정상 경로). 즉 Windows에선 Kiwi 런타임 버그 무관. `cargo test --features morphology`로 lindera 검증.
2. **Redis 라이브 검증:** Windows엔 redis-server 기본 없음 -> WSL2 / Memurai / Docker 중 택. 평소 `cargo test`는 Redis 불필요(라이브 테스트만 `#[ignore]`).
3. **원격 Ollama 터널:** Windows OpenSSH도 동일 `ssh -N -p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@<host>`. (이 맥 세션 환경에선 됐음. Windows 네트워크에서 2232 도달 가능한지 확인.)
4. **claude/codex CLI:** 러너가 `Command::new("claude")`/`("codex")`로 spawn. Windows에서 실행파일 경로/`.cmd` 래핑 필요할 수 있음(tunaFlow `wrap_windows_script` 참고). 실 에이전트 스모크 전 확인.
5. **경로 구분자/홈:** state 파일·캐시 경로가 Windows 형식. `~/.cache/kiwi`는 Windows에선 다른 위치(어차피 Kiwi 제외라 무관).
6. **소스 레포 경로:** secall/tunaSalon/tunaFlow가 Windows에 있나 확인(포팅 출처 읽기 필요). 없으면 GitHub remote 확인.

## ⑥ 다음 로드맵 (검색 레이어, secall 포팅)

1. **(다음) SQLite 시스템 오브 레코드 + FTS5** — 전사/세션/메시지를 SQLite로(현재 JSON), `tokenize_for_fts`로 선-형태소화 저장. 검색 가능 영속의 토대. (rusqlite, FTS5)
2. **벡터** — 원격 Ollama bge-m3(dim 1024) reqwest Embedder + ANN(usearch 또는 단순 cosine). MockEmbedder 폴백.
3. **하이브리드 융합** — BM25(어휘) + 벡터(의미). secall `search/hybrid.rs` 참고.
4. **검색 주입(RAG)** — `build_round_prompt`를 통째 재주입 -> 최근 N턴 + 검색 슬라이스 + ctx-handle로. **북극성의 핵심.**
5. **에이전트 검색 도구** — recall/search_context(MCP/러너 경유, 능동 검색).

**백로그(결정 필요):** 분리 터미널 A2A 협업(MCP+버스, turn-triggering 난제 / 단 토론은 오케스트레이션+`/debate`로 이미 됨) · 리치 프론트(ratatui/web) · 신규 엔진 러너 좌석(tunaLlama/opencode).

## ⑦ 핵심 파일 지도

- 러너: `src/runner/{mod,exec,claude,codex}.rs` (Runner trait, watchdog, 쓰기 하드분리)
- 오케스트레이터: `src/orchestrator/{mod,roles,prompt}.rs` (run_round + mode, build_round_prompt=재주입 지점)
- REPL: `src/repl/mod.rs` (Command/parse/Session 트리/step: Message/Only/Write/Conclude/Branches/Checkout/Debate)
- 영속: `src/store/mod.rs` (StoredMessage 트리, StoredSession, path_to_root/next_id, save/load_session)
- 멀티세션: `src/session_bus.rs` (RedisBus 6+함수 + snapshot, RedisBusHandle fire-and-forget), `src/main.rs`(tokio 런타임 + --observe/--session)
- 로스터: `src/roster.rs`, 검색: `src/search/{mod,tokenizer.rs}`(morphology feature)
- 출처 레포: secall(검색 정본)/tunaSalon(경량 lift)/tunaFlow(vector 원형)

## ⑧ 검증 / 규율

- 검증: `cargo test`(기본) + `cargo test --features morphology` + `cargo build` + `cargo clippy --all-targets`(둘 다 feature 조합). 검증과 commit/push 분리.
- 규율: docs/reference/development-guidelines.md. #5 한국어 마침표, #6 새 파일 헤더 주석, #7 plan+checklist+context-notes. 구현=Sonnet 서브에이전트, Opus 리뷰. push는 사용자 요청 시(이번 세션은 요청받아 푸시 중). 굵직한 결정 재론 금지(claude-mem `no-relitigating-decisions`).

## ⑨ 다음 세션 첫 프롬프트 (복붙용)

> tunaRound v2 Plan 01~08 맥에서 완성(전부 origin/main). 이제 Windows에서 이어간다. `docs/prompts/v2-windows-handoff_2026-06-30.md` + `docs/design/v2-context-memory-direction_2026-06-30.md` + `context-notes.md` 읽고, claude-mem `mem-search`로 과거 결정 확인해줘. `cargo test`(기본) + `cargo test --features morphology`로 현 상태 확인. 그다음 북극성(계층형 맥락+능동 검색)의 다음 스텝 = **SQLite 시스템 오브 레코드 + FTS5(선-형태소화 저장)**부터 plan으로 착수. Kiwi는 Windows cfg 제외라 lindera 경로로 간다. 임베딩은 원격 Ollama(SSH -p [사설포트] 터널, bge-m3 dim 1024). 재론 금지(결정은 design 문서/claude-mem에).
