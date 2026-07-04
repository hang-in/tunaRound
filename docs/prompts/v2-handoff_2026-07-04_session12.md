# 세션12 핸드오프 (2026-07-04): agentgateway 검토 후 D·B·C 3 PR + Mac 온보딩

> WIN 핸드오프. 세션12 = agentgateway 선별 도입 검토 → doctor·트레이스·node태그 3개 PR 머지. 이전 [session11](v2-handoff_2026-07-04_session11.md)=레지스트리(PR #5).

## 이번 세션에 한 것

**agentgateway 인프라 요소를 tunaRound에 선별 도입 검토([노트](../design/v2-agentgateway-selective-adoption_2026-07-04.md)) 후 D→B→C 3개 PR 머지.** 검토 결론: capability=태그(레지스트리로 이미 됨) + 값싼 trace/denylist만 취하고, 정책 규칙 엔진·별도 backend registry·artifact lineage DAG는 gateway 변질/YAGNI로 비채택.

- **D (PR #6=`89cdbf2`)**: doctor Stage 4 갭. `Tokenizer::backend_name()`로 Kiwi 실로드/lindera 폴백 판별 + http 레인 Ollama 도달 ping. 설계 §C 프리플라이트 잔여 채움.
- **B (PR #7=`27f04e6`)**: (B1) tasks `runner` 컬럼(스키마 v8, claim 시 기록, requeue 시 클리어) - v7이 이미 커버한 started/completed/session_id는 안 만듦(축소). (B2) 쓰기 민감 path 가드(WRITE_GUARD_DIRECTIVE, Write 시 claude/codex 프롬프트 주입, behavioral=readonly-soft 정합, 하드 차단 아님).
- **C (PR #8=`5f3ec50`)**: node.toml lane `tags` 배선(T4에서 미룬 것) → node 워커도 셀렉터 발견. doctor/node 기동 tags 형식 검증(parse_tags 재사용). backend는 별도 registry 없이 lane 정의=named backend.

4개 PR(레지스트리 #5 포함) 전부 3-OS CI green + CodeRabbit 반영 후 squash 머지. 스키마 **v8**(tasks.runner).

## 위임 vs 회담 (개념 정리, 재유도 금지)

- **A2A(브로커+워커+레지스트리) = task 위임**. 1 task → 1 워커(다중 매칭이면 후보 반환, 사람이 선택=HITL). **브로드캐스트/팬아웃 없음**(4명 의견 필요하면 4번 던지거나 단일머신 토론).
- **라이브 다자 토론(회담)은 단일머신 `chat --roster`**. 크로스머신 토론은 비목표(세션6 결정).
- **프로젝트별 격리**: `to_selector`에 `project=X` + 워커가 `project=X` 태그로 등록 = 자동 격리. **태그 규율 전제**(무태그/넓은 셀렉터면 격리 안 됨).

## 다음 세션 첫 행동

1. `git pull --rebase origin main` + `cargo test --features "morphology mcp serve worker engines"`(베이스라인 428).
2. **Mac 노드 태그 온보딩**(준비됨): Mac이 main pull → 빌드 → node.toml lane에 `tags="...,project=tunaround"` 설정 → `doctor`(태그형식 검증) → `node` 기동 → 윈도우 dispatcher가 `list_agents`/`send_task to_selector`로 왕복 확인. (Mac↔윈도우 git 규약: Mac은 MAC 최신 줄/맥 핸드오프만 편집.)
3. 후속 옵션: R9 A2A poll 견고화(옵션) · 릴리스 cadence(현 v0.2.2) · agentgateway 검토의 v1-후 잔여는 대부분 완료(C까지).

## 규율 리마인더
- 구현=Sonnet 서브 + Opus 리뷰·독립검증. GitHub Flow(PR+3-OS CI)+CodeRabbit 반영 후 머지. 커밋/검증/push 분리, push 전 pull --rebase. cargo는 Bash 툴. 레포 PUBLIC=LAN IP·토큰·사설호스트 평문 금지.
