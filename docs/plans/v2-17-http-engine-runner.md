---
title: "tunaRound v2 Plan 17: OpenAI-호환 HTTP 엔진 러너 (ollama/lmstudio/openai/cloud)"
type: plan
status: planned
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 로스터를 N-엔진 ready로 만든 토대 위에, claude/codex 외 엔진을 추가한다. ollama(local)·ollama cloud·lmstudio·openai는 전부 OpenAI 호환 /v1/chat/completions라 제네릭 HTTP chat 러너 1개로 커버. 로스터 좌석에 base_url/model/api_key_env 필드 추가, build_registry가 base_url+model 있는 좌석을 HTTP 러너로(engine 이름이 키라 서로 다른 모델 다좌석 가능). 이미 떠 있는 Ollama(gemma4:e2b)로 로컬 LLM 좌석 즉시. opencode CLI 러너는 후속. engines feature(reqwest). HTTP 좌석은 레포 직독 없는 순수 추론 참가자(프롬프트 맥락만).
---

# tunaRound v2 Plan 17: HTTP 엔진 러너 Implementation Plan

> **For agentic workers:** TDD. **cargo는 Bash 툴로.**
> 결정(2026-06-30): 로스터 N-ready인데 러너가 claude/codex뿐. OpenAI 호환 HTTP 러너로 ollama/lmstudio/openai/cloud를 한 번에. 러너 테스트는 claude/codex 패턴 답습(pure builder/parser + #[ignore] 라이브). 아키텍처 재론 금지.

**Goal:** claude/codex 외 엔진을 토론 좌석으로 쓸 수 있게 한다. OpenAI 호환 chat API(`/v1/chat/completions`)를 말하는 모든 제공자(ollama local·ollama cloud·lmstudio·openai)를 제네릭 HTTP 러너 1개로 커버. 로스터 좌석에 엔드포인트/모델을 적으면 그 좌석이 동작.

**Architecture:** 신규 `OpenAiChatRunner`(reqwest blocking, `{base_url}/v1/chat/completions`에 `{model, messages:[{role:"user",content:prompt}]}` POST, 옵션 `Authorization: Bearer <key>`, `choices[0].message.content` 파싱). RunMode 무시(HTTP LLM은 레포 직독 없음 - 순수 추론 좌석, 프롬프트 맥락만). 로스터 `SeatConfig`에 `base_url`/`model`/`api_key_env`(키 담은 env 변수명) 추가. `build_registry`가 각 distinct engine에 대해: claude/codex는 기존, 그 외 **base_url+model 있으면 OpenAiChatRunner(engine 이름이 레지스트리 키)**, 둘 다 없으면 에러. engine 이름이 키라 서로 다른 base_url/model 좌석 N개 가능. `engines` feature(reqwest, semantic과 별개로 reqwest 공유).

**Tech Stack:** Rust 2024. reqwest(이미 보유, blocking+json). 신규 feature `engines = ["dep:reqwest"]`. 선행: Plan 02(로스터) done. 라이브: 이미 떠 있는 Ollama(`http://127.0.0.1:11435/v1/chat/completions`, model gemma4:e2b - OpenAI 호환 엔드포인트).

> 규율 #5/#6, TDD, 위임 Sonnet + Opus 리뷰. **HTTP 좌석 한계 명시:** claude/codex와 달리 레포를 직접 읽지 않음(프롬프트 맥락만). 다양성/로컬 LLM 좌석 용도.

---

## 범위

- **포함:** `engines` feature + `src/runner/http.rs`(OpenAiChatRunner + pure request 빌더/response 파서 + Runner impl) + roster SeatConfig 필드 확장 + build_registry HTTP 분기 + 테스트(pure + #[ignore] 라이브).
- **비포함:** opencode CLI 러너(후속) · 스트리밍 파싱(non-stream로 시작) · 도구/MCP 연결(HTTP 좌석은 search_context 미배선, 후속) · per-seat 모델 외 파라미터(temperature 등).
- **불변식:** engines feature off = HTTP 러너 미컴파일, build_registry는 base_url 있는 좌석에 "engines feature 필요" 에러. claude/codex 좌석·기존 동작·테스트 불변.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) `engines = ["dep:reqwest"]`. |
| `src/runner/http.rs` | (신규) OpenAiChatRunner + build_chat_request(pure) + parse_chat_response(pure) + Runner impl. 첫 줄 역할 주석. |
| `src/runner/mod.rs` | (수정) `#[cfg(feature="engines")] pub mod http;`. |
| `src/roster.rs` | (수정) SeatConfig base_url/model/api_key_env + build_registry HTTP 분기. |

---

### Task 1: HTTP 러너 + 로스터 배선

**Files:** Modify `Cargo.toml`, `src/runner/mod.rs`, `src/roster.rs`; Create `src/runner/http.rs`

- [ ] **Step 1: feature + 실패 테스트(pure)** — `Cargo.toml`에 `engines = ["dep:reqwest"]`. `cargo build --features engines`로 컴파일 확인.
  - `src/runner/http.rs` tests(첫 줄 역할 주석 `// OpenAI 호환 chat API HTTP 러너(ollama/lmstudio/openai/cloud).`):
```rust
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
```

- [ ] **Step 2: 구현(http.rs)**
  - `pub fn build_chat_request(model: &str, prompt: &str) -> serde_json::Value`: `{"model":model,"messages":[{"role":"user","content":prompt}],"stream":false}`.
  - `pub fn parse_chat_response(v: &serde_json::Value) -> Result<RunOutput, RunError>`: `choices[0].message.content`(없으면 `RunError::Empty`). 토큰 = `usage.prompt_tokens`/`usage.completion_tokens`(없으면 0).
  - `pub struct OpenAiChatRunner { base_url: String, model: String, api_key: Option<String>, client: reqwest::blocking::Client }` + `new(base_url, model, api_key)`.
  - `impl Runner`: `run(&self, input) -> Result<RunOutput, RunError>`: POST `{base_url}/v1/chat/completions`, body=build_chat_request(self.model, input.prompt), api_key 있으면 bearer 헤더. 상태/네트워크 에러는 `RunError::Spawn`/`Io`(적절히). 응답 json -> parse_chat_response. **RunMode는 무시**(주석으로 명시: HTTP LLM은 레포 직독 없음).
  - `src/runner/mod.rs`에 `#[cfg(feature="engines")] pub mod http;`.

- [ ] **Step 3: 로스터 배선(roster.rs)**
  - `SeatConfig`에 `#[serde(default)] pub base_url: Option<String>`, `pub model: Option<String>`, `pub api_key_env: Option<String>` 추가.
  - `build_registry`: 각 distinct engine에 대해 claude/codex 기존. 그 외:
    - `#[cfg(feature="engines")]`: 그 engine의 첫 좌석에서 base_url+model 있으면 `OpenAiChatRunner::new(base_url, model, api_key_env.and_then(|e| std::env::var(e).ok()))`를 engine 이름으로 insert. base_url/model 없으면 에러("HTTP 엔진엔 base_url, model 필요").
    - `#[cfg(not(feature="engines"))]`: base_url 있는 좌석이면 "engines feature 필요" 에러, 아니면 기존 "알 수 없는 엔진" 에러.
  - 테스트: HTTP 좌석 로스터(`{"engine":"local","base_url":"http://x","model":"m"}`)가 `--features engines`에서 build_registry Ok / base_url 없으면 에러 / 기존 claude·codex 불변.

- [ ] **Step 4: 검증 + 커밋**
  - `cargo test`(기본=morphology+sqlite) 불변 + `cargo test --features engines`(pure 테스트) PASS + `cargo build --features engines` OK. clippy(기본/engines) 0.
  - (선택) #[ignore] 라이브: `OpenAiChatRunner::new("http://127.0.0.1:11435","gemma4:e2b",None).run(...)` -> 응답(터널 필요, 수동).
  - 커밋: `feat(runner): OpenAI 호환 HTTP 엔진 러너 + 로스터 배선(ollama/lmstudio/openai)`.

---

## Self-Review (작성자 체크)
- **한 러너로 다수 제공자:** OpenAI 호환 /v1/chat/completions 공통분모 -> ollama/lmstudio/openai/cloud 동시 커버. 재발명 안 함.
- **engine 이름이 키:** 서로 다른 base_url/model 좌석 N개 가능(레지스트리 키 = engine 이름).
- **불변:** engines off = 미컴파일, claude/codex 불변. base_url 좌석은 feature 없으면 명확한 에러.
- **테스트성:** request/response를 pure 함수로(claude/codex 패턴). 네트워크는 #[ignore] 라이브.
- **한계 명시:** HTTP 좌석은 레포 직독 없음(프롬프트 맥락만). search_context MCP 미배선(후속).

## 위험 / 한계 (후속)
- **레포 직독 없음:** HTTP LLM은 claude/codex와 달리 작업 디렉터리를 직접 못 읽음 -> "같은 레포" 속성 일부 상실. 다양성/로컬 LLM 좌석 용도로 한정.
- **검색 도구 미배선:** HTTP 좌석은 search_context(MCP) 없음(MCP는 CLI 러너 경유). HTTP 좌석에 검색 맥락은 프롬프트 주입(RAG)으로만.
- **non-stream:** 스트리밍 파싱 없이 한 방 응답. 긴 응답/타임아웃은 idle watchdog 밖(HTTP 자체 타임아웃 필요할 수 있음).
- **opencode:** CLI 러너는 별 slice(opencode argv/출력 포맷 실측 필요).
- **api_key:** env 변수명으로 받음(로스터에 키 평문 금지). 키 없으면 무인증(ollama/lmstudio local OK).
