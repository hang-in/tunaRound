// FTS 어휘 검색의 recall@k·MRR을 통제 코퍼스로 측정하는 결정적 하네스
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

/// 20개 발언 현실 코퍼스. 굴절·외래어·동의어를 의도적으로 흩어 리콜을 시험한다.
fn corpus() -> StoredSession {
    StoredSession {
        messages: vec![
            msg(1,  "claude", "인증 방식을 정하자. 세션 쿠키 대신 토큰 기반으로 가는 게 확장에 유리하다."),
            msg(2,  "codex",  "동의한다. 액세스 토큰은 짧게, 리프레시 토큰으로 갱신하는 구조가 안전하다."),
            msg(3,  "claude", "토큰을 어디 보관하지? 로컬스토리지는 XSS에 취약하니 httpOnly 쿠키를 쓰자."),
            msg(4,  "codex",  "로그인 흐름은 OAuth 위임으로 가면 비밀번호를 직접 안 다뤄도 된다."),
            msg(5,  "claude", "검색은 형태소 분석으로 색인하고 FTS5 전문검색으로 질의하자."),
            msg(6,  "codex",  "외래어가 문제다. 임베딩 같은 단어가 형태소 분석에서 음절로 쪼개진다."),
            msg(7,  "claude", "그러면 raw 토큰을 같이 색인해서 외래어를 원형으로 살리자."),
            msg(8,  "codex",  "의미 검색이 필요하면 벡터 임베딩을 붙여 하이브리드로 융합하면 된다."),
            msg(9,  "claude", "맥락이 길어지면 매 턴 전체를 다시 넣는 건 토큰 낭비다."),
            msg(10, "codex",  "최근 몇 턴만 유지하고 나머지는 검색으로 필요할 때 끌어오자."),
            msg(11, "claude", "요약을 만들어 라운드 사이에 이월하면 재전송을 더 줄일 수 있다."),
            msg(12, "codex",  "여러 프로세스가 한 토론을 공유하려면 Redis로 상태를 미러링하자."),
            msg(13, "claude", "스냅샷과 이벤트 스트림을 함께 두면 끊겨도 재개가 된다."),
            msg(14, "codex",  "참가자가 늘면 좌석마다 역할을 주입해서 발언을 시켜야 한다."),
            msg(15, "claude", "로컬 LLM도 좌석으로 받자. ollama나 lmstudio를 HTTP로 부르면 된다."),
            msg(16, "codex",  "코드 작성은 한 명에게만 쓰기 권한을 주고 나머지는 읽기만 하자."),
            msg(17, "claude", "결론이 나면 문서로 자동 기록해서 구현 단계로 넘기자."),
            msg(18, "codex",  "테스트와 빌드 검증은 커밋과 분리해서 단계로 두는 게 안전하다."),
            msg(19, "claude", "배포는 점진적으로. 한 번에 다 바꾸면 되돌리기 어렵다."),
            msg(20, "codex",  "관측을 위해 다른 프로세스가 토론을 구경만 하는 모드도 필요하다."),
        ],
        head: Some(20),
    }
}

/// recall@k = |retrieved ∩ gold| / |gold|
fn recall_at_k(retrieved: &[u64], gold: &[u64], k: usize) -> f64 {
    let top_k: Vec<u64> = retrieved.iter().take(k).copied().collect();
    let hits = gold.iter().filter(|g| top_k.contains(g)).count();
    hits as f64 / gold.len() as f64
}

/// MRR = 1/rank(첫 gold hit), 없으면 0
fn mrr(retrieved: &[u64], gold: &[u64]) -> f64 {
    for (i, id) in retrieved.iter().enumerate() {
        if gold.contains(id) {
            return 1.0 / (i as f64 + 1.0);
        }
    }
    0.0
}

#[test]
fn fts_recall_baseline() {
    let tok = create_tokenizer("lindera").expect("tokenizer");
    let store = SqliteStore::open_memory().expect("sqlite");
    let ss = corpus();
    store.save_session("q", &ss, |t| tok.fts_index(t)).expect("save_session");

    // (질의 텍스트, gold msg_id 집합, 종류 설명)
    let queries: &[(&str, &[u64], &str)] = &[
        ("토큰",               &[1, 2, 3],    "정확"),
        ("인증을",             &[1],          "굴절(조사)"),
        ("임베딩",             &[6, 8],       "외래어 정확"),
        ("외래어 색인",        &[6, 7],       "다중어(AND면 깨질 후보)"),
        ("로그인 방식",        &[1, 4],       "동의어(인증/OAuth)"),
        ("재주입을 줄이는 방법", &[9, 10, 11], "다중어+굴절"),
        ("프로세스 공유 재개", &[12, 13],     "다중어(AND면 0건 후보)"),
        ("로컬 LLM 좌석",      &[15],         "외래어+다중어"),
        ("쓰기 권한",          &[16],         "정확"),
        ("결론 문서 기록",     &[17],         "다중어"),
    ];

    const K3: usize = 3;
    const K5: usize = 5;

    let mut sum_r3 = 0.0f64;
    let mut sum_r5 = 0.0f64;
    let mut sum_mrr = 0.0f64;

    println!();
    println!("{:-<78}", "");
    println!("{:<22} {:>7} {:>7} {:>7}  retrieved_top5", "질의", "R@3", "R@5", "MRR");
    println!("{:-<78}", "");

    for (q, gold, kind) in queries {
        let fts_q = tok.fts_query(q);
        let hits = store.search(&fts_q, K5).unwrap_or_default();
        let retrieved: Vec<u64> = hits.iter().map(|h| h.msg_id).collect();

        let r3 = recall_at_k(&retrieved, gold, K3);
        let r5 = recall_at_k(&retrieved, gold, K5);
        let rr = mrr(&retrieved, gold);

        sum_r3 += r3;
        sum_r5 += r5;
        sum_mrr += rr;

        let ids_str = if retrieved.is_empty() {
            "(없음)".to_string()
        } else {
            retrieved.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        };

        println!(
            "{:<22} {:>7.3} {:>7.3} {:>7.3}  [{}]  ({})",
            q, r3, r5, rr, ids_str, kind
        );
    }

    let n = queries.len() as f64;
    let mean_r3  = sum_r3  / n;
    let mean_r5  = sum_r5  / n;
    let mean_mrr = sum_mrr / n;

    println!("{:-<78}", "");
    println!(
        "{:<22} {:>7.3} {:>7.3} {:>7.3}  (mean, n={})",
        "MEAN", mean_r3, mean_r5, mean_mrr, queries.len()
    );
    println!("{:-<78}", "");
    println!();

    // 회귀 가드: OR 개선 후 측정값(mean R@5=0.900)을 floor로. lindera 결정적 경로.
    assert!(mean_r5 >= 0.89, "mean recall@5 회귀: {mean_r5:.3} < 0.89 (OR 개선 전 0.55)");
}
