// A2A poll_tasks 응답의 구조화(JSON) wire 포맷(v2-52 ④). 브로커(mcp)와 워커(worker)가 공유하는 계약이라
// sqlite/mcp/worker 어느 피처에도 매이지 않는 crate 루트 모듈에 둔다(serde만 의존, 경량 워커 빌드도 접근).

use serde::{Deserialize, Serialize};

/// poll_tasks 응답 JSON 프리픽스 라인의 sentinel. 이 접두로 시작하는 첫 줄이 있으면 그 뒤가 task 목록 JSON.
pub const POLL_JSON_PREFIX: &str = "TASKS_JSON ";

/// poll_tasks JSON 항목: 워커(worker::ParsedTask)가 필요로 하는 필드와 1:1. state는 표시 주석
/// (stuck?/no-consumer?)이 없는 clean 상태다(문자열 경로의 state_token 스트립 불요).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PollTaskDto {
    pub id: String,
    pub state: String,
    pub context_id: Option<String>,
    pub msg: String,
}

/// task 목록을 `TASKS_JSON <compact-json>` 한 줄로 인코딩한다(브로커 생산). compact(실개행 없음)라야
/// 구 워커의 find_header_starts가 이 줄 안에서 거짓 헤더를 만들지 않는다(msg 내 개행도 `\n`으로 이스케이프됨).
/// 직렬화 실패 시 None(String/Option만이라 사실상 무오류지만, 실패 시 호출자가 프리픽스를 생략해 워커가
/// 문자열로 폴백하도록 = 오도하는 빈 JSON으로 task를 은닉하지 않는다, 적대 리뷰 방어).
pub fn encode_poll_json(tasks: &[PollTaskDto]) -> Option<String> {
    // serde_json::to_string(pretty 아님)은 실개행 없이 직렬화하므로 단일 라인이 보장된다.
    let json = serde_json::to_string(tasks).ok()?;
    Some(format!("{POLL_JSON_PREFIX}{json}"))
}

/// 응답 텍스트의 첫 줄이 POLL_JSON_PREFIX면 그 뒤 JSON을 디코드한다(워커 소비). 프리픽스가 없거나
/// 파싱 실패면 None(호출자가 문자열 파싱으로 폴백). 프리픽스는 항상 맨 앞이라 첫 줄만 검사한다.
pub fn decode_poll_json(text: &str) -> Option<Vec<PollTaskDto>> {
    let first_line = text.lines().next()?;
    let json = first_line.strip_prefix(POLL_JSON_PREFIX)?;
    serde_json::from_str(json).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn poll_json_roundtrip_preserves_fields_and_is_single_line() {
        let tasks = vec![
            PollTaskDto {
                id: "a".repeat(32),
                state: "submitted".into(),
                context_id: Some("projA".into()),
                msg: "리뷰\n\n부탁 [본문]".into(), // 개행·한글·브래킷.
            },
            PollTaskDto {
                id: "b".repeat(32),
                state: "working".into(),
                context_id: None,
                msg: "진행".into(),
            },
        ];
        let encoded = encode_poll_json(&tasks).unwrap();
        assert!(encoded.starts_with(POLL_JSON_PREFIX));
        // compact = 단일 라인(구 워커 find_header_starts 안전): 프리픽스 뒤에 실개행이 없어야 한다.
        assert!(
            !encoded[POLL_JSON_PREFIX.len()..].contains('\n'),
            "compact JSON은 실개행 없어야 함: {encoded}"
        );
        // 라운드트립 = 무손실(msg 내 개행·한글·브래킷 포함).
        assert_eq!(decode_poll_json(&encoded).unwrap(), tasks);
    }

    #[test]
    fn decode_poll_json_none_without_prefix() {
        // 문자열 프로토콜(구 broker)·빈 목록 안내는 프리픽스가 없어 None → 호출자가 문자열 폴백.
        let id = "a".repeat(32);
        assert!(decode_poll_json(&format!("[{id}] from=x state=submitted msg=y")).is_none());
        assert!(decode_poll_json("mac 앞 열린 task 없음").is_none());
        // 프리픽스지만 JSON 파싱 실패도 None(안전 폴백).
        assert!(decode_poll_json("TASKS_JSON not-json").is_none());
    }
}
