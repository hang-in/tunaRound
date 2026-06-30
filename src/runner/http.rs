// OpenAI 호환 chat API HTTP 러너(ollama/lmstudio/openai/cloud).

use super::{RunError, RunInput, RunOutput, Runner};

/// POST body를 조립한다(순수 함수, 네트워크 없음).
pub fn build_chat_request(model: &str, prompt: &str) -> serde_json::Value {
    serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "stream": false
    })
}

/// /v1/chat/completions 응답에서 내용과 토큰을 뽑는다(순수 함수).
pub fn parse_chat_response(v: &serde_json::Value) -> Result<RunOutput, RunError> {
    let content = v
        .get("choices")
        .and_then(|c| c.as_array())
        .and_then(|arr| arr.first())
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .ok_or_else(|| RunError::Empty("choices[0].message.content 없음".to_string()))?
        .to_string();

    let input_tokens = v
        .get("usage")
        .and_then(|u| u.get("prompt_tokens"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);
    let output_tokens = v
        .get("usage")
        .and_then(|u| u.get("completion_tokens"))
        .and_then(|t| t.as_i64())
        .unwrap_or(0);

    Ok(RunOutput { content, input_tokens, output_tokens })
}

/// OpenAI 호환 /v1/chat/completions HTTP 러너.
/// HTTP LLM은 레포를 직접 읽지 않음(프롬프트 맥락만). RunMode 무시.
pub struct OpenAiChatRunner {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
}

impl OpenAiChatRunner {
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key,
            client: reqwest::blocking::Client::new(),
        }
    }
}

impl Runner for OpenAiChatRunner {
    fn run(&self, input: &RunInput) -> Result<RunOutput, RunError> {
        // RunMode는 무시한다. HTTP LLM은 레포 직독 없음(프롬프트 맥락만).
        let body = build_chat_request(&self.model, &input.prompt);
        let url = format!("{}/v1/chat/completions", self.base_url);
        let mut req = self.client.post(&url).json(&body);
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key);
        }
        let resp = req
            .send()
            .map_err(|e| RunError::Spawn(format!("HTTP 요청 실패: {e}")))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body_text = resp.text().unwrap_or_default();
            return Err(RunError::Io(format!("HTTP {status}: {body_text}")));
        }
        let json: serde_json::Value = resp
            .json()
            .map_err(|e| RunError::Io(format!("응답 JSON 파싱 실패: {e}")))?;
        parse_chat_response(&json)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_has_model_and_user_message() {
        let body = build_chat_request("gemma4:e2b", "이 설계 어때?");
        assert_eq!(body["model"], "gemma4:e2b");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "이 설계 어때?");
    }

    #[test]
    fn parse_extracts_content_and_tokens() {
        let json = serde_json::json!({
            "choices":[{"message":{"content":"좋은 설계입니다"}}],
            "usage":{"prompt_tokens":11,"completion_tokens":7}
        });
        let out = parse_chat_response(&json).unwrap();
        assert_eq!(out.content, "좋은 설계입니다");
        assert_eq!(out.input_tokens, 11);
        assert_eq!(out.output_tokens, 7);
    }

    #[test]
    fn parse_empty_choices_errs() {
        let json = serde_json::json!({"choices":[]});
        assert!(parse_chat_response(&json).is_err());
    }
}
