// 머신당 presence 스캐너: 로컬 라이브 세션(claude jsonl + codex rollout)을 열거해 브로커에 일괄 보고한다(v2-44).

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 스캐너가 발견한 라이브 세션 한 건. 브로커 report_presence의 sessions 원소로 직렬화된다.
#[derive(Debug, Clone, PartialEq)]
pub struct LiveSession {
    /// 세션 id(claude=jsonl stem, codex=session_meta.payload.session_id).
    pub uuid: String,
    /// 러너 종류(claude | codex).
    pub runner: String,
    /// 정규화된 프로젝트명(home 정규화 적용, 불명이면 None).
    pub project: Option<String>,
    /// 스캐너가 관측한 마지막 사람 입력 시각(v2-45 P5, codex rollout user_message tail 스캔, DB datetime
    /// 포맷). claude는 human-ping 훅이 별도 신호 경로라 None. 브로커 sync_presence가 max-merge한다.
    pub human_input_at: Option<String>,
    /// 세션 생성시각(codex session_meta timestamp, DB datetime 포맷). 이슈 #88 게이트의 신규-미입력 세션
    /// grace 신호(사람 입력 전이라도 최근 생성이면 유지). codex만 채우고 claude/idle/기타는 None.
    pub created_at: Option<String>,
    /// 마지막 활동 시각(이슈 #123, codex rollout mtime → DB datetime). 턴 생성 중 rollout append로
    /// mtime이 신선 = "지금 응답 생성 중" 프록시(15초 스캔 입도, 짧은 턴 FN 수용). claude는 turn-ping
    /// 훅이 정밀 신호라 None.
    pub active_at: Option<String>,
}

/// cwd가 홈 디렉토리 자체면 "home", 아니면 마지막 세그먼트. 훅의 project_from_cwd와 같은 규약
/// (개인 폴더명=사용자명이 project로 새는 것 방지, #42). cwd 불명이면 None.
pub fn project_from_cwd_normalized(cwd: Option<&str>, home: Option<&Path>) -> Option<String> {
    let cwd = cwd?;
    if let Some(h) = home {
        let p = Path::new(cwd);
        // 경로 문자열 비교(canonicalize는 존재하지 않는 원격 경로에서 실패) - 구분자만 통일.
        let norm = |s: &Path| {
            s.to_string_lossy()
                .replace('\\', "/")
                .trim_end_matches('/')
                .to_lowercase()
        };
        if norm(p) == norm(h) {
            return Some("home".to_string());
        }
    }
    crate::discover::project_from_cwd(cwd)
}

/// child가 base와 같거나 base 하위 경로인지(정규화된 슬래시 경로 문자열 기준, 할당 없음).
/// "/repo2"가 "/repo" 하위로 오검출되지 않게 base 뒤에 경로 구분자(`/`)가 와야 한다.
fn path_is_under(child: &str, base: &str) -> bool {
    child == base
        || child
            .strip_prefix(base)
            .is_some_and(|rest| rest.starts_with('/'))
}

/// cwd가 시스템 temp 아래인지(자동화 headless 세션 = 로스터 노이즈, 훅 is_temp_cwd와 같은 규약).
pub fn is_temp_cwd(cwd: &str) -> bool {
    let t = std::env::temp_dir();
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let (c, t) = (norm(cwd), norm(&t.to_string_lossy()));
    if path_is_under(&c, &t) {
        return true;
    }
    // macOS 관행 temp 프리픽스는 macOS에서만 적용한다. std::env::temp_dir()은 /var/folders/...만
    // 잡고 셸/도구가 흔히 쓰는 /tmp·/private/tmp·/private/var/folders 아래 cwd를 놓친다. 단 이들을
    // 전 플랫폼에서 매칭하면 Linux 프로젝트가 /private/tmp 아래 있을 때 오분류되므로 cfg로 macOS에
    // 한정한다(coderabbit).
    #[cfg(target_os = "macos")]
    {
        const MAC_TEMP_PREFIXES: [&str; 3] = ["/tmp", "/private/tmp", "/private/var/folders"];
        if MAC_TEMP_PREFIXES.iter().any(|p| path_is_under(&c, p)) {
            return true;
        }
    }
    false
}

