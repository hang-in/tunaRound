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

/// codex rollout jsonl의 session_meta 줄에서 (session_id, cwd, originator)를 뽑는다. 실패는 None.
/// parse_codex_meta_line 반환: (session_id, cwd, originator, created_at). created_at은 이슈 #88 grace 신호.
type CodexMeta = (String, Option<String>, Option<String>, Option<String>);

/// enumerate_codex_sessions 후보: (uuid, project, path, mtime, created_at).
type CodexCandidate = (String, Option<String>, PathBuf, SystemTime, Option<String>);

pub fn parse_codex_meta_line(line: &str) -> Option<CodexMeta> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("session_meta") {
        return None;
    }
    let p = v.get("payload")?;
    let id = p
        .get("session_id")
        .or_else(|| p.get("id"))
        .and_then(|x| x.as_str())?
        .to_string();
    let cwd = p.get("cwd").and_then(|c| c.as_str()).map(str::to_string);
    let originator = p
        .get("originator")
        .and_then(|o| o.as_str())
        .map(str::to_string);
    // 세션 생성시각(신규-미입력 세션 grace용, 이슈 #88). payload.timestamp(세션 시작) 우선, 없으면 라인
    // top-level timestamp. ISO → DB datetime(normalize는 offset을 스트립·UTC 유지, 롤아웃은 Z=UTC).
    let created_at = p
        .get("timestamp")
        .or_else(|| v.get("timestamp"))
        .and_then(|t| t.as_str())
        .and_then(normalize_iso_to_db_datetime);
    Some((id, cwd, originator, created_at))
}

/// 기본 codex 세션 디렉토리(`~/.codex/sessions`). HOME 미확장이면 None.
pub fn default_codex_sessions_dir() -> Option<PathBuf> {
    let expanded = crate::config::expand_home("~/.codex/sessions");
    if expanded.starts_with("~/") {
        None
    } else {
        Some(PathBuf::from(expanded))
    }
}

/// codex rollout tail 스캔 상한(256KB 역방향). 세션당 마지막 사람 입력 시각만 필요하므로 전체를 읽지 않는다.
const CODEX_TAIL_BYTES: usize = 256 * 1024;

/// 파일 끝에서 최대 max 바이트를 읽는다(역방향 tail). 앞쪽 경계에서 잘린 라인은 호출부가 JSON 파싱
/// 실패로 자연 스킵한다. 파일 없음·IO 실패는 None.
fn read_tail_bytes(path: &Path, max: usize) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let start = len.saturating_sub(max as u64);
    if start > 0 {
        f.seek(SeekFrom::Start(start)).ok()?;
    }
    let mut buf = Vec::new();
    f.read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// "YYYY-MM-DD HH:MM:SS"(19자, 공백 구분) 형태 검사(사전순=시간순 비교 안전 보장).
fn is_db_datetime_19(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 19
        && b.iter().enumerate().all(|(i, &c)| match i {
            4 | 7 => c == b'-',
            10 => c == b' ',
            13 | 16 => c == b':',
            _ => c.is_ascii_digit(),
        })
}

/// ISO8601 타임스탬프("2026-07-11T09:00:00.894Z")를 DB datetime 포맷("2026-07-11 09:00:00")으로
/// 정규화한다('T'→공백, 소수초·'Z'·offset 절단). §5-3 계약: 'T' > ' ' 사전순 왜곡을 없애 로스터·영속
/// 워터마크와 비교 가능하게 한다. 결과가 19자 DB 포맷이 아니면 None(오염 방어).
pub fn normalize_iso_to_db_datetime(ts: &str) -> Option<String> {
    // 날짜의 '-'와 타임존 offset '-'(-05:00)를 혼동하지 않게 먼저 날짜/시간을 분리한 뒤(공백 또는 'T'),
    // 시간 부분만 소수초·'Z'·offset('+'/'-')에서 절단한다(gemini 리뷰: 음수 offset도 안전 처리).
    let replaced = ts.trim().replacen('T', " ", 1);
    let mut parts = replaced.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    let core_time = time
        .split(['.', 'Z', '+', '-'])
        .next()
        .unwrap_or(time)
        .trim();
    let core = format!("{date} {core_time}");
    if is_db_datetime_19(&core) {
        Some(core)
    } else {
        None
    }
}

/// SystemTime을 UTC "YYYY-MM-DD HH:MM:SS" DB datetime으로 포맷한다(chrono 없이, 이슈 #88 게이트의
/// threshold 계산용). UNIX epoch 초 → Howard Hinnant civil-from-days. human_input_at·created_at이
/// normalize_iso_to_db_datetime(UTC 유지)로 만든 값과 같은 UTC·같은 포맷이라, DB datetime의
/// 사전순=시간순 성질로 문자열 비교만으로 신선도를 판정한다(store::a2a::age_secs=sqlite-gated 회피).
pub fn system_time_to_db_datetime(t: std::time::SystemTime) -> Option<String> {
    let secs = t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let (hh, mm, ss) = (tod / 3600, (tod % 3600) / 60, tod % 60);
    // civil_from_days(Hinnant): epoch days → (year, month, day).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y0 = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y0 + 1 } else { y0 };
    Some(format!("{y:04}-{m:02}-{d:02} {hh:02}:{mm:02}:{ss:02}"))
}

/// user_message가 사람 입력인지(= relay 기계 주입이 아닌지) 판정한다(§5-6 고정 계약). relay 주입은
/// [`build_inject_text`](crate::codex_relay::build_inject_text)가 정확히 [`RELAY_INJECT_PREFIX`]
/// (crate::codex_relay)로 시작하는 텍스트라, 선행 공백 없이 그 prefix로 시작하는 메시지만 기계로 본다.
/// trim은 하지 않는다(선행 공백이 붙은 "  브로커 task …"는 relay가 만들지 않는 형태이므로 사람 입력으로
/// 남긴다 - 사람 입력을 드롭하는 오분류를 최소화). 사람이 정확히 이 prefix로 문장을 시작하면 드롭되지만,
/// 이는 자연어 마커의 내재적 한계로 수용한다(대안=주입 이벤트 메타 마커, codex가 구분 필드를 안 실어 불가).
fn message_is_human_input(message: &str) -> bool {
    !message.starts_with(crate::codex_relay::RELAY_INJECT_PREFIX)
}

