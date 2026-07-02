// A2A task 위임의 데이터 모델과 SQLite 영속(순수 타입/조립 함수, HTTP·MCP 배선은 후속 범위).

use serde::{Deserialize, Serialize};

/// A2A task 수명주기 상태. 8-state 중 v1 채택분(auth_required는 후속).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskState {
    Submitted,
    Working,
    InputRequired,
    Completed,
    Failed,
    Canceled,
}

impl TaskState {
    /// SQL TEXT 컬럼 저장용 문자열(serde와 별개 경로. tasks.state와 list_open_tasks_for의 IN(...) 리터럴이
    /// 이 값과 일치해야 한다).
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskState::Submitted => "submitted",
            TaskState::Working => "working",
            TaskState::InputRequired => "input_required",
            TaskState::Completed => "completed",
            TaskState::Failed => "failed",
            TaskState::Canceled => "canceled",
        }
    }

    /// as_str의 역변환. 알 수 없는 문자열은 Err.
    pub fn parse(s: &str) -> Result<TaskState, String> {
        match s {
            "submitted" => Ok(TaskState::Submitted),
            "working" => Ok(TaskState::Working),
            "input_required" => Ok(TaskState::InputRequired),
            "completed" => Ok(TaskState::Completed),
            "failed" => Ok(TaskState::Failed),
            "canceled" => Ok(TaskState::Canceled),
            other => Err(format!("unknown TaskState: {other}")),
        }
    }

    /// dispatcher가 여전히 응답을 기다리는 상태인가. list_open_tasks_for의 SQL 필터와 의미를 동기 유지한다.
    pub fn is_open(&self) -> bool {
        matches!(self, TaskState::Submitted | TaskState::Working | TaskState::InputRequired)
    }
}

/// 콘텐츠 컨테이너. text|data|url 중 하나만 채워지는 것을 기대한다(A2A Part).
/// Wire(JSON-RPC 전송)는 camelCase(mediaType). Rust 필드명은 snake_case를 유지한다.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Part {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub media_type: Option<String>,
}

/// A2A 메시지. 요청 본문·task 상태 메시지·history 항목 공용 타입. role은 "user"|"agent".
/// Wire는 camelCase(messageId/taskId/contextId).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub message_id: String,
    pub role: String,
    pub parts: Vec<Part>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
}

/// 위임 결과 산출물(A2A Artifact). Part를 재사용해 여러 콘텐츠 조각으로 구성될 수 있다.
/// Wire는 camelCase(artifactId).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    pub artifact_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub parts: Vec<Part>,
}

/// A2A task: 위임 단위의 상태·이력·산출물 전체.
/// Wire는 camelCase(contextId/fromAgent/toAgent/statusMessage/createdAt/updatedAt). from_agent/to_agent는
/// 순정 A2A에 없는 tunaRound 중앙-브로커 라우팅 확장 필드(docs/design/v2-a2a-partner-delegation_2026-07-02.md §4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_id: Option<String>,
    pub from_agent: String,
    pub to_agent: String,
    pub state: TaskState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_message: Option<Message>,
    #[serde(default)]
    pub artifacts: Vec<Artifact>,
    #[serde(default)]
    pub history: Vec<Message>,
    pub created_at: String,
    pub updated_at: String,
}

/// A2A task 상태변이 이벤트. SSE 스트리밍 구독자에게 broadcast된다.
#[derive(Clone, Debug)]
pub enum TaskEvent {
    /// 생성(submitted) 또는 상태 전이(working/canceled). 변이 후 Task 전체 스냅샷.
    Status(Task),
    /// 완료(completed + artifacts). 변이 후 Task 전체 스냅샷(artifacts 포함).
    Completed(Task),
}

impl Task {
    /// 신규 submitted task를 만든다. 시각은 호출자가 넘긴다(테스트·실사용 모두 결정적, 숨은 시계 의존 없음).
    pub fn new(
        id: impl Into<String>,
        context_id: Option<String>,
        from_agent: impl Into<String>,
        to_agent: impl Into<String>,
        now: impl Into<String>,
    ) -> Self {
        let now = now.into();
        Task {
            id: id.into(),
            context_id,
            from_agent: from_agent.into(),
            to_agent: to_agent.into(),
            state: TaskState::Submitted,
            status_message: None,
            artifacts: Vec::new(),
            history: Vec::new(),
            created_at: now.clone(),
            updated_at: now,
        }
    }
}

