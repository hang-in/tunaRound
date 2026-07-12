// 한 자리의 라운드 프롬프트를 조립하는 순수 함수(tunapi prompt.py 답습, 순차-인지). carried 이월 요약 지원.

use crate::orchestrator::roles::role_guidance;
use crate::orchestrator::{Participant, Utterance};

/// 컨텍스트에 넣는 발언 본문 최대 길이(tunapi _MAX_ANSWER_LENGTH 답습).
const MAX_ANSWER_LEN: usize = 4000;

fn join_utterances(utts: &[Utterance]) -> String {
    utts.iter()
        .map(|u| {
            let body: String = u.content.chars().take(MAX_ANSWER_LEN).collect();
            // 큐레이션 표면화(v2-51): abstraction 있으면 증류 요약을 원문 앞에 얹는다. content(raw)는
            // repl 중복제거용이라 불변이고, 표면화는 실제 주입되는 이 렌더 경계에서만 일어난다.
            // abstraction도 발언 본문과 동일하게 MAX_ANSWER_LEN으로 캡한다(발언당 길이 상한 보장, CodeRabbit).
            let body = match &u.abstraction {
                Some(a) if !a.trim().is_empty() => {
                    let a_capped: String = a.trim().chars().take(MAX_ANSWER_LEN).collect();
                    format!("[요약] {}\n{}", a_capped, body)
                }
                _ => body,
            };
            format!("**[{}]**:\n{}", u.speaker, body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// build_round_prompt에 넘기는 라운드 맥락 인자 묶음. 파라미터 폭발 방지용.
pub struct PromptContext<'a> {
    /// 이전 라운드들의 발언 슬라이스(recent_turns 적용 후).
    pub prior: &'a [Utterance],
    /// 이번 라운드에서 앞선 자리의 응답(순차-인지).
    pub same_round: &'a [Utterance],
    /// 검색으로 끌어온 과거 맥락 슬라이스(push 모드에서만 주입).
    pub retrieved: &'a [Utterance],
    /// 드롭된 옛 턴의 압축 이월 요약(비면 이월 섹션 없음).
    pub carried: &'a str,
    /// true이면 pull 모드(포인터 프롬프트), false이면 push 모드(기본).
    pub pull: bool,
    /// 활성 전사 전체 발언 수(포인터 힌트용).
    pub transcript_len: usize,
}

/// 한 자리의 라운드 프롬프트를 조립한다.
/// 순서(push): 역할 지시 → (이월 요약) → (검색 맥락) → (이전 라운드) → (이번 라운드 앞 자리 = 순차-인지) → 주제.
/// 순서(pull): 역할 지시 → (이월 요약) → 포인터 섹션 → (이번 라운드 앞 자리) → 주제.
/// 빈 컨텍스트면 주제만. role/instruction이 없으면 해당 섹션 생략.
/// carried 비면 이월 섹션 없음(behavior-preserving). retrieved 비면 검색 섹션 없음(push 모드).
/// pull=true이면 retrieved·prior 섹션을 생략하고 포인터로 대체. pull=false(기본)면 현행과 완전 동일.
pub fn build_round_prompt(
    participant: &Participant,
    topic: &str,
    ctx: PromptContext<'_>,
) -> String {
    let mut sections: Vec<String> = Vec::new();
    if ctx.pull {
        // pull 모드: retrieved·prior 섹션 생략. carried 이월 요약은 유지, 그 다음에 포인터 주입.
        if !ctx.carried.is_empty() {
            sections.push(format!("이전 논의 요약(이월):\n\n{}", ctx.carried));
        }
        sections.push(format!(
            "이전 토론 전사(약 {}턴)는 read_transcript(session_id, max_turns?)로, 관련 과거 맥락은 search_context(query)로 직접 읽을 수 있습니다. 답변 전 필요한 만큼 읽으세요.",
            ctx.transcript_len
        ));
    } else {
        // push 모드(기본): 현행과 완전 동일.
        // [v1.x] consensus carry-forward: carried가 비어있지 않으면 이전 논의 이월 요약 섹션 주입.
        if !ctx.carried.is_empty() {
            sections.push(format!("이전 논의 요약(이월):\n\n{}", ctx.carried));
        }
        if !ctx.retrieved.is_empty() {
            sections.push(format!(
                "참고할 만한 과거 맥락(검색):\n\n{}",
                join_utterances(ctx.retrieved)
            ));
        }
        if !ctx.prior.is_empty() {
            sections.push(format!(
                "이전 라운드 응답:\n\n{}",
                join_utterances(ctx.prior)
            ));
        }
    }
    if !ctx.same_round.is_empty() {
        sections.push(format!(
            "이번 라운드 다른 에이전트 답변:\n\n{}",
            join_utterances(ctx.same_round)
        ));
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::{Participant, Utterance};

    fn p(engine: &str, role: Option<&str>) -> Participant {
        Participant {
            engine: engine.into(),
            role: role.map(|s| s.into()),
            instruction: String::new(),
        }
    }

    fn ctx<'a>(
        prior: &'a [Utterance],
        same_round: &'a [Utterance],
        retrieved: &'a [Utterance],
        carried: &'a str,
        pull: bool,
        transcript_len: usize,
    ) -> PromptContext<'a> {
        PromptContext {
            prior,
            same_round,
            retrieved,
            carried,
            pull,
            transcript_len,
        }
    }

    #[test]
    fn prompt_includes_role_directive_and_topic() {
        let out = build_round_prompt(
            &p("claude", Some("reviewer")),
            "이 설계 어떤가요?",
            ctx(&[], &[], &[], "", false, 0),
        );
        assert!(out.contains("## Your role"));
        assert!(out.contains("verdict"));
        assert!(out.contains("이 설계 어떤가요?"));
    }

    #[test]
    fn prompt_sequential_aware_includes_same_round_responses() {
        let same = vec![Utterance {
            speaker: "claude/architect".into(),
            content: "API부터 잡자".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("codex", Some("reviewer")),
            "주제",
            ctx(&[], &same, &[], "", false, 0),
        );
        assert!(out.contains("이번 라운드 다른 에이전트 답변"));
        assert!(out.contains("API부터 잡자"));
        assert!(out.contains("claude/architect"));
    }

    #[test]
    fn prompt_includes_prior_rounds() {
        let prior = vec![Utterance {
            speaker: "codex".into(),
            content: "지난 결론".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&prior, &[], &[], "", false, 0),
        );
        assert!(out.contains("이전 라운드 응답"));
        assert!(out.contains("지난 결론"));
    }

    #[test]
    fn prompt_appends_instruction() {
        let mut part = p("claude", Some("proposer"));
        part.instruction = "API 설계에 집중".into();
        let out = build_round_prompt(&part, "주제", ctx(&[], &[], &[], "", false, 0));
        assert!(out.contains("API 설계에 집중"));
    }

    #[test]
    fn prompt_includes_retrieved_context_section() {
        let retrieved = vec![Utterance {
            speaker: "codex/reviewer".into(),
            content: "과거 분기 결론".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&[], &[], &retrieved, "", false, 0),
        );
        assert!(out.contains("참고할 만한 과거 맥락"));
        assert!(out.contains("과거 분기 결론"));
    }

    #[test]
    fn prompt_surfaces_retrieved_abstraction_and_keeps_raw() {
        // 큐레이션(v2-51): 검색 맥락에 abstraction이 있으면 렌더 시점에 "[요약] 증류문"이 원문 앞에 표면화된다.
        let retrieved = vec![Utterance {
            speaker: "past".into(),
            content: "원문 본문".into(),
            abstraction: Some("증류 요약".into()),
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&[], &[], &retrieved, "", false, 0),
        );
        assert!(
            out.contains("[요약] 증류 요약"),
            "abstraction 표면화 없음: {out}"
        );
        assert!(out.contains("원문 본문"), "원문 보존 없음: {out}");
    }

    #[test]
    fn prompt_caps_abstraction_length() {
        // CodeRabbit: abstraction도 발언 본문과 같은 MAX_ANSWER_LEN으로 캡돼야 한다(발언당 길이 상한 보장).
        let long = "가".repeat(MAX_ANSWER_LEN + 500);
        let retrieved = vec![Utterance {
            speaker: "past".into(),
            content: "원문".into(),
            abstraction: Some(long),
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&[], &[], &retrieved, "", false, 0),
        );
        assert!(out.contains("[요약]"), "요약 표면화 없음: {out}");
        // MAX_ANSWER_LEN+1 길이의 연속 '가'는 없어야 한다(캡 적용).
        assert!(
            !out.contains(&"가".repeat(MAX_ANSWER_LEN + 1)),
            "abstraction이 MAX_ANSWER_LEN으로 캡되지 않음"
        );
    }

    #[test]
    fn prompt_empty_retrieved_is_unchanged() {
        // retrieved=&[] -> 검색 섹션 없음(기존과 동일).
        let out = build_round_prompt(&p("claude", None), "주제", ctx(&[], &[], &[], "", false, 0));
        assert!(!out.contains("참고할 만한 과거 맥락"));
    }

    #[test]
    fn prompt_carried_empty_means_no_carry_section() {
        // carried="" -> "이전 논의 요약" 섹션 없음(기존 동작 불변).
        let out = build_round_prompt(&p("claude", None), "주제", ctx(&[], &[], &[], "", false, 0));
        assert!(!out.contains("이전 논의 요약"));
    }

    #[test]
    fn prompt_carried_section_present_and_before_retrieved_and_prior() {
        // carried 있으면 섹션 존재, 이월 요약이 검색/prior보다 앞에 위치해야 한다.
        let retrieved = vec![Utterance {
            speaker: "p".into(),
            content: "검색결과".into(),
            abstraction: None,
        }];
        let prior = vec![Utterance {
            speaker: "p".into(),
            content: "이전발언".into(),
            abstraction: None,
        }];
        let carried = "초기 합의 요약";
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&prior, &[], &retrieved, carried, false, 0),
        );
        assert!(out.contains("이전 논의 요약(이월)"), "이월 섹션 없음");
        assert!(out.contains(carried), "carried 내용 없음");
        let pos_carry = out.find("이전 논의 요약(이월)").unwrap();
        let pos_retrieved = out.find("참고할 만한 과거 맥락").unwrap();
        let pos_prior = out.find("이전 라운드 응답").unwrap();
        assert!(pos_carry < pos_retrieved, "이월 요약이 검색보다 뒤에 있음");
        assert!(pos_carry < pos_prior, "이월 요약이 prior보다 뒤에 있음");
    }

    // --- pull 모드 테스트 ---

    #[test]
    fn pull_prompt_includes_pointer_no_prior_no_retrieved() {
        // pull=true이면 포인터 섹션 포함, "이전 라운드 응답"·"참고할 만한 과거 맥락" 없음.
        let prior = vec![Utterance {
            speaker: "codex".into(),
            content: "지난 결론".into(),
            abstraction: None,
        }];
        let retrieved = vec![Utterance {
            speaker: "p".into(),
            content: "검색결과".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&prior, &[], &retrieved, "", true, 10),
        );
        assert!(out.contains("read_transcript"), "포인터 없음");
        assert!(out.contains("search_context"), "search_context 포인터 없음");
        assert!(out.contains("10턴"), "전사 길이 힌트 없음");
        assert!(!out.contains("이전 라운드 응답"), "prior 섹션이 남아 있음");
        assert!(
            !out.contains("참고할 만한 과거 맥락"),
            "retrieved 섹션이 남아 있음"
        );
    }

    #[test]
    fn pull_prompt_preserves_carried_and_same_round_and_topic() {
        // pull=true여도 carried 이월 요약, same_round, topic은 유지.
        let same = vec![Utterance {
            speaker: "codex".into(),
            content: "앞 발언".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "이 설계 어떤가",
            ctx(&[], &same, &[], "합의 요약", true, 5),
        );
        assert!(out.contains("이전 논의 요약(이월)"), "이월 섹션 없음");
        assert!(out.contains("합의 요약"), "carried 내용 없음");
        assert!(
            out.contains("이번 라운드 다른 에이전트 답변"),
            "same_round 없음"
        );
        assert!(out.contains("앞 발언"), "same_round 내용 없음");
        assert!(out.contains("이 설계 어떤가"), "topic 없음");
    }

    #[test]
    fn push_false_is_fully_behavior_preserving() {
        // pull=false(기본)이면 기존과 완전 동일. 포인터 없음.
        let prior = vec![Utterance {
            speaker: "codex".into(),
            content: "결론".into(),
            abstraction: None,
        }];
        let out = build_round_prompt(
            &p("claude", None),
            "주제",
            ctx(&prior, &[], &[], "", false, 99),
        );
        assert!(out.contains("이전 라운드 응답"), "prior 섹션 없음");
        assert!(
            !out.contains("read_transcript"),
            "포인터가 push 모드에 나타남"
        );
    }
}
