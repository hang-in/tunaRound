// 검색 레이어: 한국어 형태소 토크나이저(추후 FTS·벡터·하이브리드).

/// 공백·ASCII구두점 분리, 소문자, 1글자 제외.
pub fn tokenize_fallback(text: &str) -> Vec<String> {
    text.split(|c: char| c.is_whitespace() || c.is_ascii_punctuation())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .filter(|s| s.chars().count() > 1)
        .collect()
}

/// 개발·설계 외래어 음역 병기 그룹. 한 그룹 안 토큰은 서로 alias(양방향). 소문자·형태소 표면형 기준.
/// 음역(한글 외래어 ↔ 라틴 원어)에 한정한다: 의역/번역(검색↔search)은 임베딩이 담당하고, 여기 넣으면
/// noise가 커진다. 모호 단음절(풀=pull/pool/grass, 락=lock/rock, 큐=queue/cue)은 오탐 위험이라 제외.
/// FTS 질의 확장 전용(index 무변경). 실측 갭(리프레시↔refresh)이 착수 근거.
const LOANWORD_GROUPS: &[&[&str]] = &[
    &["refresh", "리프레시"],
    &["token", "토큰"],
    &["embedding", "임베딩"],
    &["cache", "캐시"],
    &["index", "인덱스"],
    &["commit", "커밋"],
    &["session", "세션"],
    &["branch", "브랜치"],
    &["sandbox", "샌드박스"],
    &["prompt", "프롬프트"],
    &["vector", "벡터"],
    &["rerank", "리랭크", "리랭커"],
    &["bearer", "베어러"],
    &["redis", "레디스"],
    &["cosine", "코사인"],
    &["snapshot", "스냅샷"],
    &["rollback", "롤백"],
    &["endpoint", "엔드포인트"],
    &["timeout", "타임아웃"],
    &["thread", "스레드"],
    &["mutex", "뮤텍스"],
    &["buffer", "버퍼"],
    &["schema", "스키마"],
    &["migration", "마이그레이션"],
    &["roster", "로스터"],
    &["pointer", "포인터"],
    &["context", "컨텍스트"],
    &["tokenizer", "토크나이저"],
    &["cursor", "커서"],
    &["keychain", "키체인"],
    &["cookie", "쿠키"],
    &["rotation", "로테이션"],
];

/// 토큰의 외래어 음역 alias들을 반환한다(자기 자신 제외). 소문자 표면형 기준, 없으면 빈 Vec.
/// FTS 질의에서 리프레시→refresh처럼 교차스크립트 병기 확장에 쓴다(임베딩이 못 잇는 갭 보강).
pub fn loanword_aliases(token: &str) -> Vec<String> {
    for group in LOANWORD_GROUPS {
        if group.contains(&token) {
            return group.iter().filter(|&&t| t != token).map(|s| s.to_string()).collect();
        }
    }
    Vec::new()
}

/// 토큰 리스트를 FTS5 질의 문자열로 조립하는 단일 규칙(정상·폴백 경로 공용 출처).
/// 규칙: 원 토큰 기준 음역 alias를 사후 추가(확장 토큰 재확장 방지) → sort → dedup →
/// 각 토큰에 prefix `*` → ` OR ` 결합. 빈 입력은 빈 문자열.
/// `Tokenizer::fts_query`와 폴백 경로(`fallback_fts_query`)가 모두 이 함수에 위임한다.
pub fn assemble_fts_query(tokens: Vec<String>) -> String {
    let mut toks = tokens;
    let aliases: Vec<String> = toks.iter().flat_map(|t| loanword_aliases(t)).collect();
    toks.extend(aliases);
    toks.sort();
    toks.dedup();
    toks.into_iter().map(|t| format!("{t}*")).collect::<Vec<_>>().join(" OR ")
}

/// 폴백 질의용 FTS5 표현: raw 토큰(`tokenize_fallback`) 기반으로 `assemble_fts_query`에 위임.
/// morphology 토크나이저 실패/미탑재 시 정상 질의 경로와 동일한 alias·OR 규칙을 낸다.
pub fn fallback_fts_query(text: &str) -> String {
    assemble_fts_query(tokenize_fallback(text))
}

/// 폴백 색인용 FTS 텍스트: raw 토큰(`tokenize_fallback`) 공백 join.
pub fn fallback_fts_index(text: &str) -> String {
    tokenize_fallback(text).join(" ")
}

#[cfg(feature = "morphology")]
pub mod tokenizer;

#[cfg(test)]
mod tests {
    use super::loanword_aliases;

    #[test]
    fn aliases_are_bidirectional() {
        assert_eq!(loanword_aliases("리프레시"), vec!["refresh".to_string()]);
        assert_eq!(loanword_aliases("refresh"), vec!["리프레시".to_string()]);
    }

    #[test]
    fn multi_member_group_returns_all_others() {
        let mut a = loanword_aliases("rerank");
        a.sort();
        assert_eq!(a, vec!["리랭커".to_string(), "리랭크".to_string()]);
    }

    #[test]
    fn unknown_token_has_no_alias() {
        assert!(loanword_aliases("데이터베이스").is_empty());
        assert!(loanword_aliases("검색").is_empty()); // 번역은 제외(임베딩 담당)
    }

    #[test]
    fn assemble_expands_alias_and_joins_with_or() {
        let q = super::assemble_fts_query(vec!["임베딩".to_string()]);
        assert!(q.contains("임베딩*"), "원 토큰 prefix 누락: {q}");
        assert!(q.contains("embedding*"), "alias 확장 누락: {q}");
        assert!(q.contains(" OR "), "OR 결합 아님: {q}");
    }

    #[test]
    fn assemble_empty_is_empty_string() {
        assert_eq!(super::assemble_fts_query(vec![]), "");
    }

    #[test]
    fn fallback_query_uses_alias_and_or_rule() {
        // 폴백 질의 경로가 alias 확장 + OR 결합을 정상 경로와 동일하게 낸다.
        let q = super::fallback_fts_query("임베딩");
        assert!(q.contains("임베딩*") && q.contains("embedding*"), "폴백 alias 확장 실패: {q}");
        assert!(q.contains(" OR "), "폴백 OR 결합 실패: {q}");
        // 공백 join(AND) 회귀 방지: 순수 공백 구분 토큰이 남으면 안 된다(전부 OR로 연결).
        assert!(!q.split(" OR ").any(|seg| seg.contains(' ')), "OR 밖 공백 잔존: {q}");
    }

    #[test]
    fn fallback_index_is_space_joined() {
        assert_eq!(super::fallback_fts_index("hello world"), "hello world");
    }

    // morphology 트레이트 질의 경로와 폴백 자유 함수가 같은 질의 규칙을 내는지 동치 검증.
    // (색인 경로는 트레이트 fts_index가 형태소+raw 이중 join이라 단일-copy 폴백과 의도적으로 다르며,
    //  질의 경로는 sort+dedup이 이중 토큰을 접어 동일해진다.)
    #[cfg(feature = "morphology")]
    #[test]
    fn simple_tokenizer_matches_fallback_query() {
        use super::tokenizer::{SimpleTokenizer, Tokenizer};
        let tok = SimpleTokenizer;
        for input in ["임베딩", "refresh 토큰", "hello world ab"] {
            assert_eq!(
                tok.fts_query(input),
                super::fallback_fts_query(input),
                "트레이트↔폴백 질의 규칙 불일치: {input}"
            );
        }
    }
}
