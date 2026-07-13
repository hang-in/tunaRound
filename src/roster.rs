// JSON 로스터 파일을 토론 좌석(Participant) + 러너 레지스트리로 만드는 로더.

use serde::Deserialize;

use crate::orchestrator::{MapRegistry, Participant};
use crate::runner::claude::ClaudeRunner;
use crate::runner::codex::CodexRunner;
use crate::runner::opencode::OpencodeRunner;

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
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub api_key_env: Option<String>,
}

/// JSON 문자열을 Roster로 파싱한다.
pub fn parse_roster(json: &str) -> Result<Roster, String> {
    serde_json::from_str(json).map_err(|e| format!("로스터 파싱 실패: {e}"))
}

/// 파일에서 로스터를 읽어 파싱한다.
pub fn load_roster(path: &str) -> Result<Roster, String> {
    let text =
        std::fs::read_to_string(path).map_err(|e| format!("로스터 읽기 실패 ({path}): {e}"))?;
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
/// search_db가 Some이면 각 러너에 stdio MCP 서버를 배선한다.
/// search_url이 Some이면 HTTP MCP 서버 URL을 우선 배선한다(search_db보다 우선).
pub fn build_registry(
    roster: &Roster,
    search_db: Option<&str>,
    search_url: Option<&str>,
    search_token: Option<&str>,
) -> Result<MapRegistry, String> {
    let mut reg = MapRegistry::new();
    // engine명 → 첫 좌석 참조. 같은 engine명 두 번째 좌석부터는 러너를 새로 만들지 않고 첫 좌석
    // 러너를 공유하는데(#10), 그 좌석 설정(base_url·model·api_key_env)이 첫 좌석과 다르면 조용히
    // 무시되던 것을 경고로 표면화한다. 완전히 동일한 중복은 조용히 스킵 유지.
    let mut seen: std::collections::HashMap<String, &SeatConfig> = std::collections::HashMap::new();
    for seat in &roster.seats {
        if let Some(&first) = seen.get(&seat.engine) {
            if seat_configs_conflict(first, seat) {
                eprintln!(
                    "[roster] 경고: engine '{}' 중복 좌석의 설정이 다릅니다 - 첫 좌석 base_url={:?} model={:?}가 쓰이고 이 좌석 base_url={:?} model={:?}는 무시됩니다",
                    seat.engine, first.base_url, first.model, seat.base_url, seat.model
                );
            }
            continue;
        }
        seen.insert(seat.engine.clone(), seat);
        match seat.engine.as_str() {
            "claude" => reg.insert(
                "claude",
                Box::new(
                    ClaudeRunner::new()
                        .with_search_db(search_db.map(String::from))
                        .with_search_url(
                            search_url.map(String::from),
                            search_token.map(String::from),
                        ),
                ),
            ),
            "codex" => reg.insert(
                "codex",
                Box::new(
                    CodexRunner::new()
                        .with_search_db(search_db.map(String::from))
                        .with_search_url(
                            search_url.map(String::from),
                            search_token.map(String::from),
                        ),
                ),
            ),
            "opencode" => reg.insert(
                "opencode",
                Box::new(OpencodeRunner::new().with_model(seat.model.clone())),
            ),
            other => {
                // HTTP 엔진 분기: base_url+model 좌석이면 OpenAiChatRunner, 없으면 에러.
                #[cfg(feature = "engines")]
                {
                    match (seat.base_url.as_deref(), seat.model.as_deref()) {
                        (Some(base), Some(mdl)) => {
                            let api_key = seat
                                .api_key_env
                                .as_ref()
                                .and_then(|e| std::env::var(e).ok());
                            reg.insert(
                                other,
                                Box::new(crate::runner::http::OpenAiChatRunner::new(
                                    base, mdl, api_key,
                                )),
                            );
                        }
                        _ => {
                            return Err(format!(
                                "HTTP 엔진 '{other}'엔 base_url과 model이 필요합니다"
                            ));
                        }
                    }
                }
                #[cfg(not(feature = "engines"))]
                {
                    if seat.base_url.is_some() {
                        return Err(format!("HTTP 엔진엔 engines feature가 필요합니다: {other}"));
                    } else {
                        return Err(format!(
                            "알 수 없는 엔진: {other} (지원: claude, codex, opencode)"
                        ));
                    }
                }
            }
        }
    }
    Ok(reg)
}

/// 두 좌석의 러너 생성 관련 설정(base_url·model·api_key_env)이 다른지 판정한다(순수부, #10). 같은
/// engine명 중복 좌석에서 build_registry가 이 판정으로 경고 여부를 정한다.
fn seat_configs_conflict(a: &SeatConfig, b: &SeatConfig) -> bool {
    a.base_url != b.base_url || a.model != b.model || a.api_key_env != b.api_key_env
}

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
        assert_eq!(roster.seats[1].role, None); // 기본 None
        assert_eq!(roster.seats[1].instruction, ""); // 기본 빈 문자열
    }

    #[test]
    fn build_participants_maps_fields() {
        let roster = parse_roster(
            r#"{"seats":[{"engine":"claude","role":"proposer","instruction":"간결히"}]}"#,
        )
        .unwrap();
        let parts = build_participants(&roster);
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].engine, "claude");
        assert_eq!(parts[0].role.as_deref(), Some("proposer"));
        assert_eq!(parts[0].instruction, "간결히");
    }

    #[test]
    fn build_registry_known_engines_ok() {
        let roster = parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(build_registry(&roster, None, None, None).is_ok());
    }

    #[test]
    fn build_registry_unknown_engine_errors() {
        let roster = parse_roster(r#"{"seats":[{"engine":"gemini"}]}"#).unwrap();
        let err = build_registry(&roster, None, None, None).err().unwrap();
        assert!(err.contains("gemini"));
    }

    #[test]
    fn build_registry_with_search_db_ok() {
        let roster = parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(build_registry(&roster, Some("x.db"), None, None).is_ok());
    }

    #[test]
    fn build_registry_with_search_url_ok() {
        // search_url 설정 시 레지스트리 구성이 정상적으로 완료된다.
        let roster = parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(
            build_registry(
                &roster,
                None,
                Some("http://127.0.0.1:8080/mcp"),
                Some("tok")
            )
            .is_ok()
        );
    }

    #[test]
    fn empty_seats_is_error() {
        let roster = parse_roster(r#"{"seats":[]}"#).unwrap();
        assert!(build_participants_checked(&roster).is_err());
    }

    #[test]
    fn seat_configs_conflict_detects_model_mismatch_and_ignores_identical_dupes() {
        // #10: 같은 engine명 두 좌석의 model이 다르면 경고 대상(true), 완전히 동일하면 조용히 스킵(false).
        let base = SeatConfig {
            engine: "opencode".to_string(),
            role: None,
            instruction: String::new(),
            base_url: None,
            model: Some("gemma3".to_string()),
            api_key_env: None,
        };
        let mut different_model = base.clone();
        different_model.model = Some("gemma4".to_string());
        assert!(seat_configs_conflict(&base, &different_model));

        let identical = base.clone();
        assert!(!seat_configs_conflict(&base, &identical));

        let mut different_base_url = base.clone();
        different_base_url.base_url = Some("http://127.0.0.1:9".to_string());
        assert!(seat_configs_conflict(&base, &different_base_url));
    }

    #[test]
    fn build_registry_duplicate_engine_with_different_model_still_ok_first_wins() {
        // 경고(eprintln)를 내지만 build_registry 자체는 여전히 성공하고, 두 번째 좌석 설정은
        // 무시된 채 첫 좌석 러너가 유지된다(완전 동일 중복과 같은 스킵 경로, 경고만 추가됨).
        let roster = parse_roster(
            r#"{"seats":[
                {"engine":"opencode","model":"gemma3"},
                {"engine":"opencode","model":"gemma4"}
            ]}"#,
        )
        .unwrap();
        assert!(build_registry(&roster, None, None, None).is_ok());
    }

    #[cfg(feature = "engines")]
    #[test]
    fn build_registry_http_seat_ok() {
        let roster = parse_roster(
            r#"{"seats":[{"engine":"local","base_url":"http://127.0.0.1:11435","model":"gemma4:e2b"}]}"#,
        )
        .unwrap();
        assert!(build_registry(&roster, None, None, None).is_ok());
    }

    #[cfg(feature = "engines")]
    #[test]
    fn build_registry_http_seat_missing_model_err() {
        let roster =
            parse_roster(r#"{"seats":[{"engine":"local","base_url":"http://x"}]}"#).unwrap();
        let err = build_registry(&roster, None, None, None).err().unwrap();
        assert!(err.contains("base_url"), "에러에 base_url 없음: {err}");
    }
}
