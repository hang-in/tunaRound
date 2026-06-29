// 한 자리의 라운드 프롬프트를 조립하는 순수 함수(tunapi prompt.py 답습, 순차-인지).

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
