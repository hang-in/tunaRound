---
title: "tunaRound v2 Plan 11: 검색 주입 (RAG, 능동 검색으로 관련 과거 맥락 주입)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 북극성 핵심. build_round_prompt에 검색으로 끌어온 관련 과거 맥락을 주입한다. 이번 슬라이스는 추가적(additive): 활성 경로(이미 prior로 주입됨) 밖의 맥락(다른 분기·과거 세션)을 SqliteStore.search로 끌어와 "참고할 만한 과거 맥락(검색)" 섹션으로 추가. prior 캡(재주입 축소)은 품질 측정 후 별 슬라이스. ContextRetriever trait + SqliteRetriever(sqlite feature, --db 읽기 연결) + build_round_prompt retrieved 파라미터 + run_round 배선 + Session retriever 필드 + 활성경로 dedup. retriever 없으면 동작 불변.
---

# tunaRound v2 Plan 11: 검색 주입(RAG) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: test-driven-development. Steps use checkbox. **cargo는 Bash 툴로 실행**(Git Bash sh 있어 exec.rs sh 테스트 통과; PowerShell이면 거짓 실패 2건).
> 결정: docs/design/v2-context-memory-direction_2026-06-30.md(전사 통째 재주입 -> 검색 슬라이스 주입). Plan 10 done(라이브 색인). 패턴: RunnerRegistry/MessageIndexer와 동형 trait. 아키텍처 재론 금지.

**Goal:** 능동 검색을 프롬프트에 연결한다. 매 라운드 topic으로 SQLite FTS를 검색해 **관련 과거 맥락**(활성 경로에 없는 다른 분기·과거 세션의 슬라이스)을 프롬프트에 주입한다. 에이전트가 "통째 재주입"에 의존하지 않고도 멀리 있는 맥락을 끌어오게 된다.

**Architecture(이번 슬라이스 = 추가적):** 현재 `run_round`가 전체 `transcript`를 `prior`로 통째 주입(활성 경로 멀티라운드 품질은 검증됨). 이번엔 그걸 **건드리지 않고**, 활성 경로 밖의 맥락을 검색해 **추가** 섹션으로 주입한다. `prior` 캡(재주입 축소)은 품질 측정 후 별 슬라이스(설계 원칙: 검색가능->주입->측정->필요시 축소). `ContextRetriever` trait(비게이트) + `SqliteRetriever`(sqlite feature) + `build_round_prompt`에 `retrieved: &[Utterance]` 파라미터(순수 유지) + `run_round`에 `retrieved` 전달 + `Session`이 retriever 보유·검색·활성경로 dedup. retriever 없으면 `retrieved=&[]` = 동작 불변.

**Tech Stack:** Rust 2024. 신규 의존성 없음(Plan 09/10 재사용). 선행: Plan 10 done. **공유 .db:** indexer(쓰기)와 retriever(읽기)가 같은 `--db`를 각자 SqliteStore로 연다(WAL = 동시 reader + 1 writer).

> 규율: #5/#6, TDD, 위임 Sonnet + Opus 리뷰, 검증/commit 분리.

---

## 범위

- **포함:** `ContextRetriever` trait(orchestrator, 비게이트) + `SqliteRetriever`(sqlite feature, SqliteStore.search + tokenize closure 래핑) + `build_round_prompt` retrieved 섹션 + `run_round` retrieved 파라미터 + repl 호출부 갱신 + `Session` retriever 필드 + topic 검색·활성경로 dedup + main `--db` retriever 배선 + 테스트.
- **비포함(후속):** `prior` 캡/ctx-handle(재주입 축소, 품질 측정 후) · 에이전트 능동 검색 도구(MCP/`/search`) · 벡터/하이브리드(Plan 12) · 세션 간 명시적 프로젝트 기억 UI.
- **불변식:** retriever 없음(기본/--db 없음/sqlite off) = `retrieved=&[]` = 기존 프롬프트·동작·테스트 불변.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/orchestrator/mod.rs` | (수정) `ContextRetriever` trait + `run_round`에 `retrieved: &[Utterance]` 파라미터. |
| `src/orchestrator/prompt.rs` | (수정) `build_round_prompt`에 `retrieved` 파라미터 + "참고할 만한 과거 맥락(검색)" 섹션(prior 위). |
| `src/store/retriever.rs` | (신규) `#[cfg(feature="sqlite")] SqliteRetriever`(SqliteStore 읽기 + tokenize closure, search -> Vec<Utterance>). 첫 줄 역할 주석. |
| `src/store/mod.rs` | (수정) `pub mod retriever;`. |
| `src/repl/mod.rs` | (수정) Session에 `retriever: Option<Box<dyn ContextRetriever>>` + 생성자 + step에서 topic 검색·dedup -> run_round 전달. |
| `src/main.rs` | (수정) `--db` 시 SqliteRetriever도 생성·배선(읽기 연결). |

