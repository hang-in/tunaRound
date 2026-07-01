// 실코퍼스 검색 회귀(step 6): seCall의 실제 tunaRound 설계 토론 턴(2026-06-30~07-01, 여러 세션)에서
// 발췌한 23발언을 코퍼스로, 굴절·동의어로 바꾼 15질의의 recall@k·MRR·precision@k를 결정적으로 잰다.
// 검색-인프라 도메인 + auth/보안 도메인(리프레시 토큰 논쟁, 별개 세션)을 섞어 대표성을 넓혔다.
// 합성 코퍼스(search_recall.rs, R@5 0.90)의 대표성 한계를 실데이터로 보완한다.
#![cfg(feature = "morphology")]

use tunaround::search::tokenizer::create_tokenizer;
use tunaround::store::sqlite::SqliteStore;
use tunaround::store::{StoredMessage, StoredSession};

fn msg(id: u64, speaker: &str, content: &str) -> StoredMessage {
    StoredMessage {
        id,
        parent_id: if id == 1 { None } else { Some(id - 1) },
        speaker: speaker.into(),
        content: content.into(),
    }
}

/// 실코퍼스: seCall project=tunaRound의 실제 턴에서 발췌(출처 주석=session:turn). 원문 충실, 1발언=1논점.
fn corpus() -> StoredSession {
    StoredSession {
        messages: vec![
            // 6274470d(06-30 아키텍처 리뷰):175
            msg(1, "opus", "FTS5의 unicode61 토크나이저는 한국어를 형태소로 못 쪼개므로, Kiwi나 lindera로 미리 형태소 분해해 공백으로 조인한 텍스트를 색인한다."),
            msg(2, "opus", "형태소 토큰에 더해 raw 토큰까지 같이 색인해서 외래어 임베딩이 조사째 원형으로 남도록 한다."),
            msg(3, "opus", "임베딩 모델을 bge-m3에서 다른 것으로 바꿔도 content_hash가 같아 증분 가드가 오래된 벡터를 건너뛴다. 해시에 모델 id와 차원을 넣어야 한다."),
            msg(4, "opus", "검색이 전역이라 세션이나 시간 필터가 없어 옛 결정이 최신 결정처럼 프롬프트에 주입될 수 있다."),
            msg(5, "opus", "버려진 분기의 발언이 검색으로 끌려와 현재 분기 프롬프트에 섞일 수 있다."),
            msg(6, "opus", "FTS 키 리스트와 벡터 키 리스트를 reciprocal rank fusion으로 융합한다. k는 60이다."),
            msg(7, "opus", "recall at k와 MRR과 precision at k를 재는 회귀 게이트 벤치마크가 코드로 있어 검색 품질 저하를 막는다."),
            msg(8, "opus", "retrieved 주입은 개수만 제한하고 글자수 상한이 없어 긴 발언 다섯 개면 프롬프트가 팽창한다."),
            msg(9, "opus", "pull 모드 계측에서 프롬프트 길이가 전사 길이와 탈동조돼 claude가 9770자에서 429자로 줄었다."),
            msg(10, "opus", "KiwiWrapper의 unsafe impl Send는 C 포인터를 Mutex 직렬화에 의존하는 잠재 위험이다."),
            // 37b034cb(06-30 캐시 무효화):2
            msg(11, "opus", "임베딩과 형태소 토큰화는 콘텐츠 주소 방식이라 해시에 model id를 넣으면 무효화 자체가 필요 없다."),
            msg(12, "opus", "진짜 무효화가 필요한 건 파생된 쿼리 결과뿐이라 인덱스 세대 스탬프로 지연 무효화한다."),
            msg(13, "opus", "세대 입도를 세션이 아니라 인덱스 단위로 두면 무관한 세션의 캐시가 살아남는다."),
            // 6274470d:89
            msg(14, "opus", "단일 프로세스가 HTTP MCP 코어를 바인드하고 REPL을 동시에 구동하며 bearer 인증이 401과 200으로 동작한다."),
            // dff85fb8(07-01):334/352/159/186
            msg(15, "opus", "codex exec는 read-only 샌드박스를 유지한 채로는 MCP 도구 호출을 자동 승인할 방법이 없다."),
            msg(16, "opus", "codex는 규칙 준수가 강해서 read-only를 샌드박스가 아니라 프롬프트 지시로 강제할 수 있다."),
            msg(17, "opus", "recency 랭킹은 messages에 created_at 컬럼을 더하고 다른 세션의 오래된 후보만 소폭 강등한다."),
            msg(18, "opus", "debug_retrieve 디버그 출력에 created_at과 recency 강등 표시를 더해 눈으로 확인할 수 있게 했다."),
            // e5a848d3(06-30 설계토론: 리프레시 토큰 회전):8 - 다른 세션·다른 도메인(auth/보안)
            msg(19, "opus", "리프레시 토큰 회전이 막아주는 탈취 탐지는 리프레시 토큰이 브라우저 JS로 읽히는 localStorage에 장기 보관될 때만 의미가 있다."),
            msg(20, "opus", "access는 메모리 전용, refresh는 OS keychain, SPA는 httpOnly 쿠키로 두면 회전이 막는 공격면이 설계상 닫힌다."),
            msg(21, "opus", "opaque refresh에 jti denylist와 token_version을 두면 로그아웃이나 유출 의심 시 토큰 패밀리를 한 번에 폐기할 수 있다."),
            msg(22, "opus", "짧은 access TTL과 refresh의 절대 만료와 idle 만료를 함께 적용하면 회전 없이 위협모델을 덮는다."),
            msg(23, "opus", "리프레시 토큰이 브라우저 JS가 닿는 저장소로 가는 순간 reuse-detection과 함께 토큰 회전을 도입한다."),
        ],
        head: Some(23),
    }
}

