// 임베딩 모델 비교(수동): bge-m3 vs qwen3-embedding:0.6b를 같은 실코퍼스에 색인해
// vec-only / hybrid의 recall@5·MRR을 나란히 잰다. Ollama 터널(11435) 필요.
// cargo test --features "semantic morphology" --test embed_model_compare -- --ignored --nocapture
#![cfg(all(feature = "morphology", feature = "semantic"))]

use tunaround::orchestrator::ContextRetriever;
use tunaround::search::tokenizer::create_tokenizer;
use tunaround::store::embedding::{Embedder, OllamaEmbedder};
use tunaround::store::retriever::SqliteRetriever;
use tunaround::store::sqlite::SqliteStore;
use tunaround::store::{StoredMessage, StoredSession};

const ENDPOINT: &str = "http://127.0.0.1:11435";
const MODELS: &[&str] = &["bge-m3", "qwen3-embedding:0.6b"];

fn m(id: u64, content: &str) -> StoredMessage {
    StoredMessage { id, parent_id: if id == 1 { None } else { Some(id - 1) }, speaker: "opus".into(), content: content.into() }
}

/// real_corpus_recall.rs와 동일한 실코퍼스(seCall project=tunaRound 실 턴 발췌).
fn corpus() -> StoredSession {
    StoredSession {
        messages: vec![
            m(1, "FTS5의 unicode61 토크나이저는 한국어를 형태소로 못 쪼개므로, Kiwi나 lindera로 미리 형태소 분해해 공백으로 조인한 텍스트를 색인한다."),
            m(2, "형태소 토큰에 더해 raw 토큰까지 같이 색인해서 외래어 임베딩이 조사째 원형으로 남도록 한다."),
            m(3, "임베딩 모델을 bge-m3에서 다른 것으로 바꿔도 content_hash가 같아 증분 가드가 오래된 벡터를 건너뛴다. 해시에 모델 id와 차원을 넣어야 한다."),
            m(4, "검색이 전역이라 세션이나 시간 필터가 없어 옛 결정이 최신 결정처럼 프롬프트에 주입될 수 있다."),
            m(5, "버려진 분기의 발언이 검색으로 끌려와 현재 분기 프롬프트에 섞일 수 있다."),
            m(6, "FTS 키 리스트와 벡터 키 리스트를 reciprocal rank fusion으로 융합한다. k는 60이다."),
            m(7, "recall at k와 MRR과 precision at k를 재는 회귀 게이트 벤치마크가 코드로 있어 검색 품질 저하를 막는다."),
            m(8, "retrieved 주입은 개수만 제한하고 글자수 상한이 없어 긴 발언 다섯 개면 프롬프트가 팽창한다."),
            m(9, "pull 모드 계측에서 프롬프트 길이가 전사 길이와 탈동조돼 claude가 9770자에서 429자로 줄었다."),
            m(10, "KiwiWrapper의 unsafe impl Send는 C 포인터를 Mutex 직렬화에 의존하는 잠재 위험이다."),
            m(11, "임베딩과 형태소 토큰화는 콘텐츠 주소 방식이라 해시에 model id를 넣으면 무효화 자체가 필요 없다."),
            m(12, "진짜 무효화가 필요한 건 파생된 쿼리 결과뿐이라 인덱스 세대 스탬프로 지연 무효화한다."),
            m(13, "세대 입도를 세션이 아니라 인덱스 단위로 두면 무관한 세션의 캐시가 살아남는다."),
            m(14, "단일 프로세스가 HTTP MCP 코어를 바인드하고 REPL을 동시에 구동하며 bearer 인증이 401과 200으로 동작한다."),
            m(15, "codex exec는 read-only 샌드박스를 유지한 채로는 MCP 도구 호출을 자동 승인할 방법이 없다."),
            m(16, "codex는 규칙 준수가 강해서 read-only를 샌드박스가 아니라 프롬프트 지시로 강제할 수 있다."),
            m(17, "recency 랭킹은 messages에 created_at 컬럼을 더하고 다른 세션의 오래된 후보만 소폭 강등한다."),
            m(18, "debug_retrieve 디버그 출력에 created_at과 recency 강등 표시를 더해 눈으로 확인할 수 있게 했다."),
        ],
        head: Some(18),
    }
}