/// codex rollout tail에서 마지막 사람 입력(user_message) 시각을 뽑는다(v2-45 P5).
/// `type=="event_msg" && payload.type=="user_message"` 줄 중 relay 주입 prefix가 아닌 것의 top-level
/// timestamp 최대값을 DB datetime 포맷으로 정규화해 반환한다. 해당 입력이 없으면 None.
pub fn parse_codex_last_user_input(path: &Path, max_tail: usize) -> Option<String> {
    let tail = read_tail_bytes(path, max_tail)?;
    let text = String::from_utf8_lossy(&tail);
    let mut latest: Option<String> = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(payload) = v.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(|t| t.as_str()) != Some("user_message") {
            continue;
        }
        let msg = payload
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        if !message_is_human_input(msg) {
            continue; // relay 기계 주입 = 사람 입력 아님.
        }
        let Some(ts) = v.get("timestamp").and_then(|t| t.as_str()) else {
            continue;
        };
        let Some(norm) = normalize_iso_to_db_datetime(ts) else {
            continue;
        };
        match &latest {
            Some(cur) if norm.as_str() <= cur.as_str() => {}
            _ => latest = Some(norm),
        }
    }
    latest
}

/// codex rollout tail 스캔 캐시(uuid → (마지막 관측 mtime, human_input_at)). mtime 무변경이면 재스캔을
/// 건너뛴다(전 주기 결과 재사용). presence 스캐너 데몬이 주기 간 소유한다.
pub type CodexInputCache = std::collections::HashMap<String, (SystemTime, Option<String>)>;

/// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`을 재귀 스캔해 stale 이내 mtime의 라이브 TUI 세션을
/// 낸다. originator가 codex-tui가 아닌 것(exec 등 헤드리스)은 제외(로스터=열린 TUI 세션 계약).
/// `input_cache`가 Some이면 uuid별 최신 rollout tail에서 사람 입력 시각(human_input_at)을 mtime 캐시와
/// 함께 스캔한다(P5). None이면 스캔을 생략한다(relay 등 uuid만 필요한 호출자 = 무비용).
pub fn enumerate_codex_sessions(
    sessions_dir: &Path,
    now: SystemTime,
    stale: Duration,
    home: Option<&Path>,
    mut input_cache: Option<&mut CodexInputCache>,
) -> Vec<LiveSession> {
    // 후보 수집: (uuid, project, path, mtime, created_at). 같은 uuid의 rollout이 복수면 최신(mtime 최대)만.
    let mut cands: Vec<CodexCandidate> = Vec::new();
    let mut stack = vec![sessions_dir.to_path_buf()];
    // 디렉토리 깊이는 YYYY/MM/DD 고정이지만 방어적으로 상한을 둔다(심볼릭 링크 순환 등).
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 10_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with("rollout-")
                || path.extension().and_then(|x| x.to_str()) != Some("jsonl")
            {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if crate::discover::age_secs_since(mtime, now) as u64 > stale.as_secs() {
                continue;
            }
            let Some(first) = read_first_line(&path) else {
                continue;
            };
            let Some((uuid, cwd, originator, created_at)) = parse_codex_meta_line(&first) else {
                continue;
            };
            if originator.as_deref() != Some("codex-tui") {
                continue; // exec/워커 rollout은 로스터 대상 아님.
            }
            if cwd.as_deref().is_some_and(is_temp_cwd) {
                continue;
            }
            let project = project_from_cwd_normalized(cwd.as_deref(), home);
            cands.push((uuid, project, path, mtime, created_at));
        }
    }
    // uuid별 최신 rollout만 남긴다(uuid asc, mtime desc로 정렬 후 dedup_by가 앞=최신을 유지).
    cands.sort_by(|a, b| a.0.cmp(&b.0).then(b.3.cmp(&a.3)));
    cands.dedup_by(|a, b| a.0 == b.0);
    let mut out: Vec<LiveSession> = Vec::with_capacity(cands.len());
    for (uuid, project, path, mtime, created_at) in cands {
        let human_input_at = match input_cache.as_mut() {
            None => None, // 스캔 불필요 호출(relay): uuid만 낸다.
            Some(cache) => match cache.get(&uuid) {
                // mtime 무변경 = rollout에 새 입력 없음 → 전 주기 결과 재사용(재스캔 스킵).
                Some((cached_mtime, cached)) if *cached_mtime == mtime => cached.clone(),
                _ => {
                    let scanned = parse_codex_last_user_input(&path, CODEX_TAIL_BYTES);
                    // human_input_at은 단조 증가(사람 입력 시각은 뒤로 안 감). 장기 자율작업으로 마지막
                    // 사람 입력 이후 출력이 256KB tail 밖으로 밀리면 재스캔이 None이 되는데(#88 적대 검증
                    // minor), 그때 이전 관측값을 유지해 살아있는 세션의 조기 드롭을 막는다. 유령은 relay
                    // 주입이 human_input에 면역이라 값이 얼어붙어 있어 이 유지가 수명을 늘리지 않는다
                    // (여전히 마지막 실입력 + window에서 드롭). 둘 다 Some이면 사전순 최댓값(단조 보장).
                    let prev = cache.get(&uuid).and_then(|(_, v)| v.clone());
                    let hi = match (scanned, prev) {
                        (Some(s), Some(p)) => Some(if s >= p { s } else { p }),
                        (s, p) => s.or(p),
                    };
                    cache.insert(uuid.clone(), (mtime, hi.clone()));
                    hi
                }
            },
        };
        out.push(LiveSession {
            uuid,
            runner: "codex".to_string(),
            project,
            human_input_at,
            created_at,
        });
    }
    // 이번 주기에 사라진 세션의 캐시 항목 정리(무한 성장 방지).
    if let Some(cache) = input_cache.as_mut() {
        let present: std::collections::HashSet<&str> =
            out.iter().map(|s| s.uuid.as_str()).collect();
        cache.retain(|k, _| present.contains(k.as_str()));
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    out
}

fn read_first_line(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    BufReader::new(f).lines().next()?.ok()
}

