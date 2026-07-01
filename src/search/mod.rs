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
}
