// worker 계열 헤드리스 서브커맨드의 실행부(work·poll·discover·watch-results·presence-scan·task·codex-inject).
// main.rs에서 분할(T4.5, 동작 불변). 각 fn = 기존 if-let 블록 본문 그대로.
#![cfg(feature = "worker")]

use crate::cli::*;

/// work 모드: 원격 코어를 auto-poll->claim->실행->complete하는 헤드리스 워커 데몬(worker 피처 전용).
pub fn work(rt: &tokio::runtime::Runtime, a: WorkArgs) {
    let mode = if a.write {
        tunaround::runner::RunMode::Write
    } else {
        tunaround::runner::RunMode::ReadOnly
    };
    // 워커 격리 가드레일(거버넌스 #5): write 워커의 작업 디렉터리가 이 프로세스 실행 디렉터리(클론)와
    // 겹치면 reset --hard 같은 write가 발밑을 갈아엎어 워커가 자살한다(2026-07-03 뱃지 task). 거부하고
    // 별도 클론/워크트리를 --project-path로 지정하도록 안내한다.
    if a.write {
        let cwd = std::env::current_dir().unwrap_or_default();
        let project = a.project_path.as_deref().map(std::path::Path::new);
        if tunaround::worker::write_lane_disrupts_node(project, &cwd) {
            eprintln!(
                "[work] 거부: --write 워커의 작업 디렉터리({})가 실행 디렉터리({})와 겹칩니다. \
                 자기 클론을 갈아엎어 워커가 자살할 수 있습니다. 별도 클론/워크트리를 --project-path로 지정하세요.",
                a.project_path.as_deref().unwrap_or("<미지정=cwd>"),
                cwd.display()
            );
            std::process::exit(1);
        }
    }
    let runner: std::sync::Arc<dyn tunaround::runner::Runner + Send + Sync> = match a.runner {
        WorkRunner::Claude => std::sync::Arc::new(tunaround::runner::claude::ClaudeRunner::new()),
        WorkRunner::Codex => std::sync::Arc::new(tunaround::runner::codex::CodexRunner::new()),
        WorkRunner::Opencode => std::sync::Arc::new(
            tunaround::runner::opencode::OpencodeRunner::new().with_model(a.model.clone()),
        ),
        #[cfg(feature = "engines")]
        WorkRunner::Http => {
            let base_url = match &a.http_base_url {
                Some(u) => u.clone(),
                None => {
                    eprintln!("[work] --runner http 는 --http-base-url <url>이 필요합니다");
                    std::process::exit(1);
                }
            };
            std::sync::Arc::new(tunaround::runner::http::OpenAiChatRunner::new(
                &base_url,
                a.model.as_deref().unwrap_or(""),
                a.token.clone(),
            ))
        }
        #[cfg(not(feature = "engines"))]
        WorkRunner::Http => {
            eprintln!("[work] --runner http 는 engines 피처가 필요합니다");
            std::process::exit(1);
        }
        #[cfg(feature = "a2a-out")]
        WorkRunner::A2a => {
            let card = match &a.a2a_card {
                Some(c) => c.clone(),
                None => {
                    eprintln!("[work] --runner a2a 는 --a2a-card <url>이 필요합니다");
                    std::process::exit(1);
                }
            };
            std::sync::Arc::new(tunaround::runner::a2a::A2ARunner::new(
                card,
                a.a2a_token.clone(),
            ))
        }
        #[cfg(not(feature = "a2a-out"))]
        WorkRunner::A2a => {
            eprintln!("[work] --runner a2a 는 a2a-out 피처가 필요합니다");
            std::process::exit(1);
        }
    };

    // --context-map "k=v,k=v" -> HashMap. 오타·빈 항목·중복은 조용히 버리지 않고 진입 시 거부한다
    // (worker::parse_context_map). 오폴백으로 엉뚱한 레포를 --write하는 사고를 막는다.
    let context_map = match a.context_map.as_deref() {
        Some(spec) => match tunaround::worker::parse_context_map(spec) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("[work] --context-map 파싱 실패: {e}");
                std::process::exit(1);
            }
        },
        None => std::collections::HashMap::new(),
    };

    let agent_id = a
        .agent
        .clone()
        .unwrap_or_else(tunaround::worker::generate_agent_uuid);
    if a.agent.is_none() {
        eprintln!("[work] --agent 미지정 -> 자가 uuid 생성: {agent_id}");
    }

    // claim 시 tasks.runner에 기록할 러너 종류 이름(v8 트레이스). WorkRunner enum -> 소문자 문자열.
    let runner_name = match a.runner {
        WorkRunner::Claude => "claude",
        WorkRunner::Codex => "codex",
        WorkRunner::Opencode => "opencode",
        WorkRunner::Http => "http",
        WorkRunner::A2a => "a2a",
    };

    let result = rt.block_on(async {
        // 브로커 토큰은 --token 우선, 없으면 TUNA_BROKER_TOKEN env 폴백(argv 노출 회피, serve/poll과 동일 계약).
        let broker_token = a
            .token
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
        let client =
            tunaround::mcp_client::McpHttpClient::connect(a.core.clone(), broker_token).await?;
        tunaround::worker::run_worker_loop(
            &client,
            runner,
            &agent_id,
            runner_name,
            a.tags.clone(),
            a.model.clone(),
            a.project_path.clone(),
            context_map,
            mode,
            a.interval,
            a.once,
        )
        .await
    });
    if let Err(e) = result {
        eprintln!("[work] 오류: {e}");
        std::process::exit(1);
    }
}

