# 세션23 핸드오프 (2026-07-12) - v2-47 마무리 + 문서 리프레시 + lease/cancel + init 온보딩 + v0.4.0 릴리즈

> 진입점: 이 파일 먼저 → 다음 목표 정본 [v2-48 opencode](../design/v2-48-opencode-wiring_2026-07-11.md)(감독 레인, 착수 시 §1 재대조 필수). 이전 세션: [세션22](v2-handoff_2026-07-12_session22.md).

## 한 줄 요약

세션22가 남긴 "방향 선택"에서 사용자가 순차로 지시: v2-47 #3 후속 → 잔여 3건 → README/문서 → Codex 제안 검토 → 설치 단순화 → 릴리즈. **6개 PR(#70~#75) 머지 + 브랜치 정리 + v0.4.0 릴리즈 + WMI 배포 + 맥 알림**까지 완주. main=`6c78dbe`(v0.4.0), 미커밋 없음, 열린 브랜치 없음(미머지 잔여 = `origin/feat/session17-roster-supervision` 하나, 삭제 권고).

## 완료 사항 (전부 머지·CI green·봇리뷰 반영)

| PR | 머지 SHA | 항목 |
|----|---------|------|
| #70 | `5fdfb4b` | **v2-47 #3 후속**: 브로커 헬스 패널에 uptime(broker_started_at config) + WAL 크기. store db_path 필드 + get_config/set_config/wal_bytes(마이그레이션 불요=config 테이블 재사용). fail-visible 유지 |
| #71 | `56221c7` | **v2-48 opencode 워커 현황 정정**: 워커 러너는 이미 완성(커밋 7fedac2)·opencode v1.17.18·CLI 드리프트 0 확인. fixture 타임아웃 테스트 추가 + 문서 정정. 감독 레인은 R2로 defer |
| #72 | `1ecaa8b` | **v2-47 낮은우선**: ★ recency 표시(비-총괄 "★ 마지막 N분 전") + 관전 뱃지 eye 아이콘 + 모바일 반응형 @media(≤640px). 봇 오버플로 지적 2건 반영(.rst-title·.rst-runner-name 말줄임) |
| #73 | `7bca39a` | **README 리프레시**(650→171줄, 개인악기 톤, 완료목록→CHANGELOG) + 신설 `docs/reference/onboarding.md`·`mesh-architecture.md`. 봇 3건 반영 |
| #74 | `238eb96` | **task lease 자동연장(#6)** + **cancel_task MCP/CLI(#4)**. 워커가 러너 실행 중 select!로 lease 주기 연장(장기 task requeue 방지) + MAX_LEASE_EXTENSIONS 상한(고착 회수) + MissedTickBehavior::Skip. 적대 리뷰 2 + gemini 1 반영 |
| #75 | `bda5542` | **init 원커맨드 온보딩**: node.toml + ~/.tunaround/config 동시 스캐폴드(--machine, unix 0600), 토큰 env 통일(TUNAROUND_TOKEN→TUNA_BROKER_TOKEN) + **AI 설치 안내**(docs/prompts/install-with-ai.md). 봇 3건 반영(0600·문구·히스토리) |

**브랜치 정리(③)**: 머지분 로컬 16·원격 다수 삭제(fetch --prune). session16 핸드오프 문서를 main 편입(`2e65e12`, CLAUDE.md dangling 참조 해소) 후 브랜치 삭제. **미머지 잔여 = `origin/feat/session17-roster-supervision`**(v2-41 superseded, 비가역 유실 위험이라 단독삭제 안 함 → **다음 세션에 삭제 권고**: `git push origin --delete feat/session17-roster-supervision`).

## v0.4.0 릴리즈 (2026-07-12)

- **`cargo release minor --execute`**: 0.3.0→0.4.0, CHANGELOG [Unreleased]→[0.4.0] 확정(v2-42~v2-49 전부), 커밋 `6c78dbe` + **태그 v0.4.0** push → **cargo-dist 릴리즈 워크플로우 발화**(run 29171411699, shell/powershell/homebrew + 4타깃).
- ⚠ **다음 세션 첫 확인**: 릴리즈 CI(`gh run view 29171411699`) 성공 여부 + brew(`hang-in/tap/tunaround`)·릴리스 페이지 아티팩트 발행 확인. 핸드오프 시점엔 in_progress였음.
- 릴리즈 바이너리 피처 = `semantic mcp serve worker engines a2a-out`(dashboard 미포함=의도, 소스 빌드 필요).

## 배포·검증

