// A2A 워커 데몬: poll_tasks 텍스트 파싱 + poll->claim->runner.run->complete 루프.

use std::sync::Arc;
use std::time::Duration;

use crate::mcp_client::McpHttpClient;
use crate::runner::{RunInput, Runner};

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
fn find_header_starts(text: &str) -> Vec<usize> {
    let bytes = text.as_bytes();
    let mut starts = Vec::new();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'[' {
            continue;
        }
        let at_start = i == 0;
        let after_blank_line = i >= 2 && &text[i - 2..i] == "\n\n";
        if !(at_start || after_blank_line) {
            continue;
        }
        let rest = &text[i + 1..];
        if rest.len() < ID_LEN + "] from=".len() {
            continue;
        }
        if !is_hex32(&rest[..ID_LEN]) {
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
/// 빈 목록 안내 문구(`"... 앞 열린 task 없음"`)를 포함하면 빈 Vec을 반환한다.
pub fn parse_open_tasks(poll_text: &str) -> Vec<ParsedTask> {
    if poll_text.contains("앞 열린 task 없음") {
        return Vec::new();
    }

    let starts = find_header_starts(poll_text);
    let mut tasks = Vec::with_capacity(starts.len());

    for (idx, &start) in starts.iter().enumerate() {
        // 다음 블록 헤더 직전의 "\n\n" 구분자는 이 블록의 msg에서 제외한다.
        let block_end = starts.get(idx + 1).map(|&next| next - 2).unwrap_or(poll_text.len());
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
                let context_id = if ctx_raw == "-" { None } else { Some(ctx_raw.to_string()) };
                (state, context_id)
            }
            None => (state_token(between), None),
        };

        let id = block[1..1 + ID_LEN].to_string();
        tasks.push(ParsedTask { id, state, context_id, msg });
    }

    tasks
}

/// task의 context_id를 `--context-map`에서 찾아 실행할 project-path를 정한다(순수 함수).
/// 매핑에 있으면 그 경로, 없거나 context_id가 없으면 기본 project-path로 폴백한다.
pub fn resolve_project_path(
    context_id: Option<&str>,
    context_map: &std::collections::HashMap<String, String>,
    default_path: Option<&str>,
) -> Option<String> {
    context_id
        .and_then(|c| context_map.get(c))
        .cloned()
        .or_else(|| default_path.map(|s| s.to_string()))
}

