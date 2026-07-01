# tunaRound v2 핸드오프 - 2026-07-01 Windows 세션 4

> 이전: [session3](v2-handoff_2026-06-30_session3.md)(half-a2a Stage 0~3a). 이 세션 = **Stage 3a-3(front=core) + Stage 3d(원격 쓰기 권위) + 시간성·유효성 로드맵 step 2~8**.
> **완전 콜드 스타트용**(다음은 `/exit` 후 새 파일 세션). 대화 맥락 없음 가정하고 이 문서만으로 이어갈 수 있게 씀.

## ⓪ 가장 먼저 할 것

1. 이 문서 + `context-notes.md`(하단 최신 노트) + `checklist.md` + `docs/plans/index.md` 읽기.
2. **cargo는 Bash 툴로**(PowerShell 아님) 상태 확인:
   - `cargo test` (기본, morphology+sqlite) → **160 pass 기대**.
   - `cargo test --features "semantic morphology mcp serve"` → **174 pass 기대**.
   - `cargo clippy --features "semantic morphology mcp serve"` → 클린 기대.
3. `git log --oneline -14` 로 이번 세션 커밋 확인. **origin/main = 3071281 (전부 푸시됨).** 워킹트리 클린(untracked `.omc/`·`docs/plans/v2-18~20.md`는 세션2 잔여, 무관).

## ① 이 세션이 한 것 (한 줄)

원격 코어를 완성(3a-3 단일 프로세스 + 3d 원격 쓰기)하고, 검색/맥락 아키텍처 리뷰를 거쳐 **시간성·유효성**을 SQLite로 흡수(로드맵 step 2~8). 전부 라이브/테스트 검증 후 푸시.

## ② 커밋 요지 (14개, 순서대로)

**Stage 3a-3 (front=core 단일 프로세스)**
- `c08ad62` `--core <addr>`: REPL이 자기 안에서 in-process HTTP MCP 코어를 띄움. 로컬 좌석이 loopback URL로 pull. `mcp::core_local_url`(0.0.0.0/[::]→127.0.0.1). `build_http_mcp_backends` 헬퍼로 serve 두 분기 공유.
- `14f9ab2` 라이브 e2e 문서.

**Stage 3d (원격 쓰기 권위, Plan 27 옵션 B = front=core 병합)**
- `d90d867` T1: `SqliteStore::append_turn`(증분 INSERT, **DB가 id 권위**=max+1, 전량 교체 아님 → 외부 writer와 REPL이 충돌·클로버 없음) + `orchestrator::TranscriptWriter`.
- `c28561d` T2: MCP `post_turn`/`get_roster` 툴 + `TunaSearchServer`에 writer/roster 필드 + `serve_http_mcp_on_listener`/`start_http_mcp_server` 시그니처 확장(writer, roster 인자). HTTP 통합테스트(reqwest 핸드셰이크).
- `f500840` T3: REPL **core-sync 병합**. `orchestrator::CoreSync` 트레잇(load_session+append_turn) + `store::SqliteCoreSync`. Session.core_sync. `step()` 시작에 `adopt_from_core`(외부 post 흡수), `append_round`가 core-sync면 append_turn으로 쓰고 다시 adopt(전량 persist 생략).
- `8a80cfe` T4: main `--core` 배선. participants 빌드 후 서버 spawn(로스터 주입) + REPL에 SqliteCoreSync 연결 + seed→코어DB 권위 반영(`Session::to_stored`). **라이브 e2e 통과.**
- `3d0d726` 문서 + 서버 호스팅 교훈.

