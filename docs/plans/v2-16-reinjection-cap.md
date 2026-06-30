---
title: "tunaRound v2 Plan 16: 재주입 축소 (최근 N턴 + 검색 슬라이스, opt-in)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 북극성의 스케일 페이오프. 지금 build_round_prompt는 활성 경로 전사를 통째로 재주입한다. 검색 주입(Plan 11)이 동작하므로, prior를 최근 N턴으로 캡하고 나머지 맥락은 검색 슬라이스가 담당하게 한다("통째 재주입 -> 최근 N턴 + 검색"). 검증된 기본 동작 보존을 위해 opt-in: Session.recent_turns(None=현행 무제한, 기본). --recent-turns N 줄 때만 축소. retrieve_for의 dedup은 전체 경로 유지. 작은 변경(step의 prior_for_prompt 헬퍼).
---

# tunaRound v2 Plan 16: 재주입 축소 Implementation Plan

> **For agentic workers:** TDD. **cargo는 Bash 툴로.**
> 결정: docs/design/v2-context-memory-direction(전사 통째 재주입 -> 검색 슬라이스). Plan 11(검색 주입) done이 전제. 설계 원칙 "측정 후 축소" -> opt-in 단계 롤아웃. 아키텍처 재론 금지.

**Goal:** 긴 토론에서 매 라운드 전사를 통째로 재주입하는 토큰 병목을 줄인다. prior(이전 라운드 전사)를 **최근 N턴**으로 제한하고, 그보다 오래된/다른 분기의 관련 맥락은 **검색 주입(Plan 11)**이 끌어오게 한다. 기본 동작(무제한 재주입, 검증됨)은 보존하고 opt-in으로 켠다.

**Architecture:** Session에 `recent_turns: Option<usize>` 추가(None=현행 무제한). step의 각 run_round 호출에서 `prior`로 넘기는 활성 경로를 `prior_for_prompt()`(recent_turns Some이면 tail N)로 캡. 검색 슬라이스(retrieved)는 그대로 주입되고, retrieve_for의 활성-경로 dedup은 **전체 경로** 유지(중복 방지 정확성). main `--recent-turns N` 플래그. retrieved + 최근 N턴 조합으로 "관련 맥락은 유지, 토큰은 절감".

**Tech Stack:** Rust 2024. 신규 의존성 없음. 선행: Plan 11 done.

> 규율 #5/#6, TDD, 위임 Sonnet + Opus 리뷰. **불변:** recent_turns None(기본) = 현행 통째 재주입 = 기존 테스트 전부 통과.

---

## 범위

- **포함:** Session `recent_turns` 필드 + `with_recent_turns` 빌더 + `prior_for_prompt()` 헬퍼(step의 active_path 사용처를 prior 용도로 교체) + main `--recent-turns N` + 테스트.
- **비포함:** ctx-handle(참조 전달·온디맨드 expand) · 자동 N 튜닝 · 요약 압축. 단순 tail-N 캡만.
- **불변식:** recent_turns None = `prior_for_prompt() == active_path()` = 기존 프롬프트·동작·테스트 불변. retrieve_for dedup은 전체 경로(축소 무관).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/repl/mod.rs` | (수정) Session `recent_turns` + `with_recent_turns` + `prior_for_prompt()` + step 5곳의 prior 경로 교체 + 테스트. |
| `src/main.rs` | (수정) `--recent-turns N` 파싱 + Session 배선. |

---

### Task 1: prior_for_prompt 캡 + 배선

**Files:** Modify `src/repl/mod.rs`, `src/main.rs`

- [ ] **Step 1: 실패 테스트 먼저(`repl::tests`)**
```rust
    #[test]
    fn prior_for_prompt_uncapped_by_default() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제1".into())); // 메시지 2개
        let _ = s.step(Command::Message("주제2".into())); // 총 4개
        // 기본(None) = 전체 활성 경로.
        assert_eq!(s.prior_for_prompt().len(), s.transcript_len());
    }

    #[test]
    fn prior_for_prompt_caps_to_recent_n() {
        let mut s = session_with_two_seats().with_recent_turns(Some(2));
        let _ = s.step(Command::Message("주제1".into()));
        let _ = s.step(Command::Message("주제2".into())); // 활성 경로 4
        let prior = s.prior_for_prompt();
        assert_eq!(prior.len(), 2); // 최근 2턴만
        // 최근 것이 유지됨(마지막 발언 포함).
        let full = s.active_path_pub_for_test(); // 또는 transcript 비교
        assert_eq!(prior.last().map(|u| &u.content), full.last().map(|u| &u.content));
    }
