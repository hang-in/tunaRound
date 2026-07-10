# tunaRound 구현 체크리스트

> 규율 #7. task 완료 시 체크. plan 전문은 docs/plans/.

## Plan 01: 스캐폴드 + Codex 러너 (docs/plans/v1-01-agent-runner.md)

- [x] Task 1: 스캐폴드 + 도메인 타입(RunInput·RunOutput·RunMode·RunError) + Runner trait (5330063)
- [x] Task 2: dedup 순수함수 (c9628e8)
- [x] Task 3: Codex JSONL 파서 순수함수 (f2872b4)
- [x] Task 4: Codex argv 빌더 (c1a5a41; 실측 교정: --full-auto 없음 -> Write=`--sandbox workspace-write`, ReadOnly=`--sandbox read-only`)
- [x] Task 5: CodexRunner 통합 (가짜 CLI fixture) (e7949f9) — Plan 01 완료, 10 테스트 green, build/clippy 클린

## Plan 02: Claude 러너 (docs/plans/v1-02-claude-runner.md) — feat/v1-claude-runner

- [x] Task 1: claude argv 빌더 (80ca2cb; --help 실측: 가정 플래그 전부 확인)
- [x] Task 2: stream-json 파서 + RunError::Agent (032e550; 전체 스위트 green, codex 무영향)
- [x] Task 3: ClaudeRunner 통합 (2b18382) — Plan 02 완료, 17 테스트 green, build/clippy 클린

## Plan 03: 토론 오케스트레이터 (docs/plans/v1-03-orchestrator.md) — feat/v1-orchestrator

- [x] Task 1: 역할 지시문 (roles) (3a13954)
- [x] Task 2: 라운드 프롬프트 조립 (순차-인지) + Participant/Utterance (123ee5d)
- [x] Task 3: run_round + RunnerRegistry (c9af140) — Plan 03 완료, 24 테스트 green, build/clippy 클린

## Plan 05: thin REPL (docs/plans/v1-05-repl.md) — feat/v1-repl (돌아가는 앱, Plan 04보다 먼저)

- [x] Task 1: 명령 파싱 (e35683d)
- [x] Task 2: render + Session.step (d5e3dfc; 5 repl 테스트 green)
- [x] Task 3: main.rs 실 러너 REPL (10dda04) — Plan 05 완료, 돌아가는 앱, 스모크 통과, 29 테스트 green

## Plan 04: 전사 영속 (docs/plans/v1-04-persistence.md) — feat/v1-store

- [x] Task 1: store 타입 + 트리-ready 변환 (21dbfc5)
- [x] Task 2: JSON save/load 라운드트립 (a5456fd; 32 테스트 green)
- [x] Task 3: Session resume + main 상태파일 인자 (1cc75bf) — Plan 04 완료, 33 테스트 green, resume 스모크 통과

## Plan 06: Hardening (docs/plans/v1-06-hardening.md) — feat/v1-hardening

- [x] Task 1: /conclude synthesizer 종합 (464bf37)
- [x] Task 2: @engine 자리 지목 (0c4b282) — Plan 06 완료, v1 완료, 38 테스트 green

## v2 Plan 01: idle watchdog (docs/plans/v2-01-idle-watchdog.md) — feat/v2-idle-watchdog -> main

- [x] Task 1: 공유 watchdog 헬퍼 src/runner/exec.rs + RunError::Timeout (3414cf2)
- [x] Task 2: 양 러너를 watchdog 헬퍼로 배선 (idle_timeout 필드, 기본 600s) (78dd033) — Plan 01 완료, 43 테스트 green, build/clippy 클린

## v2 Plan 02: N좌석 로스터 (docs/plans/v2-02-roster.md) — feat/v2-roster -> main

- [x] Task 1: src/roster.rs JSON 로스터 로더 (participants + registry) (af69db9)
- [x] Task 2: main.rs --roster 플래그 + examples/roster.json (bb23e22) — Plan 02 완료, 48 테스트 green, build/clippy 클린, 스모크 3종 통과

## v2 Plan 03: 협업 코딩 쓰기 지목 (docs/plans/v2-03-write-delegation.md) — feat/v2-write-delegation -> main

- [x] Task 1: run_round에 mode 파라미터 (behavior-preserving, 호출부 ReadOnly) (9c55b97)
- [x] Task 2: @engine! 쓰기 지목 (Command::Write + 파싱 + step Write 분기 + /help) (1ae8b49) — Plan 03 완료, 52 테스트 green, build/clippy 클린

## v2 멀티세션 (Redis=git-tree, 3 플랜) — 설계문서 확정, 사용자 GO

### Plan 04: Redis session_bus 포팅 (docs/plans/v2-04-session-bus.md) — feat/v2-session-bus -> main

