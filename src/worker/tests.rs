// worker 모듈 테스트(#138 B 분해로 파일 이동, 내용 순수 이동): 파싱·가드·루프 단위 + run_one_pass 통합.

use super::*;

#[test]
fn parse_open_tasks_empty_message_returns_empty_vec() {
    let tasks = parse_open_tasks("mac-claude 앞 열린 task 없음");
    assert!(tasks.is_empty());
}

#[test]
fn parse_open_tasks_single_task() {
    let id = "a".repeat(32);
    let text = format!("[{id}] from=win-claude state=submitted msg=리뷰 부탁");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].state, "submitted");
    assert_eq!(tasks[0].msg, "리뷰 부탁");
}

#[test]
fn parse_open_tasks_multiple_tasks_joined_by_blank_line() {
    let id1 = "1".repeat(32);
    let id2 = "2".repeat(32);
    let text = format!(
        "[{id1}] from=win-claude state=submitted msg=첫 task\n\n[{id2}] from=win-claude state=working msg=둘째 task"
    );
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, id1);
    assert_eq!(tasks[0].msg, "첫 task");
    assert_eq!(tasks[1].id, id2);
    assert_eq!(tasks[1].state, "working");
    assert_eq!(tasks[1].msg, "둘째 task");
}

#[test]
fn parse_open_tasks_msg_with_embedded_newlines() {
    let id = "3".repeat(32);
    let text = format!("[{id}] from=win-claude state=submitted msg=1번\n2번\n\n3번(빈 줄 포함)");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].msg, "1번\n2번\n\n3번(빈 줄 포함)");
}

#[test]
fn parse_open_tasks_korean_bracket_after_blank_line_does_not_panic() {
    // 회귀: 본문에 "\n\n[작업] <한글>"이 오면 find_header_starts가 '[' 뒤 32바이트를
    // 슬라이스하다 멀티바이트 경계('따', bytes 31..34)에서 패닉하던 버그(worker.rs).
    // 실측 크래시: mac-claude-sup poll이 위임 task 미리보기 중 exit 101.
    let id = "a".repeat(32);
    let body = "[통지] 규약 안내\n\n[작업] mac에서 사용자가 따로 띄운 세션들이 안 뜬다";
    let text = format!("[{id}] from=win-opus-boss state=submitted msg={body}");
    let tasks = parse_open_tasks(&text); // 패닉하지 않아야 한다
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].msg, body);
}

#[test]
fn parse_open_tasks_submitted_and_working_mixed() {
    let id1 = "4".repeat(32);
    let id2 = "5".repeat(32);
    let id3 = "6".repeat(32);
    let text = format!(
        "[{id1}] from=a state=submitted msg=하나\n\n[{id2}] from=a state=working msg=둘\n\n[{id3}] from=a state=submitted msg=셋"
    );
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 3);
    let submitted: Vec<_> = tasks.iter().filter(|t| t.state == "submitted").collect();
    assert_eq!(submitted.len(), 2);
    assert_eq!(submitted[0].id, id1);
    assert_eq!(submitted[1].id, id3);
}

#[test]
fn parse_open_tasks_extracts_context_id() {
    let id = "7".repeat(32);
    let text = format!("[{id}] from=disp state=submitted ctx=projA msg=작업 지시");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].context_id.as_deref(), Some("projA"));
    assert_eq!(tasks[0].state, "submitted");
    assert_eq!(tasks[0].msg, "작업 지시");
}

#[test]
fn parse_open_tasks_strips_health_annotation_from_state() {
    // 코어가 poll 출력에 붙이는 표시 전용 주석(⚠stuck?/⚠no-consumer?)이 있어도 워커는 state를
    // 깨끗한 "submitted"/"working"으로 인식해야 한다(그러지 않으면 no-consumer task를 못 집는 회귀).
    let id1 = "a".repeat(32);
    let id2 = "b".repeat(32);
    let text = format!(
        "[{id1}] from=disp state=submitted ⚠no-consumer?(10m) ctx=projA msg=오래된 작업\n\n[{id2}] from=disp state=working ⚠stuck?(20m) ctx=- msg=멈춘 작업"
    );
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 2);
    assert_eq!(
        tasks[0].state, "submitted",
        "no-consumer 주석이 state를 오염시킴: {:?}",
        tasks[0].state
    );
    assert_eq!(tasks[0].context_id.as_deref(), Some("projA"));
    assert_eq!(tasks[0].msg, "오래된 작업");
    assert_eq!(
        tasks[1].state, "working",
        "stuck 주석이 state를 오염시킴: {:?}",
        tasks[1].state
    );
    assert_eq!(tasks[1].context_id, None);
    assert_eq!(tasks[1].msg, "멈춘 작업");
}

#[test]
fn parse_open_tasks_ctx_dash_is_none() {
    let id = "8".repeat(32);
    let text = format!("[{id}] from=disp state=submitted ctx=- msg=작업");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].context_id, None);
    assert_eq!(tasks[0].msg, "작업");
}

