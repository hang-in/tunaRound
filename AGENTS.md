# tunaRound — AGENTS.md

> 본 파일은 codex/gemini CLI 가 인식하는 시스템 지시 파일. Claude Code 는 `CLAUDE.md` 사용.

<!-- BEGIN tunaDocs:scaffold -->
이 영역은 tunaDocs 가 갱신. 사용자 customize 는 아래 sentinel 안에서만 진행.
<!-- END tunaDocs:scaffold -->

## 1. 프로젝트

터미널에서 사용자가 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론 도구. Rust+tokio.

## 2. 핵심 규칙

- 답변 언어: 한국어 (사용자 환경 디폴트)
- 코드/식별자/경로: 원문 유지
- 신규 plan/prompt: 7필드 frontmatter 필수
- 사용자 customize 영역(`<!-- BEGIN user-customize -->`): 절대 덮어쓰기 X
- 개발 행동 규율(실험): `docs/reference/development-guidelines.md`

## 3. 문서 위치

- `docs/index.md` — 전체 진입점
- `docs/design/` — 설계 spec (현행 `tunaRound-v1-design_2026-06-29.md`)
- `docs/plans/` — 진행 중 plan
- `docs/reference/` — SSOT

## 4. 위임 패턴

- 구현(정확성 민감): Sonnet 서브에이전트 (codex 비사용)
- 벌크/초안(보일러플레이트·문서 초안·테스트 스캐폴드): tunaLlama `tuna_generate_code`/`tuna_refactor_code` 후 Opus 리뷰
- 스펙·리뷰·검증: Opus(메인)
- 상세: `.tuna-docs/routing.json`

<!-- BEGIN user-customize -->
<!-- 프로젝트별 사용자 customize. -->
<!-- END user-customize -->
