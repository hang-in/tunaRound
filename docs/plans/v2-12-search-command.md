---
title: "tunaRound v2 Plan 12: /search 명령 (사람이 인덱스를 직접 검색, FTS 품질 관측)"
type: plan
status: planned
priority: P2
updated_at: 2026-06-30
owner: shared
summary: 벡터(Plan 12 원안)는 설계 YAGNI 게이트(FTS 부족 입증 전)로 보류. 대신 설계 정렬 슬라이스 - /search 명령으로 사람이 SQLite FTS 인덱스를 직접 검색한다. 설계 로드맵의 "능동 검색 도구" 첫 발이자, FTS 검색 품질을 실측해 벡터 도입 여부(YAGNI)를 판단할 근거를 만든다. 기존 Session.retriever(ContextRetriever) 그대로 재사용 - 신규 추상화/의존성 없음. retriever 없으면(--db 없음/sqlite off) 안내 메시지.
---

# tunaRound v2 Plan 12: /search 명령 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: test-driven-development. **cargo는 Bash 툴로 실행**(PowerShell이면 exec.rs sh 테스트 거짓 실패).
> 결정(2026-06-30): 벡터는 설계 YAGNI("FTS로 부족 입증 시에만, 마지막")로 보류. 사용자 선택=정렬 슬라이스. /search로 검색을 사람에게 노출 + FTS 품질 관측. 아키텍처 재론 금지.

**Goal:** 사람이 `/search <질의>`로 현재 `--db` 인덱스(SQLite FTS, 현 세션 + 과거 세션 전부)를 직접 검색해 결과를 본다. 능동 검색을 사용자 손에 쥐여주고, FTS 검색 품질을 실제로 관측해 다음(벡터 도입 여부)을 데이터로 판단한다.

**Architecture:** 신규 추상화 없음. 기존 `Session.retriever: Option<Box<dyn ContextRetriever>>`(Plan 11)를 그대로 재사용. `Command::Search(String)` 추가 -> `Session::step`이 retriever로 검색해 결과를 렌더(run_round 호출 아님, 표시만). retriever 없으면(--db 미지정/sqlite off) "검색 비활성(--db 필요)" 안내.

**Tech Stack:** Rust 2024. 신규 의존성 없음. 선행: Plan 11 done.

> 규율: #5/#6, TDD, 위임 Sonnet + Opus 리뷰, 검증/commit 분리.

---

## 범위

