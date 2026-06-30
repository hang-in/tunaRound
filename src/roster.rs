// JSON 로스터 파일을 토론 좌석(Participant) + 러너 레지스트리로 만드는 로더.

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
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("로스터 읽기 실패 ({path}): {e}"))?;
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
/// search_db가 Some이면 각 러너에 검색 MCP 서버를 배선한다.
pub fn build_registry(roster: &Roster, search_db: Option<&str>) -> Result<MapRegistry, String> {
    let mut reg = MapRegistry::new();
    let mut seen: Vec<String> = Vec::new();
    for seat in &roster.seats {
        if seen.contains(&seat.engine) {
            continue;
        }
        match seat.engine.as_str() {
            "claude" => reg.insert(
                "claude",
                Box::new(ClaudeRunner::new().with_search_db(search_db.map(String::from))),
            ),
            "codex" => reg.insert(
                "codex",
                Box::new(CodexRunner::new().with_search_db(search_db.map(String::from))),
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
                                Box::new(
                                    crate::runner::http::OpenAiChatRunner::new(base, mdl, api_key),
                                ),
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
                        return Err(format!(
                            "HTTP 엔진엔 engines feature가 필요합니다: {other}"
                        ));
                    } else {
                        return Err(format!("알 수 없는 엔진: {other} (지원: claude, codex)"));
                    }
                }
            }
        }
        seen.push(seat.engine.clone());
    }
    Ok(reg)
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
        assert_eq!(roster.seats[1].role, None);         // 기본 None
        assert_eq!(roster.seats[1].instruction, "");    // 기본 빈 문자열
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
        let roster =
            parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(build_registry(&roster, None).is_ok());
    }

    #[test]
    fn build_registry_unknown_engine_errors() {
        let roster = parse_roster(r#"{"seats":[{"engine":"gemini"}]}"#).unwrap();
        let err = build_registry(&roster, None).err().unwrap();
        assert!(err.contains("gemini"));
    }

    #[test]
    fn build_registry_with_search_db_ok() {
        let roster =
            parse_roster(r#"{"seats":[{"engine":"claude"},{"engine":"codex"}]}"#).unwrap();
        assert!(build_registry(&roster, Some("x.db")).is_ok());
    }

    #[test]
    fn empty_seats_is_error() {
        let roster = parse_roster(r#"{"seats":[]}"#).unwrap();
        assert!(build_participants_checked(&roster).is_err());
    }

    #[cfg(feature = "engines")]
    #[test]
    fn build_registry_http_seat_ok() {
        let roster = parse_roster(
            r#"{"seats":[{"engine":"local","base_url":"http://127.0.0.1:11435","model":"gemma4:e2b"}]}"#,
        )
        .unwrap();
        assert!(build_registry(&roster, None).is_ok());
    }

    #[cfg(feature = "engines")]
    #[test]
    fn build_registry_http_seat_missing_model_err() {
        let roster =
            parse_roster(r#"{"seats":[{"engine":"local","base_url":"http://x"}]}"#).unwrap();
        let err = build_registry(&roster, None).err().unwrap();
        assert!(err.contains("base_url"), "에러에 base_url 없음: {err}");
    }
}
