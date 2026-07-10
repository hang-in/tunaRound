# v2-44 T1+T2 구현 계획 (presence 스캐너 + role 개편 + 토큰 위생)

> 설계 정본 [v2-44](../design/v2-44-presence-scanner-and-roles_2026-07-11.md). T1(코드)+T2(레포 내 훅·스크립트)를 한 PR로 묶는다(훅 다이어트는 스캐너 전제 = 머지 원자성). 라이브 기동·재태깅·전역 훅 진단은 머지 후 ops.

## MVP 스코프 조정 2건 (설계 대비, 근거 기록)

1. **프로세스 교차 확인 = 이름 카운트만.** per-session 매핑(sysinfo cwd)은 신규 dep + 플랫폼 편차 리스크라 후속. MVP = `tasklist`/`ps` 셸아웃으로 러너별 프로세스 수를 세고, **0개면 그 러너 세션 전부 제거**(전원 종료·재부팅 즉시 반영). 개별 크래시 유령은 신선도 창(기본 240분)으로 상한.
2. **SessionEnd 훅은 deregister 핑 1줄만 잔존**(설계 "제거"에서 조정). 깨끗한 종료를 ±15초 스캔 대기 없이 즉시 반영 + 프로세스 매핑 부재를 보완. poll-kill·pidfile·리핑 로직은 전부 제거(그게 유령의 근원이었음).

## 커밋 단위

- **C1 데이터층**: `normalize_legacy_tags`(supervised→infra, register+selector 양쪽) + `sync_presence`(machine 단위 일괄 upsert+제거, `src=scan` 소유 태그로 수동 등록과 격리, human_input_at 보존) + 테스트.
- **C2 API층**: MCP 도구 `report_presence` + params + `McpHttpClient::report_presence` + instructions 문자열 갱신.
- **C3 스캐너**: `src/presence_scan.rs`(claude=discover 재사용 / codex=`~/.codex/sessions` rollout 스캔 / 프로세스 카운트 게이트) + `presence-scan` 서브커맨드(15초 루프, --once).
- **C4 task CLI**: `tunaround task poll|claim|get|complete|fail`(W3, MCP 미로드 세션의 0토큰 경로). `--result -`=stdin.
- **C5 watch-results --digest**: failed=즉시 / completed=구간 묶음(W5, 기본 0=현행).
- **C6 훅 다이어트**: autoarm=안내 5줄+마커 1회(무장 로직 삭제) / ping=human-ping만(ensure_armed 삭제) / disarm=deregister만 / tuna_arm=cfg·sanitize 등 공유 유틸만 잔존 / scripts/codex 삭제.
- **C7 문서**: 설계 구현노트 + a2a-usage §9 갱신 + checklist.

## 검증

- `cargo test --features "morphology mcp serve worker"` 전체 green + clippy.
- 라이브 스모크(T2 ops 전 로컬): `presence-scan --once` → roster에 이 머신 세션들 upsert / 없는 세션 제거 확인. `task poll/claim/complete` 왕복. `--digest 30` 동작.

## T2 ops (머지 후, PR 밖)

안정 바이너리 재빌드·배포 → presence-scan 데몬 기동(win) → 구 detached poll 전량 정리 → win-codex-sup를 `role=infra,purpose=codex-inject`(project 제거)로 재기동 → thread 로테이션 설정 → 전역 훅 이중 등록 진단(W1·W6) → mac은 A2A task로 위임(T3).
