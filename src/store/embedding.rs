// 텍스트 임베딩 추상화: Embedder 트레이트, MockEmbedder(결정적 PRNG), OllamaEmbedder(원격 Ollama, semantic 피처).

#[cfg(feature = "sqlite")]
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String>;
    fn dim(&self) -> usize;
    /// 모델 정체성(provider:model). 벡터 증분 색인의 무효화 키 일부.
    /// 이 값이 바뀌면(모델 교체) 같은 내용이라도 재임베딩해야 stale 벡터를 막는다.
    fn model_id(&self) -> String;
}

/// 결정적 MockEmbedder: 텍스트 FNV-1a 해시 시드 -> LCG PRNG -> L2 정규화. 테스트/폴백용.
#[cfg(feature = "sqlite")]
pub struct MockEmbedder {
    dim: usize,
}

#[cfg(feature = "sqlite")]
impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

#[cfg(feature = "sqlite")]
impl Embedder for MockEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        // FNV-1a 해시로 결정적 시드 생성.
        let mut hash: u64 = 14695981039346656037;
        for byte in text.as_bytes() {
            hash ^= *byte as u64;
            hash = hash.wrapping_mul(1099511628211);
        }

        // LCG PRNG으로 dim개 f32 생성(-1..1 범위).
        let mut state = hash;
        let mut result = Vec::with_capacity(self.dim);
        for _ in 0..self.dim {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let val = (state >> 33) as f32 / (u32::MAX as f32) * 2.0 - 1.0;
            result.push(val);
        }

        // L2 정규화.
        let norm: f32 = result.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 1e-9 {
            for x in result.iter_mut() {
                *x /= norm;
            }
        }

        Ok(result)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> String {
        format!("mock-{}", self.dim)
    }
}

/// 원격 Ollama /api/embed 호출 임베더(reqwest blocking). semantic 피처 전용.
#[cfg(feature = "semantic")]
pub struct OllamaEmbedder {
    endpoint: String,
    model: String,
    client: reqwest::blocking::Client,
    dim: std::sync::OnceLock<usize>,
}

/// 임베딩 HTTP 요청 총 타임아웃 기본값(초). 콜드스타트 qwen3 over-tunnel 여유 + 무한행 차단.
#[cfg(feature = "semantic")]
const DEFAULT_EMBED_TIMEOUT_SECS: u64 = 30;

/// TUNAROUND_EMBED_TIMEOUT_SECS 원문(env)에서 총 타임아웃 초를 파싱한다. 미설정·비수·0이면 기본값.
/// (env 접근을 분리한 순수 함수라 결정적 단위테스트 가능.)
#[cfg(feature = "semantic")]
fn timeout_secs_from(raw: Option<String>) -> u64 {
    raw.and_then(|s| s.trim().parse::<u64>().ok())
        .filter(|&s| s > 0)
        .unwrap_or(DEFAULT_EMBED_TIMEOUT_SECS)
}

#[cfg(feature = "semantic")]
impl OllamaEmbedder {
    pub fn new(endpoint: &str, model: &str) -> Self {
        // 타임아웃 없는 Client::new()는 Ollama 행 시 .send()가 무한 대기 → search_context의 spawn_blocking
        // 스레드를 영구 점유한다. 총 타임아웃(env 조절 가능)으로 무한행을 차단한다. build 실패(TLS/resolver
        // 초기화 불가)는 Client::new()도 내부 .build().expect()로 같은 이유로 패닉하므로 폴백이 무의미하다 →
        // expect로 기존 Client::new()와 동일하게 즉시 실패시킨다(동작 불변, 타임아웃만 추가).
        let timeout = std::time::Duration::from_secs(timeout_secs_from(
            std::env::var("TUNAROUND_EMBED_TIMEOUT_SECS").ok(),
        ));
        let client = reqwest::blocking::Client::builder()
            .timeout(timeout)
            .build()
            .expect("reqwest embed client build");
        Self {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
            client,
            dim: std::sync::OnceLock::new(),
        }
    }

    fn cache_dim(&self, dim: usize) -> Result<(), String> {
        match self.dim.set(dim) {
            Ok(()) => Ok(()),
            Err(_) if self.dim.get() == Some(&dim) => Ok(()),
            Err(_) => Err(format!(
                "ollama embed: dimension changed from {} to {dim}",
                self.dim.get().copied().unwrap_or_default()
            )),
        }
    }

