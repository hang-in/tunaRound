# Changelog

이 프로젝트의 주요 변경을 기록합니다. 형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고, [Semantic Versioning](https://semver.org/lang/ko/)을 지향합니다.

## [Unreleased]

## [0.1.0] - 미발행

첫 공개 릴리스(도그푸딩 후 태그 예정). 터미널에서 사람이 운전하는 역할 부여 2-에이전트(Claude Code · Codex) 착수 전 설계 토론 도구.

### 추가 (Added)

- **토론 코어**: 역할 주입 + 순차-인지 라운드(`run_round`), thin REPL(`chat`). 러너는 Codex·Claude(공통 `Runner` trait, 읽기/쓰기 하드 분리).
- **REPL 커맨드**: `@engine`(자리 지목) · `@engine!`(쓰기 턴, 협업 코딩) · `/debate <n>`(N턴 자동 교환) · `/conclude`(종합) · `/branches`·`/checkout`(분기 트리) · `/save` · `/supersede`·`/reject`·`/explain`(유효성·검색 디버그).
- **영속·세션**: SQLite 시스템 오브 레코드(스키마 v5, `created_at`) + in-store 트리(브랜치=세션). 멀티세션 관찰/재개(Redis, 선택).
- **한국어 검색/맥락**: 형태소 FTS(Kiwi 메인 + lindera 폴백, POS keep-tags) + 외래어 음역 병기 색인 + 벡터 RRF + RAG 주입 + MCP 능동검색(`search_context`/`read_transcript`) + 유효성·세션·recency 인지 랭킹.
- **half-A2A 코어 백엔드**: `core`(단일 프로세스 REPL+in-process HTTP MCP) · `serve`(헤드리스 코어) · `join`(원격 코어 접속). `post_turn`/`get_roster` + core-sync(증분 append, DB id 권위). bearer 인증.
- **온보딩·배포**: clap 서브커맨드(`chat`/`core`/`serve`/`join`/`mcp-search`/`reindex`) · `tunaround.toml` 프로파일 · cargo-dist(6타깃, homebrew/shell/powershell 인스톨러).
- **임베딩**: 원격 Ollama HTTP(기본 `qwen3-embedding:0.6b`, dim 1024, `TUNAROUND_EMBED_MODEL`로 교체) + 결정적 MockEmbedder 폴백.

### 알려진 제약 (Known limitations)

- Kiwi 네이티브 라이브러리·모델은 첫 실행 시 [bab2min/Kiwi](https://github.com/bab2min/Kiwi) 릴리스에서 자동 다운로드하며, 실패하면 lindera로 자동 폴백합니다(검색은 동작, 품질만 소폭 차이). macOS(aarch64)에선 현재 자산 태그 이슈로 lindera 폴백 상태입니다.
- 의미검색(semantic)은 Ollama 엔드포인트(기본 `http://127.0.0.1:11435`)가 있어야 하며, 없으면 FTS 단독으로 폴백합니다.
- codex 좌석의 원격 MCP pull은 환경에 따라 불안정할 수 있습니다(claude pull은 안정). read-only는 behavioral로 보장합니다.

### 라이선스

- AGPL-3.0-only.

[Unreleased]: https://github.com/hang-in/tunaRound/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0
