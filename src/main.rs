// tunaRound 바이너리 진입점. 두 에이전트 토론 REPL을 구동한다.

use std::io::{self, Write};

use tunaround::orchestrator::{MapRegistry, Participant};
use tunaround::repl::{parse_command, Session, StepOutcome};
use tunaround::runner::claude::ClaudeRunner;
use tunaround::runner::codex::CodexRunner;

fn main() {
    // 인자: [--roster <path>] [--observe <id>] [--session <id>] [<state.json>]
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut roster_path: Option<String> = None;
    let mut state_path: Option<String> = None;
    let mut observe_id: Option<String> = None;
    let mut redis_session_id: Option<String> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--roster" => {
                roster_path = args.get(i + 1).cloned();
                i += 2;
            }
            "--observe" => {
                observe_id = args.get(i + 1).cloned();
                i += 2;
            }
            "--session" => {
                redis_session_id = args.get(i + 1).cloned();
                i += 2;
            }
            other => {
                if state_path.is_none() {
                    state_path = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    // tokio 런타임: read path(--observe/--session snapshot GET) + owner refresh에만 사용.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");

    // --observe 모드: REPL 대신 라이브 구독 루프(read-only).
    if let Some(sid) = observe_id {
        let Some(bus) = tunaround::session_bus::RedisBus::open_from_env() else {
            eprintln!("[observe] TUNAROUND_REDIS_URL 필요");
            std::process::exit(1);
        };
        rt.block_on(async move {
            if let Ok(Some(snap)) = bus.get_snapshot(&sid).await {
                println!("=== 현재 스냅샷 ===\n{snap}\n=== 라이브 ===");
            }
            let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(256);
            let subscriber = {
                let bus = bus.clone();
                let sid = sid.clone();
                tokio::spawn(async move {
                    let _ = bus.subscribe_events(&sid, tx).await;
                })
            };
            while let Ok(payload) = rx.recv().await {
                println!("{payload}");
            }
            let _ = subscriber.await;
        });
        return;
    }

    // 로스터 파일이 있으면 동적 좌석, 없으면 기본 2자리(claude proposer + codex reviewer).
    let (participants, registry): (Vec<Participant>, MapRegistry) = match &roster_path {
        Some(p) => {
            let roster = match tunaround::roster::load_roster(p) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            let parts = match tunaround::roster::build_participants_checked(&roster) {
                Ok(v) => v,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            let reg = match tunaround::roster::build_registry(&roster) {
                Ok(r) => r,
                Err(e) => { eprintln!("[로스터 실패] {e}"); std::process::exit(1); }
            };
            (parts, reg)
        }
        None => {
            let mut reg = MapRegistry::new();
            reg.insert("claude", Box::new(ClaudeRunner::new()));
            reg.insert("codex", Box::new(CodexRunner::new()));
            let parts = vec![
                Participant { engine: "claude".into(), role: Some("proposer".into()), instruction: String::new() },
                Participant { engine: "codex".into(), role: Some("reviewer".into()), instruction: String::new() },
            ];
            (parts, reg)
        }
    };

    // bus 핸들 준비: TUNAROUND_REDIS_URL 없으면 None(기존 동작 불변).
    // RedisBusHandle::spawn은 tokio::spawn을 내부 호출하므로 rt.enter() 가드 안에서 생성.
    let _g = rt.enter();
    let bus_handle = tunaround::session_bus::RedisBusHandle::spawn_from_env();
    drop(_g);
    let bus_boxed = bus_handle.map(|h| Box::new(h) as Box<dyn tunaround::session_bus::SessionBus>);

    // session_id: --session <id> 값 또는 "default".
    let sid = redis_session_id.clone().unwrap_or_else(|| "default".to_string());

    // 세션 초기 상태 결정(우선순위: 파일 resume > Redis snapshot > 신규).
    let resume_existing = state_path
        .as_deref()
        .map(|p| std::path::Path::new(p).exists())
        .unwrap_or(false);

    let mut session = if resume_existing {
        // 파일에서 트리 상태를 로드하고 new_with_bus로 bus를 연결한다.
        let p = state_path.as_deref().unwrap();
        match tunaround::store::load_session(p) {
            Ok(ss) => {
                println!("(이어받음: {p})");
                let mut s = Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus_boxed);
                s.seed_from(ss);
                s
            }
            Err(e) => {
                eprintln!("[resume 실패: {e}] 종료합니다.");
                std::process::exit(1);
            }
        }
    } else if redis_session_id.is_some() {
        // --session <id>: Redis snapshot에서 재개(라이브 Redis 있을 때만 실제 동작).
        if let Some(raw_bus) = tunaround::session_bus::RedisBus::open_from_env() {
            match rt.block_on(raw_bus.get_snapshot(&sid)) {
                Ok(Some(json)) => {
                    match serde_json::from_str::<tunaround::store::StoredSession>(&json) {
                        Ok(ss) => {
                            println!("(Redis snapshot 재개: {sid})");
                            let _g2 = rt.enter();
                            let bus2 = tunaround::session_bus::RedisBusHandle::spawn_from_env()
                                .map(|h| Box::new(h) as Box<dyn tunaround::session_bus::SessionBus>);
                            drop(_g2);
                            let mut s = Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus2);
                            s.seed_from(ss);
                            // owner lease 시도.
                            let worker_id = std::process::id().to_string();
                            match rt.block_on(raw_bus.try_acquire_owner(&sid, &worker_id, 60)) {
                                Ok(true) => {
                                    // 백그라운드 owner refresh.
                                    let refresh_bus = raw_bus.clone();
                                    let refresh_sid = sid.clone();
                                    let refresh_wid = worker_id.clone();
                                    rt.spawn(async move {
                                        loop {
                                            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                            let _ = refresh_bus.refresh_owner(&refresh_sid, &refresh_wid, 60).await;
                                        }
                                    });
                                }
                                Ok(false) => eprintln!("[경고] 다른 프로세스가 driver일 수 있음: {sid}"),
                                Err(e) => eprintln!("[owner lease 실패] {e}"),
                            }
                            s
                        }
                        Err(e) => {
                            eprintln!("[snapshot 파싱 실패: {e}] 신규 세션 시작.");
                            Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus_boxed)
                        }
                    }
                }
                _ => {
                    eprintln!("[snapshot 없음] 신규 세션 시작.");
                    Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus_boxed)
                }
            }
        } else {
            eprintln!("[--session] TUNAROUND_REDIS_URL 없음: 로컬 단일세션으로 시작.");
            Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus_boxed)
        }
    } else {
        Session::new_with_bus(participants, Box::new(registry), sid.clone(), bus_boxed)
    };

    println!("tunaRound - 메시지를 입력하세요. /help, /save, /quit.");
    let stdin = io::stdin();
    loop {
        print!("\n> ");
        let _ = io::stdout().flush();
        let mut line = String::new();
        if stdin.read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        match session.step(parse_command(&line)) {
            StepOutcome::Print(text) => println!("{text}"),
            StepOutcome::Noop => {}
            StepOutcome::Save { path, markdown } => match std::fs::write(&path, markdown) {
                Ok(()) => println!("저장됨: {path}"),
                Err(e) => println!("[저장 실패] {e}"),
            },
            StepOutcome::Exit => break,
        }
    }
    if let Some(p) = &state_path {
        match session.save_state(p) {
            Ok(()) => println!("세션 저장됨: {p}"),
            Err(e) => println!("[세션 저장 실패] {e}"),
        }
    }
}
