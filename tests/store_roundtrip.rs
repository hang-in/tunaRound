// store가 stored 메시지를 JSON 파일로 저장/로드 라운드트립하는지 검증.
use tunaround::orchestrator::Utterance;
use tunaround::store::{from_stored, load, save, to_stored};

#[test]
fn save_then_load_roundtrips() {
    let transcript = vec![
        Utterance { speaker: "claude/proposer".into(), content: "제안".into() },
        Utterance { speaker: "codex/reviewer".into(), content: "리뷰".into() },
    ];
    let stored = to_stored(&transcript);

    let dir = std::env::temp_dir();
    let path = dir.join(format!("tunaround_store_test_{}.json", std::process::id()));
    let path = path.to_str().unwrap();

    save(&stored, path).expect("save ok");
    let loaded = load(path).expect("load ok");
    assert_eq!(loaded, stored);
    assert_eq!(from_stored(&loaded), transcript);

    let _ = std::fs::remove_file(path);
}
