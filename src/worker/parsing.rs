// poll_tasks 응답(TASKS_JSON 프리픽스 또는 문자열 블록)을 ParsedTask로 해석하고, submitted task 디듑을 담당한다.

/// task id의 고정 길이(SqliteStore::new_task_id = lower(hex(randomblob(16))) = 32 hex chars).
const ID_LEN: usize = 32;

/// poll_tasks 텍스트 한 블록에서 뽑아낸 필드(from_agent는 워커 루프에 불필요해 생략).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedTask {
    pub id: String,
    pub state: String,
    /// A2A context_id(프로젝트별 라우팅 키). poll에 `ctx=-`이거나 없으면 None.
    pub context_id: Option<String>,
    pub msg: String,
}

fn is_hex32(s: &str) -> bool {
    s.len() == ID_LEN && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// state 세그먼트에서 상태 토큰만 뽑는다(첫 공백 앞). 코어가 poll 출력에 붙이는 표시 전용 주석
/// (" ⚠stuck?(20m)" 등)을 떼어내, 워커가 state를 "submitted"로 정확히 인식하게 한다. 상태값은
/// 공백을 포함하지 않으므로 첫 토큰이 곧 상태다.
fn state_token(seg: &str) -> String {
    seg.split_whitespace().next().unwrap_or("").to_string()
}

/// 텍스트에서 블록 헤더(`[<32hex>] from=...`)가 시작하는 바이트 오프셋을 모두 찾는다.
/// `format_open_tasks`(src/mcp.rs)는 블록을 `"\n\n"`로 join하므로, 헤더는 문자열 맨 앞이거나
/// 직전 두 글자가 `"\n\n"`일 때만 유효하다고 본다(메시지 본문 안의 우연한 개행과 구분).
pub(super) fn find_header_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'[' {
            continue;
        }
        let at_start = i == 0;
        // char 경계 안전: 직전 2바이트가 멀티바이트 문자 중간이면 get()이 None을 돌려준다
        // (본문에 한글이 섞인 task 메시지에서 &text[i-2..i] 슬라이스가 패닉하던 버그).
        let after_blank_line = i >= 2 && text.get(i - 2..i) == Some("\n\n");
        if !(at_start || after_blank_line) {
            continue;
        }
        let rest = &text[i + 1..];
        if rest.len() < ID_LEN + "] from=".len() {
            continue;
        }
        // char 경계 안전: ID_LEN(32)가 char 경계가 아니면(한글 등) get()이 None → 헤더 아님.
        // hex32 id는 전부 ASCII라 정상 헤더면 항상 경계에 걸린다.
        let Some(head) = rest.get(..ID_LEN) else {
            continue;
        };
        if !is_hex32(head) {
            continue;
        }
        if !rest[ID_LEN..].starts_with("] from=") {
            continue;
        }
        starts.push(i);
    }
    starts
}