#[test]
fn parse_open_tasks_no_ctx_marker_is_backward_compatible() {
    // 구 포맷(ctx= 없음)도 context_id=None으로 그대로 파싱된다.
    let id = "9".repeat(32);
    let text = format!("[{id}] from=disp state=submitted msg=구포맷");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].context_id, None);
    assert_eq!(tasks[0].msg, "구포맷");
}

#[test]
fn parse_open_tasks_prefers_json_prefix_over_string_blocks() {
    // v2-52 ④: 신 브로커 출력(JSON 프리픽스 + human 블록)에서 워커는 JSON을 우선 파싱한다(견고).
    use crate::a2a_wire::{PollTaskDto, encode_poll_json};
    let id1 = "a".repeat(32);
    let id2 = "b".repeat(32);
    let dtos = vec![
        PollTaskDto {
            id: id1.clone(),
            state: "submitted".into(),
            context_id: Some("projA".into()),
            msg: "리뷰\n\n부탁".into(), // 개행 포함 - JSON 경로라 무손실.
        },
        PollTaskDto {
            id: id2.clone(),
            state: "working".into(),
            context_id: None,
            msg: "진행".into(),
        },
    ];
    let full = format!(
        "{}\n\n[{id1}] from=x state=submitted ctx=projA msg=리뷰\n\n부탁\n\n[{id2}] from=x state=working ctx=- msg=진행",
        encode_poll_json(&dtos).unwrap()
    );
    let tasks = parse_open_tasks(&full);
    assert_eq!(tasks.len(), 2);
    assert_eq!(tasks[0].id, id1);
    assert_eq!(tasks[0].state, "submitted");
    assert_eq!(tasks[0].context_id.as_deref(), Some("projA"));
    assert_eq!(tasks[0].msg, "리뷰\n\n부탁"); // JSON 경로라 msg 개행 무손실.
    assert_eq!(tasks[1].id, id2);
    assert_eq!(tasks[1].state, "working");
    assert_eq!(tasks[1].context_id, None);
}

#[test]
fn parse_open_tasks_falls_back_to_string_when_no_json_prefix() {
    // 구 브로커(프리픽스 없음): 기존 문자열 파싱 경로로 폴백해 하위호환 유지.
    let id = "a".repeat(32);
    let text = format!("[{id}] from=win state=submitted ctx=projB msg=구포맷");
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id, id);
    assert_eq!(tasks[0].context_id.as_deref(), Some("projB"));
    assert_eq!(tasks[0].msg, "구포맷");
}

#[test]
fn parse_open_tasks_json_path_rejects_non_hex32_id() {
    // TASKS_JSON 경로도 문자열 경로(find_header_starts)와 동일하게 id를 hex32로 강제해야 한다.
    // run_on_task가 {id}를 셸 명령에 직접 치환하므로, 검증 없이 통과하면 셸 인젝션 벡터가 된다.
    use crate::a2a_wire::{PollTaskDto, encode_poll_json};
    let good_id = "a".repeat(32);
    let dtos = vec![
        PollTaskDto {
            id: good_id.clone(),
            state: "submitted".into(),
            context_id: None,
            msg: "정상".into(),
        },
        PollTaskDto {
            id: "; rm -rf / #".into(), // hex32 아님(길이·문자 모두 불일치).
            state: "submitted".into(),
            context_id: None,
            msg: "위험".into(),
        },
    ];
    let text = encode_poll_json(&dtos).unwrap();
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1, "hex32 아닌 id는 걸러져야 함: {tasks:?}");
    assert_eq!(tasks[0].id, good_id);
}

#[test]
fn parse_open_tasks_msg_containing_empty_phrase_does_not_hide_task() {
    // 회귀 방지: msg 본문에 "...앞 열린 task 없음..."이 포함돼도, 실제 헤더가 있으면(문자열
    // 폴백 경로) 그 task는 정상 파싱돼야 한다(과거엔 contains 검사가 헤더 스캔보다 앞서
    // 전체를 빈 Vec으로 조기 반환했다).
    let id = "d".repeat(32);
    let text = format!(
        "[{id}] from=disp state=submitted msg=참고: mac-claude 앞 열린 task 없음 이라고 뜨면 재시도하세요"
    );
    let tasks = parse_open_tasks(&text);
    assert_eq!(tasks.len(), 1, "헤더가 있으면 은닉되면 안 됨: {tasks:?}");
    assert_eq!(tasks[0].id, id);

    // 대조: 진짜 빈 응답(헤더 없음)은 여전히 빈 Vec.
    assert!(parse_open_tasks("mac-claude 앞 열린 task 없음").is_empty());
}

