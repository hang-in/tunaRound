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
- [ ] W2: poll/claim/complete 래퍼 + work 루프(poll->claim->runner.run->complete, --once), fake 주입 단위테스트.
- [ ] W3: Work 서브커맨드(WorkArgs) + main.rs 배선 + 러너 선택 factory.
- [ ] W4: 로컬 라이브 데모(사람 트리거 0) + (b) 이기종 Codex-on-Ollama 워커 스모크.
