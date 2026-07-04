// 에이전트 레지스트리의 데이터 모델과 인메모리 로스터 순수 함수.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// online으로 간주하는 heartbeat 최대 경과 초. 워커가 이보다 오래 heartbeat 없으면 offline.
pub const AGENT_TTL_SECS: i64 = 90;

/// 로스터에 등록된 에이전트 한 항목. tags는 발견용 KV(machine/runner/role/project/mode는 관례일 뿐 강제 없음),
/// last_heartbeat는 SQL datetime('now') 포맷 문자열(online 판정 기준).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AgentEntry {
    pub uuid: String,
    pub tags: BTreeMap<String, String>,
    pub display_name: Option<String>,
    pub last_heartbeat: String,
}

/// "k=v,k=v" 형식 태그 문자열을 파싱한다. `,`로 split 후 각 세그먼트를 trim, 빈 세그먼트는 건너뛴다
/// (후행 콤마 허용). 각 세그먼트는 첫 `=`로 key/value를 나눈다(value에 `=` 포함 가능). key가 비면
/// 에러, `=`가 없으면 에러. value는 trim만 하고 빈 값은 허용한다. 중복 키는 나중 값이 이긴다.
pub fn parse_tags(s: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for raw_seg in s.split(',') {
        let seg = raw_seg.trim();
        if seg.is_empty() {
            continue;
        }
        match seg.find('=') {
            None => return Err(format!("태그 형식 오류(k=v 필요): {seg}")),
            Some(idx) => {
                let key = seg[..idx].trim();
                let value = seg[idx + 1..].trim();
                if key.is_empty() {
                    return Err(format!("빈 태그 키: {seg}"));
                }
                out.insert(key.to_string(), value.to_string());
            }
        }
    }
    Ok(out)
}

/// selector의 모든 (k,v)가 tags에 같은 값으로 존재하면 true(부분집합). 빈 selector는 항상 true.
pub fn selector_matches(tags: &BTreeMap<String, String>, selector: &BTreeMap<String, String>) -> bool {
    selector.iter().all(|(k, v)| tags.get(k) == Some(v))
}

/// last_heartbeat가 now 기준 ttl_secs 이내면 online으로 판정한다. age_secs 파싱 실패(포맷 불량)는
/// 검증 불가로 간주해 보수적으로 offline 처리한다.
pub fn is_online(last_heartbeat: &str, now: &str, ttl_secs: i64) -> bool {
    match crate::store::a2a::age_secs(now, last_heartbeat) {
        Some(age) => age <= ttl_secs,
        None => false,
    }
}

/// send_task/SendMessage 대상 지정. Agent(구체 uuid) 또는 Selector(태그, 발송 시점 해석).
/// MCP send_task와 `/a2a` SendMessage 양쪽이 공유하는 라우팅 헬퍼(Plan v2-34 T2/T3, DRY).
pub enum SendTarget {
    Agent(String),
    Selector(String),
}

/// 위임 대상 지정 검증. to_agent와 to_selector는 정확히 하나만 있어야 한다(공백 문자열은
/// 없는 것으로 취급 - trim 후 is_empty면 None과 동일하게 본다).
pub fn validate_send_target(
    to_agent: Option<&str>,
    to_selector: Option<&str>,
) -> Result<SendTarget, String> {
    let agent = to_agent.map(str::trim).filter(|s| !s.is_empty());
    let selector = to_selector.map(str::trim).filter(|s| !s.is_empty());
    match (agent, selector) {
        (Some(_), Some(_)) => Err("to_agent와 to_selector 중 하나만 지정하세요".to_string()),
        (None, None) => Err("to_agent 또는 to_selector가 필요합니다".to_string()),
        (Some(a), None) => Ok(SendTarget::Agent(a.to_string())),
        (None, Some(s)) => Ok(SendTarget::Selector(s.to_string())),
    }
}

