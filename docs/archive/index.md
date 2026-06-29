# Archive

> 더 이상 현재 기준이 아닌 문서. 기록 보존용. 현재 구현 기준 판단에는 우선 사용 X.

## 구조

```
archive/
├── plans/
│   ├── completed/    # 구현 반영 완료
│   └── deferred/     # 보류 (P2+)
├── handoffs/         # 과거 세션/환경 핸드오프
└── prompts/
    ├── by-date/      # 날짜별 일회성 prompt
    └── one-time/     # 완료된 plan 의 일회성 실행 prompt
```

## 검색 팁

- 완료된 plan: `archive/plans/completed/` 에서 `git log` 결합
- supersede 관계: 신 reference 의 `supersedes` frontmatter 가 archive 위치 가리킴
