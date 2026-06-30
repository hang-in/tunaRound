---
title: tunaRound v2 핸드오프 (Windows 세션 2 종료, 2026-06-30)
type: prompt
status: active
priority: P0
updated_at: 2026-06-30
owner: shared
summary: Windows 세션에서 v2 검색/맥락 스택을 Plan 09~19로 완성(SQLite FTS+RAG+/search+벡터/하이브리드+에이전트 MCP+Windows 러너+재주입 축소+HTTP 엔진 러너+FTS 리콜 보강+Kiwi 활성화). 전부 origin/main(7ce7575) 푸시. 핵심 셋업=Kiwi v0.22.2 수동 libkiwi, Ollama 터널 11435(SSH -p [사설포트]), Redis 6379. 검색 품질은 측정됨(평범, 게이지 tests/search_quality.rs). 남은 항목=ctx-handle/요약, opencode CLI 참가자, 검색 품질 추가 개선, 리치 프론트(보류).
---

# tunaRound v2 핸드오프 (Windows 세션 2 종료, 2026-06-30)

> 직전 핸드오프(Mac→Windows): [v2-windows-handoff_2026-06-30.md](v2-windows-handoff_2026-06-30.md). 이 문서가 그 이후(Windows에서 Plan 09~19)를 잇는다. 진행 현황: [docs/plans/index.md](../plans/index.md), 작업 추적: [checklist.md](../../checklist.md), 결정/맥락: [context-notes.md](../../context-notes.md).

## ① 현 상태 (한 줄)

**v1 + v2 Plan 01~20 완성, 전부 origin/main 푸시·동기화.** 검증: `cargo test`(기본=morphology+sqlite) 105 + `--features "semantic morphology mcp engines"` 112 pass, clippy 전 조합 클린. 워킹트리 클린(`.omc/`만 untracked).

## ② 이번 세션(Windows) 한 일 — Plan 09~19 + gotcha #4

| Plan | 내용 | 커밋 |
|---|---|---|
| 09 | SQLite 시스템오브레코드 + FTS5(선-형태소화) | c61cf11+181f46a |
| 10 | SQLite 라이브 배선(MessageIndexer, append_round 훅, --db) | e21cf43+5d79a0a |
| 11 | 검색 주입(RAG): ContextRetriever + Session 검색·dedup | b0dd7bd+4643977 |
| 12 | /search 명령(사람이 인덱스 직접 검색) | bc2f359 |
| 13 | 벡터 임베딩(bge-m3) + 하이브리드(RRF) | 1ad8881+30efa51+8920027 |
| 14 | 에이전트 능동 검색 MCP(rmcp search_context, claude+codex 배선) | a65feba+a5a185d+c892548 |
| 15 | gotcha #4: 러너 Windows CLI 해석(codex.cmd spawn) | 8d02088 |
| 16 | 재주입 축소(--recent-turns, opt-in) | 2834a1d |
| 17 | OpenAI 호환 HTTP 엔진 러너(ollama/lmstudio/openai) | e1373f9 |
| 18 | FTS 리콜 보강(raw 토큰 + prefix 질의) | 45cf0c8 |
| 19 | Windows Kiwi 활성화(v0.22.2 수동 libkiwi) | fe0ec71 |
| 20 | opencode CLI 엔진 러너(로스터 engine "opencode") | 7fedac2 |

**라이브 검증:** 실 claude+codex 1라운드 정상 응답(Windows) → SQLite 색인 → MCP search_context가 실 색인 발언 반환. Ollama /v1/chat/completions(gemma4) + /api/embed(bge-m3 dim 1024) 응답 확인. Kiwi v0.22.2 tokenize 동작 확인.

## ③ 핵심 셋업 / gotcha (다음 세션 필수)

- **Kiwi(한국어 고품질 토크나이저) = v0.22.2 수동 설치.** kiwi-rs 0.1.4 auto-download가 깨졌고(토큰 무관), **latest Kiwi v0.23.2는 ABI 불일치로 native crash. v0.23.2 쓰지 말 것.** 설치: `scripts/install-kiwi-windows.sh`(gh로 v0.22.2 win_x64 + base model 받아 `%LOCALAPPDATA%\kiwi\{lib\kiwi.dll, models\cong\base}`에 추출). discovery 기본경로라 env 불필요. **미설치 시 lindera 자동 폴백.** 문서: docs/reference/kiwi-windows-setup.md. (이 머신엔 이미 설치됨.)
- **임베딩/LLM 백엔드 = Ollama 터널 11435.** `ssh -N -p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@[사설IP]`(호스트 이제 공개). `semantic` 피처 + HTTP 엔진 러너가 사용. `TUNAROUND_OLLAMA_URL` 기본 `http://127.0.0.1:11435`. (현재 터널 떠 있음.)
- **Redis 6379** = 멀티세션(--observe/--session). 현재 떠 있음. `TUNAROUND_REDIS_URL`.
- **feature 기본값:** default=`morphology + sqlite`(검색 기본 동작). `semantic`(reqwest/Ollama)·`mcp`(rmcp)·`engines`(HTTP 러너)는 opt-in. 가벼운 빌드 `--no-default-features`.
- **cargo는 Bash 툴로 실행**(Git Bash sh 있어 exec.rs sh 테스트 통과; PowerShell이면 거짓 실패 2건).
- **gotcha #4 해결됨:** 러너가 Windows에서 `.cmd`(codex.cmd 등) spawn 가능(resolve_bin).

## ④ 검색 품질 (측정됨, 솔직)

