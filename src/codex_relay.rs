// 머신당 codex 배달 데몬(v2-46): 로컬 codex 세션들 앞 task를 대리 claim해 그 세션 thread로 주입한다.
//
// sup 정체성(별도 poll + 사설 글루 thread + .cmd 핸들러)의 대체. 로스터에 보이는 codex 세션
// (uuid=threadId)이 곧 주입 대상이라, 사용자가 관전 중인 세션에 task와 답이 그대로 보인다.
// 설계 정본 docs/design/v2-46-codex-relay_2026-07-11.md.

use std::path::PathBuf;
use std::time::Duration;

use crate::codex_appserver::{ApprovalPolicy, SandboxMode};
use crate::mcp_client::McpHttpClient;
use crate::presence_scan::enumerate_codex_sessions;
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

/// 주입할 유저 턴 텍스트를 조립한다. relay가 이미 claim했으므로 claim 절차 지시는 없다
/// (구 sup 핸들러 텍스트에서 "claim_task로 가져와"를 뺀 형태).
pub fn build_inject_text(task_id: &str, msg: &str) -> String {
    format!(
        "브로커 task {task_id} 가 너에게 배달됐다(이미 claim됨). 아래 요청을 읽고 직접 답하라 \
         (claim/complete는 처리 절차일 뿐이니 절차를 설명하지 말고 요청에 대한 실제 답을 내라). \
         그 답변 텍스트를 result로 complete_task(task_id={task_id})를 호출해 마감하라. \
         처리 불가면 fail_task로 사유를 보고하라.\n\n[요청]\n{msg}"
    )
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
    pub interval_secs: u64,
    pub inject_timeout_secs: u64,
    pub once: bool,
}

/// relay 본체: 접속(재시도) -> [자기등록 -> codex 세션 열거 -> 세션별 poll -> claim -> 주입] 주기 루프.
/// 주입 실패는 fail_task로 전환해 dispatcher가 lease 만료를 기다리지 않게 한다.
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

    loop {
        // 자기 등록 = heartbeat 겸용(register가 last_heartbeat를 now로 덮는다. 스캐너 답습).
        if let Err(e) = client.register_agent(&self_id, Some(&self_tags), Some(&display)).await {
            eprintln!("[codex-relay] 자기 등록 실패(무시): {e}");
        }

        // 로컬 라이브 codex 세션 = 주입 대상 전집합. rollout 파일이 스캐너와 같은 SoR이라
        // 로스터의 codex 세션 카드와 자동으로 일치한다.
        let sessions = match &opts.codex_dir {
            Some(dir) => enumerate_codex_sessions(
                dir,
                std::time::SystemTime::now(),
                opts.stale,
                opts.home.as_deref(),
            ),
            None => Vec::new(),
        };

        let mut reconnect = false;
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
                let text = build_inject_text(&t.id, &t.msg);
                match crate::codex_inject::run(
                    &opts.ws,
                    "",
                    Some(&s.uuid),
                    &text,
                    ApprovalPolicy::Never,
                    SandboxMode::WorkspaceWrite,
                    opts.inject_timeout_secs,
                    false,
                )
                .await
                {
                    Ok(_) => eprintln!("[codex-relay] task {} 주입 턴 종료(complete는 codex 몫)", t.id),
                    Err(e) => {
                        eprintln!("[codex-relay] task {} 주입 실패 -> fail_task: {e}", t.id);
                        if let Err(fe) = client
                            .fail_task(&t.id, &format!("codex-relay 주입 실패: {e}"), Some(&s.uuid))
                            .await
                        {
                            eprintln!("[codex-relay] fail_task도 실패(lease 만료가 회수): {fe}");
                        }
                    }
                }
            }
        }

        if opts.once {
            // 테스트·수동 실행 모드: 폴 실패가 있었으면 성공으로 위장하지 않는다(봇리뷰).
            return if reconnect { Err("codex-relay: --once 패스 중 poll 실패".to_string()) } else { Ok(()) };
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
        assert!(tags.contains("purpose=codex-inject"), "도트·GoalForm 키 유지: {tags}");
    }

    #[test]
    fn build_inject_text_contains_task_id_body_and_no_claim_step() {
        let text = build_inject_text("abc123", "1+1은?");
        assert!(text.contains("abc123"), "task id 포함: {text}");
        assert!(text.contains("1+1은?"), "본문 포함: {text}");
        assert!(text.contains("complete_task"), "마감 지시 포함: {text}");
        assert!(text.contains("fail_task"), "실패 경로 지시 포함: {text}");
        assert!(!text.contains("claim_task로 가져와"), "claim 절차 지시는 없어야(대리 claim): {text}");
    }
}
