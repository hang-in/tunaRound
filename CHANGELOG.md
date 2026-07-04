# Changelog

이 프로젝트의 주요 변경을 기록합니다. 형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고, [Semantic Versioning](https://semver.org/lang/ko/)을 지향합니다.

## [Unreleased]

### 변경 (Changed)

- 릴리스 바이너리(cargo-dist)에 `worker`/`engines`/`a2a-out` 피처 포함. 이제 설치본 하나로 코어(`serve`)뿐 아니라 워커 노드(`tunaround node`/`poll`/`work`), http 러너(`--runner http`), 외부 표준 A2A 위임(`--runner a2a`)까지 동작한다. (v0.2.0 프리빌트는 `serve`만 있어 워커 서브커맨드가 없었다.)

## [0.2.0] - 2026-07-04

### 추가 (Added)

- **semi-A2A 파트너 위임(Phase 1)**: A2A Task 데이터모델(Task/TaskState/Message/Part/Artifact, `tasks` 테이블 스키마 v6) + JSON-RPC 엔드포인트(`SendMessage`/`GetTask`/`CancelTask`, `/.well-known/agent-card.json`) + inbox MCP 도구(`poll_tasks`/`claim_task`/`complete_task`/`fail_task`) + dispatcher 도구(`send_task`/`get_task`). 크로스머신 왕복 실증.
- **A2A 스트리밍(Phase 2, SSE)**: `SendStreamingMessage`/`SubscribeToTask` + store 이벤트 버스. Agent Card `capabilities.streaming=true` 광고.
- **A2A 자율 워커 데몬**: `tunaround work`(poll->claim->러너 실행->complete 자율 루프). 러너 교체로 이기종 파트너(`--runner claude|codex|opencode|http|a2a`). `context_id` 프로젝트 라우팅(`--context-map`). 러너 실패 시 `fail_task` 전이(completed 위장 안 함).
- **A2A outbound 러너**: `--runner a2a`로 외부 표준 A2A 에이전트에 표준 위임(a2a-client, `a2a-out` 피처).
- **워커 노드 온보딩**: `tunaround init`/`node`/`doctor` + `NodeConfig`(config 1개 + 데몬 하나 = 워커 노드). 감시 전용 `tunaround poll`(감독 레인 유휴 0토큰 wake).
- **브로커 거버넌스**: 네이밍 규약(`to_agent`는 워커만) · Agent Card `buildFeatures` 능력 광고 · 미배달/고착 표시(`⚠no-consumer?`/`⚠stuck?`) + 전역 조망 `tasks` 도구 · write 워커 self-disruption 방지 가드레일.

### 변경 (Changed)

- **개발 방법론**: GitHub Flow + PR CI(3-OS 매트릭스: ubuntu/macOS/windows, build+test+clippy) 도입.
- **리팩토링(리뷰 기반 R1-R10)**: MCP 에러 계약 정직화(실패를 isError로) · A2A 상태머신 조건부 전이(이중 claim/종료 덮어쓰기 차단) · watchdog 프로세스 트리 종료 · retriever/reader Result 계약 · Embedder dim 동적화 등.
- **저장소 공개**: `hang-in/tunaRound` PUBLIC 전환(히스토리 시크릿 퍼지).

### 고침 (Fixed)

- 워커 MCP 세션 만료 시 자동 재연결(404 -> handshake 재수행).
- `watchdog`의 Unix 프로세스 그룹 종료 이식성(외부 `kill -9 -PID` no-op -> `libc::kill`).

## [0.1.0] - 2026-07-02

첫 공개 릴리스(도그푸딩 후 태그 예정). 터미널에서 사람이 운전하는 역할 부여 2-에이전트(Claude Code · Codex) 착수 전 설계 토론 도구.

### 추가 (Added)

- **토론 코어**: 역할 주입 + 순차-인지 라운드(`run_round`), thin REPL(`chat`). 러너는 Codex·Claude(공통 `Runner` trait, 읽기/쓰기 하드 분리).
- **REPL 커맨드**: `@engine`(자리 지목) · `@engine!`(쓰기 턴, 협업 코딩) · `/debate <n>`(N턴 자동 교환) · `/conclude`(종합) · `/branches`·`/checkout`(분기 트리) · `/save` · `/supersede`·`/reject`·`/explain`(유효성·검색 디버그).
- **영속·세션**: SQLite 시스템 오브 레코드(스키마 v5, `created_at`) + in-store 트리(브랜치=세션). 멀티세션 관찰/재개(Redis, 선택).
- **한국어 검색/맥락**: 형태소 FTS(Kiwi 메인 + lindera 폴백, POS keep-tags) + 외래어 음역 병기 색인 + 벡터 RRF + RAG 주입 + MCP 능동검색(`search_context`/`read_transcript`) + 유효성·세션·recency 인지 랭킹.
- **semi-A2A 코어 백엔드**: `core`(단일 프로세스 REPL+in-process HTTP MCP) · `serve`(헤드리스 코어) · `join`(원격 코어 접속). `post_turn`/`get_roster` + core-sync(증분 append, DB id 권위). bearer 인증.
- **온보딩·배포**: clap 서브커맨드(`chat`/`core`/`serve`/`join`/`mcp-search`/`reindex`) · `tunaround.toml` 프로파일 · cargo-dist(6타깃, homebrew/shell/powershell 인스톨러).
- **임베딩**: 원격 Ollama HTTP(기본 `qwen3-embedding:0.6b`, dim 1024, `TUNAROUND_EMBED_MODEL`로 교체) + 결정적 MockEmbedder 폴백.

### 알려진 제약 (Known limitations)

- Kiwi 네이티브 라이브러리·모델은 첫 실행 시 [bab2min/Kiwi](https://github.com/bab2min/Kiwi) 릴리스에서 자동 다운로드하며, 실패하면 lindera로 자동 폴백합니다(검색은 동작, 품질만 소폭 차이). macOS(aarch64)에선 현재 자산 태그 이슈로 lindera 폴백 상태입니다.
- 의미검색(semantic)은 Ollama 엔드포인트(기본 `http://127.0.0.1:11435`)가 있어야 하며, 없으면 FTS 단독으로 폴백합니다.
- codex 좌석의 원격 MCP pull은 환경에 따라 불안정할 수 있습니다(claude pull은 안정). read-only는 behavioral로 보장합니다.

### 라이선스

- AGPL-3.0-only.

[Unreleased]: https://github.com/hang-in/tunaRound/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.0
[0.1.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0
