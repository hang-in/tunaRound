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
            msg(21, "claude", "분기를 나눠 두 방향을 따로 토론하고 나중에 합치자."),
            msg(22, "codex",  "각 가지의 결론만 모아 비교하면 결정이 빠르다."),
            msg(23, "claude", "세션이 끊겨도 상태 파일에서 다시 불러오면 이어서 한다."),
            msg(24, "codex",  "토론이 멈춘 자리를 기억했다가 그 지점부터 복원한다."),
            msg(25, "claude", "한 에이전트가 너무 오래 응답이 없으면 타임아웃으로 끊어야 한다."),
            msg(26, "codex",  "멈춘 좌석은 건너뛰고 다음 발언자로 넘어가자."),
            msg(27, "claude", "형태소 분석기는 Kiwi를 메인으로 쓰고 안 되면 lindera로 떨어뜨린다."),
            msg(28, "codex",  "조사를 떼고 어간만 남겨야 굴절된 질의가 맞는다."),
            msg(29, "claude", "코드를 직접 고칠 땐 샌드박스를 쓰기 가능으로 열어 준다."),
            msg(30, "codex",  "읽기 전용 화자는 레포를 건드리지 못하게 막는다."),
            msg(31, "claude", "출시 전에 테스트와 빌드를 반드시 통과시키자."),
            msg(32, "codex",  "한 번에 다 바꾸지 말고 작은 단위로 점진 배포하자."),
            msg(33, "claude", "바깥에서 붙으려면 신원 확인과 권한 검사가 필요하다."),
            msg(34, "codex",  "개인 서버에 코어를 올려 두고 클라이언트가 접속하는 구조도 된다."),
            msg(35, "claude", "같은 공유기 안에서는 한 대가 호스트가 되어 나눠 쓴다."),
            msg(36, "codex",  "임베딩은 원격 서버에 맡기고 우리는 코사인만 계산한다."),
            msg(37, "claude", "검색 결과를 다시 정렬해 관련도가 높은 것을 위로 올리자."),
            msg(38, "codex",  "질의를 비슷한 말로 넓히면 표현이 달라도 잡힌다."),
            msg(39, "claude", "요점만 남기고 원문은 필요할 때 찾아서 펼친다."),
            msg(40, "codex",  "누가 다음에 말할지는 사람이 정하고 자동은 나중에 켠다."),
        ],
        head: Some(40),
    }
}

