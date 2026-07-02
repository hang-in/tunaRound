# Reference — SSOT

> 현재 기준 사실. 코드와 함께 in-place 갱신. 날짜 별 복제 금지.

| 문서 | 영역 | ssot_level |
|---|---|---|
| [development-guidelines.md](development-guidelines.md) | 개발 행동 규율(이 프로젝트 실험 적용) | canonical |
| [global-claude-config-snapshot_2026-06-30.md](global-claude-config-snapshot_2026-06-30.md) | 전역 Claude/COMMON 설정 스냅샷(맥) — Windows 비교용 | snapshot(시점, non-canonical) |
| [dev-mac-windows.md](dev-mac-windows.md) | 맥↔윈도우 왕복 개발 가이드 | 상시참조 |
| [kiwi-windows-setup.md](kiwi-windows-setup.md) | Kiwi 윈도우 설치 | 상시참조 |
| [release-readiness-v0.1.0_2026-07-02.md](release-readiness-v0.1.0_2026-07-02.md) | v0.1.0 릴리스 준비(도그푸딩+맥검증+체크리스트) | snapshot(시점) |

## 갱신 정책

- in-place 갱신 (`updated_at` 메타만)
- 큰 정책 변경 시 `supersedes`/`superseded_by` 로 archive 후 신 reference 작성
- 시점성 분석 보고서는 `<name>_<YYYY-MM-DD>.md` 형식 허용 (영구 reference 와 구분)
