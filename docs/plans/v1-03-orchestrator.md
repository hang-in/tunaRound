---
title: "tunaRound v1 Plan 03: 토론 오케스트레이터"
type: plan
status: done
priority: P0
updated_at: 2026-06-29
owner: shared
summary: 사람 주도 토론 오케스트레이터. 역할 지시문(roles) + 순차-인지 라운드 프롬프트 조립(build_round_prompt 순수함수) + 두 러너를 Runner trait로 주입해 한 라운드를 구동하는 run_round(FakeRunner로 테스트). consensus 자동추출·자리지목 UI는 후속.
---

# tunaRound v1 Plan 03: 토론 오케스트레이터 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`).

**Goal:** 사람 메시지를 받아 각 자리(역할 부여 엔진)가 순차-인지로 응답하는 한 라운드를, 실제 CLI 없이 FakeRunner로 검증 가능하게 구동한다.

**Architecture:** 코어는 framework-independent. `roles`(역할 지시문)와 `build_round_prompt`(프롬프트 조립)는 순수함수. `run_round`는 Plan 01/02의 `Runner` trait에만 의존하고, 엔진→러너 매핑은 `RunnerRegistry`로 주입한다(테스트는 FakeRunner). 사람 주도라 자동 다라운드 루프는 없다(사람 메시지 1개 = 라운드 1개).

**Tech Stack:** Rust 2024, std only. 선행: Plan 01·02(done) - `runner::{RunInput,RunOutput,RunMode,RunError,Runner}`.

> 규율: docs/reference/development-guidelines.md. 청사진: tunapi `core/roundtable/{roles,prompt}.py`(실측). 설계 §4·§5.

---

## 실행 결과 (2026-06-29, done)

구현 완료(브랜치 `feat/v1-orchestrator` -> main). 전체 테스트 green(21 unit + 3 integration), `cargo build`/`clippy` 클린. 오케스트레이터는 `Runner` trait + `RunnerRegistry` 경계에만 의존(concrete 러너 미임포트, grep 확인).

- 작성 시 Task 3에 레지스트리 관찰 훅 churn이 있었으나 실행 전 정리. 순차-인지는 prompt.rs 단위테스트가 증명, 통합테스트는 구동+전사 누적만 단언.
- consensus 자동추출은 build_round_prompt에 주석 seam(`[v1.x]`)만 둠(후속).
- 커밋: 3a13954 -> 123ee5d -> c9af140.

## 범위

- **포함:** `src/orchestrator/` - `roles.rs`(canonical_role·role_guidance) / `prompt.rs`(Participant·Utterance·build_round_prompt 순수함수, 순차-인지) / `mod.rs`(RunnerRegistry + run_round, FakeRunner 테스트).
- **비포함(후속):** consensus 자동 추출(맴돌이 방지) → build_round_prompt에 주석 seam만. 자리 지목/쓰기 지목 UI·전사 영속(트리-ready) → Plan 04/05. 멀티라운드 자동 루프 → 비목표.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/lib.rs` | (수정) `pub mod orchestrator;` 추가 |
| `src/orchestrator/mod.rs` | (신규) `Participant`·`Utterance` 타입 + `RunnerRegistry` + `run_round` |
| `src/orchestrator/roles.rs` | (신규) 역할 지시문(순수) |
| `src/orchestrator/prompt.rs` | (신규) `build_round_prompt`(순수, 순차-인지) |

> 선제 설계: roles·prompt 순수함수, Participant/Utterance 처음부터 타입, run_round는 Runner trait 경계에만 의존(concrete 러너 무관).

---

### Task 1: 역할 지시문 (roles)

**Files:**
- Modify: `src/lib.rs` (`pub mod orchestrator;`)
- Create: `src/orchestrator/mod.rs` (scaffold: `pub mod roles; pub mod prompt;` + 타입 stub은 Task 2/3에서)
- Create: `src/orchestrator/roles.rs`

- [ ] **Step 1: lib.rs + mod.rs 스캐폴드**
`src/lib.rs`에 `pub mod orchestrator;` 추가(`pub mod runner;` 옆).
`src/orchestrator/mod.rs` 생성, 첫 줄 `// 사람 주도 토론 오케스트레이터의 경계. 역할·프롬프트·라운드 구동.` 그 아래 `pub mod roles;`(prompt 모듈은 Task 2에서 추가).

