# 세션26 핸드오프 (2026-07-12) - 잠복 이슈 3건 + v2-52 완주(⑤⑤④) + 배포·도그푸딩 + 이슈 #88 착수(부분→전체 #2 후속)

> 진입점: 이 파일 먼저 → 다음 = **이슈 #88 전체 해결(#2 app-server thread/loaded/list, 전용·계약 고정)** + **v0.5.0 릴리즈(도그푸딩 후·승인)**. 이전: [세션25](v2-handoff_2026-07-12_session25.md).

## 한 줄 요약

잠복 이슈 3건 수정(PR #89) → **v2-52 리팩토링 백로그 전체 완주**(⑤ store DTO=#90, ④ task JSON Stage1=#91 → 세션25의 ①②③⑥과 합쳐 6항목 전부) → 새 바이너리 mesh 배포·**④ JSON 프로토콜 라이브 검증 성공** → GPT 온보딩 답변 검토(대시보드-릴리스-포함만 유효 P0) → **이슈 #88 착수**(codex presence 유령 오라우팅). #88은 시간창 게이트를 구현했으나 적대 검증이 **부분 완화(완전 해결 아님)**로 판정 → 사용자 결정=**전체 해결(#2)을 다음 세션에서**. main=`1ec59b3`(핸드오프 커밋 전). 미커밋 없음(#88 작업은 브랜치 보존).

## 완료 사항 (전부 머지·적대 검증 GO·봇 리뷰 처리·CI green)

| PR | 트랙 | 핵심 | 머지 |
|----|------|------|------|
| #89 | 잠복 이슈 3건 | post_turn R1 계약(success→error) · index_terminal_task race 직렬화(a2a_store 락, 데드락 없음) · OllamaEmbedder embed 타임아웃(env, 30s) | `b93e277` |
| #90 | v2-52 ⑤ store DTO | `ConversationSnapshot` 중립 타입(**serde 없음**)이 SQLite 스키마·트리 append 상태머신 누수 캡슐화. store `From` impl 변환 격리 → SQLite 오라클 불변. S0~S6 점진. | `9b2a0ce` |
| #91 | v2-52 ④ task JSON | poll_tasks 응답 `TASKS_JSON <json>` 프리픽스 + human 블록 병존. 워커 JSON 우선·문자열 폴백. 라이브 mesh 4조합 하위호환. Stage 1(①②③). DTO=무게이트 `src/a2a_wire.rs`. | `d5a15e7` |

- 각 PR: understand→계약 고정→점진 구현→**적대 검증 워크플로우(전부 GO, blocker/major 0)**→봇 리뷰 전수 처리→canonical CI green→머지.
- **v2-52 리팩토링 백로그 = 전체 완료.** ① main.rs · ② fmt · ③ mcp.rs · ⑥ tasks.rs(세션25) / ⑤ · ④(세션26). **잔여=④ Stage 4(문자열 파서 제거)만** = mesh 전체 롤아웃+도그푸딩 후 defer.
- **배포·도그푸딩**: 릴리스 빌드(`morphology mcp serve worker dashboard`, semantic 제외) → **WMI 스폰**으로 `restart-win-mesh.ps1 -SourceBin`(rename-swap·PID 선별종료=세션 Monitor 생존) → broker 재기동. **④ 라이브 검증 성공**: poll 응답에 `TASKS_JSON [{...context_id:null...}]` 프리픽스 + human 블록 병존 확인(신 broker JSON emit + "-"→null 정규화 + 하위호환). mesh 정상(mac·win 스캐너 online). 구 바이너리 세션 Monitor + 신 broker 하위호환 라이브.

## 진행 중 / 다음 세션 착수 (우선순위 순)

### 1. 이슈 #88 전체 해결 (사용자가 고른 다음 작업) — 브랜치 `fix/issue-88-codex-presence-ghost` @ `592121e`(push됨)
- **문제**: 종료된 Codex TUI 세션의 stale rollout이 presence 로스터에 online으로 남아(apply_process_gate가 codex 프로세스≥1이면 러너 전체 유지=all-or-nothing) A2A task가 유령 UUID로 오라우팅됨. codex는 마커·PID 없어 per-thread 생존 판정 경로 전무.
- **이번 세션 구현(부분 완화, defer/재검토 대상)**: `apply_codex_human_input_gate`(codex는 human_input_at 또는 created_at이 now-window(60분) 이후면 유지) + `system_time_to_db_datetime`(UTC civil) + CLI `--codex-human-window-mins`. lib 595 pass·clippy·worker단독 clean.
- **⚠ 적대 검증 조건부 NO-GO(재론 금지)**: 이 게이트는 **#88 부분 완화이지 완전 해결 아님**. (major) grace 절이 최근 유령(생성 60분 내, #88 재현=~4분 churn)을 살림. 더 근본: 방금 쓰다 닫은 세션은 human_input 최근이라 활성과 구분 불가 → 60분 잔존(시간창 원리적 한계). 실효=유령 수명 240→60분 bound + relay 자기유지 차단. 부수 FP: 60분+ 미입력 살아있는 codex(장기작업 관전)가 A2A 타깃 드롭.
- **사용자 결정 = 전체 해결(#2)로 진행**: **app-server `thread/loaded/list`를 canonical source**로(설계 §2·§5.1 실측 존재, 현 codex_appserver.rs엔 빌더/파서 없음=net-new) + rollout mtime 스캔은 fallback + **killed-TUI resume 거동 라이브 실측**(app-server가 잔존 rollout을 resume 성공시키면 못 거름=최대 불확실성) + 사람 TUI 누락 fallback(loaded-set은 이 app-server 로드분만) + relay claim 전 probe(#4). **착수 전 계약 고정.** #2가 이번 시간창 게이트를 supersede 가능(그럼 브랜치 폐기 또는 게이트를 보조로).
- 상세: 이슈 #88 본문(gh issue view 88), 브랜치 context-notes.md #88 섹션, understand+design 워크플로우 산출(4렌즈 결합지도, 6접근 평가).

### 2. v0.5.0 릴리즈 (도그푸딩 후 + 사용자 승인)
- 새 바이너리(잠복3·⑤·④)가 mesh에서 라이브 중. **며칠 도그푸딩 안정 확인 후** `cargo release minor` → v0.5.0 태그 → cargo-dist(4타깃)+brew. CHANGELOG [Unreleased]→[0.5.0]. **릴리즈 태그는 리팩토링 트랙 자율 예외 밖**([[dogfood-before-release]], 별도 승인).
- **v0.5.0 전 P0 후보(GPT 온보딩 답변 검토서 확인)**: **대시보드를 릴리스 산출물에 포함.** 현 `dist-workspace.toml` 피처=`["semantic","mcp","serve","worker","engines","a2a-out"]`=**dashboard 없음** → 릴리스 바이너리가 대시보드 SPA 없이 안내 페이지만. 조치=dist 피처에 `dashboard` 추가 + **릴리스 CI에 frontend `npm run build` 선행 스텝**(비용=CI 프론트빌드, 용량 아님). north star 정합. (그 외 GPT 제안=페어링코드·첫실행 위저드·온보딩 퍼널=재발명·competitive lens라 비채택.)

## 확정 결정·교훈 (재론 금지)

- **리팩토링 트랙 push 자율**: CI all-green + 봇/적대 리뷰 이슈 전부 해소면 push·PR·머지 자율([[refactoring-push-autonomous-when-green]], 이번 세션 신규 메모리). **릴리즈 태그는 예외**.
- **적대 검증 워크플로우가 표준 오라클**: 매 PR/버그수정에 understand→계약→구현→적대 검증(독립 렌즈 반증)→봇 처리→머지. #88에서 적대 검증이 "부분 완화≠완전 해결"을 잡아 오버클레임을 막음(정직 보고의 실증).
- **DB datetime 사전순=시간순**: `age_secs`(store::a2a, sqlite-gated=worker단독 불가) 대신 threshold 문자열+사전 비교로 sqlite 비의존. `normalize_iso_to_db_datetime`은 offset 스트립(UTC 유지, codex rollout=Z 실측).
- **무게이트 공유 DTO는 crate 루트**(a2a_wire): store::a2a·store::agents는 sqlite-gated라 worker 단독 빌드 접근 불가. 브로커(mcp)·워커(worker) 공유 계약은 무게이트 모듈에.
- **배포=WMI 스폰**(하네스 job 탈출, [[mesh-restart-needs-job-escape]]) + `restart-win-mesh.ps1 -SourceBin`(rename-swap·mesh.pids 선별종료로 세션 Monitor 생존). mesh.pids 존재 확인 필수(없으면 전수종료 폴백=세션 poll 재무장 필요).
- **GPT 온보딩 답변 = competitive lens 드리프트**: 온보딩 퍼널·첫 성공 UX·페어링 코드는 tunaRound의 북극성(팔지않되 공개·재발명 금지·개인 도구)과 불일치. 유일 유효=대시보드-릴리스-포함(그것도 이미 presence 스캐너가 자동탐지·로스터 표시를 함=릴리스에 대시보드만 넣으면 됨).

## 미커밋·브랜치·백그라운드

- **미커밋: 없음**(핸드오프 커밋 제외). main=`1ec59b3`.
- **열린 브랜치**: `origin/fix/issue-88-codex-presence-ghost`(@ `592121e`, #88 부분 완화 WIP, 다음 세션 #2 진행/재검토). 그 외 origin=main만(#89·#90·#91 머지분 prune 완료).
- **백그라운드**: WMI mesh 데몬 상주(broker 8770·app-server 8790·presence-scan·codex-relay·watch-results, 새 바이너리). 이 세션 A2A 수신 Monitor는 재시작 시 SessionStart 훅 재무장. 완료된 워크플로우 4개(#89·#90·#91 적대검증 + #88 understand/verify)는 종료.

## 검증 커맨드 참고

- 상태: `cargo test --features "morphology semantic mcp serve worker dashboard"`(595 lib). 게이트: `cargo fmt --all -- --check` + `cargo clippy --features "..." --all-targets -- -D warnings` + **no-default·worker단독·all-features** 확인(무게이트 DTO/게이트가 worker 단독 빌드 유지).
- 대시보드: http://127.0.0.1:8770/dashboard. mesh 헬스: `curl -s http://127.0.0.1:8770/dashboard/health`. 재부팅 복구=`pwsh -File scripts\restart-win-mesh.ps1`.
