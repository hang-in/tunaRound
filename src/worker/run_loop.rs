// A2A 워커/수신 데몬의 폴링 루프: 로스터 등록·heartbeat·lease 연장·claim-run-complete 순환.

use super::*;

/// 러너 실행 중 lease를 연장하는 주기(초). 한 사이클을 걸러도(연장 실패·지연) 2*주기가
/// STUCK_WORKING_SECS(15분)를 넘지 않아야 거짓 stuck 표시가 안 뜬다(5분*2=10분<15분). 브로커
/// CLAIM_LEASE_SECS(30분)보다도 훨씬 짧다(6배 여유). v2-49 #6.
const LEASE_KEEPALIVE_SECS: u64 = 5 * 60;

/// roster heartbeat 주기(초). AGENT_TTL_SECS(90초, store/agents.rs)보다 촘촘해야 러너 실행이 길어도
/// 워커가 로스터에서 offline(stale)으로 빠지지 않는다. lease 연장(5분)과 별개로 이 주기로 heartbeat만
/// 보낸다(리뷰 #6: 5분 lease 틱만으론 90초 TTL을 못 지켜 90초~5분 task가 offline 처리되던 결함).
const HEARTBEAT_KEEPALIVE_SECS: u64 = 60;

/// lease 연장 상한(회). 진행 신호 없이 무한 대기하는 고착 러너가 lease로 영원히 살아남지 못하게,
/// 관대한 상한(36*5분=3시간) 뒤에는 연장을 멈춰 lease가 만료되도록 둔다(expire_stale_claims의
/// requeue→fail 안전망 복원). 상한 아래의 정당한 장기 task는 영향받지 않는다. v2-49 #6 하드닝(적대 리뷰).
const MAX_LEASE_EXTENSIONS: u32 = 36;