- **WMI 스폰 배포**: main 0.4.0 로컬 release 빌드(dashboard 포함) → `restart-win-mesh.ps1 -SourceBin` WMI 스폰 → mesh.pids 선별 종료 + rename-swap. **브로커 0.4.0 라이브**(uptime fresh, mac·win 스캐너 online, 고착 0, WAL 8KB). 세션 수신 poll 무중단.
- v2-47 #3 라이브(이전): 대시보드 "가동/WAL" 칩 렌더 확인(Chrome). ★ recency 라이브(luckyCAD ★1시간전·tunaRound ★40분전). 반응형 @media는 CSSOM으로 검증(확장 스크린샷이 뷰포트 축소 안 함 = 메모리 [[chrome-resize-screenshot-viewport]]).
- **맥 알림**: A2A task `a795e9c2244f0a496e5a5bf32c8ff109` → mac-claude-tunaRound(da9056e5)에 v0.4.0 업데이트 통지(git pull + worker 재빌드 or brew upgrade + restart-mac-mesh.sh). 맥이 poll로 자율 수신.

## 확정 결정·교훈 (재론 금지)

- **Codex A2A 10제안 검토 = 대부분 재발명**(SSE `SubscribeToTask`·watch-results·lease 이미 존재). 코드 확증 실이슈만 채택: #6 lease 자동연장(30분 lease가 장기 task를 실행 중 requeue하던 잠복 버그) + #4 cancel MCP 노출. await/notify_sender/subscribe/inbox/outbox/reply-threading/release/events는 비채택. 교훈=[[tunaround-north-star]] 재발명 금지.
- **토큰 env 이름이 둘이었음**(node.toml @env:TUNAROUND_TOKEN vs 데몬·훅 TUNA_BROKER_TOKEN) → init 기본을 TUNA_BROKER_TOKEN으로 통일. 기존 node.toml 불변.
- **설치 복잡도 = 본질(멀티머신 코어/토큰/네트워크) + 우발(피처 divergence·설정3종·토큰2개)**. 우발 완화: init 원커맨드 스캐폴드 + 토큰 통일 + AI 설치 안내. 본질은 남음. 문서가 오버셀했던 것(실 반복비용=1회+restart 한 줄)도 정리.
- **opencode "왜 안됐나"**: 워커 러너는 됨(v1.17.18 설치·CLI 불변). **감독 레인(스캐너가 opencode.db 열거)만 defer** - opencode가 JSON→SQLite 갓 전환(v1.15~)해 스키마 불안정(마이그 버그 7건+ 활성) = moving target 위 스캐너는 조용한-0 고장 위험. 기술 실패 아니라 업스트림 미성숙 회피.
- **DeepSource JS = 자문성**(main 미보호, canonical=clippy 3-OS·dashboard SPA·CodeRabbit). CodeRabbit `/dashboard/search 라우트 없음` = 오탐(별도 merge 서브라우터를 못 봄)이라 반박.

## 미커밋·브랜치·백그라운드

- **미커밋: 없음.** 브랜치: **main**(=origin/main=`6c78dbe`). 열린 피처 브랜치: 없음.
- **백그라운드**: 릴리즈 CI(run 29171411699) in_progress(핸드오프 시점). WMI mesh 데몬은 상주(정상).
- **미머지 잔여**: `origin/feat/session17-roster-supervision`(삭제 권고).

## 다음 세션 첫 행동 (우선순위 순)

1. **릴리즈 확인**: `gh run view 29171411699`(v0.4.0 CI 성공?) + brew/릴리스 아티팩트 발행 확인. 실패 시 로그 진단.
2. **v2-48 opencode 연결(감독 레인)** = 사용자 지정 다음 목표. 정본 [v2-48](../design/v2-48-opencode-wiring_2026-07-11.md). 착수 순서: ① opencode 현재 버전·`opencode.db` 스키마 **재대조**(냉각됐나 = §1 표 전체) → ② 냉각됐으면 스캐너 `enumerate_opencode_sessions`(opencode.db read-only 보수 파서 + 버전 핀) → 수신 (a) tuna-broker MCP native 우선(opencode.json mcp 필드) → human 신호 chat.message 플러그인(발화 조건 라이브 검증 선행) → ③ 아직 churn하면 보수 파서+버전핀 감내 또는 대기. 각 단계 라이브 스모크. **워커 러너는 이미 완성이니 감독 레인만.**
3. (선택) 스테일 브랜치 `origin/feat/session17-roster-supervision` 삭제.
4. 규율: 비trivial 전 plan + checklist·context-notes. 위임 tunaLlama→A2A codex→Sonnet, 아키텍트·리뷰=Opus. 커밋 자유, push·릴리즈는 승인.

## 검증 커맨드 참고

- 상태: `cargo test --features "morphology mcp serve worker dashboard"`(553 lib + 14 bin). CI clippy = `cargo clippy --features "morphology mcp serve worker[ dashboard]" -- -D warnings`.
- mesh: `curl -s http://127.0.0.1:8770/dashboard/health`. 재부팅 복구 = `pwsh -File scripts\restart-win-mesh.ps1`.
