# 세션25 핸드오프 (2026-07-12) - v2-52 리팩토링 백로그 clean 스윕 (main.rs·mcp.rs·tasks.rs 분리 + fmt + CI 게이트)

> 진입점: 이 파일 먼저 → 다음 목표 = **v0.5.0 릴리즈(도그푸딩 후)** + **v2-52 잔여(④ task JSON·⑤ store DTO, 전용 세션)**. 이전 세션: [세션24](v2-handoff_2026-07-12_session24.md).

## 한 줄 요약

세션24가 백로그로 남긴 v2-52 리팩토링의 **clean 기계적 분리 전부를 완주**: main.rs·mcp.rs·store/sqlite/tasks.rs 세 god파일 분리 + 토크나이저/임베더 dedup + 전역 fmt + CI fmt/clippy --all-targets 게이트. **PR #83~#87 5개 머지**, 전부 적대 diff 리뷰 등가 확인 + CI green + 봇 반영. 스테일 브랜치 전량 정리. **④ task JSON·⑤ store DTO는 사용자 결정으로 defer**(설계/동작 변경이라 전용 세션). main=`f795ba3`, 미커밋 없음, 열린 PR/브랜치 없음.

## 완료 사항 (전부 머지·CI green·적대+봇 리뷰 반영·순수 코드 이동=동작 불변)

| PR | 머지 | 항목 |
|----|------|------|
| #83 | `c97d3e4`(커밋 `3b86b56`) | **① main.rs 분리**: 서브커맨드 dispatch·백엔드 빌더를 새 `src/cli_run.rs`로 추출(run_observe·reindex·mcp_search·serve_mcp·node + build_indexer·retriever·validity_sink·annotation_sink·participants). main.rs 1,160→618줄 |
| #84 | `3d2754f`(커밋 `ab3bd69`) | **dedup**(#83 봇 제안): 질의 토크나이저·임베더 구성을 `build_index_tokenizer`(serve→sqlite)·`build_query_tokenizer`·`build_embedder` 팩토리로 통일(5곳 공유). cli_run.rs 635→581 |
| #85 | `043dd0b`(커밋 `f8c1406` fmt·`b64c9a4` ci) | **② 전역 fmt + CI 게이트**: `cargo fmt --all`(49파일 +6626/-1771) + 신규 `fmt` 잡(`fmt --all --check`) + 기존 clippy 2곳에 `--all-targets` |
| #86 | `6393cb2`(커밋 `0a8c3f2`) | **③ mcp.rs 분리**: 17개 rmcp #[tool]을 named tool_router 합성으로 mcp/{search,tasks,registry}.rs + indexing.rs로. mcp.rs 1,660→983 |
| #87 | `f795ba3`(커밋 `f622930`) | **⑥ tasks.rs 분리**: store/sqlite/tasks.rs의 impl SqliteStore 21메서드를 tasks/{state,lease,replay}.rs로. tasks.rs 1,880→1,341 |

## 확정 결정·교훈 (재론 금지)

- **④ task JSON·⑤ store DTO = defer**(사용자 결정 2026-07-12). ④=`mcp/format.rs`·`worker.rs`의 `[id] from=... msg=...` 문자열 프로토콜을 JSON으로(라이브 mesh 프로토콜 **동작 변경**, 다단계 마이그레이션). ⑤=orchestrator/repl/store가 공유하는 StoredSession/StoredMessage/Utterance 대신 중립 도메인 타입(ConversationSnapshot·MessageNode·BranchHead) 도입(핵심 토론 모델 **아키텍처 변경**, blast radius 큼). 둘 다 v2-52 doc이 "착수 전 계약(공개 API·테스트) 고정"을 요구 → 전용 세션에서 계약 고정 후 착수. 정본 [v2-52 §2](../design/v2-52-refactoring-backlog_2026-07-12.md).
- **rmcp 1.8 named tool_router 합성 확정**(Context7 검증): `#[tool_router(router = 이름, vis = "pub(crate)")]`로 여러 impl 블록을 내고 `Self::a() + Self::b()`로 합성. `#[tool_handler]`는 `Self::tool_router()`를 호출하므로 그 이름의 연관함수가 합성을 반환하면 됨. **서브모듈은 부모의 자식이라 private 필드/메서드/const에 그대로 접근**(위임 보일러플레이트 불요) - mcp.rs·tasks.rs 분리의 핵심.
- **CI 게이트 강화**: canonical CI에 `fmt --all --check`(ubuntu 1잡, 빌드 불요) + clippy `--all-targets`(테스트 코드 idiom 커버, P0 품질게이트가 고친 부류를 앞으로 차단). `--all-features` 확대는 dashboard 서브피처의 frontend/dist 빌드 의존이라 매트릭스에 안 맞아 **보류**(feature-scoped `--all-targets`로 목표 충족). ci.yml 참조.
- **fmt 결정성**: 로컬 rustfmt 1.8.0-stable(rustc 1.94.1)과 CI `@stable`(최신) 간 **드리프트 없음**(CI fmt 게이트 통과로 실증). rustfmt 기본 포맷은 edition 내 stable 간 동일. 다음에 PR 올릴 땐 로컬 `cargo fmt --all` 한 번 돌리면 게이트 통과.
- **봇 false positive 2종(주의)**: (a) gemini "미사용 import"(mcp.rs) = child의 `use super::*`가 쓰는 것을 놓친 오판, **clippy -D warnings 통과가 사용 증명**. (b) gemini "private const/필드를 자식이 못 써서 컴파일 에러"(tasks split) = **Rust descendant 프라이버시 오해**, CI 3-OS 빌드 통과가 반증. **둘 다 무시가 정답**(가시성 넓히면 불필요한 확대). CodeRabbit/DeepSource가 verbatim 이동 코드를 "새 라인"이라 pre-existing 이슈로 재플래그하는 것도 동일(머지 후 소멸).
- **CHANGELOG 미기록**: 순수 내부 리팩토링 5건 전부 CHANGELOG 항목 없음(정책: "내부 구현은 넣지 않음"). [Unreleased]는 세션24의 presence 타임라인·/annotate만.
- **mac 조율 완료**: fmt 전 heads-up + 직전 핑 + 머지 후 재핑 3단 왕복. mac은 tunaRound 소스 편집 홀드(데몬 업데이트 git pull만). fmt 머지 후 pull 안내함.

