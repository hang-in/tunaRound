# tunaRound - Claude Code Handoff

> 이 파일은 다음 세션이 이어가기 위한 핸드오프입니다. 제품/설계 전모는 [docs/design/tunaRound-v1-design.md](docs/design/tunaRound-v1-design.md)(승인된 spec).

## 표기 / 작업 규칙 (tuna 생태계 공통)

- 사용자 응답·문서는 **한국어 존댓말**. **em-dash 사용금지**(일반 대시 `-` 또는 콜론 `:`). ANSI 박스 드로잉 자제.
- 도메인 도착 URL/도메인은 비노출(소스공개, 서비스 비공개).
- 구현 위임은 **Sonnet 서브에이전트**(codex 비사용), Opus가 스펙·리뷰·검증.
- 한 세션 한 목적. 검증(build/test)과 commit/push는 분리.

## 현재 상태 (2026-06-29)

- **설계 단계.** brainstorming 완료 + spec 승인. 앱 코드 없음(레포는 docs만).
- 다음 단계: **writing-plans 스킬로 구현 플랜 작성** -> 레이어별 task 분해 -> 구현.

## 무엇을 만드나 (요약)

터미널에서 **Codex CLI ↔ Claude Code가 구조 라운드로 토론**하고 **수렴하면 결론**짓는 앱. 멀티세션은 Redis. 최종 목표는 협업 코딩, v1은 토론 substrate 증명.

**레이어(출처):** 에이전트 러너(tunaFlow `agents/` 포팅) + 토론 진행/Roundtable(tunaFlow 포팅) + 수렴 감지(tunaSalon `flow.rs` 차용) + Redis 멀티세션(tunaSalon `session_bus.rs` 포팅) + 영속(SQLite) + 터미널 UI(ratatui).

**핵심 결정:** 흐름엔진은 수렴 감지만(Hawkes 리듬 미사용) / Redis = 동시세션+멀티관찰+재개(분산 아님) / 에이전트 = 실제 CLI / 스택 = Rust+tokio+ratatui+redis+rusqlite.

**v1 비목표:** 실제 코드 실행/공유 작업공간(=v2 협업 코딩), 분산 에이전트, N>2, 웹/GUI.

## 출처 레포 (포팅 시 읽기)

- tunaFlow: `~/privateProject/tunaFlow` - `src-tauri/src/agents/{claude,codex,codex_app_server}.rs`, Roundtable(`docs/reference/architecture-detail.md`).
- tunaSalon: `~/privateProject/tunaSalon` - `src/flow.rs`(FlowMeter), `src/session_bus.rs`(redis-bus), `roomstore.rs`/`tui.rs`/`chat.rs`(영속·TUI 패턴).

## 다음 세션 첫 행동

1. [docs/design/tunaRound-v1-design.md](docs/design/tunaRound-v1-design.md) 정독.
2. 출처 레포의 포팅 대상(특히 tunaFlow agents 러너, tunaSalon session_bus/flow) 실측.
3. writing-plans로 v1 구현 플랜 작성(§7 리스크 반영: CLI 스트림 스키마 견고화, 턴 컨텍스트 조립, 수렴 임계값, Redis 스키마 매핑).