/// poll <...>: 감시 전용(claim/실행 없음). 코어에 연결해 새 task를 stdout으로 알린다.
pub fn poll(rt: &tokio::runtime::Runtime, a: PollArgs) {
    let result = rt.block_on(async {
        // 토큰은 --token 우선, 없으면 TUNA_BROKER_TOKEN env 폴백(argv 노출 회피용).
        let token = a
            .token
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
        let client = tunaround::mcp_client::McpHttpClient::connect(a.core.clone(), token).await?;
        tunaround::worker::run_poll_loop(
            &client,
            &a.agent,
            a.tags.clone(),
            a.interval,
            a.once,
            a.on_task.as_deref(),
            a.display_name.as_deref(),
            !a.no_register,
        )
        .await
    });
    if let Err(e) = result {
        eprintln!("[poll] 오류: {e}");
        std::process::exit(1);
    }
}

/// watch-results <...>: 총괄이 던진 task의 완료/실패를 브로커 SSE로 받아 stdout으로 알린다(worker 피처).
pub fn watch_results(rt: &tokio::runtime::Runtime, a: WatchResultsArgs) {
    let result = rt.block_on(tunaround::watch_results::run(
        &a.core,
        &a.dispatcher,
        a.digest,
        a.since.as_deref(),
    ));
    if let Err(e) = result {
        eprintln!("[watch-results] 오류: {e}");
        std::process::exit(1);
    }
}

