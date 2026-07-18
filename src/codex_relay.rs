// 머신당 codex 배달 데몬(v2-46): 로컬 codex 세션들 앞 task를 대리 claim해 그 세션 thread로 주입한다.
//
// sup 정체성(별도 poll + 사설 글루 thread + .cmd 핸들러)의 대체. 로스터에 보이는 codex 세션
// (uuid=threadId)이 곧 주입 대상이라, 사용자가 관전 중인 세션에 task와 답이 그대로 보인다.
// 설계 정본 docs/design/v2-46-codex-relay_2026-07-11.md.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::codex_appserver::{ApprovalPolicy, SandboxMode};
use crate::mcp_client::McpHttpClient;
use crate::presence_scan::{
    self, LiveSession, apply_codex_human_input_gate, enumerate_codex_sessions,
    system_time_to_db_datetime,
};
use crate::worker::parse_open_tasks;

// ---------------------------------------------------------------------------
// 순수부
// ---------------------------------------------------------------------------

/// relay의 로스터 정체성. 머신당 1개 상주라 uuid가 아니라 고정 id를 쓴다(스캐너 자기등록 규약 답습).
pub fn relay_agent_id(machine: &str) -> String {
    format!("{machine}-codex-relay")
}

/// relay의 로스터 태그. purpose=codex-inject를 유지해 대시보드 "codex 주입" 도트와 GoalForm의
/// "그 머신 relay online" 유효성 판정이 sup 시절과 같은 키로 동작한다.
pub fn relay_tags(machine: &str) -> String {
    format!("machine={machine},role=infra,purpose=codex-inject")
}

/// relay 주입 메시지의 고정 prefix(§5-6 고정 계약). presence 스캐너의 codex 입력 신호 필터
/// (presence_scan::message_is_human_input)가 이 prefix로 시작하는 user_message를 "기계 주입"으로
/// 판정해 human_input_at(총감독 ★)에서 제외한다. **이 문구를 바꾸면 P5 필터가 파손된다**(relay 주입이
/// 사람 입력으로 오분류되어 ★가 codex로 잘못 이동). build_inject_text_uses_contract_prefix 테스트로 고정.
pub const RELAY_INJECT_PREFIX: &str = "브로커 task ";

/// 주입할 유저 턴 텍스트를 조립한다. relay가 이미 claim했으므로 claim 절차 지시는 없다
/// (구 sup 핸들러 텍스트에서 "claim_task로 가져와"를 뺀 형태). 반드시 [`RELAY_INJECT_PREFIX`]로 시작한다.
pub fn build_inject_text(task_id: &str, msg: &str) -> String {
    format!(
        "{RELAY_INJECT_PREFIX}{task_id} 가 너에게 배달됐다(이미 claim됨). 아래 요청을 읽고 직접 답하라 \
         (claim/complete는 처리 절차일 뿐이니 절차를 설명하지 말고 요청에 대한 실제 답을 내라). \
         그 답변 텍스트를 result로 complete_task(task_id={task_id})를 호출해 마감하라. \
         처리 불가면 fail_task로 사유를 보고하라.\n\n[요청]\n{msg}"
    )
}

