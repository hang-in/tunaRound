---
title: "tunaRound v2 Plan 13: 벡터 임베딩 + 하이브리드 검색 (원격 Ollama bge-m3, RRF)"
type: plan
status: planned
priority: P2
updated_at: 2026-06-30
owner: shared
summary: 어휘(FTS)만으론 동의어·의미 매칭이 약하므로 의미 검색을 더한다. Embedder trait + MockEmbedder(결정적) + OllamaEmbedder(reqwest blocking, 원격 bge-m3 dim 1024, 검증된 엔드포인트 http://127.0.0.1:11435 SSH -p [사설포트] 터널) + message_vectors BLOB 저장(증분, content_hash 가드) + cosine top-K + RRF 하이브리드 융합(BM25+벡터, k=60). secall search/{embedding,vector,hybrid}.rs 답습하되 동기·reqwest로 적응. sqlite 피처=Mock+벡터저장+cosine+RRF(기계 테스트), semantic 피처=OllamaEmbedder(라이브). retriever가 embedder 있으면 하이브리드, 없으면 FTS 단독(불변).
---

# tunaRound v2 Plan 13: 벡터 + 하이브리드 Implementation Plan

> **For agentic workers:** TDD. **cargo는 Bash 툴로**(PowerShell이면 exec.rs sh 거짓 실패).
> 출처: `D:/privateProject/seCall/.../search/{embedding,vector,hybrid}.rs`(정본). 결정: docs/design/v2-context-memory-direction_2026-06-30.md(임베딩=원격 Ollama reqwest, ORT 대체). 라이브 검증됨(2232/bge-m3 dim 1024). 아키텍처 재론 금지.

**Goal:** FTS(어휘) 위에 의미 검색을 더한다. 메시지를 bge-m3로 임베딩해 SQLite에 저장하고, 쿼리 임베딩과 cosine으로 의미 유사 슬라이스를 찾는다. FTS 결과와 RRF로 융합해 하이브리드 retrieve를 만든다. retriever에 embedder가 없으면 FTS 단독(동작 불변).

**Architecture:** secall 답습하되 **동기(reqwest blocking)**로 적응(tunaRound store/retriever는 sync, Session.step은 tokio block_on 밖에서 호출되므로 blocking 안전). 에러는 `Result<T,String>`. 피처 계층: **`sqlite`** = Embedder trait + MockEmbedder + message_vectors 저장 + cosine + RRF(전부 Mock로 기계 테스트 가능). **`semantic = ["sqlite","dep:reqwest"]`** = OllamaEmbedder(라이브). SqliteIndexer/SqliteRetriever에 `Option<Box<dyn Embedder>>` 주입 - 있으면 벡터 색인/하이브리드, 없으면 기존 FTS만(불변).

**Tech Stack:** Rust 2024. 신규(optional): `reqwest`(blocking+json, rustls-tls로 Windows OpenSSL 회피). 선행: Plan 9~12 done. 라이브 엔드포인트 `http://127.0.0.1:11435`(SSH `-N -p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@<host>` 터널 떠 있을 때).

> 규율 #5/#6, TDD, 위임 Sonnet + Opus 리뷰, 검증/commit 분리. ANN(usearch)은 비도입 - brute-force cosine(프로젝트 스케일엔 충분, YAGNI). 규모 커지면 후속.

---

## 범위

- **포함:** `Embedder` trait + `MockEmbedder`(sqlite) + `OllamaEmbedder`(semantic) + `message_vectors` 스키마 + 증분 벡터 색인(content_hash 가드) + cosine top-K + RRF 하이브리드 + indexer/retriever/main embedder 배선.
- **비포함:** ANN 인덱스(usearch) · 쿼리 확장 · 청킹 · 재랭킹(cross-encoder). brute-force cosine으로 시작.
- **불변식:** embedder 없음(semantic off / --db 없음) = 벡터 경로 미동작, FTS 단독 = Plan 9~12 동작·테스트 불변.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) optional `reqwest` + `[features] semantic = ["sqlite","dep:reqwest"]`. |
| `src/store/embedding.rs` | (신규) `#[cfg(feature="sqlite")]` Embedder trait + MockEmbedder; `#[cfg(feature="semantic")]` OllamaEmbedder. 첫 줄 역할 주석. |
| `src/store/sqlite.rs` | (수정) message_vectors 스키마(migrate) + `index_vectors` + `vector_search`(cosine). |
| `src/store/indexer.rs` | (수정) SqliteIndexer에 `Option<Box<dyn Embedder>>` - persist 시 벡터도 색인. |
| `src/store/retriever.rs` | (수정) SqliteRetriever에 `Option<Box<dyn Embedder>>` - 있으면 FTS+벡터 RRF 하이브리드. |
| `src/store/mod.rs` | (수정) `pub mod embedding;` + RRF 헬퍼(또는 retriever 내). |
| `src/main.rs` | (수정) semantic 시 OllamaEmbedder 생성·배선(엔드포인트 env/기본). |

