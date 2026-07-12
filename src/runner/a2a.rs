// 외부 표준 A2A 에이전트에게 task를 표준 위임하는 outbound 러너(a2a-client 사용).

use super::{RunError, RunInput, RunOutput, Runner};
use a2a_client::A2AClient;
use a2a_types::{
    GetTaskRequest, Message, Part, Role, SendMessageRequest, Task, TaskState, part::Content,
    send_message_response::Payload,
};
use std::time::Duration;

/// 폴링 완료 대기 기본 타임아웃(초).
const DEFAULT_TIMEOUT_SECS: u64 = 300;
/// task 상태 폴링 간격.
const POLL_INTERVAL: Duration = Duration::from_millis(1500);

/// 외부 표준 A2A 에이전트 러너. `card_url`로 에이전트를 발견(agent-card.json)하고
/// SendMessage로 표준 위임한 뒤, 완료 task를 폴링해 artifact/메시지 텍스트를 반환한다.
pub struct A2ARunner {
    card_url: String,
    auth_token: Option<String>,
    timeout_secs: u64,
}

impl A2ARunner {
    /// `card_url`: 외부 에이전트 카드 발견 base URL. `auth_token`: 그 에이전트 인증(코어 --token과 별개).
    pub fn new(card_url: String, auth_token: Option<String>) -> Self {
        Self {
            card_url,
            auth_token,
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// task 완료 폴링 타임아웃(초)을 바꾼다(테스트/설정용).
    pub fn with_timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = secs;
        self
    }

    async fn run_async(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let mut client = A2AClient::from_card_url(&self.card_url)
            .await
            .map_err(|e| RunError::Io(format!("A2A 카드 발견 실패({}): {e}", self.card_url)))?;
        if let Some(token) = &self.auth_token {
            client = client.with_auth_token(token.clone());
        }

        let request = SendMessageRequest {
            tenant: String::new(),
            message: Some(build_user_message(&input.prompt)),
            configuration: None,
            metadata: None,
        };

        let response = client
            .send_message(request)
            .await
            .map_err(|e| RunError::Agent(format!("A2A send_message 실패: {e}")))?;

        let content = match response.payload {
            Some(Payload::Message(msg)) => extract_message_content(&msg)
                .ok_or_else(|| RunError::Empty("A2A 응답 메시지에 텍스트 없음".to_string()))?,
            Some(Payload::Task(task)) => self.await_task_completion(&client, task).await?,
            None => return Err(RunError::Empty("A2A 응답에 payload 없음".to_string())),
        };

        if content.trim().is_empty() {
            return Err(RunError::Empty("A2A 응답 내용이 비었음".to_string()));
        }
        Ok(RunOutput {
            content,
            input_tokens: 0,
            output_tokens: 0,
        })
    }

    /// task가 완료 상태(Completed)가 될 때까지 GetTask로 폴링한다. 실패/취소/거부는 즉시 에러,
    /// 타임아웃이면 RunError::Timeout.
    async fn await_task_completion(
        &self,
        client: &A2AClient,
        mut task: Task,
    ) -> Result<String, RunError> {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(self.timeout_secs);
        loop {
            let state = task
                .status
                .as_ref()
                .map(|s| s.state())
                .unwrap_or(TaskState::Unspecified);
            match state {
                TaskState::Completed => {
                    return extract_task_content(&task).ok_or_else(|| {
                        RunError::Empty("A2A task 완료했지만 내용 없음".to_string())
                    });
                }
                TaskState::Failed | TaskState::Rejected | TaskState::Canceled => {
                    return Err(RunError::Agent(format!(
                        "A2A task {} 종료 상태={state:?}",
                        task.id
                    )));
                }
                _ => {
                    if tokio::time::Instant::now() >= deadline {
                        return Err(RunError::Timeout(format!(
                            "A2A task {} 완료 대기 타임아웃({}s)",
                            task.id, self.timeout_secs
                        )));
                    }
                    tokio::time::sleep(POLL_INTERVAL).await;
                    let req = GetTaskRequest {
                        tenant: String::new(),
                        id: task.id.clone(),
                        history_length: None,
                    };
                    task = client
                        .get_task(req)
                        .await
                        .map_err(|e| RunError::Io(format!("A2A get_task 실패: {e}")))?;
                }
            }
        }
    }
}

impl Runner for A2ARunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| RunError::Spawn(format!("A2A용 tokio 런타임 생성 실패: {e}")))?;
        rt.block_on(self.run_async(input))
    }
}

/// prompt를 A2A user 메시지(텍스트 part 1개)로 감싼다(순수 함수, 네트워크 없음).
fn build_user_message(prompt: &str) -> Message {
    Message {
        message_id: format!("tunaround-{}", now_nanos()),
        context_id: String::new(),
        task_id: String::new(),
        role: Role::User as i32,
        parts: vec![Part {
            metadata: None,
            filename: String::new(),
            media_type: "text/plain".to_string(),
            content: Some(Content::Text(prompt.to_string())),
        }],
        metadata: None,
        extensions: Vec::new(),
        reference_task_ids: Vec::new(),
    }
}