---

### Task 1: build_round_prompt/run_round 검색 슬롯 + ContextRetriever trait (순수 플러밍, 동작 불변)

**Files:**
- Modify: `src/orchestrator/mod.rs`, `src/orchestrator/prompt.rs`, `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저(`prompt.rs` tests)**
```rust
    #[test]
    fn prompt_includes_retrieved_context_section() {
        let retrieved = vec![Utterance { speaker: "codex/reviewer".into(), content: "과거 분기 결론".into() }];
        let out = build_round_prompt(&p("claude", None), "주제", &[], &[], &retrieved);
        assert!(out.contains("참고할 만한 과거 맥락"));
        assert!(out.contains("과거 분기 결론"));
    }

    #[test]
    fn prompt_empty_retrieved_is_unchanged() {
        // retrieved=&[] -> 검색 섹션 없음(기존과 동일).
        let out = build_round_prompt(&p("claude", None), "주제", &[], &[], &[]);
        assert!(!out.contains("참고할 만한 과거 맥락"));
    }
```

- [ ] **Step 2: `build_round_prompt`에 `retrieved: &[Utterance]` 추가** — 시그니처 끝에 파라미터. 비어있지 않으면 **prior 위**에 섹션 푸시(에이전트가 "현재 흐름"보다 배경으로 인식하게): `sections.push(format!("참고할 만한 과거 맥락(검색):\n\n{}", join_utterances(retrieved)));` 를 prior push 앞에 둔다. 빈 슬라이스면 무영향(기존 테스트 그대로).

- [ ] **Step 3: `ContextRetriever` trait + `run_round` 파라미터(`src/orchestrator/mod.rs`)**
```rust
/// topic으로 관련 과거 맥락 슬라이스를 끌어오는 경계(RunnerRegistry와 동형, 비게이트).
pub trait ContextRetriever {
    fn retrieve(&self, query: &str, limit: usize) -> Vec<Utterance>;
}
```
  - `run_round(..., retrieved: &[Utterance])` 파라미터 추가. 루프에서 `build_round_prompt(part, topic, &prior, &same_round, retrieved)`로 전달(모든 자리에 동일 retrieved).

- [ ] **Step 4: repl 호출부 갱신** — `src/repl/mod.rs`의 `run_round(...)` 호출 5곳(Message/Only/Write/Conclude/Debate)에 `&[]` 전달(Task 1은 동작 불변). 컴파일 통과.

- [ ] **Step 5: 검증 + 커밋**
  - `cargo test`(기본) — 기존 + prompt 신규 2개 PASS(동작 불변). build/clippy(기본/sqlite/morphology) 경고 0.
  - 커밋(push 금지): `feat(orchestrator): build_round_prompt 검색 슬롯 + ContextRetriever + run_round 배선`.

---

### Task 2: SqliteRetriever + Session 검색·dedup + main 배선

**Files:**
- Modify: `src/store/mod.rs`, `src/repl/mod.rs`, `src/main.rs`
- Create: `src/store/retriever.rs`

- [ ] **Step 1: `SqliteRetriever`(`src/store/retriever.rs`, 첫 줄 역할 주석)** — `#[cfg(feature="sqlite")]`:
  - `SqliteRetriever { store: Mutex<SqliteStore>, tok: Box<dyn Fn(&str)->String + Send + Sync> }`(SqliteIndexer와 동형).
  - `impl ContextRetriever`: `retrieve(query, limit)` = `let q = (self.tok)(query); store.search(&q, limit)` -> `Vec<Utterance>`(SearchHit.speaker/content -> Utterance). 검색 실패는 빈 Vec + eprintln(best-effort).
  - `src/store/mod.rs`에 `pub mod retriever;`.

- [ ] **Step 2: Session 검색·dedup(`src/repl/mod.rs`)**
  - Session에 `retriever: Option<Box<dyn crate::orchestrator::ContextRetriever>>` 필드(기존 생성자 None, behavior-preserving). 새 생성자나 setter 추가(예: `with_retriever(mut self, r) -> Self` 또는 `new_with_indexer` 확장). main 배선과 맞춰 Sonnet 결정.
  - step의 각 run_round 호출 전에: `let retrieved = self.retrieve_for(topic);` 계산해 전달. 헬퍼:
```rust
const RETRIEVE_K: usize = 5;
fn retrieve_for(&self, topic: &str) -> Vec<Utterance> {
    let Some(r) = &self.retriever else { return Vec::new(); };
    let active = self.active_path();
    r.retrieve(topic, RETRIEVE_K)
        .into_iter()
        // 활성 경로에 이미 있는 내용은 중복이므로 제외.
        .filter(|u| !active.iter().any(|a| a.content == u.content))
        .collect()
}
```
  - Debate의 각 라운드도 round_topic으로 재검색(자연스러움).

- [ ] **Step 3: main 배선** — `--db` 시 indexer와 **별개로** 읽기용 `SqliteStore::open(db_path)`를 또 열어 `SqliteRetriever` 생성(같은 tokenize closure 정책). Session에 indexer+retriever 동시 배선. sqlite off/--db 없음 = retriever None.
  - 주의: tokenize closure를 indexer/retriever 둘 다 쓰려면 closure를 두 번 만들거나(create_tokenizer 두 번 = Kiwi 모델 재로드 가능) 토크나이저를 Arc로 공유. 단순화: 각자 closure 생성(폴백 경로는 무비용, morphology는 lindera라 경량). 비용 우려 시 Arc<dyn Tokenizer> 공유로 후속 최적화.

- [ ] **Step 4: 통합 테스트(`src/store/retriever.rs` 단위, `#[cfg(all(test, feature="sqlite"))]`)** — 파일 DB에 두 세션 색인 후, retriever가 다른 세션의 관련 슬라이스를 끌어오는지:
```rust
// 과거 세션 "a"를 색인 -> 새 topic 검색 시 "a"의 관련 발언이 retrieve된다(cross-session 능동 검색).
```
  - SqliteStore로 직접 색인(save_session) 후 SqliteRetriever.retrieve로 매칭 확인. (Session 전체 경로 dedup은 repl 단위 테스트 FakeRetriever로 별도 커버 가능 - Sonnet 판단.)
  - repl에 `FakeRetriever`로 "retrieved가 프롬프트까지 흘러가는지"(또는 dedup 동작) 단위 테스트 1개.

- [ ] **Step 5: 검증 + 커밋**
  - `cargo test`(기본) 불변 + `--features sqlite` + `--features "sqlite morphology"` PASS. build/clippy 3조합 경고 0. 스모크 `cargo run --features sqlite -- --db ./tmp.db` 패닉 없음(검색 경로 포함).
  - 커밋(push 금지): `feat(store): SqliteRetriever + Session 검색·dedup + main 배선`.

---

## Self-Review (작성자 체크)

- **추가적/불변:** prior 통째 재주입 미변경. retriever 없으면 retrieved=&[] = 기존 동작·테스트 불변. 검증된 단일세션 품질 보존.
- **능동 검색 기둥:** topic으로 활성 경로 밖(다른 분기·과거 세션) 맥락을 끌어옴 = 설계의 "능동 검색" 첫 실연.
- **dedup:** 활성 경로 중복 제외(검색이 prior와 겹치는 슬라이스를 다시 넣지 않음).
- **패턴 답습:** RunnerRegistry/MessageIndexer와 동형 trait. build_round_prompt 순수 유지(retrieved 슬라이스 주입은 Session 책임).
- **범위 규율:** 재주입 축소(prior 캡)·벡터·에이전트 검색도구는 명시적 비포함(측정 후/Plan 12).

## 위험 / 한계 (문서화된 후속)

- **재주입 미축소:** 이번엔 스케일(토큰)을 줄이지 않고 맥락을 더한다. 긴 세션 토큰 축소는 측정 후 별 슬라이스(prior 캡 + ctx-handle).
- **검색 품질:** FTS(어휘)만이라 동의어·의미 매칭 약함 -> Plan 12 벡터/하이브리드. 무관 슬라이스 주입 시 노이즈 가능(K=5 보수적, dedup).
- **tokenizer 이중 생성:** indexer/retriever가 각자 closure -> Kiwi면 모델 중복 로드 우려(현재 Windows=lindera라 무비용). Arc 공유는 후속 최적화.
- **동시성:** 같은 --db에 writer(indexer)+reader(retriever) 동일 프로세스 = WAL OK. 멀티프로세스 동시 쓰기 본격화는 후속.