---

### Task 1: Embedder trait + MockEmbedder + OllamaEmbedder

**Files:** Modify `Cargo.toml`, `src/store/mod.rs`; Create `src/store/embedding.rs`

- [ ] **Step 1: deps + feature** — `reqwest = { version = "0.12", default-features = false, features = ["blocking","json","rustls-tls"], optional = true }` + `semantic = ["sqlite","dep:reqwest"]`. `cargo build --features semantic`로 reqwest 컴파일 확인(rustls라 OpenSSL 불필요). 깨지면 멈추고 보고.
- [ ] **Step 2: 실패 테스트 먼저** — `embedding.rs` tests:
```rust
#[cfg(all(test, feature = "sqlite"))]
mod tests {
    use super::*;
    #[test]
    fn mock_is_deterministic_and_dim() {
        let e = MockEmbedder::new(1024);
        let a = e.embed("검색 시스템").unwrap();
        let b = e.embed("검색 시스템").unwrap();
        assert_eq!(a.len(), 1024);
        assert_eq!(a, b); // 결정적.
        assert_ne!(a, e.embed("다른 텍스트").unwrap());
    }
    #[cfg(feature = "semantic")]
    #[test]
    #[ignore] // 수동: SSH -p [사설포트] 터널 + http://127.0.0.1:11435 떠 있어야 함.
    fn ollama_embed_live_dim_1024() {
        let e = OllamaEmbedder::new("http://127.0.0.1:11435", "bge-m3");
        let v = e.embed("검색 테스트").unwrap();
        assert_eq!(v.len(), 1024);
    }
}
```
- [ ] **Step 3: 구현(`src/store/embedding.rs`, 첫 줄 역할 주석)**
  - `#[cfg(feature="sqlite")] pub trait Embedder: Send + Sync { fn embed(&self, text: &str) -> Result<Vec<f32>, String>; fn dim(&self) -> usize; }`.
  - `MockEmbedder { dim }`: 텍스트 해시 기반 결정적 f32 벡터(예: 바이트 누적 -> 시드 -> 간단 PRNG로 dim개, L2 정규화). 의미 없음(기계 테스트/폴백용).
  - `#[cfg(feature="semantic")] OllamaEmbedder { endpoint, model, client: reqwest::blocking::Client }`: `embed` = POST `{endpoint}/api/embed` body `{"model":model,"input":[text]}` -> 응답 `{"embeddings":[[f32...]]}`의 `embeddings[0]`. 실패는 `Err(String)`. `dim` = 1024(또는 첫 임베딩 길이).
  - `src/store/mod.rs`에 `pub mod embedding;`.
- [ ] **Step 4: 검증 + 커밋** — `cargo test --features sqlite`(Mock PASS) + `cargo build --features semantic` + `cargo test --features semantic`(ignore 라이브 스킵) + clippy(sqlite/semantic) 경고 0. 기본 `cargo test` 불변. 커밋 `feat(store): Embedder trait + MockEmbedder + OllamaEmbedder(semantic)`.

---

### Task 2: message_vectors 저장 + cosine 검색

**Files:** Modify `src/store/sqlite.rs`

- [ ] **Step 1: 스키마(migrate에 추가, schema_version bump 또는 IF NOT EXISTS 추가)**
```sql
CREATE TABLE IF NOT EXISTS message_vectors (
    session_id   TEXT NOT NULL,
    msg_id       INTEGER NOT NULL,
    dim          INTEGER NOT NULL,
    content_hash TEXT NOT NULL,
    embedding    BLOB NOT NULL,
    PRIMARY KEY(session_id, msg_id)
);
```
- [ ] **Step 2: `index_vectors(&self, session_id, ss, embedder: &dyn Embedder) -> Result<(),String>`** — 각 메시지: content_hash(예: 간단 FNV/표준 해시 문자열) 계산. 기존 행이 같은 hash면 **skip(증분, 재임베딩 방지)**. 아니면 `embedder.embed(content)` -> f32 LE 바이트 BLOB로 upsert(session_id,msg_id,dim,hash,embedding). 삭제된 메시지 정리는 후속(작은 세션 무시 가능).
- [ ] **Step 3: `vector_search(&self, query_vec: &[f32], limit) -> Result<Vec<(String,u64,f64)>,String>`** — message_vectors 전체(또는 한도) 로드, BLOB -> f32 vec, cosine(query, v) 계산, 내림차순 top-K (session_id, msg_id, score). brute-force.
- [ ] **Step 4: 테스트(`#[cfg(all(test, feature="sqlite"))]`, MockEmbedder)** — 두 메시지 index_vectors 후, 한 메시지 content로 만든 쿼리 벡터로 vector_search 시 그 메시지가 top(Mock은 같은 텍스트=같은 벡터라 cosine=1) + 증분 skip(같은 세션 재색인 시 임베딩 호출 수 0 확인 - 카운팅 MockEmbedder).
- [ ] **Step 5: 검증 + 커밋** — `cargo test --features sqlite` PASS, 기본 불변, clippy 클린. 커밋 `feat(store): message_vectors 증분 색인 + cosine 벡터 검색`.