/// 프로세스 목록 원문을 한 번 뜬다(win=tasklist CSV, unix=`ps -ax -o pid=,args=`).
/// 러너 카운트 게이트와 마커 생존 판정(parse_pids)이 같은 스냅샷을 공유한다.
/// 조회 실패는 None(판단 불가 = 게이트·마커 필터 모두 건너뜀 = 보수적 유지).
pub fn process_list_text() -> Option<(String, bool)> {
    let windows = cfg!(target_os = "windows");
    let mut cmd = if windows {
        let mut c = std::process::Command::new("tasklist");
        c.args(["/FO", "CSV", "/NH"]);
        c
    } else {
        // `-c`(comm 축약)는 procps/busybox 미이식 + comm은 node 래퍼로 뜨는 러너를 놓친다
        // (놓치면 게이트가 산 세션을 전부 제거, 봇리뷰 Major). pid + 전체 argv로 뜬다.
        let mut c = std::process::Command::new("ps");
        c.args(["-ax", "-o", "pid=,args="]);
        c
    };
    let (status, stdout) = output_with_deadline(&mut cmd, Duration::from_secs(10))?;
    if !status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&stdout).into_owned();
    // 건전성 가드(windows 한정): tasklist는 부하 시 exit 0인 채 "ERROR: ... timeout period
    // expired"만 뱉는다(2026-07-11 실측: 5회 중 3회). 그 스냅샷으로 필터하면 전 세션이 한
    // 사이클 떨어졌다 복귀하는 로스터 깜빡임이 된다. Windows 데스크톱 프로세스는 항상 수백
    // 개이므로 파싱 pid가 비정상적으로 적으면 스냅샷 실패로 간주하고 None(=이번 주기 필터
    // 스킵, 보수적 유지). unix는 경량 컨테이너에서 프로세스 <20이 정상이라 제외(봇리뷰 high).
    if windows && parse_pids(&text, windows).len() < 20 {
        return None;
    }
    Some((text, windows))
}

/// Command를 데드라인 안에서 실행하고 (status, stdout)을 돌려준다. 초과 시 kill 후 None.
/// tasklist는 부하 시 에러 출력(#51 가드)뿐 아니라 출력 0으로 무한 행도 실측돼(2026-07-11,
/// 15초+ 무응답) `.output()` 블로킹이 스캐너 루프 전체를 멈춘다 - 데드라인이 그 행을
/// 스냅샷 실패(=이번 주기 필터 스킵, 보수적 유지)로 강등시킨다.
fn output_with_deadline(
    cmd: &mut std::process::Command,
    deadline: Duration,
) -> Option<(std::process::ExitStatus, Vec<u8>)> {
    use std::io::Read;
    let mut child = cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let mut pipe = child.stdout.take()?;
    // 파이프는 별도 스레드로 읽는다(안 읽으면 자식이 파이프 가득참에 막혀 교착).
    let reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        pipe.read_to_end(&mut buf).ok().map(|_| buf)
    });
    let end = std::time::Instant::now() + deadline;
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break st,
            // 데드라인 초과와 try_wait 에러 모두 kill+wait로 정리해야 자식·reader
            // 스레드가 안 샌다(봇리뷰: `?` 조기 반환은 자식을 산 채로 누수).
            Ok(None) if std::time::Instant::now() >= end => {
                let _ = child.kill();
                let _ = child.wait();
                return None; // reader 스레드는 파이프가 닫히며 스스로 끝난다.
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let stdout = reader.join().ok().flatten()?;
    Some((status, stdout))
}

/// 프로세스 목록 텍스트에서 러너 라인을 센다(순수부). win = CSV 첫 필드(이미지명) /
/// unix = pid 토큰 다음 argv 앞 3개 토큰의 **basename** 매칭(경로·`node /path/claude` 인터프리터
/// 래퍼 커버). 뒤쪽 인자의 우연 매칭(`--runner claude` 등)은 게이트 미발동 방향 오차라 허용하되,
/// 3토큰 상한으로 과확장을 막는다.
pub fn count_matching_lines(text: &str, name: &str, windows: bool) -> usize {
    let text = text.to_lowercase();
    let needle = name.to_lowercase();
    let exe = format!("{needle}.exe");
    let matches_token = |tok: &str| {
        let tok = tok.trim_matches('"').trim();
        let base = std::path::Path::new(tok)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(tok);
        base == needle || base == exe
    };
    text.lines()
        .filter(|l| {
            if windows {
                l.split(',').next().is_some_and(matches_token)
            } else {
                // 첫 토큰은 pid(ps -o pid=,args=) → 건너뛰고 argv 앞 3개만 본다.
                l.split_whitespace().skip(1).take(3).any(matches_token)
            }
        })
        .count()
}

