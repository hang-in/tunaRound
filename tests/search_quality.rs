// 검색 품질 측정 하네스(수동, #[ignore]). 통제 코퍼스를 형태소 FTS·벡터·하이브리드로 색인하고
// 정확/굴절/동의어 질의의 상위 결과를 출력해 관련성을 사람이 판단한다. semantic+morphology 필요.
#![cfg(all(feature = "semantic", feature = "morphology"))]

use tunaround::search::tokenizer::create_tokenizer;
use tunaround::store::embedding::{Embedder, OllamaEmbedder};
use tunaround::store::retriever::SqliteRetriever;
use tunaround::store::sqlite::SqliteStore;
use tunaround::store::{StoredMessage, StoredSession};
use tunaround::orchestrator::ContextRetriever;

const ENDPOINT: &str = "http://127.0.0.1:11435";

fn msg(id: u64, speaker: &str, content: &str) -> StoredMessage {
    StoredMessage { id, parent_id: if id == 1 { None } else { Some(id - 1) }, speaker: speaker.into(), content: content.into() }
}

// 3개 주제 6개 발언. 질의가 정확/굴절/동의어로 맞히는지 보려고 의도적으로 다른 어휘 사용.
fn corpus() -> StoredSession {
    StoredSession {
        messages: vec![
            msg(1, "claude", "JWT 기반 인증을 어떻게 설계할까. 액세스 토큰과 리프레시 토큰을 분리하자."),
            msg(2, "codex", "세션 방식보다 토큰 방식이 수평 확장에 유리하다."),
            msg(3, "claude", "검색은 형태소 FTS와 벡터 임베딩을 RRF로 융합한 하이브리드로 간다."),
            msg(4, "codex", "재주입을 줄이려면 최근 몇 턴만 넣고 나머지는 검색으로 끌어온다."),
            msg(5, "claude", "Redis 버스로 여러 프로세스가 한 세션을 공유하고 재개한다."),
            msg(6, "codex", "스냅샷과 이벤트 스트림으로 멀티세션을 미러링한다."),
        ],
        head: Some(6),
    }
}

#[test]
#[ignore] // 수동: Ollama 터널(11435) 필요. `cargo test --features "semantic morphology" --test search_quality -- --ignored --nocapture`
fn measure_search_quality() {
    let dir = std::env::temp_dir();
    let path = dir.join("tuna_quality.db");
    let _ = std::fs::remove_file(&path);
    let p = path.to_str().unwrap();

    let tok = create_tokenizer("kiwi").expect("tokenizer");
    let store_w = SqliteStore::open(p).unwrap();
    let ss = corpus();
    store_w.save_session("q", &ss, |t| tok.fts_index(t)).unwrap();
    let embedder = OllamaEmbedder::new(ENDPOINT, "bge-m3");
    store_w.index_vectors("q", &ss, &embedder).expect("index vectors");

    let store_r = SqliteStore::open(p).unwrap();
    let tok2 = create_tokenizer("kiwi").unwrap();
    let retriever = SqliteRetriever::new(
        SqliteStore::open(p).unwrap(),
        Box::new(move |t: &str| tok2.fts_query(t)),
        Some(Box::new(OllamaEmbedder::new(ENDPOINT, "bge-m3"))),
    );

    // (질의, 기대 주제, 종류)
    let queries = [
        ("토큰", "인증(정확 토큰)"),
        ("인증을", "인증(굴절: 조사 포함)"),
        ("로그인 방식", "인증(동의어: 공유 토큰 없음 - 벡터만 가능)"),
        ("임베딩", "검색(정확)"),
        ("프로세스 공유", "Redis(어휘 일부)"),
        ("대화 기록을 어떻게 기억하지", "검색/재주입(의미)"),
    ];

    for (q, expect) in queries {
        println!("\n==== 질의: '{q}'  (기대: {expect}) ====");

        let lex = store_r.search(&tok.fts_query(q), 3).unwrap();
        println!("  [어휘 FTS] {}", if lex.is_empty() { "(없음)".into() } else {
            lex.iter().map(|h| format!("#{} {:.2} {}", h.msg_id, h.score, snip(&h.content))).collect::<Vec<_>>().join(" | ")
        });

        let qv = embedder.embed(q).unwrap();
        let vec = store_r.vector_search(&qv, 3).unwrap();
        let vec_str: Vec<String> = vec.iter().map(|(_, mid, sc)| {
            let c = store_r.get_message("q", *mid).ok().flatten().map(|(_, c)| c).unwrap_or_default();
            format!("#{mid} {sc:.3} {}", snip(&c))
        }).collect();
        println!("  [벡터]    {}", if vec_str.is_empty() { "(없음)".into() } else { vec_str.join(" | ") });

        let hyb = retriever.retrieve(q, 3);
        println!("  [하이브리드] {}", if hyb.is_empty() { "(없음)".into() } else {
            hyb.iter().map(|u| snip(&u.content)).collect::<Vec<_>>().join(" | ")
        });
    }

    let _ = std::fs::remove_file(&path);
}

fn snip(s: &str) -> String {
    s.chars().take(22).collect()
}