#[test]
fn old_worker_header_scan_ignores_json_prefix() {
    // 하위호환 핵심: 구 워커(find_header_starts만 가진 옛 바이너리)는 JSON 프리픽스를 첫 헤더 앞이라
    // 무시하고 human 블록 헤더만 찾는다. compact JSON은 실개행 없음(msg 내 개행도 이스케이프)이라
    // 그 안에서 거짓 헤더가 생기지 않음을 검증(신 브로커 출력에서 헤더는 정확히 human 블록 수만큼).
    // 이 테스트는 프리픽스 안전성만 잠근다: human 블록 자체의 msg 내 거짓 헤더는 이 PR이 안 바꾼
    // 문자열 포맷의 pre-existing 성질(회귀 아님)이라 스코프 밖이다.
    use crate::a2a_wire::{PollTaskDto, encode_poll_json};
    let id1 = "a".repeat(32);
    let id2 = "b".repeat(32);
    let hexish = "c".repeat(32);
    let dtos = vec![
        PollTaskDto {
            id: id1.clone(),
            // msg에 개행+가짜 헤더 유사 문자열을 넣어도 JSON 이스케이프로 거짓 헤더가 안 생겨야 한다.
            state: "submitted".into(),
            context_id: None,
            msg: format!("본문\n\n[{hexish}] from=fake state=submitted msg=가짜"),
        },
        PollTaskDto {
            id: id2.clone(),
            state: "working".into(),
            context_id: None,
            msg: "둘째".into(),
        },
    ];
    let full = format!(
        "{}\n\n[{id1}] from=x state=submitted ctx=- msg=본문\n\n[{id2}] from=x state=working ctx=- msg=둘째",
        encode_poll_json(&dtos).unwrap()
    );
    let starts = find_header_starts(&full);
    assert_eq!(
        starts.len(),
        2,
        "JSON 프리픽스(가짜 헤더 포함)에서 거짓 헤더를 만들면 안 됨(구 워커 안전): {starts:?}"
    );
}

#[test]
fn collect_new_submitted_dedups_across_polls() {
    // 같은 submitted task가 두 폴에 걸쳐 나와도 처음 1회만 반환된다(재출력 금지).
    let id = "a".repeat(32);
    let text = format!("[{id}] from=disp state=submitted msg=한 번만 알림");
    let mut seen = std::collections::HashSet::new();
    let first = collect_new_submitted(&text, &mut seen);
    assert_eq!(first.len(), 1, "첫 폴은 새 task 반환");
    assert_eq!(first[0].id, id);
    let second = collect_new_submitted(&text, &mut seen);
    assert!(second.is_empty(), "여전히 submitted면 재알림 안 함");
}

#[test]
fn collect_new_submitted_prunes_disappeared_ids_and_realerts_on_return() {
    // claim/완료로 사라진 id는 seen에서 정리되고(메모리 상한), 다시 submitted로 나타나면 재알림된다.
    let id = "b".repeat(32);
    let present = format!("[{id}] from=disp state=submitted msg=작업");
    let empty = "someone 앞 열린 task 없음".to_string();
    let mut seen = std::collections::HashSet::new();
    assert_eq!(collect_new_submitted(&present, &mut seen).len(), 1);
    // task가 사라진 폴 -> seen이 비워진다(무한 증가 방지).
    assert!(collect_new_submitted(&empty, &mut seen).is_empty());
    assert!(seen.is_empty(), "사라진 id는 seen에서 제거되어야 함");
    // 같은 id가 다시 submitted -> 재알림.
    assert_eq!(
        collect_new_submitted(&present, &mut seen).len(),
        1,
        "재등장 시 다시 알림"
    );
}

#[test]
fn collect_new_submitted_ignores_non_submitted() {
    // working 등 non-submitted는 알림 대상 아님(seen에도 안 들어감).
    let id = "c".repeat(32);
    let text = format!("[{id}] from=disp state=working msg=진행중");
    let mut seen = std::collections::HashSet::new();
    assert!(collect_new_submitted(&text, &mut seen).is_empty());
    assert!(seen.is_empty());
}

#[test]
fn collect_new_submitted_preserves_seen_on_soft_error_text() {
    // 구 브로커가 poll_tasks 내부 조회 실패를 success 본문 "조회 실패: ..." 텍스트로 반환해도
    // (헤더도 빈-큐 마커도 없음) seen이 비워지면 안 된다(비워지면 다음 정상 폴에서 재알림 발생).
    let id = "d".repeat(32);
    let present = format!("[{id}] from=disp state=submitted msg=작업");
    let soft_error = "조회 실패: database is locked".to_string();
    let mut seen = std::collections::HashSet::new();
    assert_eq!(collect_new_submitted(&present, &mut seen).len(), 1);
    assert!(
        collect_new_submitted(&soft_error, &mut seen).is_empty(),
        "소프트에러 텍스트는 새 task 없음"
    );
    assert!(
        seen.contains(&id),
        "소프트에러로 seen이 비워지면 안 됨(재알림 방지): {seen:?}"
    );
    // 이어서 진짜 빈 큐 마커가 오면 정상적으로 seen이 정리된다.
    let empty = "disp 앞 열린 task 없음".to_string();
    assert!(collect_new_submitted(&empty, &mut seen).is_empty());
    assert!(seen.is_empty(), "진짜 빈 큐는 seen을 정리해야 함");
}

// --- 이슈 #147 Stage 1: 좌석(seat) 수신함 이중폴 ---

