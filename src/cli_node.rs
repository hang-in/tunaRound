// node·온보딩 서브커맨드의 헬퍼(레인 러너 조립·재시도 접속·init·doctor) (main.rs에서 분할, T4.5).

#[cfg(all(feature = "serve", feature = "worker"))]
use crate::cli::InitArgs;

/// lane.runner(문자열)로부터 Runner를 만든다. 알 수 없는 이름·미충족 피처는 Err.
// token은 runner=http(engines) 경로에서만 쓰여, engines 미포함 빌드에선 미사용이 정상이다.
#[cfg(feature = "worker")]
#[cfg_attr(not(feature = "engines"), allow(unused_variables))]
pub fn build_lane_runner(
    lane: &tunaround::config::Lane,
    token: &Option<String>,
) -> Result<std::sync::Arc<dyn tunaround::runner::Runner + Send + Sync>, String> {
    use std::sync::Arc;
    let runner: Arc<dyn tunaround::runner::Runner + Send + Sync> = match lane.runner.as_str() {
        "claude" => Arc::new(tunaround::runner::claude::ClaudeRunner::new()),
        "codex" => Arc::new(tunaround::runner::codex::CodexRunner::new()),
        "opencode" => {
            Arc::new(tunaround::runner::opencode::OpencodeRunner::new().with_model(lane.model.clone()))
        }
        #[cfg(feature = "engines")]
        "http" => {
            let base =
                lane.http_base_url.as_deref().ok_or("lane runner=http는 http_base_url이 필요합니다")?;
            Arc::new(tunaround::runner::http::OpenAiChatRunner::new(
                base,
                lane.model.as_deref().unwrap_or(""),
                token.clone(),
            ))
        }
        #[cfg(not(feature = "engines"))]
        "http" => return Err("lane runner=http는 engines 피처가 필요합니다".to_string()),
        #[cfg(feature = "a2a-out")]
        "a2a" => {
            let card = lane.a2a_card.as_deref().ok_or("lane runner=a2a는 a2a_card가 필요합니다")?;
            Arc::new(tunaround::runner::a2a::A2ARunner::new(card.to_string(), lane.a2a_token.clone()))
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
        match tunaround::mcp_client::McpHttpClient::connect(core_url.to_string(), token.clone()).await {
            Ok(c) => return Ok(c),
            Err(e) => {
                last = e;
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }
    Err(format!("코어 연결 실패({tries}회 재시도): {last}"))
}


/// node.toml을 생성한다(플래그 주도). 러너 자동 탐지 + 다음 단계 안내. 성공 0, 실패 non-zero.
#[cfg(all(feature = "serve", feature = "worker"))]
pub fn run_init(args: &InitArgs) -> i32 {
    let out =
        args.out.clone().unwrap_or_else(|| tunaround::config::expand_home("~/.tunaround/node.toml"));
    if std::path::Path::new(&out).exists() && !args.force {
        eprintln!("[init] 이미 존재합니다: {out} (덮어쓰려면 --force)");
        return 1;
    }
    let core = args.core.clone().unwrap_or_else(|| "self".to_string());
    let agent = args.agent.clone().unwrap_or_else(|| "worker".to_string());
    let runner = args.runner.clone().unwrap_or_else(|| {
        ["claude", "codex", "opencode"]
            .iter()
            .find(|b| binary_on_path(b))
            .map(|s| s.to_string())
            .unwrap_or_else(|| "claude".to_string())
    });
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
    let token_env = args.token_env.clone().unwrap_or_else(|| "TUNA_BROKER_TOKEN".to_string());

    let mut toml = format!("core = \"{core}\"\n");
    if core == "self" {
        let listen = args.listen.clone().unwrap_or_else(|| "0.0.0.0:8770".to_string());
        toml.push_str(&format!("listen = \"{listen}\"\n"));
        toml.push_str("db = \"~/.tunaround/broker.db\"\n");
    }
    toml.push_str(&format!("token = \"@env:{token_env}\"\n\n"));
    toml.push_str("[[lane]]\n");
    toml.push_str(&format!("agent = \"{agent}\"\n"));
    toml.push_str(&format!("runner = \"{runner}\"\n"));
    toml.push_str("mode = \"read-only\"   # 파일 수정 맡기려면 \"write\"\n");
    toml.push_str(&format!("project = \"{project}\"\n"));
    toml.push_str("interval = 20\n");

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

    // mesh·훅용 ~/.tunaround/config(dotenv)도 한 번에 스캐폴드해 "설정 파일 3종"을 최초 1회로 압축한다.
    // 기존 config는 실제 토큰을 담고 있을 수 있으므로 --force 없이는 절대 덮지 않는다(토큰 보존).
    let config_written = if args.no_mesh_config {
        false
    } else {
        scaffold_mesh_config(&core, args.machine.as_deref(), args.force)
    };

    println!("\n다음 단계:");
    if config_written {
        println!(
            "  1) ~/.tunaround/config 의 TUNA_BROKER_TOKEN 을 실제 토큰으로 채우기(node·doctor·데몬·훅이 모두 이 토큰을 씁니다)"
        );
    } else {
        println!(
            "  1) 토큰: export {token_env}=<비밀토큰>  (Windows PowerShell: $env:{token_env}=\"...\")"
        );
    }
    println!("  2) 진단: tunaround doctor");
    println!("  3) 상주: tunaround node   (mesh 전체는 restart 스크립트가 config를 읽어 데몬에 상속)");
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
/// 토큰은 placeholder만 넣는다(실값 금지).
#[cfg(all(feature = "serve", feature = "worker"))]
fn mesh_config_content(broker_core: &str, machine: &str, bin: &str) -> String {
    format!(
        "# tunaRound mesh·훅 설정(tunaround init 자동 생성). 값을 채운 뒤 SessionStart 훅과 restart\n\
         # 스크립트가 읽는다. 형식=KEY=VALUE, 우선순위=이 파일 > env > 기본값. 상세=docs/reference/onboarding.md\n\
         TUNA_AUTOARM=1\n\
         TUNA_BIN={bin}\n\
         TUNA_BROKER_CORE={broker_core}\n\
         TUNA_MACHINE={machine}\n\
         # 브로커 인증 토큰(평문). 아래를 실제 토큰으로 바꾸세요. node.toml의 @env:TUNA_BROKER_TOKEN도\n\
         # 이 이름을 씁니다. 파일 권한 제한 권장(mac/linux: chmod 600, Windows: icacls 본인만 R/W).\n\
         TUNA_BROKER_TOKEN=여기에-실제-토큰-넣기\n"
    )
}

/// ~/.tunaround/config(mesh·훅용 dotenv)를 스캐폴드한다. 이미 있으면(force 아님) 실토큰 보존 위해
/// 건드리지 않고 false를 반환한다. 토큰은 실값을 쓰지 않고 placeholder만 넣어 사용자가 채우게 한다
/// (토큰이 argv/명령 히스토리에 남지 않게). node.toml의 @env:TUNA_BROKER_TOKEN과 같은 이름이라
/// restart 스크립트가 이 파일을 읽어 데몬 env로 상속하면 node·데몬·훅이 한 토큰을 공유한다.
#[cfg(all(feature = "serve", feature = "worker"))]
fn scaffold_mesh_config(core: &str, machine: Option<&str>, force: bool) -> bool {
    let path = tunaround::config::expand_home("~/.tunaround/config");
    if std::path::Path::new(&path).exists() && !force {
        println!("\n참고: {path} 는 이미 있어 건드리지 않았습니다(실토큰 보존, 덮으려면 --force).");
        return false;
    }
    // core=self면 브로커가 로컬이라 loopback URL, 아니면 넘겨받은 코어 URL을 그대로 쓴다.
    let broker_core = if core == "self" { "http://127.0.0.1:8770/mcp" } else { core };
    let machine = detect_machine(machine);
    let bin = std::env::current_exe()
        .ok()
        .and_then(|p| p.to_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "tunaround".to_string());
    let content = mesh_config_content(broker_core, &machine, &bin);
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
    println!("\n작성됨: {path} (TUNA_BROKER_TOKEN 을 채우세요)");
    true
}

/// 실행 파일이 PATH에 있는지 확인한다(Windows는 .exe/.cmd/.bat 확장자도 시도).
#[cfg(all(feature = "serve", feature = "worker"))]
pub fn binary_on_path(name: &str) -> bool {
    let exts: &[&str] = if cfg!(windows) { &["", ".exe", ".cmd", ".bat"] } else { &[""] };
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
                let mut req =
                    reqwest::blocking::Client::new().get(&url).timeout(std::time::Duration::from_secs(3));
                if let Some(t) = &token {
                    req = req.bearer_auth(t);
                }
                match req.send() {
                    Ok(r) if r.status().is_success() => {
                        println!("OK   core=self: {listen} 사용 중이나 브로커가 응답(node가 이미 구동 중)")
                    }
                    // 401/403만 "우리 브로커인데 인증 문제"로 보고 WARN. 404 등 다른 non-2xx는
                    // 무관한 프로세스(일반 웹서버)일 가능성이 커서 FAIL(node 기동 시 포트 충돌 예방).
                    Ok(r) if r.status() == reqwest::StatusCode::UNAUTHORIZED
                        || r.status() == reqwest::StatusCode::FORBIDDEN =>
                    {
                        println!("WARN core=self: {listen} 사용 중, 브로커가 {} (우리 브로커로 보이나 토큰 확인 필요)", r.status())
                    }
                    Ok(r) => {
                        println!("FAIL core=self: {listen} 점유한 프로세스가 {} 응답(우리 브로커 아님, 포트 충돌)", r.status());
                        fails += 1;
                    }
                    // 전송 자체 실패 = HTTP 응답 없음. 다른 프로세스가 비-HTTP로 점유 중일 수 있다.
                    Err(_) => {
                        println!("FAIL core=self: {listen} 사용 중이고 HTTP 응답 없음(다른 프로세스 점유?)");
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
                    println!("WARN db: 상위 디렉터리 없음 {} (node가 만들거나 실패할 수 있음)", p.display());
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
        let kind = if l.is_supervised() { "감독" } else { "자동" };
        match l.runner.as_str() {
            b @ ("claude" | "codex" | "opencode") => {
                if binary_on_path(b) {
                    println!("OK   lane {}[{kind}] runner={b}: PATH에 있음", l.agent);
                } else {
                    println!("FAIL lane {}[{kind}] runner={b}: PATH에 없음(설치/로그인 필요)", l.agent);
                    fails += 1;
                }
            }
            // http/a2a는 바이너리 대신 필수 설정을 검증한다(누락 시 node가 build_lane_runner에서 실패, gemini 지적).
            // http/a2a는 필수 설정 + 그 러너를 지원하는 피처가 이 바이너리에 컴파일됐는지도 본다
            // (피처 없이 빌드되면 node가 build_lane_runner에서 실패하므로, doctor가 미리 잡는다).
            "http" => {
                #[cfg(not(feature = "engines"))]
                {
                    println!("FAIL lane {}[{kind}] runner=http: 이 바이너리는 engines 피처 없이 빌드됨", l.agent);
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
                                println!("OK   lane {}[{kind}] runner=http: base_url {u} 도달", l.agent);
                            } else {
                                println!(
                                    "WARN lane {}[{kind}] runner=http: base_url {u} 도달 불가(LLM 미기동?)",
                                    l.agent
                                );
                            }
                        }
                    }
                    None => {
                        println!("FAIL lane {}[{kind}] runner=http: http_base_url 누락", l.agent);
                        fails += 1;
                    }
                }
            }
            "a2a" => {
                #[cfg(not(feature = "a2a-out"))]
                {
                    println!("FAIL lane {}[{kind}] runner=a2a: 이 바이너리는 a2a-out 피처 없이 빌드됨", l.agent);
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
                println!("FAIL lane {}[{kind}] runner={other}: 알 수 없는 runner", l.agent);
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

    if fails == 0 {
        println!("\n진단 통과. `tunaround node`로 상주하세요.");
        0
    } else {
        println!("\n{fails}개 항목 FAIL. 위를 고친 뒤 다시 진단하세요.");
        1
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
        assert!(["win", "mac", "unix"].contains(&m.as_str()), "감지된 머신 태그: {m}");
    }

    #[test]
    fn mesh_config_content_has_keys_and_placeholder_token() {
        let c = mesh_config_content("http://127.0.0.1:8770/mcp", "mac", "/usr/local/bin/tunaround");
        assert!(c.contains("TUNA_AUTOARM=1"), "autoarm 스위치 누락");
        assert!(c.contains("TUNA_BROKER_CORE=http://127.0.0.1:8770/mcp"), "코어 URL 미보간");
        assert!(c.contains("TUNA_MACHINE=mac"), "머신 태그 미보간");
        assert!(c.contains("TUNA_BIN=/usr/local/bin/tunaround"), "bin 경로 미보간");
        // 토큰은 placeholder만: 실값을 쓰지 않는다.
        assert!(c.contains("TUNA_BROKER_TOKEN=여기에-실제-토큰-넣기"), "토큰 placeholder 누락");
    }

    #[test]
    fn run_init_writes_node_toml_with_unified_token_env() {
        // no_mesh_config=true로 실 ~/.tunaround/config는 절대 건드리지 않고 node.toml만 검증한다.
        let out = std::env::temp_dir()
            .join("tuna_init_test_node.toml")
            .to_string_lossy()
            .into_owned();
        let _ = std::fs::remove_file(&out);
        let args = InitArgs {
            core: None,
            listen: None,
            agent: None,
            runner: Some("claude".to_string()),
            project: Some("/tmp/proj".to_string()),
            token_env: None, // 기본값 = TUNA_BROKER_TOKEN(통일)
            machine: None,
            out: Some(out.clone()),
            no_mesh_config: true,
            force: false,
        };
        assert_eq!(run_init(&args), 0, "init 성공(0) 반환");
        let written = std::fs::read_to_string(&out).unwrap();
        assert!(
            written.contains("token = \"@env:TUNA_BROKER_TOKEN\""),
            "토큰 env 이름이 TUNA_BROKER_TOKEN으로 통일되어야 함: {written}"
        );
        assert!(written.contains("runner = \"claude\""), "러너 미기록: {written}");
        let _ = std::fs::remove_file(&out);
    }
}

