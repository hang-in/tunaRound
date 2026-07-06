// 로컬 머신의 실행 중 Claude Code 세션을 열거해 브로커에 발견 후보로 보고하는 순수 함수와 스캐너.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// 발견된 로컬 세션 한 건(MVP=claude). 브로커 report_candidates의 CandidateInput으로 직렬화해 보고한다.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredSession {
    /// jsonl 파일 stem(= Claude Code 세션 id). roster uuid와 같은 공간이라 armed overlay가 성립한다.
    pub uuid: String,
    /// cwd basename에서 추출한 프로젝트명(불명이면 None).
    pub project: Option<String>,
    /// jsonl mtime 경과 초(세션 활동 신선도).
    pub age_secs: i64,
}

/// cwd 문자열에서 프로젝트명(마지막 경로 세그먼트)을 뽑는다. `/`·`\` 모두 분리자로 보고 후행 분리자·
/// 빈 세그먼트는 건너뛴다. 빈 결과는 None. mangled 디렉토리명 디코딩보다 cwd가 정확하므로 이걸 쓴다.
pub fn project_from_cwd(cwd: &str) -> Option<String> {
    cwd.split(['/', '\\'])
        .rfind(|s| !s.is_empty())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
}

/// Claude Code 세션 jsonl의 첫 줄(JSON 한 건)에서 cwd 필드를 뽑는다. 파싱 실패/부재는 None.
pub fn parse_cwd_from_jsonl_line(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    v.get("cwd")?.as_str().map(|s| s.to_string())
}

/// now - mtime을 초로 환산한다(시계 오차로 now<mtime이면 0으로 클램프).
pub fn age_secs_since(mtime: SystemTime, now: SystemTime) -> i64 {
    match now.duration_since(mtime) {
        Ok(d) => d.as_secs() as i64,
        Err(_) => 0,
    }
}

/// jsonl 파일의 첫 줄만 읽는다(전체 로드 회피). 열기/읽기 실패는 None.
pub fn read_first_line(path: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};
    let f = std::fs::File::open(path).ok()?;
    let mut reader = BufReader::new(f);
    let mut line = String::new();
    match reader.read_line(&mut line) {
        Ok(0) | Err(_) => None,
        Ok(_) => Some(line),
    }
}

/// 기본 Claude Code 프로젝트 디렉토리(`~/.claude/projects`)를 반환한다. HOME/USERPROFILE 미설정이면 None.
pub fn default_projects_dir() -> Option<PathBuf> {
    let expanded = crate::config::expand_home("~/.claude/projects");
    // expand_home은 확장 실패 시 원본("~/...")을 그대로 돌려주므로, 확장 안 됐으면 None.
    if expanded.starts_with("~/") {
        None
    } else {
        Some(PathBuf::from(expanded))
    }
}

/// `~/.claude/projects/<mangled-cwd>/<uuid>.jsonl`을 스캔해 stale 이내 mtime의 세션을 후보로 낸다.
/// project는 각 jsonl 첫 줄의 cwd에서 추출(정확), 없으면 None. uuid=파일 stem. uuid 오름차순 정렬.
pub fn enumerate_claude_sessions(
    projects_dir: &Path,
    now: SystemTime,
    stale: Duration,
) -> Vec<DiscoveredSession> {
    let mut out = Vec::new();
    let Ok(subdirs) = std::fs::read_dir(projects_dir) else {
        return out;
    };
    for sub in subdirs.flatten() {
        let subpath = sub.path();
        if !subpath.is_dir() {
            continue;
        }
        let Ok(files) = std::fs::read_dir(&subpath) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Ok(meta) = file.metadata() else { continue };
            let Ok(mtime) = meta.modified() else { continue };
            let age = age_secs_since(mtime, now);
            // stale window 밖(오래된 mtime)이면 비활동으로 스킵.
            if age as u64 > stale.as_secs() {
                continue;
            }
            let Some(uuid) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let project = read_first_line(&path)
                .and_then(|l| parse_cwd_from_jsonl_line(&l))
                .and_then(|cwd| project_from_cwd(&cwd));
            out.push(DiscoveredSession { uuid: uuid.to_string(), project, age_secs: age });
        }
    }
    out.sort_by(|a, b| a.uuid.cmp(&b.uuid));
    out
}