/// 워커 자가 uuid 생성(--agent 미지정 시). RNG crate 없이 나노초 타임스탬프+pid+hostname 해시를
/// 조합해 32 hex로 만든다. 개인 규모 로스터 키로 충분한 유일성(서버 randomblob(16)의 client-side 대체).
pub fn generate_agent_uuid() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id();
    // hostname을 간단한 FNV-1a 32bit로 접어 엔트로피 보강(머신 간 충돌 완화).
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let mut h: u32 = 0x811c9dc5;
    for b in host.bytes() {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
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

        run_one_pass(
            client,
            &runner,
            agent,
            runner_name,
            &model,
            &project_path,
            &context_map,
            mode,
        )
        .await;

        if once {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// on-task 명령의 `{id}` 플레이스홀더를 task id로 치환한다(순수 함수). msg는 셸 인젝션 위험이 있어
/// 명령 문자열에 치환하지 않고 환경변수(TUNAROUND_TASK_MSG)로만 전달한다.
pub(super) fn substitute_task_placeholders(cmd: &str, id: &str) -> String {
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
        use std::os::unix::process::CommandExt;
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(&expanded);
        // src/runner/exec.rs와 동일하게 별도 process group으로 묶는다. 타임아웃 시
        // crate::runner::exec::kill_pid가 이 pid를 pgid로 보고 그룹 전체(codex-inject 등
        // 손자 프로세스 포함)에 SIGKILL을 보낼 수 있으려면 여기서 그룹 리더가 돼야 한다.
        c.process_group(0);
        c
    };
    command
        .env("TUNAROUND_TASK_ID", id)
        .env("TUNAROUND_TASK_MSG", msg);
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
                    // child.kill()은 직계 자식(cmd/sh)만 죽여 손자(codex-inject 등 실제 작업)가
                    // 고아로 남는다. exec.rs가 이미 해결한 트리 kill(Windows taskkill /T,
                    // Unix process-group SIGKILL)을 재사용한다.
                    crate::runner::exec::kill_pid(child.id());
                    let _ = child.wait();
                    eprintln!(
                        "[poll] on-task 타임아웃({ON_TASK_TIMEOUT_SECS}s) 강제 종료: task {id}"
                    );
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

/// 세션 마커 내용으로 수신 루프 종료 여부를 판정하는 순수 함수(이슈 #118). SessionStart 훅
/// (tuna-autoarm.py)이 세션당 `.ctx` 마커를 쓰고, SessionEnd 훅(tuna-disarm.py)은 파일을 지우지
/// 않고 내용을 "dead"로 남긴다(tombstone) - 그래서 "파일이 사라짐"과 "내용이 dead"를 둘 다
/// 종료 신호로 봐야 한다. PID 숫자나 "unknown"(owner 탐색 실패, tuna_arm.write_marker 참고)은
/// 세션이 아직 산 것으로 보고 계속 수신한다.
pub(crate) fn marker_gone(content: Option<&str>) -> bool {
    match content {
        None => true,
        Some(c) => c.trim() == "dead",
    }
}

/// [`marker_gone`]의 파일시스템 래퍼: 마커를 읽어 판정한다. **NotFound만** 부재(=종료)로 취급하고,
/// 그 외 IO 오류(권한·Windows 공유 위반 등 일시 오류)는 생존 유지로 본다(CodeRabbit) - 훅이 마커를
/// 갱신하는 순간의 읽기 실패로 산 세션의 수신 루프를 죽이면 안 된다. 진짜 죽은 세션의 마커는
/// 오류가 아니라 "없거나 dead"로 관측되므로, 일시 오류는 다음 주기(15초) 재판정으로 충분하다.
pub(crate) fn session_marker_terminated(path: &std::path::Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(s) => marker_gone(Some(&s)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
        Err(_) => false,
    }
}

/// TASK 알림 한 줄의 포맷팅(순수 함수, 이슈 #147 Stage 1). `via`가 있으면 어느 보조 좌석 주소로
/// 왔는지 줄 끝에 표기한다("mbox:machine=..,project=.." 등). `via=None`(기본 agent 경로)은 기존
/// 포맷("TASK {id} :: {preview}")과 완전히 동일해, `--also-agent` 미사용 시 출력이 한 글자도
/// 바뀌지 않는다(하위호환 계약). Monitor는 이 줄 전체를 이벤트 텍스트로 그대로 사용하므로
/// prefix("TASK {id} ::")는 절대 바꾸지 않는다(#136 수신 규약 참고 문서 a2a-usage.md).
pub(super) fn format_task_notification(id: &str, preview: &str, via: Option<&str>) -> String {
    match via {
        Some(addr) => format!("TASK {id} :: {preview} (via {addr})"),
        None => format!("TASK {id} :: {preview}"),
    }
}

/// 새 task 알림 1건을 stdout에 찍고(포맷=[`format_task_notification`]) `on_task`가 있으면 그 명령을
/// 실행한다(블로킹 동안 `register` 워커는 heartbeat로 online을 유지). run_poll_loop의 기본 agent
/// 경로와 좌석(mbox) 보조 경로가 이 헬퍼를 공유해, 알림·on-task 글루 동작이 두 경로에서 동일하다.
async fn notify_one_task(
    client: &McpHttpClient,
    agent: &str,
    register: bool,
    on_task: Option<&str>,
    t: &ParsedTask,
    via: Option<&str>,
) {
    use std::io::Write;
    // Monitor 이벤트 = stdout 한 줄. 파이프는 블록 버퍼라 flush로 즉시 전달한다.
    let preview: String = t
        .msg
        .chars()
        .take(80)
        .collect::<String>()
        .replace('\n', " ");
    println!("{}", format_task_notification(&t.id, &preview, via));
    let _ = std::io::stdout().flush();
    // on-task 글루: 블로킹 명령(codex exec resume 등)이라 spawn_blocking으로 await한다
    // (reactor는 안 막으면서 순차 처리 - 책임자는 한 번에 하나씩 다룬다).
    // 이 명령은 ON_TASK_TIMEOUT_SECS(최대 30분)까지 블로킹할 수 있어, 그동안도
    // run_one_pass의 lease keepalive와 동일한 이유(AGENT_TTL_SECS=90초)로 roster
    // heartbeat가 끊기면 워커가 offline 처리된다. register 모드(수신 전용이 아닐 때)에서만
    // TTL보다 촘촘한 주기(HEARTBEAT_KEEPALIVE_SECS=60초)로 heartbeat를 곁들인다.
    if let Some(cmd) = on_task {
        let (cmd, id, msg) = (cmd.to_string(), t.id.clone(), t.msg.clone());
        let handle = tokio::task::spawn_blocking(move || run_on_task(&cmd, &id, &msg));
        tokio::pin!(handle);
        loop {
            tokio::select! {
                r = &mut handle => { let _ = r; break; }
                _ = tokio::time::sleep(Duration::from_secs(HEARTBEAT_KEEPALIVE_SECS)), if register => {
                    if let Err(e) = client.heartbeat(agent).await {
                        eprintln!("[poll] on-task 실행 중 heartbeat 실패(무시): {e}");
                    }
                }
            }
        }
    }
}

/// 감시 전용 루프: agent 앞 새 submitted task만 알린다(claim은 하지 않는다).
/// Claude Code 세션이 이 커맨드를 Monitor로 감싸면, task 도착이 stdout 이벤트로 세션을 깨워 스스로
/// claim/처리하게 할 수 있다(감독 레인을 유휴 0토큰으로 운용). Monitor가 없는 하네스(codex 등)를 위해
/// `on_task`가 있으면 task마다 그 명령을 실행한다(외부 wake 글루). 이미 알린 id는 HashSet으로 디듑한다
/// (task는 claim 전까지 submitted로 남아 매 폴마다 재등장하므로 중복 알림을 막는다).
/// run_worker_loop와 동일하게 로스터 자기 등록(1회) + 매 패스 heartbeat로 온라인을 유지한다
/// (감독도 AGENT_TTL_SECS를 넘기지 않아야 to_selector 발견 대상에서 stale로 빠지지 않는다).
/// `session_marker`가 있으면(이슈 #118) 매 패스 시작 시 [`session_marker_terminated`]로 종료를
/// 판정한다 - /clear·창닫기로 세션이 사라져도 마커가 남아 유령 poll이 영구 잔존하던 것을 없앤다.
/// `also_agents`(이슈 #147 Stage 1: 좌석 수신함)는 기본 `agent`와 별개로 매 주기 poll_tasks를
/// 추가로 조회할 주소 목록이다(예: "mbox:machine=win,project=tunaRound"). 서버는 무변경 - to_agent
/// 정확일치 큐(poll_tasks)를 그대로 재사용한다. 각 주소는 **독립 seen 세트**를 쓴다(공유하면 한
/// 주소의 active 집합이 다른 주소의 이미-알린 id를 `collect_new_submitted`의 retain에서 지워, 아직
/// claim 안 된 task가 매 주기 재알림되는 버그가 생긴다). 좌석 주소는 등록·heartbeat 대상이
/// 아니다(에이전트 신원이 아니라 durable 큐 주소라 로스터에 얹지 않는다, mesh 토론 합의 §Stage1).
/// `also_agents`가 비어 있으면 기본 agent 경로의 출력·타이밍은 이 파라미터 도입 전과 완전히 같다.
#[allow(clippy::too_many_arguments)] // CLI 인자를 그대로 받는 배선 함수(구조체화는 T4.7에서).
pub async fn run_poll_loop(
    client: &McpHttpClient,
    agent: &str,
    tags: Option<String>,
    interval_secs: u64,
    once: bool,
    on_task: Option<&str>,
    display_name: Option<&str>,
    register: bool,
    session_marker: Option<std::path::PathBuf>,
    also_agents: &[String],
) -> Result<(), String> {
    use std::io::Write;
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    // 좌석 주소별 독립 seen(위 함수 doc 참고). also_agents는 루프 내내 불변이라 인덱스 정렬이 안정적이다.
    let mut also_seen: Vec<std::collections::HashSet<String>> =
        also_agents.iter().map(|_| Default::default()).collect();

    // 로스터 자기 등록(1회). 실패해도 폴링은 계속한다(레지스트리 없는 구 코어 하위호환).
    // 등록 성공 시 last_heartbeat가 now로 세팅되므로 첫 패스의 heartbeat는 건너뛴다(중복 요청 회피,
    // once=true 시 특히. 리뷰 반영). 등록 실패 시엔 첫 패스에서 heartbeat로 online을 시도한다.
    // register=false = 수신 전용 모드(v2-44 이후 세션 수신 poll): presence는 머신 스캐너 소관이라
    // 등록·heartbeat를 아예 안 한다. 태그 없는 재등록이 스캐너 항목을 덮어 '기타' 유령·깜빡임을
    // 만들던 것 제거(2026-07-11 실측: 3d45a660 '기타' 등장).
    let mut skip_heartbeat = if !register {
        true
    } else {
        match client
            .register_agent(agent, tags.as_deref(), display_name)
            .await
        {
            Ok(msg) => {
                eprintln!("[poll] 로스터 등록: {msg}");
                true
            }
            Err(e) => {
                eprintln!("[poll] 로스터 등록 실패(무시하고 폴링 계속): {e}");
                false
            }
        }
    };

    loop {
        // 세션 마커 종료 판정(이슈 #118): heartbeat·poll보다 먼저 확인해, 죽은 세션에 대해
        // 불필요한 로스터 요청조차 내지 않고 즉시 정상 종료한다.
        if let Some(m) = &session_marker
            && session_marker_terminated(m)
        {
            println!("[poll] 세션 마커 종료(dead) - 수신 루프 정상 종료");
            let _ = std::io::stdout().flush();
            return Ok(());
        }
        // online 유지. 코어가 재기동돼 로스터가 비었으면(미등록 응답) 재등록한다.
        if skip_heartbeat {
            skip_heartbeat = false;
        } else if !register {
            // 수신 전용: heartbeat도 스캐너 소관이라 보내지 않는다.
        } else {
            match client.heartbeat(agent).await {
                Ok(resp) if needs_reregister(&resp) => {
                    eprintln!("[poll] 코어 재기동 감지 -> 재등록 시도");
                    match client
                        .register_agent(agent, tags.as_deref(), display_name)
                        .await
                    {
                        Ok(msg) => eprintln!("[poll] 재등록 성공: {msg}"),
                        Err(e) => eprintln!("[poll] 재등록 실패: {e}"),
                    }
                }
                Ok(_) => {}
                Err(e) => eprintln!("[poll] heartbeat 실패(무시): {e}"),
            }
        }

        match client.poll_tasks(agent).await {
            Ok(text) => {
                for t in collect_new_submitted(&text, &mut seen) {
                    notify_one_task(client, agent, register, on_task, &t, None).await;
                }
            }
            // 폴 실패는 이벤트가 아니라 stderr로(Monitor 이벤트 오염 방지). 루프는 죽지 않는다.
            Err(e) => eprintln!("[poll] poll_tasks 실패: {e}"),
        }
        // 이슈 #147 Stage 1: 좌석 수신함 이중폴. 서버는 무변경 - 각 주소를 그대로 poll_tasks
        // 재사용한다. 한 주소의 poll 실패는 그 주기만 건너뛰고(stderr 로그) 기본 agent·다른
        // 좌석 주소 수신에는 영향 없다(주소별 독립 실패 격리).
        for (addr, addr_seen) in also_agents.iter().zip(also_seen.iter_mut()) {
            match client.poll_tasks(addr).await {
                Ok(text) => {
                    for t in collect_new_submitted(&text, addr_seen) {
                        notify_one_task(client, agent, register, on_task, &t, Some(addr)).await;
                    }
                }
                Err(e) => eprintln!("[poll] poll_tasks({addr}) 실패: {e}"),
            }
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
        if let Err(e) = client
            .claim_task(&t.id, Some(agent), Some(runner_name))
            .await
        {
            eprintln!("[work] task {} claim 실패: {e}", t.id);
            continue;
        }

        // 프로젝트 라우팅: task의 context_id가 --context-map에 있으면 그 project-path로 실행하고,
        // 없으면 기본 --project-path로 폴백한다. 데몬 하나가 여러 프로젝트를 배분할 수 있다.
        let resolved_project = resolve_project_path(
            t.context_id.as_deref(),
            context_map,
            project_path.as_deref(),
        );
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
        // v2-49 #6: 러너 실행 중 주기적으로 lease를 연장해, CLAIM_LEASE_SECS(30분)를 넘는 장기 task가
        // expire_stale_claims에 실행 중 requeue되는 것을 막는다. rx 완료와 interval을 select!로 경합해
        // 러너가 끝나면 즉시 연장을 멈춘다(client를 borrow만 하므로 clone 불요).
        let run_result = {
            // heartbeat는 AGENT_TTL_SECS(90초)보다 촘촘히(HEARTBEAT_KEEPALIVE_SECS=60초) 보내 로스터
            // online을 유지하고, lease 연장은 그보다 성기게(LEASE_KEEPALIVE_SECS=5분 = heartbeat
            // ticks_per_lease틱마다) 한다. 하나의 60초 틱으로 둘을 구동한다.
            let mut ticker = tokio::time::interval(Duration::from_secs(HEARTBEAT_KEEPALIVE_SECS));
            // 노트북 절전·고부하로 tick이 밀려도 기본 Burst처럼 몰아치지 않게 Skip(밀린 tick을 버리고
            // 다음 정상 tick만) - 깨어날 때 연장 상한을 몰아서 소진하는 것 방지(gemini 리뷰).
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            ticker.tick().await; // 최초 즉시 tick 소비(방금 claim해 lease가 신선함)
            let ticks_per_lease = (LEASE_KEEPALIVE_SECS / HEARTBEAT_KEEPALIVE_SECS).max(1);
            let mut ticks: u64 = 0;
            let mut extensions: u32 = 0;
            tokio::pin!(rx);
            loop {
                tokio::select! {
                    r = &mut rx => break r,
                    _ = ticker.tick() => {
                        ticks += 1;
                        // roster heartbeat: 러너 실행 전체(수 분~수 시간)가 이 한 패스에 포함되는데
                        // AGENT_TTL_SECS(90초, store/agents.rs)를 넘기면 워커가 로스터에서 offline
                        // 처리돼 셀렉터 라우팅 대상에서 사라진다. 매 60초 틱마다 heartbeat로 online 유지
                        // (실패는 치명적이지 않아 로그만).
                        if let Err(e) = client.heartbeat(agent).await {
                            eprintln!("[work] task {} 러너 실행 중 heartbeat 실패(무시): {e}", t.id);
                        }
                        // lease 연장은 LEASE_KEEPALIVE_SECS마다(=heartbeat ticks_per_lease틱마다). 상한
                        // 도달 후에는 연장을 멈춰, 진행 신호 없이 무한 대기하는 고착 러너가 lease로 영원히
                        // 살아남지 못하게 한다(이후 lease 만료 → expire_stale_claims가 requeue/fail).
                        if ticks.is_multiple_of(ticks_per_lease) && extensions < MAX_LEASE_EXTENSIONS {
                            extensions += 1;
                            // 연장 실패(이미 requeue/재claim/종료) = 이 워커의 소유권 상실. 로그만 남기고
                            // 계속 진행한다(러너 결과는 complete 시 first-completer-wins 가드가 거른다).
                            if let Err(e) = client.extend_lease(&t.id, agent).await {
                                eprintln!("[work] task {} lease 연장 실패: {e}", t.id);
                            } else if extensions == MAX_LEASE_EXTENSIONS {
                                eprintln!(
                                    "[work] task {} lease 연장 상한 도달 - 이후 연장 중단(고착 시 requeue/fail로 회수)",
                                    t.id
                                );
                            }
                        }
                    }
                }
            }
        };
        // 성공 -> complete_task(결과 artifact, state=completed). 실패 -> fail_task(사유, state=failed).
        // 실패를 completed로 위장하지 않아 dispatcher가 성패를 구분하고 재시도를 판단할 수 있다.
        match run_result {
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
                if let Err(fe) = client
                    .fail_task(&t.id, "러너 스레드 취소(결과 유실)", Some(agent))
                    .await
                {
                    eprintln!("[work] task {} fail 처리 실패: {fe}", t.id);
                }
            }
        }
    }
}