/// 평가 질의 셋. (질의, gold msg_id, 종류). *표 = FTS 어휘·의미공백이라 벡터·확장·리랭커 측정점.
/// FTS·벡터·하이브리드 측정이 같은 gold를 공유하도록 모듈 레벨에 둔다.
type EvalQuery = (&'static str, &'static [u64], &'static str);
const QUERIES: &[EvalQuery] = &[
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
    // 확장 셋(40발언). *표 = FTS 어휘공백/동의어라 리랭커·벡터·쿼리확장 가치 측정점.
    ("분기 비교",          &[21, 22],     "다중어"),
    ("세션 복원",          &[23, 24],     "*동의어(복원↔불러오기): 23 누락 후보"),
    ("응답 없는 좌석 타임아웃", &[25, 26], "다중어"),
    ("쓰기 권한 샌드박스", &[16, 29],     "다중어"),
    ("점진 출시",          &[19, 31, 32], "*동의어(출시↔배포)"),
    ("원격 접속 인증",     &[33, 34],     "*어휘공백: 33은 신원확인이라 누락 후보"),
    ("코어 백엔드 호스팅", &[34, 35],     "*어휘공백(백엔드/호스팅)"),
    ("관련도 순 재정렬",   &[37],         "*동의어(재정렬↔다시 정렬)"),
    ("동의어 질의 확장",   &[38],         "*어휘공백: 38은 '비슷한 말로 넓히면' 누락 후보"),
    ("다음 발언자 결정",   &[26, 40],     "다중어"),
    ("오래 기억하는 방법", &[9, 11, 39],  "*의미공백(기억 단어 부재): 벡터 측정점"),
];

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

/// precision@k = |retrieved_topk ∩ gold| / min(k, |retrieved|). 빈 결과는 0.
/// 분모를 실제 반환수로 잡아 "적게 반환"엔 불이익 없이 OR이 주입하는 noise만 잰다.
fn precision_at_k(retrieved: &[u64], gold: &[u64], k: usize) -> f64 {
    let denom = retrieved.len().min(k);
    if denom == 0 {
        return 0.0;
    }
    let hits = retrieved.iter().take(k).filter(|r| gold.contains(r)).count();
    hits as f64 / denom as f64
}

#[test]
fn fts_recall_baseline() {
    let tok = create_tokenizer("lindera").expect("tokenizer");
    let store = SqliteStore::open_memory().expect("sqlite");
    let ss = corpus();
    store.save_session("q", &ss, |t| tok.fts_index(t)).expect("save_session");

    // (질의 텍스트, gold msg_id 집합, 종류 설명)
    let queries = QUERIES;

    const K3: usize = 3;
    const K5: usize = 5;

    let mut sum_r3 = 0.0f64;
    let mut sum_r5 = 0.0f64;
    let mut sum_p3 = 0.0f64;
    let mut sum_p5 = 0.0f64;
    let mut sum_mrr = 0.0f64;

    println!();
    println!("{:-<86}", "");
    println!(
        "{:<20} {:>6} {:>6} {:>6} {:>6} {:>6}  top5",
        "질의", "R@3", "R@5", "P@3", "P@5", "MRR"
    );
    println!("{:-<86}", "");

    for (q, gold, kind) in queries {
        let fts_q = tok.fts_query(q);
        let hits = store.search(&fts_q, K5).unwrap_or_default();
        let retrieved: Vec<u64> = hits.iter().map(|h| h.msg_id).collect();

        let r3 = recall_at_k(&retrieved, gold, K3);
        let r5 = recall_at_k(&retrieved, gold, K5);
        let p3 = precision_at_k(&retrieved, gold, K3);
        let p5 = precision_at_k(&retrieved, gold, K5);
        let rr = mrr(&retrieved, gold);

        sum_r3 += r3;
        sum_r5 += r5;
        sum_p3 += p3;
        sum_p5 += p5;
        sum_mrr += rr;

        let ids_str = if retrieved.is_empty() {
            "(없음)".to_string()
        } else {
            retrieved.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(",")
        };

        println!(
            "{:<20} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  [{}] ({})",
            q, r3, r5, p3, p5, rr, ids_str, kind
        );
    }

    let n = queries.len() as f64;
    let mean_r3  = sum_r3  / n;
    let mean_r5  = sum_r5  / n;
    let mean_p3  = sum_p3  / n;
    let mean_p5  = sum_p5  / n;
    let mean_mrr = sum_mrr / n;

    println!("{:-<86}", "");
    println!(
        "{:<20} {:>6.3} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  (mean, n={})",
        "MEAN", mean_r3, mean_r5, mean_p3, mean_p5, mean_mrr, queries.len()
    );
    println!("{:-<86}", "");
    println!();

    // 회귀 가드(양면): OR 개선 후 측정값을 floor로. lindera 결정적 경로.
    // recall floor = OR이 리콜을 유지하는지, precision floor = OR noise가 과하지 않은지.
    // 확장 셋(21질의/40발언) 측정값 floor. 어휘·의미공백 질의 포함이라 easy셋(0.90)보다 낮음.
    assert!(mean_r5 >= 0.85, "mean recall@5 회귀: {mean_r5:.3} < 0.85 (확장셋 baseline 0.857)");
    assert!(mean_p5 >= 0.58, "mean precision@5 회귀: {mean_p5:.3} < 0.58 (확장셋 baseline 0.592)");
}

/// 벡터·하이브리드 리콜 측정(수동, semantic+Ollama 터널). FTS가 못 메운 어휘·의미공백 질의
/// (*표 Q들)를 벡터/하이브리드가 회복하는지 같은 gold로 잰다.
#[cfg(feature = "semantic")]
#[test]
#[ignore] // 수동: Ollama 터널(11435). cargo test --features "semantic morphology" --test search_recall -- --ignored --nocapture
fn vector_hybrid_recall() {
    use tunaround::orchestrator::ContextRetriever;
    use tunaround::store::embedding::{Embedder, OllamaEmbedder};
    use tunaround::store::retriever::SqliteRetriever;

    const ENDPOINT: &str = "http://127.0.0.1:11435";
    let path = std::env::temp_dir().join("tuna_recall_sem.db");
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();

    let tok = create_tokenizer("lindera").expect("tokenizer");
    let store_w = SqliteStore::open(p).unwrap();
    let ss = corpus();
    store_w.save_session("q", &ss, |t| tok.fts_index(t)).unwrap();
    let embedder = OllamaEmbedder::new(ENDPOINT, "bge-m3");
    store_w.index_vectors("q", &ss, &embedder).expect("index vectors (터널 확인)");

    // 하이브리드 결과는 Utterance라 id가 없어 content->id 역매핑.
    let id_of = |content: &str| -> Option<u64> {
        ss.messages.iter().find(|m| m.content == content).map(|m| m.id)
    };

    let store_r = SqliteStore::open(p).unwrap();
    let tok2 = create_tokenizer("lindera").unwrap();
    let retriever = SqliteRetriever::new(
        SqliteStore::open(p).unwrap(),
        Box::new(move |t: &str| tok2.fts_query(t)),
        Some(Box::new(OllamaEmbedder::new(ENDPOINT, "bge-m3"))),
    );

    const K: usize = 5;
    let (mut sv_r, mut sv_m, mut sh_r, mut sh_m) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);

    println!();
    println!("{:-<92}", "");
    println!("{:<22} {:>6} {:>6} {:>6} {:>6}  종류", "질의", "vecR", "vecM", "hybR", "hybM");
    println!("{:-<92}", "");
    for (q, gold, kind) in QUERIES {
        let qv = embedder.embed(q).expect("embed");
        let vec_ids: Vec<u64> = store_r
            .vector_search(&qv, K)
            .unwrap_or_default()
            .iter()
            .map(|(_, mid, _)| *mid)
            .collect();
        let hyb_ids: Vec<u64> = retriever
            .retrieve(q, K)
            .iter()
            .filter_map(|u| id_of(&u.content))
            .collect();
        let (vr, vm) = (recall_at_k(&vec_ids, gold, K), mrr(&vec_ids, gold));
        let (hr, hm) = (recall_at_k(&hyb_ids, gold, K), mrr(&hyb_ids, gold));
        sv_r += vr;
        sv_m += vm;
        sh_r += hr;
        sh_m += hm;
        println!("{q:<22} {vr:>6.3} {vm:>6.3} {hr:>6.3} {hm:>6.3}  {kind}");
    }
    let n = QUERIES.len() as f64;
    println!("{:-<92}", "");
    println!(
        "{:<22} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  (mean, n={})",
        "MEAN", sv_r / n, sv_m / n, sh_r / n, sh_m / n, QUERIES.len()
    );
    println!("{:-<92}", "");
    let _ = std::fs::remove_file(&path);
}