- **포함:** `Command::Search(String)` 파싱(`/search <q>`) + `Session::step` 핸들러(retriever.retrieve로 검색·렌더) + `/help` 갱신 + 테스트.
- **비포함:** 에이전트(러너)가 도구로 호출하는 능동 검색(MCP/툴콜) · 벡터/하이브리드 · 검색 결과 필터·페이지네이션.
- **불변식:** 기존 명령·동작 불변. retriever 없으면 안내만(검색 안 함). `/search` 인자 없으면 일반 메시지로 폴스루(기존 명령 패턴 답습).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/repl/mod.rs` | (수정) `Command::Search` variant + `parse_command` 분기 + `step` 핸들러 + `/help` 문구 + 테스트. |

---

### Task 1: /search 파싱 + 핸들러

**Files:**
- Modify: `src/repl/mod.rs`

- [ ] **Step 1: 실패 테스트 먼저(`repl::tests`)**
```rust
    #[test]
    fn parses_search() {
        assert_eq!(parse_command("/search 검색 시스템"), Command::Search("검색 시스템".into()));
        // 인자 없으면 일반 메시지로 폴스루(기존 명령 패턴)
        assert_eq!(parse_command("/search"), Command::Message("/search".into()));
    }

    #[test]
    fn step_search_without_retriever_explains() {
        let mut s = session_with_two_seats(); // retriever 없음
        match s.step(Command::Search("아무거나".into())) {
            StepOutcome::Print(t) => assert!(t.contains("검색") && t.contains("--db")),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn step_search_with_retriever_renders_hits() {
        // FakeRetriever(고정 Utterance 반환)로 검색 결과 렌더 확인.
        struct FakeRetriever(Vec<Utterance>);
        impl crate::orchestrator::ContextRetriever for FakeRetriever {
            fn retrieve(&self, _q: &str, _l: usize) -> Vec<Utterance> { self.0.clone() }
        }
        let hits = vec![Utterance { speaker: "claude/proposer".into(), content: "검색 시스템 설계".into() }];
        let mut s = session_with_two_seats().with_retriever(Some(Box::new(FakeRetriever(hits))));
        match s.step(Command::Search("검색".into())) {
            StepOutcome::Print(t) => { assert!(t.contains("검색 시스템 설계")); assert!(t.contains("claude/proposer")); }
            other => panic!("got {other:?}"),
        }
    }
```

- [ ] **Step 2: 구현**
  - `Command`에 `Search(String)` 추가.
  - `parse_command`: `"search"` 분기 - `arg`가 비면 `Command::Message(line)`(폴스루), 있으면 `Command::Search(arg)`. (기존 `save`/`conclude` 패턴 답습.)
  - `Session::step`에 `Command::Search(q)` 핸들러:
    - retriever 없으면 `StepOutcome::Print("검색이 비활성화돼 있습니다. --db <경로>로 실행하면 인덱스를 검색할 수 있습니다.".into())`.
    - 있으면 `const SEARCH_K: usize = 10;`로 `r.retrieve(&q, SEARCH_K)` -> 결과를 렌더. 빈 결과면 "검색 결과 없음: {q}". 결과 있으면 헤더 + `render(&hits)`(기존 render 재사용) 또는 간결 목록. **주의:** retrieve는 Vec<Utterance>라 score/세션ID 없음 - 이 슬라이스는 speaker+content 표시로 충분(richer 메타는 후속).
  - `/help` 문구에 `/search <질의> 인덱스 검색(--db 필요)` 추가.

- [ ] **Step 3: 검증 + 커밋**
  - `cargo test`(기본) — 기존 + 신규 3개 PASS(retriever=None 경로는 sqlite 불필요, FakeRetriever라 sqlite off에서도 컴파일·통과). `cargo test --features sqlite` / `--features "sqlite morphology"` PASS. build/clippy 3조합 경고 0.
  - 스모크(선택): `cargo run --features sqlite -- --db ./tmp.db` 후 `/search` 입력 한 번(수동, 결과 표시 확인).
  - 커밋(push 금지): `feat(repl): /search 명령 - 인덱스 직접 검색(retriever 재사용)`.

---

## Self-Review (작성자 체크)

- **설계 정렬:** 로드맵 "능동 검색 도구"의 사람용 첫 발. 벡터(YAGNI)는 보류, 대신 FTS 품질을 실측 가능하게 함.
- **재사용:** 기존 ContextRetriever/Session.retriever 그대로 - 신규 추상화/의존성 0.
- **불변:** 기존 명령·동작 불변. retriever 없으면 안내만. 인자 없는 /search는 폴스루(기존 패턴).
- **범위:** 표시 전용(run_round 아님). 에이전트 툴콜·벡터는 비포함.

## 위험 / 한계 (문서화된 후속)

- **메타 부족:** retrieve가 Utterance만 반환 -> 결과에 세션ID/score/스니펫 위치 없음. richer 검색 결과(SearchHit 직접 노출)는 후속.
- **에이전트 능동 검색:** 이번은 사람이 치는 /search. 러너가 도구로 검색을 호출(MCP/툴콜)하는 진짜 "능동"은 별 슬라이스.
- **품질 관측 -> 벡터 결정:** /search로 FTS가 의미 매칭을 놓치는 사례가 쌓이면 그것이 벡터(Plan 원안) 도입의 YAGNI 입증이 된다.
