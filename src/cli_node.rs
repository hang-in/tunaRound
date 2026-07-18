// node·온보딩 서브커맨드의 헬퍼(레인 러너 조립·재시도 접속·init·doctor) (main.rs에서 분할, T4.5).

#[cfg(all(feature = "serve", feature = "worker"))]
use crate::cli::InitArgs;

/// lane.runner(문자열)로부터 Runner를 만든다. 알 수 없는 이름·미충족 피처는 Err.
// _token(브로커 인증)은 어느 runner 분기도 쓰지 않는다: http는 lane.http_api_key(분리된 전용 키)를
// 쓰고, a2a는 lane.a2a_token을 쓴다(브로커 토큰이 외부 엔드포인트로 새지 않도록 분리, 보안 하드닝).
// 시그니처는 호출부 호환을 위해 유지한다.
#[cfg(feature = "worker")]
pub fn build_lane_runner(
    lane: &tunaround::config::Lane,
    _token: &Option<String>,
) -> Result<std::sync::Arc<dyn tunaround::runner::Runner + Send + Sync>, String> {
    use std::sync::Arc;
    let runner: Arc<dyn tunaround::runner::Runner + Send + Sync> = match lane.runner.as_str() {
        "claude" => Arc::new(tunaround::runner::claude::ClaudeRunner::new()),
        "codex" => Arc::new(tunaround::runner::codex::CodexRunner::new()),
        "opencode" => Arc::new(
            tunaround::runner::opencode::OpencodeRunner::new().with_model(lane.model.clone()),
        ),
        #[cfg(feature = "engines")]
        "http" => {
            let base = lane
                .http_base_url
                .as_deref()
                .ok_or("lane runner=http는 http_base_url이 필요합니다")?;
            Arc::new(tunaround::runner::http::OpenAiChatRunner::new(
                base,
                lane.model.as_deref().unwrap_or(""),
                // 빈/공백 http_api_key는 무헤더(None)로 강등(coderabbit/gemini).
                lane.http_api_key.clone().filter(|k| !k.trim().is_empty()),
            ))
        }
        #[cfg(not(feature = "engines"))]
        "http" => return Err("lane runner=http는 engines 피처가 필요합니다".to_string()),
        #[cfg(feature = "a2a-out")]
        "a2a" => {
            let card = lane
                .a2a_card
                .as_deref()
                .ok_or("lane runner=a2a는 a2a_card가 필요합니다")?;
            Arc::new(tunaround::runner::a2a::A2ARunner::new(
                card.to_string(),
                lane.a2a_token.clone(),
            ))
        }
        #[cfg(not(feature = "a2a-out"))]
        "a2a" => return Err("lane runner=a2a는 a2a-out 피처가 필요합니다".to_string()),
        other => return Err(format!("알 수 없는 runner: {other}")),
    };
    Ok(runner)
}

/// 브로커가 뜰 때까지 MCP 연결을 재시도한다(node self 모드: 브로커 기동과 워커 연결이 경합).
#[cfg(feature = "worker")]
pub async fn connect_with_retry(
    core_url: &str,
    token: &Option<String>,
    tries: u32,
) -> Result<tunaround::mcp_client::McpHttpClient, String> {
    let mut last = String::new();
    for _ in 0..tries {
        match tunaround::mcp_client::McpHttpClient::connect(core_url.to_string(), token.clone())
            .await
        {
            Ok(c) => return Ok(c),
            Err(e) => {
                last = e;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(format!("코어 연결 실패({tries}회 재시도): {last}"))
}

/// listen 주소가 loopback(127.0.0.1/localhost/[::1] 계열)인지 단순 host 문자열로 판정한다(순수
/// 함수). mcp/server.rs::warn_if_insecure_bind와 같은 취지의 판정이지만 크레이트 경계(bin↔lib)로
/// 직접 재사용이 안 되어 단순 prefix 비교로 동등 로직을 재구현했다(P0-①, 감사문서 D절 §1).
#[cfg(all(feature = "serve", feature = "worker"))]
fn is_loopback_listen(addr: &str) -> bool {
    let host = if let Some(rest) = addr.strip_prefix('[') {
        rest.split(']').next().unwrap_or("")
    } else {
        addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr)
    };
    // IPv6 zone identifier(::1%lo0)는 파싱 전에 떼어낸다(gemini 리뷰).
    let host = host.split('%').next().unwrap_or(host);
    if host.eq_ignore_ascii_case("localhost") {
        return true;
    }
    // 리터럴 IP만 신뢰한다(CodeRabbit 리뷰): "127." 접두사 문자열 비교는 127.attacker.example 같은
    // 호스트명(비-loopback으로 해석될 수 있음)까지 loopback으로 오판해 무토큰 바인드를 허용한다.
    host.parse::<std::net::IpAddr>()
        .map(|ip| ip.is_loopback())
        .unwrap_or(false)
}

/// listen 바인드 주소에서 로컬 훅·MCP 등록이 쓸 도달 URL을 만든다(순수 함수). 와일드카드 바인드
/// (0.0.0.0/::)는 그 주소로 접속할 수 없으므로 127.0.0.1로 치환하고 포트는 보존한다(CodeRabbit:
/// 사용자 지정 --listen 포트가 mesh config·MCP 등록에 전파되지 않던 문제의 단일 소스).
#[cfg(all(feature = "serve", feature = "worker"))]
fn broker_core_url_from_listen(listen: &str) -> String {
    let (host, port) = if let Some(rest) = listen.strip_prefix('[') {
        let host = rest.split(']').next().unwrap_or("");
        let port = rest.rsplit_once(':').map(|(_, p)| p).unwrap_or("8770");
        (host.split('%').next().unwrap_or(host), port)
    } else {
        match listen.rsplit_once(':') {
            Some((h, p)) => (h, p),
            None => (listen, "8770"),
        }
    };
    let is_wildcard = host
        .parse::<std::net::IpAddr>()
        .map(|ip| ip.is_unspecified())
        .unwrap_or(false);
    if is_wildcard {
        format!("http://127.0.0.1:{port}/mcp")
    } else if host.contains(':') {
        format!("http://[{host}]:{port}/mcp")
    } else {
        format!("http://{host}:{port}/mcp")
    }
}

/// 자동탐지 lane 계획 하나(agent 이름 + runner). 순수부라 탐지 목록을 주입해 테스트하기 쉽다.
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Debug, PartialEq)]
struct AutoLane {
    agent: String,
    runner: String,
}