    /// 환경변수로 구성: TUNAROUND_OLLAMA_URL(기본 127.0.0.1:11435) + TUNAROUND_EMBED_MODEL.
    /// 기본 모델 qwen3-embedding:0.6b는 실코퍼스에서 bge-m3보다 hybrid MRR 우위(측정, 둘 다 1024-dim).
    /// bge-m3로 복귀는 TUNAROUND_EMBED_MODEL=bge-m3. 모델 교체 시 model_id 무효화 키가 재임베딩을 자동 처리.
    pub const DEFAULT_MODEL: &'static str = "qwen3-embedding:0.6b";
    pub fn from_env() -> Self {
        let endpoint = std::env::var("TUNAROUND_OLLAMA_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:11435".to_string());
        let model = std::env::var("TUNAROUND_EMBED_MODEL")
            .unwrap_or_else(|_| Self::DEFAULT_MODEL.to_string());
        Self::new(&endpoint, &model)
    }
}

#[cfg(feature = "semantic")]
#[derive(serde::Deserialize)]
struct Resp {
    embeddings: Vec<Vec<f32>>,
}

#[cfg(feature = "semantic")]
impl Embedder for OllamaEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>, String> {
        let resp = self
            .client
            .post(format!("{}/api/embed", self.endpoint))
            .json(&serde_json::json!({"model": self.model, "input": [text]}))
            .send()
            .map_err(|e| format!("ollama embed: {e}"))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().unwrap_or_default();
            return Err(format!("ollama embed: {status} {body}"));
        }

        let data: Resp = resp.json().map_err(|e| format!("ollama embed: {e}"))?;
        let embedding = data
            .embeddings
            .into_iter()
            .next()
            .ok_or_else(|| "ollama embed: empty embeddings".to_string())?;
        self.cache_dim(embedding.len())?;
        Ok(embedding)
    }

    fn dim(&self) -> usize {
        self.dim.get().copied().unwrap_or_default()
    }

    fn model_id(&self) -> String {
        format!("ollama:{}", self.model)
    }
}

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

    #[test]
    fn model_id_reflects_identity() {
        assert_eq!(MockEmbedder::new(64).model_id(), "mock-64");
        assert_eq!(MockEmbedder::new(1024).model_id(), "mock-1024");
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn embed_timeout_parsing_falls_back_on_bad_input() {
        // 정상 값은 그대로, 미설정·비수·0·음수·공백은 기본값으로 폴백(무한행 방지 불변식).
        assert_eq!(timeout_secs_from(Some("15".to_string())), 15);
        assert_eq!(timeout_secs_from(Some("  45  ".to_string())), 45);
        assert_eq!(timeout_secs_from(None), DEFAULT_EMBED_TIMEOUT_SECS);
        assert_eq!(
            timeout_secs_from(Some("0".to_string())),
            DEFAULT_EMBED_TIMEOUT_SECS
        );
        assert_eq!(
            timeout_secs_from(Some("-5".to_string())),
            DEFAULT_EMBED_TIMEOUT_SECS
        );
        assert_eq!(
            timeout_secs_from(Some("abc".to_string())),
            DEFAULT_EMBED_TIMEOUT_SECS
        );
        assert_eq!(
            timeout_secs_from(Some("".to_string())),
            DEFAULT_EMBED_TIMEOUT_SECS
        );
    }

    #[cfg(feature = "semantic")]
    #[test]
    fn ollama_dim_is_cached_from_first_embedding_length() {
        let e = OllamaEmbedder::new("http://unused", "test-model");
        assert_eq!(e.dim(), 0);

        e.cache_dim(768).unwrap();
        assert_eq!(e.dim(), 768);
        assert!(e.cache_dim(1024).is_err());
        assert_eq!(e.dim(), 768);
    }

    #[cfg(feature = "semantic")]
    #[test]
    #[ignore] // 수동: SSH -p [사설포트] 터널 + http://127.0.0.1:11435 떠 있어야 함.
    fn ollama_embed_live_dim_matches_response() {
        let e = OllamaEmbedder::new("http://127.0.0.1:11435", "bge-m3");
        let v = e.embed("검색 테스트").unwrap();
        assert_eq!(e.dim(), v.len());
    }
}
