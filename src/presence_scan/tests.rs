// presence_scan 모듈(claude·codex 스캔, 프로세스/마커 게이트, 보고 조립) 단위테스트.

use super::*;

#[test]
fn codex_meta_line_parses_id_cwd_originator() {
    let line = r#"{"timestamp":"2026-07-11T09:00:05Z","type":"session_meta","payload":{"session_id":"abc-123","id":"abc-123","timestamp":"2026-07-11T09:00:00Z","cwd":"C:\\Users\\me\\proj","originator":"codex-tui"}}"#;
    let (id, cwd, orig, created_at) = parse_codex_meta_line(line).unwrap();
    assert_eq!(id, "abc-123");
    assert_eq!(cwd.as_deref(), Some("C:\\Users\\me\\proj"));
    assert_eq!(orig.as_deref(), Some("codex-tui"));
    // created_at = payload.timestamp(세션 시작) 우선, ISO→DB. 이슈 #88 grace 신호.
    assert_eq!(created_at.as_deref(), Some("2026-07-11 09:00:00"));
    // payload.timestamp 없으면 top-level로 폴백.
    let no_payload_ts = r#"{"timestamp":"2026-07-11T10:00:00Z","type":"session_meta","payload":{"session_id":"x","originator":"codex-tui"}}"#;
    assert_eq!(
        parse_codex_meta_line(no_payload_ts).unwrap().3.as_deref(),
        Some("2026-07-11 10:00:00")
    );
    // session_meta가 아닌 줄은 None.
    assert!(parse_codex_meta_line(r#"{"type":"turn","payload":{}}"#).is_none());
}

#[test]
fn project_normalizes_home_and_falls_back_to_basename() {
    let home = Path::new("C:\\Users\\me");
    assert_eq!(
        project_from_cwd_normalized(Some("C:\\Users\\me"), Some(home)),
        Some("home".to_string())
    );
    // 대소문자·구분자 차이도 home으로 인식.
    assert_eq!(
        project_from_cwd_normalized(Some("c:/users/me/"), Some(home)),
        Some("home".to_string())
    );
    assert_eq!(
        project_from_cwd_normalized(Some("C:\\Users\\me\\tunaRound"), Some(home)),
        Some("tunaRound".to_string())
    );
    assert_eq!(project_from_cwd_normalized(None, Some(home)), None);
}

#[test]
fn is_temp_cwd_matches_macos_conventional_tmp_prefixes() {
    // std::env::temp_dir()은 /var/folders/...만 잡으므로, 이 케이스들은 그 비교로는 못 잡고
    // MAC_TEMP_PREFIXES 폴백이 잡아야 한다. 그 폴백은 macOS 전용이라(coderabbit) 양성 단언도
    // macOS에서만 성립한다(Linux/Windows에선 /tmp를 temp로 보지 않아 오분류가 없다).
    #[cfg(target_os = "macos")]
    {
        assert!(is_temp_cwd("/tmp/work-xyz"));
        assert!(is_temp_cwd("/tmp"));
        assert!(is_temp_cwd("/private/tmp/work-xyz"));
        assert!(is_temp_cwd("/private/var/folders/ab/xyz/T/foo"));
        // 대소문자·구분자 정규화(기존 규약 유지).
        assert!(is_temp_cwd("/TMP/Work-Xyz"));
    }
    // 접두만 겹치는 비-temp 경로는 어느 플랫폼에서도 temp가 아니다.
    assert!(!is_temp_cwd("/tmpfoo/bar"));
    assert!(!is_temp_cwd("/home/user/project"));
}

#[test]
fn count_matching_lines_covers_paths_wrappers_and_csv() {
    // unix(`ps -o pid=,args=`): pid 토큰 뒤 argv에서 단독 실행 / 전체 경로 / node 인터프리터
    // 래퍼를 잡고, 뒤쪽 인자 매칭(3토큰 밖)은 제외.
    let unix = "  11 claude --resume abc\n  22 /usr/local/bin/claude\n  33 node /home/u/.npm/bin/claude --flag\n  44 ps -ax\n  55 tunaround poll --tags a b runner=claude\n";
    assert_eq!(count_matching_lines(unix, "claude", false), 3);
    // node 래퍼가 2번째 토큰이 아니라도 argv 3토큰 안이면 잡힌다.
    assert_eq!(
        count_matching_lines("77 env FOO=1 /opt/claude serve\n", "claude", false),
        1
    );
    // win CSV: 이미지명 필드만 본다.
    let win = "\"claude.exe\",\"123\",\"Console\"\n\"notepad.exe\",\"9\",\"Console\"\n\"x.exe\",\"1\",\"claude\"\n";
    assert_eq!(count_matching_lines(win, "claude", true), 1);
    assert_eq!(count_matching_lines(win, "codex", true), 0);
}

#[test]
fn snapshot_sanity_rejects_error_only_output() {
    // tasklist 부하 에러 출력(exit 0)은 pid가 안 나와 스냅샷 실패로 간주돼야 한다(깜빡임 방지).
    let err_text = "ERROR: This operation returned because the timeout period expired.\n";
    assert!(parse_pids(err_text, true).is_empty());
    assert!(parse_pids(err_text, false).is_empty());
}

#[test]
fn parse_pids_extracts_from_both_formats() {
    let unix = "  11 claude\n  22 /bin/ps\nbadline\n";
    let pids = parse_pids(unix, false);
    assert!(pids.contains(&11) && pids.contains(&22) && pids.len() == 2);
    let win = "\"claude.exe\",\"123\",\"Console\"\n\"x.exe\",\"9\",\"c\"\n";
    let pids = parse_pids(win, true);
    assert!(pids.contains(&123) && pids.contains(&9) && pids.len() == 2);
}

#[test]
fn marker_liveness_drops_only_dead_pid_sessions() {
    use std::collections::HashSet;
    let alive: HashSet<u32> = [100u32, 200].into_iter().collect();
    // PID 기록 + 스냅샷에 있음 → 유지 / 없음 → 유령 제거.
    assert!(is_session_live(&MarkerState::Pid(100), &alive));
    assert!(!is_session_live(&MarkerState::Pid(999), &alive));
    // 마커 없음·PID 미상 → 보수적 유지(신선도 창 폴백).
    assert!(is_session_live(&MarkerState::NoMarker, &alive));
    assert!(is_session_live(&MarkerState::Unknown, &alive));
}

#[test]
fn read_marker_parses_pid_empty_and_missing() {
    let dir = std::env::temp_dir().join(format!("tuna-marker-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("s1.ctx"), "4242\n").unwrap();
    std::fs::write(dir.join("s2.ctx"), "").unwrap();
    assert_eq!(read_marker(&dir, "s1"), MarkerState::Pid(4242));
    assert_eq!(read_marker(&dir, "s2"), MarkerState::Unknown);
    assert_eq!(read_marker(&dir, "none"), MarkerState::NoMarker);
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn process_gate_drops_runner_only_when_zero() {
    let s = |r: &str| LiveSession {
        uuid: r.to_string(),
        runner: r.to_string(),
        project: None,
        human_input_at: None,
        created_at: None,
        active_at: None,
    };
    let all = vec![s("claude"), s("codex")];
    // 확실한 0 → 해당 러너만 제거.
    let gated = apply_process_gate(all.clone(), "codex", Some(0));
    assert_eq!(gated.len(), 1);
    assert_eq!(gated[0].runner, "claude");
    // 조회 실패(None)·1개 이상 → 그대로.
    assert_eq!(apply_process_gate(all.clone(), "codex", None).len(), 2);
    assert_eq!(apply_process_gate(all, "codex", Some(3)).len(), 2);
}

#[test]
fn windows_gate_count_prefers_confirmed_image_match() {
    // 이미지명 매칭>0 → Some(그 카운트)(node 존재 여부 무관, 확정 신호).
    let text = "\"claude.exe\",\"123\",\"Console\"\n\"node.exe\",\"456\",\"Console\"\n";
    assert_eq!(windows_runner_gate_count(text, "claude"), Some(1));
}

#[test]
fn windows_gate_count_none_when_only_node_wrapper_present() {
    // 이미지 매칭 0 && node.exe 있음 → 판단 불가(None, 게이트 스킵 = 산 세션 보존).
    let text = "\"node.exe\",\"456\",\"Console\"\n\"notepad.exe\",\"9\",\"Console\"\n";
    assert_eq!(windows_runner_gate_count(text, "claude"), None);
}

#[test]
fn windows_gate_count_zero_when_no_image_and_no_node() {
    // 이미지 매칭 0 && node.exe도 없음 → 확정 0(기존 동작 유지).
    let text = "\"notepad.exe\",\"9\",\"Console\"\n";
    assert_eq!(windows_runner_gate_count(text, "claude"), Some(0));
}

fn codex_session(uuid: &str, hi: Option<&str>, ca: Option<&str>) -> LiveSession {
    LiveSession {
        uuid: uuid.into(),
        runner: "codex".into(),
        project: None,
        human_input_at: hi.map(str::to_string),
        created_at: ca.map(str::to_string),
        active_at: None,
    }
}

#[test]
fn codex_human_input_gate_keeps_fresh_drops_ghost() {
    // 이슈 #88: threshold(= now-window) = 2026-07-11 09:00:00. codex는 human_input 또는 created가
    // 이 이후면 유지, 둘 다 stale/None이면 드롭. claude는 무조건 통과.
    let threshold = "2026-07-11 09:00:00";
    let sessions = vec![
        // 유령: 사람입력·생성 둘 다 threshold 이전 → 드롭(핵심 케이스).
        codex_session(
            "ghost",
            Some("2026-07-11 08:00:00"),
            Some("2026-07-11 07:00:00"),
        ),
        // 활성: 최근 사람입력 → 유지.
        codex_session(
            "active",
            Some("2026-07-11 09:30:00"),
            Some("2026-07-11 06:00:00"),
        ),
        // 신규-미입력: 사람입력 None이나 최근 생성(grace) → 유지.
        codex_session("new", None, Some("2026-07-11 09:10:00")),
        // 신호 없음(둘 다 None) → 드롭.
        codex_session("nosignal", None, None),
        // claude는 게이트 비대상 → human/created None이어도 통과.
        LiveSession {
            uuid: "claude-s".into(),
            runner: "claude".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
    ];
    let kept: Vec<String> = apply_codex_human_input_gate(sessions, threshold, None)
        .into_iter()
        .map(|s| s.uuid)
        .collect();
    assert_eq!(kept, vec!["active", "new", "claude-s"]);
}

#[test]
fn codex_human_input_gate_boundary_is_inclusive() {
    let threshold = "2026-07-11 09:00:00";
    // 정확히 threshold(>= 비교) = 유지.
    assert_eq!(
        apply_codex_human_input_gate(
            vec![codex_session("b", Some(threshold), None)],
            threshold,
            None
        )
        .len(),
        1
    );
    // threshold보다 1초 이전(created도 없음) = 드롭.
    assert_eq!(
        apply_codex_human_input_gate(
            vec![codex_session("b", Some("2026-07-11 08:59:59"), None)],
            threshold,
            None
        )
        .len(),
        0
    );
}

#[test]
fn codex_gate_drops_ghost_that_process_gate_kept_and_is_snapshot_independent() {
    // apply_process_gate는 codex 프로세스≥1이면 유령도 통과(all-or-nothing). human_input 게이트가 그
    // 뒤에서 유령을 드롭하는 협업을 못박는다. 게이트는 프로세스 count를 안 받으므로 스냅샷 실패 주기에도 동작.
    let threshold = "2026-07-11 09:00:00";
    let ghost = codex_session(
        "ghost",
        Some("2026-07-11 08:00:00"),
        Some("2026-07-11 07:00:00"),
    );
    let fresh = codex_session("fresh", Some("2026-07-11 09:30:00"), None);
    let after_proc = apply_process_gate(vec![ghost, fresh], "codex", Some(1));
    assert_eq!(
        after_proc.len(),
        2,
        "process_gate는 count>=1이면 유령도 유지"
    );
    let after_human: Vec<String> = apply_codex_human_input_gate(after_proc, threshold, None)
        .into_iter()
        .map(|s| s.uuid)
        .collect();
    assert_eq!(after_human, vec!["fresh"]);
}

#[test]
fn codex_gate_fresh_churn_ghost_lingers_documented_residual() {
    // 이슈 #88의 원리적 잔여(재론 금지): 방금 쓰다 닫은 세션(=fresh-churn 유령)은 human_input이 최근이라
    // 살아있는 idle 세션과 시간만으론 구분 불가 → window 동안 로스터에 잔존한다. 재현 데이터(2026-07-12):
    // 유령 019f5547과 라이브 019f554b가 ~4분 간격 생성. 유령도 닫히기 직전 사람입력이 있었으므로 그 시각이
    // window 이내면 게이트를 통과한다. 이 테스트는 "게이트가 #88을 완전 제거한다"는 오해를 막는 명세다.
    let threshold = "2026-07-11 09:00:00"; // now-window.
    // 유령: 닫히기 직전(threshold 이후) 사람입력 → 게이트 통과(=수용된 잔여, 드롭 안 됨).
    let fresh_ghost = codex_session("fresh-ghost", Some("2026-07-11 09:20:00"), None);
    // 라이브: 더 최근 입력 → 당연히 통과. 둘 다 남아 dispatcher/사용자가 유령을 고를 수 있다.
    let live = codex_session("live", Some("2026-07-11 09:40:00"), None);
    let kept: Vec<String> = apply_codex_human_input_gate(vec![fresh_ghost, live], threshold, None)
        .into_iter()
        .map(|s| s.uuid)
        .collect();
    assert_eq!(
        kept,
        vec!["fresh-ghost", "live"],
        "fresh-churn 유령은 window 동안 잔존(원리적 한계): 게이트는 유령 수명 상한과 relay 자기유지 차단만 보장"
    );
}

#[test]
fn codex_gate_marker_hybrid_overrides_window_by_state() {
    // 이슈 #119: marker_dir가 주어지면 threadId<->래퍼PID 마커가 시간창보다 우선한다.
    use std::collections::HashSet;
    let marker_dir = std::env::temp_dir().join(format!(
        "tuna-codex-hybrid-{}-{}",
        std::process::id(),
        line!()
    ));
    std::fs::create_dir_all(&marker_dir).unwrap();
    // Dead(래퍼가 종료 확정 통보) → human_input이 신선해도 무조건 드롭.
    std::fs::write(marker_dir.join("codex-dead.ctx"), "dead").unwrap();
    // Pid(래퍼 생존 중) → 사람입력·생성 둘 다 stale이어도 window 면제로 유지.
    std::fs::write(marker_dir.join("codex-pid-stale.ctx"), "4242").unwrap();
    // Unknown(마커는 있으나 owner 미상) → 마찬가지로 stale이어도 유지.
    std::fs::write(marker_dir.join("codex-unknown-stale.ctx"), "").unwrap();
    // codex-nomarker-* 는 마커 파일 자체가 없음(NoMarker) → 기존 시간창 판정 폴백.

    let threshold = "2026-07-11 09:00:00";
    let stale = Some("2026-07-11 08:00:00");
    let fresh = Some("2026-07-11 09:30:00");
    let sessions = vec![
        codex_session("codex-dead", fresh, fresh),
        codex_session("codex-pid-stale", stale, stale),
        codex_session("codex-unknown-stale", stale, stale),
        codex_session("codex-nomarker-stale", stale, stale),
        codex_session("codex-nomarker-fresh", fresh, None),
    ];
    let kept: HashSet<String> =
        apply_codex_human_input_gate(sessions.clone(), threshold, Some(&marker_dir))
            .into_iter()
            .map(|s| s.uuid)
            .collect();
    assert!(
        !kept.contains("codex-dead"),
        "Dead 마커는 신선해도 드롭: {kept:?}"
    );
    assert!(
        kept.contains("codex-pid-stale"),
        "Pid 마커는 stale이어도 window 면제: {kept:?}"
    );
    assert!(
        kept.contains("codex-unknown-stale"),
        "Unknown 마커도 window 면제: {kept:?}"
    );
    assert!(
        !kept.contains("codex-nomarker-stale"),
        "마커 없음은 window 판정 폴백(stale=드롭): {kept:?}"
    );
    assert!(
        kept.contains("codex-nomarker-fresh"),
        "마커 없음+fresh는 기존과 동일하게 유지: {kept:?}"
    );

    // marker_dir=None이면 마커 존재와 무관하게 순수 시간창 판정(하위호환)으로 폴백한다.
    let kept_no_marker_dir: HashSet<String> =
        apply_codex_human_input_gate(sessions, threshold, None)
            .into_iter()
            .map(|s| s.uuid)
            .collect();
    assert_eq!(
        kept_no_marker_dir,
        ["codex-dead", "codex-nomarker-fresh"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<String>>(),
        "marker_dir=None은 마커 파일이 있어도 순수 시간창 판정만 수행: {kept_no_marker_dir:?}"
    );

    std::fs::remove_dir_all(&marker_dir).ok();
}

#[test]
fn system_time_to_db_datetime_formats_utc_civil() {
    use std::time::{Duration, UNIX_EPOCH};
    assert_eq!(
        system_time_to_db_datetime(UNIX_EPOCH).as_deref(),
        Some("1970-01-01 00:00:00")
    );
    // +1일 +1시간 +1분 +1초.
    assert_eq!(
        system_time_to_db_datetime(UNIX_EPOCH + Duration::from_secs(86_400 + 3661)).as_deref(),
        Some("1970-01-02 01:01:01")
    );
    // 알려진 epoch 1700000000 = 2023-11-14 22:13:20 UTC(월/윤년 경계 civil 정확성).
    assert_eq!(
        system_time_to_db_datetime(UNIX_EPOCH + Duration::from_secs(1_700_000_000)).as_deref(),
        Some("2023-11-14 22:13:20")
    );
}

#[test]
fn codex_input_cache_round_trips_through_disk() {
    let path = std::env::temp_dir().join(format!("tuna-codex-cache-{}.json", std::process::id()));
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    let mut cache: CodexInputCache = CodexInputCache::new();
    cache.insert(
        "abc".to_string(),
        (mtime, Some("2026-07-11 09:00:00".to_string())),
    );
    cache.insert("no-input".to_string(), (mtime, None));
    save_codex_input_cache_to_disk(&path, &cache);
    let loaded = load_codex_input_cache_from_disk(&path);
    std::fs::remove_file(&path).ok();
    assert_eq!(loaded.len(), 2);
    assert_eq!(
        loaded.get("abc").map(|(t, hi)| (*t, hi.clone())),
        Some((mtime, Some("2026-07-11 09:00:00".to_string())))
    );
    assert_eq!(loaded.get("no-input"), Some(&(mtime, None)));
}

#[test]
fn codex_input_cache_load_missing_or_corrupt_falls_back_empty() {
    let missing = std::env::temp_dir().join(format!(
        "tuna-codex-cache-missing-{}.json",
        std::process::id()
    ));
    std::fs::remove_file(&missing).ok();
    assert!(load_codex_input_cache_from_disk(&missing).is_empty());

    let corrupt = std::env::temp_dir().join(format!(
        "tuna-codex-cache-corrupt-{}.json",
        std::process::id()
    ));
    std::fs::write(&corrupt, "not json").unwrap();
    assert!(load_codex_input_cache_from_disk(&corrupt).is_empty());
    std::fs::remove_file(&corrupt).ok();
}

#[test]
fn codex_enumerate_scans_tree_and_filters_non_tui() {
    let dir = std::env::temp_dir().join(format!("tuna-prescan-{}", std::process::id()));
    let day = dir.join("2026").join("07").join("11");
    std::fs::create_dir_all(&day).unwrap();
    let mk = |name: &str, body: &str| std::fs::write(day.join(name), body).unwrap();
    mk(
        "rollout-2026-07-11T01-aaa.jsonl",
        r#"{"type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#,
    );
    mk(
        "rollout-2026-07-11T02-bbb.jsonl",
        r#"{"type":"session_meta","payload":{"session_id":"exec-1","cwd":"/u/x/projA","originator":"codex exec"}}"#,
    );
    mk(
        "not-a-rollout.jsonl",
        r#"{"type":"session_meta","payload":{"session_id":"zzz","originator":"codex-tui"}}"#,
    );
    let found = enumerate_codex_sessions(
        &dir,
        SystemTime::now(),
        Duration::from_secs(3600),
        None,
        None,
    );
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 1, "TUI 세션만: {found:?}");
    assert_eq!(found[0].uuid, "tui-1");
    assert_eq!(found[0].project.as_deref(), Some("projA"));
    assert_eq!(
        found[0].human_input_at, None,
        "input_cache 없으면 신호 스캔 생략"
    );
}

#[test]
fn report_json_shape_and_display_name() {
    let sessions = vec![
        LiveSession {
            uuid: "u1".into(),
            runner: "claude".into(),
            project: Some("tunaRound".into()),
            human_input_at: Some("2026-07-11 09:00:00".into()),
            created_at: None,
            active_at: None,
        },
        LiveSession {
            uuid: "x1".into(),
            runner: "codex".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            // 이슈 #123: codex rollout mtime이 활동 신호로 payload에 실린다.
            active_at: Some("2026-07-18 10:00:00".into()),
        },
    ];
    let v = to_report_json("win", &sessions);
    assert_eq!(v[0]["uuid"], "u1");
    assert_eq!(v[0]["display_name"], "win-claude-tunaRound");
    assert_eq!(v[0]["human_input_at"], "2026-07-11 09:00:00");
    assert_eq!(v[0]["active_at"], serde_json::Value::Null);
    assert_eq!(v[1]["active_at"], "2026-07-18 10:00:00");
}

// --- v2-45 P5: codex 입력 신호 tail 스캔 ---

#[test]
fn normalize_iso_to_db_datetime_handles_t_frac_z_offset() {
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00.894Z").as_deref(),
        Some("2026-07-11 09:00:00")
    );
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00Z").as_deref(),
        Some("2026-07-11 09:00:00")
    );
    // 비영 offset은 변환 없이 절단하면 그 시간만큼 조용히 왜곡되므로 None(거부, 발견 이후 정정).
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00+09:00"),
        None
    );
    // 음수 offset도 날짜의 '-'와 혼동 없이 판정해 거부한다(gemini 리뷰 케이스는 유지, 기대값만 갱신).
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00-05:00"),
        None
    );
    // 영(0) offset은 UTC와 같아 안전 절단(+00:00·-00:00 둘 다).
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00+00:00").as_deref(),
        Some("2026-07-11 09:00:00")
    );
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00-00:00").as_deref(),
        Some("2026-07-11 09:00:00")
    );
    // 소수초 뒤에 붙은 비영 offset도 놓치지 않고 거부(소수초 절단이 offset 탐지를 가리면 안 된다).
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11T09:00:00.894+09:00"),
        None
    );
    // 이미 DB 포맷이면 그대로.
    assert_eq!(
        normalize_iso_to_db_datetime("2026-07-11 09:00:00").as_deref(),
        Some("2026-07-11 09:00:00")
    );
    // 형태 불량은 None(오염 방어).
    assert_eq!(normalize_iso_to_db_datetime("어제"), None);
    assert_eq!(normalize_iso_to_db_datetime("2026-07-11T09:00"), None);
}

