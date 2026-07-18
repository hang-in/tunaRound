// presence 보고 조립: LiveSession 목록을 report_presence의 sessions JSON 배열로 직렬화한다.

use super::*;

/// report_presence의 sessions JSON 배열로 직렬화한다. display_name = {machine}-{runner}-{project|?}.
pub fn to_report_json(machine: &str, sessions: &[LiveSession]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            let display = format!(
                "{machine}-{}-{}",
                s.runner,
                s.project.as_deref().unwrap_or("unknown")
            );
            serde_json::json!({
                "uuid": s.uuid,
                "runner": s.runner,
                "project": s.project,
                "display_name": display,
                "human_input_at": s.human_input_at,
                "active_at": s.active_at,
            })
        })
        .collect();
    serde_json::Value::Array(arr)
}