/// selector가 여러 online 에이전트에 매칭될 때 발신자에게 돌려줄 후보 목록 텍스트(task 생성은 안 함).
pub fn format_ambiguous_candidates(selector: &str, uuids: &[String]) -> String {
    let list = uuids.iter().map(|u| format!("- {u}")).collect::<Vec<_>>().join("\n");
    format!("셀렉터 '{selector}'가 여러 에이전트에 매칭됩니다. to_agent로 하나를 골라 재요청하세요:\n{list}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tags_multiple_pairs() {
        let tags = parse_tags("machine=mac,runner=claude").unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("machine"), Some(&"mac".to_string()));
        assert_eq!(tags.get("runner"), Some(&"claude".to_string()));
    }

    #[test]
    fn parse_tags_empty_input_is_empty_map() {
        assert!(parse_tags("").unwrap().is_empty());
        assert!(parse_tags("   ").unwrap().is_empty());
    }

    #[test]
    fn parse_tags_allows_blank_segments_and_trailing_comma() {
        let tags = parse_tags("a=1, ,b=2,").unwrap();
        assert_eq!(tags.len(), 2);
        assert_eq!(tags.get("a"), Some(&"1".to_string()));
        assert_eq!(tags.get("b"), Some(&"2".to_string()));
    }

    #[test]
    fn parse_tags_missing_equals_is_err() {
        assert!(parse_tags("noequals").is_err());
    }

    #[test]
    fn parse_tags_empty_key_is_err() {
        assert!(parse_tags("=v").is_err());
    }

    #[test]
    fn parse_tags_duplicate_key_last_wins() {
        let tags = parse_tags("k=first,k=second").unwrap();
        assert_eq!(tags.get("k"), Some(&"second".to_string()));
    }

    #[test]
    fn parse_tags_value_may_contain_equals() {
        let tags = parse_tags("k=a=b").unwrap();
        assert_eq!(tags.get("k"), Some(&"a=b".to_string()));
    }

    #[test]
    fn selector_matches_subset_is_true() {
        let mut tags = BTreeMap::new();
        tags.insert("machine".to_string(), "mac".to_string());
        tags.insert("runner".to_string(), "claude".to_string());
        let mut selector = BTreeMap::new();
        selector.insert("machine".to_string(), "mac".to_string());
        assert!(selector_matches(&tags, &selector));
    }

    #[test]
    fn selector_matches_value_mismatch_is_false() {
        let mut tags = BTreeMap::new();
        tags.insert("machine".to_string(), "mac".to_string());
        let mut selector = BTreeMap::new();
        selector.insert("machine".to_string(), "win".to_string());
        assert!(!selector_matches(&tags, &selector));
    }

    #[test]
    fn selector_matches_missing_key_is_false() {
        let tags: BTreeMap<String, String> = BTreeMap::new();
        let mut selector = BTreeMap::new();
        selector.insert("machine".to_string(), "mac".to_string());
        assert!(!selector_matches(&tags, &selector));
    }

    #[test]
    fn selector_matches_empty_selector_is_always_true() {
        let tags: BTreeMap<String, String> = BTreeMap::new();
        let selector: BTreeMap<String, String> = BTreeMap::new();
        assert!(selector_matches(&tags, &selector));
    }

    #[test]
    fn is_online_within_ttl_is_true() {
        assert!(is_online("2026-07-04 09:59:00", "2026-07-04 10:00:00", 90));
    }

    #[test]
    fn is_online_beyond_ttl_is_false() {
        assert!(!is_online("2026-07-04 09:58:00", "2026-07-04 10:00:00", 90));
    }

    #[test]
    fn is_online_at_ttl_boundary_is_true() {
        // 경과가 정확히 ttl_secs일 때 <=이므로 online.
        assert!(is_online("2026-07-04 09:58:30", "2026-07-04 10:00:00", 90));
    }

    #[test]
    fn is_online_parse_failure_is_false() {
        assert!(!is_online("bogus", "2026-07-04 10:00:00", 90));
    }

    // --- 레지스트리 라우팅: 순수 함수 단위테스트 (Plan v2-34 T2, mcp.rs에서 T3에서 이동) ---

    #[test]
    fn validate_send_target_rejects_both_and_neither() {
        assert!(validate_send_target(Some("a"), Some("k=v")).is_err());
        assert!(validate_send_target(None, None).is_err());
        // 공백 문자열은 없는 것으로 취급하므로 둘 다 공백이면 "둘 다 없음" 에러.
        assert!(validate_send_target(Some("  "), None).is_err());
    }

    #[test]
    fn validate_send_target_agent_only() {
        match validate_send_target(Some("mac-claude"), None).unwrap() {
            SendTarget::Agent(a) => assert_eq!(a, "mac-claude"),
            SendTarget::Selector(_) => panic!("Agent여야 함"),
        }
    }

    #[test]
    fn validate_send_target_selector_only() {
        match validate_send_target(None, Some("runner=claude")).unwrap() {
            SendTarget::Selector(s) => assert_eq!(s, "runner=claude"),
            SendTarget::Agent(_) => panic!("Selector여야 함"),
        }
    }

    #[test]
    fn format_ambiguous_candidates_lists_uuids() {
        let text = format_ambiguous_candidates(
            "runner=claude",
            &["uuid-1".to_string(), "uuid-2".to_string()],
        );
        assert!(text.contains("runner=claude"));
        assert!(text.contains("uuid-1"));
        assert!(text.contains("uuid-2"));
    }
}
