// 토론 자리의 역할별 행동 지시문. 같은 엔진이 다른 역할을 연기하게 한다(tunapi roles.py 답습).

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