#[test]
fn message_is_human_input_excludes_relay_prefix() {
    assert!(message_is_human_input("현재 WSL 설정 확인 바람"));
    // relay 주입(build_inject_text prefix)은 사람 입력 아님(§5-6). 실제 출력은 선행 공백이 없다.
    assert!(!message_is_human_input(
        &crate::codex_relay::build_inject_text("t1", "요청")
    ));
    assert!(!message_is_human_input("브로커 task abc 가 배달됐다"));
    // 선행 공백이 붙은 형태는 relay가 만들지 않으므로 사람 입력으로 남긴다(trim 안 함 = 오분류 좁힘).
    assert!(message_is_human_input("  브로커 task 이거 뭐야"));
}

#[test]
fn parse_codex_last_user_input_takes_latest_human_message() {
    let dir = std::env::temp_dir().join(format!("tuna-codex-input-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout.jsonl");
    let body = concat!(
        r#"{"timestamp":"2026-07-11T09:00:00.100Z","type":"session_meta","payload":{"session_id":"s","originator":"codex-tui"}}"#,
        "\n",
        r#"{"timestamp":"2026-07-11T09:01:00.200Z","type":"event_msg","payload":{"type":"user_message","message":"첫 질문"}}"#,
        "\n",
        r#"{"timestamp":"2026-07-11T09:02:00.300Z","type":"event_msg","payload":{"type":"agent_message","message":"답변"}}"#,
        "\n",
        r#"{"timestamp":"2026-07-11T09:03:00.400Z","type":"event_msg","payload":{"type":"user_message","message":"브로커 task xyz 가 배달됐다(이미 claim됨)"}}"#,
        "\n",
        r#"{"timestamp":"2026-07-11T09:04:00.500Z","type":"event_msg","payload":{"type":"user_message","message":"둘째 질문"}}"#,
        "\n",
    );
    std::fs::write(&path, body).unwrap();
    let got = parse_codex_last_user_input(&path, CODEX_TAIL_BYTES);
    std::fs::remove_dir_all(&dir).ok();
    // 마지막 사람 user_message(09:04)를 정규화해 반환. relay 주입(09:03)·agent_message는 제외.
    assert_eq!(got.as_deref(), Some("2026-07-11 09:04:00"));
}

#[test]
fn parse_codex_last_user_input_none_when_only_relay() {
    let dir = std::env::temp_dir().join(format!("tuna-codex-relay-only-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("rollout.jsonl");
    let body = concat!(
        r#"{"timestamp":"2026-07-11T09:03:00Z","type":"event_msg","payload":{"type":"user_message","message":"브로커 task xyz 가 배달됐다"}}"#,
        "\n",
    );
    std::fs::write(&path, body).unwrap();
    let got = parse_codex_last_user_input(&path, CODEX_TAIL_BYTES);
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(got, None, "relay 주입만 있으면 사람 입력 없음");
}

#[test]
fn enumerate_with_cache_scans_input_and_skips_on_unchanged_mtime() {
    let dir = std::env::temp_dir().join(format!("tuna-codex-cache-{}", std::process::id()));
    let day = dir.join("2026").join("07").join("11");
    std::fs::create_dir_all(&day).unwrap();
    std::fs::write(
            day.join("rollout-2026-07-11T01-aaa.jsonl"),
            concat!(
                r#"{"timestamp":"2026-07-11T09:00:00Z","type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#, "\n",
                r#"{"timestamp":"2026-07-11T09:05:00.123Z","type":"event_msg","payload":{"type":"user_message","message":"사람 입력"}}"#, "\n",
            ),
        ).unwrap();
    let mut cache = CodexInputCache::new();
    let found = enumerate_codex_sessions(
        &dir,
        SystemTime::now(),
        Duration::from_secs(3600),
        None,
        Some(&mut cache),
    );
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].human_input_at.as_deref(),
        Some("2026-07-11 09:05:00"),
        "tail 스캔이 사람 입력 시각 추출"
    );
    // 캐시에 mtime+값 저장 → 같은 파일 재스캔 시 재사용(mtime 무변경).
    assert!(cache.contains_key("tui-1"));
    let found2 = enumerate_codex_sessions(
        &dir,
        SystemTime::now(),
        Duration::from_secs(3600),
        None,
        Some(&mut cache),
    );
    assert_eq!(
        found2[0].human_input_at.as_deref(),
        Some("2026-07-11 09:05:00"),
        "캐시 재사용도 동일 값"
    );
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn enumerate_cache_preserves_human_input_when_scrolled_out_of_tail() {
    // 이슈 #88 적대 검증 minor: 장기 자율작업의 마지막 사람입력이 256KB tail 밖으로 밀려 재스캔이
    // None이어도 캐시의 이전 관측값을 유지한다(살아있는 세션의 조기 드롭 방지, human_input 단조성).
    // 캐시 mtime을 EPOCH로 심어 이번 주기 파일 mtime과 무조건 달라 rescan 경로를 강제한다.
    let dir = std::env::temp_dir().join(format!("tuna-codex-persist-{}", std::process::id()));
    let day = dir.join("2026").join("07").join("11");
    std::fs::create_dir_all(&day).unwrap();
    // 사람 입력(user_message) 없는 rollout → 재스캔 시 parse_codex_last_user_input=None.
    std::fs::write(
            day.join("rollout-2026-07-11T01-aaa.jsonl"),
            concat!(
                r#"{"timestamp":"2026-07-11T09:00:00Z","type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#, "\n",
                r#"{"timestamp":"2026-07-11T09:30:00Z","type":"event_msg","payload":{"type":"agent_message","message":"긴 출력"}}"#, "\n",
            ),
        )
        .unwrap();
    let mut cache = CodexInputCache::new();
    // 이전 주기에 관측한 사람입력을 심는다(mtime=EPOCH ≠ 파일 mtime → rescan 발생).
    cache.insert(
        "tui-1".to_string(),
        (
            SystemTime::UNIX_EPOCH,
            Some("2026-07-11 09:05:00".to_string()),
        ),
    );
    let found = enumerate_codex_sessions(
        &dir,
        SystemTime::now(),
        Duration::from_secs(3600),
        None,
        Some(&mut cache),
    );
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(found.len(), 1);
    assert_eq!(
        found[0].human_input_at.as_deref(),
        Some("2026-07-11 09:05:00"),
        "재스캔이 None이어도 캐시의 이전 human_input_at 유지(단조)"
    );
}

// --- v2-45 P8: 유휴-열림 세션 로스터 유지 ---

#[test]
fn runner_pids_extracts_only_named_runner_pids() {
    // unix: pid 토큰 뒤 argv basename이 claude인 행만(경로·node 래퍼 커버), 다른 러너/도구는 제외.
    let unix = concat!(
        "  11 claude --resume abc\n",
        "  22 /usr/local/bin/claude\n",
        "  33 node /home/u/.npm/bin/claude --flag\n",
        "  44 codex app-server\n",
        "  55 tunaround presence-scan\n",
        "  66 ps -ax\n",
    );
    use std::collections::HashSet;
    let pids = runner_pids(unix, "claude", false);
    assert_eq!(
        pids,
        [11u32, 22, 33].into_iter().collect::<HashSet<_>>(),
        "claude 프로세스 pid만: {pids:?}"
    );
    assert_eq!(
        runner_pids(unix, "codex", false),
        [44u32].into_iter().collect::<HashSet<_>>()
    );
    // win CSV: 이미지명 필드가 claude(.exe)인 행의 2번째 필드 pid만. 뒤 필드의 우연 매칭은 무시.
    let win = concat!(
        "\"claude.exe\",\"123\",\"Console\"\n",
        "\"node.exe\",\"9\",\"Console\"\n",
        "\"x.exe\",\"1\",\"claude\"\n",
        "\"CLAUDE.EXE\",\"456\",\"Console\"\n",
    );
    let pids = runner_pids(win, "claude", true);
    assert_eq!(
        pids,
        [123u32, 456].into_iter().collect::<HashSet<_>>(),
        "이미지명=claude.exe 행만(대소문자 무관): {pids:?}"
    );
}

#[test]
fn live_idle_guard1_excludes_dead_or_nonclaude_pids() {
    use std::collections::HashSet;
    let t = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(n);
    let claude_pids: HashSet<u32> = [100u32, 200].into_iter().collect();
    let markers = vec![
        ("live".to_string(), 100u32, t(10)), // 살아있는 claude pid → 유지.
        ("dead".to_string(), 999u32, t(20)), // 스냅샷에 없는 pid(죽음/비claude) → 제외(가드①).
    ];
    let keep = live_idle_marker_uuids(&markers, &claude_pids);
    assert_eq!(
        keep,
        ["live".to_string()].into_iter().collect::<HashSet<_>>(),
        "가드①: 산 claude pid만: {keep:?}"
    );
}

#[test]
fn live_idle_guard2_keeps_only_latest_marker_per_pid() {
    use std::collections::HashSet;
    let t = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(n);
    let claude_pids: HashSet<u32> = [100u32].into_iter().collect();
    // 같은 살아있는 pid를 3개 마커가 가리킴(= /clear 훅 실패로 남은 스테일 마커 유령).
    let markers = vec![
        ("stale-a".to_string(), 100u32, t(10)),
        ("newest".to_string(), 100u32, t(30)),
        ("stale-b".to_string(), 100u32, t(20)),
    ];
    let keep = live_idle_marker_uuids(&markers, &claude_pids);
    assert_eq!(
        keep,
        ["newest".to_string()].into_iter().collect::<HashSet<_>>(),
        "가드②: mtime 최신 하나만: {keep:?}"
    );
}

#[test]
fn enumerate_idle_revives_open_session_and_respects_filters() {
    use std::collections::HashSet;
    let base = std::env::temp_dir().join(format!("tuna-p8-{}", std::process::id()));
    let marker_dir = base.join("markers");
    let projects_dir = base.join("projects");
    let proj = projects_dir.join("D--privateProject-tunaRound");
    std::fs::create_dir_all(&marker_dir).unwrap();
    std::fs::create_dir_all(&proj).unwrap();
    // 유휴이지만 열려 있는 세션(마커 pid=100 산 claude, jsonl은 오래됨). 되살아나야 함.
    std::fs::write(
            proj.join("idle-1.jsonl"),
            "{\"type\":\"summary\"}\n{\"type\":\"user\",\"cwd\":\"D:\\\\privateProject\\\\tunaRound\",\"message\":{\"content\":\"안녕\"}}\n",
        ).unwrap();
    std::fs::write(marker_dir.join("idle-1.ctx"), "100").unwrap();
    // automation 세션(첫 user 메시지 <!-- 마커) - 마커 pid는 살아있어도 노이즈 필터로 제외돼야 함.
    // (각 세션은 고유 owner pid를 가진다 - 가드②는 pid별 최신 하나만 남기므로 별도 pid를 준다.)
    std::fs::write(
            proj.join("auto-1.jsonl"),
            "{\"type\":\"summary\"}\n{\"type\":\"user\",\"cwd\":\"D:\\\\privateProject\\\\tunaRound\",\"message\":{\"content\":\"<!-- secall:wiki -->\"}}\n",
        ).unwrap();
    std::fs::write(marker_dir.join("auto-1.ctx"), "200").unwrap();
    // tombstone(dead)·unknown 마커는 P8 비대상(Pid만 대상, 가드③=마커없음은 기존 창 폴백).
    std::fs::write(marker_dir.join("gone-1.ctx"), "dead").unwrap();
    std::fs::write(marker_dir.join("unk-1.ctx"), "unknown").unwrap();
    // jsonl 없는 마커(세션 파일 소멸)는 조용히 스킵.
    std::fs::write(marker_dir.join("orphan.ctx"), "300").unwrap();

    let claude_pids: HashSet<u32> = [100u32, 200, 300].into_iter().collect();
    let existing: HashSet<String> = HashSet::new();
    let found =
        enumerate_idle_marker_sessions(&marker_dir, &projects_dir, &claude_pids, &existing, None);
    let uuids: Vec<&str> = found.iter().map(|s| s.uuid.as_str()).collect();
    assert_eq!(
        uuids,
        vec!["idle-1"],
        "유휴 열림 세션만 되살림(automation·dead·unknown·orphan 제외): {uuids:?}"
    );
    assert_eq!(found[0].runner, "claude");
    assert_eq!(found[0].project.as_deref(), Some("tunaRound"));
    assert_eq!(found[0].human_input_at, None);

    // 이미 신선도 창으로 잡힌(existing) uuid는 다시 추가하지 않는다(기존 우선).
    let existing2: HashSet<String> = ["idle-1".to_string()].into_iter().collect();
    let none =
        enumerate_idle_marker_sessions(&marker_dir, &projects_dir, &claude_pids, &existing2, None);
    assert!(
        none.is_empty(),
        "existing에 있으면 중복 추가 안 함: {none:?}"
    );

    // pid가 죽으면(가드①) 되살리지 않는다.
    let dead_pids: HashSet<u32> = HashSet::new();
    let none2 =
        enumerate_idle_marker_sessions(&marker_dir, &projects_dir, &dead_pids, &existing, None);
    assert!(
        none2.is_empty(),
        "산 claude pid 없으면 되살림 없음: {none2:?}"
    );

    std::fs::remove_dir_all(&base).ok();
}

#[test]
fn enumerate_idle_empty_when_no_markers() {
    // 마커 디렉토리 자체가 없으면(가드③ 폴백 = 기존 enumerate 담당) 빈 결과.
    use std::collections::HashSet;
    let missing = std::env::temp_dir().join(format!("tuna-p8-none-{}", std::process::id()));
    let claude_pids: HashSet<u32> = [100u32].into_iter().collect();
    let existing: HashSet<String> = HashSet::new();
    let found = enumerate_idle_marker_sessions(
        &missing.join("markers"),
        &missing.join("projects"),
        &claude_pids,
        &existing,
        None,
    );
    assert!(found.is_empty());
}

/// cli_daemons.rs presence_scan 루프의 게이트 합성 순서(tombstone → codex 사람활동 게이트(#119
/// hybrid marker 포함) → 죽은 owner-pid 마커 제외 → idle 캐시 병합)를 그대로 재현하는 시나리오
/// 테스트. 개별 게이트는 각자 단위테스트가 있지만, 순서대로 합성했을 때의 누적 효과(이 단계에서
/// 뭐가 왜 빠지는지)는 무테스트였다(회귀 위험: 순서를 바꾸면 tombstone된 codex 유령이 codex
/// 게이트를 먼저 통과해버리는 식의 은닉 버그가 생길 수 있다).
#[test]
fn presence_pipeline_composes_gates_in_cli_daemons_order() {
    use std::collections::HashSet;
    let marker_dir =
        std::env::temp_dir().join(format!("tuna-pipeline-{}-{}", std::process::id(), line!()));
    std::fs::create_dir_all(&marker_dir).unwrap();
    // SessionEnd 훅이 깨끗한 종료를 기록한 세션(tombstone) → filter_tombstoned 대상.
    std::fs::write(marker_dir.join("claude-tomb.ctx"), "dead").unwrap();
    // owner pid가 기록됐으나 이번 스냅샷엔 없는(죽은) 세션 → filter_dead_sessions 대상.
    std::fs::write(marker_dir.join("claude-deadpid.ctx"), "9999").unwrap();
    // owner pid가 스냅샷에 살아있는 세션 → 끝까지 생존.
    std::fs::write(marker_dir.join("claude-live.ctx"), "100").unwrap();
    // #119: codex_wrapper.py가 남긴 tombstone - 신선한 timestamp라도 1단계에서 이미 빠져야 한다
    // (2단계 codex 게이트가 먼저 통과시켜 버리는 순서 버그를 이 케이스가 못박는다).
    std::fs::write(marker_dir.join("codex-tomb.ctx"), "dead").unwrap();
    // #119: codex_wrapper.py 래퍼가 살아있다고 기록한 세션 - human_input/created 둘 다 stale이라
    // 예전(순수 시간창) 규약이면 2단계에서 드롭됐을 것. hybrid 마커가 2단계는 면제시키고,
    // 3단계 프로세스 스냅샷(pid 200 alive)이 그 생존을 권위 판정으로 재확인한다.
    std::fs::write(marker_dir.join("codex-pid-live.ctx"), "200").unwrap();

    let threshold = "2026-07-11 09:00:00";
    let stale = Some("2026-07-11 08:00:00");
    let sessions = vec![
        LiveSession {
            uuid: "claude-tomb".into(),
            runner: "claude".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
        LiveSession {
            uuid: "claude-deadpid".into(),
            runner: "claude".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
        LiveSession {
            uuid: "claude-live".into(),
            runner: "claude".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
        // #119 codex tombstone(위 참고) - fresh timestamp인데도 1단계에서 빠져야 함.
        codex_session("codex-tomb", Some("2026-07-11 09:30:00"), None),
        // #119 marker-live codex(위 참고) - stale timestamp인데도 마커로 2단계를 통과해야 함.
        codex_session("codex-pid-live", stale, stale),
        // 유령: 마커 없음 + 사람입력·생성 둘 다 threshold 이전(codex 게이트에서 드롭돼야 함).
        codex_session(
            "codex-ghost",
            Some("2026-07-11 08:00:00"),
            Some("2026-07-11 07:00:00"),
        ),
        // 활성: 마커 없음 + 최근 사람입력(codex 게이트 통과, filter_dead_sessions은 보수적 유지).
        codex_session("codex-fresh", Some("2026-07-11 09:30:00"), None),
    ];

    // 1) tombstone 제거는 스냅샷과 무관하게 항상 먼저 적용된다(cli_daemons.rs 주석과 동일 순서,
    // claude·codex 구분 없이 uuid 마커만 본다).
    let sessions = filter_tombstoned(sessions, &marker_dir);
    let after_tomb: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
    assert!(
        !after_tomb.contains("claude-tomb") && !after_tomb.contains("codex-tomb"),
        "tombstone 세션은 claude·codex 구분 없이 1단계에서 제거돼야 함: {after_tomb:?}"
    );
    assert_eq!(
        sessions.len(),
        5,
        "tombstone 둘(claude-tomb·codex-tomb)만 빠져야 함: {after_tomb:?}"
    );

    // 2) codex 사람활동 신선도 게이트(#88) + hybrid 마커(#119) - 마커 없는 유령 codex만 드롭,
    // 마커로 살아있다 기록된 codex는 stale이어도 면제, claude는 무관.
    let sessions = apply_codex_human_input_gate(sessions, threshold, Some(&marker_dir));
    let after_codex: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
    assert!(
        !after_codex.contains("codex-ghost"),
        "마커 없는 유령 codex는 2단계에서 드롭돼야 함: {after_codex:?}"
    );
    assert!(
        after_codex.contains("codex-pid-live"),
        "마커로 생존 기록된 codex는 stale이어도 2단계를 통과해야 함(hybrid 면제): {after_codex:?}"
    );
    assert!(after_codex.contains("codex-fresh"));
    assert_eq!(sessions.len(), 4);

    // 3) 죽은 owner-pid 마커 제외(claude-deadpid는 alive 집합에 없음). codex-pid-live(200)는
    // alive 집합에 있어 2단계의 "면제"가 여기서 권위 있게 재확인된다(가짜 생존이면 여기서 빠진다).
    let alive: HashSet<u32> = [100u32, 200].into_iter().collect();
    let sessions = filter_dead_sessions(sessions, &marker_dir, &alive);
    let after_dead: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
    assert!(
        !after_dead.contains("claude-deadpid"),
        "죽은 owner pid 세션은 3단계에서 제외돼야 함: {after_dead:?}"
    );
    assert!(after_dead.contains("claude-live"));
    assert!(
        after_dead.contains("codex-pid-live"),
        "codex-pid-live는 3단계 프로세스 스냅샷으로 생존이 재확인돼야 함: {after_dead:?}"
    );
    assert!(
        after_dead.contains("codex-fresh"),
        "마커 없는 codex는 filter_dead_sessions에서 보수적으로 유지돼야 함"
    );
    assert_eq!(
        sessions.len(),
        3,
        "claude-live + codex-pid-live + codex-fresh만 남아야 함: {after_dead:?}"
    );

    // 4) idle 캐시 병합(cli_daemons.rs 마지막 단계): 이번 주기에 이미 있는 uuid는 idle 사본으로
    // 덮이지 않고, 새 uuid만 추가된다.
    let mut sessions = sessions;
    let last_idle = vec![
        LiveSession {
            uuid: "idle-revived".into(),
            runner: "claude".into(),
            project: None,
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
        // 이미 존재하는 uuid의 스테일 idle 사본 - 병합 시 무시돼야 한다(existing 우선).
        LiveSession {
            uuid: "claude-live".into(),
            runner: "claude".into(),
            project: Some("stale-idle-copy".into()),
            human_input_at: None,
            created_at: None,
            active_at: None,
        },
    ];
    let present: HashSet<String> = sessions.iter().map(|s| s.uuid.clone()).collect();
    sessions.extend(last_idle.into_iter().filter(|s| !present.contains(&s.uuid)));

    let mut final_uuids: Vec<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
    final_uuids.sort_unstable();
    assert_eq!(
        final_uuids,
        vec![
            "claude-live",
            "codex-fresh",
            "codex-pid-live",
            "idle-revived"
        ]
    );
    let claude_live = sessions.iter().find(|s| s.uuid == "claude-live").unwrap();
    assert_eq!(
        claude_live.project, None,
        "이미 존재하는 세션이 idle 사본으로 덮이면 안 됨(existing 우선)"
    );

    std::fs::remove_dir_all(&marker_dir).ok();
}