type EvalQuery = (&'static str, &'static [u64]);
const QUERIES: &[EvalQuery] = &[
    ("한국어 형태소 색인", &[1, 2]),
    ("모델 바꾸면 재색인", &[3, 11]),
    ("오래된 결정이 최신처럼 섞임", &[4, 17]),
    ("버려진 분기 발언", &[5]),
    ("BM25와 벡터 융합", &[6]),
    ("검색 품질 회귀 방지", &[7]),
    ("프롬프트 팽창 제한", &[8]),
    ("풀 모드 토큰 절감", &[9]),
    ("codex 도구 승인 막힘", &[15, 16]),
    ("캐시 무효화 전략", &[11, 12, 13]),
    ("원격 코어 인증", &[14]),
    ("검색 디버그 창구", &[18]),
];

fn recall_at_k(retrieved: &[u64], gold: &[u64], k: usize) -> f64 {
    let top: Vec<u64> = retrieved.iter().take(k).copied().collect();
    gold.iter().filter(|g| top.contains(g)).count() as f64 / gold.len() as f64
}
fn mrr(retrieved: &[u64], gold: &[u64]) -> f64 {
    for (i, id) in retrieved.iter().enumerate() {
        if gold.contains(id) {
            return 1.0 / (i as f64 + 1.0);
        }
    }
    0.0
}

#[test]
#[ignore] // 수동: Ollama 터널(11435) + 두 모델 pull 필요.
fn embed_model_compare() {
    let ss = corpus();
    const K: usize = 5;
    let mut summary: Vec<(String, f64, f64, f64, f64)> = Vec::new();

    for model in MODELS {
        let safe = model.replace([':', '/'], "_");
        let path = std::env::temp_dir().join(format!("tuna_embcmp_{safe}.db"));
        let _ = std::fs::remove_file(&path);
        let p = path.to_str().unwrap();

        let tok = create_tokenizer("lindera").unwrap();
        let store_w = SqliteStore::open(p).unwrap();
        store_w.save_session("real", &ss, |t| tok.fts_index(t)).unwrap();
        let embedder = OllamaEmbedder::new(ENDPOINT, model);
        store_w
            .index_vectors("real", &ss, &embedder)
            .unwrap_or_else(|e| panic!("index_vectors({model}) 실패(터널?): {e}"));
        drop(store_w);

        let id_of = |content: &str| -> Option<u64> { ss.messages.iter().find(|x| x.content == content).map(|x| x.id) };
        let store_r = SqliteStore::open(p).unwrap();
        let tok2 = create_tokenizer("lindera").unwrap();
        let retriever = SqliteRetriever::new(
            SqliteStore::open(p).unwrap(),
            Box::new(move |t: &str| tok2.fts_query(t)),
            Some(Box::new(OllamaEmbedder::new(ENDPOINT, model))),
        );

        let (mut vr, mut vm, mut hr, mut hm) = (0.0f64, 0.0, 0.0, 0.0);
        println!("\n{:=<74}", "");
        println!("모델: {model}");
        println!("{:-<74}", "");
        println!("{:<28} {:>6} {:>6} {:>6} {:>6}", "질의", "vecR", "vecM", "hybR", "hybM");
        for (q, gold) in QUERIES {
            let qv = embedder.embed(q).expect("embed query");
            let vec_ids: Vec<u64> = store_r.vector_search(&qv, K).unwrap_or_default().iter().map(|(_, mid, _)| *mid).collect();
            let hyb_ids: Vec<u64> = retriever.retrieve(q, K).unwrap().iter().filter_map(|u| id_of(&u.content)).collect();
            let (a, b) = (recall_at_k(&vec_ids, gold, K), mrr(&vec_ids, gold));
            let (c, d) = (recall_at_k(&hyb_ids, gold, K), mrr(&hyb_ids, gold));
            vr += a; vm += b; hr += c; hm += d;
            println!("{q:<28} {a:>6.3} {b:>6.3} {c:>6.3} {d:>6.3}");
        }
        let n = QUERIES.len() as f64;
        println!("{:-<74}", "");
        println!("{:<28} {:>6.3} {:>6.3} {:>6.3} {:>6.3}  (mean)", "MEAN", vr / n, vm / n, hr / n, hm / n);
        summary.push((model.to_string(), vr / n, vm / n, hr / n, hm / n));
        let _ = std::fs::remove_file(&path);
    }

    println!("\n{:#<74}", "");
    println!("비교 요약 (real corpus 18발언 / {}질의)", QUERIES.len());
    println!("{:<26} {:>8} {:>8} {:>8} {:>8}", "모델", "vecR@5", "vecMRR", "hybR@5", "hybMRR");
    for (name, a, b, c, d) in &summary {
        println!("{name:<26} {a:>8.3} {b:>8.3} {c:>8.3} {d:>8.3}");
    }
    println!("{:#<74}", "");
}