/// PATH에서 발견된 러너 이름 목록(found)으로 자동 레인 계획을 만든다(P0-③). 발견마다 lane 1개,
/// agent 이름은 "<runner>-worker". 0개 발견이면 기존 폴백(claude 1개, agent=default_agent)으로 강등한다.
#[cfg(all(feature = "serve", feature = "worker"))]
fn plan_auto_lanes(found: &[&str], default_agent: &str) -> Vec<AutoLane> {
    if found.is_empty() {
        return vec![AutoLane {
            agent: default_agent.to_string(),
            runner: "claude".to_string(),
        }];
    }
    found
        .iter()
        .map(|r| AutoLane {
            agent: format!("{r}-worker"),
            runner: r.to_string(),
        })
        .collect()
}

/// node.toml을 생성한다(플래그 주도). 러너 자동 탐지 + MCP 자동 등록 + 다음 단계 안내. 성공 0, 실패 non-zero.
#[cfg(all(feature = "serve", feature = "worker"))]
pub fn run_init(args: &InitArgs) -> i32 {
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| tunaround::config::expand_home("~/.tunaround/node.toml"));
    if std::path::Path::new(&out).exists() && !args.force {
        eprintln!("[init] 이미 존재합니다: {out} (덮어쓰려면 --force)");
        return 1;
    }
    let core = args.core.clone().unwrap_or_else(|| "self".to_string());
    let agent = args.agent.clone().unwrap_or_else(|| "worker".to_string());
    let project = args
        .project
        .clone()
        .unwrap_or_else(|| {
            std::env::current_dir()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".to_string())
        })
        .replace('\\', "/"); // TOML 이중따옴표 이스케이프 회피(Windows 백슬래시 -> 슬래시).
    // 토큰 env 이름을 데몬·훅(TUNA_BROKER_TOKEN)과 통일한다: 예전 기본값 TUNAROUND_TOKEN은 node만
    // 쓰던 별도 이름이라 "토큰 env가 둘"인 혼란을 만들었다. 이제 node.toml·데몬·훅·config가 한 이름을 쓴다.
    let token_env = args
        .token_env
        .clone()
        .unwrap_or_else(|| "TUNA_BROKER_TOKEN".to_string());

    let mut toml = format!("core = \"{core}\"\n");
    // core=self 기본을 로컬 전용(loopback)으로 낮춘다(P0-①): 기존 0.0.0.0 기본은 무심코 LAN에
    // 노출되는 함정이었다. --listen으로 비-loopback을 명시하면 기존 동작(토큰 노출)을 그대로 유지한다.
    let listen: Option<String> = if core == "self" {
        let l = args
            .listen
            .clone()
            .unwrap_or_else(|| "127.0.0.1:8770".to_string());
        toml.push_str(&format!("listen = \"{l}\"\n"));
        toml.push_str("db = \"~/.tunaround/broker.db\"\n");
        Some(l)
    } else {
        None
    };
    // 이 브로커에 로컬에서 도달하는 URL(훅 config·MCP 등록 공용 단일 소스 - 사용자 지정 포트 전파,
    // CodeRabbit 리뷰). core가 원격이면 그 URL 그대로.
    let broker_core_url = match &listen {
        Some(l) => broker_core_url_from_listen(l),
        None => core.clone(),
    };
    // core=self이고 listen이 loopback이면 "로컬 무토큰" 계약을 그대로 활용한다: token 키 자체를
    // node.toml에 넣지 않는다(resolve_node_token(None)=None, 경고 없이 조용히 무토큰 - src/config/node.rs).
    // --listen으로 비-loopback을 지정했거나 core가 원격이면 기존처럼 토큰 키를 남긴다.
    let is_local = listen.as_deref().map(is_loopback_listen).unwrap_or(false);
    if is_local {
        toml.push('\n');
    } else {
        toml.push_str(&format!("token = \"@env:{token_env}\"\n\n"));
    }

    // 러너 레인 계획(P0-③): --runner 명시 시 기존처럼 단일 lane, 미지정이면 PATH의
    // claude·codex·opencode를 전부 탐지해 발견된 만큼 lane을 스캐폴드한다(0개면 claude 폴백).
    let lanes: Vec<AutoLane> = match &args.runner {
        Some(r) => vec![AutoLane {
            agent: agent.clone(),
            runner: r.clone(),
        }],
        None => {
            let found: Vec<&str> = ["claude", "codex", "opencode"]
                .into_iter()
                .filter(|b| binary_on_path(b))
                .collect();
            plan_auto_lanes(&found, &agent)
        }
    };
    for (i, lane) in lanes.iter().enumerate() {
        if i > 0 {
            toml.push('\n');
        }
        toml.push_str("[[lane]]\n");
        toml.push_str(&format!("agent = \"{}\"\n", lane.agent));
        toml.push_str(&format!("runner = \"{}\"\n", lane.runner));
        toml.push_str("mode = \"read-only\"   # 파일 수정 맡기려면 \"write\"\n");
        toml.push_str(&format!("project = \"{project}\"\n"));
        toml.push_str("interval = 20\n");
    }

    if let Some(parent) = std::path::Path::new(&out).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("[init] 디렉터리 생성 실패 {}: {e}", parent.display());
        return 1;
    }
    if let Err(e) = std::fs::write(&out, &toml) {
        eprintln!("[init] 쓰기 실패 {out}: {e}");
        return 1;
    }

    println!("작성됨: {out}\n");
    print!("{toml}");
    println!("\n감지된 워커 레인:");
    for lane in &lanes {
        println!("  - {} (runner={})", lane.agent, lane.runner);
    }

    // mesh·훅용 ~/.tunaround/config(dotenv)도 한 번에 스캐폴드해 "설정 파일 3종"을 최초 1회로 압축한다.
    // 기존 config는 실제 토큰을 담고 있을 수 있으므로 --force 없이는 절대 덮지 않는다(토큰 보존).
    let config_written = if args.no_mesh_config {
        false
    } else {
        scaffold_mesh_config(
            &broker_core_url,
            args.machine.as_deref(),
            args.force,
            is_local,
        )
    };

    println!("\n다음 단계:");
    let mut step = 1;
    if !is_local {
        if config_written {
            println!(
                "  {step}) ~/.tunaround/config 의 TUNA_BROKER_TOKEN 을 실제 토큰으로 채우기\n     (데몬·훅·node/doctor 전부 이 파일을 읽습니다. 셸 env {token_env}를 설정하면 그쪽이 우선합니다)"
            );
        } else {
            println!(
                "  {step}) 토큰: export {token_env}=<비밀토큰>  (Windows PowerShell: $env:{token_env}=\"...\")"
            );
        }
        step += 1;
    }
    println!("  {step}) 진단: tunaround doctor");
    step += 1;
    println!(
        "  {step}) 상주: tunaround node   (mesh 전체는 restart 스크립트가 config를 읽어 데몬에 상속)"
    );

    // MCP 자동 등록(P0-②): 로컬(loopback)일 때만 시도한다. 원격/비-loopback은 토큰이 필요해
    // 자동화가 안전하지 않으니 --header 포함 수동 명령만 안내한다. --no-mcp-register는 완전 옵트아웃.
    if args.no_mcp_register {
        println!("\ntuna-broker MCP 자동 등록: --no-mcp-register로 건너뜁니다.");
    } else if is_local {
        try_register_mcp(&broker_core_url);
    } else {
        println!(
            "\nClaude Code에 MCP 등록(원격/토큰 필요라 수동):\n  claude mcp add --transport http --scope user tuna-broker {broker_core_url} --header \"Authorization: Bearer <토큰>\""
        );
    }
    0
}