#[test]
fn format_task_notification_default_matches_legacy_format() {
    // via=None(기본 agent 경로)은 --also-agent 도입 전 리터럴 포맷("TASK {id} :: {preview}")과
    // 완전히 동일해야 한다(하위호환 계약: also 비면 출력이 한 글자도 안 바뀜).
    let id = "a".repeat(32);
    assert_eq!(
        format_task_notification(&id, "미리보기", None),
        format!("TASK {id} :: 미리보기")
    );
}

#[test]
fn format_task_notification_via_appends_address() {
    // via가 있으면(좌석 mbox 경로) 어느 주소로 왔는지 줄 끝에 표기하되, prefix("TASK {id} ::")는
    // 그대로 유지한다(Monitor·#136 수신 규약이 이 prefix로 이벤트를 인식).
    let id = "b".repeat(32);
    let line =
        format_task_notification(&id, "좌석 도착", Some("mbox:machine=win,project=tunaRound"));
    assert!(
        line.starts_with(&format!("TASK {id} :: 좌석 도착")),
        "prefix 보존 안 됨: {line}"
    );
    assert!(
        line.contains("mbox:machine=win,project=tunaRound"),
        "주소 표기 누락: {line}"
    );
}

#[test]
fn dual_poll_addresses_use_independent_seen_sets() {
    // 이중폴은 주소별 독립 seen을 써야 한다. 공유 seen을 오용하면 한 주소의 poll이 다른 주소의
    // 이미-알린 id를 collect_new_submitted의 retain()에서 지워, 아직 claim 안 된 task가 매 주기
    // 재알림되는 버그가 생긴다(원 사건: 유령 poll의 무장 없는 재등록이 로스터를 덮던 것과 같은 급의
    // "조용한 재발" 클래스). run_poll_loop는 also_agents마다 별도 HashSet을 두어 이를 피한다.
    let id_primary = "1".repeat(32);
    let id_mbox = "2".repeat(32);
    let primary_text = format!("[{id_primary}] from=disp state=submitted msg=기본 큐");
    let mbox_text = format!("[{id_mbox}] from=disp state=submitted msg=좌석 큐");

    // 설계대로: 주소별 독립 seen.
    let mut primary_seen = std::collections::HashSet::new();
    let mut mbox_seen = std::collections::HashSet::new();
    assert_eq!(
        collect_new_submitted(&primary_text, &mut primary_seen).len(),
        1
    );
    assert_eq!(collect_new_submitted(&mbox_text, &mut mbox_seen).len(), 1);
    // 다음 주기: 둘 다 여전히 submitted(=미claim)지만 독립 seen 덕에 재알림 없음.
    assert!(collect_new_submitted(&primary_text, &mut primary_seen).is_empty());
    assert!(collect_new_submitted(&mbox_text, &mut mbox_seen).is_empty());

    // 대조(회귀 문서화): 하나의 공유 seen을 오용하면 무슨 일이 나는지.
    let mut shared_seen = std::collections::HashSet::new();
    assert_eq!(
        collect_new_submitted(&primary_text, &mut shared_seen).len(),
        1
    );
    assert_eq!(
        collect_new_submitted(&mbox_text, &mut shared_seen).len(),
        1,
        "mbox 자신의 새 task는 공유 seen이라도 정상 포착됨"
    );
    assert_eq!(
        collect_new_submitted(&primary_text, &mut shared_seen).len(),
        1,
        "버그 재현: 공유 seen을 쓰면 mbox 폴의 active 집합(id_mbox만)이 retain()에서 \
         id_primary를 지워 다음 primary 폴에서 재알림된다 - 그래서 run_poll_loop는 주소별 \
         독립 seen을 쓴다"
    );
}

#[test]
fn resolve_project_path_uses_map_then_falls_back() {
    let mut map = std::collections::HashMap::new();
    map.insert("projA".to_string(), "/repos/A".to_string());
    // 매핑에 있으면 그 경로.
    assert_eq!(
        resolve_project_path(Some("projA"), &map, Some("/default")),
        Some("/repos/A".to_string())
    );
    // context_id가 매핑에 없으면 기본값.
    assert_eq!(
        resolve_project_path(Some("projX"), &map, Some("/default")),
        Some("/default".to_string())
    );
    // context_id 자체가 없으면 기본값.
    assert_eq!(
        resolve_project_path(None, &map, Some("/default")),
        Some("/default".to_string())
    );
    // 매핑도 기본값도 없으면 None.
    assert_eq!(resolve_project_path(Some("projX"), &map, None), None);
}

#[test]
fn parse_context_map_valid_entries() {
    let m = parse_context_map("projA=/repos/A, projB=/repos/B").unwrap();
    assert_eq!(m.get("projA").map(String::as_str), Some("/repos/A"));
    assert_eq!(m.get("projB").map(String::as_str), Some("/repos/B"));
    assert_eq!(m.len(), 2);
}

#[test]
fn parse_context_map_trailing_comma_ok() {
    // 완전히 빈 항목(후행 콤마)만 무해하게 스킵한다.
    let m = parse_context_map("projA=/repos/A,").unwrap();
    assert_eq!(m.len(), 1);
}

