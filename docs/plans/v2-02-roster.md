---
title: "tunaRound v2 Plan 02: 설정 구동 N좌석 로스터"
type: plan
status: done
priority: P1
updated_at: 2026-06-29
owner: shared
summary: 하드코딩된 2자리(main.rs)를 JSON 로스터 파일로 구동되는 N자리로. 오케스트레이터는 이미 N-ready(run_round이 임의 길이 순회)라 변경은 main.rs + 신규 roster 로더에 집중. engine/role/instruction 구성, 같은 엔진 다중 좌석 허용. 신규 의존성 0(serde_json 기존). 비포함: 신규 엔진 러너(tunaLlama/opencode), per-seat model.
---

# tunaRound v2 Plan 02: 설정 구동 N좌석 로스터 Implementation Plan

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v2-roster` -> main). 48 테스트 green(기존 43 + roster 신규 5), `cargo build`/`clippy` 경고 0. 스모크 3종 통과(--roster 정상 / 없는파일 에러+exit1 / positional state backward compat). Opus 리뷰: 계획서 충실, 큰 문제 없음.

- Task 1: `src/roster.rs`(Roster/SeatConfig serde + parse/load/build_participants(_checked)/build_registry) + `src/lib.rs` (커밋 `af69db9`).
- Task 2: `src/main.rs` `--roster` 수동 플래그 파싱 + 예시 `examples/roster.json` (커밋 `bb23e22`).
- 계획서 대비: `build_registry_unknown_engine_errors` 테스트가 `.unwrap_err()` 대신 `.err().unwrap()`(MapRegistry가 Debug 미구현, orchestrator 범위 밖이라 테스트만 최소 수정). 검증 효과 동일.
- 사소(비차단, 후속): `--roster` 뒤 경로 누락 시 조용히 기본 좌석 폴백. 알려진 엔진 claude/codex만(신규 엔진 러너는 후속). per-seat model 미지원.

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`). TDD red->green.
> 선행: v2 Plan 01(idle watchdog) 머지 후. 이 plan은 runner/*를 건드리지 않는다(main.rs + 신규 roster 모듈).

**Goal:** 토론 좌석 구성을 코드 하드코딩에서 JSON 로스터 파일로 옮긴다. 사용자가 자리 수·엔진·역할·추가 지시를 구성할 수 있게 한다(예: claude 2자리 proposer+critic, 또는 codex 단독 3역할).

**Architecture:** `src/orchestrator/`의 `run_round`/`Participant`/`MapRegistry`는 이미 임의 길이·임의 엔진을 지원한다(N-ready 확인됨). 하드코딩된 건 `main.rs`의 2자리 + 2러너뿐. 따라서: (1) 신규 `src/roster.rs`가 JSON을 읽어 `Vec<Participant>` + `MapRegistry`를 만든다. (2) `main.rs`가 `--roster <path>` 플래그를 파싱해 로스터를 적용, 없으면 기존 기본 2자리. 오케스트레이터/REPL/runner 무변경.

**Tech Stack:** Rust 2024, `serde`(derive)/`serde_json` 기존 의존성만. 신규 의존성 0(toml 안 씀, clap 안 씀 - 플래그는 수동 파싱).

> 규율: #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, TDD.

---

## 범위

- **포함:** 신규 `src/roster.rs`(JSON 로스터 -> participants + registry, 알려진 엔진 claude/codex 매핑, 미지 엔진/빈 좌석 에러). `main.rs`에 `--roster <path>` 플래그(없으면 기본 2자리, 기존 positional state 인자 유지). 예시 로스터 파일 1개(`examples/roster.json`).
- **비포함(후속):** 신규 엔진 러너(tunaLlama·opencode 좌석 = 별 plan, 외부 CLI 통합), per-seat model 주입(Participant.model 추가 = 별 작업), 로스터 핫리로드.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/roster.rs` | (신규) `Roster`/`SeatConfig`(serde) + `load_roster(path)` + `build_participants` + `build_registry`(엔진->러너). |
| `src/lib.rs` | (수정) `pub mod roster;` 추가. |
| `src/main.rs` | (수정) `--roster <path>` 수동 파싱 + 로스터 적용(없으면 기본 2자리). |
| `examples/roster.json` | (신규) 예시 로스터(claude proposer + codex reviewer). |

> 선제 설계: 오케스트레이터 N-ready를 활용(새 경로 발명 안 함). 미지 엔진은 startup에서 명확히 에러(런타임 run_round 실패보다 빠른 피드백). registry는 엔진별 1러너(같은 엔진 다중 좌석은 같은 러너 공유 - run_round이 자리별 프롬프트를 다르게 줌).

---

### Task 1: roster 모듈 (JSON -> participants + registry)

**Files:**
- Create: `src/roster.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: 실패 테스트 먼저 (`src/roster.rs`의 `mod tests`)**
  - 헤더 한 줄 주석(#6): `// JSON 로스터 파일을 토론 좌석(Participant) + 러너 레지스트리로 만드는 로더.`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_roster_with_defaults() {
        let json = r#"{"seats":[
            {"engine":"claude","role":"proposer"},
            {"engine":"codex"}
        ]}"#;
        let roster: Roster = parse_roster(json).expect("ok");
        assert_eq!(roster.seats.len(), 2);
        assert_eq!(roster.seats[0].role.as_deref(), Some("proposer"));
        assert_eq!(roster.seats[1].role, None);          // 기본 None
        assert_eq!(roster.seats[1].instruction, "");     // 기본 빈 문자열
    }

    #[test]
    fn build_participants_maps_fields() {
        let roster = parse_roster(r#"{"seats":[{"engine":"claude","role":"proposer","instruction":"간결히"}]}"#).unwrap();
        let parts = build_participants(&roster);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].engine, "claude");
        assert_eq!(parts[0].role.as_deref(), Some("proposer"));
        assert_eq!(parts[0].instruction, "간결히");
    }

    #[test]
    fn build_registry_known_engines_ok() {
        let roster = parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(build_registry(&roster).is_ok());
    }

    #[test]
    fn build_registry_unknown_engine_errors() {
        let roster = parse_roster(r#"{"seats":[{"engine":"gemini"}]}"#).unwrap();
        let err = build_registry(&roster).unwrap_err();
        assert!(err.contains("gemini"));
    }

    #[test]
    fn empty_seats_is_error() {
        let roster = parse_roster(r#"{"seats":[]}"#).unwrap();
        assert!(build_participants_checked(&roster).is_err());
    }
}
```

- [ ] **Step 2: 실패 확인** — `cargo test --lib roster` -> 컴파일 에러/FAIL.

- [ ] **Step 3: 구현 (`src/roster.rs`, tests 위)**
```rust
use serde::Deserialize;

use crate::orchestrator::{MapRegistry, Participant};
use crate::runner::claude::ClaudeRunner;
use crate::runner::codex::CodexRunner;

/// 로스터 파일 루트. 좌석 목록.
#[derive(Debug, Clone, Deserialize)]
pub struct Roster {
    pub seats: Vec<SeatConfig>,
}

/// 한 좌석 설정. engine 필수, 나머지는 기본값.
#[derive(Debug, Clone, Deserialize)]
pub struct SeatConfig {
    pub engine: String,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub instruction: String,
}

/// JSON 문자열을 Roster로 파싱한다.
pub fn parse_roster(json: &str) -> Result<Roster, String> {
    serde_json::from_str(json).map_err(|e| format!("로스터 파싱 실패: {e}"))
}

/// 파일에서 로스터를 읽어 파싱한다.
pub fn load_roster(path: &str) -> Result<Roster, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("로스터 읽기 실패 ({path}): {e}"))?;
    parse_roster(&text)
}

/// 로스터 좌석을 토론 참가자로 변환한다.
pub fn build_participants(roster: &Roster) -> Vec<Participant> {
    roster
        .seats
        .iter()
        .map(|s| Participant {
            engine: s.engine.clone(),
            role: s.role.clone(),
            instruction: s.instruction.clone(),
        })
        .collect()
}

/// 빈 좌석을 거른 뒤 참가자를 만든다.
pub fn build_participants_checked(roster: &Roster) -> Result<Vec<Participant>, String> {
    if roster.seats.is_empty() {
        return Err("로스터에 좌석이 없습니다.".to_string());
    }
    Ok(build_participants(roster))
}

/// 로스터의 distinct 엔진마다 러너를 만들어 레지스트리를 구성한다.
/// 알려진 엔진: claude, codex. 그 외는 에러.
pub fn build_registry(roster: &Roster) -> Result<MapRegistry, String> {
    let mut reg = MapRegistry::new();
    let mut seen: Vec<String> = Vec::new();
    for seat in &roster.seats {
        if seen.contains(&seat.engine) {
            continue;
        }
        match seat.engine.as_str() {
            "claude" => reg.insert("claude", Box::new(ClaudeRunner::new())),
            "codex" => reg.insert("codex", Box::new(CodexRunner::new())),
            other => return Err(format!("알 수 없는 엔진: {other} (지원: claude, codex)")),
        }
        seen.push(seat.engine.clone());
    }
    Ok(reg)
}
```
  - `src/lib.rs`에 `pub mod roster;` 추가(다른 모듈 선언 옆).

- [ ] **Step 4: 통과 + 커밋** — `cargo test --lib roster` PASS, clippy 클린.
  `git add src/roster.rs src/lib.rs && git commit -m "feat(roster): JSON 로스터 로더 (participants + registry)"` (push 금지).

---

### Task 2: main.rs 로스터 배선 + 예시 파일

**Files:**
- Modify: `src/main.rs`
- Create: `examples/roster.json`

- [ ] **Step 1: 예시 로스터 작성 (`examples/roster.json`)**
```json
{
  "seats": [
    { "engine": "claude", "role": "proposer", "instruction": "" },
    { "engine": "codex", "role": "reviewer", "instruction": "" }
  ]
}
```

- [ ] **Step 2: `main.rs` 인자 파싱 교체**
  - 현재 `std::env::args().nth(1)`를 state 경로로 쓴다. 이를 수동 파서로 교체:
    - `--roster <path>`: 로스터 파일. 있으면 `load_roster` -> `build_participants_checked` + `build_registry`. 실패 시 명확한 메시지 + exit(1).
    - 나머지 첫 positional 인자: 기존처럼 state 경로(backward compat).
    - `--roster` 없으면 기존 기본 2자리(claude proposer + codex reviewer) 유지.
  - 스케치:
```rust
// 인자: [--roster <path>] [<state.json>]
let args: Vec<String> = std::env::args().skip(1).collect();
let mut roster_path: Option<String> = None;
let mut state_path: Option<String> = None;
let mut i = 0;
while i < args.len() {
    match args[i].as_str() {
        "--roster" => {
            roster_path = args.get(i + 1).cloned();
            i += 2;
        }
        other => {
            if state_path.is_none() {
                state_path = Some(other.to_string());
            }
            i += 1;
        }
    }
}

let (participants, registry): (Vec<Participant>, MapRegistry) = match &roster_path {
    Some(p) => {
        let roster = match tunaround::roster::load_roster(p) {
            Ok(r) => r,
            Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
        };
        let parts = match tunaround::roster::build_participants_checked(&roster) {
            Ok(v) => v,
            Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
        };
        let reg = match tunaround::roster::build_registry(&roster) {
            Ok(r) => r,
            Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
        };
        (parts, reg)
    }
    None => {
        let mut reg = MapRegistry::new();
        reg.insert("claude", Box::new(ClaudeRunner::new()));
        reg.insert("codex", Box::new(CodexRunner::new()));
        let parts = vec![
            Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
            Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
        ];
        (parts, reg)
    }
};
```
  - 이후 기존 resume/Session 구동 로직은 `registry`(MapRegistry)를 `Box::new(registry)`로 넘기는 형태로 연결(기존과 동일). state_path 기반 resume/save 로직 유지.
  - 주의: 타입 일치를 위해 두 분기 모두 `MapRegistry`를 반환(Box는 Session 생성 직전에). build_registry가 `MapRegistry`를 돌려주므로 일관.

- [ ] **Step 3: 비대화형 스모크(빌드 + 실행 인자 파싱 확인)**
  - `cargo build` 경고 0.
  - `printf '/quit\n' | cargo run -- --roster examples/roster.json` -> 배너 출력 후 정상 종료(exit 0), 로스터 파싱 에러 없음. (실 에이전트 호출은 /quit이라 발생 안 함.)
  - 잘못된 로스터: `printf '/quit\n' | cargo run -- --roster /nonexistent.json` -> `[로스터 실패]` 메시지 + 비정상 종료.

- [ ] **Step 4: 전체 검증 + 커밋**
  - `cargo test`(전체) PASS. `cargo clippy --all-targets` 클린.
  - `git add src/main.rs examples/roster.json && git commit -m "feat(roster): main.rs --roster 플래그 + 예시 로스터"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** N좌석 로스터(역할×엔진 동적 구성)의 핵심 = 설정 구동 좌석. 오케스트레이터 N-ready 활용. 같은 엔진 다중 좌석 지원(registry 엔진별 1러너 공유).
- **placeholder:** 없음.
- **타입 일관성:** `Roster`/`SeatConfig` serde derive. `build_*`가 기존 `Participant`/`MapRegistry` 그대로 사용(orchestrator 무변경). main 두 분기 모두 `(Vec<Participant>, MapRegistry)`.
- **backward compat:** `--roster` 없으면 기존 기본 2자리 + positional state 인자 유지. 기존 `cargo run -- state.json` 동작 불변.
- **선제 설계:** 신규 의존성 0. 미지 엔진 startup 에러(빠른 피드백). concrete 엔진은 roster/main만 의존(orchestrator 경계 유지).

## 위험 / 한계 (문서화된 후속)

- **per-seat model 미지원:** 현재 모든 좌석 model=None(러너 기본). 좌석별 모델은 Participant.model 추가 + run_round 반영 필요(별 작업, Participant 리터럴 churn 동반).
- **신규 엔진 러너 없음:** roster가 참조 가능한 엔진은 claude/codex뿐. tunaLlama/opencode 좌석은 외부 CLI 통합 별 plan.
- **로스터 검증 최소:** 중복 좌석/역할 충돌 등은 검증 안 함(허용). 잘못된 JSON/미지 엔진/빈 좌석만 에러.