/// 평가 질의: 코퍼스 원문과 다른 표현(굴절·동의어)로 바꿔 형태소 검색의 실난이도를 잰다.
type EvalQuery = (&'static str, &'static [u64], &'static str);
const QUERIES: &[EvalQuery] = &[
    ("한국어 형태소 색인",     &[1, 2],       "다중어"),
    ("모델 바꾸면 재색인",     &[3, 11],      "*동의어(재색인↔무효화)"),
    ("오래된 결정이 최신처럼 섞임", &[4, 17],  "*동의어(강등↔필터)"),
    ("버려진 분기 발언",       &[5],          "정확"),
    ("BM25와 벡터 융합",       &[6],          "외래어+정확"),
    ("검색 품질 회귀 방지",    &[7],          "다중어"),
    ("프롬프트 팽창 제한",     &[8],          "다중어"),
    ("풀 모드 토큰 절감",      &[9],          "*외래어(풀↔pull)"),
    ("codex 도구 승인 막힘",   &[15, 16],     "외래어+다중어"),
    ("캐시 무효화 전략",       &[11, 12, 13], "다중어"),
    ("원격 코어 인증",         &[14],         "*어휘공백(bearer)"),
    ("검색 디버그 창구",       &[18],         "*동의어(창구↔출력)"),
    ("토큰 회전 필요한가",     &[19, 22, 23], "auth 도메인"),
    ("리프레시 토큰 어디 저장", &[20],        "auth+외래어"),
    ("토큰 즉시 폐기 방법",    &[21],         "*동의어(폐기↔denylist)"),
];

fn recall_at_k(retrieved: &[u64], gold: &[u64], k: usize) -> f64 {
    let top_k: Vec<u64> = retrieved.iter().take(k).copied().collect();
    let hits = gold.iter().filter(|g| top_k.contains(g)).count();
    hits as f64 / gold.len() as f64
}

fn mrr(retrieved: &[u64], gold: &[u64]) -> f64 {
    for (i, id) in retrieved.iter().enumerate() {
        if gold.contains(id) {
            return 1.0 / (i as f64 + 1.0);
        }
    }
    0.0
}

fn precision_at_k(retrieved: &[u64], gold: &[u64], k: usize) -> f64 {
    let denom = retrieved.len().min(k);
    if denom == 0 {
        return 0.0;
    }
    let hits = retrieved.iter().take(k).filter(|r| gold.contains(r)).count();
    hits as f64 / denom as f64
}

#[test]
fn real_corpus_fts_recall() {
    let tok = create_tokenizer("lindera").expect("tokenizer");
    let store = SqliteStore::open_memory().expect("sqlite");
    let ss = corpus();
    store.save_session("real", &ss, |t| tok.fts_index(t)).expect("save_session");

    const K3: usize = 3;
    const K5: usize = 5;
    let (mut sr3, mut sr5, mut sp3, mut sp5, mut sm) = (0.0f64, 0.0, 0.0, 0.0, 0.0);

    println!();
    println!("{:-<94}", "");
    println!("{:<26} {:>6} {:>6} {:>6} {:>6} {:>6}  top5", "질의", "R@3", "R@5", "P@3", "P@5", "MRR");
    println!("{:-<94}", "");
    for (q, gold, kind) in QUERIES {
        let fts_q = tok.fts_query(q);
        let hits = store.search(&fts_q, K5).unwrap_or_default();
        let retrieved: Vec<u64> = hits.iter().map(|h| h.msg_id).collect();
        let (r3, r5) = (recall_at_k(&retrieved, gold, K3), recall_at_k(&retrieved, gold, K5));
        let (p3, p5) = (precision_at_k(&retrieved, gold, K3), precision_at_k(&retrieved, gold, K5));
        let rr = mrr(&retrieved, gold);
        sr3 += r3; sr5 += r5; sp3 += p3; sp5 += p5; sm += rr;
        let ids = if retrieved.is_empty() { "(없음)".into() } else { retrieved.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",") };
        println!("{q:<26} {r3:>6.3} {r5:>6.3} {p3:>6.3} {p5:>6.3} {rr:>6.3}  [{ids}] ({kind})");
    }
    let n = QUERIES.len() as f64;
    let (mr3, mr5, mp3, mp5, mm) = (sr3 / n, sr5 / n, sp3 / n, sp5 / n, sm / n);
    println!("{:-<94}", "");
    println!("{:<26} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  (mean, n={})", "MEAN", mr3, mr5, mp3, mp5, mm, QUERIES.len());
    println!("{:-<94}", "");
    println!();

    // 실코퍼스 baseline floor(측정값 R@5 0.878 / P@5 0.494, lindera 결정적). 여유를 둔 회귀 가드.
    // ⚠ 실발견: "리프레시 토큰 어디 저장"(gold 20) R@5 0.0 = 한국어 외래어 "리프레시"와 영어 "refresh"
    //   음역 갭을 FTS 형태소가 못 이음(외래어 표기 정규화 미구현). auth 질의가 검색-인프라 발언을
    //   distractor로 끌어 P 하락. 쉬운 코퍼스(0.958)가 숨긴 실난이도 → 확장 코퍼스가 노출.
    assert!(mr5 >= 0.80, "mean recall@5 회귀: {mr5:.3} < 0.80 (실코퍼스 baseline 0.878)");
    assert!(mp5 >= 0.42, "mean precision@5 회귀: {mp5:.3} < 0.42 (실코퍼스 baseline 0.494)");
}