/// 두 경로가 겹치는지(같거나 한쪽이 다른 쪽의 조상) 판정한다(순수 함수, 파일시스템 접근 없음).
/// Path::starts_with는 컴포넌트 단위라 "/repo"와 "/repo2"를 오검출하지 않는다. write 워커의 작업
/// 디렉터리가 node 실행 클론과 겹치면 reset --hard 같은 write가 발밑을 갈아엎으므로, 그 판정의 핵심.
fn paths_overlap(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// 경로를 파일시스템 접근 없이 어휘적으로 절대·정규화한다(존재하지 않는 경로도 처리). 상대경로는 base에
/// 이어붙이고, `.`는 버리고 `..`는 직전 컴포넌트를 pop한다. canonicalize가 실패하는(=아직 없는) 경로의
/// overlap 판정 폴백으로 쓴다. 심볼릭 링크는 해석하지 않으므로 canonical과 완전 동치는 아니나, cwd 하위
/// 여부를 보수적으로 보는 데는 충분하다.
fn normalize_lexically(p: &std::path::Path, base: &std::path::Path) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let combined: PathBuf = if p.is_absolute() { p.to_path_buf() } else { base.join(p) };
    let mut out = PathBuf::new();
    for comp in combined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// write 모드 워커가 node 자신이 도는 클론을 갈아엎을(self-disruption) 위험이 있는지 판정한다.
/// project=None이면 러너가 node 실행 디렉터리(cwd)에서 돌아 위험(true). Some(p)이면 cwd와 겹치면
/// (같거나 한쪽이 조상) 위험. read-only 워커엔 호출하지 않는다(쓰기가 없어 무해). 2026-07-03 뱃지 task
/// self-disruption을 구조적으로 막는다.
///
/// 존재하는 경로는 canonical끼리 비교하고, 아직 없는 경로(canonicalize 실패)는 어휘 정규화로 폴백
/// 판정한다(gemini 리뷰: 러너가 실행 중 cwd 하위에 그 경로를 생성하면 뒤늦게 self-disruption 여지 -
/// 보수적으로 미리 겹침으로 본다).
pub fn write_lane_disrupts_node(
    project: Option<&std::path::Path>,
    node_cwd: &std::path::Path,
) -> bool {
    let Some(p) = project else {
        return true; // 작업 디렉터리 미지정 = node cwd에서 write = 위험.
    };
    match std::fs::canonicalize(p) {
        Ok(pc) => {
            let cwd = std::fs::canonicalize(node_cwd).unwrap_or_else(|_| node_cwd.to_path_buf());
            paths_overlap(&pc, &cwd)
        }
        Err(_) => {
            // 아직 없는 경로: canonical 대신 어휘 정규화로 양쪽을 같은 형태로 만들어 겹침을 본다
            // (Windows verbatim `\\?\` 접두 불일치를 피하려 cwd도 canonical 대신 어휘 정규화).
            let p_lex = normalize_lexically(p, node_cwd);
            let cwd_lex = normalize_lexically(node_cwd, node_cwd);
            paths_overlap(&p_lex, &cwd_lex)
        }
    }
}

/// `--context-map` 문자열("k=v,k=v")을 context_id->project-path 맵으로 파싱한다(순수 함수).
/// 형식 오류(= 없음)·빈 key·빈 value·중복 key는 조용히 버리지 않고 Err로 거부한다. 오타 항목이
/// 조용히 사라져 기본 project-path로 폴백되면 --write 시 엉뚱한 레포를 고칠 수 있어서다. 완전히 빈
/// 항목(후행 콤마 등)만 무해하게 건너뛴다.
pub fn parse_context_map(spec: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    for entry in spec.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (k, v) = entry
            .split_once('=')
            .ok_or_else(|| format!("--context-map 항목이 'key=value' 형식이 아닙니다: {entry:?}"))?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || v.is_empty() {
            return Err(format!("--context-map 항목의 key 또는 value가 비어있습니다: {entry:?}"));
        }
        if let Some(prev) = map.insert(k.to_string(), v.to_string()) {
            return Err(format!("--context-map에 중복 key '{k}'가 있습니다(이전 값 {prev:?})"));
        }
    }
    Ok(map)
}

/// 워커 자가 uuid 생성(--agent 미지정 시). RNG crate 없이 나노초 타임스탬프+pid+hostname 해시를
/// 조합해 32 hex로 만든다. 개인 규모 로스터 키로 충분한 유일성(서버 randomblob(16)의 client-side 대체).
pub fn generate_agent_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(0);
    let pid = std::process::id();
    // hostname을 간단한 FNV-1a 32bit로 접어 엔트로피 보강(머신 간 충돌 완화).
    let host = std::env::var("COMPUTERNAME").or_else(|_| std::env::var("HOSTNAME")).unwrap_or_default();
    let mut h: u32 = 0x811c9dc5;
    for b in host.bytes() { h ^= b as u32; h = h.wrapping_mul(0x0100_0193); }
    format!("{nanos:016x}{pid:08x}{h:08x}")
}

/// heartbeat 응답이 "미등록"이면 코어 로스터가 사라진 것(브로커 재기동)으로 보고 재등록이 필요하다고 판정.
pub fn needs_reregister(heartbeat_response: &str) -> bool {
    heartbeat_response.contains("미등록")
}

