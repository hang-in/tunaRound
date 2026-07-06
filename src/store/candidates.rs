// 발견 리포터가 보고하는 미무장 세션 후보(candidate)의 데이터 모델과 순수 함수.

use serde::{Deserialize, Serialize};

/// candidate가 fresh(집계 포함)로 간주되는 reported_at 최대 경과 초. 리포터가 이보다 오래 보고
/// 없으면 stale로 후보에서 제외한다(리포터/세션이 죽으면 자연 소멸). roster online TTL보다 넉넉하게 둔다.
pub const CANDIDATE_TTL_SECS: i64 = 180;

/// 발견된(미무장) 세션 후보 한 항목. 리포터가 로컬 세션을 열거해 브로커에 보고한다.
/// `armed`는 저장하지 않고(브로커가 조회 시 online roster 소속 여부로 overlay 계산), reported_at은
/// 브로커 수신 시각으로 fresh 판정 기준이다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CandidateEntry {
    /// 세션 id(claude=jsonl 파일 stem). roster uuid와 같은 공간이라 armed overlay가 가능하다.
    pub uuid: String,
    /// 러너 종류(claude | codex | ...).
    pub runner: String,
    /// 추정 프로젝트(발견 출처의 경로/맥락 유래). 불명이면 None.
    pub project: Option<String>,
    /// 리포터 머신(win|mac|unix). 크로스머신 발견 시 어느 머신의 세션인지 구분한다. 불명이면 None.
    pub machine: Option<String>,
    /// 발견 출처(예: claude-jsonl). 어떤 열거 경로로 찾았는지.
    pub source: String,
    /// 보고 시점 세션 활동 경과 초(claude=jsonl mtime 유래). 신선도 표시용.
    pub age_secs: i64,
    /// 브로커 수신 시각(SQL datetime('now') 포맷). fresh 판정 기준.
    pub reported_at: String,
}

/// reported_at이 now 기준 ttl_secs 이내면 fresh(집계 포함). age_secs 파싱 실패(포맷 불량)는
/// 검증 불가로 간주해 보수적으로 stale 처리한다(is_online과 같은 규율).
pub fn is_fresh(reported_at: &str, now: &str, ttl_secs: i64) -> bool {
    match crate::store::a2a::age_secs(now, reported_at) {
        Some(age) => age <= ttl_secs,
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_fresh_within_ttl_is_true() {
        assert!(is_fresh("2026-07-06 09:58:00", "2026-07-06 10:00:00", 180));
    }

    #[test]
    fn is_fresh_beyond_ttl_is_false() {
        assert!(!is_fresh("2026-07-06 09:56:00", "2026-07-06 10:00:00", 180));
    }

    #[test]
    fn is_fresh_at_boundary_is_true() {
        // 경과가 정확히 ttl_secs일 때 <=이므로 fresh.
        assert!(is_fresh("2026-07-06 09:57:00", "2026-07-06 10:00:00", 180));
    }

    #[test]
    fn is_fresh_parse_failure_is_false() {
        assert!(!is_fresh("bogus", "2026-07-06 10:00:00", 180));
    }
}