- [ ] **Step 2: 실패 테스트 (`src/orchestrator/roles.rs`)**
파일 첫 줄: `// 토론 자리의 역할별 행동 지시문. 같은 엔진이 다른 역할을 연기하게 한다(tunapi roles.py 답습).`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_maps_aliases() {
        assert_eq!(canonical_role(Some("critic")), Some("reviewer"));
        assert_eq!(canonical_role(Some("Judge")), Some("verifier"));
        assert_eq!(canonical_role(Some("lead")), Some("synthesizer"));
        assert_eq!(canonical_role(Some("nope")), None);
        assert_eq!(canonical_role(None), None);
    }

    #[test]
    fn guidance_nonempty_for_known_empty_for_unknown() {
        assert!(role_guidance(Some("proposer")).contains("proposal"));
        assert!(role_guidance(Some("reviewer")).contains("verdict"));
        assert_eq!(role_guidance(Some("nope")), "");
        assert_eq!(role_guidance(None), "");
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test --lib orchestrator::roles` → FAIL(미정의).

- [ ] **Step 4: 구현 (roles.rs, 테스트 위)**
```rust
/// 별칭을 표준 역할명으로 정규화. 모르는/None은 None.
pub fn canonical_role(role: Option<&str>) -> Option<&'static str> {
    match role?.trim().to_lowercase().as_str() {
        "proposer" => Some("proposer"),
        "reviewer" | "critic" => Some("reviewer"),
        "verifier" | "judge" => Some("verifier"),
        "synthesizer" | "lead" => Some("synthesizer"),
        _ => None,
    }
}

/// 역할별 행동 지시문. 모르는/None이면 "".
pub fn role_guidance(role: Option<&str>) -> &'static str {
    match canonical_role(role) {
        Some("proposer") => concat!(
            "Put forward a clear position or proposal with concrete rationale.\n",
            "State your key claims up front; support each with evidence or examples.\n",
            "Keep the proposal focused and actionable.\n",
            "Invite specific critique rather than seeking blanket agreement.",
        ),
        Some("reviewer") => concat!(
            "Critique others' proposals: identify strengths, weaknesses, and risks.\n",
            "Be specific - reference exact claims rather than vague impressions.\n",
            "Acknowledge what works before flagging concerns.\n",
            "End with a one-line verdict: agree / disagree / conditional.",
        ),
        Some("verifier") => concat!(
            "Independently judge the soundness of each proposal.\n",
            "Do NOT defer to other participants; verify claims from first principles.\n",
            "Flag any unsupported or contradictory claims explicitly.\n",
            "State your own conclusion clearly, even if it diverges from the group.",
        ),
        Some("synthesizer") => concat!(
            "Reduce all responses into: ## Consensus, ## Disagreements, ## Open questions.\n",
            "Preserve each participant's verdict - do not overwrite or reinterpret them.\n",
            "Highlight where proposals align and where they conflict.\n",
            "End with a final recommendation grounded in the discussion.",
        ),
        _ => "",
    }
}
```

- [ ] **Step 5: 통과 + 커밋** — `cargo test --lib orchestrator::roles` PASS.
`git add src/lib.rs src/orchestrator/mod.rs src/orchestrator/roles.rs && git commit -m "feat(orchestrator): 역할 지시문 (roles)"` (push 금지).

---

### Task 2: 라운드 프롬프트 조립 (순차-인지)

**Files:**
- Modify: `src/orchestrator/mod.rs` (`Participant`·`Utterance` 타입 + `pub mod prompt;`)
- Create: `src/orchestrator/prompt.rs`

- [ ] **Step 1: 도메인 타입 (`src/orchestrator/mod.rs`)**
mod.rs에 추가(헤더 아래):
```rust
pub mod prompt;