## 발견된 잠복 이슈 3건 (pre-existing, 이번 리팩토링이 만든 것 아님. 별도 처리 대상)

CodeRabbit이 mcp.rs 분리 시 verbatim 이동된 코드를 재검토하며 표면화(순수 이동 PR 범위 밖이라 미수정, 다음 세션 후보):
1. **`post_turn`이 writer 실패 시 `CallToolResult::success` 반환**(mcp/search.rs) = R1 계약 위반(search_context/read_transcript/registry 툴은 실패를 error로). 클라가 write 실패를 못 봄. **quick win**(Err 분기를 error로).
2. **`index_terminal_task` delete-then-append 동시성 race**(mcp/indexing.rs) = 백필↔완료 색인 경합 시 중복/유실 가능. heavy lift(직렬화 필요).
3. **`OllamaEmbedder`가 reqwest blocking 타임아웃 없음**(store/embedding.rs) = semantic 빌드에서 search_context의 spawn_blocking이 무한 대기 가능. heavy lift(embed timeout).

## 다음 세션 첫 행동 (우선순위 순)

1. **v0.5.0 릴리즈** (도그푸딩 안정 확인 후, 세션24 유예분): `cargo release minor` → v0.5.0 태그 → cargo-dist(4타깃+brew) → 맥 알림. **push·태그는 승인 후.** CHANGELOG [Unreleased]→[0.5.0]. (세션25 리팩토링은 순수 내부라 릴리즈 노트 무영향.)
2. **v2-52 잔여**(전용 세션, 계약 고정 먼저): ⑤ store DTO(중립 타입 계약 설계 → orchestrator/repl/store 배선) / ④ task JSON(JSON 응답 추가 → worker 우선 → 문자열 하위호환 → 파서 제거). 정본 [v2-52 §2](../design/v2-52-refactoring-backlog_2026-07-12.md).
3. **잠복 이슈 3건**(위): post_turn(quick) 먼저, index race·embed timeout은 각각 검증 동반.
4. 규율: 비trivial 전 plan + checklist·context-notes. 위임 tunaLlama→A2A codex→Sonnet, 아키텍트·리뷰·검증=Opus. 커밋 자유, push·릴리즈는 승인(이번 세션은 사용자가 push+PR+머지 자율 승인).

## 미커밋·브랜치·백그라운드

- **미커밋: 없음.** main=`f795ba3`. 열린 PR: 없음. 열린 브랜치: 없음(origin=main만, 머지분 전량 prune).
- **백그라운드**: WMI mesh 데몬 상주(broker 8770·app-server 8790·presence-scan·codex-relay·watch-results, uptime ~3.6h 정상). 이 세션의 A2A 수신 Monitor는 재시작 시 SessionStart 훅으로 재무장.
- **배포 상태**: broker 0.4.0 바이너리 라이브(세션25 리팩토링은 미배포 - 순수 내부 구조 변경이라 재배포 불요, 다음 배포 시 자동 포함). broker.db v11.

## 검증 커맨드 참고

- 상태: `cargo test --features "morphology mcp serve worker dashboard"`(577 lib). **CI 게이트 강화됨**: `cargo fmt --all -- --check`(0 diff) + `cargo clippy --features "..." --all-targets -- -D warnings`(테스트 코드 포함). PR 전 로컬 `cargo fmt --all` 필수.
- 대시보드: http://127.0.0.1:8770/dashboard. mesh 재부팅 복구 = `pwsh -File scripts\restart-win-mesh.ps1`.
- god파일 분리 후 크기: main.rs 646 / mcp.rs 985(production ~166 + tests ~800) / tasks.rs 1,341(production 제거·tests 유지). production 로직은 cli_run.rs·mcp/{search,tasks,registry,indexing}.rs·tasks/{state,lease,replay}.rs에.