/// tasks 테이블 원시 컬럼(직렬화 전 상태). SQL 조회와 JSON 조립 로직을 분리하기 위한 중간 표현.
pub struct TaskRow {
    pub id: String,
    pub context_id: Option<String>,
    pub from_agent: String,
    pub to_agent: String,
    pub state: String,
    pub message_json: Option<String>,
    pub artifacts_json: Option<String>,
    pub history_json: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

impl TaskRow {
    /// 원시 컬럼에서 Task를 조립한다(순수 함수, JSON 파싱 포함, SQLite 없이 테스트 가능).
    pub fn into_task(self) -> Result<Task, String> {
        let state = TaskState::parse(&self.state)?;
        let status_message = match self.message_json {
            Some(s) => Some(serde_json::from_str(&s).map_err(|e| format!("json: {e}"))?),
            None => None,
        };
        let artifacts = match self.artifacts_json {
            Some(s) => serde_json::from_str(&s).map_err(|e| format!("json: {e}"))?,
            None => Vec::new(),
        };
        let history = match self.history_json {
            Some(s) => serde_json::from_str(&s).map_err(|e| format!("json: {e}"))?,
            None => Vec::new(),
        };
        Ok(Task {
            id: self.id,
            context_id: self.context_id,
            from_agent: self.from_agent,
            to_agent: self.to_agent,
            state,
            status_message,
            artifacts,
            history,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// 기존 history_json(NULL·빈 문자열 가능)에 새 메시지를 append한 JSON 문자열을 만든다(순수 함수).
pub fn append_history_json(existing: Option<&str>, new_msg: &Message) -> Result<String, String> {
    let mut history: Vec<Message> = match existing {
        Some(s) if !s.trim().is_empty() => {
            serde_json::from_str(s).map_err(|e| format!("json: {e}"))?
        }
        _ => Vec::new(),
    };
    history.push(new_msg.clone());
    serde_json::to_string(&history).map_err(|e| format!("json: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_states() -> [TaskState; 6] {
        [
            TaskState::Submitted,
            TaskState::Working,
            TaskState::InputRequired,
            TaskState::Completed,
            TaskState::Failed,
            TaskState::Canceled,
        ]
    }

    #[test]
    fn task_state_as_str_and_parse_roundtrip() {
        for s in all_states() {
            let text = s.as_str();
            assert_eq!(TaskState::parse(text).unwrap(), s, "roundtrip: {text}");
        }
    }

    #[test]
    fn task_state_parse_unknown_is_err() {
        assert!(TaskState::parse("bogus").is_err());
    }

    #[test]
    fn task_state_serde_roundtrip_matches_a2a_wire_strings() {
        let expected = [
            (TaskState::Submitted, "\"submitted\""),
            (TaskState::Working, "\"working\""),
            (TaskState::InputRequired, "\"input_required\""),
            (TaskState::Completed, "\"completed\""),
            (TaskState::Failed, "\"failed\""),
            (TaskState::Canceled, "\"canceled\""),
        ];
        for (state, wire) in expected {
            let json = serde_json::to_string(&state).unwrap();
            assert_eq!(json, wire);
            let back: TaskState = serde_json::from_str(&json).unwrap();
            assert_eq!(back, state);
        }
    }

    #[test]
    fn task_state_is_open_matches_open_states() {
        assert!(TaskState::Submitted.is_open());
        assert!(TaskState::Working.is_open());
        assert!(TaskState::InputRequired.is_open());
        assert!(!TaskState::Completed.is_open());
        assert!(!TaskState::Failed.is_open());
        assert!(!TaskState::Canceled.is_open());
    }

    #[test]
    fn task_new_sets_submitted_and_stamps_given_timestamp() {
        let t = Task::new("t1", Some("ctx1".into()), "win", "mac", "2026-07-02 09:00:00");
        assert_eq!(t.state, TaskState::Submitted);
        assert_eq!(t.context_id.as_deref(), Some("ctx1"));
        assert_eq!(t.created_at, "2026-07-02 09:00:00");
        assert_eq!(t.updated_at, "2026-07-02 09:00:00");
        assert!(t.artifacts.is_empty());
        assert!(t.history.is_empty());
        assert!(t.status_message.is_none());
    }

    #[test]
    fn wire_json_uses_camel_case_field_names() {
        // Task 2(A2A JSON-RPC 엔드포인트) 소비 전제: Message/Task/Artifact/Part는 camelCase로 방출되어야 한다.
        // TaskState는 예외(snake_case 유지, 위 task_state_serde_roundtrip_matches_a2a_wire_strings로 별도 커버).
        let msg = Message {
            message_id: "m1".into(),
            role: "user".into(),
            parts: vec![Part {
                text: Some("본문".into()),
                media_type: Some("text/plain".into()),
                ..Default::default()
            }],
            task_id: Some("t1".into()),
            context_id: Some("ctx1".into()),
        };
        let msg_json = serde_json::to_value(&msg).unwrap();
        assert!(msg_json.get("messageId").is_some(), "messageId 없음: {msg_json}");
        assert!(msg_json.get("taskId").is_some(), "taskId 없음: {msg_json}");
        assert!(msg_json.get("contextId").is_some(), "contextId 없음: {msg_json}");
        assert!(msg_json.get("message_id").is_none(), "snake_case 잔존: {msg_json}");
        let part_json = &msg_json["parts"][0];
        assert!(part_json.get("mediaType").is_some(), "mediaType 없음: {part_json}");

        let mut task = Task::new("t1", Some("ctx1".into()), "win-claude", "mac-claude", "2026-07-02 09:00:00");
        task.status_message = Some(msg.clone());
        task.artifacts = vec![Artifact { artifact_id: "a1".into(), name: None, parts: vec![] }];
        let task_json = serde_json::to_value(&task).unwrap();
        assert!(task_json.get("contextId").is_some(), "contextId 없음: {task_json}");
        assert!(task_json.get("fromAgent").is_some(), "fromAgent 없음: {task_json}");
        assert!(task_json.get("toAgent").is_some(), "toAgent 없음: {task_json}");
        assert!(task_json.get("statusMessage").is_some(), "statusMessage 없음: {task_json}");
        assert!(task_json.get("createdAt").is_some(), "createdAt 없음: {task_json}");
        assert!(task_json.get("updatedAt").is_some(), "updatedAt 없음: {task_json}");
        assert_eq!(task_json["artifacts"][0]["artifactId"], "a1");
        // state는 snake_case 그대로(A2A wire 관례, task 지시로 변경 금지).
        assert_eq!(task_json["state"], "submitted");

        // 대칭 확인: camelCase JSON에서 역직렬화해도 원래 구조체와 같아야 한다(round-trip 불변).
        let msg_back: Message = serde_json::from_value(msg_json).unwrap();
        assert_eq!(msg_back, msg);
    }

    fn sample_message(id: &str) -> Message {
        Message {
            message_id: id.into(),
            role: "user".into(),
            parts: vec![Part { text: Some("내용".into()), ..Default::default() }],
            task_id: Some("t1".into()),
            context_id: None,
        }
    }

    #[test]
    fn task_row_into_task_roundtrips_json_columns() {
        let msg = sample_message("m1");
        let artifacts = vec![Artifact {
            artifact_id: "a1".into(),
            name: Some("결과".into()),
            parts: vec![Part { text: Some("산출물".into()), ..Default::default() }],
        }];
        let history = vec![sample_message("h1"), sample_message("h2")];

        let row = TaskRow {
            id: "t1".into(),
            context_id: Some("ctx1".into()),
            from_agent: "win".into(),
            to_agent: "mac".into(),
            state: "working".into(),
            message_json: Some(serde_json::to_string(&msg).unwrap()),
            artifacts_json: Some(serde_json::to_string(&artifacts).unwrap()),
            history_json: Some(serde_json::to_string(&history).unwrap()),
            created_at: "2026-07-02 09:00:00".into(),
            updated_at: "2026-07-02 09:05:00".into(),
        };

        let task = row.into_task().unwrap();
        assert_eq!(task.state, TaskState::Working);
        assert_eq!(task.status_message, Some(msg));
        assert_eq!(task.artifacts, artifacts);
        assert_eq!(task.history, history);
    }

    #[test]
    fn task_row_into_task_none_json_defaults_to_empty() {
        let row = TaskRow {
            id: "t1".into(),
            context_id: None,
            from_agent: "win".into(),
            to_agent: "mac".into(),
            state: "submitted".into(),
            message_json: None,
            artifacts_json: None,
            history_json: None,
            created_at: "2026-07-02 09:00:00".into(),
            updated_at: "2026-07-02 09:00:00".into(),
        };
        let task = row.into_task().unwrap();
        assert!(task.status_message.is_none());
        assert!(task.artifacts.is_empty());
        assert!(task.history.is_empty());
    }

    #[test]
    fn task_row_into_task_unknown_state_is_err() {
        let row = TaskRow {
            id: "t1".into(),
            context_id: None,
            from_agent: "win".into(),
            to_agent: "mac".into(),
            state: "not_a_state".into(),
            message_json: None,
            artifacts_json: None,
            history_json: None,
            created_at: "2026-07-02 09:00:00".into(),
            updated_at: "2026-07-02 09:00:00".into(),
        };
        assert!(row.into_task().is_err());
    }

    #[test]
    fn append_history_json_appends_in_order() {
        let m1 = sample_message("h1");
        let after_first = append_history_json(None, &m1).unwrap();
        let history1: Vec<Message> = serde_json::from_str(&after_first).unwrap();
        assert_eq!(history1, vec![m1.clone()]);

        let m2 = sample_message("h2");
        let after_second = append_history_json(Some(&after_first), &m2).unwrap();
        let history2: Vec<Message> = serde_json::from_str(&after_second).unwrap();
        assert_eq!(history2, vec![m1, m2]);
    }

    #[test]
    fn append_history_json_treats_empty_string_as_no_history() {
        let m1 = sample_message("h1");
        let out = append_history_json(Some(""), &m1).unwrap();
        let history: Vec<Message> = serde_json::from_str(&out).unwrap();
        assert_eq!(history, vec![m1]);
    }

    #[test]
    fn part_default_is_all_none() {
        let p = Part::default();
        assert!(p.text.is_none());
        assert!(p.data.is_none());
        assert!(p.url.is_none());
        assert!(p.media_type.is_none());
    }
}
