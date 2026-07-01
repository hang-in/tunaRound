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
}

#[cfg(feature = "semantic")]
impl OllamaEmbedder {
    pub fn new(endpoint: &str, model: &str) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            model: model.to_string(),
            client: reqwest::blocking::Client::new(),
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
        data.embeddings
            .into_iter()
            .next()
            .ok_or_else(|| "ollama embed: empty embeddings".to_string())
    }

    fn dim(&self) -> usize {
        1024
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
    #[ignore] // 수동: SSH -p [사설포트] 터널 + http://127.0.0.1:11435 떠 있어야 함.
    fn ollama_embed_live_dim_1024() {
        let e = OllamaEmbedder::new("http://127.0.0.1:11435", "bge-m3");
        let v = e.embed("검색 테스트").unwrap();
        assert_eq!(v.len(), 1024);
    }
}