/// 이 머신 태그를 OS로 감지한다(win/mac/unix). --machine 플래그가 있으면 그 값을 그대로 쓴다.
#[cfg(all(feature = "serve", feature = "worker"))]
fn detect_machine(explicit: Option<&str>) -> String {
    if let Some(m) = explicit {
        return m.to_string();
    }
    if cfg!(target_os = "windows") {
        "win".to_string()
    } else if cfg!(target_os = "macos") {
        "mac".to_string()
    } else {
        "unix".to_string()
    }
}

/// ~/.tunaround/config(mesh·훅용 dotenv)의 내용을 만든다(순수 함수, 파일 IO 없음 - 테스트 용이).
/// 토큰은 placeholder만 넣는다(실값 금지). `local`이면(P0-①) 브로커가 무토큰 계약으로 뜨는
/// 상태이므로 TUNA_BROKER_TOKEN 줄을 주석 처리해 둔다(LAN 확장 시 그대로 발견 가능하게 남겨둠).
#[cfg(all(feature = "serve", feature = "worker"))]
fn mesh_config_content(broker_core: &str, machine: &str, bin: &str, local: bool) -> String {
    // placeholder는 config 계층 const와 단일 소스(dotenv 폴백 파서가 이 값을 걸러낸다).
    let ph = tunaround::config::TOKEN_PLACEHOLDER;
    let token_block = if local {
        format!(
            "# 로컬(loopback) 전용이라 브로커가 무토큰 계약으로 뜹니다. LAN으로 확장하면 아래 주석을 해제하고\n\
             # 실제 토큰으로 채우세요(node.toml의 @env:TUNA_BROKER_TOKEN도 같은 이름을 씁니다).\n\
             # TUNA_BROKER_TOKEN={ph}\n"
        )
    } else {
        format!(
            "# 브로커 인증 토큰(평문). 아래를 실제 토큰으로 바꾸세요. node.toml의 @env:TUNA_BROKER_TOKEN도\n\
             # 이 이름을 씁니다. 파일 권한 제한 권장(mac/linux: chmod 600, Windows: icacls 본인만 R/W).\n\
             TUNA_BROKER_TOKEN={ph}\n"
        )
    };
    format!(
        "# tunaRound mesh·훅 설정(tunaround init 자동 생성). 값을 채운 뒤 SessionStart 훅과 restart\n\
         # 스크립트가 읽는다. 형식=KEY=VALUE, 우선순위=이 파일 > env > 기본값. 상세=docs/reference/onboarding.md\n\
         TUNA_AUTOARM=1\n\
         TUNA_BIN={bin}\n\
         TUNA_BROKER_CORE={broker_core}\n\
         TUNA_MACHINE={machine}\n\
         {token_block}"
    )
}

/// ~/.tunaround/config(mesh·훅용 dotenv)를 스캐폴드한다. 이미 있으면(force 아님) 실토큰 보존 위해
/// 건드리지 않고 false를 반환한다. 토큰은 실값을 쓰지 않고 placeholder만 넣어 사용자가 채우게 한다
/// (토큰이 argv/명령 히스토리에 남지 않게). node.toml의 @env:TUNA_BROKER_TOKEN과 같은 이름이라
/// restart 스크립트가 이 파일을 읽어 데몬 env로 상속하면 node·데몬·훅이 한 토큰을 공유한다.
/// `local`은 run_init이 판정한 loopback 여부를 그대로 넘겨받아 토큰 줄 주석 처리에 쓴다(P0-①).
/// `broker_core`는 호출부(run_init)가 listen에서 도출한 도달 URL을 그대로 받는다(사용자 지정
/// 포트 전파, CodeRabbit 리뷰).
#[cfg(all(feature = "serve", feature = "worker"))]
fn scaffold_mesh_config(
    broker_core: &str,
    machine: Option<&str>,
    force: bool,
    local: bool,
) -> bool {
    let path = tunaround::config::expand_home("~/.tunaround/config");
    if std::path::Path::new(&path).exists() && !force {
        println!("\n참고: {path} 는 이미 있어 건드리지 않았습니다(실토큰 보존, 덮으려면 --force).");
        return false;
    }
    let machine = detect_machine(machine);
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "tunaround".to_string());
    let content = mesh_config_content(broker_core, &machine, &bin, local);
    if let Some(parent) = std::path::Path::new(&path).parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        eprintln!("[init] config 디렉터리 생성 실패 {}: {e}", parent.display());
        return false;
    }
    if let Err(e) = std::fs::write(&path, &content) {
        eprintln!("[init] config 쓰기 실패 {path}: {e}");
        return false;
    }
    // 실토큰이 담길 파일이라 유닉스에선 소유자만 R/W(0600)로 강제한다(다중 사용자 노출 차단, 봇 리뷰).
    // Windows는 %USERPROFILE% ACL이 기본 사용자 스코프라 여기서 별도 강제 없이 파일 주석 안내로 둔다.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    // 로컬 무토큰 프리셋에선 토큰 입력을 요구하지 않는다(CodeRabbit: 주석 처리해 놓고 채우라고
    // 안내하면 모순).
    if local {
        println!("\n작성됨: {path}");
    } else {
        println!("\n작성됨: {path} (TUNA_BROKER_TOKEN 을 채우세요)");
    }
    true
}

