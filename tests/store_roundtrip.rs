// store가 stored 메시지를 JSON 파일로 저장/로드 라운드트립하는지 검증.
use tunaround::orchestrator::{MapRegistry, Participant, Utterance};
use tunaround::repl::Session;
use tunaround::store::{from_stored, load, load_session, save, to_stored};

#[test]
fn save_then_load_roundtrips() {
    let transcript = vec![
        Utterance {
            speaker: "claude/proposer".into(),
            content: "제안".into(),
            abstraction: None,
        },
        Utterance {
            speaker: "codex/reviewer".into(),
            content: "리뷰".into(),
            abstraction: None,
        },
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

#[test]
fn session_save_state_then_resume() {
    let transcript = vec![Utterance {
        speaker: "claude/proposer".into(),
        content: "이전 결론".into(),
        abstraction: None,
    }];
    let dir = std::env::temp_dir();
    let path = dir.join(format!(
        "tunaround_session_test_{}.json",
        std::process::id()
    ));
    let path = path.to_str().unwrap();
    save(&to_stored(&transcript), path).expect("seed save");

    let participants = vec![Participant {
        engine: "claude".into(),
        role: Some("proposer".into()),
        instruction: String::new(),
    }];
    let resumed =
        Session::resume(participants, Box::new(MapRegistry::new()), path).expect("resume ok");
    assert_eq!(resumed.transcript_len(), 1);

    resumed.save_state(path).expect("save_state ok");
    // save_state는 JSON(StoredSession 와이어) 저장이므로 load_session(ConversationSnapshot)으로 확인.
    assert_eq!(load_session(path).expect("reload").node_count(), 1);

    let _ = std::fs::remove_file(path);
}