- [x] Task 1: 의존성(tokio/redis/futures) + session_bus 포팅 (room->session, pure 테스트 2) (0783179)
- [x] Task 2: 라이브 Redis 왕복 통합 테스트 (#[ignore]) (86aa482) — Plan 04 완료, 56 테스트(49+2 ignored+5), build/clippy 클린. 리뷰 주석정리 11e1f52

### Plan 05: 세션 모델 (docs/plans/v2-05-session-model.md) — feat/v2-session-model -> main

- [x] Task 1: store 트리 순수함수(path_to_root/next_id/tree_summary) + StoredSession 저장 포맷 (7ded26d)
- [x] Task 2: Session 트리 리팩토링(messages+head, append-to-tree, 영속) (c9510fe)
- [x] Task 3: /branches + /checkout 분기 탐색 (5b25827) — Plan 05 완료, 63 테스트(61+2 ignored), build/clippy 클린

### Plan 06: Redis 멀티세션 통합 (docs/plans/v2-06-redis-integration.md) — feat/v2-redis-integration -> main

- [x] Task 1: session_bus snapshot 지원(set/get + snapshot_json fire-and-forget) (e72c867)
- [x] Task 2: Session 미러 통합(Option<bus>+session_id, append_round 미러, new_with_bus) (c46121c)
- [x] Task 3: main.rs tokio 런타임 + --observe(관찰) + --session(재개) + owner lease (eb470b8, 정리 389fe09) — Plan 06 완료, 66 테스트(63+3 ignored), build/clippy 클린
- [x] 라이브 검증(2026-06-30, 로컬 Redis): bus 3 #[ignore] / resume / observe / 3라운드 컨텍스트 유지 전부 통과. **버그 발견·수정**: 종료 시 마지막 snapshot 유실 -> 동기 flush (fix/v2-06-snapshot-flush, 50edea4)

## v2 Plan 07: 바운드 자동 교환 (docs/plans/v2-07-bounded-debate.md) — feat/v2-bounded-debate -> main

- [x] Task 1: /debate 파싱 (N턴, 기본 3, 최대 10) (c5b9339)
- [x] Task 2: Session::step 바운드 자동 교환 루프 (run_round N회) (01b8860) — Plan 07 완료, 69 테스트(66+3 ignored), build/clippy 클린

## v2 능동 검색/맥락 (북극성, secall 포팅) — docs/design/v2-context-memory-direction_2026-06-30.md

### Plan 08: 한국어 토크나이저 (docs/plans/v2-08-ko-tokenizer.md) — feat/v2-ko-tokenizer -> main

- [x] Task 1: 의존성(kiwi-rs/lindera, morphology feature) + lindera 경로 + factory (74f8771)
- [x] Task 2: Kiwi 경로 + 메인 백엔드 (kiwi-rs 컴파일 성공) (1059be8) — Plan 08 완료, 기본 66/morphology 72 pass, clippy 클린
- ⚠️ **Kiwi 런타임 부트스트랩 실패**(libkiwi v0.23.2 에셋 404) -> 현재 lindera 폴백 실효. 해결 후속(kiwi-rs 버전 핀/libkiwi 수동). Windows에선 Kiwi cfg 제외=lindera만이라 무관.

### Plan 09: SQLite 시스템오브레코드 + FTS5 (docs/plans/v2-09-sqlite-fts.md) — done (격리 모듈 우선 + sqlite feature, 사용자 확정 2026-06-30)

- [x] Task 1: 의존성(rusqlite 0.31 bundled, sqlite feature) + 스키마/마이그레이션 + 세션 저장/로드 라운드트립(트리+head) (c61cf11; Sonnet 위임, Windows rusqlite bundled 컴파일 21초 OK)
- [x] Task 2: FTS5 선-형태소화 색인 + bm25 검색 + 테스트(코어 4 + sqlite+morphology 통합 1) (181f46a; **morpheme_indexing_matches_inflected_form 통과**: "검색을"->"검색" end-to-end 실증) — Plan 09 완료, sqlite 68/sqlite+morphology 75 pass, 기본 61 불변, clippy 양 조합 클린
- 비포함(다음 슬라이스): REPL/main 영속 SQLite 전환, Redis 스냅샷 조정, 검색 주입(RAG), 벡터/하이브리드
- 리뷰 노트(후속 폴리시): load_session head 조회 `.ok()`가 실DB에러도 삼킴(QueryReturnedNoRows만 None 처리 권장) · 트랜잭션은 unchecked_transaction()이 더 관용적

### Plan 10: SQLite 라이브 배선 (docs/plans/v2-10-sqlite-wiring.md) — done

- [x] Task 1: MessageIndexer trait + SqliteIndexer(sqlite feature) + Session indexer 배선(append_round 훅) + tokenize_fallback 비게이트화 (e21cf43; Sonnet 위임. Rc→Arc<Mutex>, Connection !Sync라 Mutex<SqliteStore>)
- [x] Task 2: main --db 배선(3분기 일관 전달, feature-gated) + 색인 roundtrip 테스트(persist->재오픈->search) (5d79a0a; Sonnet 위임) — Plan 10 완료, sqlite 74/sqlite+morphology 81 pass, 기본 불변, clippy 3조합 클린, 스모크 OK. **origin 푸시됨**(README와 함께)
- 패턴: SessionBus 미러 답습(Option 필드 + append_round). 추가적(JSON/Redis 미접촉), sqlite off=None=불변. 검색 소비(RAG)는 Plan 11.

### Plan 11: 검색 주입 RAG (docs/plans/v2-11-rag-injection.md) — done

- [x] Task 1: build_round_prompt retrieved 슬롯 + ContextRetriever trait + run_round 배선(동작 불변, retrieved=&[]) (b0dd7bd; Sonnet)
- [x] Task 2: SqliteRetriever + Session retriever(with_retriever 빌더) + retrieve_for(활성경로 dedup, K=5) + main --db 읽기 배선 (4643977; Sonnet) — Plan 11 완료, sqlite 76/sqlite+morphology 83 pass, 기본 불변, clippy 3조합 클린, 스모크 OK. **cross-session 검색 단위 테스트 통과**(능동 검색 실연)
- 방식: 추가적(활성 경로 밖 다른 분기·과거 세션 맥락만 검색 주입). prior 캡(재주입 축소)은 품질 측정 후 별 슬라이스. 미푸시.

### Plan 12: /search 명령 (docs/plans/v2-12-search-command.md) — done (벡터 보류, 정렬 슬라이스. 사용자 확정 2026-06-30)

- [x] Task 1: Command::Search 파싱 + step 핸들러(retriever 재사용, 없으면 안내) + /help (bc2f359; Sonnet) — Plan 12 완료, 기본 70/sqlite 79/sqlite+morphology 86 pass, clippy 3조합 클린. 신규 의존성 0
- 목적: 사람이 인덱스 직접 검색 -> FTS 품질 관측 -> 벡터(원안) 도입 YAGNI 근거 수집. 미푸시.
- **벡터 보류 근거:** 설계 YAGNI(FTS 부족 입증 시에만). 라이브 블로커는 해소(원격 Ollama 2232 검증, bge-m3 dim 1024).

### Plan 13: 벡터 임베딩 + 하이브리드 (docs/plans/v2-13-vector-hybrid.md) — done (사용자 요청 2026-06-30, 블로커 해소 후)

- [x] Task 1: Embedder trait + MockEmbedder + OllamaEmbedder(reqwest blocking, semantic feature) (1ad8881; Sonnet, reqwest rustls Windows 22s) — **라이브 검증: ollama_embed_live dim 1024 ok**(터널 11435)
- [x] Task 2: message_vectors(schema v2, f32 LE BLOB) 증분 색인(content_hash 가드) + cosine 벡터 검색 (30efa51; Sonnet)
- [x] Task 3: RRF 하이브리드(k=60, secall 답습) + indexer/retriever/main embedder 배선 + get_message (8920027; Sonnet) — Plan 13 완료, sqlite 86/semantic 86 pass, 기본 불변, clippy 클린, 스모크 OK
- embedder 없으면(semantic off/--db 없음) FTS 단독=불변. ANN 미도입(brute-force cosine, YAGNI). 라이브 의미 품질은 실사용 측정.

### Plan 14: 에이전트 능동 검색 도구 MCP (docs/plans/v2-14-agent-search-mcp.md) — Task 1·2 done, Task 3 라이브 대기 (사용자 선택 2026-06-30)

- [x] Task 1: rmcp search_context 서버(SqliteRetriever 래핑) + main --mcp-search stdio 모드 (a65feba; Sonnet) — **rmcp Windows 빌드 OK**(1.3.0->1.8.0, 10초). ContextRetriever에 Send+Sync 추가. mcp 88 pass
- [x] Task 2: claude --mcp-config 배선(self-exe를 --mcp-search --db로 spawn, serde_json 조립) + with_search_db + main cfg(mcp) (a5a185d; Sonnet) — mcp 89 pass, clippy 클린
- [x] Task 3: 라이브 검증(2026-06-30) — **실 claude+codex 라운드 정상 응답(gotcha #4 수정으로 codex spawn OK)** → SQLite 색인 → **MCP 서버 직접 JSON-RPC(initialize+tools/call)로 search_context("발제자")가 실 색인 발언 반환**. 전 체인 입증. 에이전트 자율호출은 모델 행동(별도). morphology/semantic 빌드면 형태소+벡터 품질↑
- [x] Task 4(codex): codex에 -c mcp_servers 오버라이드 배선(claude와 동형, TOML 리터럴로 Windows 경로 안전, 영속 config 미변경) (c892548; Sonnet) — 기본 77/mcp 95 pass, clippy 클린. **라이브: codex가 -c MCP 인자 받고도 정상 응답(비파괴 확인)**. 이제 두 자리 모두 search_context 보유
- 단일 툴 search_context. 로스터·다중툴(recall/get)은 후속.

## Plan 15: 러너 Windows CLI 해석 (docs/plans/v2-15-windows-cli-resolve.md) — done (gotcha #4)

- [x] Task 1: exec.rs resolve_bin(Windows PATH .exe/.cmd/.bat/.com 풀경로화) + run_with_watchdog 배선 (8d02088; Sonnet) — 기본 74/전체 99 pass, 기존 .cmd 픽스처 무영향(확장자 있으면 no-op), clippy 클린. **라이브: codex(codex.cmd)가 실제 spawn돼 응답 확인**

### Plan 16: 재주입 축소 (docs/plans/v2-16-reinjection-cap.md) — done

- [x] Task 1: Session.recent_turns + prior_for_prompt(최근 N턴 캡) + step 5곳 배선 + main --recent-turns (2834a1d; Sonnet) — opt-in(기본 None=현행 통째 재주입 불변), 기본 76/전체 101 pass, clippy 클린. 북극성 스케일 페이오프(통째 재주입 -> 최근 N턴 + 검색 슬라이스)
- 후속: ctx-handle/요약 carry-forward, 토큰예산 동적 캡, 기본화는 품질 측정 후.

### Plan 17: HTTP 엔진 러너 (docs/plans/v2-17-http-engine-runner.md) — done

- [x] Task 1: OpenAI 호환 HTTP chat 러너(`runner/http.rs`, pure builder/parser + Runner) + 로스터 SeatConfig(base_url/model/api_key_env) + build_registry HTTP 분기 (e1373f9; Sonnet) — engines feature(reqwest). 기본 101/engines 106 pass, no-default 빌드 OK, clippy 클린. **라이브: Ollama /v1/chat/completions(gemma4:e2b) 응답 확인 = 로컬 LLM 좌석 동작**
- 한 러너로 ollama·ollama cloud·lmstudio·openai 커버(engine 이름이 키라 다모델 다좌석). HTTP 좌석은 레포 직독 없음(프롬프트 맥락만). opencode CLI 러너 + HTTP 좌석 search_context는 후속.
- UI(리치 프론트)는 보류 결정(2026-06-30): 코어 아닌 폴리시. 필요 페인(분기트리/observe/맥락투명성) 입증 시 경량 ratatui.

### Plan 18: FTS 리콜 보강 (docs/plans/v2-18-fts-recall.md) — done

- [x] Task 1: raw 토큰 색인(fts_index) + prefix 질의(fts_query) + index/query 클로저 분리 (45cf0c8; Sonnet) — 측정으로 발견한 lindera 외래어 누락("임베딩") 메움(재측정으로 #3 히트 확인). 기존 "검색을→검색" 보존. 기본 103/전체 105 pass, clippy 클린. 품질 게이지 tests/search_quality.rs(#[ignore])

### Plan 19: Windows Kiwi 활성화 (docs/plans/v2-19-enable-kiwi-windows.md) — done

- [x] Task 1: Windows cfg 허용 + Kiwi keep-tags base 매칭(VA-I 등 변종) + install 스크립트/문서 (fe0ec71; Sonnet) — **Kiwi v0.22.2가 Windows에서 작동(검증).** 규명: kiwi-rs 0.1.4 auto-download 깨짐(토큰 무관)·latest v0.23.2 ABI crash → **v0.22.2 수동 libkiwi**(%LOCALAPPDATA%\kiwi, discovery 기본경로라 env 불필요), 미설치 시 lindera 폴백. 외래어 음절분할은 Plan 18 raw+prefix가 FTS 커버. 기본 103/전체 105 pass, clippy 클린. 바이너리 미커밋(scripts/install-kiwi-windows.sh로 설치)

### Plan 20: opencode CLI 엔진 러너 (docs/plans/v2-20-opencode-runner.md) — done

- [x] Task 1: OpencodeRunner(`opencode run --format json` JSONL 파싱: text.part.text=본문, step_finish.part.tokens=토큰) + 로스터 engine "opencode"(seat.model) (7fedac2; Sonnet) — claude/codex 패턴 답습, 신규 의존성 0, gotcha #4 resolve_bin이 opencode.cmd spawn. ollama cloud 검증(cold start로 idle 600s). 기본 105/전체 112 pass, clippy 클린
- **신규 엔진 완성**: HTTP(Plan 17: ollama/lmstudio/openai) + opencode CLI(Plan 20). 한계: opencode ReadOnly 샌드박싱 후속(플래그 불명확).

### 후속 (검색 레이어 폴리시)
- [x] load_session `.ok()` 에러 삼킴 보정(QueryReturnedNoRows만 None, 나머지 전파) + indexer let-chain clippy 정리 (cd7e4e5)
- [~] indexer/retriever 토크나이저·embedder Arc 공유 — **백로그(저가치)**: 중복은 startup 1회 인스턴스뿐, 라운드당 추가비용 없음. 시그니처 3곳 churn > 메모리 1회 절약. 보류.

## v2 (A) 코어-백엔드 (docs/design/v2-A2A-core-backend_2026-06-30.md) — 사용자 확정 2026-06-30

> 상주 코어 + 접속 클라이언트(사람 운전). 핵심 솔기 = turn-policy(HumanDriven 디폴트, AutoLoop=미래 (B) 플러그인). Stage 0~4.
- [~] Stage 0: 코어 서비스 경화 — 검색 품질(항목1) + 요약 carry-forward(항목2). **이번 세션 착수.**
  - [x] Plan 21 검색품질: 현실코퍼스+recall/MRR 하네스(tests/search_recall.rs) + AND→OR 개선. **R@5 0.55→0.90, MRR 0.60→0.90** (미커밋, 리뷰 완료). docs/plans/v2-21-search-quality.md
  - [x] precision@k 하네스 추가(`30543fb`): mean P@3=0.70 P@5=0.727, K=5 정당화(정밀도 손실 없이 리콜↑), 양면 회귀 가드
  - [x] Plan 22 요약 carry-forward(항목2)(`590ae83`): carry_forward_digest(드롭 턴 압축 이월, MAX_CARRY=1500, UTF-8 안전) + 예약 슬롯 주입, opt-in. 신규 6테스트
- [ ] 품질 트랙(Stage 0 후속, 측정-증분): ChromaDB/GRPO out, 리랭커+쿼리확장 in
  - [x] eval 코퍼스 확대: 20→40발언, 10→21질의(어휘·의미공백 포함). R@5 0.857/P@5 0.592/MRR 0.833. 리콜공백(Q6/16/17/21)=벡터·확장 영역, 정밀도noise=리랭커 영역 분리 확인
  - [x] 벡터/하이브리드 측정(vector_hybrid_recall, 터널): FTS R@5 0.857→**벡터 0.952**, MRR 0.976. 공백 회복(Q16/17/6/21). **결론: 쿼리확장 YAGNI 확정 + 리랭커 보류**(MRR 0.976=gold 1순위, 재정렬 이득 미미). 측정이 두 기능 도입 취소
  - [~] 검색 품질 트랙 = 현 eval 충분. 프로덕션 코퍼스 확보 후 재측정(그때 리랭커 재검토)
- [~] Stage 1: 오케스트레이션 툴(read_transcript/get_roster/post_turn) 기존 rmcp 서버 확장
  - [x] read_transcript(Plan 23): TranscriptReader 트레잇 + SqliteTranscriptReader(Mutex) + MCP 툴 + main 배선. 기본 111/mcp+sqlite 119 pass, clippy 클린(미커밋→리뷰완료). 세션 id=파라미터/기본 default
  - [x] Task 2 세션 id 주입: 러너 with_search_session → MCP spawn에 --session-id, main이 현재 sid 전달, 서버 default_session 보유. behavior-preserving(빌더 미호출 시 불변). 기본 113/mcp+sqlite 119 pass, clippy 클린
  - [ ] 후속: get_roster · post_turn
- [~] Stage 2: 주입 모델 전환(push->pull), 재전송량 감소 실측 (crux)
  - [x] 설계(Plan 24): Push/Pull 모드, 좌석 능력 게이트(비MCP→push 폴백), 포인터+carried/same_round 유지, 통제 리스크 완화, 측정=페이오프 증명. 구현은 승인 후
  - [x] Task 1 메커니즘: ContextMode(Push/Pull) + is_mcp_capable + build_round_prompt pull 분기(포인터, prior/retrieved 생략) + --pull-context(--db 없으면 경고+Push) + 프롬프트 크기 계측([ctx]). behavior-preserving(기본 Push). 기본 118/mcp+sqlite 124 pass, clippy 클린
  - [x] Task 2 라이브 측정(실 claude/codex, 3턴): **토큰 페이오프 증명**(pull 평평: claude 9770→429 95%↓, codex 12489→2417 81%↓, 전사길이와 탈동조). **블로커 발견**: read_transcript가 헤드리스 `-p` 권한모드서 차단(claude 응답에 "read_transcript 권한이 막혀" 명시)→에이전트가 레포/사전지식으로 보충(전사 grounding 아님). pull 아직 프로덕션 불가
  - [x] **Task 3(블로커 해소)**: claude ReadOnly에 `--allowedTools mcp__tuna-search__{search_context,read_transcript}`(MCP일 때만, 쓰기차단 유지=fail-safe). codex는 exec 비대화형이라 자동승인=수정 불필요. **재측정 검증: "권한 막힘" 사라짐 + 두 에이전트가 전사 실제 인용("합의 요약" 과제 정확 수행) + 프롬프트 평평 유지**. Stage 2 작동 검증 완료
- [x] **Stage 2 검증 완료**: push→pull 페이오프 실증(토큰 80~95%↓·전사길이 탈동조 + grounding 유지). half-a2a 척추 작동
- [~] Stage 3: 코어 프로세스 분리(상주 데몬 + 멀티 프론트/세션)
  - [x] 설계(Plan 25): 린치핀=코어를 HTTP MCP 서비스로 상주. 실측 확정(claude --transport http / codex --url + bearer / rmcp streamable-http). 분해 3a(HTTP MCP 상주)→3b(토큰)→3c(Tailscale)→3d(post_turn/get_roster)→3e(영속 에이전트 보류). 구현 승인 후
  - [x] 3a-1 HTTP MCP 서브 모드: `--serve-mcp <addr> --token` + rmcp StreamableHttpService(axum 마운트) + bearer 미들웨어(401) + serve feature(axum 격리). 기본 105 불변/serve 신규 2 pass, clippy 클린. disable_allowed_hosts(원격 허용, bearer가 인증)
  - [x] 3a-2(502e458): 러너 with_search_url(URL+bearer) → 에이전트가 원격 HTTP MCP 접속. claude HTTP config, codex url(bearer-env TODO). **라이브 e2e: 코어 --serve-mcp + 별도 REPL claude가 원격 HTTP로 read_transcript 전사 정확 인용 = remote core 동작**
  - [x] 3a-3: front=core 단일프로세스(Plan 26). `--core <addr>` = REPL+in-process HTTP MCP 코어(bind 동기 선행→rt.spawn 서빙→로컬좌석 search_url 자동배선→REPL). serve 두 분기 `build_http_mcp_backends` 헬퍼 공유, mcp.rs `core_local_url`+단위테스트. 기본 137/serve 146 pass, clippy 클린. **풀 라이브 e2e 통과**: 단일 프로세스로 실 claude+codex 2턴, claude(pull, 프롬프트 604자 포인터)가 in-process 코어에서 read_transcript로 자기 turn1 발언 verbatim 인용 = pull 확정. [ctx] claude 513/604(평평) vs codex 1511(push). half-a2a 척추 단일 front=core 라이브 동작
  - [x] 3d post_turn(쓰기 권위)+get_roster(Plan 27, 옵션 B front=core 병합): append_turn(증분, DB id 권위)+TranscriptWriter, MCP post_turn/get_roster, REPL core-sync(adopt+append, 클로버 차단), --core 배선. 라이브 e2e: 원격 post_turn→core-sync 흡수→claude 인용. 커밋 d90d867/c28561d/f500840/8a80cfe. 기본 142/serve 156. **서버는 전용 스레드 block_on 서빙**(공유 rt spawn은 유휴 중 간헐 신뢰불가)
  - [ ] 3a 잔여: codex bearer-env · post_turn 권한/인가 · --core+resume 검증 · 3e 영속 에이전트(보류)
- [ ] Stage 4(범위 밖): 영속 에이전트 세션 + AutoLoop = (B), 경제 조건 입증 시에만

## 시간성·유효성 로드맵 step 5c: recency 랭킹 (2026-07-01 세션5 완료, 정책 A=보수)
- [x] 스키마 v5: messages에 created_at TEXT(nullable) 추가. CREATE_MESSAGES + migrate(ALTER, column_exists 가드). 기존 행 NULL.
- [x] INSERT 경로 2곳에 created_at: append_turn=datetime('now') / save_session=기존 보존(DELETE 전 SELECT→맵→COALESCE(?, now))
- [x] 랭킹(정책 A=보수): rerank 2-pass. 다른세션 && created_at존재 && 후보최신 대비 7일 초과 히트만 +1. 현재세션·active·최신·NULL은 불변(relevance/validity 우선). parse_ts_approx 단조 파싱
- [x] get_created_at 헬퍼 + set_created_at(백필/테스트용). NULL=recency 판단 유보(강등 없음)
- [x] 테스트: migration_v4_to_v5_adds_created_at_nullable · save_session_preserves_created_at_on_resave · retrieve_demotes_stale_cross_session. 기존 랭킹 테스트 불변
- [x] 검증: 기본 163 / features 177 pass, clippy 클린(양쪽). 커밋 1aa0661 push됨
- [x] 라이브 검증(/explain 확장): debug_retrieve에 created_at + recency↓ 표시. 실 라이브러리 코드로 seed+8일aging 확인 - plumbing(save_session이 created_at 실제 채움) + /explain에 recency↓ 표시 + retrieve 순서 최신 우선. 신규 테스트 debug_retrieve_marks_stale_cross_session_recency
- [ ] 잔여: 유기적 recency(며칠 간격 실 다세션)는 step 6 실코퍼스와 함께

## 잔여 항목 배치 (2026-07-01 세션5 완료)
### A. 안전성/견고성 배치 (자체완결, 코드 검증)
- [x] #1 KiwiWrapper unsafe Send: SAFETY 주석 강화(직렬화 근거+잔여리스크=kiwi TLS 미확인+thread_local 비채택 이유+Windows 제외). 문서만
- [x] #2 session_bus unbounded→bounded: channel(CAP=1024) + enqueue()가 try_send(Full=drop+경고, Closed=무시). sync fire-and-forget·non-blocking 유지
- [x] #3 snapshot_json: 실패 시 eprintln 후 빈 문자열(빈 스냅샷 조용한 발행 방지)
### B. codex bearer-env (원격 인증 배선, TODO 제거)
- [x] ExecSpec.env 필드 + run_with_watchdog cmd.env(). claude/opencode/exec-test env: Vec::new()
- [x] codex.rs: build_mcp_wiring 추출(테스트 가능). url+token이면 `-c ...bearer_token_env_var="TUNA_SEARCH_TOKEN"` + env로 토큰 주입(argv 비노출). TODO 제거
- [x] 단위테스트: with_search_token_wires_bearer_env_not_argv · no_token_means_no_bearer_wiring + 기존 url 테스트를 build_mcp_wiring 직접 호출로 강화
- [x] 검증: 기본 160 / features 174 pass, clippy 클린. A/B 커밋 분리
- [ ] ⚠ 라이브 미검증: codex exec 승인 이슈로 codex MCP 도구 호출은 여전히 막힘(pull=claude 전용 결정). bearer는 인증 배선 완결이나 codex 도구사용 활성화는 별개(승인 심층조사 후속)
### C. abstraction/anchors 생성 파이프라인 → 보류(2026-07-01 세션5)
> 결정 B: 결정적(first_clause) 버전은 carry_forward_digest와 중복=저가치, 에이전트(LLM) 요약은 진짜 가치나 트리거·비용 설계 필요=큼. 실사용에서 "retrieved 주입이 무겁다" 신호 나오면 A(에이전트 요약)로 착수(YAGNI). 지금은 억지 결정적 버전 안 만듦.

### read-only 강제 수준 (2026-07-01 세션5 방침)
> 동구님: read-only는 하드(샌드박스)로 꼭 지킬 필요 없음. 프론티어 모델은 지시를 잘 따르고, 1년전 저성능 LLM/Gemini나 안 지킴. → codex bypass+지시(behavioral) posture 유지가 정답. codex app-server(Stage 3e)를 "하드 read-only" 목적으로 밀어붙이지 않음.

## codex pull 활성화 (behavioral read-only, 2026-07-01 세션5)
> 근거: codex exec는 read-only 샌드박스 유지한 채 MCP 승인 불가(업스트림 #24135). 유일 우회=--dangerously-bypass-approvals-and-sandbox(샌드박스 제거). codex는 규칙 준수가 강해 read-only를 지시로 강제 가능(동구님 통찰). 결정: 프롬프트 지시 주입 + pull+ReadOnly+MCP일 때만 발동.
- [x] is_mcp_capable에 codex 추가(claude|codex) + 테스트/주석 갱신
- [x] RunInput에 pull: bool + Default 파생(RunMode도 Default=ReadOnly). run_round가 per-seat pull로 채움. 나머지 리터럴은 ..Default::default()
- [x] codex: build_codex_args(input, mcp_args, bypass). ReadOnly && pull && (search_url|search_db) → `--dangerously-bypass-approvals-and-sandbox`(read-only 대체). Write=workspace-write, 비pull ReadOnly=read-only 유지
- [x] READONLY_DIRECTIVE를 bypass 시 codex 프롬프트에 prepend
- [x] 테스트: args_readonly_bypass_replaces_sandbox · is_mcp_capable(claude|codex) · 리터럴 갱신
- [x] 검증: 기본 161 / features 175 pass, clippy 클린
- [x] ⚠ 라이브 e2e 통과: 실 codex 0.142.5로 --serve-mcp 코어(seed=PELICAN/이벤트소싱) + 별도 codex-only pull REPL. codex가 tuna-search MCP 호출→전사 정확 인용("코드명 PELICAN", 프롬프트에 없던 것=실제 pull) + read-only 준수(파일 변경 0). "사용자 취소" 사라짐. [ctx] mode=pull 확인

## step 6 실코퍼스 regression (2026-07-01 세션5 완료, seCall 복구 후)
> 소스=seCall project=tunaRound 실 턴(06-30~07-01). semantic+한국어 keyword 복구(v0.6.4, 3142세션).
- [x] 실 턴 발췌: 6274470d:175(아키텍처리뷰)·37b034cb:2(캐시)·6274470d:89(코어)·dff85fb8(codex/recency). 18발언
- [x] tests/real_corpus_recall.rs: 실 발언 코퍼스(1발언=1논점) + 12질의(굴절·동의어로 변형) + R@k/MRR/P@k. 하드코딩(search_recall 패턴)
- [x] **측정: mean R@5 0.958 / P@5 0.621 / MRR 1.000** (합성 확장셋 0.857/0.592보다 높음). 유일 약점=재색인↔무효화 동의어(Q2 R@5 0.5)
- [x] 회귀 floor R@5>=0.85, P@5>=0.55 (lindera 결정적). 새 파일 clippy 클린
- [~] recency 유기 검증: step 5c 라이브 e2e로 이미 실증(별도 flaky 테스트 안 만듦). 실 날짜 코퍼스라 향후 확장 가능
- [x] ⚠ 라벨=Opus 판단(주관성)·18발언 소규모(검정력 한계)·발언이 주로 assistant 턴(문체 동질) 명시. 결론: 검색 스택이 실 한국어 설계토론 어휘서도 품질 유지 실증
- [x] **확장(seCall 패치 후, 07-02)**: 재수집이 드러낸 실 설계토론 세션(e5a848d3 리프레시토큰 논쟁=auth도메인)에서 5발언 추가→23발언/15질의(2도메인). 재측정 R@5 0.878/P@5 0.494/MRR 0.900, floor 0.80/0.42
- [x] **⚠ 실발견**: "리프레시 토큰 어디 저장" R@5 0.0 = 외래어 음역 갭(리프레시↔refresh) FTS 미대응. 개선후보=토크나이저 외래어 음역 정규화/영한 병기. 쉬운코퍼스가 숨긴 실패모드를 확장 실코퍼스가 노출

## 외래어 음역 병기 색인 (2026-07-02 세션5 완료, 93a2481)
> 근거: 실코퍼스 확장이 리프레시↔refresh 갭 노출, 하이브리드(임베딩)도 못 메움(반증 실측). 임베딩은 의역/동의어는 잇고 음역+영어term조밀만 못 이음 → 어휘층 alias 병기가 직접 해법.
- [x] search/mod.rs: LOANWORD_GROUPS(음역 32그룹, 소문자·모호단음절 제외) + loanword_aliases(token) + 단위테스트 3
- [x] tokenizer.rs fts_query(default trait): 질의 토큰별 alias 사후 추가(모든 백엔드 공유, index 무변경)
- [x] main.rs 비morphology fallback 2곳도 동일 확장
- [x] 재측정: "리프레시 토큰 어디 저장" R@5 0→1.0. FTS R@5 0.878→0.944, hybrid 0.933→0.978. 합성 0.857 불변. floor R@5>=0.88/P@5>=0.45
- [x] ⚠ 정밀도: P@5 0.494→0.508(↑), 대가는 MRR 소폭↓(OR확장, top-k주입 수용). 자동음역모델 비채택. 흔한공통어 alias는 과적합회피 위해 유지(재튜닝 여지)

## 배포·온보딩 (2026-07-02 설계 확정, docs/design/v2-deploy-onboarding_2026-07-02.md)
> 결정: 배포=cargo-dist(sshc 답습, homebrew+powershell, 풀피처 단일바이너리). scoop/winget 보류. 코어 홈랩호스팅 보류. 온보딩=clap 서브커맨드 + tunaround.toml 프로파일(진입선택). doctor 다음.
- [x] **Stage 1 clap 서브커맨드**(Sonnet5 위임+Opus 리뷰/검증): chat/core/serve/join/mcp-search/reindex. Cli{Option<Commands>}→None=Chat, cfg 게이트 variant, CommonSessionArgs flatten, match로 기존 지역변수 매핑(본문 246+ 불변). 러너 spawn `--mcp-search`→`mcp-search`(codex build_mcp_wiring·claude build_mcp_config 추출) + 테스트 갱신. clap 단위테스트 기본6/features9. 검증: 기본166+6 / features180+9 pass, clippy 클린(no-default 포함). README 예시 서브커맨드화. ⚠ bare `tunaround file.json`→이제 에러(chat file.json 필요, 설계 의도). 미커밋→리뷰 후 커밋
- [x] **Stage 2 cargo-dist 설정**(태그 미푸시=릴리스 안 나감): dist-workspace.toml(cargo-dist 0.31.0, installers shell/powershell/homebrew, 6타깃 mac/win/linux, tap hang-in/homebrew-tap, features semantic/mcp/serve) + .github/workflows/release.yml(dist generate). Cargo.toml에 description/repository/homepage. 검증: `dist generate --check` 동기 OK, `dist plan` v0.1.0 6바이너리+installer+formula 경고없이. **license 미정(동구님 결정)**. ⚠ 크로스컴파일 리스크(rusqlite bundled C·reqwest rustls ring, 특히 aarch64-linux)는 첫 릴리스 CI에서 확인
- [x] 라이선스 확정: **AGPL-3.0**(동구님 2026-07-02). Cargo.toml `license="AGPL-3.0-only"` + LICENSE(공식 전문 661줄). dist plan이 각 아티팩트에 LICENSE 번들.
- [x] 맥-윈도우 왕복 개발 핸드오프: docs/reference/dev-mac-windows.md(상시 참조, 사설 도메인 미포함).
- [ ] **Stage 2 릴리스(도그푸딩 후, 동구님 승인)**: 맥에서 git pull/clone로 빌드·실행 확인 → 며칠 사용 후 `git tag v0.1.0` 푸시 → 공개 Release + homebrew-tap 발행. 맥 brew install + Kiwi 자동다운로드 실기 확인
  - [x] **맥 검증 완료(2026-07-02)**: 빌드/테스트(195/212)/`cargo install`/E2E 도그푸딩/`dist plan`(6타깃)/미설치CLI graceful/크로스머신 A2A 스모크(claude leg ✅, codex pull ✗). 도그푸딩 판정=**v0.1.0-rc.1 먼저**. 상세: docs/reference/release-readiness-v0.1.0_2026-07-02.md
  - [x] **rc.1 CI 성공(2026-07-02)**: `v0.1.0-rc.1` 태그→릴리스 CI green(run 28564666085), 프리릴리스 생성(4타깃+인스톨러). rc.1이 CI버그 3개 잡음(버전=태그 / [profile.dist] / aarch64 ring→4타깃). homebrew=prerelease라 skip.
  - [x] **Windows rc 아티팩트 검증(세션6, 2026-07-02)**: win x64 sha256 일치·`--version`=0.1.0-rc.1·전 서브커맨드/피처(semantic/mcp/serve/sqlite) 컴파일 확인 = 바이너리 양호. **⚠ installer.ps1/sh/brew 익명 404 = 레포 private 탓(스크립트·아티팩트 정상)**: 공개 설치 경로는 레포 public 전환 후에만 작동. context-notes 세션6 참조
  - [ ] 다음(동구님, 배포 비우선): rc 아티팩트 맥/윈도우 설치검증(단 익명 installer/brew는 **레포 public 전환** 후에만 작동 = IP 히스토리 퍼지 선행) → homebrew-tap+시크릿 → Cargo.toml `0.1.0` 되돌림 + `git tag v0.1.0`(CHANGELOG "미발행" 헤딩 정리 후)
- [x] **Stage 3 tunaround.toml + 프로파일**(Sonnet5 구현): 신규 `src/config.rs`(Config/Profile serde, parse_config/load_config/discover_config_path, expand_home, resolve_search_token, select_profile 순수함수 + match_profile_pick 순수매칭/prompt_profile_pick stdin분리, MergedSessionArgs+merge_profile_into 순수병합). main.rs: CommonSessionArgs(chat/core 공유)+JoinArgs에 `--config`/`--profile` 추가, match 직후 profile_capable 게이트(chat/core/join만)로 병합 블록 삽입(db_path를 mut로 변경). pull_context는 OR 병합, 나머지는 CLI 우선. `tunaround.toml.example`(레포 루트, 플레이스홀더) + `.gitignore`에 `/tunaround.toml` 추가 + README "설정 프로파일" 섹션 + dev-mac-windows.md 갱신. 검증: 기본 184+6/풀피처 198+9 pass(신규 테스트 ~20개), clippy 3조합(기본/풀피처/no-default) 0경고. **미커밋(Opus 리뷰 대기)**
- [ ] Stage 4(다음) doctor: claude/codex·Ollama·Kiwi·포트·코어도달 프리플라이트
- [ ] 각 단계 cargo test(기본/features)+clippy, 커밋 분리

## semi-a2a 파트너 위임 Phase 1 (2026-07-02 세션6 설계, docs/design/v2-a2a-partner-delegation_2026-07-02.md)
> A2A 표준(Task 위임) 채택. 중앙 브로커: 코어=A2A서버+큐, worker=/loop+inbox MCP툴 폴링, dispatcher=SendMessage/GetTask. worker=CLI 에이전트(모델=config). 상세 결정 context-notes 세션6(후반).
- [x] Task 1: A2A 데이터모델(Task/TaskState/Message/Part/Artifact serde) + tasks 테이블(스키마 v6) + store ops(create/get/list_open_for/update_state/complete/append_history) + 마이그레이션 + 라운드트립·상태전이·필터·마이그레이션 테스트. 격리 store 모듈(src/store/a2a.rs), sqlite 게이트. **완료(Sonnet 구현+Opus 리뷰): lib +19 test green(203 기본/217 풀피처), clippy 클린.** 리뷰노트: Artifact=A2A스펙(artifact_id/name/parts) · timestamp create=호출자/update=datetime('now')(dispatcher는 SQLite호환 포맷) · wire camelCase는 Task 2 요건.
- [x] Task 2: A2A 서버 엔드포인트(SendMessage/GetTask/CancelTask JSON-RPC + /.well-known/agent-card.json) 코어 axum(serve)에 + bearer 재사용. **완료(Sonnet+Opus): src/a2a_server.rs, 메서드=ADR-001 PascalCase(스펙확정), camelCase wire, task_id=randomblob, merge 마운트+bearer 공유, 신규 dep 0(axum json 회피). lib +18 test(206 기본/235 풀피처), clippy 클린.** Phase2 interop 후속: Agent Card 최소필드·공개(현 bearer 뒤)·TaskState SCREAMING_SNAKE.
- [x] Task 3: inbox MCP 툴(poll_tasks/claim_task/complete_task) 코어 MCP(mcp+serve)에. **완료(Sonnet+Opus): TunaSearchServer에 a2a_store(Task2 Arc 공유, 새 커넥션 0) + 3툴, 순수함수 분리, 존재검증, HTTP e2e(poll→claim→complete+DB확인). src/mcp.rs만 수정, 신규 dep 0. mcp 31 test(+16), 풀피처 lib 251.** 후속: get_info instructions에 inbox 툴 언급(Task 4).
- [x] Task 4: dispatcher MCP 툴(send_task/get_task, worker와 대칭) + create_task_from_message DRY 헬퍼(a2a_server::handle_send와 공유) + get_info instructions + **/loop 워커 레시피·dispatcher 흐름(설계문서 §12)**. **완료(Sonnet 코드+Opus 리뷰·레시피): 풀피처 lib 262(+11), 기본 209, clippy 클린, 신규 dep 0.** get_task 부재=Ok안내(조회는 실패 아님).
- [x] Task 5: 라이브 e2e(윈 dispatch→맥 worker→artifacts→검토) 최소 round-trip. **완료(2026-07-03 크로스머신 라이브): 윈도우 코어(192.0.2.10:8770, throwaway 토큰) `/a2a` SendMessage(win-claude→mac-claude, "TaskState enum 요약") → 맥 worker poll/claim/complete → 윈도우 GetTask=completed+artifact 1. artifact 소스 교차검증 통과(6-state 정확). task_id=83f0e576, 19:11→19:17(맥 HITL 승인 포함). semi-a2a(공유 데이터평면+HITL) 크로스머신 실증. Phase 1 완료.**
- [x] 후속: half-a2a→semi-a2a 용어 정정 완료(CLAUDE.md·CHANGELOG. README엔 해당 용어 없음, 역사적 핸드오프는 시점기록이라 미변경).

## v2 백로그 (착수 전 결정 필요)
- [~] 분리 터미널 A2A 협업 — (A) 설계로 승격(위), 자율(B)은 Stage 4로 분리
- [x] 신규 엔진 러너(HTTP): ollama·lmstudio·openai (Plan 17 done). opencode CLI 참가자는 후속(외부 CLI 통합)
- [ ] 리치 프론트(ratatui/web) — 신규 의존성 결정 필요

## A2A 스트리밍 SSE (Phase 2) (docs/design/v2-a2a-streaming_2026-07-03.md)

> 정찰 완료(스펙 표면+현 코드 위치). 최종 목표=스펙 준수 A2A 서버(streaming:true 광고, 외부 A2A 클라가 task 던지고 SSE 실시간 구독). 비목표=자율성/워커 push. 미착수.

- [x] T1: 이벤트 버스(store 계층 broadcast::Sender) + 세 변이(create/update_state/complete) emit. 단위테스트. (785fb25; 기본 211/풀 264 pass, Opus 리뷰·독립검증) — ⚠T3 유의: SendStreamingMessage는 create 전에 subscribe해야 초기 submitted 이벤트 안 놓침(broadcast는 늦은 구독자에 replay 안 함). SubscribeToTask는 스냅샷 선전송 후 스트림.
- [x] T2: 스트리밍 타입(TaskStatus/TaskStatusUpdateEvent/TaskArtifactUpdateEvent/StreamResponse) serde + 순수 task_event_to_frames 매핑. (25619c4; 218 pass, Opus 리뷰·독립검증) 와일 final/lastChunk/statusUpdate/artifactUpdate 검증, TaskState snake_case 재사용.
- [x] T3: SendStreamingMessage SSE 엔드포인트(생성+스트림, final 종료). (9ed6380; 274 pass, Opus 정독리뷰·독립검증) subscribe-before-create, task_id 필터, testable string 스트림 분리, serve store with_task_events 배선(MCP claim/complete와 버스 공유), 버스없으면 -32004.
- [x] T4: SubscribeToTask SSE 엔드포인트(기존 task 재구독). (ea3e855; 279 pass, Opus 리뷰·독립검증) 스냅샷 먼저->terminal=최종프레임 종료/아니면 라이브 chain, subscribe-후-get_task, 없는id=-32001.
- [x] T5: Agent Card capabilities.streaming=true 플립(두 스트리밍 메서드 동작하니 정직). push_notifications는 false 유지. (2bc5437; 22 a2a tests pass)
- [x] T6: 이벤트 시퀀스 검증(task_frame_json_stream 단위테스트) + content-type/-32004/-32001 oneshot 테스트 + **로컬 라이브 데모 성공(복붙 0)**. boss가 SendStreamingMessage SSE 열고 -> 워커 MCP claim/complete -> 같은 스트림에 task(submitted)->statusUpdate(working)->artifactUpdate(lastChunk)->statusUpdate(completed,final) 실시간 도착 후 종료. agent-card streaming:true 라이브 확인. = **A2A 스트리밍 Phase 2 완료.**

## A2A 자율 워커 데몬 (worker auto-poll) (docs/design/v2-a2a-worker-daemon_2026-07-03.md)

> (a) 워커 auto-poll = 사람 트리거 릴레이 제거 마지막 조각. (b) 이기종 파트너 = 데몬의 --runner/--model. 미착수.

- [x] W1: 프로덕션 MCP HTTP 클라이언트(handshake + call_tool + SSE 파싱) 추출·일반화 + 단위테스트. (ad5ca38; 281/218 pass, Opus 리뷰·독립검증) McpHttpClient(connect/call_tool/poll·claim·complete 래퍼), worker feature=dep:reqwest async, serve 하네스로 왕복 테스트.
- [x] W2: parse_open_tasks(견고 블록 파싱, 단위테스트 5) + run_worker_loop(poll->submitted claim->spawn_blocking run->complete, --once/interval, 에러격리). (60364d8; 286 pass, Opus 리뷰·독립검증)
- [x] W3: Work 서브커맨드(WorkArgs) + main.rs 배선 + 러너 factory(claude/codex/opencode/http). (60364d8; work --help OK)
- [x] W4 로컬 데모(사람 트리거 0) 성공: dispatcher가 SendStreamingMessage SSE 개방 -> `tunaround work --once --agent win-worker --runner claude`가 자율 발견->claim->**claude 실제 실행**->complete -> SSE에 submitted->working->artifactUpdate(claude 실답변)->completed(final) 실시간. = "복붙 제거" 실증.
- [x] W4b (b) 이기종 파트너 실증: **`--runner codex`로 Codex가 워커** = Claude 아닌 다른 파트너가 자율 처리. dispatcher SendStreamingMessage(to=codex-worker) -> `tunaround work --once --runner codex`가 발견->claim(SSE working)->**codex exec 실행**->complete. GetTask=completed + **codex 생성 답변** artifact("A2A 프로토콜의 목적은 서로 다른 AI 에이전트가...협업하도록..."). = (a)=(b) 실증(같은 데몬, --runner만 교체). ⚠SSE는 codex가 curl --max-time(150s) 초과해 완료프레임은 SSE 대신 GetTask로 확인(codex 느림). 
  - **Ollama-http 경로도 라이브 성공(8c9f6d6)**: `--runner http --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b`로 **로컬 LLM이 워커**. GetTask=completed + qwen3.5:4b 답변("A tokio broadcast channel is an asynchronous communication primitive..."). = 3번째 이기종 파트너(Claude/Codex/로컬LLM 전부 --runner만 교체). **버그픽스**: 러너를 tokio spawn_blocking 대신 순수 std::thread에서 실행(reqwest::blocking이 spawn_blocking의 Handle::current() 때문에 거부하던 것 해소). (초기 실패는 GPU 좀비상태였고 ollama unload 후 정상.) minor: http factory가 --token(코어 토큰)을 러너 api_key로 전달(Ollama 무시라 무해, --http-api-key 분리 후속).

## A2A outbound 러너 (표준 에이전트 위임) (docs/design/v2-a2a-outbound-runner_2026-07-03.md)

> inbound(A+B) 폐기. outbound=우리가 외부 표준 A2A 에이전트에 표준으로 던지는 기반(a2a-client 채택). 미착수.

- [x] WA1+WA2: A2ARunner(a2a-client 0.2, sync-over-async block_on, from_card_url->send_message->(Task면)GetTask 폴링->artifact/agent메시지 매핑) + a2a-out feature + --runner a2a/--a2a-card/--a2a-token 배선. (6399443; 매핑 7테스트, 304/218 pass, Opus 리뷰·독립검증)
- [x] WA3 outbound 표준 interop 스모크 성공: 진짜 독립 표준 A2A 서버(radkit 0.0.5, 별도 프로세스, FakeLlm으로 negotiator 스텁)를 띄우고, 우리 코어 경유 `work --once --runner a2a --a2a-card http://127.0.0.1:9911/`가 외부 에이전트에 표준 위임 -> GetTask=completed + artifact에 외부 에이전트 응답("ECHO from external standard A2A target..."). = **우리가 표준 A2A로 나갈 수 있음 실증.** 덤: 1차 실패(radkit LLM 401) 때 A2ARunner 에러매핑+fail-전이 정확 동작(=(2) 재검증). ⚠단서: radkit=a2a-client와 같은 상류(microagents->a2aproject/a2a-rs) 계열이라 "같은 레퍼런스 구현군 내 표준 왕복" 검증(완전 이종 a2a-rs/turul-a2a 대상은 미시도, timebox). 프로토콜 왕복(카드발견->SendMessage->task완료->artifact) 자체는 유효 실증.

## 1차 리팩토링 (제미나이+코덱스 리뷰 기반) (docs/plans/v2-refactor-from-reviews_2026-07-03.md)

> Opus 자체검증·삼분류. 세션9에서 3자(Opus 통합 + 맥 worker + 로컬 Codex worker) A2A 도그푸딩으로 처리. **완료: PR #1로 main 머지(merge afdecea, 8/9 + R10 + CI).** 검증 3-OS CI green(ubuntu/macOS/windows) + 로컬 313 pass.

- [x] R1 [높음] MCP 에러 계약 정직화(claim/complete/fail 실패를 isError로, 클라·워커가 감지). 코덱스 #1. (b78df01, Opus+Sonnet)
- [x] R2 [높음] A2A 상태머신 조건부 전이(이중 claim/terminal 덮어쓰기 차단, rows_affected 체크). 코덱스 #2. R1과 묶음. (b78df01)
- [x] R3 [높음] watchdog 프로세스 트리 종료(Win /T, Unix process group). 코덱스 #6/제미나이 #2. (98b6298, Codex worker) **+ CI가 잡은 이식성 버그 수정(c9905e8): 외부 `kill -9 -PID`가 util-linux에서 no-op -> `libc::kill(-pid, SIGKILL)`. #[cfg(unix)]라 Windows 로컬 미실행 -> Linux CI가 첫 포착, macOS 실기 검증도 통과.**
- [x] R4 [높음/소] --context-map parse->Result(오타 거부, 기본레포 오폴백 차단). 코덱스 #5. 도그푸딩 워밍업 1순위. (a8b894e, Opus)
- [x] R5 [중] save_session orphan vectors/validity 정리. 코덱스 #8. (d4b6815, Mac worker A2A/LAN)
- [x] R6 [중/소] Embedder dim 동적화(비기본 모델 벡터유실). 제미나이 #5. 위임 이상적. (ced09e6, Codex worker A2A)
- [x] R7 [중] retriever/reader Result 계약(장애를 빈결과로 은폐 방지). 코덱스 #9. (b15172c, Mac worker A2A 헤드리스 데몬)
- [x] R8 [중] 검색 폴백 통일(tokenizer builder 1회). 코덱스 #7. (4c27ab2)
- [x] R10 [도그푸딩 finding] 워커 MCP 세션 만료 시 자동 재연결(404->handshake 재수행+재시도). (c58df41, Opus+Sonnet)
- [ ] R9 [낮/옵션] A2A poll 견고화(현 구현 견고성 감안 후순위). 제미나이 #1. **미착수(옵션 유지).**
- [x] 방법론: PR CI(.github/workflows/ci.yml, build+test+clippy 3-OS 매트릭스, 32cd48c+18371fa) + GitHub Flow(PR #1) 도입.
- 미루기: Runner async trait(YAGNI), main/mcp 분해(여유시), session-id pull·CoreSync(검증 먼저), 모델 결합(안정적).

## 브로커 거버넌스 구현 (세션10, 2026-07-04, docs/design/v2-broker-governance_2026-07-03.md §4)

> 세션9 두 실패를 구조적으로 제거: (a) no-consumer(폴러 없는 id로 간 task 영구 submitted), (b) self-disruption(워커가 자기 클론 갈아엎어 working 고착). 사용자 결정=전체 5개(#1~#5). 구현=Sonnet 서브 + Opus 리뷰·검증. cargo=Bash `-j 4 CARGO_INCREMENTAL=0`, 검증 = `cargo test --features "morphology mcp serve worker"`.

- [x] #1 네이밍 컨벤션 문서(cost 0, Opus 직접): a2a-usage.md에 "to_agent는 폴링하는 워커 id만(dispatcher id는 from_agent 전용)" + 네이밍 `{머신}-{역할|러너}`(worker=-worker/-codex/-llm, dispatcher=-dispatch/사람이름) + auto=-worker/supervised=-claude 관례.
- [x] #3+#4 고착·no-consumer 노출(Sonnet, 표시 전용): store/a2a.rs 순수 age 헬퍼(parse_sql_datetime/age_secs) + mcp.rs format_open_tasks(poll)·format_task_status(get_task)에 stuck?(working·updated_at 낡음)·no-consumer?(submitted·created_at TTL초과) 주석 + 신규 `tasks` MCP 도구(브로커 전역 열린 task 조망, list_all_open_tasks 저장소 메서드). 임계값=named const. **A2A expired state 미추가(스펙 부재)=표시 신호로.**
- [x] #2 빌드 피처 광고(Sonnet): a2a_server.rs AgentCard에 buildFeatures: Vec<String>(compile-time cfg! for serve/worker/mcp/engines/semantic/morphology/a2a-out) + build_agent_card 배선 + 카드 테스트. **poll엔 미추가(poll=task목록, capability 아님). 워커별 runner/write 광고=워커 레지스트리 필요=§6 후속.**
- [x] #5 워커 격리 가드레일(Sonnet): worker.rs/config.rs 순수 헬퍼 write_lane_disrupts_node(project: Option<&Path>, node_cwd) = None→true(cwd에서 실행=위험), Some(p)→canonical(p)==cwd or cwd⊃p면 true. node 레인 배선·work 서브커맨드에서 write+disrupt면 그 레인 거부(명확 안내). **자동 워크트리 프로비저닝=후속.**
- [ ] 최종: 검증(풀피처 pass 확인) + CLAUDE.md 현재상태·WIN포인터 갱신(Windows 단독 편집 규약) + 세션10 핸드오프.

## 에이전트 레지스트리 (UUID+태그) (세션11, 2026-07-04, docs/plans/v2-34-agent-registry.md)

> 어드레싱: 자유 문자열 → UUID(라우팅)+태그(발견). 로스터=SqliteStore 인메모리 필드(양 경로 공유). 하위호환=레거시 문자열 exact-match 유지. 베이스라인 377. 정본 [설계](docs/design/v2-agent-registry-uuid-tags_2026-07-04.md).

- [x] T1: 로스터 데이터모델(src/store/agents.rs: AgentEntry/parse_tags/selector_matches/is_online) + SqliteStore 인메모리 roster 필드(RefCell<HashMap>) + register/heartbeat/list_agents/resolve_selector + 단위테스트 20개. (1c692ca; Sonnet 구현+Opus 리뷰·독립검증) 풀피처 397 pass, clippy 클린.
- [x] T2: MCP 도구(register_agent/heartbeat/list_agents) + send_task to_selector(0=no-consumer, 1=라우팅, 2+=후보반환) + McpHttpClient 대칭 + HTTP e2e. (5214a33; Sonnet+Opus 리뷰·독립검증) 순수함수 validate_send_target/format_ambiguous_candidates/format_agents/send_task_routed. 풀피처 407 pass, clippy 클린. 하위호환 to_agent 문자열 불변.
- [x] T3: /a2a SendMessage toSelector(공유 resolve, to_agent Option화 하위호환) + 단위테스트. 리팩토링으로 validate_send_target/SendTarget/format_ambiguous_candidates를 store/agents.rs로 이동(serve·mcp 공유, 피처 커플링 회피). (Sonnet 구현+Opus 리뷰·독립검증) 풀피처 396 lib pass, clippy 클린. 하위호환 to_agent 단독 지정 불변.
- [x] T4: 워커 CLI --agent(자가 uuid)/--tags + 자동 register + 매 패스 heartbeat(재기동 시 재등록). (ed2966b; Sonnet+Opus 리뷰·독립검증) generate_agent_uuid/needs_reregister 순수함수. 풀피처 414 pass, clippy 클린.
- [x] T5: docs(a2a-usage §0 어드레싱 UUID+태그 재프레이밍, --tags 옵션, 신규 §9 등록·발견·셀렉터 레시피) + 하위호환 확인 + **라이브 스모크 4/4 통과**.

## doctor Stage 4 갭 채우기 (Kiwi/형태소 + Ollama 도달) (docs/plans/v2-35-doctor-stage4.md)

> 배포·온보딩 §C의 doctor 잔여. 기존 run_doctor(세션9, node.toml 기반)에 additive 2갭. 베이스라인 414. claude/codex 인증심층·config-less 모드는 비범위.

- [x] T1: Tokenizer::backend_name()(lindera/kiwi/simple) + doctor 형태소 백엔드 probe(morphology 게이트, Kiwi 로드=OK/폴백·미빌드=WARN). (Sonnet+Opus 리뷰·독립검증) 단위테스트 2. 라이브: "OK morphology: Kiwi 로드됨" 확인.
- [x] T2: doctor http 레인 Ollama 도달 ping(engines 게이트, 3s GET, 도달불가=WARN, 기존 None=FAIL 보존). 라이브: "WARN ... 도달 불가" 확인. 검증 421 pass, 표준 clippy 클린. **PR #6 머지(89cdbf2)**, CodeRabbit 2건(스키마 검증·OS별 안내) 반영.

## task runner 트레이스 + 쓰기 민감 path 가드 (B 축소판) (docs/plans/v2-36-trace-runner-write-guard.md)

> agentgateway P1. 축소 근거: v7이 started/completed/session_id 커버, net-new=runner 하나. 베이스라인 421.

- [x] B1: tasks `runner` 컬럼(스키마 v8) + try_claim에 runner 기록(claimed_at와 동시점) + mcp/client/worker 배선 + get_task 노출. 하위호환 None=NULL. (bb299cd; Sonnet+Opus 리뷰·독립검증) TASK_COLUMNS 11컬럼 정합 확인, v7→v8 마이그레이션 테스트, 428 pass. poll/tasks 텍스트 표시는 파서 안전성으로 보류(get_task 우선).
- [x] B2: 쓰기 민감 path 가드(WRITE_GUARD_DIRECTIVE, Write 시 claude/codex 프롬프트 주입, behavioral=readonly-soft 정합, READONLY와 배타) + write_guard_prefix 순수테스트 2. (e833f22) **PR #7 머지(27f04e6)**, CodeRabbit 1건(requeue runner 클리어) 반영.

## C 축소판: node 레인 태그 배선 (config→런타임 태그 seed) (agentgateway 검토 v1-후)

> T4에서 None으로 미룬 node 레인 태그를 배선. node 워커도 셀렉터로 발견되게. backend는 별도 registry 없이 lane 정의=named backend(문서만).

- [x] C: Lane에 tags 필드(work --tags 동일 형식) + node 레인 run_worker_loop 호출부 배선(T4 None 대체) + 파싱 테스트 + node-onboarding 문서(tags + backend=named-seat 명시). (Opus 직접) 428 pass, clippy 클린. backend registry는 비채택(lane 정의로 충분). (Opus 직접) 스모크: 코어(127.0.0.1:8899) + `work --once --tags`로 워커 2개 자기등록 → `/a2a` SendMessage toSelector: 단일매칭(smoke-worker 라우팅)/무매칭(no-consumer 에러·미생성)/다중매칭(후보 smoke-worker+smoke-worker2 반환·미생성)/부분집합(machine=mac,runner=claude→smoke-worker2 유일) 전부 정확. 레거시 to_agent 문자열 경로 불변(기존 handle_send 테스트 그대로 pass).

## Plan v2-37: codex 라이브 감독 (app-server ws + turn/start 주입) (docs/plans/v2-37-codex-live-supervisor.md)

> 설계 정본 docs/design/v2-codex-live-supervisor-appserver_2026-07-05.md. codex 감독을 헤드리스 exec -> 라이브 app-server thread로. 신규 `tunaround codex-inject`(ws)가 turn/start로 외부 wake. 구현 Sonnet, Opus 리뷰. **P0~T5 구현 완료 + 라이브 스모크 통과(PR #9). 리뷰 findings 반영.**

- [x] P0: 완료(stdio 실측). thread id=result.thread.id, 승인=MCP호출이 never여도 mcpServer/elicitation/request→injector가 action:accept 필수, accept 후 tuna-broker list_agents native 호출 정답(raw HTTP 0). enum 확정. 설계 §5.2·§7 반영
- [x] T1: 프로토콜 순수부 src/codex_appserver.rs(요청빌더+분류+파싱헬퍼+승인응답빌더). 25테스트, 스키마 대조 검증(Opus). 커밋 45d7f33.
- [x] T2: ws 클라 src/codex_inject.rs(tokio-tungstenite 0.24, connect→initialize→thread resume|start→turn/start→펌프) + main.rs CodexInject 서브커맨드. 커밋 159364b.
- [x] T3: 승인 자동응답(decide_action: elicitation accept/승인 granted/unknown LogOnly). T2와 함께 159364b.
- [x] T4: node 감독 레인 안내 runner별 분기(codex→app-server+codex-inject 레시피, claude→Monitor+poll). main.rs. (Opus 직접)
- [x] T5: 문서(a2a-usage §10 + dev-mac-windows SSH, 96c8b34) + **라이브 스모크 통과**(Opus). 스모크 A: codex-inject로 list_agents 왕복(ws→initialize→thread/start→turn/start→elicitation 자동accept→native MCP→"2명"→turn/completed→exit0). 스모크 B: 총감독 SendMessage로 task 생성→codex-inject claim/처리/complete→**GetTask state=completed, runner=codex, artifact="2"**(raw HTTP 폴백0), thread resume으로 맥락연속(티키타카) 실증. **라이브서 turn/completed params=turn.id 중첩 발견·수정**(fix 커밋).
- **Plan v2-37 완료**(P0+T1~T5). 46 신규 순수 테스트, 전체 lib 453 pass, CI조합 clippy 클린. HITL `--remote` 관전만 사용자 수동 확인 잔여(설계상 성립).

## Plan v2-38: 통합 총감독 대시보드 MVP (docs/plans/v2-38-orchestrator-dashboard.md)

> 설계 정본 docs/design/v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06.md. `tunaround serve`의 `/dashboard`가 read-only 웹으로 4자 감독 roster + 라이브 task 피드 + goal 폼 서빙. 브로커 기존 SSE 이벤트버스·roster·task 상태 재사용(net-new 최소). 구현 위임 ①tunaLlama ②A2A codex ③Sonnet, Opus 리뷰. 베이스라인 453.

- [x] T1: `/dashboard` GET 라우트 + 정적 read-only HTML 스켈레톤(roster/task/goal placeholder). tunaLlama 생성→Opus 리뷰(bearer 밖 outer router merge, auth 유지)→적용. 라이브 GET=200, POST /mcp=401. 커밋 4aa586c.
- [x] T2: SSE 배선 완료. **tunaLlama(kimi) 생성 → Opus 리뷰·적용**(src/mcp.rs만). GET `/dashboard/events`(전역 SSE: 모든 TaskEvent를 `{"event":"status|completed","task":{camelCase}}` JSON, Lagged 스킵·Closed 종료, 무인증 outer) + GET `/dashboard/roster`(list_agents 빈selector JSON, axum json피처 미활성이라 serde_json 수동 응답=신규의존0) + DASHBOARD_HTML JS(EventSource 피드 200cap + roster 5초 폴). 순수스트림 단위테스트 1. **검증**: lib 456 pass(회귀0)+통합/doc pass, clippy 클린. **라이브 스모크**: /dashboard=200, /dashboard/roster=200 JSON(3자 감독 online), /dashboard/events=text/event-stream 유지+실 task submitted 이벤트 수신 확인, /mcp 401(auth 경계 불변). 미커밋(리뷰 후).
- [x] T3: goal 폼 → SendMessage 완료. **tunaLlama 생성 → Opus 리뷰·적용**(DASHBOARD_HTML만). 폼=토큰(password)·목표·대상 select(roster로 채움 + "모든 감독" 셀렉터 옵션)·상태줄. `submitGoal`이 기존 인증 `POST /a2a SendMessage`를 fetch(Authorization: Bearer 토큰, 미저장) 재사용, `sel:`/`agent:` 접두로 toSelector/toAgent 분기. 신규 Rust 0. **검증**: lib 456 pass(회귀0), clippy 클린. **라이브 스모크**: /dashboard 폼 렌더, JS 요청형태로 인증 write→task submitted, /a2a 무토큰 401, 셀렉터 role=supervised→다중매칭 후보3자 에러(설계대로 HITL). 미커밋(리뷰 후). **관찰**: 기본 "모든 감독" 셀렉터는 다중 online 시 후보에러→드롭다운서 특정 감독 골라 재제출(원클릭 브로드캐스트는 T4후 UX 개선 여지).
- [ ] T4: claude 감독 post_turn emit 배선(피드 합류, 최소). 범위 크면 별 PR.
- [ ] T5: 검증 - serve 기동 후 /dashboard 렌더 + goal→감독 처리→피드 반영 라이브 스모크. 라우트/SSE 프레임 단위테스트. 3-OS CI green.

## Plan v2-39: 대시보드 SPA (Vite + React + DaleUI) (docs/plans/v2-39-dashboard-spa.md)

> 설계 정본 docs/design/v2-39-dashboard-spa-daleui_2026-07-06.md. v2-38 백엔드(SSE·roster·goal API) 재사용, 인라인 HTML→DaleUI React SPA. 서빙=rust-embed + `dashboard` feature-gate(사용자 확정). daleui@1.1.1(React 19 peer). feat/orchestrator-dashboard 위 이어감→한 PR.

- [x] S1: frontend/ 스캐폴드(직접) - Vite8+React19.2+TS+daleui@1.1.1(+pretendard·jetbrains-mono), base:/dashboard/, dev proxy(events/roster/a2a→8770), daleui/styles.css+폰트 import. npm build 성공.
- [x] S2: 3요소 DaleUI 구현. **tunaLlama 버전 API 드리프트→서브에이전트 직접 구현, Opus 리뷰**. api.ts+Roster(Card+Tag online)+Feed(EventSource seq키 200cap)+GoalForm(PasswordInput+Select+Button→/a2a, 토큰 sessionStorage). Opus 수정=index.css 데드CSS 정리+main.tsx 미import(.dash-grid 죽어있던) 버그. npm build 성공.
- [x] S3: 브로커 서빙(직접) - Cargo `dashboard` feature + rust-embed(frontend/dist) + /dashboard·favicon·assets/{*path}(MIME 매핑), events/roster는 serve 유지, OFF=안내 페이지, 인라인 HTML 제거. curl 검증(200/MIME/roster/401). lib 456 pass, clippy 클린(ON/OFF).
- [x] S4: CI - ci.yml ubuntu `dashboard` 잡(node22→npm ci+build→cargo build/clippy --features dashboard). embed=OS독립이라 1잡. 3-OS 매트릭스(dashboard 없이) 유지.
- [x] S5: 검증 - curl 임베드 전부 통과 + **브라우저 실렌더 확인(사용자 스크린샷: 3자 online 로스터·SSE 연결·goal 폼 DaleUI 렌더, 2열 그리드).** 남음=커밋+push+PR(3-OS+dashboard CI).

## Plan v2-40 S1: SessionStart 자동무장 훅 (docs/plans/v2-40-universal-session-bus.md)

> 설계 정본 docs/design/v2-40-universal-session-bus_2026-07-06.md. opt-in(TUNA_AUTOARM=1) claude 세션이 시작 시 detached `tunaround poll`(register+heartbeat 내장)로 자동 무장 → 로스터 등장(총감독도 편입). 정리=TTL 90초(deregister 도구 없음). 구현=Opus 직접(hook JSON I/O + CLI 정밀 배선, tunaLlama 드리프트 회피).

- [x] S1a: .claude/hooks/tuna-autoarm.py(SessionStart) - opt-in 게이트·detached poll 기동·pidfile·additionalContext. Windows DETACHED_PROCESS/POSIX start_new_session, 중복 무장 가드, 토큰 미저장.
- [x] S1b: .claude/hooks/tuna-disarm.py(SessionEnd) - pidfile poll kill(taskkill /T · SIGTERM) + pidfile 제거. 로스터 TTL 90초 소멸.
- [x] S1c: .claude/settings.json 두 훅 배선(${CLAUDE_PROJECT_DIR} 경로, env self-gate).
- [x] S1d: 문서 a2a-usage §11(env 계약·동작·발견≠제어·LAN 복제).
- [x] S1e: 라이브 테스트 통과 - mock stdin autoarm → win-autoarm-smoke online 등장(6태그) → disarm → poll kill + 90초 TTL 후 online=False 확인. 나머지 3자 감독 online 유지.

## Plan v2-40 S2: 발견 리포터 (docs/plans/v2-40-universal-session-bus.md)

> 미무장 세션도 대시보드 후보로. MVP=claude 세션(jsonl mtime, 무의존). candidate={uuid,runner,project,source,age_secs,reported_at}, armed는 브로커 overlay(online roster 소속). stale=reported_at TTL.

- [x] S2a(Opus 직접): store/candidates.rs(CandidateEntry+is_fresh+CANDIDATE_TTL_SECS=180) / sqlite.rs candidate_pool+report_candidates(uuid upsert, now 덮어씀)+list_candidates(fresh만) / mcp.rs 도구 report_candidates·list_candidates(armed overlay=online roster) + GET /dashboard/candidates + format_candidates + 안내텍스트 / mcp_client.rs 래퍼. **검증: lib 385 pass(신규 8: is_fresh 4·store 2·format 2), clippy 클린.** bin 재빌드는 브로커 락으로 보류(라이브 스모크 S2c에서 조율).
- [x] S2b(Opus 직접, 폴백: 경로디코딩 heuristic 스펙민감): src/discover.rs(DiscoveredSession + project_from_cwd·parse_cwd_from_jsonl_line·age_secs_since·read_first_line·enumerate_claude_sessions·sessions_to_candidates_json) + main.rs Discover 서브커맨드(--core/--token/--projects-dir/--stale-mins/--interval/--once) → client.report_candidates 루프. **project는 mangled-dir 대신 jsonl 첫줄 cwd에서 정확 추출**(mangled 디코딩은 lossy). **검증: check(bin+lib) 통과, discover 단위 5건 pass, clippy 클린(rfind 반영).** bin 재빌드는 라이브 스모크 S2c에서.
- [ ] S2c: 테스트 + 라이브 스모크(이 머신 discover→내 세션 후보→/dashboard/candidates armed overlay).

## Plan v2-40 S3: 대시보드 "발견된 세션" 패널 (docs/plans/v2-40)

> S2 백엔드(/dashboard/candidates) 소비. plain React(프론트=Opus 직접, tunaLlama 부적합). 로스터/피드 스타일 통일. armed(로스터 소속)는 제외하고 미무장 후보만 노출. claude arm은 외부 소켓 부재라 "연결"=세션 id 복사+수동 안내(발견≠제어 정직).

- [x] S3 코드: api.ts Candidate 타입+fetchCandidates / Candidates.tsx(자체 5초 폴, roster 스타일 재사용, armed 필터, runner/project/source shield, amber 상태닷, "연결" 복사 버튼) / App.tsx mount(Feed 다음) / index.css candidates-section(full-width)+status-dot.candidate+candidate-arm. **npm run build 통과(208KB, tsc 클린).**
- [x] S2c+S3 라이브 스모크(묶음): 브로커 재빌드(dashboard worker)·재기동 → discover --once → **/dashboard/candidates 후보 2건**: 3332c84f(project=secall, armed=False=미무장 후보), 4a46a380(project=tunaRound, armed=True=보스 dedup). **설계 §0 예시(tunaRound→secall 발견) 실현.** roster=win-opus-boss(display, uuid=세션id). project=cwd 정확추출. 브라우저 패널 렌더는 사용자 확인 대기(대시보드 라이브). **정합성 수정 반영(3c21dce): uuid=세션id+display_name, cwd 다중행 스캔.**

## Plan v2-40 S4: codex 직접 제어 (docs/plans/v2-40)

> 대시보드→codex app-server turn/start 직접 주입(codex-inject 재사용). MVP=수동 ws 제어(자동발견 후속). loopback 전용, 브로커 in-process(worker 피처).

- [x] S4a: codex_inject::run→Result<String>(최종답 반환) + POST /dashboard/control(loopback·worker게이트, in-process codex_inject::run) + route. check(worker 유무)·clippy 클린, codex_inject 23 pass.
- [x] S4b: ControlForm.tsx(ws+지시→POST, answer pre) + api sendControl + App mount + CSS. npm build 211KB.
- [x] S4c: 라이브 스모크 - POST /dashboard/control(loopback)→브로커→ws://8790 접속→initialize→thread→**turn/start 주입 성공**→codex 실제응답=usageLimitExceeded(win codex 사용량 초과, 외부요인)→브로커 **502+실제 codex에러 정직 반환**. **제어경로·에러처리 검증 완료**(깨끗한 응답만 quota reset 후). 브로커 재기동 PID 28348.

## 세션17: codex 감독 관전 결정 + 총괄 dedup

> 사용자 대화로 스펙(48a0dbb2) 개선: codex 관전=--remote 유지, 대시보드=통합 로그, 스트림=헤드리스(별건). 결정기록=설계 §10.

- [x] 라이브 메시 rebuild(dashboard worker, main=fca18fb, 43s) → 4프로세스 재기동(broker 35960·discover 28884·watch-results 7324·win-codex-sup 12244), 로스터 3자 online + mac 자동재접속, 대시보드 200.
- [x] mac 인박스 2건 수신(uuid 폴링): 566d54a3 북극성(ack 완료) / 48a0dbb2 codex 감독 스펙(처리 중).
- [x] Point 2 총괄 dedup: 이 세션 win-opus-boss로 무장(PID 36020, session 태그) + pidfile → 후보 armed=True dedup, 로스터 online.
- [x] item 1 codex 관전 결정: main.rs node 힌트 갱신 + 설계 §10 결정기록. 코드 로직 무변경.
- [ ] task 48a0dbb2 A2A 종료 보고(claim+complete, 개선된 결정 요약).
- [ ] cargo check(main.rs 힌트 변경 컴파일 확인) → 커밋 → push 승인 후 PR.

## Plan v2-44: presence 스캐너 + role 체계 개편 (docs/design/v2-44-presence-scanner-and-roles_2026-07-11.md)

> 2026-07-11 사용자 승인(세션18 §6 제안 + sup=인프라 인디케이터 재정의 + role 명칭 정리). PR #46 머지 후 T4.

- [x] 설계 정본 작성(v2-44 문서: 스캐너·role 개편·sup 재정의·마이그레이션 5단계).
- [x] 토큰 위생 감사 W1~W6 통합(§7: 주입 중복·안내 다이어트·task CLI·thread 로테이션·digest·전역 훅 진단).
- [x] T1 브로커·코어: report_presence + machine 동기화 + supervised→infra alias + 스캐너 순수부·presence-scan 서브커맨드 + `tunaround task` CLI(W3) + watch-results --digest(W5). (브랜치 feat/v2-44-presence-scanner)
- [x] T2 코드분: 훅 다이어트(W1·W2, 마커 1회·안내 5줄, 무장 로직 전삭제) + SessionEnd=deregister 핑만 + codex 래퍼 3종 삭제 + 프로젝트 settings 훅 등록 제거. **W1 근본 실측 확정**=전역 settings의 python·python3 이중 엔트리 + 프로젝트 등록(3중 발화).
- [x] T2 ops(2026-07-11 라이브 완료): PR #47 머지(2ad7c7d) → 풀피처(dashboard) 재빌드·안정 경로 배포 → 구 스택 6프로세스 전량 종료+pidfile 정리 → 새 스택 기동(브로커 33372·presence-scan 6704·win-codex-sup infra 38548·watch-results --digest 60 46836) → 로스터에 win 세션 7건 src=scan 확인 + **mac-codex-sup가 alias로 role=infra 라이브 실증** → 전역 훅 재배포+python3 이중 엔트리 3건 제거(W6, 백업=settings.json.bak-v2-44).
- [ ] W4 thread 로테이션: **codex-inject에 로테이션 기능 자체가 미구현**(T2 ops 중 발견, 설정만으론 불가) → 후속 코드 task(요약 turn→새 thread 시드 옵션).
- [x] T3 mac 배포 완료(task 526f402c, 운영자 승인 하 mac 자율 수행): 스캐너 pid 94847(mac 세션 3건 src=scan) + mac-codex-sup infra 재태깅(pid 96522) + 훅 v2-44판 재배포 + 구 poll·pidfile 정리 + codex 래퍼 PATH 원복 + restart-mac-mesh.sh 신구성. **mac 발견: 실행 중 바이너리 in-place cp는 macOS 코드서명 무효화로 프로세스 SIGKILL** → 원자적 재배포(cp .new → codesign → mv)로 해결(win 안정경로 분리와 동근 교훈). digest 인박스(60s) wake 실증.
- [ ] T4 대시보드 뷰: 머신 헤더 인프라 도트 + infra 카드 제거(+선택 수신중 뱃지). PR #46 머지 후.
- [ ] T4.5 main.rs 분할 refactor(사용자 확정 2026-07-11, T5 전): fn main() ~1,330줄의 서브커맨드 인라인 루프를 각 도메인 모듈 run()으로 이동(watch_results::run 패턴 답습) + 인자 구조체 src/cli/ 분리. 별도 PR, 기계적 이동만(동작 불변).
- [ ] T5 정리: alias 제거·report_candidates 제거·문서 일괄 갱신(a2a-usage §9·§10 등).