/// 프로세스 목록 텍스트에서 살아있는 PID 집합을 뽑는다(마커 생존 판정용).
/// win CSV = 둘째 필드, unix = 첫 토큰.
pub fn parse_pids(text: &str, windows: bool) -> std::collections::HashSet<u32> {
    text.lines()
        .filter_map(|l| {
            let tok = if windows {
                l.split(',').nth(1).map(|s| s.trim_matches('"').trim())
            } else {
                l.split_whitespace().next()
            };
            tok.and_then(|t| t.parse::<u32>().ok())
        })
        .collect()
}

/// 프로세스 목록에서 특정 러너(name) 이름 프로세스의 살아있는 PID 집합을 뽑는다(순수부, v2-45 P8
/// 가드①: "pid 생존"과 "그 pid가 실제 claude"를 한 번에 판정 - 단순 pid 생존만으론 pid 재사용을
/// 못 막는다). win CSV = 이미지명 필드가 name(.exe)인 행의 2번째 필드 pid / unix = pid 토큰 뒤
/// argv 앞 3개 토큰의 **basename**이 name인 행의 pid. 매칭 규약은 count_matching_lines(이름 판정)와
/// parse_pids(pid 추출)를 그대로 답습해 프로세스 게이트와 어긋나지 않게 한다.
pub fn runner_pids(text: &str, name: &str, windows: bool) -> std::collections::HashSet<u32> {
    let text = text.to_lowercase(); // 이름은 소문자 정규화, pid는 숫자라 무영향.
    let needle = name.to_lowercase();
    let exe = format!("{needle}.exe");
    let matches_token = |tok: &str| {
        let tok = tok.trim_matches('"').trim();
        let base = std::path::Path::new(tok)
            .file_name()
            .and_then(|f| f.to_str())
            .unwrap_or(tok);
        base == needle || base == exe
    };
    text.lines()
        .filter_map(|l| {
            if windows {
                // CSV: [이미지명, pid, ...]. 이미지명이 러너면 pid를 취한다.
                let mut fields = l.split(',');
                if !fields.next().is_some_and(matches_token) {
                    return None;
                }
                fields
                    .next()
                    .map(|s| s.trim_matches('"').trim())
                    .and_then(|p| p.parse::<u32>().ok())
            } else {
                // `ps -o pid=,args=`: 첫 토큰=pid, argv 앞 3개 basename이 러너면 그 pid를 취한다.
                let mut toks = l.split_whitespace();
                let pid = toks.next()?.parse::<u32>().ok()?;
                toks.take(3).any(matches_token).then_some(pid)
            }
        })
        .collect()
}

/// 세션 마커(.ctx)의 판독 결과. 훅(tuna_arm.write_marker)이 owner claude PID를 기록한다.
#[derive(Debug, Clone, PartialEq)]
pub enum MarkerState {
    /// 마커 파일 없음(훅 배포 전 세션·codex) → 판단 불가, 신선도 창으로 폴백(유지).
    NoMarker,
    /// owner PID 기록됨 → 프로세스 스냅샷과 대조해 생존 판정.
    Pid(u32),
    /// 마커는 있으나 PID 미상(owner 탐색 실패 등) → 보수적으로 유지.
    Unknown,
    /// tombstone: SessionEnd 훅이 깨끗한 종료(/clear·/exit·창닫기)를 확정 기록 → 즉시 제외.
    /// 종전엔 훅이 마커를 삭제했는데, 삭제=NoMarker=보수적 유지라서 jsonl mtime이 신선한 직전
    /// 세션이 창 만료(240분)까지 유령 B석으로 남았다(2026-07-11 실측 f09e84dc·46a26152). v2-46.
    Dead,
}