/// presence-scan <...>: 머신당 스캐너 데몬 = 라이브 세션 전집합을 브로커에 일괄 동기화(v2-44).
pub fn presence_scan(rt: &tokio::runtime::Runtime, a: PresenceScanArgs) {
    let result = rt.block_on(async {
        let core = a
            .core
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_CORE).ok())
            .ok_or_else(|| "--core 또는 TUNA_BROKER_CORE가 필요합니다".to_string())?;
        let token = a
            .token
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
        // 브로커보다 먼저/직후에 떠도 죽지 않게 접속을 재시도한다(기동 순서 취약성 제거).
        // --once(테스트)는 즉시 실패를 반환해 문제를 숨기지 않는다.
        let mut client = loop {
            match tunaround::mcp_client::McpHttpClient::connect(core.clone(), token.clone()).await {
                Ok(c) => break c,
                Err(e) if a.once => return Err(e),
                Err(e) => {
                    eprintln!("[presence-scan] 코어 접속 실패(15초 후 재시도): {e}");
                    tokio::time::sleep(std::time::Duration::from_secs(15)).await;
                }
            }
        };
        let machine = a
            .machine
            .clone()
            .unwrap_or_else(tunaround::discover::default_machine);
        let projects_dir = match a.projects_dir.clone() {
            Some(p) => Some(std::path::PathBuf::from(tunaround::config::expand_home(&p))),
            None => tunaround::discover::default_projects_dir(),
        };
        let codex_dir = match a.codex_dir.clone() {
            Some(p) => Some(std::path::PathBuf::from(tunaround::config::expand_home(&p))),
            None => tunaround::presence_scan::default_codex_sessions_dir(),
        };
        let home = std::env::var(ENV_USERPROFILE)
            .or_else(|_| std::env::var(ENV_HOME))
            .ok()
            .map(std::path::PathBuf::from);
        let stale = std::time::Duration::from_secs(a.stale_mins.saturating_mul(60));
        // 이슈 #88: codex 세션 사람활동 신선도 window(사람입력/생성이 이보다 오래된 codex 유령 배제).
        let codex_window =
            std::time::Duration::from_secs(a.codex_human_window_mins.saturating_mul(60));
        let interval = a.interval.max(1);
        let mut last_report = String::new();
        // codex 입력 신호 tail 스캔의 주기 간 캐시(uuid→(mtime, human_input_at)). mtime 무변경 rollout은
        // 재스캔을 건너뛴다(v2-45 P5).
        let mut codex_input_cache = tunaround::presence_scan::CodexInputCache::new();
        // P8 유휴-열림 세션 캐시: 프로세스 스냅샷 성공 주기에 갱신하고, 실패(None) 주기엔 직전 세트를
        // 유지해 fresh 세션과 대칭으로 None 주기 깜빡임(→ sync_presence stale 제거·★ GC)을 막는다
        // (적대 리뷰 finding 1). 스냅샷이 실제로 죽음을 확인한 세션은 다음 성공 주기에 자연히 빠진다.
        let mut last_idle: Vec<tunaround::presence_scan::LiveSession> = Vec::new();
        loop {
            let now = std::time::SystemTime::now();
            let mut sessions = Vec::new();
            if let Some(dir) = &projects_dir {
                sessions.extend(tunaround::presence_scan::enumerate_claude_live(
                    dir,
                    now,
                    stale,
                    home.as_deref(),
                ));
            }
            if let Some(dir) = &codex_dir {
                sessions.extend(tunaround::presence_scan::enumerate_codex_sessions(
                    dir,
                    now,
                    stale,
                    home.as_deref(),
                    Some(&mut codex_input_cache),
                ));
            }
            // tombstone(깨끗한 종료 확정)은 프로세스 스냅샷과 무관하게 항상 제거한다(v2-46:
            // 스냅샷 실패 주기에도 직전 종료 세션이 유령 B석으로 남지 않게). last_idle 캐시에도 적용해,
            // 스냅샷 None 주기에 tombstone된 세션이 캐시로 되살아나지 않게 한다(CodeRabbit 리뷰).
            if let Some(h) = &home {
                let marker_dir = h.join(".tunaround").join("autoarm");
                sessions = tunaround::presence_scan::filter_tombstoned(sessions, &marker_dir);
                last_idle = tunaround::presence_scan::filter_tombstoned(last_idle, &marker_dir);
            }
            // 이슈 #88: codex 전용 사람활동 신선도 게이트(스냅샷 비의존, filter_tombstoned와 같은 위치).
            // codex는 마커·PID가 없어 종료된 유령 세션(rollout mtime이 relay resume으로 fresh)이
            // apply_process_gate(러너 all-or-nothing)를 통과해 오라우팅됐다. human_input_at(relay 주입 면역)
            // 또는 created_at(신규 grace)이 window 이내인 codex만 남긴다. threshold 계산 실패는 fail-open(미드롭).
            if let Some(min_active) = now
                .checked_sub(codex_window)
                .and_then(tunaround::presence_scan::system_time_to_db_datetime)
            {
                sessions =
                    tunaround::presence_scan::apply_codex_human_input_gate(sessions, &min_active);
            }
            // 프로세스 스냅샷 1회: 러너 카운트 게이트 + 마커 생존 판정이 공유한다.
            if let Some((proc_text, is_win)) = tunaround::presence_scan::process_list_text() {
                // 게이트: 러너 프로세스가 확실히 0개면 그 러너 세션 전부 죽음(재부팅 즉시 반영).
                for runner in ["claude", "codex"] {
                    let count =
                        tunaround::presence_scan::count_matching_lines(&proc_text, runner, is_win);
                    sessions =
                        tunaround::presence_scan::apply_process_gate(sessions, runner, Some(count));
                }
                // 마커 생존: 훅이 기록한 owner PID가 죽었으면 유령(/clear·창닫기·크래시) → 즉시 제외.
                if let Some(h) = &home {
                    let marker_dir = h.join(".tunaround").join("autoarm");
                    let alive = tunaround::presence_scan::parse_pids(&proc_text, is_win);
                    sessions = tunaround::presence_scan::filter_dead_sessions(
                        sessions,
                        &marker_dir,
                        &alive,
                    );
                }
                // P8: 유휴-열림 claude 세션 되살리기(순수 additive). 마커 owner pid가 살아있는 claude면
                // 신선도 창(stale, 기본 240분)과 무관하게 로스터에 유지한다. 프로세스 스냅샷이 있는 주기
                // 에만 수행(스냅샷 실패 주기엔 추가 안 함 = 보수적). 마커 없음/codex는 비대상(기존 창 폴백).
                if let (Some(h), Some(pdir)) = (&home, &projects_dir) {
                    let marker_dir = h.join(".tunaround").join("autoarm");
                    let claude_pids =
                        tunaround::presence_scan::runner_pids(&proc_text, "claude", is_win);
                    let existing: std::collections::HashSet<String> =
                        sessions.iter().map(|s| s.uuid.clone()).collect();
                    // 스냅샷 성공 주기에만 유휴 세트를 재계산해 캐시에 담는다(아래에서 dedup 후 추가).
                    last_idle = tunaround::presence_scan::enumerate_idle_marker_sessions(
                        &marker_dir,
                        pdir,
                        &claude_pids,
                        &existing,
                        home.as_deref(),
                    );
                }
            }
            // 유휴-열림 세션을 추가한다(스냅샷 실패 주기엔 직전 캐시 사용 = fresh와 대칭, 깜빡임 방지).
            // 이번 주기 fresh에 이미 있으면 제외(중복 방지).
            {
                let present: std::collections::HashSet<String> =
                    sessions.iter().map(|s| s.uuid.clone()).collect();
                sessions.extend(
                    last_idle
                        .iter()
                        .filter(|s| !present.contains(&s.uuid))
                        .cloned(),
                );
            }
            // 스캐너 자신도 로스터에 등록(설계 v2-44 §3: 스캐너 heartbeat = 머신 도달성 신호).
            // register는 last_heartbeat를 now로 덮으므로 매 주기 호출 = heartbeat 겸용.
            let self_uuid = format!("{machine}-presence-scan");
            let self_tags = format!("machine={machine},role=infra,purpose=presence");
            if let Err(e) = client
                .register_agent(
                    &self_uuid,
                    Some(&self_tags),
                    Some(&format!("{machine}-스캐너")),
                )
                .await
            {
                eprintln!("[presence-scan] 자기 등록 실패(무시): {e}");
            }
            let payload = tunaround::presence_scan::to_report_json(&machine, &sessions);
            match client.report_presence(&machine, payload).await {
                Ok(resp) => {
                    // 매 15초 같은 로그는 노이즈: 결과가 달라졌을 때만 stdout에 남긴다.
                    if resp != last_report {
                        println!("[presence-scan] {resp}");
                        last_report = resp;
                    }
                }
                Err(e) => {
                    // 브로커 재시작으로 MCP 세션이 만료되면 모든 호출이 계속 실패한다(R10 교훈).
                    // 재접속을 시도해 다음 주기부터 새 세션으로 복구한다.
                    eprintln!("[presence-scan] 보고 실패(재접속 시도): {e}");
                    if let Ok(c) =
                        tunaround::mcp_client::McpHttpClient::connect(core.clone(), token.clone())
                            .await
                    {
                        client = c;
                    }
                }
            }
            if a.once {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_secs(interval)).await;
        }
        Ok::<(), String>(())
    });
    if let Err(e) = result {
        eprintln!("[presence-scan] 오류: {e}");
        std::process::exit(1);
    }
}