/// 토론 한 자리. 엔진(어떤 러너로 구동) + 역할 + 추가 지시.
#[derive(Debug, Clone)]
pub struct Participant {
    pub engine: String,
    pub role: Option<String>,
    pub instruction: String,
}

impl Participant {
    /// 전사 표기용 라벨(역할 있으면 "engine/role", 없으면 engine).
    pub fn label(&self) -> String {
        match &self.role {
            Some(r) => format!("{}/{}", self.engine, r),
            None => self.engine.clone(),
        }
    }
}

/// 한 발언. speaker=Participant.label(), content=응답 본문.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: String,
    pub content: String,
}
```

- [ ] **Step 2: 실패 테스트 (`src/orchestrator/prompt.rs`)**
파일 첫 줄: `// 한 자리의 라운드 프롬프트를 조립하는 순수 함수(tunapi prompt.py 답습, 순차-인지).`
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{Participant, Utterance};

    fn p(engine: &str, role: Option<&str>) -> Participant {
        Participant { engine: engine.into(), role: role.map(|s| s.into()), instruction: String::new() }
    }

    #[test]
    fn prompt_includes_role_directive_and_topic() {
        let out = build_round_prompt(&p("claude", Some("reviewer")), "이 설계 어떤가요?", &[], &[]);
        assert!(out.contains("## Your role"));
        assert!(out.contains("verdict"));
        assert!(out.contains("이 설계 어떤가요?"));
    }

    #[test]
    fn prompt_sequential_aware_includes_same_round_responses() {
        let same = vec![Utterance { speaker: "claude/architect".into(), content: "API부터 잡자".into() }];
        let out = build_round_prompt(&p("codex", Some("reviewer")), "주제", &[], &same);
        assert!(out.contains("이번 라운드 다른 에이전트 답변"));
        assert!(out.contains("API부터 잡자"));
        assert!(out.contains("claude/architect"));
    }

    #[test]
    fn prompt_includes_prior_rounds() {
        let prior = vec![Utterance { speaker: "codex".into(), content: "지난 결론".into() }];
        let out = build_round_prompt(&p("claude", None), "주제", &prior, &[]);
        assert!(out.contains("이전 라운드 응답"));
        assert!(out.contains("지난 결론"));
    }

    #[test]
    fn prompt_appends_instruction() {
        let mut part = p("claude", Some("proposer"));
        part.instruction = "API 설계에 집중".into();
        let out = build_round_prompt(&part, "주제", &[], &[]);
        assert!(out.contains("API 설계에 집중"));
    }
}
```

- [ ] **Step 3: 실패 확인** — `cargo test --lib orchestrator::prompt` → FAIL.

- [ ] **Step 4: 구현 (prompt.rs)**
```rust
use crate::orchestrator::roles::role_guidance;
use crate::orchestrator::{Participant, Utterance};

/// 컨텍스트에 넣는 발언 본문 최대 길이(tunapi _MAX_ANSWER_LENGTH 답습).
const MAX_ANSWER_LEN: usize = 4000;

