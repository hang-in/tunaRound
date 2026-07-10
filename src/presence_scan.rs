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
}

/// cwd가 홈 디렉토리 자체면 "home", 아니면 마지막 세그먼트. 훅의 project_from_cwd와 같은 규약
/// (개인 폴더명=사용자명이 project로 새는 것 방지, #42). cwd 불명이면 None.
pub fn project_from_cwd_normalized(cwd: Option<&str>, home: Option<&Path>) -> Option<String> {
    let cwd = cwd?;
    if let Some(h) = home {
        let p = Path::new(cwd);
        // 경로 문자열 비교(canonicalize는 존재하지 않는 원격 경로에서 실패) - 구분자만 통일.
        let norm = |s: &Path| s.to_string_lossy().replace('\\', "/").trim_end_matches('/').to_lowercase();
        if norm(p) == norm(h) {
            return Some("home".to_string());
        }
    }
    crate::discover::project_from_cwd(cwd)
}

/// cwd가 시스템 temp 아래인지(자동화 headless 세션 = 로스터 노이즈, 훅 is_temp_cwd와 같은 규약).
pub fn is_temp_cwd(cwd: &str) -> bool {
    let t = std::env::temp_dir();
    let norm = |s: &str| s.replace('\\', "/").trim_end_matches('/').to_lowercase();
    let (c, t) = (norm(cwd), norm(&t.to_string_lossy()));
    c == t || c.starts_with(&format!("{t}/"))
}

/// codex rollout jsonl의 session_meta 줄에서 (session_id, cwd, originator)를 뽑는다. 실패는 None.
pub fn parse_codex_meta_line(line: &str) -> Option<(String, Option<String>, Option<String>)> {
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
    let originator = p.get("originator").and_then(|o| o.as_str()).map(str::to_string);
    Some((id, cwd, originator))
}

/// 기본 codex 세션 디렉토리(`~/.codex/sessions`). HOME 미확장이면 None.
pub fn default_codex_sessions_dir() -> Option<PathBuf> {
    let expanded = crate::config::expand_home("~/.codex/sessions");
    if expanded.starts_with("~/") { None } else { Some(PathBuf::from(expanded)) }
}

/// `~/.codex/sessions/YYYY/MM/DD/rollout-*.jsonl`을 재귀 스캔해 stale 이내 mtime의 라이브 TUI 세션을
/// 낸다. originator가 codex-tui가 아닌 것(exec 등 헤드리스)은 제외(로스터=열린 TUI 세션 계약).
pub fn enumerate_codex_sessions(
    sessions_dir: &Path,
    now: SystemTime,
    stale: Duration,
    home: Option<&Path>,
) -> Vec<LiveSession> {
    let mut out = Vec::new();
    let mut stack = vec![sessions_dir.to_path_buf()];
    // 디렉토리 깊이는 YYYY/MM/DD 고정이지만 방어적으로 상한을 둔다(심볼릭 링크 순환 등).
    let mut visited = 0usize;
    while let Some(dir) = stack.pop() {
        visited += 1;
        if visited > 10_000 {
            break;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for e in entries.flatten() {
            let path = e.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.starts_with("rollout-") || path.extension().and_then(|x| x.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            if crate::discover::age_secs_since(mtime, now) as u64 > stale.as_secs() {
                continue;
            }
            let Some(first) = read_first_line(&path) else { continue };
            let Some((uuid, cwd, originator)) = parse_codex_meta_line(&first) else { continue };
            if originator.as_deref() != Some("codex-tui") {
                continue; // exec/워커 rollout은 로스터 대상 아님.
            }
            if cwd.as_deref().is_some_and(is_temp_cwd) {
                continue;
            }
            let project = project_from_cwd_normalized(cwd.as_deref(), home);
            out.push(LiveSession { uuid, runner: "codex".to_string(), project });
        }
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    out.dedup_by(|a, b| a.uuid == b.uuid); // 같은 세션의 rollout이 복수면 1건만.
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
        })
        .collect()
}

/// 러너 프로세스 수를 센다(win=tasklist CSV, unix=ps comm). 조회 실패는 None(판단 불가 = 게이트 안 함).
/// per-session 매핑이 아니라 러너 단위 러프 체크다: 0개면 그 러너 세션 전부 죽음(재부팅·전원 종료 즉시 반영).
pub fn count_runner_processes(name: &str) -> Option<usize> {
    let out = if cfg!(target_os = "windows") {
        std::process::Command::new("tasklist").args(["/FO", "CSV", "/NH"]).output()
    } else {
        std::process::Command::new("ps").args(["-axco", "comm="]).output()
    }
    .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).to_lowercase();
    let needle = name.to_lowercase();
    Some(
        text.lines()
            .filter(|l| {
                // win CSV 첫 필드("claude.exe") / unix comm 라인("claude")에서 이름 매칭.
                let head = l.split(',').next().unwrap_or(l).trim_matches('"').trim();
                head == needle || head == format!("{needle}.exe")
            })
            .count(),
    )
}