/// 세션 uuid의 마커를 읽는다(마커 디렉토리 = ~/.tunaround/autoarm, 훅과 같은 sanitize 규약 전제 -
/// uuid는 hex+하이픈이라 파일명 그대로).
/// 방어(gemini, 이슈 #119): codex 경로에선 uuid가 rollout 파일 **내용**(session_meta)에서 오므로
/// 신뢰 경계 밖이다 - 허용 문자 밖(경로 구분자 등)이 섞인 uuid는 join 전에 NoMarker로 거른다
/// (마커 파일명 sanitize 집합과 동일: 영숫자·`.`·`_`·`-`). 경로 이탈로 임의 파일을 읽는 것 차단.
pub fn read_marker(dir: &Path, uuid: &str) -> MarkerState {
    if uuid.is_empty()
        || !uuid
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        return MarkerState::NoMarker;
    }
    let path = dir.join(format!("{uuid}.ctx"));
    match std::fs::read_to_string(&path) {
        Err(_) => MarkerState::NoMarker,
        Ok(s) => {
            let t = s.trim();
            if t == "dead" {
                return MarkerState::Dead;
            }
            match t.parse::<u32>() {
                Ok(pid) => MarkerState::Pid(pid),
                Err(_) => MarkerState::Unknown,
            }
        }
    }
}

/// 마커 생존 판정(순수부): owner PID가 기록돼 있고 스냅샷에 없으면 죽은 세션(유령) → 제외.
/// tombstone(깨끗한 종료 확정)도 제외. 미기록·미상은 유지(오판으로 산 세션을 지우는 것보다
/// 유령이 창 만료로 늦게 죽는 쪽이 안전).
pub fn is_session_live(marker: &MarkerState, alive: &std::collections::HashSet<u32>) -> bool {
    match marker {
        MarkerState::Pid(pid) => alive.contains(pid),
        MarkerState::Dead => false,
        MarkerState::NoMarker | MarkerState::Unknown => true,
    }
}

/// tombstone 세션만 제거한다(순수부). PID 생존 판정과 달리 프로세스 스냅샷이 필요 없으므로,
/// 스냅샷 실패(tasklist 행·부하) 주기에도 항상 적용한다(깨끗한 종료 = 스냅샷 무관 확정 죽음).
pub fn filter_tombstoned(sessions: Vec<LiveSession>, marker_dir: &Path) -> Vec<LiveSession> {
    sessions
        .into_iter()
        .filter(|s| read_marker(marker_dir, &s.uuid) != MarkerState::Dead)
        .collect()
}

/// 세션 목록에 마커 생존 필터를 적용한다(v2-44 §10: /clear·창닫기·크래시 유령 즉시 제거).
pub fn filter_dead_sessions(
    sessions: Vec<LiveSession>,
    marker_dir: &Path,
    alive: &std::collections::HashSet<u32>,
) -> Vec<LiveSession> {
    sessions
        .into_iter()
        .filter(|s| is_session_live(&read_marker(marker_dir, &s.uuid), alive))
        .collect()
}

/// 마커-생존 유휴 세션 판정(순수부, v2-45 P8 가드①②). 입력=각 Pid 마커의 (uuid, owner_pid,
/// marker_mtime), `claude_pids`=살아있는 claude pid 집합. 가드①=owner_pid ∈ claude_pids(죽었거나
/// claude 아닌 pid 제외 - pid 재사용 방지), 가드②=같은 살아있는 pid를 여러 마커가 가리키면
/// marker_mtime **최신** uuid만 인정(/clear 훅 실패로 남은 스테일 마커 유령의 구조적 해소).
/// 반환=로스터에 유지할 uuid 집합.
pub fn live_idle_marker_uuids(
    markers: &[(String, u32, SystemTime)],
    claude_pids: &std::collections::HashSet<u32>,
) -> std::collections::HashSet<String> {
    // pid → (그 pid를 가리키는 마커 중 mtime 최신 uuid, 그 mtime). 동률 mtime은 먼저 본 것 유지.
    let mut best: std::collections::HashMap<u32, (&str, SystemTime)> =
        std::collections::HashMap::new();
    for (uuid, pid, mtime) in markers {
        if !claude_pids.contains(pid) {
            continue; // 가드①: 죽었거나 claude 아님.
        }
        match best.get(pid) {
            Some((_, cur)) if *cur >= *mtime => {} // 가드②: 이미 더 최신 마커가 있음.
            _ => {
                best.insert(*pid, (uuid.as_str(), *mtime));
            }
        }
    }
    best.values().map(|(u, _)| (*u).to_string()).collect()
}

