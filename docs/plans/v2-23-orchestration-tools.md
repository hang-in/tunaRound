# Plan v2-23: Stage 1 오케스트레이션 툴 read_transcript

> (A) 코어-백엔드 Stage 1. docs/design/v2-A2A-core-backend_2026-06-30.md.
> 기존 MCP 서버(mcp.rs, search_context)에 전사 pull 툴을 더한다. 백엔드/터널 불필요. 에이전트가 "검색"뿐 아니라 "전사 통째"도 코어에서 당겨오게 하는 첫 primitive.

## 범위

- **Task 1 = `read_transcript` 툴만.** get_roster는 후속(로스터가 DB에 없음, 별도 전달 필요).
- 세션 id 스코핑: 이번엔 **파라미터 + 기본 "default"**. 실제 현재 세션 id를 spawn 인자로 주입하는 배선은 Task 2(claude.rs/codex.rs MCP config).
- 에이전트는 여전히 stateless spawn(저위험). pull 전환의 실제 사용(push 축소)은 Stage 2.

## 확인된 API

- `SqliteStore::load_session(&self, session_id) -> Result<Option<StoredSession>, String>` (sqlite.rs:210). StoredSession{messages: Vec<StoredMessage>, head}.
- `crate::store::path_to_root(&messages, head) -> Vec<Utterance>` (mod.rs:148). 활성 경로(root->head).
- Send+Sync: rusqlite Connection은 Send이나 !Sync → `Mutex<SqliteStore>`로 감싼다(SqliteRetriever 답습, retriever.rs:14-16).

## Task 1 (Sonnet 위임)

### 1a. `TranscriptReader` 트레잇 (src/orchestrator/mod.rs, ContextRetriever 옆)
```rust
/// 세션 전사를 읽어 오는 추상(코어를 백엔드로 노출하는 오케스트레이션 primitive).
pub trait TranscriptReader: Send + Sync {
    /// session_id의 활성 경로(root->head) 발언. max_turns=Some(n)이면 마지막 n턴만.
    fn read_transcript(&self, session_id: &str, max_turns: Option<usize>) -> Vec<Utterance>;
}
```

### 1b. `SqliteTranscriptReader` (src/store/retriever.rs sqlite 모듈 또는 신규, sqlite feature)
- `SqliteRetriever` 동형: `store: std::sync::Mutex<SqliteStore>`. `new(store: SqliteStore)`.
- impl:
```rust
fn read_transcript(&self, session_id: &str, max_turns: Option<usize>) -> Vec<Utterance> {
    let store = self.store.lock().unwrap_or_else(|e| e.into_inner());
    let Ok(Some(ss)) = store.load_session(session_id) else { return Vec::new(); };
    let path = crate::store::path_to_root(&ss.messages, ss.head);
    match max_turns {
        Some(n) if path.len() > n => path[path.len() - n..].to_vec(),
        _ => path,
    }
}
```

### 1c. MCP 서버 (src/mcp.rs, mcp feature)
- `TunaSearchServer`에 필드 `reader: Option<Arc<dyn TranscriptReader>>` 추가(기본 None).
- `new(retriever)`는 유지(reader=None), 빌더 `with_transcript_reader(mut self, reader: Arc<dyn TranscriptReader>) -> Self` 추가(테스트 churn 최소).
- 신규 툴:
```rust
#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscriptParams {
    /// 세션 id(기본 "default").
    pub session_id: Option<String>,
    /// 마지막 N턴만(생략=전체).
    pub max_turns: Option<usize>,
}

#[tool(description = "현재 토론 전사를 읽는다(활성 경로). 검색이 아니라 통째 맥락이 필요할 때.")]
async fn read_transcript(&self, Parameters(p): Parameters<TranscriptParams>) -> Result<CallToolResult, McpError> {
    let Some(reader) = &self.reader else {
        return Ok(CallToolResult::success(vec![Content::text("전사 리더 미연결".to_string())]));
    };
    let sid = p.session_id.unwrap_or_else(|| "default".to_string());
    let utts = reader.read_transcript(&sid, p.max_turns);
    let text = if utts.is_empty() { "전사 없음".to_string() }
        else { utts.iter().map(|u| format!("[{}] {}", u.speaker, u.content)).collect::<Vec<_>>().join("\n\n") };
    Ok(CallToolResult::success(vec![Content::text(text)]))
}
```
- `get_info` instructions에 read_transcript 안내 한 줄 추가.

### 1d. 배선
- `start_mcp_server(retriever, reader: Option<Arc<dyn TranscriptReader>>)` 시그니처에 reader 추가 → `TunaSearchServer::new(retriever)` 후 reader Some이면 `.with_transcript_reader(r)`.
- main.rs `--mcp-search` 분기(mcp+sqlite feature): --db로 SqliteRetriever 만들 때 같은 db로 `SqliteTranscriptReader`도 만들어 `Arc`로 전달. --db 없으면 None.

### 테스트 (mcp.rs)
- `FakeTranscriptReader`(고정 Utterance 반환)로 read_transcript가 Ok + 전사 포함.
- reader=None이면 "전사 리더 미연결" 안내 반환.
- 기존 search_context 테스트 무영향(new 시그니처 유지).

### 검증
- cargo는 **Bash 툴**(Windows). `cargo test`(기본) + `cargo test --features "mcp sqlite morphology"` 통과, `cargo clippy --features "mcp sqlite"` 클린. 기존 통과 수 유지.
- diff 요약 + 새 테스트 결과 + 빌드/clippy 보고. **커밋 금지**(Opus 리뷰 후).

## 후속 (Task 2+)
- 현재 세션 id를 MCP 서버 spawn 인자(--session-id)로 주입 → 에이전트가 자기 토론 전사를 정확히 읽음.
- get_roster(로스터 전달 경로 필요).
- Stage 2: 프롬프트를 통째 push 대신 "전사는 read_transcript로 당겨라" 포인터로 축소 + 재전송량 실측.
