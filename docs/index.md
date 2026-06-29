# tunaRound — Documentation Index

> 터미널에서 사용자가 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론 도구. Rust+tokio.

| 폴더 | 역할 |
|---|---|
| [plans/](plans/) | 진행 중 plan (active + partial). 완료는 archive 로 이동 |
| [reference/](reference/) | SSOT — 데이터 모델, 규칙, 컨벤션, 용어집 |
| [prompts/](prompts/) | Developer handoff, 재사용 prompt |
| [archive/](archive/) | 완료/보류 plan, 구버전 reference |
| [design/](design/) | 설계 spec (tunaDocs 표준 외 프로젝트 폴더). 현행 spec = `design/tunaRound-v1-design_2026-06-29.md` |

## 추천 읽기 순서

새 세션 시.

1. `CLAUDE.md` — 사용자 글로벌 + 프로젝트 지시
2. `design/tunaRound-v1-design_2026-06-29.md` — 현행 v1 설계 spec
3. `plans/index.md` — 진행 중 plan 한 줄 확인
4. 현재 작업 관련 plan 1개 + paired prompt(있으면)
5. 필요 시 reference 1~2개

## 문서 메타 규칙

신규 plan/prompt 는 다음 7필드 frontmatter 필수.

```yaml
---
title: ...
type: plan | prompt | reference | how-to | archive
status: draft | in_progress | partial | done | archived
priority: P0 | P1 | P2 | P3      # plan 만 필수
updated_at: YYYY-MM-DD
owner: claude | human | shared
summary: 한두 줄
---
```