---

### Task 3: RRF 하이브리드 + indexer/retriever/main 배선

**Files:** Modify `src/store/{retriever,indexer,mod}.rs`, `src/main.rs`

- [ ] **Step 1: RRF 융합 헬퍼(`src/store/mod.rs` 또는 retriever)** — secall 답습:
```rust
const RRF_K: f64 = 60.0;
// 두 랭킹 리스트(키=(session_id,msg_id))를 RRF로 융합해 키 순위를 반환.
fn reciprocal_rank_fusion(lexical: &[(String,u64)], vector: &[(String,u64)]) -> Vec<(String,u64)> { ... }
```
  단위 테스트(secall test_rrf_basic 적응).
- [ ] **Step 2: SqliteIndexer 벡터 색인** — `Option<Box<dyn Embedder>>` 추가. persist에서 save_session(FTS) 다음, embedder 있으면 `store.index_vectors(session_id, ss, embedder)`(best-effort eprintln).
- [ ] **Step 3: SqliteRetriever 하이브리드** — `Option<Box<dyn Embedder>>` 추가. retrieve: FTS(`store.search`) 결과 키 리스트 + embedder 있으면 `store.vector_search(embedder.embed(query))` 키 리스트 -> RRF 융합 -> 상위 키를 원문 Utterance로(messages 조회 또는 search/vector 결과 매핑). embedder 없으면 기존 FTS 단독(불변).
- [ ] **Step 4: main 배선** — semantic 시 `OllamaEmbedder`(엔드포인트 = env `TUNAROUND_OLLAMA_URL` 또는 기본 `http://127.0.0.1:11435`, model bge-m3) 생성, **단 연결 실패해도 graceful**(embedder 생성은 즉시, embed 실패는 best-effort). indexer/retriever에 embedder 주입(각자 또는 Arc 공유). semantic off면 embedder None.
- [ ] **Step 5: 테스트** — retriever 하이브리드 단위(MockEmbedder + Fake면 불가하니 파일 DB로 index_vectors+FTS 후 하이브리드 retrieve가 결과 반환). + RRF 단위. 
- [ ] **Step 6: 검증 + 커밋** — `cargo test --features sqlite`(하이브리드 Mock) + `cargo build/test --features semantic` + clippy 전 조합 경고 0. 기본 불변. (라이브 의미 검증은 터널 띄우고 `cargo test --features semantic -- --ignored ollama_embed_live` + 수동 /search 비교 - 사용자 환경.) 커밋 `feat(store): RRF 하이브리드 + embedder 배선(indexer/retriever/main)`.

---

## Self-Review (작성자 체크)
- **답습:** secall embedding(요청 shape)/hybrid(RRF k=60) 정본. 동기·reqwest로 적응(ORT 대체=원격 Ollama, 설계 확정).
- **피처 계층:** sqlite=Mock+저장+cosine+RRF(기계 검증), semantic=OllamaEmbedder(라이브). 기본 빌드 무영향.
- **불변/additive:** embedder 없으면 FTS 단독 = Plan 9~12 불변. 벡터는 best-effort(실패가 토론·검색을 막지 않음).
- **증분:** content_hash 가드로 재임베딩(HTTP 왕복) 방지 - 매 라운드 전량 재persist 대비 비용 억제.
- **YAGNI:** ANN 미도입(brute-force cosine). 규모 입증 시 usearch.

## 위험 / 한계 (후속)
- **라이브 의존:** OllamaEmbedder는 터널(2232) 떠 있어야 동작. 다운 시 embed 실패 -> best-effort(벡터 스킵, FTS만). 라이브 의미 품질 검증은 사용자 환경에서 수동.
- **reqwest blocking + tokio:** Session.step은 block_on 밖이라 안전. 향후 async화하면 충돌 주의.
- **brute-force cosine:** 메시지 수 많아지면 느림 -> ANN 후속.
- **전량 재persist:** save_session은 전량 교체지만 index_vectors는 hash로 증분 -> 임베딩은 새/변경분만.
- **차원 고정:** dim 1024(bge-m3). 모델 바뀌면 message_vectors 재색인 필요.
