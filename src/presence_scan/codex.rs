// codex 세션 스캔: rollout jsonl 파싱, 사람 입력 tail 스캔, 입력시각 캐시, DB datetime 정규화, 사람활동 신선도 게이트.

use super::*;

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
pub(super) const CODEX_TAIL_BYTES: usize = 256 * 1024;

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
/// 정규화한다('T'→공백, 소수초 절단 + Z/영(0) offset은 UTC와 같으므로 그대로 절단). §5-3 계약: 'T' > ' '
/// 사전순 왜곡을 없애 로스터·영속 워터마크와 비교 가능하게 한다. 결과가 19자 DB 포맷이 아니면 None(오염 방어).
/// **비영 offset은 UTC로 가감 변환하지 않고 None을 반환한다**(발견: 과거엔 offset을 변환 없이 그냥
/// 잘라 UTC로 오인했다 - 예를 들어 "+09:00"의 9시간이 유령처럼 잔존하거나 음수 offset이 라이브 세션을
/// 조기 드롭시킬 수 있었다. codex rollout이 실측상 전부 Z(UTC)라 지금까지 드러나지 않았을 뿐이다).
/// 비영 offset 관측은 eprintln으로 남겨 로컬 offset 포맷 유입이 조용히 넘어가지 않게 한다(폴백 경로로
/// 흡수되되 관측 가능하게).
pub fn normalize_iso_to_db_datetime(ts: &str) -> Option<String> {
    // 날짜의 '-'와 타임존 offset '-'(-05:00)를 혼동하지 않게 먼저 날짜/시간을 분리한다(공백 또는 'T').
    let replaced = ts.trim().replacen('T', " ", 1);
    let mut parts = replaced.split_whitespace();
    let date = parts.next()?;
    let time = parts.next()?;
    // offset은 ISO8601 고정 순서(HH:MM:SS[.ffffff][Z|±HH:MM])상 항상 소수초 뒤·문자열 끝에 온다.
    // 소수초 안에 '+'/'-'가 섞일 일이 없으므로, 소수초 절단보다 offset 탐지를 먼저 해야 안전하다
    // (소수초부터 잘라내면 그 뒤에 붙은 offset이 통째로 사라져 비영 offset을 놓친다).
    let (before_offset, offset) = match time.find(['+', '-']) {
        Some(idx) => (&time[..idx], Some(&time[idx..])),
        None => match time.strip_suffix('Z') {
            Some(t) => (t, None),
            None => (time, None),
        },
    };
    if let Some(off) = offset {
        // 영(0) offset("+00:00"/"-00:00")만 UTC와 같아 안전하게 절단한다. 비영 offset을 변환 없이
        // 버리면 그 시간만큼 조용히 왜곡되므로 거부한다(기존 Z·오프셋없음 케이스는 이 분기를 안 탄다).
        if off != "+00:00" && off != "-00:00" {
            eprintln!(
                "[presence] normalize_iso_to_db_datetime: 비영 offset({off}) 관측, 변환 없이 거부: {ts}"
            );
            return None;
        }
    }
    let core_time = before_offset
        .split('.')
        .next()
        .unwrap_or(before_offset)
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
pub(super) fn message_is_human_input(message: &str) -> bool {
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

/// CodexInputCache 디스크 영속화 포맷(취약성 완화, 이슈 #88 minor2 후속). SystemTime은 직접
/// 직렬화되지 않아 UNIX epoch 초로 담는다.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct PersistedCodexInputCache(std::collections::HashMap<String, (u64, Option<String>)>);

/// 디스크에서 CodexInputCache를 로드한다(스캐너 기동 첫 주기 seed 전용). 파일 부재·손상·파싱 실패는
/// 전부 빈 캐시로 폴백한다(fail-open - 이 캐시는 애초에 "없어도 되는" 최선의 힌트일 뿐이다).
///
/// **잔여 취약성**: 이 파일은 스캐너 루프가 매 주기 갱신 저장하므로(아래 save), 데몬이 재시작되는
/// 순간까지의 값은 복구되지만 "재시작 직전 마지막 저장~재시작 사이"의 변화는 여전히 유실된다. 완전한
/// 해결(브로커의 영속 agent_human_input을 조회해 seed)은 별도 MCP 조회 배선이 필요해 더 큰 변경이라
/// 후속으로 남긴다 - 이 디스크 캐시만으로도 "매 배포마다 완전히 빈 캐시로 재시작"이라는 취약성의
/// 폭을 크게 좁힌다(정상 종료·재기동 사이 값은 보존).
pub fn load_codex_input_cache_from_disk(path: &Path) -> CodexInputCache {
    let Ok(text) = std::fs::read_to_string(path) else {
        return CodexInputCache::new();
    };
    let Ok(parsed) = serde_json::from_str::<PersistedCodexInputCache>(&text) else {
        return CodexInputCache::new();
    };
    parsed
        .0
        .into_iter()
        // 손상·조작된 캐시의 비정상적으로 큰 secs는 UNIX_EPOCH + Duration이 오버플로 패닉을 낼 수
        // 있으므로 checked_add로 안전하게 처리하고, 넘치는 항목은 건너뛴다(gemini HIGH: 데몬 크래시 방지).
        .filter_map(|(k, (secs, hi))| {
            SystemTime::UNIX_EPOCH
                .checked_add(Duration::from_secs(secs))
                .map(|t| (k, (t, hi)))
        })
        .collect()
}

/// CodexInputCache를 디스크에 저장한다(매 스캔 주기 후 best-effort, 실패는 조용히 무시 - 캐시는
/// 힌트일 뿐이라 저장 실패가 스캐너를 막으면 안 된다).
pub fn save_codex_input_cache_to_disk(path: &Path, cache: &CodexInputCache) {
    let serializable: std::collections::HashMap<String, (u64, Option<String>)> = cache
        .iter()
        .filter_map(|(k, (mtime, hi))| {
            mtime
                .duration_since(SystemTime::UNIX_EPOCH)
                .ok()
                .map(|d| (k.clone(), (d.as_secs(), hi.clone())))
        })
        .collect();
    if let Ok(json) = serde_json::to_string(&PersistedCodexInputCache(serializable)) {
        let _ = std::fs::write(path, json);
    }
}

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
            // 이슈 #123: rollout mtime = "지금 응답 생성 중" 프록시(턴 중 append로 신선 유지).
            active_at: system_time_to_db_datetime(mtime),
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
///
/// **hybrid 게이트(이슈 #119)**: 위 (1)의 "PID-마커 이식 불가"는 app-server 프로토콜 얘기였을 뿐, codex CLI
/// 자체를 PATH shim(`scripts/codex_wrapper.py`)으로 감싸면 래퍼 프로세스의 threadId<->PID 생존은 claude와
/// 동형으로 마커링(`~/.tunaround/autoarm/<threadId>.ctx`)할 수 있다(codex_wrapper.py가 argv `resume <uuid>`
/// 또는 rollout session_meta 관측으로 threadId를 바인딩해 기록). `marker_dir`가 주어지면 시간창보다
/// **마커를 우선** 판정한다: `Dead`(래퍼가 종료를 확정 통보) → window 무관 즉시 드롭 / `Pid`·`Unknown`(래퍼
/// 생존 중 또는 owner 미상) → window 면제하고 유지 - **PID가 실제로 살아있는지의 권위 판정은 이 함수가 아니라
/// 뒤이어 합성되는 [`filter_dead_sessions`]가 프로세스 스냅샷으로 내린다**(여기선 "래퍼가 아직 안 죽었다고
/// 주장한다"는 값싼 사전 통과만) / `NoMarker`(래퍼 비경유 - VS Code 자체 codex 확장 등) → 기존 시간창 판정으로
/// 폴백한다(마커가 없는 세션은 이 hybrid 도입 전과 동일하게 동작, 하위호환). `marker_dir=None`이면 마커 조회를
/// 아예 생략하고 순수 시간창 판정만 수행한다(기존 호출부·테스트 하위호환, presence-scan은 항상 Some을 준다).
pub fn apply_codex_human_input_gate(
    sessions: Vec<LiveSession>,
    min_active_db: &str,
    marker_dir: Option<&Path>,
) -> Vec<LiveSession> {
    sessions
        .into_iter()
        .filter(|s| {
            if s.runner != "codex" {
                return true; // codex 전용 게이트.
            }
            if let Some(dir) = marker_dir {
                match read_marker(dir, &s.uuid) {
                    MarkerState::Dead => return false, // 래퍼가 종료 확정 통보 → window 무관 드롭.
                    MarkerState::Pid(_) | MarkerState::Unknown => return true, // 래퍼 생존 중 → window 면제(PID 생존 권위 판정은 filter_dead_sessions 몫).
                    MarkerState::NoMarker => {} // 래퍼 비경유 → 아래 window 판정으로 폴백.
                }
            }
            let fresh = |ts: &Option<String>| ts.as_deref().is_some_and(|t| t >= min_active_db);
            fresh(&s.human_input_at) || fresh(&s.created_at)
        })
        .collect()
}
