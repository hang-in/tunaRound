// claude 세션 스캔: discover 열거를 presence 규약(home 정규화·temp 제외)에 맞춰 얹는다.

use super::*;

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
            active_at: None,  // claude 턴 신호는 turn-ping 훅 경로(이슈 #123).
        })
        .collect()
}