/// projects_dir 하위(`<mangled-cwd>/<uuid>.jsonl`)에서 특정 uuid의 jsonl 경로를 찾는다(P8). 서브
/// 디렉토리마다 `<uuid>.jsonl` 존재만 stat으로 확인해 첫 매치를 반환한다(전체 파일 열거 안 함).
/// 살아남은 유휴 uuid(= 열린 세션 수, 소수)에 대해서만 호출된다. subdirs는 호출부가 projects_dir을
/// 한 번만 읽어 넘긴다(유휴 세션마다 read_dir 시스템 콜 반복을 피한다, gemini 리뷰).
fn find_session_jsonl(subdirs: &[PathBuf], uuid: &str) -> Option<PathBuf> {
    let file = format!("{uuid}.jsonl");
    for subpath in subdirs {
        let candidate = subpath.join(&file);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

/// 유휴-열림 claude 세션을 로스터에 추가로 되살린다(v2-45 P8, 순수 additive). 마커(.ctx) owner pid가
/// 살아있는 claude면 jsonl mtime 신선도 창(stale)과 무관하게 유지한다(240분 넘게 입력이 없어도 창이
/// 열려 있으면 로스터에 남는다). 순수 판정(가드①②)은 [`live_idle_marker_uuids`]가, IO(마커 열거·
/// jsonl 탐색)만 여기서 한다. `existing`에 이미 있는 uuid는 건너뛴다(신선도 창으로 이미 잡힘 = 기존
/// 우선). 마커가 Pid가 아닌 것(NoMarker/Unknown/Dead=tombstone)은 대상이 아니다 - 가드③(마커 없음)은
/// 기존 enumerate의 신선도 창 폴백이 처리하며 P8은 여기에 아무것도 더하지 않는다. codex는 마커가
/// 없어 비대상(설계: rollout session_meta pid 정찰 후 후속).
///
/// **잔여 위험(pid 재사용, 적대 리뷰 confirmed minor)**: 크래시로 Pid 마커가 남은 세션의 pid를,
/// 마커를 쓰지 않는 산 claude(headless·temp cwd·TUNA_AUTOARM 미설정)가 재사용하면 가드①(claude pid)이
/// 통과하고 가드②(그 pid를 가리키는 마커가 스테일 하나뿐)도 밀어내지 못해 죽은 세션이 유령으로
/// 되살아날 수 있다. 정상 /clear 경로는 SessionStart가 같은 pid에 더 최신 마커를 써 밀어내므로 안전하다.
/// 근본 차단은 "그 pid 프로세스 시작시각 > 마커 mtime이면 재사용"인 시작시각 가드(win CIM CreationDate·
/// unix ps lstart)이나, 케이스가 좁고(마커 없는 정확한 pid 재사용) 결과가 유휴 카드 1개라 후속 하드닝으로 남긴다.
pub fn enumerate_idle_marker_sessions(
    marker_dir: &Path,
    projects_dir: &Path,
    claude_pids: &std::collections::HashSet<u32>,
    existing: &std::collections::HashSet<String>,
    home: Option<&Path>,
) -> Vec<LiveSession> {
    // 1) 마커 열거: Pid 마커만 (uuid, owner_pid, marker_mtime)으로 수집.
    let mut markers: Vec<(String, u32, SystemTime)> = Vec::new();
    let Ok(entries) = std::fs::read_dir(marker_dir) else {
        return Vec::new();
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("ctx") {
            continue;
        }
        let Some(uuid) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        // 내용 파싱은 read_marker에 위임(Pid/Unknown/Dead/NoMarker 규약 공유). Pid만 P8 대상.
        let MarkerState::Pid(pid) = read_marker(marker_dir, uuid) else {
            continue;
        };
        let Ok(meta) = e.metadata() else { continue };
        let Ok(mtime) = meta.modified() else { continue };
        markers.push((uuid.to_string(), pid, mtime));
    }
    // 2) 가드①②로 유지할 uuid 판정.
    let keep = live_idle_marker_uuids(&markers, claude_pids);
    // 3) 기존에 없는 uuid만 jsonl에서 project를 뽑아 LiveSession 생성(내부/자동화/temp cwd 필터 존중).
    //    projects_dir 서브디렉토리 목록을 1회만 읽어 재사용한다(gemini 리뷰: 세션마다 read_dir 반복 회피).
    let subdirs: Vec<PathBuf> = std::fs::read_dir(projects_dir)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir())
                .collect()
        })
        .unwrap_or_default();
    let mut out: Vec<LiveSession> = Vec::new();
    for uuid in keep {
        if existing.contains(&uuid) {
            continue; // 신선도 창으로 이미 잡힘(기존 우선, 중복 방지).
        }
        let Some(path) = find_session_jsonl(&subdirs, &uuid) else {
            continue;
        };
        let (cwd, first_user) = crate::discover::scan_jsonl_head(&path, 60);
        // discover 열거와 같은 노이즈 필터(claude-mem observer·secall automation·temp 헤드리스) 존중.
        if cwd.as_deref().is_some_and(crate::discover::is_internal_cwd)
            || first_user
                .as_deref()
                .is_some_and(crate::discover::is_automation_first_message)
            || cwd.as_deref().is_some_and(is_temp_cwd)
        {
            continue;
        }
        let project = project_from_cwd_normalized(cwd.as_deref(), home);
        out.push(LiveSession {
            uuid,
            runner: "claude".to_string(),
            project,
            // claude ★ 신호는 human-ping 훅 경로(enumerate_claude_live와 동일). jsonl age는 무시(유휴라 오래됨).
            human_input_at: None,
            created_at: None, // codex 전용 grace 신호(claude idle은 게이트 비대상).
            active_at: None,  // claude 턴 신호는 turn-ping 훅 경로(이슈 #123).
        });
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid)); // HashSet 순회의 비결정성을 없애 보고 payload를 안정화.
    out
}