/// 발견된 세션들을 report_candidates 툴이 받는 candidates JSON 배열로 직렬화한다.
/// source는 발견 출처(claude-jsonl 고정, MVP), runner=claude. project=None이면 필드 생략(null).
pub fn sessions_to_candidates_json(sessions: &[DiscoveredSession]) -> serde_json::Value {
    let arr: Vec<serde_json::Value> = sessions
        .iter()
        .map(|s| {
            serde_json::json!({
                "uuid": s.uuid,
                "runner": "claude",
                "project": s.project,
                "source": "claude-jsonl",
                "age_secs": s.age_secs,
            })
        })
        .collect();
    serde_json::Value::Array(arr)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_from_cwd_extracts_last_segment() {
        assert_eq!(project_from_cwd("D:\\privateProject\\tunaRound"), Some("tunaRound".to_string()));
        assert_eq!(project_from_cwd("/home/u/folkProject/my-harness"), Some("my-harness".to_string()));
        // 후행 분리자 무시.
        assert_eq!(project_from_cwd("/home/u/proj/"), Some("proj".to_string()));
        assert_eq!(project_from_cwd(""), None);
        assert_eq!(project_from_cwd("/"), None);
    }

    #[test]
    fn parse_cwd_from_jsonl_line_reads_cwd_field() {
        let line = r#"{"type":"user","cwd":"D:\\privateProject\\tunaRound","sessionId":"abc"}"#;
        assert_eq!(parse_cwd_from_jsonl_line(line), Some("D:\\privateProject\\tunaRound".to_string()));
        // cwd 없으면 None.
        assert_eq!(parse_cwd_from_jsonl_line(r#"{"type":"user"}"#), None);
        // JSON 아니면 None.
        assert_eq!(parse_cwd_from_jsonl_line("not json"), None);
    }

    #[test]
    fn age_secs_since_clamps_future_to_zero() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1000);
        let past = SystemTime::UNIX_EPOCH + Duration::from_secs(940);
        assert_eq!(age_secs_since(past, now), 60);
        let future = SystemTime::UNIX_EPOCH + Duration::from_secs(1100);
        assert_eq!(age_secs_since(future, now), 0);
    }

    #[test]
    fn enumerate_filters_by_stale_and_extracts_fields() {
        // 임시 projects 디렉토리에 활동/비활동 세션 jsonl을 만들어 스캔한다.
        let base = std::env::temp_dir().join(format!("tuna_discover_test_{}", std::process::id()));
        let proj = base.join("D--privateProject-tunaRound");
        std::fs::create_dir_all(&proj).unwrap();

        // 활동 세션: 방금 쓴 jsonl(cwd 포함).
        let fresh = proj.join("11111111-aaaa.jsonl");
        std::fs::write(&fresh, "{\"cwd\":\"D:\\\\privateProject\\\\tunaRound\"}\n").unwrap();

        // 비활동 세션: mtime을 과거로(수동 설정 불가하니 stale=0으로 필터 검증).
        let now = SystemTime::now();
        // stale=매우 큼 → 방금 쓴 파일 잡힘.
        let found = enumerate_claude_sessions(&base, now, Duration::from_secs(3600));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].uuid, "11111111-aaaa");
        assert_eq!(found[0].project, Some("tunaRound".to_string()));

        // stale=0(now 기준 0초 이내만) → 방금 쓴 파일도 age가 0 이상이라 잡히거나 말거나 경계.
        // 확실히 배제하려면 미래 now로 age를 크게: now+10초 기준 stale=1초면 age≈10초>1초 스킵.
        let later = now + Duration::from_secs(10);
        let none = enumerate_claude_sessions(&base, later, Duration::from_secs(1));
        assert!(none.is_empty(), "stale window 밖 세션은 제외되어야 함");

        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn sessions_to_candidates_json_shapes_array() {
        let sessions = vec![
            DiscoveredSession { uuid: "s1".into(), project: Some("tunaround".into()), age_secs: 5 },
            DiscoveredSession { uuid: "s2".into(), project: None, age_secs: 9 },
        ];
        let json = sessions_to_candidates_json(&sessions);
        let arr = json.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["uuid"], "s1");
        assert_eq!(arr[0]["runner"], "claude");
        assert_eq!(arr[0]["project"], "tunaround");
        assert_eq!(arr[0]["source"], "claude-jsonl");
        assert_eq!(arr[0]["age_secs"], 5);
        assert!(arr[1]["project"].is_null());
    }
}