/// 실행 파일이 PATH에 있는지 확인한다(Windows는 .exe/.cmd/.bat 확장자도 시도).
#[cfg(all(feature = "serve", feature = "worker"))]
pub fn binary_on_path(name: &str) -> bool {
    let exts: &[&str] = if cfg!(windows) {
        &["", ".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let Some(path) = std::env::var_os("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path) {
        for ext in exts {
            if dir.join(format!("{name}{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

/// spawn용 실행 파일 전체 경로를 PATH에서 찾는다(Windows는 .exe/.cmd/.bat 순). Windows의
/// `Command::new`는 확장자 없는 이름으로 PATH를 뒤지지 않아, claude가 .cmd 셸 스크립트로 설치된
/// 경우(nvm·npm 글로벌 설치 등) 그냥 이름만 넘기면 spawn이 실패한다. 전체 경로(.cmd 포함)를 주면
/// CreateProcess가 셸 연계로 정상 실행한다. src/runner/exec.rs::resolve_bin과 같은 취지지만
/// 크레이트 경계(bin↔lib)로 재사용이 안 되어 여기 동등 로직으로 재구현했다(P0-②).
#[cfg(all(feature = "serve", feature = "worker"))]
fn resolve_spawnable_bin(name: &str) -> String {
    #[cfg(windows)]
    {
        if let Some(path) = std::env::var_os("PATH") {
            for dir in std::env::split_paths(&path) {
                for ext in ["exe", "cmd", "bat"] {
                    let cand = dir.join(format!("{name}.{ext}"));
                    if cand.is_file() {
                        return cand.to_string_lossy().into_owned();
                    }
                }
            }
        }
        name.to_string()
    }
    #[cfg(not(windows))]
    {
        name.to_string()
    }
}

/// MCP 자동 등록 판정 결과(순수부, P0-②). 실행 없이 무엇을 할지만 결정해 테스트 용이하게 한다.
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Debug, PartialEq)]
enum McpRegistrationPlan {
    /// claude CLI가 PATH에 없음: 수동 안내만 출력.
    NoClaudeBinary,
    /// 이미 등록돼 있음: 기존 등록을 보존하고(remove 없음) 아무것도 하지 않음.
    AlreadyRegistered,
    /// 미등록: 이 argv로 `claude` 를 실행해 등록.
    Register { add_args: Vec<String> },
}

/// (core_url, claude 존재 여부, 이미 등록됐는지)로 등록 계획을 판정하는 순수 함수.
#[cfg(all(feature = "serve", feature = "worker"))]
fn plan_mcp_registration(
    core_url: &str,
    claude_found: bool,
    already_registered: bool,
) -> McpRegistrationPlan {
    if !claude_found {
        return McpRegistrationPlan::NoClaudeBinary;
    }
    if already_registered {
        return McpRegistrationPlan::AlreadyRegistered;
    }
    McpRegistrationPlan::Register {
        add_args: vec![
            "mcp".to_string(),
            "add".to_string(),
            "--transport".to_string(),
            "http".to_string(),
            "--scope".to_string(),
            "user".to_string(),
            "tuna-broker".to_string(),
            core_url.to_string(),
        ],
    }
}

/// claude CLI 데드라인 실행 결과.
#[cfg(all(feature = "serve", feature = "worker"))]
enum BoundedRun {
    /// 데드라인 안에 종료(성공 여부 + stderr 요약용).
    Completed { success: bool, stderr: String },
    /// 데드라인 초과: 자식을 kill하고 반환.
    TimedOut,
    /// spawn 자체 실패.
    SpawnErr(String),
}

/// claude CLI를 데드라인 안에서 실행한다(CodeRabbit 리뷰: status()/output()은 무기한 대기라
/// claude가 멈추거나 입력을 기다리면 init이 끝나지 않는다). stdin은 null로 막아 대화형 대기를
/// 차단하고, 초과 시 자식을 kill한 뒤 수동 안내로 강등할 수 있게 TimedOut을 돌려준다.
/// stderr는 파이프로 모은다(claude mcp 에러는 짧아 파이프 버퍼 안에서 안전).
/// 실제 claude CLI를 실행하는 유일한 지점이라, 단위 테스트에서는 절대 호출하지 않는다
/// (이 머신의 실 등록을 건드리면 안 됨 - plan_mcp_registration 순수부만 테스트).
#[cfg(all(feature = "serve", feature = "worker"))]
fn run_claude_bounded(args: &[String], deadline: std::time::Duration) -> BoundedRun {
    use std::process::{Command, Stdio};
    let bin = resolve_spawnable_bin("claude");
    let mut child = match Command::new(&bin)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => return BoundedRun::SpawnErr(e.to_string()),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stderr = child
                    .stderr
                    .take()
                    .and_then(|s| std::io::read_to_string(s).ok())
                    .unwrap_or_default();
                return BoundedRun::Completed {
                    success: status.success(),
                    stderr,
                };
            }
            Ok(None) => {
                if start.elapsed() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return BoundedRun::TimedOut;
                }
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            Err(e) => return BoundedRun::SpawnErr(e.to_string()),
        }
    }
}

/// claude CLI 호출 데드라인. get은 조회라 짧게, add는 설정 쓰기까지라 여유를 둔다.
#[cfg(all(feature = "serve", feature = "worker"))]
const CLAUDE_GET_DEADLINE_SECS: u64 = 15;
#[cfg(all(feature = "serve", feature = "worker"))]
const CLAUDE_ADD_DEADLINE_SECS: u64 = 20;

/// core가 loopback일 때 Claude Code에 tuna-broker MCP 서버 등록을 시도한다(P0-②). 판정은
/// plan_mcp_registration(순수)이 맡고, 여기는 claude CLI 실행만 담당하는 thin wrapper다.
/// 실패해도 init 자체는 성공 종료(fail-open) - 수동 명령을 안내해 사용자가 이어갈 수 있게 한다.
/// 기존 등록은 절대 remove하지 않는다(보존).
#[cfg(all(feature = "serve", feature = "worker"))]
fn try_register_mcp(core_url: &str) {
    let manual_hint =
        format!("claude mcp add --transport http --scope user tuna-broker {core_url}");
    if !binary_on_path("claude") {
        println!(
            "\n참고: claude CLI를 PATH에서 찾지 못해 tuna-broker MCP 자동 등록을 건너뜁니다. 수동 등록:\n  {manual_hint}"
        );
        return;
    }
    // 존재 확인(get)부터 데드라인 실행: claude가 행이면 판정 불가이므로 add까지 가지 않고
    // 바로 수동 안내로 강등한다(초 단위 대기 2번 연속을 피함).
    let get_args: Vec<String> = ["mcp", "get", "tuna-broker"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let already_registered = match run_claude_bounded(
        &get_args,
        std::time::Duration::from_secs(CLAUDE_GET_DEADLINE_SECS),
    ) {
        BoundedRun::Completed { success, .. } => success,
        BoundedRun::TimedOut => {
            println!(
                "\ntuna-broker MCP 자동 등록: claude가 {CLAUDE_GET_DEADLINE_SECS}초 안에 응답하지 않아 건너뜁니다. 수동 등록:\n  {manual_hint}"
            );
            return;
        }
        BoundedRun::SpawnErr(e) => {
            println!(
                "\ntuna-broker MCP 자동 등록 실패(claude 실행: {e}). 수동 등록:\n  {manual_hint}"
            );
            return;
        }
    };
    match plan_mcp_registration(core_url, true, already_registered) {
        McpRegistrationPlan::NoClaudeBinary => unreachable!("claude 존재는 위에서 확인됨"),
        McpRegistrationPlan::AlreadyRegistered => {
            println!("\ntuna-broker MCP: 이미 등록돼 있어 그대로 둡니다.");
        }
        McpRegistrationPlan::Register { add_args } => {
            match run_claude_bounded(
                &add_args,
                std::time::Duration::from_secs(CLAUDE_ADD_DEADLINE_SECS),
            ) {
                BoundedRun::Completed { success: true, .. } => {
                    println!("\ntuna-broker MCP 등록 완료.");
                    println!("Claude Code를 재시작(새 세션)해야 tuna-broker 도구가 보입니다.");
                }
                BoundedRun::Completed { stderr, .. } => {
                    println!(
                        "\ntuna-broker MCP 자동 등록 실패({}). 수동 등록:\n  {manual_hint}",
                        stderr.trim()
                    );
                }
                BoundedRun::TimedOut => {
                    println!(
                        "\ntuna-broker MCP 자동 등록: claude가 {CLAUDE_ADD_DEADLINE_SECS}초 안에 끝나지 않아 중단했습니다. 수동 등록:\n  {manual_hint}"
                    );
                }
                BoundedRun::SpawnErr(e) => {
                    println!("\ntuna-broker MCP 자동 등록 실패({e}). 수동 등록:\n  {manual_hint}");
                }
            }
        }
    }
}

/// node 설정을 진단한다. 각 항목을 OK/WARN/FAIL로 출력하고, FAIL이 있으면 non-zero exit code를 돌려준다.
#[cfg(all(feature = "serve", feature = "worker"))]
pub fn run_doctor(cfg_path: Option<&str>) -> i32 {
    let mut fails = 0;

    let cfg = match tunaround::config::load_node_config(cfg_path) {
        Ok(c) => {
            println!("OK   config: 로드/파싱 성공");
            c
        }
        Err(e) => {
            println!("FAIL config: {e}");
            return 1; // 설정 없으면 더 볼 게 없다.
        }
    };

    let token = tunaround::config::resolve_node_token(cfg.token.as_deref());
    match &token {
        Some(_) => println!("OK   token: 설정됨"),
        None => println!("WARN token: 없음(코어가 토큰을 요구하면 실패)"),
    }

    // 형태소 백엔드 진단(설계 §C: Kiwi 자동다운로드 성공 확인). lindera 폴백도 동작하므로 FAIL 아닌 WARN.
    #[cfg(feature = "morphology")]
    match tunaround::search::tokenizer::create_tokenizer("kiwi") {
        Ok(tk) if tk.backend_name() == "kiwi" => {
            println!("OK   morphology: Kiwi 로드됨(자동다운로드/캐시 성공)")
        }
        Ok(tk) => {
            // Kiwi 미로드 시 개선책은 OS마다 다르다: Windows는 자동다운로드가 깨져 install 스크립트로
            // 수동 설치, mac/linux는 자동다운로드 재시도(자산/네트워크 확인). 잘못된 OS 안내를 피한다.
            #[cfg(windows)]
            let hint = "scripts/install-kiwi-windows.sh로 수동 설치";
            #[cfg(not(windows))]
            let hint = "Kiwi 자산 자동다운로드 실패(네트워크/캐시 확인)";
            println!(
                "WARN morphology: Kiwi 폴백={}(형태소 품질 저하, {hint})",
                tk.backend_name()
            )
        }
        Err(e) => println!("WARN morphology: 토크나이저 초기화 실패({e}), FTS는 fallback 사용"),
    }
    #[cfg(not(feature = "morphology"))]
    println!("WARN morphology: 미빌드(FTS는 fallback 토크나이저 사용)");

    if cfg.core == "self" {
        let listen = cfg.listen.as_deref().unwrap_or("0.0.0.0:8770");
        match std::net::TcpListener::bind(listen) {
            Ok(l) => {
                drop(l);
                println!("OK   core=self: {listen} 바인드 가능");
            }
            // 포트가 이미 쓰이면, 그게 우리 브로커(node가 이미 구동 중)인지 확인한다.
            // agent-card가 응답하면 OK로 본다(진단 목적이면 정상 상태이므로 오진단 방지, gemini 지적).
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
                // 와일드카드(0.0.0.0/[::]) 바인드만 루프백으로 조회한다. 특정 IP로 바인드했다면
                // 127.0.0.1로는 못 닿아 false FAIL이 나므로 그 주소 그대로 쓴다(gemini 지적).
                // 와일드카드만 루프백으로 조회하되, IPv6 와일드카드([::])는 [::1]로(IPv6-only에서
                // 127.0.0.1이 안 닿을 수 있음). 특정 IP 바인드는 그 주소 그대로.
                let port = listen.rsplit(':').next().unwrap_or("8770");
                let target = if listen.starts_with("0.0.0.0") {
                    format!("127.0.0.1:{port}")
                } else if listen.starts_with("[::]") {
                    format!("[::1]:{port}")
                } else {
                    listen.to_string()
                };
                let url = format!("http://{target}/.well-known/agent-card.json");
                let mut req = reqwest::blocking::Client::new()
                    .get(&url)
                    .timeout(std::time::Duration::from_secs(3));
                if let Some(t) = &token {
                    req = req.bearer_auth(t);
                }
                match req.send() {
                    Ok(r) if r.status().is_success() => {
                        println!(
                            "OK   core=self: {listen} 사용 중이나 브로커가 응답(node가 이미 구동 중)"
                        )
                    }
                    // 401/403만 "우리 브로커인데 인증 문제"로 보고 WARN. 404 등 다른 non-2xx는
                    // 무관한 프로세스(일반 웹서버)일 가능성이 커서 FAIL(node 기동 시 포트 충돌 예방).
                    Ok(r)
                        if r.status() == reqwest::StatusCode::UNAUTHORIZED
                            || r.status() == reqwest::StatusCode::FORBIDDEN =>
                    {
                        println!(
                            "WARN core=self: {listen} 사용 중, 브로커가 {} (우리 브로커로 보이나 토큰 확인 필요)",
                            r.status()
                        )
                    }
                    Ok(r) => {
                        println!(
                            "FAIL core=self: {listen} 점유한 프로세스가 {} 응답(우리 브로커 아님, 포트 충돌)",
                            r.status()
                        );
                        fails += 1;
                    }
                    // 전송 자체 실패 = HTTP 응답 없음. 다른 프로세스가 비-HTTP로 점유 중일 수 있다.
                    Err(_) => {
                        println!(
                            "FAIL core=self: {listen} 사용 중이고 HTTP 응답 없음(다른 프로세스 점유?)"
                        );
                        fails += 1;
                    }
                }
            }
            Err(e) => {
                println!("FAIL core=self: {listen} 바인드 불가({e})");
                fails += 1;
            }
        }
        if let Some(db) = &cfg.db {
            let db_e = tunaround::config::expand_home(db);
            let parent = std::path::Path::new(&db_e).parent();
            match parent {
                Some(p) if p.as_os_str().is_empty() || p.is_dir() => {
                    println!("OK   db: {db_e} (상위 디렉터리 존재)");
                }
                Some(p) => {
                    println!(
                        "WARN db: 상위 디렉터리 없음 {} (node가 만들거나 실패할 수 있음)",
                        p.display()
                    );
                }
                None => println!("OK   db: {db_e}"),
            }
        }
    } else {
        let base = cfg.core.strip_suffix("/mcp").unwrap_or(&cfg.core);
        let url = format!("{base}/.well-known/agent-card.json");
        let client = reqwest::blocking::Client::new();
        let mut req = client.get(&url).timeout(std::time::Duration::from_secs(5));
        if let Some(t) = &token {
            req = req.bearer_auth(t);
        }
        match req.send() {
            Ok(r) if r.status().is_success() => {
                println!("OK   core: {} 도달(agent-card {})", cfg.core, r.status())
            }
            Ok(r) => {
                println!("FAIL core: agent-card 상태 {} @ {url}", r.status());
                fails += 1;
            }
            Err(e) => {
                println!("FAIL core: {} 도달 불가({e})", cfg.core);
                fails += 1;
            }
        }
    }

    for l in &cfg.lane {
        let kind = if l.is_supervised() {
            "감독"
        } else {
            "자동"
        };
        match l.runner.as_str() {
            b @ ("claude" | "codex" | "opencode") => {
                if binary_on_path(b) {
                    println!("OK   lane {}[{kind}] runner={b}: PATH에 있음", l.agent);
                } else {
                    println!(
                        "FAIL lane {}[{kind}] runner={b}: PATH에 없음(설치/로그인 필요)",
                        l.agent
                    );
                    fails += 1;
                }
            }
            // http/a2a는 바이너리 대신 필수 설정을 검증한다(누락 시 node가 build_lane_runner에서 실패, gemini 지적).
            // http/a2a는 필수 설정 + 그 러너를 지원하는 피처가 이 바이너리에 컴파일됐는지도 본다
            // (피처 없이 빌드되면 node가 build_lane_runner에서 실패하므로, doctor가 미리 잡는다).
            "http" => {
                #[cfg(not(feature = "engines"))]
                {
                    println!(
                        "FAIL lane {}[{kind}] runner=http: 이 바이너리는 engines 피처 없이 빌드됨",
                        l.agent
                    );
                    fails += 1;
                }
                #[cfg(feature = "engines")]
                match &l.http_base_url {
                    Some(u) => {
                        // 스키마(http://·https://) 누락은 도달 문제가 아니라 설정 형식 오류라 별도로 알린다
                        // (그러지 않으면 reqwest URL 파싱 실패가 "도달 불가"로 오진단된다).
                        if !u.starts_with("http://") && !u.starts_with("https://") {
                            println!(
                                "FAIL lane {}[{kind}] runner=http: base_url {u} 형식 오류(http:// 또는 https:// 스키마 필요)",
                                l.agent
                            );
                            fails += 1;
                        } else {
                            // base_url이 응답하면(HTTP 상태 무관) 도달로 본다. 콜드 스타트/나중 기동
                            // 가능성이 있으니 도달 불가는 FAIL이 아닌 WARN.
                            let reachable = reqwest::blocking::Client::new()
                                .get(u)
                                .timeout(std::time::Duration::from_secs(3))
                                .send()
                                .is_ok();
                            if reachable {
                                println!(
                                    "OK   lane {}[{kind}] runner=http: base_url {u} 도달",
                                    l.agent
                                );
                            } else {
                                println!(
                                    "WARN lane {}[{kind}] runner=http: base_url {u} 도달 불가(LLM 미기동?)",
                                    l.agent
                                );
                            }
                        }
                    }
                    None => {
                        println!(
                            "FAIL lane {}[{kind}] runner=http: http_base_url 누락",
                            l.agent
                        );
                        fails += 1;
                    }
                }
            }
            "a2a" => {
                #[cfg(not(feature = "a2a-out"))]
                {
                    println!(
                        "FAIL lane {}[{kind}] runner=a2a: 이 바이너리는 a2a-out 피처 없이 빌드됨",
                        l.agent
                    );
                    fails += 1;
                }
                #[cfg(feature = "a2a-out")]
                match &l.a2a_card {
                    Some(c) => println!("OK   lane {}[{kind}] runner=a2a: card {c}", l.agent),
                    None => {
                        println!("FAIL lane {}[{kind}] runner=a2a: a2a_card 누락", l.agent);
                        fails += 1;
                    }
                }
            }
            other => {
                println!(
                    "FAIL lane {}[{kind}] runner={other}: 알 수 없는 runner",
                    l.agent
                );
                fails += 1;
            }
        }
        if let Some(p) = &l.project {
            let pe = tunaround::config::expand_home(p);
            if std::path::Path::new(&pe).is_dir() {
                println!("OK   lane {} project: {pe}", l.agent);
            } else {
                println!("FAIL lane {} project 디렉터리 없음: {pe}", l.agent);
                fails += 1;
            }
        }
        // 로스터 태그 형식 검증(k=v,k=v). 잘못된 형식은 node 기동 후 register 때야 실패해 혼란스러우므로
        // 프리플라이트에서 잡는다(register_agent가 쓰는 parse_tags 재사용).
        if let Some(t) = &l.tags {
            match tunaround::store::agents::parse_tags(t) {
                Ok(_) => println!("OK   lane {} tags: 형식 OK", l.agent),
                Err(e) => {
                    println!("FAIL lane {} tags 형식 오류: {e}", l.agent);
                    fails += 1;
                }
            }
        }
    }

    // 훅 배포 동기화 진단(#6, 정보성 - exit code에 영향 없음). 바이너리와 달리 훅에는 sync 메커니즘이
    // 없어(각 머신이 ~/.claude/hooks에 수동 복사한 사본을 실제로 실행), 레포 훅을 고쳐 머지해도 각
    // 머신 사본을 재복사하기 전까지 반영되지 않는 잠복 문제가 있었다. 레포 밖에서 doctor를 돌리는
    // 경우(레포 .claude/hooks가 cwd 기준으로 없음)는 조용히 스킵한다.
    check_hook_sync();

    if fails == 0 {
        println!("\n진단 통과. `tunaround node`로 상주하세요.");
        0
    } else {
        println!("\n{fails}개 항목 FAIL. 위를 고친 뒤 다시 진단하세요.");
        1
    }
}

/// 레포 훅 파일 하나와 그 배포 사본의 비교 결과(#6, 순수부 - 테스트 용이하게 IO만 여기서 하고 판정은
/// 반환값으로 낸다).
#[cfg(all(feature = "serve", feature = "worker"))]
#[derive(Debug, PartialEq)]
enum HookSyncStatus {
    Match,
    Mismatch,
    Missing,
}

/// 레포 훅 경로와 배포 사본 경로를 바이트 단위로 비교한다. 레포 파일 자체를 못 읽으면 None(비교
/// 불가 - 호출부가 건너뜀).
#[cfg(all(feature = "serve", feature = "worker"))]
fn hook_sync_status(
    repo_path: &std::path::Path,
    home_path: &std::path::Path,
) -> Option<HookSyncStatus> {
    let repo_bytes = std::fs::read(repo_path).ok()?;
    Some(match std::fs::read(home_path) {
        Ok(home_bytes) if home_bytes == repo_bytes => HookSyncStatus::Match,
        Ok(_) => HookSyncStatus::Mismatch,
        Err(_) => HookSyncStatus::Missing,
    })
}

/// 레포 `.claude/hooks/*.py`와 실제 실행되는 `~/.claude/hooks/*.py` 사본을 바이트 단위로 비교해
/// 불일치·부재를 WARN으로 보고한다(#6, doctor의 기존 OK/WARN/FAIL 출력 패턴을 따름). 레포 훅
/// 디렉터리가 없으면(cwd가 레포 밖) 아무것도 출력하지 않고 조용히 스킵한다. 정보성 진단이라 doctor의
/// fail count·exit code는 건드리지 않는다.
#[cfg(all(feature = "serve", feature = "worker"))]
fn check_hook_sync() {
    let repo_hooks = std::path::Path::new(".claude/hooks");
    let Ok(entries) = std::fs::read_dir(repo_hooks) else {
        return; // 레포 밖에서 실행 - 비교 대상 없음, 조용히 스킵.
    };
    let home_hooks = tunaround::config::expand_home("~/.claude/hooks");
    let mut checked = 0usize;
    let mut mismatched = 0usize;
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("py") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let dest = std::path::Path::new(&home_hooks).join(name);
        let Some(status) = hook_sync_status(&path, &dest) else {
            continue; // 레포 훅 자체를 못 읽으면 비교 불가 - 조용히 건너뜀.
        };
        checked += 1;
        match status {
            HookSyncStatus::Match => {}
            HookSyncStatus::Mismatch => {
                mismatched += 1;
                println!(
                    "WARN hook {name}: 레포 사본과 ~/.claude/hooks 사본 내용이 다릅니다(재복사 필요할 수 있음)"
                );
            }
            HookSyncStatus::Missing => {
                mismatched += 1;
                println!("WARN hook {name}: ~/.claude/hooks에 사본 없음(미배포)");
            }
        }
    }
    if checked > 0 && mismatched == 0 {
        println!("OK   hooks: 레포 훅 {checked}개 전부 ~/.claude/hooks 사본과 일치");
    }
}

