---
title: "tunaRound 북극성: 계층형 공유 맥락 + 능동 검색 (메모리 아키텍처 방향)"
type: design
status: direction
priority: P0
updated_at: 2026-06-30
owner: shared
summary: 에이전트들이 서로의 대화 맥락을 능동적으로 기억·파악·검색해, 단기(세션)부터 프로젝트까지 모든 층에서 맥락을 제대로 잡는다. 핵심 전환 = "전사 통째 재주입" -> "검색해서 관련 슬라이스만 주입(RAG)". 저장소 계층화(Redis 핫 / SQLite 시스템오브레코드+FTS 백본 / vector DB는 필요 입증 시 마지막). 점진 순서로 과설계 경계.
---

# tunaRound 북극성: 계층형 공유 맥락 + 능동 검색

> 사용자 비전(2026-06-30): "중요한 건 서로가 서로의 대화 맥락을 능동적으로 기억하고 파악하고 검색해서, 단기기억(세션)부터 프로젝트까지 가능한 한 모든 상황에서 맥락을 제대로 파악하는 것. Redis·SQLite 적극 활용, 필요시 vector DB 도입."

## 핵심 전환

지금 tunaRound는 매 턴 **전사를 통째로 프롬프트에 재주입**한다(그래서 단일 세션 멀티라운드 맥락이 완벽 - 실측 검증됨). 그러나 이 방식이 이 비전의 **병목**이다: 세션이 길어지거나 프로젝트 스케일로 가면 토큰 한계에 부딪힌다. 비전은 "통째 주입"이 아니라 **검색 -> 관련 맥락만 주입(RAG)**을 요구한다. 이것이 중심 아키텍처 전환이다.

## 맥락의 층과 현황 (2026-06-30)

| 층 | 무엇 | 현황 |
|---|---|---|
| 턴 내(순차-인지) | 같은 라운드 앞 발언 | 있음 (`build_round_prompt`) |
| 세션(단기 기억) | 현재 토론 전사 | 있음 (트리 재주입) - 단 통째라 스케일 한계 |
| 분기 간 | 트리의 형제 가지 | 부분 (트리 있으나 active path만 봄) |
| 세션 간(프로젝트 기억) | 과거 토론들 | 없음 |
| 코드베이스 | 레포 | 부분 (ReadOnly 탐색, 인덱싱 아님) |
| 결론/결정 지식 | /save 결과·합의 | 없음 (검색 안 됨) |

## 저장소 계층화 (강점대로)

- **Redis = 핫/실시간.** 라이브 턴·presence·pubsub. 이미 있음(session_bus, Plan 06).
- **SQLite = 시스템 오브 레코드 + FTS5(전문검색).** 메시지/세션/화자/날짜를 키워드·구조로 검색. **백본.** 설계문서가 이미 SQLite를 v2로 둠(현재 JSON 파일).
- **Vector DB = 의미검색 전용, 마지막.** 키워드(FTS)로 안 잡히는 의미 매칭이 필요함이 입증될 때만. 선제 도입은 과설계(YAGNI).

## "능동적"의 의미

수동 주입만으론 "능동"이 아니다. 에이전트에게 **검색 도구**(`search_context(query)` / `recall(topic)` / `expand(handle)`)를 줘서 필요한 맥락을 스스로 끌어오게 한다. 이는 A2A의 MCP 도구와 같은 통로(메시지 라우팅 옆에 검색 도구).

## 추천 순서 (점진, 과설계 경계)

1. **(done) `/debate` 바운드 자동 교환** - 세션 내 단기 기억 멀티턴(Plan 07).
2. **SQLite 시스템 오브 레코드 + FTS** - JSON -> 쿼리 가능 저장. *모든 상위 층의 토대.* [다음 후보, 첫 스텝 결정 대기]
3. **검색 주입(RAG)** - 통째 대신 관련 슬라이스만 프롬프트에. 스케일 해결.
4. **에이전트 검색 도구** - recall/search_context (MCP/러너 경유).
5. **세션 간 프로젝트 기억** - 과거 토론·결정 recall.
6. **vector DB** - 위가 부족함이 입증되면.

> 원칙: 검색 가능(FTS) -> 검색 주입 -> 토론 품질 측정 -> 필요시 vector. 메모리 스택을 한 번에 다 짓지 않는다.