/// 워커 한 패스: poll -> (submitted만) claim -> runner.run -> complete.
/// `once=true`면 한 패스 후 반환, 아니면 `interval_secs` 간격으로 무한 루프한다.
/// poll/claim/complete 실패는 eprintln 로그 후 그 task만 건너뛰고 루프는 죽지 않는다.
/// 루프 진입 전 1회 로스터 자기 등록을 시도하고(실패해도 폴링은 계속, 레지스트리 없는 구 코어 하위호환),
/// 매 패스 시작 시 heartbeat로 online을 알린다(코어 재기동으로 로스터가 비면 재등록).
/// 인자들은 work 서브커맨드 옵션을 그대로 투영한 것이라(WorkArgs 필드 1:1, runner_name은 v8 트레이스용
/// 신규), 별도 struct로 묶기보다 이 시그니처를 유지한다(설계문서 §2.2 계약).
#[allow(clippy::too_many_arguments)]
pub async fn run_worker_loop(
    client: &McpHttpClient,
    runner: Arc<dyn Runner + Send + Sync>,
    agent: &str,
    runner_name: &str,
    tags: Option<String>,
    model: Option<String>,
    project_path: Option<String>,
    context_map: std::collections::HashMap<String, String>,
    mode: crate::runner::RunMode,
    interval_secs: u64,
    once: bool,
) -> Result<(), String> {
    // 로스터 자기 등록(1회). 실패해도 폴링은 계속한다(레지스트리 없는 구 코어 하위호환).
    match client.register_agent(agent, tags.as_deref(), None).await {
        Ok(msg) => eprintln!("[work] 로스터 등록: {msg}"),
        Err(e) => eprintln!("[work] 로스터 등록 실패(무시하고 폴링 계속): {e}"),
    }

    loop {
        // online 유지. 코어가 재기동돼 로스터가 비었으면(미등록 응답) 재등록한다.
        match client.heartbeat(agent).await {
            Ok(resp) if needs_reregister(&resp) => {
                eprintln!("[work] 코어 재기동 감지 -> 재등록 시도");
                match client.register_agent(agent, tags.as_deref(), None).await {
                    Ok(msg) => eprintln!("[work] 재등록 성공: {msg}"),
                    Err(e) => eprintln!("[work] 재등록 실패: {e}"),
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("[work] heartbeat 실패(무시): {e}"),
        }

        run_one_pass(client, &runner, agent, runner_name, &model, &project_path, &context_map, mode)
            .await;

        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// poll 텍스트에서 새로 알릴 submitted task만 뽑고, `seen`을 현재 활성(submitted) 집합으로 정리한다.
/// run_poll_loop의 테스트 가능한 핵심: 디듑(같은 task 재출력 금지) + 장수명 데몬 메모리 상한
/// (claim/완료로 사라진 id를 seen에서 제거 -> 그 id가 다시 submitted로 나타나면 재알림). I/O는 호출자.
fn collect_new_submitted(
    poll_text: &str,
    seen: &mut std::collections::HashSet<String>,
) -> Vec<ParsedTask> {
    let tasks = parse_open_tasks(poll_text);
    let active: std::collections::HashSet<&str> =
        tasks.iter().filter(|t| t.state == "submitted").map(|t| t.id.as_str()).collect();
    seen.retain(|id| active.contains(id.as_str()));
    let mut fresh = Vec::new();
    for t in tasks {
        if t.state == "submitted" && seen.insert(t.id.clone()) {
            fresh.push(t);
        }
    }
    fresh
}

/// on-task 명령의 `{id}` 플레이스홀더를 task id로 치환한다(순수 함수). msg는 셸 인젝션 위험이 있어
/// 명령 문자열에 치환하지 않고 환경변수(TUNAROUND_TASK_MSG)로만 전달한다.
fn substitute_task_placeholders(cmd: &str, id: &str) -> String {
    cmd.replace("{id}", id)
}

/// on-task 명령이 멈춰(대화형 입력 대기·네트워크 행) 폴 루프를 영구 정지시키는 걸 막는 안전 상한.
/// codex/claude 실행이 길 수 있어 넉넉히 준다(초과 시 강제 종료 후 다음 폴로 넘어간다).
const ON_TASK_TIMEOUT_SECS: u64 = 30 * 60;

/// task 도착 시 --on-task 명령을 셸로 실행한다(타임아웃 상한 있음). `{id}`는 치환하고, id/msg는
/// 환경변수로도 넘긴다. Monitor가 없는 하네스(codex 등)의 0토큰 wake 글루다.
/// unix=sh -c, windows=cmd /C. Windows는 raw_arg로 명령행을 원본 그대로 넘긴다(cmd.exe가 표준
/// CommandLineToArgvW와 다른 규칙이라 .arg()의 이스케이프가 명령 내부 큰따옴표를 깨뜨리기 때문).
/// 명령이 ON_TASK_TIMEOUT_SECS를 넘기면 강제 종료해 폴 루프가 영구 정지하지 않게 한다(0토큰 감시 목적 보전).
fn run_on_task(cmd: &str, id: &str, msg: &str) {
    let expanded = substitute_task_placeholders(cmd, id);
    #[cfg(windows)]
    let mut command = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.raw_arg("/C");
        c.raw_arg(&expanded);
        c
    };
    #[cfg(not(windows))]
    let mut command = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(&expanded);
        c
    };
    command.env("TUNAROUND_TASK_ID", id).env("TUNAROUND_TASK_MSG", msg);
    eprintln!("[poll] on-task 실행: task {id}");
    let mut child = match command.spawn() {
        Ok(c) => c,
        Err(e) => {
            eprintln!("[poll] on-task 실행 실패: {e}");
            return;
        }
    };
    // 타임아웃 감시: 멈춘 명령이 폴 루프를 영구 정지시키지 않게. spawn_blocking 스레드라 블로킹 sleep OK.
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(ON_TASK_TIMEOUT_SECS);
    loop {
        match child.try_wait() {
            Ok(Some(s)) if s.success() => {
                eprintln!("[poll] on-task 완료: task {id}");
                return;
            }
            Ok(Some(s)) => {
                eprintln!("[poll] on-task 비정상 종료(코드 {:?}): task {id}", s.code());
                return;
            }
            Ok(None) => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    eprintln!("[poll] on-task 타임아웃({ON_TASK_TIMEOUT_SECS}s) 강제 종료: task {id}");
                    return;
                }
                std::thread::sleep(Duration::from_millis(200));
            }
            Err(e) => {
                eprintln!("[poll] on-task 상태 확인 실패: {e}");
                return;
            }
        }
    }
}