/// task <...>: A2A task 수동 조작 CLI(v2-44 W3). 결과 텍스트를 그대로 stdout에 낸다(컴팩트).
pub fn task_cli(rt: &tokio::runtime::Runtime, a: TaskArgs) {
    // `-` 자리엔 stdin 본문을 채운다(긴 결과의 argv 한도 회피).
    fn arg_or_stdin(v: &str) -> Result<String, String> {
        if v != "-" {
            return Ok(v.to_string());
        }
        let mut buf = String::new();
        std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)
            .map_err(|e| format!("stdin 읽기 실패: {e}"))?;
        Ok(buf.trim_end().to_string())
    }
    let result = rt.block_on(async {
        let core = a
            .core
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_CORE).ok())
            .ok_or_else(|| "--core 또는 TUNA_BROKER_CORE가 필요합니다".to_string())?;
        let token = a
            .token
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
        let client = tunaround::mcp_client::McpHttpClient::connect(core, token).await?;
        let out = match &a.action {
            TaskAction::Poll { agent } => client.poll_tasks(agent).await?,
            TaskAction::Claim { task_id, agent } => {
                client.claim_task(task_id, Some(agent), None).await?
            }
            TaskAction::Get { task_id } => client.get_task(task_id).await?,
            TaskAction::Complete {
                task_id,
                result,
                agent,
            } => {
                let text = arg_or_stdin(result)?;
                client
                    .complete_task(task_id, &text, agent.as_deref())
                    .await?
            }
            TaskAction::Fail {
                task_id,
                reason,
                agent,
            } => {
                let text = arg_or_stdin(reason)?;
                client.fail_task(task_id, &text, agent.as_deref()).await?
            }
            TaskAction::Cancel { task_id, reason } => {
                client.cancel_task(task_id, reason.as_deref()).await?
            }
        };
        println!("{out}");
        Ok::<(), String>(())
    });
    if let Err(e) = result {
        eprintln!("[task] 오류: {e}");
        std::process::exit(1);
    }
}

