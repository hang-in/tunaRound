# Plans — 진행 현황

> 현재 진행 중인 plan (active + partial). 완료된 plan 은 `../archive/plans/completed/` 로 이동.

## 진행 중

| 문서 | 우선순위 | 상태 | 요약 |
|---|---|---|---|
| [v2-17-http-engine-runner.md](v2-17-http-engine-runner.md) | P1 | done | OpenAI 호환 HTTP 엔진 러너: 한 러너로 ollama/lmstudio/openai/cloud 커버. 로스터 base_url/model/api_key_env, engine 이름 키로 다모델 다좌석. engines feature. 라이브: Ollama gemma4:e2b 응답 확인. 기본 101/engines 106 pass. e1373f9 |
| [v2-16-reinjection-cap.md](v2-16-reinjection-cap.md) | P1 | done | 재주입 축소: --recent-turns N으로 prior를 최근 N턴만 재주입(나머지는 검색 주입이 담당). opt-in(기본 None=현행 통째 재주입 불변). 북극성 스케일 페이오프. 기본 76/전체 101 pass. 2834a1d |
| [v2-15-windows-cli-resolve.md](v2-15-windows-cli-resolve.md) | P0 | done | gotcha #4: 러너 Windows CLI 해석(resolve_bin, PATH .exe/.cmd/.bat). codex.cmd가 spawn됨(Rust .cmd 자동 래핑). 비Windows·확장자 bin 불변. **라이브: codex 실제 spawn·응답 확인.** 8d02088 |
| [v2-14-agent-search-mcp.md](v2-14-agent-search-mcp.md) | P1 | done | 에이전트 능동 검색 MCP: rmcp search_context 서버 + --mcp-search stdio + **claude(--mcp-config)·codex(-c mcp_servers) 양 자리 배선.** 라이브: MCP search_context가 실 발언 반환 + codex 비파괴 확인. mcp 95 pass. a65feba+a5a185d+c892548 |
| [v2-13-vector-hybrid.md](v2-13-vector-hybrid.md) | P2 | done | 벡터 임베딩 + 하이브리드: Embedder(Mock/Ollama reqwest) + message_vectors 증분 색인 + cosine + RRF 융합(BM25+벡터, k=60). embedder 없으면 FTS 단독(불변). 라이브 임베더 dim 1024 검증. brute-force cosine(ANN 후속). sqlite 86/semantic 86 pass. 1ad8881+30efa51+8920027 |
| [v2-12-search-command.md](v2-12-search-command.md) | P2 | done | /search 명령: 사람이 SQLite 인덱스 직접 검색(retriever 재사용, 신규 의존성 0). 벡터는 설계 YAGNI로 보류 - FTS 품질 관측해 도입 근거 수집. 라이브 블로커는 해소(원격 Ollama 2232/bge-m3 dim 1024 검증). 기본 70/sqlite 79/+morphology 86 pass. bc2f359 |
| [v2-11-rag-injection.md](v2-11-rag-injection.md) | P1 | done | 검색 주입(RAG): build_round_prompt에 ContextRetriever로 끌어온 관련 과거 맥락 주입. 추가적(활성 경로 밖 다른 분기·과거 세션만, 재주입 미축소). SqliteRetriever + Session retrieve_for(dedup, K=5) + main --db. cross-session 검색 실연. sqlite 76/+morphology 83 pass. b0dd7bd+4643977 |
| [v2-10-sqlite-wiring.md](v2-10-sqlite-wiring.md) | P1 | done | SQLite 라이브 배선: MessageIndexer trait + SqliteIndexer + Session append_round 훅 + main --db. SessionBus 미러 패턴 답습, 추가적(JSON/Redis 미접촉). sqlite 74/+morphology 81 pass. 검색 인덱스가 라이브로 채워짐(검색 소비=Plan 11). e21cf43+5d79a0a |
| [v2-09-sqlite-fts.md](v2-09-sqlite-fts.md) | P1 | done | SQLite 시스템오브레코드 + FTS5 선-형태소화 색인/검색. secall store/schema.rs+bm25.rs 답습. 격리 모듈(store/sqlite.rs)+테스트, REPL/main JSON 미접촉. sqlite feature, 토크나이저 비의존. "검색을"->"검색" end-to-end 실증(Windows lindera). sqlite 68/+morphology 75 pass. c61cf11+181f46a |
| [v2-08-ko-tokenizer.md](v2-08-ko-tokenizer.md) | P1 | done | 한국어 형태소 토크나이저 포팅(secall): Kiwi 메인 + lindera 폴백, POS keep-tags(SL). morphology feature. 기본 66/morphology 72 pass, main 머지. ⚠️ Kiwi 런타임 버그(libkiwi 404)->lindera 실효 |
| [v2-07-bounded-debate.md](v2-07-bounded-debate.md) | P1 | done | v2 바운드 자동 교환: `/debate <n> <주제>`로 사람 발화 1회 -> 에이전트 N턴 자동 교환 -> 복귀. run_round N회 재사용, 최대 10 clamp. 69 테스트, main 머지됨 |
| [v2-06-redis-integration.md](v2-06-redis-integration.md) | P1 | done | v2 멀티세션 통합: Redis 미러(이벤트+스냅샷) + `--observe` 라이브 관찰 + `--session` 재개 + owner lease. 66 테스트, main 머지됨. observe/resume 라이브는 수동 검증 필요. 멀티세션 3플랜(04+05+06) 완성 |
| [v2-05-session-model.md](v2-05-session-model.md) | P1 | done | v2 세션 모델: in-store 논리 트리(Session messages+head, parent_id 실사용), /branches·/checkout 분기 탐색. 저장 포맷 StoredSession(레거시 폴백). 61 테스트, main 머지됨. 단일 프로세스 분기 토론 동작 |
| [v2-04-session-bus.md](v2-04-session-bus.md) | P1 | done | v2 멀티세션 토대: tunaSalon Redis session_bus 포팅(room->session), tokio/redis/futures 신규 의존. 격리 모듈, 라이브 Redis 테스트 #[ignore]. 56 테스트, main 머지됨. 멀티세션 3플랜의 1단계(다음 05 세션모델/06 통합) |
| [v2-03-write-delegation.md](v2-03-write-delegation.md) | P1 | done | v2 협업 코딩: `@engine!` 쓰기 지목, run_round mode 파라미터, Session::step Write 분기. 쓰기 인프라(러너 인자)는 v1 구현 재사용. 52 테스트, main 머지됨 |
| [v2-02-roster.md](v2-02-roster.md) | P1 | done | v2 설정 구동 N좌석 로스터: JSON 로스터 -> participants+registry, main.rs --roster 플래그. 오케스트레이터 N-ready 활용, 48 테스트, main 머지됨 |
| [v2-01-idle-watchdog.md](v2-01-idle-watchdog.md) | P0 | done | v2 idle watchdog(INV-4): 공유 헬퍼 exec.rs + RunError::Timeout + 양 러너 배선. 무출력 행 방지, stderr 동시 배수. 43 테스트, main 머지됨 |
| [v1-01-agent-runner.md](v1-01-agent-runner.md) | P0 | done | 스캐폴드 + Codex 러너(argv·JSONL 파싱·dedup·read/write 모드), 순수함수 TDD. main 머지됨 |
| [v1-02-claude-runner.md](v1-02-claude-runner.md) | P0 | done | Claude 러너(stream-json NDJSON, result 라인 content + INV-3 토큰 fallback, RunError::Agent). main 머지됨 |
| [v1-03-orchestrator.md](v1-03-orchestrator.md) | P0 | done | 토론 오케스트레이터(roles + build_round_prompt 순차-인지 + run_round/RunnerRegistry, FakeRunner). main 머지됨 |
| [v1-05-repl.md](v1-05-repl.md) | P0 | done | thin REPL(명령 파싱 + Session.step + main.rs 실 러너). 돌아가는 앱(`cargo run`). main 머지됨 |
| [v1-04-persistence.md](v1-04-persistence.md) | P1 | done | 전사 영속(StoredMessage id/parent 트리-ready + JSON save/load) + Session resume + main 상태파일 인자. main 머지됨 |
| [v1-06-hardening.md](v1-06-hardening.md) | P1 | done | Hardening: /conclude(synthesizer 종합) + @engine(자리 지목). run_round 재사용 additive. main 머지됨 |

## 부분 완료 / 보류

| 문서 | 사유 |
|---|---|

## 완료

(`../archive/plans/completed/` 참조)