/// 감시 전용 루프: agent 앞 새 submitted task만 알린다(claim은 하지 않는다).
/// Claude Code 세션이 이 커맨드를 Monitor로 감싸면, task 도착이 stdout 이벤트로 세션을 깨워 스스로
/// claim/처리하게 할 수 있다(감독 레인을 유휴 0토큰으로 운용). Monitor가 없는 하네스(codex 등)를 위해
/// `on_task`가 있으면 task마다 그 명령을 실행한다(외부 wake 글루). 이미 알린 id는 HashSet으로 디듑한다
/// (task는 claim 전까지 submitted로 남아 매 폴마다 재등장하므로 중복 알림을 막는다).
/// run_worker_loop와 동일하게 로스터 자기 등록(1회) + 매 패스 heartbeat로 online을 유지한다
/// (감독도 AGENT_TTL_SECS를 넘기지 않아야 to_selector 발견 대상에서 stale로 빠지지 않는다).
pub async fn run_poll_loop(
    client: &McpHttpClient,
    agent: &str,
    tags: Option<String>,
    interval_secs: u64,
    once: bool,
    on_task: Option<&str>,
) -> Result<(), String> {
    use std::io::Write;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    // 로스터 자기 등록(1회). 실패해도 폴링은 계속한다(레지스트리 없는 구 코어 하위호환).
    match client.register_agent(agent, tags.as_deref(), None).await {
        Ok(msg) => eprintln!("[poll] 로스터 등록: {msg}"),
        Err(e) => eprintln!("[poll] 로스터 등록 실패(무시하고 폴링 계속): {e}"),
    }

    loop {
        // online 유지. 코어가 재기동돼 로스터가 비었으면(미등록 응답) 재등록한다.
        match client.heartbeat(agent).await {
            Ok(resp) if needs_reregister(&resp) => {
                eprintln!("[poll] 코어 재기동 감지 -> 재등록 시도");
                match client.register_agent(agent, tags.as_deref(), None).await {
                    Ok(msg) => eprintln!("[poll] 재등록 성공: {msg}"),
                    Err(e) => eprintln!("[poll] 재등록 실패: {e}"),
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("[poll] heartbeat 실패(무시): {e}"),
        }

        match client.poll_tasks(agent).await {
            Ok(text) => {
                for t in collect_new_submitted(&text, &mut seen) {
                    // Monitor 이벤트 = stdout 한 줄. 파이프는 블록 버퍼라 flush로 즉시 전달한다.
                    let preview: String =
                        t.msg.chars().take(80).collect::<String>().replace('\n', " ");
                    println!("TASK {} :: {preview}", t.id);
                    let _ = std::io::stdout().flush();
                    // on-task 글루: 블로킹 명령(codex exec resume 등)이라 spawn_blocking으로 await한다
                    // (reactor는 안 막으면서 순차 처리 - 책임자는 한 번에 하나씩 다룬다).
                    if let Some(cmd) = on_task {
                        let (cmd, id, msg) = (cmd.to_string(), t.id.clone(), t.msg.clone());
                        let _ = tokio::task::spawn_blocking(move || run_on_task(&cmd, &id, &msg)).await;
                    }
                }
            }
            // 폴 실패는 이벤트가 아니라 stderr로(Monitor 이벤트 오염 방지). 루프는 죽지 않는다.
            Err(e) => eprintln!("[poll] poll_tasks 실패: {e}"),
        }
        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// 한 패스(poll -> submitted task들 순회 claim/run/complete)를 수행한다. 항상 정상 반환(에러는 로그만).
#[allow(clippy::too_many_arguments)]
async fn run_one_pass(
    client: &McpHttpClient,
    runner: &Arc<dyn Runner + Send + Sync>,
    agent: &str,
    runner_name: &str,
    model: &Option<String>,
    project_path: &Option<String>,
    context_map: &std::collections::HashMap<String, String>,
    mode: crate::runner::RunMode,
) {
    let poll_text = match client.poll_tasks(agent).await {
        Ok(t) => t,
        Err(e) => {
            eprintln!("[work] poll_tasks 실패: {e}");
            return;
        }
    };

    let tasks = parse_open_tasks(&poll_text);
    for t in tasks.iter().filter(|t| t.state == "submitted") {
        eprintln!("[work] task {} claim 시도", t.id);
        if let Err(e) = client.claim_task(&t.id, Some(agent), Some(runner_name)).await {
            eprintln!("[work] task {} claim 실패: {e}", t.id);
            continue;
        }

        // 프로젝트 라우팅: task의 context_id가 --context-map에 있으면 그 project-path로 실행하고,
        // 없으면 기본 --project-path로 폴백한다. 데몬 하나가 여러 프로젝트를 배분할 수 있다.
        let resolved_project =
            resolve_project_path(t.context_id.as_deref(), context_map, project_path.as_deref());
        if let Some(cid) = t.context_id.as_deref()
            && let Some(p) = context_map.get(cid)
        {
            eprintln!("[work] task {} context={cid} -> project-path {p}", t.id);
        }
        let input = RunInput {
            prompt: t.msg.clone(),
            model: model.clone(),
            project_path: resolved_project,
            mode,
            pull: false,
        };
        // 러너는 sync이고 일부(OpenAiChatRunner)는 내부에서 reqwest::blocking을 쓴다. tokio의
        // spawn_blocking 스레드는 Handle::current()가 살아 있어 reqwest::blocking이 "런타임 안에서
        // blocking 불가"로 거부한다. 그래서 런타임 핸들이 전혀 없는 순수 std 스레드에서 러너를 돌린다
        // (subprocess 러너 claude/codex도 std 스레드에서 정상 동작).
        let runner2 = Arc::clone(runner);
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let _ = tx.send(runner2.run(&input));
        });
        // 성공 -> complete_task(결과 artifact, state=completed). 실패 -> fail_task(사유, state=failed).
        // 실패를 completed로 위장하지 않아 dispatcher가 성패를 구분하고 재시도를 판단할 수 있다.
        match rx.await {
            Ok(Ok(out)) => match client.complete_task(&t.id, &out.content, Some(agent)).await {
                Ok(_) => eprintln!("[work] task {} complete 완료", t.id),
                Err(e) => eprintln!("[work] task {} complete 실패: {e}", t.id),
            },
            Ok(Err(e)) => {
                eprintln!("[work] task {} 러너 실패: {e:?}", t.id);
                let reason = format!("러너 실행 실패: {e:?}");
                if let Err(fe) = client.fail_task(&t.id, &reason, Some(agent)).await {
                    eprintln!("[work] task {} fail 처리 실패: {fe}", t.id);
                }
            }
            Err(_canceled) => {
                eprintln!("[work] task {} 러너 스레드 취소(결과 유실)", t.id);
                if let Err(fe) = client.fail_task(&t.id, "러너 스레드 취소(결과 유실)", Some(agent)).await {
                    eprintln!("[work] task {} fail 처리 실패: {fe}", t.id);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
        assert_eq!(tasks[0].state, "submitted", "no-consumer 주석이 state를 오염시킴: {:?}", tasks[0].state);
        assert_eq!(tasks[0].context_id.as_deref(), Some("projA"));
        assert_eq!(tasks[0].msg, "오래된 작업");
        assert_eq!(tasks[1].state, "working", "stuck 주석이 state를 오염시킴: {:?}", tasks[1].state);
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
        assert_eq!(collect_new_submitted(&present, &mut seen).len(), 1, "재등장 시 다시 알림");
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
    fn resolve_project_path_uses_map_then_falls_back() {
        let mut map = std::collections::HashMap::new();
        map.insert("projA".to_string(), "/repos/A".to_string());
        // 매핑에 있으면 그 경로.
        assert_eq!(resolve_project_path(Some("projA"), &map, Some("/default")), Some("/repos/A".to_string()));
        // context_id가 매핑에 없으면 기본값.
        assert_eq!(resolve_project_path(Some("projX"), &map, Some("/default")), Some("/default".to_string()));
        // context_id 자체가 없으면 기본값.
        assert_eq!(resolve_project_path(None, &map, Some("/default")), Some("/default".to_string()));
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

    #[test]
    fn normalize_lexically_resolves_dot_and_dotdot() {
        use std::path::Path;
        let base = Path::new("/home/user/repo");
        // 상대경로는 base에 이어붙는다.
        assert_eq!(normalize_lexically(Path::new("sub"), base), Path::new("/home/user/repo/sub"));
        // `.`은 무시, `..`은 pop.
        assert_eq!(normalize_lexically(Path::new("./a/../b"), base), Path::new("/home/user/repo/b"));
        // 절대경로는 base 무시.
        assert_eq!(normalize_lexically(Path::new("/x/y/../z"), base), Path::new("/x/z"));
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
    fn generate_agent_uuid_is_32_hex() {
        let id = generate_agent_uuid();
        assert_eq!(id.len(), 32);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn needs_reregister_detects_missing_registration() {
        assert!(needs_reregister("미등록 uuid=x(register_agent 먼저 호출하세요)"));
        assert!(!needs_reregister("heartbeat 갱신: x"));
    }
}