#[test]
fn parse_context_map_rejects_malformed_entry() {
    // '=' 없는 오타 항목은 조용히 버리지 않고 거부한다(기본 레포 오폴백 방지).
    assert!(parse_context_map("projA=/repos/A,badentry").is_err());
}

#[test]
fn parse_context_map_rejects_empty_key_or_value() {
    assert!(parse_context_map("=/repos/A").is_err());
    assert!(parse_context_map("projA=").is_err());
}

#[test]
fn parse_context_map_rejects_duplicate_key() {
    assert!(parse_context_map("projA=/x,projA=/y").is_err());
}

#[test]
fn substitute_task_placeholders_replaces_id_only() {
    let out = substitute_task_placeholders("codex exec resume --last \"task {id} 처리\"", "abc123");
    assert_eq!(out, "codex exec resume --last \"task abc123 처리\"");
    // {id}가 여러 번 나와도 모두 치환.
    assert_eq!(substitute_task_placeholders("{id}-{id}", "x"), "x-x");
    // {id}가 없으면 그대로(msg는 셸에 치환하지 않으므로 {msg}는 남는다 = env로 전달 전제).
    assert_eq!(substitute_task_placeholders("run {msg}", "x"), "run {msg}");
}

#[test]
fn paths_overlap_detects_equal_and_ancestry_not_siblings() {
    use std::path::Path;
    // 같은 경로.
    assert!(paths_overlap(Path::new("/repo"), Path::new("/repo")));
    // 조상-자손 양방향.
    assert!(paths_overlap(Path::new("/repo/sub"), Path::new("/repo")));
    assert!(paths_overlap(Path::new("/repo"), Path::new("/repo/sub")));
    // 완전 분리.
    assert!(!paths_overlap(Path::new("/repo"), Path::new("/other")));
    // 컴포넌트 단위라 문자열 접두(/repo vs /repo2)는 겹침 아님.
    assert!(!paths_overlap(Path::new("/repo"), Path::new("/repo2")));
}

#[test]
fn write_lane_disrupts_node_none_project_is_dangerous() {
    // 작업 디렉터리 미지정 = node cwd에서 write = self-disruption 위험.
    let cwd = std::env::current_dir().unwrap();
    assert!(write_lane_disrupts_node(None, &cwd));
}

#[test]
fn write_lane_disrupts_node_same_as_cwd_is_dangerous() {
    let cwd = std::env::current_dir().unwrap();
    assert!(write_lane_disrupts_node(Some(&cwd), &cwd));
}

#[test]
fn write_lane_disrupts_node_nonexistent_under_cwd_is_dangerous() {
    // 아직 없는 경로라도 cwd 하위면 위험(gemini 리뷰: 러너가 실행 중 생성 후 self-disruption 여지).
    let cwd = std::env::current_dir().unwrap();
    let missing = cwd.join("이_경로는_존재하지_않음_zzz");
    assert!(write_lane_disrupts_node(Some(&missing), &cwd));
}

#[test]
fn write_lane_disrupts_node_nonexistent_disjoint_is_safe() {
    // cwd와 완전히 분리된(조상/자손 아닌) 미존재 절대경로는 안전.
    let cwd = std::env::current_dir().unwrap();
    // cwd의 부모 밑 형제 경로(cwd 하위가 아님)를 미존재로 만든다.
    let parent = cwd.parent().unwrap_or(&cwd);
    let sibling = parent.join("tunaround_없는_형제_zzz");
    // 방어: 극히 드물게 sibling이 cwd와 겹치면(동일 이름 등) 이 단정은 건너뛴다.
    if !normalize_lexically(&sibling, &cwd).starts_with(normalize_lexically(&cwd, &cwd)) {
        assert!(!write_lane_disrupts_node(Some(&sibling), &cwd));
    }
}

#[cfg(windows)]
#[test]
fn write_lane_disrupts_node_windows_case_insensitive_overlap() {
    // Windows는 대소문자를 구분하지 않으므로, 어휘 정규화 폴백(경로가 존재하지 않아 canonicalize
    // 실패)에서도 대소문자만 다른 겹침(d:\... vs D:\...)을 잡아야 한다.
    let cwd = std::path::PathBuf::from("D:\\repo_없는_zzz\\node");
    let project = std::path::PathBuf::from("d:\\repo_없는_zzz\\node\\sub");
    assert!(
        write_lane_disrupts_node(Some(&project), &cwd),
        "대소문자만 다른 겹침 경로를 못 잡음"
    );
}

#[test]
fn context_map_disrupting_paths_flags_only_overlapping_values() {
    let cwd = std::env::current_dir().unwrap();
    let tmp = std::env::temp_dir();
    let mut map = std::collections::HashMap::new();
    // 안전(cwd와 분리): temp_dir. 위험(cwd 자체): node cwd 그대로.
    map.insert("safe".to_string(), tmp.to_string_lossy().to_string());
    map.insert("danger".to_string(), cwd.to_string_lossy().to_string());
    // 방어: 극히 드문 환경에서 temp가 cwd와 겹치면 이 단정은 건너뛴다(오탐 방지, 기존 테스트와 동일 패턴).
    if !paths_overlap(
        &std::fs::canonicalize(&tmp).unwrap_or_else(|_| tmp.clone()),
        &std::fs::canonicalize(&cwd).unwrap_or_else(|_| cwd.clone()),
    ) {
        let bad = context_map_disrupting_paths(&map, &cwd);
        assert_eq!(bad.len(), 1, "겹치는 value 하나만 걸려야 함: {bad:?}");
        assert!(
            bad[0].starts_with("danger="),
            "겹치는 key가 danger여야 함: {bad:?}"
        );
    }
}

