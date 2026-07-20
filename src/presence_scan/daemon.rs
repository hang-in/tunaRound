// claude 데몬(bg slash) 세션 판정: roster.json 파싱과 로스터 유령 필터(이슈 #161).

use super::*;

/// claude 데몬(`~/.claude/daemon/roster.json`)이 소유한 세션 판정 정보. `fork_uuids`=데몬이 돌리는
/// bg fork 세션(진짜 TUI가 아님), `resume_source_uuids`=fork가 되살린 원본 세션(jsonl mtime만
/// 갱신됨). 둘 다 스캐너 신선도 폴백의 "mtime = 사람 세션 활동" 가정을 깨는 원천이다.
#[derive(Debug, Default, PartialEq)]
pub struct DaemonRoster {
    /// 데몬 bg fork 세션 uuid(workers.*.sessionId). 로스터에서 무조건 제외한다(bg 세션에서도
    /// SessionStart 훅이 돌아 Pid 마커가 생기므로 유휴 부활 경로까지 막아야 한다 - 실측 2026-07-20).
    pub fork_uuids: std::collections::HashSet<String>,
    /// fork가 resume한 소스 세션 uuid(workers.*.dispatch.launch.sessionId의 stem,
    /// mode=="resume" && fork==true 한정). 마커가 없을(NoMarker) 때만 제외한다 - 같은 세션이
    /// 실제 TUI로도 열려 있으면(Pid 마커) 산 세션이므로 유지.
    pub resume_source_uuids: std::collections::HashSet<String>,
}

/// 마커 파일명 sanitize 집합과 같은 규약(read_marker 참조)의 uuid 후보 판정. 경로 구분자 등
/// 허용 밖 문자가 섞인 값은 신뢰 경계 밖(데몬 파일 내용)이므로 버린다.
fn is_uuid_like(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// roster.json(proto 1) 텍스트를 파싱한다. Claude Code 내부 포맷이라 버전 취약성을 fail-open으로
/// 흡수한다: 파싱 실패·proto 불일치·필드 부재 = 빈 집합(필터 미적용 = 현행 동작 유지).
pub fn parse_daemon_roster(text: &str) -> DaemonRoster {
    let mut out = DaemonRoster::default();
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return out;
    };
    if v.get("proto").and_then(serde_json::Value::as_u64) != Some(1) {
        return out;
    }
    let Some(workers) = v.get("workers").and_then(serde_json::Value::as_object) else {
        return out;
    };
    for w in workers.values() {
        if let Some(sid) = w.get("sessionId").and_then(serde_json::Value::as_str)
            && is_uuid_like(sid)
        {
            out.fork_uuids.insert(sid.to_string());
        }
        let Some(launch) = w.get("dispatch").and_then(|d| d.get("launch")) else {
            continue;
        };
        if launch.get("mode").and_then(serde_json::Value::as_str) != Some("resume")
            || launch.get("fork").and_then(serde_json::Value::as_bool) != Some(true)
        {
            continue;
        }
        // launch.sessionId는 resume 소스의 jsonl **경로**로 실측됐다(bare id 가능성도 stem이 흡수).
        // 백슬래시는 슬래시로 정규화해 win 경로를 어느 OS에서 파싱해도 stem이 같게 한다.
        if let Some(src) = launch.get("sessionId").and_then(serde_json::Value::as_str) {
            let norm = src.replace('\\', "/");
            let stem = Path::new(&norm)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or(norm.as_str());
            if is_uuid_like(stem) {
                out.resume_source_uuids.insert(stem.to_string());
            }
        }
    }
    out
}

/// roster.json 파일을 읽어 파싱한다. 부재·읽기 실패 = 빈 집합(fail-open). 데몬 종료 후 파일이
/// 남아도 안전하다: fork uuid는 애초에 TUI 세션이 아니고, resume 소스는 사람이 실제로 다시 열면
/// SessionStart 훅이 Pid 마커를 써 NoMarker 조건을 벗어나므로 정상 표시된다(별도 staleness 불요).
pub fn read_daemon_roster(path: &Path) -> DaemonRoster {
    match std::fs::read_to_string(path) {
        Ok(text) => parse_daemon_roster(&text),
        Err(_) => DaemonRoster::default(),
    }
}

/// claude 데몬 bg 세션 유령을 로스터에서 제외한다(이슈 #161). fork 세션=무조건 제외(신선도·
/// 유휴 부활 두 경로 모두), resume 소스=NoMarker일 때만 제외(tombstone은 filter_tombstoned가,
/// 죽은 Pid는 filter_dead_sessions가 이미 처리). claude 러너에만 적용한다.
pub fn filter_daemon_bg_sessions(
    sessions: Vec<LiveSession>,
    roster: &DaemonRoster,
    marker_dir: &Path,
) -> Vec<LiveSession> {
    if roster.fork_uuids.is_empty() && roster.resume_source_uuids.is_empty() {
        return sessions;
    }
    sessions
        .into_iter()
        .filter(|s| {
            if s.runner != "claude" {
                return true;
            }
            if roster.fork_uuids.contains(&s.uuid) {
                return false;
            }
            if roster.resume_source_uuids.contains(&s.uuid) {
                return read_marker(marker_dir, &s.uuid) != MarkerState::NoMarker;
            }
            true
        })
        .collect()
}