## 정답: 한국어 검색은 secall/tunaSalon에 이미 구현됨 (포팅 대상, 2026-06-30)

FTS의 한국어 형태소 문제 해법이 코드에 있다(직접 확인). **재발명 말고 포팅.**

- **어휘(FTS):** 형태소 분석기로 **선-토크나이즈 후 FTS5(unicode61)에 공백-조인해 저장** → FTS가 형태소를 색인("검색을"→"검색"). POS keep-tags = NNG/NNP/NNB/VV/VA/**SL(외국어)**. SL이 한/영/코드 혼용을 살린다.
  - **토크나이저: Kiwi가 메인(품질 최고), lindera는 폴백.** secall `crates/secall-core/src/search/tokenizer.rs`에 `Tokenizer` trait + `KiwiTokenizer`(kiwi-rs 0.1) + `LinderaKoTokenizer`(lindera 2.3.4 embed-ko-dic) + `create_tokenizer(backend)` 팩토리(kiwi 실패 시 lindera 자동 폴백). Kiwi cfg 제외 = Windows + linux-aarch64(그 외 mac/linux-x86_64는 Kiwi). Kiwi 초기 Mac 컴파일 이슈는 과거형(현재 mac aarch64 dev에서 동작). Kiwi는 첫 init 시 모델 ~50MB 다운로드(~/.cache/kiwi), lindera는 ko-dic 임베드(다운로드 0 → CI/폴백 적합).
  - tunaSalon `src/tokenize_ko.rs`는 이 중 **lindera 경로만 lift**(경량화). Kiwi 메인 원하면 secall 정본을 써야 함.
- **의미(벡터):** secall `search/{embedding,ann,vector}.rs` + tunaSalon `embed.rs`(`Embedder` trait, `OrtEmbedder`=**BGE-M3 ONNX**, `MockEmbedder`=결정적 폴백/테스트), `memory_vectors` BLOB + **usearch ANN**.
- **하이브리드:** secall `search/{bm25,hybrid,query_expand,chunker,model_manager}.rs` = BM25(어휘) + ANN(의미) 융합 + 쿼리확장 + 청킹. **가장 완비된 정본.**
- **진화:** tunaFlow(`vector_search/`, 벡터 원형) -> secall(독립 hybrid recall 엔진, Kiwi+lindera/bm25/ann/hybrid 정본) -> tunaSalon(lindera+BGE-M3+SQLite memory 경량 서브셋, feature-gate).

**tunaRound 포팅 방침:** secall 정본 `tokenizer.rs`(Kiwi 메인 + lindera 폴백 + factory) 통째 포팅, `backend="kiwi"` 기본. 그 위에 벡터 + 하이브리드 단계적.

**임베딩 = 원격 Ollama (결정 2026-06-30, 로컬 ORT 대체):** BGE-M3를 로컬 ONNX(ort/ndarray/tokenizers)로 돌리지 않고, **지인 서버 Ollama를 SSH 터널로** 쓴다. `ssh -N -L 11435:127.0.0.1:11434 [사설계정]@<host>` -> 로컬 `http://127.0.0.1:11435/api/embed`(model `bge-m3`). tunaRound의 `Embedder`는 **reqwest HTTP 클라이언트**(OrtEmbedder 대신) + `MockEmbedder`(결정적 폴백/테스트/터널 다운 시). 장점: 무거운 ONNX 의존 제거(reqwest만), 연산 오프로드. 주의: (1) 터널 떠 있어야 동작 -> 다운 시 graceful(에러 또는 Mock 폴백), (2) 원격에 `bge-m3` pull돼 있어야(`/api/tags`로 확인), (3) 호스트/IP는 비공개. 의존성 feature-gate(`morphology` = kiwi-rs/lindera, `semantic` = reqwest 임베딩 클라이언트).

## 열린 결정

- **첫 콘크리트 스텝 = 2번(SQLite+FTS)인가?** (내 추천: 그렇다. 토대.) 사용자 확인 대기.
- 형식 A2A 프로토콜(Google Agent2Agent) 정렬 여부는 후속(로컬 MCP+버스로 시작).
- 분리 터미널 A2A 자율 협업(turn-triggering)은 별 트랙(백로그).