fn now_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}

/// A2A Task에서 표시 텍스트를 뽑는다(순수 함수). artifacts를 우선하고, 비어있으면
/// history의 마지막 agent 메시지로 폴백한다.
pub fn extract_task_content(task: &Task) -> Option<String> {
    let from_artifacts = task
        .artifacts
        .iter()
        .flat_map(|a| a.parts.iter())
        .filter_map(part_text)
        .collect::<Vec<_>>()
        .join("\n");
    if !from_artifacts.trim().is_empty() {
        return Some(from_artifacts);
    }
    task.history
        .iter()
        .rev()
        .find(|m| m.role == Role::Agent as i32)
        .and_then(message_text)
}

/// A2A Message에서 표시 텍스트를 뽑는다(순수 함수).
pub fn extract_message_content(message: &Message) -> Option<String> {
    message_text(message)
}

fn message_text(message: &Message) -> Option<String> {
    let joined = message
        .parts
        .iter()
        .filter_map(part_text)
        .collect::<Vec<_>>()
        .join("\n");
    if joined.trim().is_empty() {
        None
    } else {
        Some(joined)
    }
}

fn part_text(part: &Part) -> Option<String> {
    match &part.content {
        Some(Content::Text(t)) if !t.is_empty() => Some(t.clone()),
        Some(Content::Data(v)) => serde_json::to_string(v).ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_part(text: &str) -> Part {
        Part {
            metadata: None,
            filename: String::new(),
            media_type: "text/plain".to_string(),
            content: Some(Content::Text(text.to_string())),
        }
    }

    fn agent_message(text: &str) -> Message {
        Message {
            message_id: "m1".to_string(),
            context_id: String::new(),
            task_id: String::new(),
            role: Role::Agent as i32,
            parts: vec![text_part(text)],
            metadata: None,
            extensions: Vec::new(),
            reference_task_ids: Vec::new(),
        }
    }

    fn completed_task_with_artifact(text: &str) -> Task {
        Task {
            id: "t1".to_string(),
            context_id: "c1".to_string(),
            status: Some(a2a_types::TaskStatus {
                state: TaskState::Completed as i32,
                message: None,
                timestamp: None,
            }),
            artifacts: vec![a2a_types::Artifact {
                artifact_id: "art1".to_string(),
                name: "result".to_string(),
                description: String::new(),
                parts: vec![text_part(text)],
                metadata: None,
                extensions: Vec::new(),
            }],
            history: Vec::new(),
            metadata: None,
        }
    }

    #[test]
    fn build_user_message_wraps_prompt_as_user_text_part() {
        let msg = build_user_message("설계 검토해줘");
        assert_eq!(msg.role, Role::User as i32);
        assert_eq!(msg.parts.len(), 1);
        assert_eq!(part_text(&msg.parts[0]).as_deref(), Some("설계 검토해줘"));
    }

    #[test]
    fn extract_task_content_prefers_artifact_text() {
        let task = completed_task_with_artifact("아티팩트 결과");
        assert_eq!(
            extract_task_content(&task).as_deref(),
            Some("아티팩트 결과")
        );
    }

    #[test]
    fn extract_task_content_falls_back_to_last_agent_history_message() {
        let mut task = completed_task_with_artifact("");
        task.artifacts.clear();
        task.history = vec![agent_message("agent 응답 1"), agent_message("agent 응답 2")];
        assert_eq!(extract_task_content(&task).as_deref(), Some("agent 응답 2"));
    }

    #[test]
    fn extract_task_content_none_when_no_artifact_and_no_agent_history() {
        let mut task = completed_task_with_artifact("");
        task.artifacts.clear();
        assert_eq!(extract_task_content(&task), None);
    }

    #[test]
    fn extract_message_content_reads_text_parts() {
        let msg = agent_message("응답 텍스트");
        assert_eq!(
            extract_message_content(&msg).as_deref(),
            Some("응답 텍스트")
        );
    }

    #[test]
    fn part_text_ignores_non_text_non_data_parts() {
        let part = Part {
            metadata: None,
            filename: "img.png".to_string(),
            media_type: "image/png".to_string(),
            content: Some(Content::Url("http://example.com/img.png".to_string())),
        };
        assert_eq!(part_text(&part), None);
    }

    #[test]
    fn part_text_stringifies_data_variant() {
        // pbjson_types::Value(protobuf well-known Value)를 직접 이름으로 안 걸고 Default로 만든다
        // (a2a-types가 재노출하지 않아 별도 dep 없이는 타입명을 못 씀). 여기선 Data 분기가
        // serde_json::to_string 경로를 타서 패닉 없이 Some을 반환하는지만 확인한다.
        let part = Part {
            metadata: None,
            filename: String::new(),
            media_type: "application/json".to_string(),
            content: Some(Content::Data(Default::default())),
        };
        assert!(part_text(&part).is_some());
    }
}
