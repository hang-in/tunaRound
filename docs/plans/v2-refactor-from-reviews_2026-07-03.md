# 1차 리팩토링 계획 (제미나이+코덱스 리뷰 기반) - A2A 도그푸딩 겸용

> 2026-07-03 세션8. 제미나이·코덱스의 1차 정적 리팩토링 리뷰([reviews/](../reviews/))를 Opus가 자체검증·삼분류한 실행 계획. **다음 세션에 3자(Windows-Opus 통합자 + 맥-claude worker + 로컬 Codex worker)가 A2A 파트너 위임으로 나눠 처리 = 리팩토링이면서 워커 데몬·이기종 위임 도그푸딩.** 출처 리뷰: [제미나이](../reviews/제미나이로부터 1차 리팩토링 제안.md) · [코덱스](../reviews/코덱스로부터 1차 리팩토링 제안.md).

## 0. 자체검증 요지 (Opus)

- 두 리뷰 다 품질 높음. **코덱스가 더 정밀·실행가능**(라인 지목·조건부 UPDATE 등 구체 제안·"문제없음" 판별이 우리 YAGNI와 일치). 제미나이는 일부 심각도 과장(async Runner trait을 top3에 둠, A2A 파싱을 "토론 완전 멈춤"으로).
- **코드로 직접 확인한 HIGH 4건**(R1·R2·R3·R4)은 실버그로 확정. 특히 R1·R4·(fail_task 상속)는 **우리가 방금 쓴 코드**의 결함이라 외부 리뷰의 값이 실증됨.
- 둘 다 짚은 "건드리지 말 것"(DB 교체·stream 파서 추상화·저장모델 전면재작성·파일크기만으로 분리)은 **우리 기존 결정과 일치** -> 그대로 존중.

## 1. 리팩토링 태스크 (우선순위·위임적합성 포함)

각 태스크는 자기완결적이어서 A2A task로 dispatch 가능하다. **위임적합성**: 순수/작음=자율 워커 이상적, cross-cutting/플랫폼=스펙 명확화 + Opus 리뷰 밀도↑.

### Tier 1 - 실버그, 우선 (워커 신뢰성과 직결)

- **R1 [높음] MCP 에러 계약 정직화.** 문제: `claim_task`/`complete_task`/`fail_task`가 내부 실패를 텍스트로 감싸 `CallToolResult::success`로 반환 → 클라(`McpHttpClient`)는 `isError`만 보고 성공 처리 → **claim 실패해도 러너 실행, complete 실패해도 "완료" 출력**. 위치: `mcp.rs:422·426`(+fail_task), `mcp_client.rs`(parse_jsonrpc_sse), `worker.rs`. 수정: 실패를 `CallToolResult::error`(isError:true) 또는 McpError로, 클라는 isError→Err, 워커는 claim/complete 실패를 감지(claim 실패면 러너 미실행). 검증: 실패 유도 시 워커가 러너 안 돌리고 로그. 난이도: 중. 위임: cross-cutting, Opus 리뷰 밀도↑. 출처: 코덱스 #1.
- **R2 [높음] A2A 상태머신을 저장소 조건부 전이로.** 문제: `update_task_state`/`complete_task`가 현재상태 가드 없는 무조건 UPDATE → 두 워커가 같은 task 다 claim 가능, completed를 canceled로 덮어씀. 위치: `sqlite.rs:849·870`, `mcp.rs:197`, `a2a_server.rs:122`. 수정: 저장소에 조건부 전이(claim: `WHERE state='submitted'`, complete: `WHERE state='working'`, fail/cancel 유사) + `rows_affected != 1`이면 TransitionConflict 반환. 검증: 이중 claim 테스트(둘째=conflict), terminal 덮어쓰기 차단. 난이도: 중. 위임: cross-cutting. 출처: 코덱스 #2. **R1과 묶음(코덱스 top1).**
- **R3 [높음] watchdog 프로세스 트리 종료.** 문제: 타임아웃 시 `kill -9 PID`/`taskkill /F /PID`로 **부모만** 죽여 자식(실제 claude/codex 작업)은 생존. 위치: `exec.rs:184`(`kill_pid`). 수정: Windows `taskkill /T`, Unix는 process group(setsid + killpg) 또는 job object로 트리 종료. 검증: 자식 스폰 fixture로 플랫폼별 트리 종료 확인. 난이도: 중(플랫폼 검증). 위임: 플랫폼 지식 필요. 출처: 코덱스 #6 / 제미나이 #2.
- **R4 [높음] `--context-map` 파싱을 Result로.** 문제: 오타(k=v 아닌 항목)를 `filter_map`으로 조용히 버림 + context 없으면 default project_path 폴백 → `--write`면 엉뚱한 레포 수정. 위치: `main.rs:639`, `worker.rs`. 수정: `parse_context_map(&str) -> Result<HashMap, String>`로 분리, 빈 key/value·형식오류 진입 시 거부. 검증: 잘못된 map → 에러 exit. 난이도: **소**. 위임: **이상적**(작고 순수). 출처: 코덱스 #5(우리 최근 코드).

### Tier 2 - 실이슈, 낮은 긴급도

