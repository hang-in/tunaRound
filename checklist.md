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
  - [ ] Task 2 라이브 측정: 실 claude/codex로 push vs pull 토큰 비교 + 게으른 pull 여부 + 일관성(사용자 승인)
- [ ] Stage 3: 코어 프로세스 분리(상주 데몬 + 멀티 프론트/세션)
- [ ] Stage 4(범위 밖): 영속 에이전트 세션 + AutoLoop = (B), 경제 조건 입증 시에만

## v2 백로그 (착수 전 결정 필요)
- [~] 분리 터미널 A2A 협업 — (A) 설계로 승격(위), 자율(B)은 Stage 4로 분리
- [x] 신규 엔진 러너(HTTP): ollama·lmstudio·openai (Plan 17 done). opencode CLI 참가자는 후속(외부 CLI 통합)
- [ ] 리치 프론트(ratatui/web) — 신규 의존성 결정 필요
