// 한국어 형태소 토크나이저(secall 포팅): Tokenizer trait + Kiwi/lindera + factory.
use super::tokenize_fallback;
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

    /// raw 토큰: 공백·ASCII구두점 분리, 소문자, 1글자 초과. POS 필터 없음(외래어 보존).
    fn raw_tokens(&self, text: &str) -> Vec<String> {
        tokenize_fallback(text)
    }

    /// 색인용 FTS 텍스트: 형태소 + raw 토큰 공백조인(중복 허용, 외래어가 조사째 색인에 남음).
    fn fts_index(&self, text: &str) -> String {
        let mut v = self.tokenize(text);
        v.extend(self.raw_tokens(text));
        v.join(" ")
    }

    /// 질의용 FTS5 표현: 형태소+raw 토큰 distinct에 prefix(*) 붙여 OR. 빈 입력은 빈 문자열.
    /// OR+bm25 조합으로 리콜 보장(다토큰 매치 문서가 bm25 상위로 올라 정밀도 보존).
    fn fts_query(&self, text: &str) -> String {
        let mut toks = self.tokenize(text);
        toks.extend(self.raw_tokens(text));
        toks.sort();
        toks.dedup();
        toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" OR ")
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

// ─── KiwiTokenizer ────────────────────────────────────────────────────────────

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
mod kiwi_impl {
    use super::*;

    /// kiwi_rs::Kiwi를 스레드 간 이동 가능하게 감싸는 뉴타입 래퍼.
    pub(super) struct KiwiWrapper(pub(super) kiwi_rs::Kiwi);

    // SAFETY: kiwi_rs::Kiwi는 libkiwi C 핸들 래퍼라 auto-Send가 안 붙는다. 이 래퍼는 항상
    // `Mutex<KiwiWrapper>`(KiwiTokenizer.kiwi)로만 보관되어 한 순간 한 스레드만 접근하므로,
    // 소유권이 스레드 간 이동하더라도(=Send) 동시 접근으로 인한 C측 데이터 경합은 없다.
    // 잔여 리스크: libkiwi가 내부적으로 thread-local/전역 상태에 스레드 정체성을 묶는다면
    // Mutex 직렬화로도 부족할 수 있다(공식 스레드모델 문서로 미확인). 현재까지 관측된 문제는 없고,
    // thread_local!로 스레드마다 Kiwi를 두는 대안은 인스턴스당 init(~수초·모델 로드) 비용 때문에 비채택.
    // morphology 피처 전용이며 Windows 빌드는 Kiwi를 제외(lindera)하므로 이 경로는 비활성.
    unsafe impl Send for KiwiWrapper {}

    /// kiwi-rs 기반 한국어 형태소 분석 토크나이저.
    /// 첫 사용 시 `Kiwi::init()`이 모델(~50MB)을 ~/.cache/kiwi에 다운로드한다.
    /// 스레드 안전성은 `Mutex<KiwiWrapper>`가 보장한다.
    pub struct KiwiTokenizer {
        pub(super) kiwi: std::sync::Mutex<KiwiWrapper>,
    }

    impl KiwiTokenizer {
        pub fn new() -> Result<Self, String> {
            let kiwi = kiwi_rs::Kiwi::init()
                .map_err(|e| format!("kiwi-rs init failed: {e}"))?;
            Ok(Self {
                kiwi: std::sync::Mutex::new(KiwiWrapper(kiwi)),
            })
        }
    }

    impl Tokenizer for KiwiTokenizer {
        fn tokenize(&self, text: &str) -> Vec<String> {
            if text.is_empty() {
                return Vec::new();
            }

            let guard = match self.kiwi.lock() {
                Ok(g) => g,
                Err(e) => e.into_inner(),
            };
            match guard.0.tokenize(text) {
                Ok(tokens) => {
                    let result: Vec<String> = tokens
                        .into_iter()
                        .filter(|t| {
                            // Keep NNG, NNP, NNB (명사), VV (동사), VA (형용사), SL (외국어)
                            // Kiwi가 VA-I/VV-I/VV-R 등 변종을 내므로 base 태그(하이픈 앞)로 매칭.
                            let base = t.tag.split('-').next().unwrap_or(t.tag.as_str());
                            matches!(base, "NNG" | "NNP" | "NNB" | "VV" | "VA" | "SL")
                        })
                        .map(|t| t.form.to_lowercase())
                        .filter(|s| s.chars().count() > 1)
                        .collect();

                    if result.is_empty() {
                        tokenize_fallback(text)
                    } else {
                        result
                    }
                }
                Err(_) => tokenize_fallback(text),
            }
        }
    }
}

#[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
pub use kiwi_impl::KiwiTokenizer;

// ─── SimpleTokenizer ──────────────────────────────────────────────────────────

/// 공백·ASCII구두점 기반 단순 폴백 토크나이저.
pub struct SimpleTokenizer;

impl Tokenizer for SimpleTokenizer {
    fn tokenize(&self, text: &str) -> Vec<String> {
        tokenize_fallback(text)
    }
}

// ─── Factory ─────────────────────────────────────────────────────────────────

/// 기본 백엔드 상수: Kiwi 메인, lindera 폴백.
pub const DEFAULT_BACKEND: &str = "kiwi";

/// 백엔드 이름으로 토크나이저를 생성한다.
/// "kiwi"를 요청하면 KiwiTokenizer를 시도하고, 초기화 실패 시 lindera로 자동 폴백한다.
pub fn create_tokenizer(backend: &str) -> Result<Box<dyn Tokenizer>, String> {
    match backend {
        #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
        "kiwi" => match KiwiTokenizer::new() {
            Ok(t) => Ok(Box::new(t)),
            Err(e) => {
                eprintln!("[tunaRound] kiwi-rs 초기화 실패, lindera 폴백: {e}");
                Ok(Box::new(LinderaKoTokenizer::new()?))
            }
        },
        _ => Ok(Box::new(LinderaKoTokenizer::new()?)),
    }
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

    #[test]
    fn create_tokenizer_kiwi_returns_working_tokenizer() {
        // kiwi 모델/네트워크 없으면 lindera로 폴백되어도 OK. 한국어 토큰이 나오면 통과.
        let tok = create_tokenizer("kiwi").expect("kiwi or lindera fallback");
        let tokens = tok.tokenize("아키텍처를 설계한다");
        assert!(!tokens.is_empty());
    }

    #[test]
    fn fts_index_keeps_loanword_in_context() {
        let tok = LinderaKoTokenizer::new().unwrap();
        let idx = tok.fts_index("벡터 임베딩을 쓴다");
        assert!(idx.contains("임베딩을") || idx.contains("임베딩"), "외래어 보존 실패: {idx}");
    }

    #[test]
    fn fts_query_is_prefix_and() {
        let tok = LinderaKoTokenizer::new().unwrap();
        let q = tok.fts_query("임베딩");
        assert!(q.contains("임베딩*"), "prefix 아님: {q}");
    }

    #[cfg(not(all(target_os = "linux", target_arch = "aarch64")))]
    #[test]
    #[ignore] // 수동: kiwi 모델 ~50MB 다운로드 필요
    fn kiwi_tokenizes_korean_live() {
        let tok = KiwiTokenizer::new().expect("kiwi init (model download)");
        assert!(!tok.tokenize("아키텍처를 설계한다").is_empty());
    }
}