#[test]
fn context_map_disrupting_paths_empty_map_is_safe() {
    let cwd = std::env::current_dir().unwrap();
    let map = std::collections::HashMap::new();
    assert!(context_map_disrupting_paths(&map, &cwd).is_empty());
}

#[test]
fn normalize_lexically_resolves_dot_and_dotdot() {
    use std::path::Path;
    let base = Path::new("/home/user/repo");
    // 상대경로는 base에 이어붙는다.
    assert_eq!(
        normalize_lexically(Path::new("sub"), base),
        Path::new("/home/user/repo/sub")
    );
    // `.`은 무시, `..`은 pop.
    assert_eq!(
        normalize_lexically(Path::new("./a/../b"), base),
        Path::new("/home/user/repo/b")
    );
    // 절대경로는 base 무시.
    assert_eq!(
        normalize_lexically(Path::new("/x/y/../z"), base),
        Path::new("/x/z")
    );
}

#[test]
fn write_lane_disrupts_node_disjoint_existing_dir_is_safe() {
    // temp_dir는 보통 cwd(레포)와 분리된 실재 디렉터리라 안전.
    let cwd = std::env::current_dir().unwrap();
    let tmp = std::env::temp_dir();
    // 방어: 극히 드문 환경에서 temp가 cwd 하위/상위면 이 단정은 건너뛴다(오탐 아님을 보장 못 함).
    if !paths_overlap(
        &std::fs::canonicalize(&tmp).unwrap_or(tmp.clone()),
        &std::fs::canonicalize(&cwd).unwrap_or(cwd.clone()),
    ) {
        assert!(!write_lane_disrupts_node(Some(&tmp), &cwd));
    }
}

#[test]
fn write_lane_disrupts_node_relative_project_resolves_against_node_cwd_not_process_cwd() {
    // #138 C-3 회귀: project가 상대경로면 이 프로세스의 CWD가 아니라 인자로 받은 node_cwd 기준으로
    // 판정해야 한다. node_cwd를 프로세스 CWD와 분리된 실재 temp 디렉터리로 두고, project="."(자기
    // 자신)을 넘기면 node_cwd 기준으로는 node_cwd 자신과 겹쳐 위험이어야 한다.
    //
    // 수정 전에는 write_lane_disrupts_node 내부가 std::fs::canonicalize(p)를 그대로 호출해 "."을
    // 이 테스트 프로세스의 실제 CWD(레포 루트) 기준으로 풀었다 - node_cwd(temp)와는 무관한 경로가
    // 나와 겹침이 감지되지 않고 거짓으로 안전 판정됐다(미탐). CWD를 실제로 바꾸지 않고 node_cwd를
    // 인자로만 다르게 넘겨 재현한다(함수 시그니처가 node_cwd를 받으므로 std::env::set_current_dir
    // 불필요).
    let process_cwd = std::env::current_dir().unwrap();
    let node_cwd =
        std::env::temp_dir().join(format!("tuna-worker-guard-nodecwd-{}", std::process::id()));
    // 생성 실패를 삼키면 canonicalize Err 폴백(어휘 정규화) 경로로 새서 테스트 의도(실경로 실측)가
    // 무너진다 - 명시 실패(gemini 리뷰 반영).
    std::fs::create_dir_all(&node_cwd).expect("테스트용 node_cwd temp 디렉터리 생성 실패");

    // 방어: 극히 드문 환경에서 temp node_cwd가 프로세스 CWD와 같은 트리에 놓이면(예: TMPDIR이 레포
    // 하위) 이 테스트의 전제(둘이 분리돼야 실측이 됨)가 깨지므로 건너뛴다(기존 테스트들과 동일 패턴).
    let node_cwd_canon = std::fs::canonicalize(&node_cwd).unwrap_or_else(|_| node_cwd.clone());
    let process_cwd_canon =
        std::fs::canonicalize(&process_cwd).unwrap_or_else(|_| process_cwd.clone());
    if !paths_overlap(&node_cwd_canon, &process_cwd_canon) {
        assert!(
            write_lane_disrupts_node(Some(std::path::Path::new(".")), &node_cwd),
            "node_cwd와 다른 프로세스 CWD에서 상대경로 project('.')는 node_cwd 자신과 겹쳐 \
             위험(true)으로 판정돼야 함"
        );
    }

    let _ = std::fs::remove_dir(&node_cwd);
}

