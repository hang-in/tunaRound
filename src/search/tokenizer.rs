// 한국어 형태소 토크나이저(secall 포팅): Tokenizer trait + Kiwi/lindera + factory.
use std::collections::HashSet;

use lindera::{
    dictionary::{load_embedded_dictionary, DictionaryKind},
    mode::Mode,
    segmenter::Segmenter,
    token_filter::{korean_keep_tags::KoreanKeepTagsTokenFilter, BoxTokenFilter},
    tokenizer::Tokenizer as LinderaInner,
};

pub trait Tokenizer: Send + Sync {
    fn tokenize(&self, text: &str) -> Vec<String>;

    fn tokenize_for_fts(&self, text: &str) -> String {
        self.tokenize(text).join(" ")
    }
}

// ─── LinderaKoTokenizer ───────────────────────────────────────────────────────

pub struct LinderaKoTokenizer {
    inner: LinderaInner,
}

impl LinderaKoTokenizer {
    pub fn new() -> Result<Self, String> {
        let dictionary = load_embedded_dictionary(DictionaryKind::KoDic)
            .map_err(|e| format!("lindera ko-dic load failed: {e}"))?;
        let segmenter = Segmenter::new(Mode::Normal, dictionary, None);
        let mut tokenizer = LinderaInner::new(segmenter);

        // Keep: NNG (일반명사), NNP (고유명사), NNB (의존명사), VV (동사), VA (형용사), SL (외국어)
        let tags: HashSet<String> = ["NNG", "NNP", "NNB", "VV", "VA", "SL"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let keep_filter = KoreanKeepTagsTokenFilter::new(tags);
        tokenizer.append_token_filter(BoxTokenFilter::from(keep_filter));

        Ok(Self { inner: tokenizer })
    }
}

impl Tokenizer for LinderaKoTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        let tokens = match self.inner.tokenize(text) {
            Ok(t) => t,
            Err(_) => return tokenize_fallback(text),
        };

        let mut result: Vec<String> = Vec::new();
        for token in tokens {
            let surface = token.surface.to_lowercase();
            if surface.chars().count() > 1 {
                result.push(surface);
            }
        }

        if result.is_empty() {
            tokenize_fallback(text)
        } else {
            result
        }
    }
}

// ─── SimpleTokenizer ──────────────────────────────────────────────────────────

/// 공백·ASCII구두점 기반 단순 폴백 토크나이저.
pub struct SimpleTokenizer;

impl Tokenizer for SimpleTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        tokenize_fallback(text)
    }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/// 기본 백엔드 상수: Kiwi 메인, lindera 폴백 의도를 코드에 명시.
pub const DEFAULT_BACKEND: &str = "kiwi";

/// 백엔드 이름으로 토크나이저를 생성한다. Task 1: lindera 경로만 활성.
/// (Task 2에서 kiwi 메인 분기 추가 예정)
pub fn create_tokenizer(_backend: &str) -> Result<Box<dyn Tokenizer>, String> {
    Ok(Box::new(LinderaKoTokenizer::new()?))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// 공백·ASCII구두점 분리, 소문자, 1글자 제외.
pub fn tokenize_fallback(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() > 1)
        .collect()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lindera_splits_josa_and_keeps_stem() {
        let tok = LinderaKoTokenizer::new().expect("lindera ko-dic load");
        let tokens = tok.tokenize("아키텍처를 설계한다");
        assert!(!tokens.is_empty());
        let joined = tokens.join(" ");
        assert!(
            joined.contains("아키텍처") || joined.contains("설계"),
            "조사 분리 실패: {joined:?}"
        );
    }

    #[test]
    fn lindera_keeps_foreign_sl_terms() {
        let tok = LinderaKoTokenizer::new().expect("load");
        let tokens = tok.tokenize("Rust workspace 검색");
        assert!(!tokens.join(" ").is_empty()); // 영어 용어가 SL로 살아남음
    }

    #[test]
    fn empty_and_special_no_panic() {
        let tok = LinderaKoTokenizer::new().expect("load");
        assert!(tok.tokenize("").is_empty());
        let _ = tok.tokenize("!@#$%^"); // 패닉만 없으면 됨
    }

    #[test]
    fn create_tokenizer_unknown_falls_back_to_lindera() {
        assert!(create_tokenizer("unknown").is_ok());
    }

    #[test]
    fn fallback_splits_and_filters() {
        let t = tokenize_fallback("hello,world ab");
        assert!(
            t.contains(&"hello".to_string())
                && t.contains(&"world".to_string())
                && t.contains(&"ab".to_string())
        );
    }
}
