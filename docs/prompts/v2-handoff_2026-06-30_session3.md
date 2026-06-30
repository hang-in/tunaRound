# tunaRound v2 핸드오프 - 2026-06-30 Windows 세션 3

> 이전: [session2](v2-handoff_2026-06-30_session2.md)(Plan 09~20). 이 세션 = (A) 코어-백엔드 half-a2a Stage 0~3a + 코드리뷰/리팩토링.

## ① 한 줄 요약

이 세션은 **(A) 코어-백엔드 = half-a2a** 방향으로 Stage 0(검색품질·요약이월) → Stage 1(read_transcript·세션id) → **Stage 2(push→pull, 라이브 검증)** → Stage 3 설계 + **3a-1(HTTP MCP 상주)·3a-2(러너 URL 접속)**까지 갔고, 중간에 **코드 전반 리뷰 + 버그수정(Batch A) + 리팩토링(Batch B)**을 했다. 검증/실측 위주.

## ② 핵심 방향 (정본: docs/design/v2-A2A-core-backend_2026-06-30.md)

- **(A) half-a2a = 목표**: 에이전트가 **공유 코어를 통해** 서로 발언을 읽고 쓰는 클라이언트, **사람이 운전**. Stage 0~3.
- **(B) full a2a = 안 지음**: 에이전트 자율 turn-triggering. 조건부(토큰단가↓ or 기계검증 과제 자율수렴)이고 전망 없음. turn-policy 솔기만 비용0 보험으로 유지.
- **turn-policy 솔기**: HumanDriven(기본·유일) / AutoLoop(미래 (B) 플러그인).
- **배포**: 자체호스팅 단일테넌트. 데이터=사용자별 SQLite + 단일 Redis(PG 비채택). WAN=Tailscale 오버레이(포트포워딩 대신). 임베딩=원격 Ollama(사용자 소유·상시).

## ③ 이 세션이 한 것 (커밋순 요지)

- **Stage 0 검색품질**: FTS 질의 AND→OR(리콜 R@5 0.55→0.90) + precision@k + 요약 carry-forward(드롭 턴 압축 이월) + eval 코퍼스 40발언/21질의 확대.
  - **벡터 측정으로 쿼리확장·리랭커 도입 취소**(벡터가 어휘공백 메움, MRR 0.976). ChromaDB/GRPO 비채택(측정 근거). 검색 트랙은 현 eval 기준 충분.
- **Stage 1**: read_transcript MCP 툴(TranscriptReader + SqliteTranscriptReader) + 세션id 주입(with_search_session → --session-id).
- **Stage 2 push→pull (crux, 검증됨)**: ContextMode(Push/Pull) + 좌석 능력 게이트 + 포인터 프롬프트 + 프롬프트 크기 계측. 라이브 실측: **토큰 80~95%↓·전사길이와 탈동조 + grounding 유지**. claude `--allowedTools`로 MCP 권한 해소. pull 모드 carried 요약 항상 켜기(안전망).
  - **⚠ codex pull 불가**: codex exec가 MCP 도구 호출을 승인 모델로 막음(헤드리스 "사용자 취소"). approval_policy=never 무효. → **pull=claude 전용(is_mcp_capable), codex=push 폴백(grounded)**. codex pull 활성화는 후속.
- **Stage 3 설계(Plan 25) + 3a-1·3a-2 (e2e 검증)**: 코어를 HTTP MCP로 상주(`--serve-mcp <addr> --token`, rmcp StreamableHttpService+axum+bearer, serve feature). 러너 `with_search_url`로 에이전트가 원격 HTTP MCP 접속(claude 우선, codex bearer-env는 TODO). **라이브 e2e**: 코어 `--serve-mcp 127.0.0.1:8766 --db shared --token T` 띄우고, 별도 REPL의 claude가 `--search-url ...mcp --search-token T --pull-context`로 원격 HTTP(bearer 인증)에서 read_transcript 호출해 전사 정확 인용. **remote core 동작 = half-a2a 네트워크 위 실증.**
- **코드리뷰 + 수정**: 리뷰 15건 큐레이션 → Batch A(get_message .ok 삼킴·path_to_root 순환+O(N²)·codex TOML 주입·DefaultHasher 비결정) + Batch B 리팩토링(파라미터 구조체화·run_round &[]·active_path 1회).

## ④ 현재 상태 / 검증

- 기본 ~128 / `--features "mcp sqlite morphology semantic"` ~135 pass, clippy 클린(too_many_arguments allow 제거). serve feature 별도 빌드 OK.
- 라이브: claude+codex 실제 spawn, pull(claude)·push(codex) 혼합 grounded. HTTP MCP 401 인증 테스트.
- 백엔드: Ollama 터널(SSH [사설호스트]:[사설포트] → 11435, bge-m3), Redis 6379. 검색 측정용으로만 사용(Stage 2/3는 불요).

## ⑤ 남은 항목

- **Stage 3 잔여**: 3a-3 front=core(REPL+HTTP MCP 단일프로세스) · 3b 토큰(3a-1에 이미 있음) · 3c Tailscale(ops) · 3d post_turn/get_roster(원격 쓰기 권위) · 3e 영속 에이전트(codex app-server, 보류).
- **codex pull 활성화**: codex 승인 설정 심층 조사(mcp trust? config 키?) 또는 Stage 3e 영속 모드.
- **리뷰 잠재 항목(저긴급)**: unsafe Send KiwiWrapper(libkiwi 스레드모델 검증) · session_bus unbounded_channel · snapshot_json unwrap_or_default. opencode Write/search_db 미배선(알려진 갭).
- 검색: 프로덕션 코퍼스 확보 후 재측정(그때 리랭커 재검토, 로컬 GPU RTX3060Ti 가능).

## ⑥ 다음 세션 첫 행동

1. 이 문서 + context-notes.md + checklist.md + docs/design/v2-A2A-core-backend_2026-06-30.md 읽기. `cargo test` + `--features "mcp sqlite morphology"` 상태 확인(Bash 툴).
2. Stage 3a-3(front=core)로 원격 코어 e2e 완성, 또는 사용자 지정.
3. 위임 Sonnet + Opus 리뷰·라이브검증. 굵직한 결정 재론 금지(설계/decision은 design doc·context-notes에 정본).