**시간성·유효성 로드맵 (외부 memory 프레임워크 리뷰 후 확정, 정본 [docs/design/v2-temporal-validity-direction_2026-07-01.md])**
- `ec4ba0f` step 2(실버그): 임베딩 무효화 키에 `model_id`. `Embedder::model_id()`(Mock=`mock-{dim}`, Ollama=`ollama:{model}`) + `message_vectors.model_id`(스키마 **v3**). index_vectors skip은 (content_hash AND model_id) 일치 시만 → 모델 교체 시 재임베딩(전엔 content만 해싱=stale 조용히 skip).
- `065318d` step 3: retrieved 길이 cap(`MAX_RETRIEVED_CHARS=2000`) + session diversity cap(`cap_per_session_backfill`, over-fetch 4배, max 2/세션, **단일 세션이면 backfill로 불변**).
- `fb68aea` step 4: **별도 `message_validity` 테이블**(valid_state DEFAULT active / superseded_by_msg_id / abstraction / anchors, 스키마 **v4**). StoredMessage 컬럼 추가 안 함(리터럴 붕괴·직렬화 하위호환 회피 + Memora식 원문/메타 분리). `store::Validity` + set_validity/set_annotation(COALESCE 부분갱신)/get_validity.
- `7fe9715` step 5: 유효성 인지 랭킹. `rerank`(rejected 드롭·superseded/stale 강등) + `/supersede <id> [<대체id>]`·`/reject <id>` 커맨드(HITL) + `ValiditySink`/`SqliteValiditySink`.
- `08d1d5e` step 5b: 분기/세션 인지 랭킹. `ContextRetriever::retrieve_ctx`(default 위임) + penalty 통합(현재 세션 off-branch +1). repl이 retrieve_ctx 호출.
- `8acd3fb` step 7: `/explain <질의>` 검색 디버그(`debug_retrieve`: 토큰화·bm25·valid_state·cur-session 표시).
- `3071281` step 8: `--reindex` 서브모드(모든 세션 FTS·벡터 재생성) + `list_sessions`/`index_stats`.

## ③ 현재 상태 / 검증

- **기본 160 / features(semantic morphology mcp serve) 174 pass, clippy 클린(전 조합).**
- 스키마 버전 **v4**(v2→v3=model_id, v3→v4=message_validity). 마이그레이션 테스트 있음(수동 v2 스키마→ALTER 검증).
- Stage 3d 라이브 e2e 통과(아래 재현법). `--reindex` 스모크 통과.
- 백엔드(측정용): Ollama 터널(SSH [사설호스트]:[사설포트] → 11435, bge-m3 dim 1024), Redis 6379. 3d/3a-3/랭킹엔 불요.

## ④ ⚠ 중요 교훈 (다음 세션 반드시 참고)

- **`--core` 서버 호스팅**: 메인 스레드가 동기 블로킹 REPL(std stdin)이라, **공유 tokio rt에 서버를 spawn하면 accept 루프가 유휴 중 간헐적으로만 구동돼 신뢰 불가**(실측: 유휴 4s UP, 6s/8s down). **해결 = 서버를 전용 OS 스레드의 자체 런타임 block_on으로 서빙**(main.rs `--core` 블록 참조). 헤드리스 `--serve-mcp`는 메인 block_on이라 정상.
- **디버깅 타이밍 함정**(e2e "멈춤"으로 오인했던 것들): (1) Kiwi 토크나이저 init로 서버 기동 ~3초 → 고정 `sleep 3` curl이 경계 레이스. (2) FIFO `printf >&9`가 즉시 flush 안 됨 → agent 라인이 close까지 지연. (3) 2-에이전트 라운드 ~35초라 짧은 타임아웃이 자름. → **서버 준비 폴링** 후 호출, agent는 **파이프 입력** + 넉넉한 타임아웃(300s).

## ⑤ 남은 항목

**로드맵 잔여**
- **step 6 (실코퍼스 regression)**: 실제 tunaRound 전사 코퍼스 + gold 라벨 확보가 선행. **코드만으로 불가** — 동구님이 실제 세션 데이터 지정해야 착수. 현재 코퍼스는 합성(tests/search_recall.rs, 40발언/21질의)라 대표성 약함.
- **step 5c (recency 가중)**: cross-session recency는 타임스탬프 필요. msg_id는 세션별이라 비교 불가 → `messages`에 `created_at` 컬럼 추가 후 랭킹에 반영.
- **abstraction/anchors 생성 파이프라인**: message_validity에 컬럼은 있으나 채우는 로직 없음(에이전트 요약/앵커 추출). set_annotation은 준비됨.

**Stage 3d 후속(저긴급)**
- codex bearer-env: codex 원격 HTTP MCP 인증 접속(ExecSpec env 필드). 현재 codex는 bearer 헤더 미배선(claude만 원격 인증). runner/codex.rs `with_search_url` TODO.
- post_turn 인가: 현재 토큰만 있으면 누구나 씀(단일 테넌트라 OK, 다자 시 필요).
- --core + resume 엣지: seed→DB 권위 반영은 구현했으나 미검증(--core에 state 파일 동시 지정 시).