- 게이지 `tests/search_quality.rs`(#[ignore], 통제 코퍼스+Ollama). 실행: `cargo test --features "semantic morphology" --test search_quality -- --ignored --nocapture`.
- 결과: **형태소 굴절 OK**("인증을"→"인증"), **외래어 누락은 Plan 18 raw+prefix로 메움**("임베딩"). **벡터는 소규모 코퍼스에서 노이즈 큼**(진짜 동의어 약함). 즉 기계는 동작·품질은 평범. 현실 코퍼스(긴 발언 다수) 재측정이 다음 판단 근거.

## ⑤ 남은 항목 (우선순위 제안)

1. **검색 품질 추가 개선** — 현실 코퍼스로 벡터/하이브리드 재측정 → 필요시 RRF 튜닝/쿼리확장/재랭킹. (제품 크럭스, 현재 "평범".)
2. **요약 carry-forward** — 재주입 축소(Plan 16)에서 잘린 맥락의 러닝 요약. 단 "온디맨드 확장"은 이미 MCP search_context(Plan 14)가 커버 → 남는 건 요약 주입뿐. enhancement.
3. **예시 로스터 확장** — examples/roster.json에 ollama-cloud/opencode 좌석 예시(작음, 신규 엔진 사용성).
4. **코어-백엔드 + 에이전트-클라이언트(A2A)** — ⑧-A 방향. 큰 포크, 별도 설계 세션.
5. **리치 프론트(ratatui)** — 보류. 페인(분기트리/observe/맥락 투명성) 입증 시 경량 TUI.
6. **"좌석" 잔존 정리** — 코드 식별자(SeatConfig)는 유지, Korean prose 문서의 "좌석"→"참가자" 일괄(저가치).
- (done) opencode CLI 참가자 = Plan 20. HTTP 엔진 = Plan 17.

## ⑥ 검증 / 규율

- 검증: `cargo test`(기본) + `--features "semantic morphology mcp"` + `--features engines` + `cargo build --no-default-features` + clippy 각 조합. **Bash 툴로.**
- 규율(docs/reference/development-guidelines.md): #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, #7 plan+checklist+context-notes. **구현 위임=Sonnet 서브에이전트, Opus 스펙·리뷰·검증.** 검증/commit/push 분리(push는 사용자 요청 시). 굵직한 결정 재론 금지.
- **서브에이전트 레이스 주의:** 서브에이전트가 `git add -A`로 커밋하므로, 그 진행 중엔 추적 문서 등 다른 파일 편집/커밋 자제(완료 후).

## ⑦ 출처 레포 (포팅 시) — D 드라이브

- secall: `D:/privateProject/seCall/crates/secall-core/src/{search,store,mcp}/` (FTS/벡터/하이브리드/rmcp 정본)
- tunaSalon: `D:/privateProject/tunaSalon` (Redis/lindera/embed 경량)
- tunaFlow: `D:/privateProject/tunaFlow` (CLI 러너)
- kiwi-rs 소스: `D:/.cargo/registry/src/.../kiwi-rs-0.1.4` (bootstrap/discovery - Kiwi 경로 규칙)

## ⑧-A 검토할 아키텍처 방향: 코어-백엔드 + 에이전트-클라이언트 (사용자 제기 2026-06-30)

> 사용자 질문: "지금 cargo run이 `-p` 모드로 매 라운드 에이전트를 stateless spawn하는데, **코어(오케스트레이션+검색/메모리)를 백엔드 서비스로 상주**시키고 claude code·codex·ollama/openai 클라이언트가 **접속해서 쓰는** 게 낫지 않나?"

- **현재:** tunaRound가 코어를 들고 매 라운드 에이전트를 stateless spawn(`-p`/`exec`/HTTP). 사람↔tunaRound 대화. 분산 turn-triggering 난제를 우회한 선택.
- **이미 깔린 씨앗:** `--mcp-search`(Plan 14) = 코어의 **검색/메모리를 백엔드로 노출, 에이전트가 MCP 클라이언트로 사용**. 제안은 이를 **오케스트레이션까지 확장**.
- **장점:** 영속 코어 공유(멀티 프론트, Redis 멀티세션이 부분 시작) · 에이전트 자체 세션 유지로 재전송↓ · 모델/클라이언트 플러그인.
- **난점(핵심):** 분산 turn-triggering(누가 언제·앞 발언 순차 노출을 백엔드가 조정) = CLAUDE.md A2A 백로그 난제. + 컨텍스트 통제(재주입축소/RAG의 "그 턴에 뭘 보일지" 정밀 통제) 약화 위험.
- **권고:** 두 모델 공존. 현재(사람주도 순차, 동작함) 유지 + **점진**: MCP 서버에 검색 외 오케스트레이션 툴(`read_transcript`/`post_turn` 등) 추가 → 코어-백엔드 A2A로 성장. **큰 포크라 별도 설계 세션 필요**(지금 구현 X). 관련 백로그: 분리터미널 A2A.

## ⑧ 다음 세션 첫 행동

1. 이 문서 + context-notes.md + checklist.md + docs/plans/index.md 읽기. `cargo test`(기본) + `--features "semantic morphology mcp"`로 상태 확인(Bash 툴).
2. 백엔드 확인: 11435(Ollama 터널)·6379(Redis) 떠 있나. Kiwi는 `%LOCALAPPDATA%\kiwi` 설치돼 있나(없으면 lindera).
3. 남은 항목(⑤) 중 사용자 지정으로 착수. plan+checklist+notes(규율 #7), 위임 Sonnet + Opus 리뷰.