/// poll_tasks(agent) 응답 텍스트를 파싱해 각 task 블록을 구조체로 반환한다.
/// v2-52 ④: 신 브로커가 얹는 `TASKS_JSON <json>` 프리픽스가 있으면 그 구조화 JSON을 우선 파싱한다(견고,
/// id는 is_hex32 검증을 통과한 것만 채택). 없으면(구 브로커) 기존 문자열 블록 파싱으로 폴백해
/// 하위호환을 유지한다. 헤더가 하나도 없고 빈 목록 안내 문구(`"... 앞 열린 task 없음"`)면 빈 Vec을
/// 반환한다(그 문구가 어떤 task의 msg 본문에 우연히 포함돼 있어도 헤더가 있으면 정상 파싱한다).
pub fn parse_open_tasks(poll_text: &str) -> Vec<ParsedTask> {
    if let Some(dtos) = crate::a2a_wire::decode_poll_json(poll_text) {
        // 문자열 경로(find_header_starts)는 is_hex32로 id를 강제하는데, JSON 경로는 이 검증이
        // 없었다. run_on_task가 {id}를 셸 명령 문자열에 직접 치환하므로(msg는 env로만 전달) hex32
        // 전제가 깨지면 세공된 id가 셸 인젝션이 된다. 여기서도 같은 검증을 걸어 불통과 항목은 버린다.
        return dtos
            .into_iter()
            .filter_map(|d| {
                if !is_hex32(&d.id) {
                    eprintln!(
                        "[worker] TASKS_JSON id가 hex32 형식이 아니라 건너뜀: {:?}",
                        d.id
                    );
                    return None;
                }
                Some(ParsedTask {
                    id: d.id,
                    state: d.state,
                    context_id: d.context_id,
                    msg: d.msg,
                })
            })
            .collect();
    }

    // 헤더 스캔을 먼저 한다. task msg 본문에 "앞 열린 task 없음" 문구가 우연히 포함돼 있어도
    // 실제 헤더가 있으면(starts 비어있지 않으면) 그 문구로 조기 반환하지 않는다(과거 버그: msg에
    // 이 문구가 있으면 그 task까지 통째로 은닉됨). 진짜 빈 응답(헤더 없음)만 빈 Vec으로 취급한다.
    let starts = find_header_starts(poll_text);
    if starts.is_empty() && poll_text.contains("앞 열린 task 없음") {
        return Vec::new();
    }
    let mut tasks = Vec::with_capacity(starts.len());

    for (idx, &start) in starts.iter().enumerate() {
        // 다음 블록 헤더 직전의 "\n\n" 구분자는 이 블록의 msg에서 제외한다.
        let block_end = starts
            .get(idx + 1)
            .map(|&next| next - 2)
            .unwrap_or(poll_text.len());
        let block = &poll_text[start..block_end];

        // block = "[<32hex id>] from=<from_agent> state=<state> msg=<msg...>"
        let after_bracket = match block[1 + ID_LEN..].strip_prefix("] from=") {
            Some(s) => s,
            None => continue,
        };
        let state_marker = " state=";
        let ctx_marker = " ctx=";
        let msg_marker = " msg=";
        let state_pos = match after_bracket.find(state_marker) {
            Some(p) => p,
            None => continue,
        };
        let after_state = &after_bracket[state_pos + state_marker.len()..];
        // msg를 앵커로 삼는다(항상 있음). state와 msg 사이의 " ctx=<id>"는 선택적으로 처리해
        // 구 포맷(ctx 없음)과도 호환한다.
        let msg_pos = match after_state.find(msg_marker) {
            Some(p) => p,
            None => continue,
        };
        let between = &after_state[..msg_pos]; // "submitted ctx=projA" 또는 "submitted ⚠no-consumer?(10m)"
        let msg = after_state[msg_pos + msg_marker.len()..].to_string();
        let (state, context_id) = match between.find(ctx_marker) {
            Some(cp) => {
                let state = state_token(&between[..cp]);
                let ctx_raw = &between[cp + ctx_marker.len()..];
                let context_id = if ctx_raw == "-" {
                    None
                } else {
                    Some(ctx_raw.to_string())
                };
                (state, context_id)
            }
            None => (state_token(between), None),
        };

        let id = block[1..1 + ID_LEN].to_string();
        tasks.push(ParsedTask {
            id,
            state,
            context_id,
            msg,
        });
    }

    tasks
}

/// poll 텍스트에서 새로 알릴 submitted task만 뽑고, `seen`을 현재 활성(submitted) 집합으로 정리한다.
/// run_poll_loop의 테스트 가능한 핵심: 디듑(같은 task 재출력 금지) + 장수명 데몬 메모리 상한
/// (claim/완료로 사라진 id를 seen에서 제거 -> 그 id가 다시 submitted로 나타나면 재알림). I/O는 호출자.
/// 구 브로커 하위호환 방어: poll_tasks 내부 조회 실패도 success 본문 "조회 실패: {e}" 텍스트로 반환하는
/// 구 브로커가 있다(신 브로커는 R1 계약으로 isError=true를 낸다, 워커는 그 경로에선 Err로 받아 이 함수를
/// 아예 호출하지 않는다). 그런 소프트에러 텍스트는 파싱 결과가 빈 Vec이면서도 진짜 빈 큐 안내
/// ("... 앞 열린 task 없음")를 포함하지 않는다 - 이 경우 seen을 비우면(디듑 상태 소실) 다음 정상 폴에서
/// 이미 알린 task가 재알림된다. 그래서 진짜 빈 큐(마커 있음)만 seen을 정리하고, 마커 없는 빈 결과는
/// seen을 유지한 채 경고만 남긴다.
pub(super) fn collect_new_submitted(
    poll_text: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<ParsedTask> {
    let tasks = parse_open_tasks(poll_text);
    // 구조화 응답(TASKS_JSON 프리픽스)은 빈 배열이라도 정상 응답이라 소프트에러가 아니다(gemini medium:
    // 향후 브로커가 `TASKS_JSON []`을 보내도 오판 방지). 진짜 빈 큐 안내도 아니고 TASKS_JSON도 없는
    // 빈 결과만 소프트에러로 의심한다.
    if tasks.is_empty()
        && !poll_text.contains("앞 열린 task 없음")
        && !poll_text.contains("TASKS_JSON")
    {
        // poll_text 원문은 task id·msg(사용자 입력·비밀 포함 가능)라 로그에 남기지 않는다(coderabbit
        // Major, 보안). 길이만 기록해 소프트에러를 진단 가능하게 한다.
        eprintln!(
            "[poll] poll 응답이 빈 목록도 빈-큐 안내도 아님(소프트에러 의심, seen 유지, {}바이트)",
            poll_text.len()
        );
        return Vec::new();
    }
    let active: std::collections::HashSet<&str> = tasks
        .iter()
        .filter(|t| t.state == "submitted")
        .map(|t| t.id.as_str())
        .collect();
    seen.retain(|id| active.contains(id.as_str()));
    let mut fresh = Vec::new();
    for t in tasks {
        if t.state == "submitted" && seen.insert(t.id.clone()) {
            fresh.push(t);
        }
    }
    fresh
}