**이전 세션 백로그(잔존)**
- codex pull 활성화(codex exec가 MCP 도구 승인 막음 → 현재 pull=claude 전용, codex=push 폴백). 심층 조사 또는 codex app-server 경로.
- 잠재 리뷰: unsafe Send KiwiWrapper(libkiwi 스레드모델) · session_bus unbounded_channel · snapshot_json unwrap_or_default. opencode Write/search_db 미배선.
- Kiwi 런타임(libkiwi 404)→lindera 실효; Windows는 Kiwi cfg 제외라 lindera만(무관). Kiwi v0.22.2 수동 설치=scripts/install-kiwi-windows.sh.

## ⑥ 다음 세션 첫 행동 (권고 순)

1. ⓪ 상태 확인 후, 동구님 지정 방향으로 착수.
2. **코드 가능한 남은 것**: 5c(recency, created_at 컬럼) · abstraction/anchors 생성 · codex bearer-env · 잠재 리뷰 항목. 자체 완결이라 바로 가능.
3. **step 6(실코퍼스)**: 동구님이 실제 전사(예: seCall MCP의 tunaRound 세션들)를 코퍼스로 지정하면 gold 라벨링 + regression 하네스 확장.
4. **규율**: 비trivial 작업 전 plan + checklist·context-notes(규율 #7). 위임 Sonnet + Opus 리뷰. 검증(build/test)과 commit/push 분리. **cargo는 Bash 툴.** 새 소스 첫 줄=역할 한국어 주석(#6). 한국어 문장 끝 마침표(#5). em-dash 금지.

## ⑦ 핵심 파일 지도 (이 세션 신규/변경)

- `src/main.rs`: `--core`(전용 스레드 서버+core-sync+로스터+seed 반영) · `--reindex` · `--serve-mcp`(헤드리스) · `build_http_mcp_backends`/`build_index_tokenizer` · validity_sink 배선.
- `src/mcp.rs`: `TunaSearchServer`(search_context/read_transcript/**post_turn/get_roster**) · `serve_http_mcp_on_listener`(writer/roster) · `core_local_url`.
- `src/store/sqlite.rs`: 스키마 v4(message_validity) · `append_turn` · `set/get_validity`·`set_annotation` · `index_vectors`(model_id 키) · `list_sessions`/`index_stats` · migrate.
- `src/store/retriever.rs`: `SqliteRetriever`(retrieve_impl+rerank penalty+finish+**debug_retrieve**) · `SqliteTranscriptReader/Writer` · `SqliteCoreSync` · `SqliteValiditySink`.
- `src/store/mod.rs`: `Validity` 구조체 · `cap_per_session_backfill`.
- `src/store/embedding.rs`: `Embedder::model_id`.
- `src/orchestrator/mod.rs`: `TranscriptWriter`/`CoreSync`/`ValiditySink`/`RosterSeat` 트레잇+타입 · `ContextRetriever::retrieve_ctx`/`debug_retrieve`(default).
- `src/repl/mod.rs`: Session(core_sync/validity_sink 필드+빌더) · `adopt_from_core` · append_round core-sync 분기 · `retrieve_for_from_path`(길이 cap+retrieve_ctx) · 커맨드 Supersede/Reject/Explain · `mark_validity`.

## ⑧ 라이브 e2e 재현법 (Stage 3d, 참고)

serve 빌드 후(`cargo build --features serve`), 단일 `--core` 프로세스에 원격 post_turn 주입 → REPL 흡수 → claude 인용:
1. `( sleep 16; printf '@claude read_transcript로 전사 확인해 직전 발언자 결론을 키워드 포함 인용해줘.\n' ) | tunaround --core 127.0.0.1:<port> --db <db> --token <T> --pull-context` (백그라운드).
2. **서버 준비 폴링**(`/dev/tcp` 연결 시도 반복) 후 MCP 핸드셰이크: initialize→mcp-session-id 캡처→notifications/initialized→tools/call post_turn.
3. sleep 16에 @claude 라운드 발화 → adopt_from_core가 post 흡수 → claude가 in-process 코어 read_transcript로 인용. (스크립트는 세션4 대화 로그 참조; 넉넉한 타임아웃 필수.)

## 무엇을 만드나 (불변 요약)

터미널에서 사람이 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론 도구. 같은 레포 위에서 토론하고 결론을 결과 문서로 자동 기록. 검색/맥락은 형태소 FTS + 벡터 RRF + RAG 주입 + MCP 능동검색 + 유효성 인지 랭킹(SQLite-light). 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md]. 방향 정본: [docs/design/v2-A2A-core-backend_2026-06-30.md](A2A) + [docs/design/v2-temporal-validity-direction_2026-07-01.md](시간성·유효성).