```
  (active_path가 private이면 테스트용 접근은 transcript_markdown/len 등 기존 공개 API로 대체하거나 prior_for_prompt만 검증.)

- [ ] **Step 2: 구현(`src/repl/mod.rs`)**
  - Session에 `recent_turns: Option<usize>` 필드(기존 생성자 None, behavior-preserving). `pub fn with_recent_turns(mut self, n: Option<usize>) -> Self`.
  - `pub fn prior_for_prompt(&self) -> Vec<Utterance>`: `let p = self.active_path(); match self.recent_turns { Some(n) if p.len() > n => p[p.len()-n..].to_vec(), _ => p }`.
  - step의 각 분기(Message/Only/Write/Conclude/Debate)에서 run_round에 넘기는 `let mut path = self.active_path();`를 **`let mut path = self.prior_for_prompt();`**로 교체. (retrieve_for는 그대로 active_path 기반 dedup 유지 - 변경 금지.)
  - **주의:** run_round는 path를 transcript로 받아 prior로 클론 후 round를 extend한다. path는 프롬프트 조립용 로컬이고 트리는 append_round가 별도 관리하므로, 캡은 재주입 분량만 줄이고 트리/저장엔 영향 없음(확인).

- [ ] **Step 3: main 배선** — `--recent-turns N` 파싱(usize). Session 생성 뒤 `.with_recent_turns(parsed)`로 1회 적용(retriever 빌더처럼). 미지정=None=현행.

- [ ] **Step 4: 검증 + 커밋**
  - `cargo test`(기본) — 기존 전부 불변(None 경로) + 신규 PASS. `--features sqlite` 등 불변. clippy 0.
  - 스모크(선택): `--recent-turns 2`로 여러 라운드 후 프롬프트가 줄어드는지(로그/수동).
  - 커밋: `feat(repl): 재주입 축소(--recent-turns, 최근 N턴 + 검색 슬라이스, opt-in)`.

---

## Self-Review (작성자 체크)
- **북극성 완성:** "통째 재주입 -> 최근 N턴 + 검색 슬라이스" 실현. 검색 주입(Plan 11)이 오래된 맥락을 보완.
- **opt-in/불변:** 기본 None = 현행 무제한 재주입 = 검증된 멀티라운드 품질 보존. 단계 롤아웃(측정 후 기본화 검토).
- **정확성:** retrieve_for dedup은 전체 경로 유지(캡과 무관). 캡은 프롬프트 재주입 분량만, 트리/저장 무영향.
- **범위:** 단순 tail-N. ctx-handle·요약은 후속.

## 위험 / 한계 (후속)
- **맥락 손실 가능:** N이 너무 작고 검색이 관련 맥락을 못 끌면 품질 저하 가능 -> opt-in + 보수적 사용, 측정 권장.
- **검색 의존:** 축소 모드의 품질은 retrieve 품질에 의존(FTS/하이브리드). --db 없이 --recent-turns만 쓰면 오래된 맥락이 통째로 빠질 수 있음 -> 문서화(검색과 함께 쓰라).
- **자동 튜닝 없음:** N 고정. 토큰 예산 기반 동적 캡은 후속.
- **요약 압축 없음:** 잘린 맥락의 요약 carry-forward는 미구현(consensus carry-forward 자리와 연계 후속).