- **R5 [중] `save_session` orphan 보조행 정리.** messages/FTS만 지우고 `message_vectors`·`message_validity`는 남김 → orphan 벡터가 top-K 차지·ID 재사용 시 옛 유효성 상속. 위치: `sqlite.rs:289·724`. 수정: 트랜잭션에서 사라진 message_id의 두 테이블 행 정리(또는 FK ON DELETE). 검증: 축소 저장 후 orphan 0. 난이도: 소-중. 출처: 코덱스 #8.
- **R6 [중/낮] Embedder `dim` 동적화.** `OllamaEmbedder::dim()`이 1024 하드코딩 → 비기본 모델(768 등) 벡터 유실. 위치: `embedding.rs:126`. 수정: 첫 임베딩 결과 길이로 확정 또는 config. 검증: mock 768 왕복. 난이도: 소. 위임: **이상적**. 출처: 제미나이 #5.
- **R7 [중] retriever/reader Result 계약.** `ContextRetriever`/`TranscriptReader`가 Result 미반환 → DB 오류와 빈 결과 구분 불가(장애를 "결과 없음"으로 은폐). 위치: `orchestrator/mod.rs`, `retriever.rs`, `mcp.rs`. 수정: 오류 중요한 reader는 `Result<Vec<_>, StoreError>`, fallback은 REPL 경계에서. 검증: 오류 주입 시 구분. 난이도: 중-대(trait+구현+테스트더블). 위임: 신중(넓음). 출처: 코덱스 #9(checklist 기존 노트와 동근).
- **R8 [중] 검색 폴백 통일.** mcp-search 폴백은 외래어 alias 추가·정상 fts_query는 OR인데 수동 폴백은 공백조인 → 진입점별 검색결과 상이. 위치: `main.rs` 여러 tokenizer 조립부, `tokenizer.rs:35`. 수정: 색인용/질의용 tokenizer builder를 search 모듈에 1회. 난이도: 중. 출처: 코덱스 #7.
- **R9 [낮/옵션] A2A poll 견고화.** ad-hoc 문자열 파싱 대신 구조화(JSON) 또는 파서 강화. 위치: `mcp.rs`(format_open_tasks), `worker.rs`(parse_open_tasks). **현 구현은 블록경계(`\n\n[32hex] from=`)+통제된 state/ctx라 생각보다 견고**하므로 우선순위 낮음(편할 때). 출처: 제미나이 #1.

## 2. 미루는 것 (유효하나 지금 아님)

- **Runner async trait 전환**(제미나이 #3): 모든 러너+호출처 전면 리팩터, std::thread(현행)로 이 스케일 충분. **스킵/보류.**
- **main.rs/mcp.rs 분해**(제미나이 #4/코덱스 #10): 온건한 SRP 청소(run_chat/run_work/run_reindex/build_search_backends). 여유 시, 급하지 않음.
- **session-id pull·CoreSync 일관성**(코덱스 #3·#4): 오래된 Stage-3 코드. 실이슈 가능성 있으나 **먼저 재현·검증** 후 착수.
- **도메인-저장 모델 결합**(코덱스 모델결합): 안정적이라 다음 모델 변경 시에만.

## 3. 도그푸딩 협업 방식 (3자)

**핵심: 각 R태스크를 A2A task로 코어에 dispatch → 파트너 워커가 처리 → Opus 리뷰 → 커밋 = 리팩토링이면서 A2A 파트너 위임 도그푸딩.**

- **역할**: Windows(Opus) = 코어 호스팅 + dispatcher + **통합자**(스펙 확정·리뷰·테스트·머지). 맥(claude) = worker(대화형 HITL 승인). 로컬 Codex = worker(`tunaround work --runner codex` 자율, 또는 대화형 codex).
- **위임 순서 추천**: **R4·R6**(작고 순수 = 자율 워커 워밍업, 도그푸딩 첫 타깃) → **R1+R2**(top, 묶어서, Opus 리뷰 밀도↑) → **R3**(플랫폼) → R5·R8 → (여유) R7·R9.
- **워커 실행**: `--write` 필요(코드 수정). project-path 격리, 크로스머신은 git push/pull로 변경 이동. 자율 워커면 `tunaround work --runner codex --write --agent codex-worker`, 대화형이면 세션에서 승인.
- **규율(공통)**: 태스크당 **테스트 필수**, 커밋 분리(태스크당 1커밋), Opus가 머지 전 검증(cargo test 해당 피처 + clippy), 굵직한 재론 금지. cross-cutting(R1·R2·R7)은 스펙을 이 문서보다 더 구체화한 뒤 위임.
- **주의**: R1·R2는 서로 얽혀(상태전이+에러계약) **함께** 가는 게 안전. 리팩토링 중 기존 테스트(288~304)가 깨지면 그 태스크의 회귀로 간주.

## 4. 다음 세션 진입

1. 이 문서 + 두 리뷰(`docs/reviews/`) 읽기.
2. 코어 기동(Windows) + 맥/코덱스 워커 온보딩(`docs/reference/a2a-usage.md`).
3. R4로 도그푸딩 워밍업(작은 태스크 1개를 A2A로 위임→처리→리뷰→커밋) → 흐름 확인 후 R1+R2.
4. checklist에 R1~R9 추가(별 섹션).