/// codex 세션 목록에 이슈 #88 시간창 게이트(+ #119 hybrid 마커)를 적용한다(순수부). presence
/// 스캐너(cli_daemons.rs)와 동일하게 [`apply_codex_human_input_gate`]를 재사용해, 게이트로
/// 로스터에서 GC됐어야 할 유령 codex thread(사람활동이 window보다 오래되거나 없는 thread)에 relay가
/// 대리 claim해 주입하는 것을 막는다(스캐너는 로스터에만 배선돼 있었고 relay는 별도 세션 소비
/// 경로라 게이트가 미적용이었다). threshold 계산 실패(예: now가 UNIX epoch보다 이전)는 fail-open
/// (미드롭, 스캐너와 동일 규약).
///
/// **marker_dir(#119)**: presence 스캐너와 같은 codex_wrapper.py 마커(`~/.tunaround/autoarm/
/// <threadId>.ctx`)를 relay도 본다. 다만 relay는 presence 스캐너와 달리 **프로세스 스냅샷을 뜨지
/// 않는다**(tasklist/ps 조회가 없다) - 그래서 [`apply_codex_human_input_gate`]의 `Pid`/`Unknown`
/// 분기(window 면제)까지는 스캐너와 동일하게 적용되지만, 그 뒤 스캐너가 하는 `filter_dead_sessions`
/// (프로세스 스냅샷으로 PID 생존을 권위 판정)에 대응하는 단계가 relay엔 없다. 즉 **relay가 실제로
/// 얻는 효과는 `Dead` 마커(래퍼가 종료를 확정 통보)의 즉시 제외뿐**이고, "Pid 마커가 있는데 실은
/// 그 PID가 죽었다"는 오탐은 relay 단독으론 못 거른다(주입 시도 자체가 codex_inject 접속 실패로
/// 이어져 fail_task로 흡수되므로 안전망은 있다 - 완전한 생존 검증 부재를 수용한다).
pub fn gate_sessions(
    sessions: Vec<LiveSession>,
    now: SystemTime,
    window: Duration,
    marker_dir: Option<&Path>,
) -> Vec<LiveSession> {
    match now.checked_sub(window).and_then(system_time_to_db_datetime) {
        Some(min_active) => apply_codex_human_input_gate(sessions, &min_active, marker_dir),
        None => sessions,
    }
}

// ---------------------------------------------------------------------------
// 데몬 루프 (라이브 IO)
// ---------------------------------------------------------------------------

/// relay 데몬 옵션(CLI 인자 해석 결과). cli_daemons가 채워 run에 넘긴다.
pub struct RelayOpts {
    pub core: String,
    pub token: Option<String>,
    pub ws: String,
    pub machine: String,
    pub codex_dir: Option<PathBuf>,
    pub home: Option<PathBuf>,
    pub stale: Duration,
    /// 이슈 #88: 사람활동 신선도 window(gate_sessions). presence-scan의 codex_human_window_mins와
    /// 동일 규약(기본 60분).
    pub codex_human_window: Duration,
    /// 이슈 #119: codex_wrapper.py 마커 디렉토리(`~/.tunaround/autoarm`, presence-scan의 marker_dir
    /// 도출과 동일 규약 - cli_daemons.rs가 home으로부터 만들어 넘긴다). None이면 순수 시간창 게이트로
    /// 폴백(home 미확정 등, apply_codex_human_input_gate와 동일 하위호환 규약).
    pub marker_dir: Option<PathBuf>,
    pub interval_secs: u64,
    pub inject_timeout_secs: u64,
    /// 주입 승인 정책(기본 Never, --approval로 노브 제공).
    pub approval: ApprovalPolicy,
    /// 주입 샌드박스 모드(기본 WorkspaceWrite, --sandbox로 노브 제공).
    pub sandbox: SandboxMode,
    pub once: bool,
}

/// 주입 중 heartbeat·lease 연장 주기(초). AGENT_TTL_SECS(90초)·CLAIM_LEASE_SECS(30분)보다 짧아야
/// 긴 주입(기본 30분)이 relay를 로스터에서 offline 시키거나(#8) task를 실행 중 requeue시키지(#62) 않는다.
const RELAY_KEEPALIVE_SECS: u64 = 30;

/// ws URL의 host:port로 짧은 TCP 접속을 시도해 app-server 도달성을 확인한다(#65). app-server가 없는
/// 창(재부팅·재배포)에 codex task를 claim했다가 즉시 fail로 승격시키지 않기 위해, 도달 불가면 그 주기
/// 주입을 건너뛰어 task를 submitted로 남긴다(app-server 복구 후 다음 주기에 배달).
async fn ws_reachable(ws_url: &str) -> bool {
    // ws://host:port/... 또는 wss://... 에서 host:port만 뽑는다.
    let after = ws_url.split("://").nth(1).unwrap_or(ws_url);
    let hostport = after.split('/').next().unwrap_or(after);
    // 포트가 없으면 TcpStream::connect로 판정할 수 없다. 이 도달성 체크는 best-effort 최적화(#65)이므로
    // 판정 불가면 게이트를 열어(true) 주입을 진행시키고, 실제 접속 실패는 codex_inject의 정상 실패
    // 경로가 다룬다(파싱 불가한 URL이 주입을 영구히 막지 않게, gemini).
    if !hostport.contains(':') {
        return true;
    }
    matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::TcpStream::connect(hostport),
        )
        .await,
        Ok(Ok(_))
    )
}

