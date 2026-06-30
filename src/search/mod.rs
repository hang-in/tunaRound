// 검색 레이어: 한국어 형태소 토크나이저(추후 FTS·벡터·하이브리드).

/// 공백·ASCII구두점 분리, 소문자, 1글자 제외.
pub fn tokenize_fallback(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() > 1)
        .collect()
}

#[cfg(feature = "morphology")]
pub mod tokenizer;