#[test]
fn generate_agent_uuid_is_32_hex() {
    let id = generate_agent_uuid();
    assert_eq!(id.len(), 32);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn needs_reregister_detects_missing_registration() {
    assert!(needs_reregister(
        "미등록 uuid=x(register_agent 먼저 호출하세요)"
    ));
    assert!(!needs_reregister("heartbeat 갱신: x"));
}

// --- 이슈 #118: 세션 마커 종료 판정 ---

#[test]
fn marker_gone_none_is_gone() {
    // 마커 부재(파일 삭제) = 종료. tuna-disarm.py는 보통 삭제 대신 tombstone("dead")을
    // 남기지만, 파일 자체가 없는 경로(수동 삭제·GC)도 방어적으로 종료로 본다.
    assert!(marker_gone(None));
}

#[test]
fn marker_gone_dead_content_is_gone() {
    // SessionEnd 훅(tuna-disarm.py)의 tombstone 규약.
    assert!(marker_gone(Some("dead")));
    // 개행·공백이 섞여도 trim 후 판정(파일 쓰기 관례상 개행이 붙을 수 있음).
    assert!(marker_gone(Some("dead\n")));
    assert!(marker_gone(Some("  dead  ")));
}

#[test]
fn marker_gone_pid_content_is_alive() {
    // owner PID 숫자 = 세션이 살아 있음(스캐너의 per-session 생존 판정과 동일 규약).
    assert!(!marker_gone(Some("12345")));
}

#[test]
fn marker_gone_unknown_content_is_alive() {
    // owner 탐색 실패 sentinel "unknown"은 죽음을 의미하지 않는다(보수적 유지).
    assert!(!marker_gone(Some("unknown")));
}

#[test]
fn session_marker_terminated_reads_fs_and_matches_marker_gone() {
    let dir = std::env::temp_dir().join(format!("tuna-worker-marker-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);

    // 파일 없음 = 종료.
    let missing = dir.join("missing.ctx");
    let _ = std::fs::remove_file(&missing);
    assert!(session_marker_terminated(&missing));

    // dead tombstone = 종료.
    let dead = dir.join("dead.ctx");
    std::fs::write(&dead, "dead").unwrap();
    assert!(session_marker_terminated(&dead));

    // PID 내용 = 생존.
    let alive = dir.join("alive.ctx");
    std::fs::write(&alive, "98765").unwrap();
    assert!(!session_marker_terminated(&alive));

    let _ = std::fs::remove_file(&dead);
    let _ = std::fs::remove_file(&alive);
    let _ = std::fs::remove_dir(&dir);
}

// run_one_pass(run_worker_loop once=true 경유)의 claim->실행->complete/fail 계약 검증.
// in-process MCP 서버(serve_http_mcp_on_listener) + in-memory SqliteStore + 호출 기록 러너로
// fail-visible 불변식(러너 성공=complete, 러너 실패=fail_task, completed 위장 금지)을 확인한다.
#[cfg(feature = "serve")]
mod run_one_pass_integration {
    use super::*;
    use crate::store::a2a::{Message, Part, TaskState};
    use crate::store::sqlite::SqliteStore;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// 테스트 전용 빈 retriever(mcp_client.rs 테스트의 NullRetriever와 동등).
    struct NullRetriever;
    impl crate::orchestrator::ContextRetriever for NullRetriever {
        fn retrieve(
            &self,
            _q: &str,
            _limit: usize,
        ) -> Result<Vec<crate::orchestrator::Utterance>, String> {
            Ok(vec![])
        }
    }

    fn test_store() -> Arc<Mutex<SqliteStore>> {
        Arc::new(Mutex::new(
            SqliteStore::open_memory().expect("in-memory sqlite"),
        ))
    }

    /// ephemeral 포트로 HTTP MCP 서버를 띄우고 그 base URL을 반환한다(mcp_client.rs 테스트와 동일 관례).
    async fn spawn_test_server(store: Arc<Mutex<SqliteStore>>) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().unwrap().port();
        let retriever = Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
        tokio::spawn(async move {
            let _ = crate::mcp::serve_http_mcp_on_listener(
                listener, retriever, None, None, None, None, store,
            )
            .await;
        });
        tokio::time::sleep(Duration::from_millis(120)).await;
        format!("http://127.0.0.1:{port}/mcp")
    }

    /// 러너 실행 여부/횟수를 기록하며 고정 결과(Ok/Err)를 내는 가짜 러너.
    enum FakeResult {
        Ok(String),
        Err(String),
    }
    struct RecordingRunner {
        calls: Arc<AtomicUsize>,
        result: FakeResult,
    }
    impl Runner for RecordingRunner {
        fn run(
            &self,
            _input: &RunInput,
        ) -> Result<crate::runner::RunOutput, crate::runner::RunError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match &self.result {
                FakeResult::Ok(s) => Ok(crate::runner::RunOutput {
                    content: s.clone(),
                    input_tokens: 0,
                    output_tokens: 0,
                }),
                FakeResult::Err(e) => Err(crate::runner::RunError::Agent(e.clone())),
            }
        }
    }

    /// store에 to_agent 앞 submitted task를 하나 만들고 task_id를 반환한다.
    fn make_submitted_task(
        store: &Arc<Mutex<SqliteStore>>,
        from: &str,
        to: &str,
        text: &str,
    ) -> String {
        let guard = store.lock().unwrap();
        let msg = Message {
            message_id: guard.new_task_id().unwrap(),
            role: "user".to_string(),
            parts: vec![Part {
                text: Some(text.to_string()),
                ..Default::default()
            }],
            task_id: None,
            context_id: None,
        };
        let task = guard.create_task_from_message(from, to, msg).unwrap();
        task.id
    }

    #[tokio::test]
    async fn runner_ok_marks_task_completed_with_artifact() {
        let store = test_store();
        let task_id = make_submitted_task(&store, "dispatcher", "worker-a", "테스트 지시");
        let url = spawn_test_server(store.clone()).await;
        let client = McpHttpClient::connect(url, None).await.expect("connect");

        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn Runner + Send + Sync> = Arc::new(RecordingRunner {
            calls: calls.clone(),
            result: FakeResult::Ok("완료 결과".to_string()),
        });

        run_worker_loop(
            &client,
            runner,
            "worker-a",
            "fake-runner",
            None,
            None,
            None,
            std::collections::HashMap::new(),
            crate::runner::RunMode::ReadOnly,
            1,
            true,
        )
        .await
        .expect("once 패스는 정상 반환");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "러너가 정확히 1회 실행돼야 함"
        );

        let task = store
            .lock()
            .unwrap()
            .get_task(&task_id)
            .unwrap()
            .expect("task 존재해야 함");
        assert_eq!(
            task.state,
            TaskState::Completed,
            "러너 성공은 completed로 전이"
        );
        assert_eq!(task.artifacts.len(), 1, "완료 산출물이 있어야 함");
        assert_eq!(
            task.artifacts[0].parts[0].text.as_deref(),
            Some("완료 결과")
        );
    }

    #[tokio::test]
    async fn runner_err_marks_task_failed_with_reason_not_completed() {
        let store = test_store();
        let task_id = make_submitted_task(&store, "dispatcher", "worker-b", "실패할 지시");
        let url = spawn_test_server(store.clone()).await;
        let client = McpHttpClient::connect(url, None).await.expect("connect");

        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn Runner + Send + Sync> = Arc::new(RecordingRunner {
            calls: calls.clone(),
            result: FakeResult::Err("모의 실패".to_string()),
        });

        run_worker_loop(
            &client,
            runner,
            "worker-b",
            "fake-runner",
            None,
            None,
            None,
            std::collections::HashMap::new(),
            crate::runner::RunMode::ReadOnly,
            1,
            true,
        )
        .await
        .expect("once 패스는 러너 실패에도 정상 반환(에러는 fail_task로 처리)");

        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let task = store
            .lock()
            .unwrap()
            .get_task(&task_id)
            .unwrap()
            .expect("task 존재해야 함");
        assert_eq!(
            task.state,
            TaskState::Failed,
            "러너 실패는 failed로 전이해야 함(completed 위장 금지)"
        );
        assert!(task.artifacts.is_empty(), "실패 시 산출물이 생기면 안 됨");
        let msg = task.status_message.expect("실패 사유 메시지가 있어야 함");
        let text = msg.parts[0].text.clone().unwrap_or_default();
        assert!(
            text.contains("모의 실패"),
            "실패 사유에 러너 에러 내용이 포함돼야 함: {text}"
        );
    }

    #[tokio::test]
    async fn already_claimed_by_other_worker_skips_runner_execution() {
        // 다른 워커가 먼저 claim(state -> working, claimed_by=other-worker)한 task는 이 워커의
        // poll에서 state=working으로 보이므로 submitted 필터에 걸려 claim/실행 자체를 시도하지
        // 않는다(claim 경합에서 진 워커가 러너를 돌리면 안 되는 계약의 결정적 대체 시나리오).
        let store = test_store();
        let task_id = make_submitted_task(&store, "dispatcher", "worker-c", "선점된 지시");
        let url = spawn_test_server(store.clone()).await;
        let client = McpHttpClient::connect(url, None).await.expect("connect");

        client
            .claim_task(&task_id, Some("other-worker"), Some("other-runner"))
            .await
            .expect("다른 워커의 선점 claim은 성공해야 함");

        let calls = Arc::new(AtomicUsize::new(0));
        let runner: Arc<dyn Runner + Send + Sync> = Arc::new(RecordingRunner {
            calls: calls.clone(),
            result: FakeResult::Ok("실행되면 안 됨".to_string()),
        });

        run_worker_loop(
            &client,
            runner,
            "worker-c",
            "fake-runner",
            None,
            None,
            None,
            std::collections::HashMap::new(),
            crate::runner::RunMode::ReadOnly,
            1,
            true,
        )
        .await
        .expect("once 패스는 정상 반환");

        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "선점된 task에 대해 러너가 실행되면 안 됨"
        );

        let task = store
            .lock()
            .unwrap()
            .get_task(&task_id)
            .unwrap()
            .expect("task 존재해야 함");
        assert_eq!(
            task.state,
            TaskState::Working,
            "다른 워커 소유 상태가 그대로 유지돼야 함(이 워커가 건드리면 안 됨)"
        );
    }
}
