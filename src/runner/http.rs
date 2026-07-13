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

    Ok(RunOutput {
        content,
        input_tokens,
        output_tokens,
    })
}

/// HTTP 러너 기본 요청 타임아웃(초). reqwest::blocking::Client::new()의 기본 30초는 stream:false라
/// 응답시간=전체 생성시간인데 로컬LLM 긴 생성이 30초를 흔히 넘겨 실패했다. 다른 러너의 idle_timeout
/// 기본값(600초)과 정합을 맞춘다.
const DEFAULT_TIMEOUT_SECS: u64 = 600;

/// 지정 타임아웃(초)으로 blocking Client를 만든다. 빌더 실패(TLS 초기화 실패 등, 사실상 발생 안 함)는
/// expect로 즉시 드러낸다. Client::new()로 폴백하지 않는다: Client::new()도 내부에서 같은 build를
/// unwrap하므로 이 실패 상황에선 동일하게 패닉하고, 폴백은 설정한 timeout_secs를 조용히 30초로
/// 되돌려 "긴 요청이 예상보다 빨리 끊기는" 어긋남만 만든다(coderabbit Major).
fn build_client(timeout_secs: u64) -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(timeout_secs))
        .build()
        .expect("reqwest blocking Client 빌드 실패(TLS 초기화 등)")
}

/// OpenAI 호환 /v1/chat/completions HTTP 러너.
/// HTTP LLM은 레포를 직접 읽지 않음(프롬프트 맥락만). RunMode 무시.
pub struct OpenAiChatRunner {
    base_url: String,
    model: String,
    api_key: Option<String>,
    client: reqwest::blocking::Client,
    timeout_secs: u64,
}

impl OpenAiChatRunner {
    pub fn new(base_url: &str, model: &str, api_key: Option<String>) -> Self {
        Self {
            base_url: base_url.to_string(),
            model: model.to_string(),
            api_key,
            client: build_client(DEFAULT_TIMEOUT_SECS),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// 요청 타임아웃(초)을 기본값(600) 대신 다른 값으로 재설정한다.
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.timeout_secs = timeout_secs;
        self.client = build_client(timeout_secs);
        self
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

    #[test]
    fn default_timeout_is_600_and_with_timeout_overrides() {
        // reqwest::blocking::Client 내부 타임아웃은 직접 조회할 수 없으니, 우리가 보관하는
        // timeout_secs 필드로 기본값(600, 다른 러너 idle_timeout과 정합)과 세터 동작을 검증한다.
        let r = OpenAiChatRunner::new("http://localhost:1234", "m", None);
        assert_eq!(r.timeout_secs, 600);
        let r2 = r.with_timeout(30);
        assert_eq!(r2.timeout_secs, 30);
    }
}