/// relay 본체: 접속(재시도) -> [자기등록 -> codex 세션 열거 -> app-server 도달성 확인 -> 세션별
/// poll -> claim -> 주입(중 heartbeat/lease 연장)] 주기 루프. 주입 실패는 fail_task로 전환해
/// dispatcher가 lease 만료를 기다리지 않게 한다.
pub async fn run(opts: RelayOpts) -> Result<(), String> {
    // 브로커보다 먼저 떠도 죽지 않게 접속을 재시도한다(presence-scan과 동일 규약).
    let mut client = loop {
        match McpHttpClient::connect(opts.core.clone(), opts.token.clone()).await {
            Ok(c) => break c,
            Err(e) if opts.once => return Err(e),
            Err(e) => {
                eprintln!("[codex-relay] 코어 접속 실패(15초 후 재시도): {e}");
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        }
    };
    let self_id = relay_agent_id(&opts.machine);
    let self_tags = relay_tags(&opts.machine);
    let display = format!("{}-릴레이", opts.machine);
    // codex 입력 신호 tail 스캔의 주기 간 캐시(uuid→(mtime, human_input_at)). 이슈 #88 게이트가
    // human_input_at을 요구하므로(스캐너와 동일 규약) 더는 스캔을 생략하지 않는다.
    let mut codex_input_cache = presence_scan::CodexInputCache::new();

    loop {
        // 자기 등록 = heartbeat 겸용(register가 last_heartbeat를 now로 덮는다. 스캐너 답습).
        if let Err(e) = client
            .register_agent(&self_id, Some(&self_tags), Some(&display))
            .await
        {
            eprintln!("[codex-relay] 자기 등록 실패(무시): {e}");
        }

        let now = std::time::SystemTime::now();
        // 로컬 라이브 codex 세션 = 주입 대상 전집합. rollout 파일이 스캐너와 같은 SoR이라
        // 로스터의 codex 세션 카드와 자동으로 일치한다. human_input_at 스캔(input_cache)을 켜서
        // 이슈 #88 게이트(gate_sessions)가 실데이터로 판정하게 한다(스캐너와 동일 규약).
        let sessions = match &opts.codex_dir {
            Some(dir) => enumerate_codex_sessions(
                dir,
                now,
                opts.stale,
                opts.home.as_deref(),
                Some(&mut codex_input_cache),
            ),
            None => Vec::new(),
        };
        // 이슈 #88: 게이트로 로스터에서 GC됐어야 할 유령 codex thread에 대리 claim해 주입하지 않게
        // 시간창 밖 세션을 여기서도 제외한다(스캐너는 로스터에만 배선, relay는 별도 소비 경로였음).
        // 이슈 #119: marker_dir가 있으면 hybrid 마커가 시간창보다 우선한다(gate_sessions 주석 참고).
        let sessions = gate_sessions(
            sessions,
            now,
            opts.codex_human_window,
            opts.marker_dir.as_deref(),
        );

        // #65: app-server(ws) 도달성을 이 주기 시작에 1회 확인한다(머신당 app-server 1개). 미도달이면
        // codex task를 claim했다가 ws 접속 실패로 즉시 fail로 승격시키지 않도록 이번 주기 주입을 통째로
        // 건너뛴다(task는 submitted로 남아 app-server 복구 후 배달된다).
        let ws_ok = sessions.is_empty() || ws_reachable(&opts.ws).await;
        if !sessions.is_empty() && !ws_ok {
            eprintln!(
                "[codex-relay] app-server({}) 미도달 - 이번 주기 codex 주입 스킵(task는 submitted 유지)",
                opts.ws
            );
        }

        let mut reconnect = false;
        if ws_ok {
            for s in &sessions {
                let poll_text = match client.poll_tasks(&s.uuid).await {
                    Ok(t) => t,
                    Err(e) => {
                        // 브로커 재시작으로 MCP 세션이 만료되면 모든 호출이 계속 실패한다(R10 교훈).
                        eprintln!("[codex-relay] poll 실패({}): {e}", s.uuid);
                        reconnect = true;
                        break;
                    }
                };
                for t in parse_open_tasks(&poll_text) {
                    if t.state != "submitted" {
                        continue;
                    }
                    // 대리 claim: 재주입 방지 + claimed_by=세션 uuid(트레이스는 그 세션 소유로 남는다).
                    // 실패 = 다른 소비자가 선점(세션이 직접 claim했거나 워커) - 조용히 넘어간다.
                    if let Err(e) = client.claim_task(&t.id, Some(&s.uuid), Some("codex")).await {
                        eprintln!("[codex-relay] claim 실패(선점됨?) task {}: {e}", t.id);
                        continue;
                    }
                    // Monitor 관측용 이벤트 한 줄(stdout).
                    println!("RELAY {} -> {}", t.id, s.uuid);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    // in-process 주입(--thread 직지정): resume 실패·타임아웃은 fail_task로 전환.
                    // #8/#62: 주입은 inject_timeout(기본 30분)까지 블록한다. 그동안 relay가 로스터에서
                    // offline(AGENT_TTL 90초)으로 빠지거나(#8) task가 lease 만료로 실행 중 requeue되지
                    // 않도록(#62), RELAY_KEEPALIVE_SECS 주기로 heartbeat + extend_lease를 곁들인다.
                    let text = build_inject_text(&t.id, &t.msg);
                    let inject = crate::codex_inject::run(
                        &opts.ws,
                        "",
                        Some(&s.uuid),
                        &text,
                        opts.approval,
                        opts.sandbox,
                        opts.inject_timeout_secs,
                        false,
                    );
                    tokio::pin!(inject);
                    let mut keepalive =
                        tokio::time::interval(Duration::from_secs(RELAY_KEEPALIVE_SECS));
                    // 절전·고부하로 tick이 밀려도 밀린 tick을 몰아치지 않게 Skip(heartbeat/lease 폭주
                    // 방지, worker.rs lease keepalive와 동일 규약, gemini medium).
                    keepalive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                    keepalive.tick().await; // 최초 즉시 tick 소비.
                    let inject_result = loop {
                        tokio::select! {
                            r = &mut inject => break r,
                            _ = keepalive.tick() => {
                                if let Err(e) = client.heartbeat(&self_id).await {
                                    eprintln!("[codex-relay] 주입 중 heartbeat 실패(무시): {e}");
                                }
                                if let Err(e) = client.extend_lease(&t.id, &s.uuid).await {
                                    eprintln!("[codex-relay] task {} lease 연장 실패: {e}", t.id);
                                }
                            }
                        }
                    };
                    match inject_result {
                        Ok(_) => eprintln!(
                            "[codex-relay] task {} 주입 턴 종료(complete는 codex 몫)",
                            t.id
                        ),
                        Err(e) => {
                            // #9: app-server에 턴 취소(interrupt) API가 없어, 타임아웃 시 서버측 codex 턴은
                            // 계속 실행될 수 있다. fail 사유에 이를 명시해 dispatcher가 이중 실행 가능성을
                            // 인지하게 한다(완전한 취소는 app-server 프로토콜 지원 전까지 불가).
                            let note = if e.contains("타임아웃") {
                                " (app-server 취소 API 부재로 서버측 턴이 계속 실행 중일 수 있음)"
                            } else {
                                ""
                            };
                            eprintln!("[codex-relay] task {} 주입 실패 -> fail_task: {e}", t.id);
                            if let Err(fe) = client
                                .fail_task(
                                    &t.id,
                                    &format!("codex-relay 주입 실패: {e}{note}"),
                                    Some(&s.uuid),
                                )
                                .await
                            {
                                eprintln!(
                                    "[codex-relay] fail_task도 실패(lease 만료가 회수): {fe}"
                                );
                            }
                        }
                    }
                }
            }
        }

        if opts.once {
            // 테스트·수동 실행 모드: 폴 실패가 있었으면 성공으로 위장하지 않는다(봇리뷰).
            return if reconnect {
                Err("codex-relay: --once 패스 중 poll 실패".to_string())
            } else {
                Ok(())
            };
        }
        if reconnect
            && let Ok(c) = McpHttpClient::connect(opts.core.clone(), opts.token.clone()).await
        {
            client = c;
        }
        tokio::time::sleep(Duration::from_secs(opts.interval_secs.max(1))).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_identity_follows_infra_conventions() {
        assert_eq!(relay_agent_id("win"), "win-codex-relay");
        let tags = relay_tags("mac");
        assert!(tags.contains("machine=mac"), "머신 태그 포함: {tags}");
        assert!(tags.contains("role=infra"), "infra 역할 포함: {tags}");
        assert!(
            tags.contains("purpose=codex-inject"),
            "도트·GoalForm 키 유지: {tags}"
        );
    }

    #[test]
    fn build_inject_text_contains_task_id_body_and_no_claim_step() {
        let text = build_inject_text("abc123", "1+1은?");
        assert!(text.contains("abc123"), "task id 포함: {text}");
        assert!(text.contains("1+1은?"), "본문 포함: {text}");
        assert!(text.contains("complete_task"), "마감 지시 포함: {text}");
        assert!(text.contains("fail_task"), "실패 경로 지시 포함: {text}");
        assert!(
            !text.contains("claim_task로 가져와"),
            "claim 절차 지시는 없어야(대리 claim): {text}"
        );
    }

    fn codex_session(uuid: &str, human_input_at: Option<&str>) -> LiveSession {
        LiveSession {
            uuid: uuid.to_string(),
            runner: "codex".to_string(),
            project: None,
            human_input_at: human_input_at.map(str::to_string),
            created_at: None,
            active_at: None,
        }
    }

    #[test]
    fn gate_sessions_drops_stale_and_keeps_fresh_human_input() {
        // 이슈 #88: relay가 게이트로 GC 대상인 유령(사람입력이 window보다 오래됨)을 주입 대상에서
        // 뺴는지 확인. window=60분, now 기준 90분 전 입력은 밖, 5분 전 입력은 안.
        let now = SystemTime::now();
        let window = Duration::from_secs(60 * 60);
        let stale_ts = system_time_to_db_datetime(now - Duration::from_secs(90 * 60)).unwrap();
        let fresh_ts = system_time_to_db_datetime(now - Duration::from_secs(5 * 60)).unwrap();
        let sessions = vec![
            codex_session("ghost", Some(&stale_ts)),
            codex_session("live", Some(&fresh_ts)),
            codex_session("nosignal", None),
        ];
        let kept: Vec<String> = gate_sessions(sessions, now, window, None)
            .into_iter()
            .map(|s| s.uuid)
            .collect();
        assert_eq!(
            kept,
            vec!["live"],
            "window 밖·무신호 세션은 주입 대상에서 제외돼야"
        );
    }

    #[test]
    fn gate_sessions_excludes_dead_marker_even_with_fresh_human_input() {
        // 이슈 #119: relay는 프로세스 스냅샷이 없어 Pid 생존까지는 권위 판정 못 하지만, 래퍼가 종료를
        // 확정 통보한 Dead 마커는 human_input이 신선해도 즉시 제외해야 한다(죽은 세션에 대리 claim해
        // 주입을 시도하지 않게).
        let marker_dir = std::env::temp_dir().join(format!(
            "tuna-relay-marker-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&marker_dir).unwrap();
        std::fs::write(marker_dir.join("ghost-dead.ctx"), "dead").unwrap();
        // Pid 마커(생존 기록)는 stale이어도 relay 단계에선 면제(프로세스 스냅샷 부재로 그 이상의
        // 권위 판정은 못 함 - 실제 접속 실패는 codex_inject의 fail_task 경로가 흡수).
        std::fs::write(marker_dir.join("pid-marked.ctx"), "4242").unwrap();

        let now = SystemTime::now();
        let window = Duration::from_secs(60 * 60);
        let stale_ts = system_time_to_db_datetime(now - Duration::from_secs(90 * 60)).unwrap();
        let fresh_ts = system_time_to_db_datetime(now - Duration::from_secs(5 * 60)).unwrap();
        let sessions = vec![
            codex_session("ghost-dead", Some(&fresh_ts)), // 신선해도 Dead 마커가 이긴다.
            codex_session("pid-marked", Some(&stale_ts)), // stale이어도 Pid 마커가 면제한다.
            codex_session("no-marker-stale", Some(&stale_ts)), // 마커 없음 → 기존 window 판정.
        ];
        let kept: Vec<String> = gate_sessions(sessions, now, window, Some(&marker_dir))
            .into_iter()
            .map(|s| s.uuid)
            .collect();
        assert_eq!(
            kept,
            vec!["pid-marked"],
            "Dead 마커는 신선해도 제외, Pid 마커는 stale이어도 유지, 마커 없음은 window 판정: {kept:?}"
        );

        std::fs::remove_dir_all(&marker_dir).ok();
    }

    #[test]
    fn build_inject_text_uses_contract_prefix() {
        // §5-6 고정 계약: 주입 텍스트는 반드시 RELAY_INJECT_PREFIX로 시작한다(P5 스캐너 필터가 이걸로
        // relay 주입을 사람 입력에서 배제). 이 테스트가 깨지면 P5 필터도 함께 갱신해야 한다.
        let text = build_inject_text("abc123", "본문");
        assert!(
            text.starts_with(RELAY_INJECT_PREFIX),
            "prefix 계약 위반: {text}"
        );
    }

    // run()의 루프 본체(대리 claim → 실패 스킵, 주입 실패 → fail_task 전환)를 실제 in-process
    // MCP 브로커로 검증한다. codex app-server ws는 가짜로 흉내내지 않고, "방금 닫은 포트"에 접속을
    // 시도하게 해 codex_inject::run이 접속 실패로 빠르게 Err를 내도록 유도한다(fail_task 전환 경로
    // 검증에는 충분 - 프로토콜 성공 경로는 실 app-server가 필요해 범위 밖).
    #[cfg(feature = "serve")]
    mod run_integration {
        use super::*;
        use crate::store::a2a::{Message, Part, TaskState};
        use crate::store::sqlite::SqliteStore;
        use std::sync::{Arc, Mutex};

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

        async fn spawn_broker(store: Arc<Mutex<SqliteStore>>) -> String {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind");
            let port = listener.local_addr().unwrap().port();
            let retriever =
                Arc::new(NullRetriever) as Arc<dyn crate::orchestrator::ContextRetriever>;
            tokio::spawn(async move {
                let _ = crate::mcp::serve_http_mcp_on_listener(
                    listener, retriever, None, None, None, None, store,
                )
                .await;
            });
            tokio::time::sleep(Duration::from_millis(120)).await;
            format!("http://127.0.0.1:{port}/mcp")
        }

        /// 아무도 리슨하지 않는(방금 bind 후 즉시 drop한) 포트의 ws URL. codex_inject::run이 접속을
        /// 즉시 거부받아 빠르게 실패하게 만든다(가짜 app-server 없이도 "주입 실패" 경로를 결정론적으로 유도).
        fn unreachable_ws_url() -> String {
            let l = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            let port = l.local_addr().unwrap().port();
            drop(l);
            format!("ws://127.0.0.1:{port}/ws")
        }

        /// enumerate_codex_sessions가 인식하는 형식의 codex TUI rollout 파일을 하나 만든다
        /// (presence_scan.rs 테스트 픽스처와 동일 관례). timestamp를 지금 시각으로 채워 이슈 #88
        /// 사람활동 게이트(gate_sessions)의 grace 조건(created_at 신선)을 통과하게 한다.
        fn write_codex_session(dir: &std::path::Path, uuid: &str) {
            let day = dir.join("2026").join("07").join("13");
            std::fs::create_dir_all(&day).unwrap();
            let now_db = presence_scan::system_time_to_db_datetime(SystemTime::now()).unwrap();
            let iso = format!("{}Z", now_db.replacen(' ', "T", 1));
            let body = format!(
                r#"{{"type":"session_meta","payload":{{"session_id":"{uuid}","cwd":"/u/x/projA","originator":"codex-tui","timestamp":"{iso}"}}}}"#
            );
            std::fs::write(
                day.join(format!("rollout-2026-07-13T00-{uuid}.jsonl")),
                body,
            )
            .unwrap();
        }

        fn submitted_task(store: &Arc<Mutex<SqliteStore>>, to: &str, text: &str) -> String {
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
            guard
                .create_task_from_message("dispatcher", to, msg)
                .unwrap()
                .id
        }

        #[tokio::test]
        async fn run_once_skips_preclaimed_task_and_skips_injection_when_appserver_unreachable() {
            let store = test_store();
            let mcp_url = spawn_broker(store.clone()).await;
            let setup_client = McpHttpClient::connect(mcp_url.clone(), None)
                .await
                .expect("connect");

            let codex_dir = std::env::temp_dir().join(format!(
                "tuna-relay-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ));
            write_codex_session(&codex_dir, "codex-open");
            write_codex_session(&codex_dir, "codex-preclaimed");

            let open_task_id = submitted_task(&store, "codex-open", "1+1은?");
            let preclaimed_task_id = submitted_task(&store, "codex-preclaimed", "선점된 지시");
            // 다른 소비자가 먼저 claim(state -> working) - relay는 이 task를 건드리면 안 된다.
            setup_client
                .claim_task(
                    &preclaimed_task_id,
                    Some("other-worker"),
                    Some("other-runner"),
                )
                .await
                .expect("사전 선점 claim 성공");

            let opts = RelayOpts {
                core: mcp_url,
                token: None,
                ws: unreachable_ws_url(),
                machine: "test-machine".to_string(),
                codex_dir: Some(codex_dir.clone()),
                home: None,
                stale: Duration::from_secs(3600),
                codex_human_window: Duration::from_secs(3600),
                marker_dir: None,
                interval_secs: 1,
                inject_timeout_secs: 5,
                approval: ApprovalPolicy::Never,
                sandbox: SandboxMode::WorkspaceWrite,
                once: true,
            };

            let result = run(opts).await;
            // poll 자체는 실패하지 않으므로(claim/주입 실패와는 별개 경로) --once는 Ok를 반환해야
            // 한다(poll 실패만 Err로 이어짐 - "성공 위장 금지"는 poll 경로 계약, 주입 실패는 fail_task로
            // 흡수되고 --once 자체는 정상 종료).
            assert!(
                result.is_ok(),
                "poll은 정상이라 once 패스는 Ok여야 함: {result:?}"
            );

            // (a) 대리 claim 실패(이미 선점) → 건드리지 않고 스킵.
            let preclaimed = store
                .lock()
                .unwrap()
                .get_task(&preclaimed_task_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                preclaimed.state,
                TaskState::Working,
                "선점된 task는 그대로 유지돼야 함(relay가 손대면 안 됨)"
            );

            // (b) #65: app-server(ws)가 도달 불가면 relay는 이 주기 주입을 통째로 건너뛴다. claim조차
            // 하지 않으므로 task는 submitted로 남아 app-server 복구 후 배달된다(도달 불가를 즉시
            // terminal failed로 승격시키지 않는다 - 재부팅·재배포 창의 일시 장애를 영구 실패로 만들지 않음).
            let opened = store
                .lock()
                .unwrap()
                .get_task(&open_task_id)
                .unwrap()
                .unwrap();
            assert_eq!(
                opened.state,
                TaskState::Submitted,
                "app-server 미도달 시 relay는 claim/주입을 건너뛰고 task를 submitted로 남겨야 함(#65)"
            );

            std::fs::remove_dir_all(&codex_dir).ok();
        }
    }
}