fn join_utterances(utts: &[Utterance]) -> String {
    utts.iter()
        .map(|u| {
            let body: String = u.content.chars().take(MAX_ANSWER_LEN).collect();
            format!("**[{}]**:\n{}", u.speaker, body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// 한 자리의 라운드 프롬프트를 조립한다.
/// 순서: 역할 지시 → (이전 라운드) → (이번 라운드 앞 자리 = 순차-인지) → 주제.
/// 빈 컨텍스트면 주제만. role/instruction이 없으면 해당 섹션 생략.
pub fn build_round_prompt(
    participant: &Participant,
    topic: &str,
    prior: &[Utterance],
    same_round: &[Utterance],
) -> String {
    let mut sections: Vec<String> = Vec::new();
    // [v1.x] consensus carry-forward: 합의 추출 후 여기에 "이미 합의된 사항(전제)" 섹션 주입. 삭제 금지.
    if !prior.is_empty() {
        sections.push(format!("이전 라운드 응답:\n\n{}", join_utterances(prior)));
    }
    if !same_round.is_empty() {
        sections.push(format!("이번 라운드 다른 에이전트 답변:\n\n{}", join_utterances(same_round)));
    }

    let body = if sections.is_empty() {
        topic.to_string()
    } else {
        format!(
            "{}\n\n---\n\n위 의견들을 참고하여 답변해주세요: {}",
            sections.join("\n\n---\n\n"),
            topic
        )
    };

    let mut directive = role_guidance(participant.role.as_deref()).to_string();
    if !participant.instruction.is_empty() {
        if !directive.is_empty() {
            directive.push('\n');
        }
        directive.push_str(&participant.instruction);
    }

    if directive.is_empty() {
        body
    } else {
        format!("## Your role\n{}\n\n---\n\n{}", directive, body)
    }
}
```

- [ ] **Step 5: 통과 + 커밋** — `cargo test --lib orchestrator::prompt` PASS(4개).
`git add src/orchestrator/mod.rs src/orchestrator/prompt.rs && git commit -m "feat(orchestrator): 라운드 프롬프트 조립 (순차-인지)"` (push 금지).

---

### Task 3: 라운드 구동 (run_round + RunnerRegistry, FakeRunner 테스트)

**Files:**
- Modify: `src/orchestrator/mod.rs` (`RunnerRegistry` + `run_round`)
- Create: `tests/orchestrator_round.rs`

- [ ] **Step 1: 실패 통합테스트 (`tests/orchestrator_round.rs`)**
```rust
// run_round가 자리들을 구동하고 전사를 누적하는지 FakeRunner로 검증(실 CLI 없음).
use tunaround::orchestrator::{run_round, MapRegistry, Participant, Utterance};
use tunaround::runner::{RunError, RunInput, RunOutput, Runner};

/// 고정 응답을 내는 가짜 러너.
struct FakeRunner {
    reply: String,
}
impl Runner for FakeRunner {
    fn run(&self, _input: &RunInput) -> Result<RunOutput, RunError> {
        Ok(RunOutput { content: self.reply.clone(), input_tokens: 0, output_tokens: 0 })
    }
}

#[test]
fn run_round_drives_seats_and_accumulates_transcript() {
    let mut reg = MapRegistry::new();
    reg.insert("claude", Box::new(FakeRunner { reply: "아키텍트 의견".into() }));
    reg.insert("codex", Box::new(FakeRunner { reply: "리뷰어 의견".into() }));

    let participants = vec![
        Participant { engine: "claude".into(), role: Some("architect".into()), instruction: String::new() },
        Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
    ];
    let mut transcript: Vec<Utterance> = Vec::new();

    let round = run_round(&participants, &mut transcript, "이 설계 어떤가요?", &reg).expect("ok");

    assert_eq!(round.len(), 2);
    assert_eq!(round[0].content, "아키텍트 의견");
    assert_eq!(round[1].content, "리뷰어 의견");
    assert_eq!(transcript.len(), 2);
}
// 순차-인지(2번째 자리가 1번째 응답을 봄)는 Task 2 prompt.rs 단위테스트가 증명한다.
```

- [ ] **Step 2: 실패 확인** — `cargo test --test orchestrator_round` → FAIL(`run_round`/`MapRegistry` 미정의).

- [ ] **Step 3: 구현 (`src/orchestrator/mod.rs`)**
import 추가 + 아래를 mod.rs에 추가:
```rust
use std::collections::HashMap;
use crate::orchestrator::prompt::build_round_prompt;
use crate::runner::{RunInput, RunMode, RunError, RunOutput, Runner};

/// 엔진 이름 → 러너 조회 경계. 오케스트레이터는 이 trait에만 의존한다.
pub trait RunnerRegistry {
    fn get(&self, engine: &str) -> Option<&dyn Runner>;
}

/// HashMap 기반 기본 레지스트리. 테스트는 FakeRunner를 넣는다.
pub struct MapRegistry {
    runners: HashMap<String, Box<dyn Runner>>,
}

impl MapRegistry {
    pub fn new() -> Self {
        Self { runners: HashMap::new() }
    }
    pub fn insert(&mut self, engine: &str, runner: Box<dyn Runner>) {
        self.runners.insert(engine.to_string(), runner);
    }
}

impl Default for MapRegistry {
    fn default() -> Self { Self::new() }
}

impl RunnerRegistry for MapRegistry {
    fn get(&self, engine: &str) -> Option<&dyn Runner> {
        self.runners.get(engine).map(|b| b.as_ref())
    }
}

/// 한 라운드를 구동한다. 사람 주도이므로 topic = 사용자 메시지.
/// 각 자리를 순서대로 호출하되, 뒤 자리는 같은 라운드 앞 응답을 본다(순차-인지).
/// transcript는 이전 라운드들이며, 이번 라운드 응답이 끝에 append된다.
pub fn run_round(
    participants: &[Participant],
    transcript: &mut Vec<Utterance>,
    topic: &str,
    registry: &dyn RunnerRegistry,
) -> Result<Vec<Utterance>, RunError> {
    let prior: Vec<Utterance> = transcript.clone();
    let mut same_round: Vec<Utterance> = Vec::new();

    for part in participants {
        let prompt = build_round_prompt(part, topic, &prior, &same_round);
        let runner = registry
            .get(&part.engine)
            .ok_or_else(|| RunError::Spawn(format!("엔진 러너 없음: {}", part.engine)))?;
        // v1 토론 턴은 읽기 전용(쓰기 지목은 Plan 05 REPL에서 mode 분기).
        let input = RunInput {
            prompt,
            model: None,
            project_path: None,
            mode: RunMode::ReadOnly,
        };
        let out: RunOutput = runner.run(&input)?;
        same_round.push(Utterance { speaker: part.label(), content: out.content });
    }

    transcript.extend(same_round.iter().cloned());
    Ok(same_round)
}
```
- [ ] **Step 4: 통과 + 전체 검증 + 커밋**
- `cargo test --test orchestrator_round` PASS.
- `cargo test`(전체), `cargo build`(경고 0), `cargo clippy --all-targets`(클린).
- `git add src/orchestrator/mod.rs tests/orchestrator_round.rs && git commit -m "feat(orchestrator): run_round + RunnerRegistry (FakeRunner 통합테스트)"` (push 금지).

---

## Self-Review (작성자 체크)

- **spec 커버리지:** 역할 주입(roles) + 순차-인지 프롬프트 조립(build_round_prompt) + 두 러너를 trait로 주입한 라운드 구동(run_round). 사람 주도(라운드=사용자 메시지). consensus 자동추출·자리/쓰기 지목 UI·영속은 명시적 후속.
- **placeholder:** 없음. Task 3 Step 3-4는 의도적 단순화(레지스트리 관찰 훅 제거) 지시 — 구현자가 따른다.
- **타입 일관성:** Participant/Utterance를 Task 2 mod.rs에서 정의, prompt.rs·run_round·테스트에서 동일 사용. run_round/RunnerRegistry/MapRegistry/build_round_prompt/role_guidance 이름 일관. RunInput/RunOutput/RunError/Runner는 Plan 01/02 재사용.
- **선제 설계:** roles·prompt 순수함수, run_round는 Runner trait·RunnerRegistry 경계에만 의존(concrete 러너 무관), consensus는 코드 아닌 주석 seam.

## 다음 plan

- **Plan 04:** 전사·영속(트리-ready 메시지 id/parent + 결과 문서). run_round의 in-memory transcript를 영속 모델로.
- **Plan 05:** thin REPL(사용자 입력 → run_round → 렌더, 자리/쓰기 지목 mode 분기).
- **Hardening:** 양 러너 idle watchdog + consensus carry-forward + 실 CLI 스모크.