/// codex-inject <...>: codex app-server 라이브 thread에 turn/start로 유저 턴 1건 주입(worker 피처).
pub fn codex_inject(rt: &tokio::runtime::Runtime, a: CodexInjectArgs) {
    let approval = match tunaround::codex_inject::parse_approval_policy(&a.approval) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[codex-inject] {e}");
            std::process::exit(1);
        }
    };
    let sandbox = match tunaround::codex_inject::parse_sandbox_mode(&a.sandbox) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("[codex-inject] {e}");
            std::process::exit(1);
        }
    };
    // clap이 --agent/--thread 배타·최소 1개를 보장한다(required_unless_present).
    let agent = a.agent.as_deref().unwrap_or("");
    let result = rt.block_on(tunaround::codex_inject::run(
        &a.ws,
        agent,
        a.thread.as_deref(),
        &a.text,
        approval,
        sandbox,
        a.timeout,
        a.new,
    ));
    if let Err(e) = result {
        eprintln!("[codex-inject] 오류: {e}");
        std::process::exit(1);
    }
}

/// codex-relay <...>: 머신당 codex 배달 데몬(v2-46). 로컬 codex 세션들 앞 task를 대리 claim해
/// 그 세션 thread로 in-process 주입한다(sup poll + .cmd 핸들러의 대체).
pub fn codex_relay(rt: &tokio::runtime::Runtime, a: CodexRelayArgs) {
    let result = rt.block_on(async {
        let core = a
            .core
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_CORE).ok())
            .ok_or_else(|| "--core 또는 TUNA_BROKER_CORE가 필요합니다".to_string())?;
        let token = a
            .token
            .clone()
            .or_else(|| std::env::var(ENV_BROKER_TOKEN).ok());
        let machine = a
            .machine
            .clone()
            .unwrap_or_else(tunaround::discover::default_machine);
        let codex_dir = match a.codex_dir.clone() {
            Some(p) => Some(std::path::PathBuf::from(tunaround::config::expand_home(&p))),
            None => tunaround::presence_scan::default_codex_sessions_dir(),
        };
        let home = std::env::var(ENV_USERPROFILE)
            .or_else(|_| std::env::var(ENV_HOME))
            .ok()
            .map(std::path::PathBuf::from);
        tunaround::codex_relay::run(tunaround::codex_relay::RelayOpts {
            core,
            token,
            ws: a.ws.clone(),
            machine,
            codex_dir,
            home,
            stale: std::time::Duration::from_secs(a.stale_mins.saturating_mul(60)),
            interval_secs: a.interval,
            inject_timeout_secs: a.inject_timeout,
            once: a.once,
        })
        .await
    });
    if let Err(e) = result {
        eprintln!("[codex-relay] 오류: {e}");
        std::process::exit(1);
    }
}