/// 프로세스 게이트: 해당 러너 프로세스가 확실히 0개면(count=Some(0)) 그 러너 세션을 전부 제외한다.
/// None(조회 실패)이나 1개 이상이면 그대로 둔다(파일 신선도 창이 상한).
pub fn apply_process_gate(sessions: Vec<LiveSession>, runner: &str, count: Option<usize>) -> Vec<LiveSession> {
    match count {
        Some(0) => sessions.into_iter().filter(|s| s.runner != runner).collect(),
        _ => sessions,
    }
}

/// report_presence의 sessions JSON 배열로 직렬화한다. display_name = {machine}-{runner}-{project|?}.
pub fn to_report_json(machine: &str, sessions: &[LiveSession]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            let display = format!("{machine}-{}-{}", s.runner, s.project.as_deref().unwrap_or("unknown"));
            serde_json::json!({
                "uuid": s.uuid,
                "runner": s.runner,
                "project": s.project,
                "display_name": display,
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
        let line = r#"{"timestamp":"t","type":"session_meta","payload":{"session_id":"abc-123","id":"abc-123","cwd":"C:\\Users\\me\\proj","originator":"codex-tui"}}"#;
        let (id, cwd, orig) = parse_codex_meta_line(line).unwrap();
        assert_eq!(id, "abc-123");
        assert_eq!(cwd.as_deref(), Some("C:\\Users\\me\\proj"));
        assert_eq!(orig.as_deref(), Some("codex-tui"));
        // session_meta가 아닌 줄은 None.
        assert!(parse_codex_meta_line(r#"{"type":"turn","payload":{}}"#).is_none());
    }

    #[test]
    fn project_normalizes_home_and_falls_back_to_basename() {
        let home = Path::new("C:\\Users\\me");
        assert_eq!(project_from_cwd_normalized(Some("C:\\Users\\me"), Some(home)), Some("home".to_string()));
        // 대소문자·구분자 차이도 home으로 인식.
        assert_eq!(project_from_cwd_normalized(Some("c:/users/me/"), Some(home)), Some("home".to_string()));
        assert_eq!(
            project_from_cwd_normalized(Some("C:\\Users\\me\\tunaRound"), Some(home)),
            Some("tunaRound".to_string())
        );
        assert_eq!(project_from_cwd_normalized(None, Some(home)), None);
    }

    #[test]
    fn process_gate_drops_runner_only_when_zero() {
        let s = |r: &str| LiveSession { uuid: r.to_string(), runner: r.to_string(), project: None };
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
        mk("not-a-rollout.jsonl", r#"{"type":"session_meta","payload":{"session_id":"zzz","originator":"codex-tui"}}"#);
        let found = enumerate_codex_sessions(&dir, SystemTime::now(), Duration::from_secs(3600), None);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!(found.len(), 1, "TUI 세션만: {found:?}");
        assert_eq!(found[0].uuid, "tui-1");
        assert_eq!(found[0].project.as_deref(), Some("projA"));
    }

    #[test]
    fn report_json_shape_and_display_name() {
        let sessions = vec![LiveSession {
            uuid: "u1".into(),
            runner: "claude".into(),
            project: Some("tunaRound".into()),
        }];
        let v = to_report_json("win", &sessions);
        assert_eq!(v[0]["uuid"], "u1");
        assert_eq!(v[0]["display_name"], "win-claude-tunaRound");
    }
}