/// Windows 전용 러너 카운트 판별(순수부). tasklist CSV 이미지명만으로는 npm 설치 러너(node.exe로 뜸,
/// 이미지명이 claude.exe/codex.exe가 아님)를 못 잡아, 하드 매칭 실패를 "확정 0"으로 오판하면
/// apply_process_gate가 그 머신의 살아있는 세션을 매 주기 전부 드롭한다. unix는 argv 토큰 basename
/// 매칭(count_matching_lines)으로 이미 node 래퍼를 커버하지만, tasklist엔 argv 컬럼이 없어 이식 불가.
/// 이미지명 매칭이 1개 이상이면 그 값을 그대로 확정 신호로 쓴다(Some). 매칭이 0인데 node.exe 프로세스가
/// 있으면 "그 node가 claude/codex 래퍼인지" 판단 불가이므로 None(=게이트 스킵, 산 세션 보존, 보수적).
/// node.exe도 없으면 확정으로 0개다(Some(0), 기존 동작 유지).
pub fn windows_runner_gate_count(text: &str, name: &str) -> Option<usize> {
    let image_count = count_matching_lines(text, name, true);
    if image_count > 0 {
        return Some(image_count);
    }
    if count_matching_lines(text, "node", true) > 0 {
        None // node 래퍼 러너 가능성 → 판단 불가, 게이트 미적용.
    } else {
        Some(0) // node도 없음 → 확정 0.
    }
}

/// 프로세스 게이트: 해당 러너 프로세스가 확실히 0개면(count=Some(0)) 그 러너 세션을 전부 제외한다.
/// None(조회 실패)이나 1개 이상이면 그대로 둔다(파일 신선도 창이 상한).
pub fn apply_process_gate(
    sessions: Vec<LiveSession>,
    runner: &str,
    count: Option<usize>,
) -> Vec<LiveSession> {
    match count {
        Some(0) => sessions
            .into_iter()
            .filter(|s| s.runner != runner)
            .collect(),
        _ => sessions,
    }
}

// #138 B: 도메인별 서브모듈(순수 이동). claude·codex 스캔과 보고 조립을 분리한다. 항목들은 glob으로 이
// 모듈 스코프에 유지되어 프로세스/마커 게이트(이 파일에 남은 공유 오케스트레이션)와 테스트
// (`use super::*`)의 기존 경로가 그대로 해석된다.
mod claude;
mod codex;
mod report;

pub use claude::enumerate_claude_live;
pub use codex::{
    CodexInputCache, apply_codex_human_input_gate, default_codex_sessions_dir,
    enumerate_codex_sessions, load_codex_input_cache_from_disk, normalize_iso_to_db_datetime,
    parse_codex_last_user_input, parse_codex_meta_line, save_codex_input_cache_to_disk,
    system_time_to_db_datetime,
};
pub use report::to_report_json;
// codex.rs의 pub(super) 헬퍼(CODEX_TAIL_BYTES·message_is_human_input)는 원래도 이 파일 내부·테스트
// 전용이었다 - 외부 재공개 없이 tests 자손에게만 super::* 글롭 체인으로 보이게 하는 브릿지.
#[cfg(test)]
use codex::{CODEX_TAIL_BYTES, message_is_human_input};

#[cfg(test)]
mod tests;