#[cfg(all(test, feature = "serve", feature = "worker"))]
mod tests {
    use super::*;
    use crate::cli::InitArgs;

    #[test]
    fn detect_machine_uses_explicit_then_os() {
        assert_eq!(detect_machine(Some("mac")), "mac");
        assert_eq!(detect_machine(Some("win")), "win");
        // 명시 없으면 OS 감지 - 셋 중 하나.
        let m = detect_machine(None);
        assert!(
            ["win", "mac", "unix"].contains(&m.as_str()),
            "감지된 머신 태그: {m}"
        );
    }

    #[test]
    fn hook_sync_status_matches_mismatches_and_missing() {
        let dir = std::env::temp_dir().join(format!("tuna-hooksync-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let repo = dir.join("repo.py");
        let same = dir.join("same.py");
        let diff = dir.join("diff.py");
        let missing = dir.join("missing.py");
        std::fs::write(&repo, "print(1)").unwrap();
        std::fs::write(&same, "print(1)").unwrap();
        std::fs::write(&diff, "print(2)").unwrap();

        assert_eq!(hook_sync_status(&repo, &same), Some(HookSyncStatus::Match));
        assert_eq!(
            hook_sync_status(&repo, &diff),
            Some(HookSyncStatus::Mismatch)
        );
        assert_eq!(
            hook_sync_status(&repo, &missing),
            Some(HookSyncStatus::Missing)
        );
        // 레포 파일 자체가 없으면 비교 불가(None).
        assert_eq!(hook_sync_status(&missing, &same), None);

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn mesh_config_content_has_keys_and_placeholder_token() {
        let c = mesh_config_content(
            "http://127.0.0.1:8770/mcp",
            "mac",
            "/usr/local/bin/tunaround",
            false,
        );
        assert!(c.contains("TUNA_AUTOARM=1"), "autoarm 스위치 누락");
        assert!(
            c.contains("TUNA_BROKER_CORE=http://127.0.0.1:8770/mcp"),
            "코어 URL 미보간"
        );
        assert!(c.contains("TUNA_MACHINE=mac"), "머신 태그 미보간");
        assert!(
            c.contains("TUNA_BIN=/usr/local/bin/tunaround"),
            "bin 경로 미보간"
        );
        // 토큰은 placeholder만: 실값을 쓰지 않는다. local=false라 주석 처리되지 않은 채로 존재.
        assert!(
            c.contains("TUNA_BROKER_TOKEN=여기에-실제-토큰-넣기"),
            "토큰 placeholder 누락"
        );
        assert!(
            !c.contains("# TUNA_BROKER_TOKEN="),
            "local=false면 토큰 줄이 주석 처리되면 안 됨"
        );
    }

    #[test]
    fn mesh_config_content_local_comments_out_token_line() {
        // local=true(P0-①): 토큰 줄이 주석으로 남아 LAN 확장 시 발견 가능해야 하고,
        // 활성 TUNA_BROKER_TOKEN= 줄은 없어야 한다(무토큰 계약).
        let c = mesh_config_content(
            "http://127.0.0.1:8770/mcp",
            "win",
            "/usr/local/bin/tunaround",
            true,
        );
        assert!(
            c.contains("# TUNA_BROKER_TOKEN=여기에-실제-토큰-넣기"),
            "local이면 토큰 줄이 주석으로 남아야 함: {c}"
        );
        assert!(
            !c.lines().any(|l| l.starts_with("TUNA_BROKER_TOKEN=")),
            "local이면 활성 TUNA_BROKER_TOKEN= 줄이 없어야 함: {c}"
        );
    }

    #[test]
    fn is_loopback_listen_detects_loopback_forms() {
        assert!(is_loopback_listen("127.0.0.1:8770"));
        assert!(
            is_loopback_listen("127.5.5.5:9999"),
            "127.0.0.0/8 리터럴은 전부 loopback"
        );
        assert!(is_loopback_listen("localhost:8770"));
        assert!(is_loopback_listen("[::1]:8770"));
        assert!(
            is_loopback_listen("[::1%lo0]:8770"),
            "IPv6 zone identifier는 떼고 판정(gemini)"
        );
        assert!(!is_loopback_listen("0.0.0.0:8770"));
        assert!(
            !is_loopback_listen("192.0.2.10:8770"),
            "LAN IP는 비-loopback"
        );
        assert!(
            !is_loopback_listen("127.attacker.example:8770"),
            "127. 접두사 호스트명은 리터럴 IP가 아니라 신뢰하지 않는다(CodeRabbit)"
        );
    }

    #[test]
    fn broker_core_url_from_listen_propagates_port_and_rewrites_wildcard() {
        // 사용자 지정 포트가 훅 config·MCP 등록 URL에 그대로 전파된다(CodeRabbit).
        assert_eq!(
            broker_core_url_from_listen("127.0.0.1:9999"),
            "http://127.0.0.1:9999/mcp"
        );
        // 와일드카드 바인드는 그 주소로 접속 불가라 127.0.0.1로 치환(포트 보존).
        assert_eq!(
            broker_core_url_from_listen("0.0.0.0:8123"),
            "http://127.0.0.1:8123/mcp"
        );
        assert_eq!(
            broker_core_url_from_listen("[::]:8770"),
            "http://127.0.0.1:8770/mcp"
        );
        // IPv6 리터럴은 대괄호 유지.
        assert_eq!(
            broker_core_url_from_listen("[::1]:8770"),
            "http://[::1]:8770/mcp"
        );
        // LAN 바인드는 그 주소 그대로(훅이 같은 머신에서 접속 가능).
        assert_eq!(
            broker_core_url_from_listen("192.0.2.10:8770"),
            "http://192.0.2.10:8770/mcp"
        );
    }

    #[test]
    fn plan_auto_lanes_scaffolds_one_lane_per_found_runner() {
        let lanes = plan_auto_lanes(&["claude", "codex"], "worker");
        assert_eq!(
            lanes,
            vec![
                AutoLane {
                    agent: "claude-worker".to_string(),
                    runner: "claude".to_string(),
                },
                AutoLane {
                    agent: "codex-worker".to_string(),
                    runner: "codex".to_string(),
                },
            ]
        );
    }

    #[test]
    fn plan_auto_lanes_falls_back_to_claude_when_none_found() {
        let lanes = plan_auto_lanes(&[], "worker");
        assert_eq!(
            lanes,
            vec![AutoLane {
                agent: "worker".to_string(),
                runner: "claude".to_string(),
            }]
        );
    }

    #[test]
    fn plan_mcp_registration_no_claude_binary() {
        assert_eq!(
            plan_mcp_registration("http://127.0.0.1:8770/mcp", false, false),
            McpRegistrationPlan::NoClaudeBinary
        );
    }

    #[test]
    fn plan_mcp_registration_already_registered_skips() {
        assert_eq!(
            plan_mcp_registration("http://127.0.0.1:8770/mcp", true, true),
            McpRegistrationPlan::AlreadyRegistered
        );
    }

    #[test]
    fn plan_mcp_registration_unregistered_builds_add_command() {
        let plan = plan_mcp_registration("http://127.0.0.1:8770/mcp", true, false);
        assert_eq!(
            plan,
            McpRegistrationPlan::Register {
                add_args: vec![
                    "mcp".to_string(),
                    "add".to_string(),
                    "--transport".to_string(),
                    "http".to_string(),
                    "--scope".to_string(),
                    "user".to_string(),
                    "tuna-broker".to_string(),
                    "http://127.0.0.1:8770/mcp".to_string(),
                ]
            }
        );
    }

    /// InitArgs 테스트 빌더. no_mcp_register는 항상 true로 고정한다: 이 머신에 실제
    /// `claude mcp add/get`을 실행하는 부작용을 테스트에서 절대 만들지 않기 위함(spec 요구).
    fn init_args_for_test(listen: Option<&str>, runner: Option<&str>, out: &str) -> InitArgs {
        InitArgs {
            core: None,
            listen: listen.map(|s| s.to_string()),
            agent: None,
            runner: runner.map(|s| s.to_string()),
            project: Some("/tmp/proj".to_string()),
            token_env: None, // 기본값 = TUNA_BROKER_TOKEN(통일)
            machine: None,
            out: Some(out.to_string()),
            no_mesh_config: true,
            force: false,
            no_mcp_register: true,
        }
    }

    #[test]
    fn run_init_local_default_omits_token_key_and_binds_loopback() {
        // core/listen 미지정 -> P0-① 기본이 127.0.0.1:8770(로컬 무토큰 계약)이라 token 키가 없어야 함.
        let out = std::env::temp_dir()
            .join("tuna_init_test_local.toml")
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(&out);
        let args = init_args_for_test(None, Some("claude"), &out);
        assert_eq!(run_init(&args), 0, "init 성공(0) 반환");
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(
            written.contains("listen = \"127.0.0.1:8770\""),
            "로컬 기본 listen이 127.0.0.1:8770이어야 함: {written}"
        );
        assert!(
            !written.contains("token ="),
            "loopback이면 token 키 자체가 없어야 함: {written}"
        );
        let _ = std::fs::remove_file(&out);
    }

    #[test]
    fn run_init_non_loopback_listen_keeps_token_key() {
        // --listen으로 비-loopback을 명시하면 기존 동작(토큰 키 포함) 유지.
        let out = std::env::temp_dir()
            .join("tuna_init_test_nonlocal.toml")
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(&out);
        let args = init_args_for_test(Some("0.0.0.0:8770"), Some("claude"), &out);
        assert_eq!(run_init(&args), 0, "init 성공(0) 반환");
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(
            written.contains("token = \"@env:TUNA_BROKER_TOKEN\""),
            "토큰 env 이름이 TUNA_BROKER_TOKEN으로 통일되어야 함: {written}"
        );
        assert!(
            written.contains("runner = \"claude\""),
            "러너 미기록: {written}"
        );
        let _ = std::fs::remove_file(&out);
    }
}