/// claude 세션 스캔: discover 열거를 재사용하고 presence 규약(home 정규화·temp 제외)만 얹는다.
pub fn enumerate_claude_live(
    projects_dir: &Path,
    now: SystemTime,
    stale: Duration,
    home: Option<&Path>,
) -> Vec<LiveSession> {
    crate::discover::enumerate_claude_sessions(projects_dir, now, stale)
        .into_iter()
        .filter(|s| !s.cwd.as_deref().is_some_and(is_temp_cwd))
        .map(|s| LiveSession {
            uuid: s.uuid,
            runner: "claude".to_string(),
            project: project_from_cwd_normalized(s.cwd.as_deref(), home).or(s.project),
            // claude ★ 신호는 UserPromptSubmit 훅(→ human-ping) 경로라 스캐너는 보고하지 않는다.
            human_input_at: None,
            created_at: None, // codex 전용 grace 신호(claude는 마커 경로라 게이트 비대상).
        })
        .collect()
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
pub fn read_marker(dir: &Path, uuid: &str) -> MarkerState {
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

/// codex 세션 사람활동 신선도 게이트(순수부, 이슈 #88). codex는 마커·PID가 없어 rollout mtime만으론 개별
/// thread 생존을 못 가른다(relay가 죽은 thread를 resume하며 mtime을 갱신 → stale 창 무한 연장). human_input_at은
/// relay 주입(RELAY_INJECT_PREFIX)에 면역이라 유령에선 얼어붙고, created_at은 신규-미입력 세션의 grace 신호다.
/// codex 세션은 **사람입력 또는 생성이 min_active_db(= now-window, DB datetime) 이후**면 유지하고, 둘 다 그보다
/// 오래됐거나 없으면 드롭한다(유령 배제). claude·워커·infra 등 비-codex는 무조건 통과(마커·별도 신호 경로).
/// DB datetime은 고정폭이라 사전순=시간순 → 문자열 비교만으로 판정한다(store::a2a::age_secs=sqlite-gated 회피).
/// upstream 필터라, 드롭된 uuid는 sync_presence의 stale 제거가 로스터·A2A·영속행까지 자동 GC한다.
///
/// **왜 시간창인가(2026-07-12 실측, codex-cli 0.144.1, 재론 금지)**: codex per-thread 생존을 읽을 깨끗한
/// 신호가 도달 범위에 없다. (1) session_meta에 PID·프로세스 식별자가 없다(키=session_id·cwd·originator·
/// source·cli_version·context_window뿐) → claude식 PID-마커 이식 불가. (2) app-server `thread/list` status와
/// `thread/loaded/list`는 **그 인스턴스가 메모리에 로드한 것만** 반영(전역 아님)이고, 죽은 thread도
/// `thread/resume`이 성공하며 `idle`/loaded가 된다 → relay 주입이 유령을 loaded로 만들어 오히려 악화. 사람의
/// codex TUI는 별도(VS Code 자체) app-server에 살아 mesh app-server가 그 생존을 못 본다. 즉 이 시간창은
/// **원리적 최선**이며 다음 두 성질을 준다: relay 자기유지 루프 차단(human_input 얼음) + 유령 수명 상한
/// (stale_mins→window). **수용된 잔여**: 방금 쓰다 닫은 세션은 human_input이 최근이라 살아있는 idle 세션과
/// 시간만으론 구분 불가 → window 동안 로스터에 잔존(codex_gate_fresh_churn_ghost_lingers 테스트가 명시).
/// 부수 FP: window 넘게 입력 없는 살아있는 codex(장기작업 관전)는 드롭되나 다음 입력 시 ≤interval 자기치유.
pub fn apply_codex_human_input_gate(
    sessions: Vec<LiveSession>,
    min_active_db: &str,
) -> Vec<LiveSession> {
    sessions
        .into_iter()
        .filter(|s| {
            if s.runner != "codex" {
                return true; // codex 전용 게이트.
            }
            let fresh = |ts: &Option<String>| ts.as_deref().is_some_and(|t| t >= min_active_db);
            fresh(&s.human_input_at) || fresh(&s.created_at)
        })
        .collect()
}

/// report_presence의 sessions JSON 배열로 직렬화한다. display_name = {machine}-{runner}-{project|?}.
pub fn to_report_json(machine: &str, sessions: &[LiveSession]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            let display = format!(
                "{machine}-{}-{}",
                s.runner,
                s.project.as_deref().unwrap_or("unknown")
            );
            serde_json::json!({
                "uuid": s.uuid,
                "runner": s.runner,
                "project": s.project,
                "display_name": display,
                "human_input_at": s.human_input_at,
            })
        })
        .collect();
    serde_json::Value::Array(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_meta_line_parses_id_cwd_originator() {
        let line = r#"{"timestamp":"2026-07-11T09:00:05Z","type":"session_meta","payload":{"session_id":"abc-123","id":"abc-123","timestamp":"2026-07-11T09:00:00Z","cwd":"C:\\Users\\me\\proj","originator":"codex-tui"}}"#;
        let (id, cwd, orig, created_at) = parse_codex_meta_line(line).unwrap();
        assert_eq!(id, "abc-123");
        assert_eq!(cwd.as_deref(), Some("C:\\Users\\me\\proj"));
        assert_eq!(orig.as_deref(), Some("codex-tui"));
        // created_at = payload.timestamp(세션 시작) 우선, ISO→DB. 이슈 #88 grace 신호.
        assert_eq!(created_at.as_deref(), Some("2026-07-11 09:00:00"));
        // payload.timestamp 없으면 top-level로 폴백.
        let no_payload_ts = r#"{"timestamp":"2026-07-11T10:00:00Z","type":"session_meta","payload":{"session_id":"x","originator":"codex-tui"}}"#;
        assert_eq!(
            parse_codex_meta_line(no_payload_ts).unwrap().3.as_deref(),
            Some("2026-07-11 10:00:00")
        );
        // session_meta가 아닌 줄은 None.
        assert!(parse_codex_meta_line(r#"{"type":"turn","payload":{}}"#).is_none());
    }

    #[test]
    fn project_normalizes_home_and_falls_back_to_basename() {
        let home = Path::new("C:\\Users\\me");
        assert_eq!(
            project_from_cwd_normalized(Some("C:\\Users\\me"), Some(home)),
            Some("home".to_string())
        );
        // 대소문자·구분자 차이도 home으로 인식.
        assert_eq!(
            project_from_cwd_normalized(Some("c:/users/me/"), Some(home)),
            Some("home".to_string())
        );
        assert_eq!(
            project_from_cwd_normalized(Some("C:\\Users\\me\\tunaRound"), Some(home)),
            Some("tunaRound".to_string())
        );
        assert_eq!(project_from_cwd_normalized(None, Some(home)), None);
    }

    #[test]
    fn is_temp_cwd_matches_macos_conventional_tmp_prefixes() {
        // std::env::temp_dir()은 /var/folders/...만 잡으므로, 이 케이스들은 그 비교로는 못 잡고
        // MAC_TEMP_PREFIXES 폴백이 잡아야 한다. 그 폴백은 macOS 전용이라(coderabbit) 양성 단언도
        // macOS에서만 성립한다(Linux/Windows에선 /tmp를 temp로 보지 않아 오분류가 없다).
        #[cfg(target_os = "macos")]
        {
            assert!(is_temp_cwd("/tmp/work-xyz"));
            assert!(is_temp_cwd("/tmp"));
            assert!(is_temp_cwd("/private/tmp/work-xyz"));
            assert!(is_temp_cwd("/private/var/folders/ab/xyz/T/foo"));
            // 대소문자·구분자 정규화(기존 규약 유지).
            assert!(is_temp_cwd("/TMP/Work-Xyz"));
        }
        // 접두만 겹치는 비-temp 경로는 어느 플랫폼에서도 temp가 아니다.
        assert!(!is_temp_cwd("/tmpfoo/bar"));
        assert!(!is_temp_cwd("/home/user/project"));
    }

    #[test]
    fn count_matching_lines_covers_paths_wrappers_and_csv() {
        // unix(`ps -o pid=,args=`): pid 토큰 뒤 argv에서 단독 실행 / 전체 경로 / node 인터프리터
        // 래퍼를 잡고, 뒤쪽 인자 매칭(3토큰 밖)은 제외.
        let unix = "  11 claude --resume abc\n  22 /usr/local/bin/claude\n  33 node /home/u/.npm/bin/claude --flag\n  44 ps -ax\n  55 tunaround poll --tags a b runner=claude\n";
        assert_eq!(count_matching_lines(unix, "claude", false), 3);
        // node 래퍼가 2번째 토큰이 아니라도 argv 3토큰 안이면 잡힌다.
        assert_eq!(
            count_matching_lines("77 env FOO=1 /opt/claude serve\n", "claude", false),
            1
        );
        // win CSV: 이미지명 필드만 본다.
        let win = "\"claude.exe\",\"123\",\"Console\"\n\"notepad.exe\",\"9\",\"Console\"\n\"x.exe\",\"1\",\"claude\"\n";
        assert_eq!(count_matching_lines(win, "claude", true), 1);
        assert_eq!(count_matching_lines(win, "codex", true), 0);
    }

    #[test]
    fn snapshot_sanity_rejects_error_only_output() {
        // tasklist 부하 에러 출력(exit 0)은 pid가 안 나와 스냅샷 실패로 간주돼야 한다(깜빡임 방지).
        let err_text = "ERROR: This operation returned because the timeout period expired.\n";
        assert!(parse_pids(err_text, true).is_empty());
        assert!(parse_pids(err_text, false).is_empty());
    }

    #[test]
    fn parse_pids_extracts_from_both_formats() {
        let unix = "  11 claude\n  22 /bin/ps\nbadline\n";
        let pids = parse_pids(unix, false);
        assert!(pids.contains(&11) && pids.contains(&22) && pids.len() == 2);
        let win = "\"claude.exe\",\"123\",\"Console\"\n\"x.exe\",\"9\",\"c\"\n";
        let pids = parse_pids(win, true);
        assert!(pids.contains(&123) && pids.contains(&9) && pids.len() == 2);
    }

    #[test]
    fn marker_liveness_drops_only_dead_pid_sessions() {
        use std::collections::HashSet;
        let alive: HashSet<u32> = [100u32, 200].into_iter().collect();
        // PID 기록 + 스냅샷에 있음 → 유지 / 없음 → 유령 제거.
        assert!(is_session_live(&MarkerState::Pid(100), &alive));
        assert!(!is_session_live(&MarkerState::Pid(999), &alive));
        // 마커 없음·PID 미상 → 보수적 유지(신선도 창 폴백).
        assert!(is_session_live(&MarkerState::NoMarker, &alive));
        assert!(is_session_live(&MarkerState::Unknown, &alive));
    }

    #[test]
    fn read_marker_parses_pid_empty_and_missing() {
        let dir = std::env::temp_dir().join(format!("tuna-marker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("s1.ctx"), "4242\n").unwrap();
        std::fs::write(dir.join("s2.ctx"), "").unwrap();
        assert_eq!(read_marker(&dir, "s1"), MarkerState::Pid(4242));
        assert_eq!(read_marker(&dir, "s2"), MarkerState::Unknown);
        assert_eq!(read_marker(&dir, "none"), MarkerState::NoMarker);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn process_gate_drops_runner_only_when_zero() {
        let s = |r: &str| LiveSession {
            uuid: r.to_string(),
            runner: r.to_string(),
            project: None,
            human_input_at: None,
            created_at: None,
        };
        let all = vec![s("claude"), s("codex")];
        // 확실한 0 → 해당 러너만 제거.
        let gated = apply_process_gate(all.clone(), "codex", Some(0));
        assert_eq!(gated.len(), 1);
        assert_eq!(gated[0].runner, "claude");
        // 조회 실패(None)·1개 이상 → 그대로.
        assert_eq!(apply_process_gate(all.clone(), "codex", None).len(), 2);
        assert_eq!(apply_process_gate(all, "codex", Some(3)).len(), 2);
    }

    #[test]
    fn windows_gate_count_prefers_confirmed_image_match() {
        // 이미지명 매칭>0 → Some(그 카운트)(node 존재 여부 무관, 확정 신호).
        let text = "\"claude.exe\",\"123\",\"Console\"\n\"node.exe\",\"456\",\"Console\"\n";
        assert_eq!(windows_runner_gate_count(text, "claude"), Some(1));
    }

    #[test]
    fn windows_gate_count_none_when_only_node_wrapper_present() {
        // 이미지 매칭 0 && node.exe 있음 → 판단 불가(None, 게이트 스킵 = 산 세션 보존).
        let text = "\"node.exe\",\"456\",\"Console\"\n\"notepad.exe\",\"9\",\"Console\"\n";
        assert_eq!(windows_runner_gate_count(text, "claude"), None);
    }

    #[test]
    fn windows_gate_count_zero_when_no_image_and_no_node() {
        // 이미지 매칭 0 && node.exe도 없음 → 확정 0(기존 동작 유지).
        let text = "\"notepad.exe\",\"9\",\"Console\"\n";
        assert_eq!(windows_runner_gate_count(text, "claude"), Some(0));
    }

    fn codex_session(uuid: &str, hi: Option<&str>, ca: Option<&str>) -> LiveSession {
        LiveSession {
            uuid: uuid.into(),
            runner: "codex".into(),
            project: None,
            human_input_at: hi.map(str::to_string),
            created_at: ca.map(str::to_string),
        }
    }

    #[test]
    fn codex_human_input_gate_keeps_fresh_drops_ghost() {
        // 이슈 #88: threshold(= now-window) = 2026-07-11 09:00:00. codex는 human_input 또는 created가
        // 이 이후면 유지, 둘 다 stale/None이면 드롭. claude는 무조건 통과.
        let threshold = "2026-07-11 09:00:00";
        let sessions = vec![
            // 유령: 사람입력·생성 둘 다 threshold 이전 → 드롭(핵심 케이스).
            codex_session(
                "ghost",
                Some("2026-07-11 08:00:00"),
                Some("2026-07-11 07:00:00"),
            ),
            // 활성: 최근 사람입력 → 유지.
            codex_session(
                "active",
                Some("2026-07-11 09:30:00"),
                Some("2026-07-11 06:00:00"),
            ),
            // 신규-미입력: 사람입력 None이나 최근 생성(grace) → 유지.
            codex_session("new", None, Some("2026-07-11 09:10:00")),
            // 신호 없음(둘 다 None) → 드롭.
            codex_session("nosignal", None, None),
            // claude는 게이트 비대상 → human/created None이어도 통과.
            LiveSession {
                uuid: "claude-s".into(),
                runner: "claude".into(),
                project: None,
                human_input_at: None,
                created_at: None,
            },
        ];
        let kept: Vec<String> = apply_codex_human_input_gate(sessions, threshold)
            .into_iter()
            .map(|s| s.uuid)
            .collect();
        assert_eq!(kept, vec!["active", "new", "claude-s"]);
    }

    #[test]
    fn codex_human_input_gate_boundary_is_inclusive() {
        let threshold = "2026-07-11 09:00:00";
        // 정확히 threshold(>= 비교) = 유지.
        assert_eq!(
            apply_codex_human_input_gate(
                vec![codex_session("b", Some(threshold), None)],
                threshold
            )
            .len(),
            1
        );
        // threshold보다 1초 이전(created도 없음) = 드롭.
        assert_eq!(
            apply_codex_human_input_gate(
                vec![codex_session("b", Some("2026-07-11 08:59:59"), None)],
                threshold
            )
            .len(),
            0
        );
    }

    #[test]
    fn codex_gate_drops_ghost_that_process_gate_kept_and_is_snapshot_independent() {
        // apply_process_gate는 codex 프로세스≥1이면 유령도 통과(all-or-nothing). human_input 게이트가 그
        // 뒤에서 유령을 드롭하는 협업을 못박는다. 게이트는 프로세스 count를 안 받으므로 스냅샷 실패 주기에도 동작.
        let threshold = "2026-07-11 09:00:00";
        let ghost = codex_session(
            "ghost",
            Some("2026-07-11 08:00:00"),
            Some("2026-07-11 07:00:00"),
        );
        let fresh = codex_session("fresh", Some("2026-07-11 09:30:00"), None);
        let after_proc = apply_process_gate(vec![ghost, fresh], "codex", Some(1));
        assert_eq!(
            after_proc.len(),
            2,
            "process_gate는 count>=1이면 유령도 유지"
        );
        let after_human: Vec<String> = apply_codex_human_input_gate(after_proc, threshold)
            .into_iter()
            .map(|s| s.uuid)
            .collect();
        assert_eq!(after_human, vec!["fresh"]);
    }

    #[test]
    fn codex_gate_fresh_churn_ghost_lingers_documented_residual() {
        // 이슈 #88의 원리적 잔여(재론 금지): 방금 쓰다 닫은 세션(=fresh-churn 유령)은 human_input이 최근이라
        // 살아있는 idle 세션과 시간만으론 구분 불가 → window 동안 로스터에 잔존한다. 재현 데이터(2026-07-12):
        // 유령 019f5547과 라이브 019f554b가 ~4분 간격 생성. 유령도 닫히기 직전 사람입력이 있었으므로 그 시각이
        // window 이내면 게이트를 통과한다. 이 테스트는 "게이트가 #88을 완전 제거한다"는 오해를 막는 명세다.
        let threshold = "2026-07-11 09:00:00"; // now-window.
        // 유령: 닫히기 직전(threshold 이후) 사람입력 → 게이트 통과(=수용된 잔여, 드롭 안 됨).
        let fresh_ghost = codex_session("fresh-ghost", Some("2026-07-11 09:20:00"), None);
        // 라이브: 더 최근 입력 → 당연히 통과. 둘 다 남아 dispatcher/사용자가 유령을 고를 수 있다.
        let live = codex_session("live", Some("2026-07-11 09:40:00"), None);
        let kept: Vec<String> = apply_codex_human_input_gate(vec![fresh_ghost, live], threshold)
            .into_iter()
            .map(|s| s.uuid)
            .collect();
        assert_eq!(
            kept,
            vec!["fresh-ghost", "live"],
            "fresh-churn 유령은 window 동안 잔존(원리적 한계): 게이트는 유령 수명 상한과 relay 자기유지 차단만 보장"
        );
    }

    #[test]
    fn system_time_to_db_datetime_formats_utc_civil() {
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(
            system_time_to_db_datetime(UNIX_EPOCH).as_deref(),
            Some("1970-01-01 00:00:00")
        );
        // +1일 +1시간 +1분 +1초.
        assert_eq!(
            system_time_to_db_datetime(UNIX_EPOCH + Duration::from_secs(86_400 + 3661)).as_deref(),
            Some("1970-01-02 01:01:01")
        );
        // 알려진 epoch 1700000000 = 2023-11-14 22:13:20 UTC(월/윤년 경계 civil 정확성).
        assert_eq!(
            system_time_to_db_datetime(UNIX_EPOCH + Duration::from_secs(1_700_000_000)).as_deref(),
            Some("2023-11-14 22:13:20")
        );
    }

    #[test]
    fn codex_enumerate_scans_tree_and_filters_non_tui() {
        let dir = std::env::temp_dir().join(format!("tuna-prescan-{}", std::process::id()));
        let day = dir.join("2026").join("07").join("11");
        std::fs::create_dir_all(&day).unwrap();
        let mk = |name: &str, body: &str| std::fs::write(day.join(name), body).unwrap();
        mk(
            "rollout-2026-07-11T01-aaa.jsonl",
            r#"{"type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#,
        );
        mk(
            "rollout-2026-07-11T02-bbb.jsonl",
            r#"{"type":"session_meta","payload":{"session_id":"exec-1","cwd":"/u/x/projA","originator":"codex exec"}}"#,
        );
        mk(
            "not-a-rollout.jsonl",
            r#"{"type":"session_meta","payload":{"session_id":"zzz","originator":"codex-tui"}}"#,
        );
        let found = enumerate_codex_sessions(
            &dir,
            SystemTime::now(),
            Duration::from_secs(3600),
            None,
            None,
        );
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 1, "TUI 세션만: {found:?}");
        assert_eq!(found[0].uuid, "tui-1");
        assert_eq!(found[0].project.as_deref(), Some("projA"));
        assert_eq!(
            found[0].human_input_at, None,
            "input_cache 없으면 신호 스캔 생략"
        );
    }

    #[test]
    fn report_json_shape_and_display_name() {
        let sessions = vec![LiveSession {
            uuid: "u1".into(),
            runner: "claude".into(),
            project: Some("tunaRound".into()),
            human_input_at: Some("2026-07-11 09:00:00".into()),
            created_at: None,
        }];
        let v = to_report_json("win", &sessions);
        assert_eq!(v[0]["uuid"], "u1");
        assert_eq!(v[0]["display_name"], "win-claude-tunaRound");
        assert_eq!(v[0]["human_input_at"], "2026-07-11 09:00:00");
    }

    // --- v2-45 P5: codex 입력 신호 tail 스캔 ---

    #[test]
    fn normalize_iso_to_db_datetime_handles_t_frac_z_offset() {
        assert_eq!(
            normalize_iso_to_db_datetime("2026-07-11T09:00:00.894Z").as_deref(),
            Some("2026-07-11 09:00:00")
        );
        assert_eq!(
            normalize_iso_to_db_datetime("2026-07-11T09:00:00Z").as_deref(),
            Some("2026-07-11 09:00:00")
        );
        assert_eq!(
            normalize_iso_to_db_datetime("2026-07-11T09:00:00+09:00").as_deref(),
            Some("2026-07-11 09:00:00")
        );
        // 음수 offset도 날짜의 '-'와 혼동 없이 처리(gemini 리뷰).
        assert_eq!(
            normalize_iso_to_db_datetime("2026-07-11T09:00:00-05:00").as_deref(),
            Some("2026-07-11 09:00:00")
        );
        // 이미 DB 포맷이면 그대로.
        assert_eq!(
            normalize_iso_to_db_datetime("2026-07-11 09:00:00").as_deref(),
            Some("2026-07-11 09:00:00")
        );
        // 형태 불량은 None(오염 방어).
        assert_eq!(normalize_iso_to_db_datetime("어제"), None);
        assert_eq!(normalize_iso_to_db_datetime("2026-07-11T09:00"), None);
    }

    #[test]
    fn message_is_human_input_excludes_relay_prefix() {
        assert!(message_is_human_input("현재 WSL 설정 확인 바람"));
        // relay 주입(build_inject_text prefix)은 사람 입력 아님(§5-6). 실제 출력은 선행 공백이 없다.
        assert!(!message_is_human_input(
            &crate::codex_relay::build_inject_text("t1", "요청")
        ));
        assert!(!message_is_human_input("브로커 task abc 가 배달됐다"));
        // 선행 공백이 붙은 형태는 relay가 만들지 않으므로 사람 입력으로 남긴다(trim 안 함 = 오분류 좁힘).
        assert!(message_is_human_input("  브로커 task 이거 뭐야"));
    }

    #[test]
    fn parse_codex_last_user_input_takes_latest_human_message() {
        let dir = std::env::temp_dir().join(format!("tuna-codex-input-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        let body = concat!(
            r#"{"timestamp":"2026-07-11T09:00:00.100Z","type":"session_meta","payload":{"session_id":"s","originator":"codex-tui"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-11T09:01:00.200Z","type":"event_msg","payload":{"type":"user_message","message":"첫 질문"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-11T09:02:00.300Z","type":"event_msg","payload":{"type":"agent_message","message":"답변"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-11T09:03:00.400Z","type":"event_msg","payload":{"type":"user_message","message":"브로커 task xyz 가 배달됐다(이미 claim됨)"}}"#,
            "\n",
            r#"{"timestamp":"2026-07-11T09:04:00.500Z","type":"event_msg","payload":{"type":"user_message","message":"둘째 질문"}}"#,
            "\n",
        );
        std::fs::write(&path, body).unwrap();
        let got = parse_codex_last_user_input(&path, CODEX_TAIL_BYTES);
        std::fs::remove_dir_all(&dir).ok();
        // 마지막 사람 user_message(09:04)를 정규화해 반환. relay 주입(09:03)·agent_message는 제외.
        assert_eq!(got.as_deref(), Some("2026-07-11 09:04:00"));
    }

    #[test]
    fn parse_codex_last_user_input_none_when_only_relay() {
        let dir =
            std::env::temp_dir().join(format!("tuna-codex-relay-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("rollout.jsonl");
        let body = concat!(
            r#"{"timestamp":"2026-07-11T09:03:00Z","type":"event_msg","payload":{"type":"user_message","message":"브로커 task xyz 가 배달됐다"}}"#,
            "\n",
        );
        std::fs::write(&path, body).unwrap();
        let got = parse_codex_last_user_input(&path, CODEX_TAIL_BYTES);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(got, None, "relay 주입만 있으면 사람 입력 없음");
    }

    #[test]
    fn enumerate_with_cache_scans_input_and_skips_on_unchanged_mtime() {
        let dir = std::env::temp_dir().join(format!("tuna-codex-cache-{}", std::process::id()));
        let day = dir.join("2026").join("07").join("11");
        std::fs::create_dir_all(&day).unwrap();
        std::fs::write(
            day.join("rollout-2026-07-11T01-aaa.jsonl"),
            concat!(
                r#"{"timestamp":"2026-07-11T09:00:00Z","type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#, "\n",
                r#"{"timestamp":"2026-07-11T09:05:00.123Z","type":"event_msg","payload":{"type":"user_message","message":"사람 입력"}}"#, "\n",
            ),
        ).unwrap();
        let mut cache = CodexInputCache::new();
        let found = enumerate_codex_sessions(
            &dir,
            SystemTime::now(),
            Duration::from_secs(3600),
            None,
            Some(&mut cache),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].human_input_at.as_deref(),
            Some("2026-07-11 09:05:00"),
            "tail 스캔이 사람 입력 시각 추출"
        );
        // 캐시에 mtime+값 저장 → 같은 파일 재스캔 시 재사용(mtime 무변경).
        assert!(cache.contains_key("tui-1"));
        let found2 = enumerate_codex_sessions(
            &dir,
            SystemTime::now(),
            Duration::from_secs(3600),
            None,
            Some(&mut cache),
        );
        assert_eq!(
            found2[0].human_input_at.as_deref(),
            Some("2026-07-11 09:05:00"),
            "캐시 재사용도 동일 값"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn enumerate_cache_preserves_human_input_when_scrolled_out_of_tail() {
        // 이슈 #88 적대 검증 minor: 장기 자율작업의 마지막 사람입력이 256KB tail 밖으로 밀려 재스캔이
        // None이어도 캐시의 이전 관측값을 유지한다(살아있는 세션의 조기 드롭 방지, human_input 단조성).
        // 캐시 mtime을 EPOCH로 심어 이번 주기 파일 mtime과 무조건 달라 rescan 경로를 강제한다.
        let dir = std::env::temp_dir().join(format!("tuna-codex-persist-{}", std::process::id()));
        let day = dir.join("2026").join("07").join("11");
        std::fs::create_dir_all(&day).unwrap();
        // 사람 입력(user_message) 없는 rollout → 재스캔 시 parse_codex_last_user_input=None.
        std::fs::write(
            day.join("rollout-2026-07-11T01-aaa.jsonl"),
            concat!(
                r#"{"timestamp":"2026-07-11T09:00:00Z","type":"session_meta","payload":{"session_id":"tui-1","cwd":"/u/x/projA","originator":"codex-tui"}}"#, "\n",
                r#"{"timestamp":"2026-07-11T09:30:00Z","type":"event_msg","payload":{"type":"agent_message","message":"긴 출력"}}"#, "\n",
            ),
        )
        .unwrap();
        let mut cache = CodexInputCache::new();
        // 이전 주기에 관측한 사람입력을 심는다(mtime=EPOCH ≠ 파일 mtime → rescan 발생).
        cache.insert(
            "tui-1".to_string(),
            (
                SystemTime::UNIX_EPOCH,
                Some("2026-07-11 09:05:00".to_string()),
            ),
        );
        let found = enumerate_codex_sessions(
            &dir,
            SystemTime::now(),
            Duration::from_secs(3600),
            None,
            Some(&mut cache),
        );
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].human_input_at.as_deref(),
            Some("2026-07-11 09:05:00"),
            "재스캔이 None이어도 캐시의 이전 human_input_at 유지(단조)"
        );
    }

    // --- v2-45 P8: 유휴-열림 세션 로스터 유지 ---

    #[test]
    fn runner_pids_extracts_only_named_runner_pids() {
        // unix: pid 토큰 뒤 argv basename이 claude인 행만(경로·node 래퍼 커버), 다른 러너/도구는 제외.
        let unix = concat!(
            "  11 claude --resume abc\n",
            "  22 /usr/local/bin/claude\n",
            "  33 node /home/u/.npm/bin/claude --flag\n",
            "  44 codex app-server\n",
            "  55 tunaround presence-scan\n",
            "  66 ps -ax\n",
        );
        use std::collections::HashSet;
        let pids = runner_pids(unix, "claude", false);
        assert_eq!(
            pids,
            [11u32, 22, 33].into_iter().collect::<HashSet<_>>(),
            "claude 프로세스 pid만: {pids:?}"
        );
        assert_eq!(
            runner_pids(unix, "codex", false),
            [44u32].into_iter().collect::<HashSet<_>>()
        );
        // win CSV: 이미지명 필드가 claude(.exe)인 행의 2번째 필드 pid만. 뒤 필드의 우연 매칭은 무시.
        let win = concat!(
            "\"claude.exe\",\"123\",\"Console\"\n",
            "\"node.exe\",\"9\",\"Console\"\n",
            "\"x.exe\",\"1\",\"claude\"\n",
            "\"CLAUDE.EXE\",\"456\",\"Console\"\n",
        );
        let pids = runner_pids(win, "claude", true);
        assert_eq!(
            pids,
            [123u32, 456].into_iter().collect::<HashSet<_>>(),
            "이미지명=claude.exe 행만(대소문자 무관): {pids:?}"
        );
    }

    #[test]
    fn live_idle_guard1_excludes_dead_or_nonclaude_pids() {
        use std::collections::HashSet;
        let t = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(n);
        let claude_pids: HashSet<u32> = [100u32, 200].into_iter().collect();
        let markers = vec![
            ("live".to_string(), 100u32, t(10)), // 살아있는 claude pid → 유지.
            ("dead".to_string(), 999u32, t(20)), // 스냅샷에 없는 pid(죽음/비claude) → 제외(가드①).
        ];
        let keep = live_idle_marker_uuids(&markers, &claude_pids);
        assert_eq!(
            keep,
            ["live".to_string()].into_iter().collect::<HashSet<_>>(),
            "가드①: 산 claude pid만: {keep:?}"
        );
    }

    #[test]
    fn live_idle_guard2_keeps_only_latest_marker_per_pid() {
        use std::collections::HashSet;
        let t = |n: u64| SystemTime::UNIX_EPOCH + Duration::from_secs(n);
        let claude_pids: HashSet<u32> = [100u32].into_iter().collect();
        // 같은 살아있는 pid를 3개 마커가 가리킴(= /clear 훅 실패로 남은 스테일 마커 유령).
        let markers = vec![
            ("stale-a".to_string(), 100u32, t(10)),
            ("newest".to_string(), 100u32, t(30)),
            ("stale-b".to_string(), 100u32, t(20)),
        ];
        let keep = live_idle_marker_uuids(&markers, &claude_pids);
        assert_eq!(
            keep,
            ["newest".to_string()].into_iter().collect::<HashSet<_>>(),
            "가드②: mtime 최신 하나만: {keep:?}"
        );
    }

    #[test]
    fn enumerate_idle_revives_open_session_and_respects_filters() {
        use std::collections::HashSet;
        let base = std::env::temp_dir().join(format!("tuna-p8-{}", std::process::id()));
        let marker_dir = base.join("markers");
        let projects_dir = base.join("projects");
        let proj = projects_dir.join("D--privateProject-tunaRound");
        std::fs::create_dir_all(&marker_dir).unwrap();
        std::fs::create_dir_all(&proj).unwrap();
        // 유휴이지만 열려 있는 세션(마커 pid=100 산 claude, jsonl은 오래됨). 되살아나야 함.
        std::fs::write(
            proj.join("idle-1.jsonl"),
            "{\"type\":\"summary\"}\n{\"type\":\"user\",\"cwd\":\"D:\\\\privateProject\\\\tunaRound\",\"message\":{\"content\":\"안녕\"}}\n",
        ).unwrap();
        std::fs::write(marker_dir.join("idle-1.ctx"), "100").unwrap();
        // automation 세션(첫 user 메시지 <!-- 마커) - 마커 pid는 살아있어도 노이즈 필터로 제외돼야 함.
        // (각 세션은 고유 owner pid를 가진다 - 가드②는 pid별 최신 하나만 남기므로 별도 pid를 준다.)
        std::fs::write(
            proj.join("auto-1.jsonl"),
            "{\"type\":\"summary\"}\n{\"type\":\"user\",\"cwd\":\"D:\\\\privateProject\\\\tunaRound\",\"message\":{\"content\":\"<!-- secall:wiki -->\"}}\n",
        ).unwrap();
        std::fs::write(marker_dir.join("auto-1.ctx"), "200").unwrap();
        // tombstone(dead)·unknown 마커는 P8 비대상(Pid만 대상, 가드③=마커없음은 기존 창 폴백).
        std::fs::write(marker_dir.join("gone-1.ctx"), "dead").unwrap();
        std::fs::write(marker_dir.join("unk-1.ctx"), "unknown").unwrap();
        // jsonl 없는 마커(세션 파일 소멸)는 조용히 스킵.
        std::fs::write(marker_dir.join("orphan.ctx"), "300").unwrap();

        let claude_pids: HashSet<u32> = [100u32, 200, 300].into_iter().collect();
        let existing: HashSet<String> = HashSet::new();
        let found = enumerate_idle_marker_sessions(
            &marker_dir,
            &projects_dir,
            &claude_pids,
            &existing,
            None,
        );
        let uuids: Vec<&str> = found.iter().map(|s| s.uuid.as_str()).collect();
        assert_eq!(
            uuids,
            vec!["idle-1"],
            "유휴 열림 세션만 되살림(automation·dead·unknown·orphan 제외): {uuids:?}"
        );
        assert_eq!(found[0].runner, "claude");
        assert_eq!(found[0].project.as_deref(), Some("tunaRound"));
        assert_eq!(found[0].human_input_at, None);

        // 이미 신선도 창으로 잡힌(existing) uuid는 다시 추가하지 않는다(기존 우선).
        let existing2: HashSet<String> = ["idle-1".to_string()].into_iter().collect();
        let none = enumerate_idle_marker_sessions(
            &marker_dir,
            &projects_dir,
            &claude_pids,
            &existing2,
            None,
        );
        assert!(
            none.is_empty(),
            "existing에 있으면 중복 추가 안 함: {none:?}"
        );

        // pid가 죽으면(가드①) 되살리지 않는다.
        let dead_pids: HashSet<u32> = HashSet::new();
        let none2 =
            enumerate_idle_marker_sessions(&marker_dir, &projects_dir, &dead_pids, &existing, None);
        assert!(
            none2.is_empty(),
            "산 claude pid 없으면 되살림 없음: {none2:?}"
        );

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn enumerate_idle_empty_when_no_markers() {
        // 마커 디렉토리 자체가 없으면(가드③ 폴백 = 기존 enumerate 담당) 빈 결과.
        use std::collections::HashSet;
        let missing = std::env::temp_dir().join(format!("tuna-p8-none-{}", std::process::id()));
        let claude_pids: HashSet<u32> = [100u32].into_iter().collect();
        let existing: HashSet<String> = HashSet::new();
        let found = enumerate_idle_marker_sessions(
            &missing.join("markers"),
            &missing.join("projects"),
            &claude_pids,
            &existing,
            None,
        );
        assert!(found.is_empty());
    }

    /// cli_daemons.rs presence_scan 루프의 게이트 합성 순서(tombstone → codex 사람활동 게이트 →
    /// 죽은 owner-pid 마커 제외 → idle 캐시 병합)를 그대로 재현하는 시나리오 테스트. 개별 게이트는
    /// 각자 단위테스트가 있지만, 순서대로 합성했을 때의 누적 효과(이 단계에서 뭐가 왜 빠지는지)는
    /// 무테스트였다(회귀 위험: 순서를 바꾸면 tombstone된 codex 유령이 codex 게이트를 먼저 통과해버리는
    /// 식의 은닉 버그가 생길 수 있다).
    #[test]
    fn presence_pipeline_composes_gates_in_cli_daemons_order() {
        use std::collections::HashSet;
        let marker_dir =
            std::env::temp_dir().join(format!("tuna-pipeline-{}-{}", std::process::id(), line!()));
        std::fs::create_dir_all(&marker_dir).unwrap();
        // SessionEnd 훅이 깨끗한 종료를 기록한 세션(tombstone) → filter_tombstoned 대상.
        std::fs::write(marker_dir.join("claude-tomb.ctx"), "dead").unwrap();
        // owner pid가 기록됐으나 이번 스냅샷엔 없는(죽은) 세션 → filter_dead_sessions 대상.
        std::fs::write(marker_dir.join("claude-deadpid.ctx"), "9999").unwrap();
        // owner pid가 스냅샷에 살아있는 세션 → 끝까지 생존.
        std::fs::write(marker_dir.join("claude-live.ctx"), "100").unwrap();

        let threshold = "2026-07-11 09:00:00";
        let sessions = vec![
            LiveSession {
                uuid: "claude-tomb".into(),
                runner: "claude".into(),
                project: None,
                human_input_at: None,
                created_at: None,
            },
            LiveSession {
                uuid: "claude-deadpid".into(),
                runner: "claude".into(),
                project: None,
                human_input_at: None,
                created_at: None,
            },
            LiveSession {
                uuid: "claude-live".into(),
                runner: "claude".into(),
                project: None,
                human_input_at: None,
                created_at: None,
            },
            // 유령: 사람입력·생성 둘 다 threshold 이전(codex 게이트에서 드롭돼야 함).
            codex_session(
                "codex-ghost",
                Some("2026-07-11 08:00:00"),
                Some("2026-07-11 07:00:00"),
            ),
            // 활성: 최근 사람입력(codex 게이트 통과, 마커가 없어 filter_dead_sessions은 보수적 유지).
            codex_session("codex-fresh", Some("2026-07-11 09:30:00"), None),
        ];

        // 1) tombstone 제거는 스냅샷과 무관하게 항상 먼저 적용된다(cli_daemons.rs 주석과 동일 순서).
        let sessions = filter_tombstoned(sessions, &marker_dir);
        let after_tomb: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
        assert!(
            !after_tomb.contains("claude-tomb"),
            "tombstone 세션은 1단계에서 제거돼야 함: {after_tomb:?}"
        );
        assert_eq!(
            sessions.len(),
            4,
            "tombstone 하나만 빠져야 함: {after_tomb:?}"
        );

        // 2) codex 사람활동 신선도 게이트(#88) - 유령 codex만 드롭, claude는 무관.
        let sessions = apply_codex_human_input_gate(sessions, threshold);
        let after_codex: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
        assert!(
            !after_codex.contains("codex-ghost"),
            "유령 codex는 2단계에서 드롭돼야 함: {after_codex:?}"
        );
        assert!(after_codex.contains("codex-fresh"));
        assert_eq!(sessions.len(), 3);

        // 3) 죽은 owner-pid 마커 제외(claude-deadpid는 alive 집합에 없음).
        let alive: HashSet<u32> = [100u32].into_iter().collect();
        let sessions = filter_dead_sessions(sessions, &marker_dir, &alive);
        let after_dead: HashSet<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
        assert!(
            !after_dead.contains("claude-deadpid"),
            "죽은 owner pid 세션은 3단계에서 제외돼야 함: {after_dead:?}"
        );
        assert!(after_dead.contains("claude-live"));
        assert!(
            after_dead.contains("codex-fresh"),
            "마커 없는 codex는 filter_dead_sessions에서 보수적으로 유지돼야 함"
        );
        assert_eq!(
            sessions.len(),
            2,
            "claude-live + codex-fresh만 남아야 함: {after_dead:?}"
        );

        // 4) idle 캐시 병합(cli_daemons.rs 마지막 단계): 이번 주기에 이미 있는 uuid는 idle 사본으로
        // 덮이지 않고, 새 uuid만 추가된다.
        let mut sessions = sessions;
        let last_idle = vec![
            LiveSession {
                uuid: "idle-revived".into(),
                runner: "claude".into(),
                project: None,
                human_input_at: None,
                created_at: None,
            },
            // 이미 존재하는 uuid의 스테일 idle 사본 - 병합 시 무시돼야 한다(existing 우선).
            LiveSession {
                uuid: "claude-live".into(),
                runner: "claude".into(),
                project: Some("stale-idle-copy".into()),
                human_input_at: None,
                created_at: None,
            },
        ];
        let present: HashSet<String> = sessions.iter().map(|s| s.uuid.clone()).collect();
        sessions.extend(last_idle.into_iter().filter(|s| !present.contains(&s.uuid)));

        let mut final_uuids: Vec<&str> = sessions.iter().map(|s| s.uuid.as_str()).collect();
        final_uuids.sort_unstable();
        assert_eq!(
            final_uuids,
            vec!["claude-live", "codex-fresh", "idle-revived"]
        );
        let claude_live = sessions.iter().find(|s| s.uuid == "claude-live").unwrap();
        assert_eq!(
            claude_live.project, None,
            "이미 존재하는 세션이 idle 사본으로 덮이면 안 됨(existing 우선)"
        );

        std::fs::remove_dir_all(&marker_dir).ok();
    }
}
