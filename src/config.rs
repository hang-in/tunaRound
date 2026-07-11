// tunaround.toml 설정 파일과 프로파일 선택·병합을 담당하는 모듈.

use std::collections::HashMap;
use std::io::Write as _;

use serde::Deserialize;

/// tunaround.toml 최상위 구조. 프로파일 맵 + 기본 프로파일 이름(선택).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profile: HashMap<String, Profile>,
}

/// 프로파일 하나. 전부 선택 필드다(미지정 필드는 CLI 값이나 프로그램 기본값을 그대로 쓴다).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct Profile {
    #[serde(default)]
    pub db: Option<String>,
    #[serde(default)]
    pub roster: Option<String>,
    #[serde(default)]
    pub recent_turns: Option<usize>,
    #[serde(default)]
    pub pull_context: Option<bool>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub search_url: Option<String>,
    #[serde(default)]
    pub search_token: Option<String>,
    #[serde(default)]
    pub search_token_env: Option<String>,
}

/// TOML 문자열을 Config로 파싱한다.
pub fn parse_config(text: &str) -> Result<Config, String> {
    toml::from_str(text).map_err(|e| format!("설정 파싱 실패: {e}"))
}

/// 경로에서 설정 파일을 읽어 파싱한다.
pub fn load_config_file(path: &str) -> Result<Config, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("설정 읽기 실패 ({path}): {e}"))?;
    parse_config(&text)
}

/// 후보 경로 중 실제 존재하는 첫 파일을 고른다(순수 로직, 존재 확인만 수행).
fn first_existing(paths: &[String]) -> Option<String> {
    paths.iter().find(|p| std::path::Path::new(p).is_file()).cloned()
}

/// 설정 파일 탐색. 우선순위: 명시 경로(`--config`) > `./tunaround.toml` > `~/.config/tunaround/config.toml`.
/// 명시 경로가 주어졌는데 파일이 없으면 에러. 그 외엔 못 찾아도 Ok(None)(설정 없음, 기존 동작 유지).
pub fn discover_config_path(explicit: Option<&str>) -> Result<Option<String>, String> {
    if let Some(p) = explicit {
        return if std::path::Path::new(p).is_file() {
            Ok(Some(p.to_string()))
        } else {
            Err(format!("설정 파일을 찾을 수 없습니다: {p}"))
        };
    }
    let candidates = vec![
        "tunaround.toml".to_string(),
        expand_home("~/.config/tunaround/config.toml"),
    ];
    Ok(first_existing(&candidates))
}

/// `--config` 지정 또는 탐색 경로에서 설정을 읽는다. 아무 것도 없으면 Ok(None)(설정 미적용).
pub fn load_config(explicit: Option<&str>) -> Result<Option<Config>, String> {
    match discover_config_path(explicit)? {
        Some(path) => load_config_file(&path).map(Some),
        None => Ok(None),
    }
}

/// 경로 선행 `~/`를 홈 디렉터리로 확장한다(HOME 우선, 없으면 USERPROFILE). 둘 다 없으면 원본 그대로 둔다.
pub fn expand_home(path: &str) -> String {
    let Some(rest) = path.strip_prefix("~/") else {
        return path.to_string();
    };
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) => format!("{}/{rest}", home.to_string_lossy()),
        None => path.to_string(),
    }
}

/// 프로파일의 검색 토큰을 해석한다. 평문(`search_token`) 우선, 없으면 `search_token_env` 이름의
/// 환경변수를 읽는다(레포·설정파일에 토큰 평문 노출을 피하려는 용도, 평문도 허용은 하되 비권장).
pub fn resolve_search_token(profile: &Profile) -> Option<String> {
    if profile.search_token.is_some() {
        return profile.search_token.clone();
    }
    profile.search_token_env.as_ref().and_then(|key| std::env::var(key).ok())
}

/// 여러 프로파일 중 사람이 고를 때, raw 입력(번호 또는 이름)을 프로파일 이름으로 판정하는 순수 로직.
/// stdin 읽기 자체는 `prompt_profile_pick`에서 하고, 이 함수는 입력 문자열만 받아 판정한다.
fn match_profile_pick(input: &str, names: &[String]) -> Result<String, String> {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return Err("입력이 없습니다.".to_string());
    }
    if let Ok(idx) = trimmed.parse::<usize>() {
        return names
            .get(idx.wrapping_sub(1))
            .cloned()
            .ok_or_else(|| format!("범위를 벗어난 번호입니다: {trimmed}"));
    }
    names
        .iter()
        .find(|n| n.as_str() == trimmed)
        .cloned()
        .ok_or_else(|| format!("일치하는 프로파일이 없습니다: {trimmed}"))
}

/// stdin에서 한 줄 읽어 프로파일을 고르는 대화형 픽커(실 입출력). `select_profile`의 interactive 분기 전용.
fn prompt_profile_pick(names: &[String]) -> Result<String, String> {
    println!("여러 프로파일이 있습니다. 번호를 선택하세요.");
    for (i, name) in names.iter().enumerate() {
        println!("  {}) {name}", i + 1);
    }
    print!("> ");
    let _ = std::io::stdout().flush();
    let mut line = String::new();
    std::io::stdin().read_line(&mut line).map_err(|e| format!("입력 읽기 실패: {e}"))?;
    match_profile_pick(&line, names)
}

/// 설정과 요청된 프로파일 이름으로부터 실제 사용할 프로파일을 고른다(순수 결정 로직, interactive 분기만 stdin).
///
/// - 프로파일이 하나도 없으면 `Ok(None)`(미적용).
/// - `requested`가 있으면 그 이름(없으면 에러).
/// - 미지정 + `default_profile`이 있으면 그것(맵에 없으면 에러).
/// - 미지정 + default 없음 + 프로파일 1개면 그것.
/// - 미지정 + default 없음 + 여러 개면: interactive면 대화형 픽커, 아니면 이름 정렬 후 첫 항목
///   (HashMap 순회 순서가 불안정하므로 정렬로 결정적 선택을 보장한다).
pub fn select_profile<'a>(
    cfg: &'a Config,
    requested: Option<&str>,
    interactive: bool,
) -> Result<Option<&'a Profile>, String> {
    if cfg.profile.is_empty() {
        return Ok(None);
    }
    if let Some(name) = requested {
        return cfg
            .profile
            .get(name)
            .map(Some)
            .ok_or_else(|| format!("프로파일을 찾을 수 없습니다: {name}"));
    }
    if let Some(default_name) = &cfg.default_profile {
        return cfg
            .profile
            .get(default_name)
            .map(Some)
            .ok_or_else(|| format!("default_profile을 찾을 수 없습니다: {default_name}"));
    }
    let mut names: Vec<String> = cfg.profile.keys().cloned().collect();
    names.sort();
    if names.len() == 1 {
        return Ok(cfg.profile.get(&names[0]));
    }
    let picked = if interactive { prompt_profile_pick(&names)? } else { names[0].clone() };
    Ok(cfg.profile.get(&picked))
}

/// CLI에서 넘어온 세션 배선 값(병합 전). main.rs 지역변수 묶음과 1:1 대응.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct MergedSessionArgs {
    pub db: Option<String>,
    pub roster: Option<String>,
    pub recent_turns: Option<usize>,
    pub pull_context: bool,
    pub session: Option<String>,
    pub search_url: Option<String>,
    pub search_token: Option<String>,
}

/// CLI 값(`cli`)에 선택된 프로파일 값을 채운다. 우선순위: CLI 플래그 > 프로파일 > 없음.
/// `pull_context`만 예외로 OR 병합한다(CLI/프로파일 중 하나라도 켜져 있으면 켜진다).
/// `profile`이 `None`이면(프로파일 미선택) `cli`를 그대로 돌려준다.
pub fn merge_profile_into(mut cli: MergedSessionArgs, profile: Option<&Profile>) -> MergedSessionArgs {
    let Some(p) = profile else {
        return cli;
    };
    if cli.db.is_none() {
        cli.db = p.db.as_deref().map(expand_home);
    }
    if cli.roster.is_none() {
        cli.roster = p.roster.as_deref().map(expand_home);
    }
    if cli.recent_turns.is_none() {
        cli.recent_turns = p.recent_turns;
    }
    cli.pull_context = cli.pull_context || p.pull_context.unwrap_or(false);
    if cli.session.is_none() {
        cli.session = p.session.clone();
    }
    if cli.search_url.is_none() {
        cli.search_url = p.search_url.clone();
    }
    if cli.search_token.is_none() {
        cli.search_token = resolve_search_token(p);
    }
    cli
}

// node.toml 워커 노드 설정은 세션 프로파일과 별개 도메인이라 config/node.rs 서브모듈로 분리한다.
// 재export로 기존 `crate::config::{NodeConfig, Lane, parse_node_config, ...}` 경로를 그대로 유지한다.
mod node;
pub use node::*;

// cargo test는 테스트를 병렬 실행하는데 set_var/remove_var는 프로세스 전역 env 블록을 비원자적으로
// 바꿔 서로 다른 변수라도 동시 수정 시 libc 레벨 UB가 날 수 있다(gemini 지적). config.rs·config/node.rs의
// env 만지는 테스트가 하나의 락으로 직렬화되도록 모듈 레벨 pub(crate)로 둔다(자식 node가 super::ENV_LOCK로 공유).
#[cfg(test)]
pub(crate) static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;

    fn profile_with_db(db: &str) -> Profile {
        Profile { db: Some(db.to_string()), ..Default::default() }
    }

    #[test]
    fn parse_config_reads_default_and_profiles() {
        let toml_text = r#"
default_profile = "local"

[profile.local]
db = "~/.tunaround/local.db"
pull_context = false

[profile.homelab]
search_url = "https://example.internal/mcp"
search_token_env = "TUNA_TOKEN"
pull_context = true
db = "~/.tunaround/homelab.db"
recent_turns = 20
"#;
        let cfg = parse_config(toml_text).expect("파싱 성공");
        assert_eq!(cfg.default_profile.as_deref(), Some("local"));
        assert_eq!(cfg.profile.len(), 2);
        let local = cfg.profile.get("local").expect("local 존재");
        assert_eq!(local.db.as_deref(), Some("~/.tunaround/local.db"));
        assert_eq!(local.pull_context, Some(false));
        assert_eq!(local.roster, None);
        let homelab = cfg.profile.get("homelab").expect("homelab 존재");
        assert_eq!(homelab.search_url.as_deref(), Some("https://example.internal/mcp"));
        assert_eq!(homelab.search_token_env.as_deref(), Some("TUNA_TOKEN"));
        assert_eq!(homelab.pull_context, Some(true));
        assert_eq!(homelab.recent_turns, Some(20));
    }

    #[test]
    fn parse_config_invalid_toml_errors() {
        let err = parse_config("this is not valid toml {{{").unwrap_err();
        assert!(err.contains("파싱"), "에러 메시지: {err}");
    }

    #[test]
    fn select_profile_no_profiles_is_none() {
        let cfg = Config::default();
        assert_eq!(select_profile(&cfg, None, false).unwrap(), None);
        // 설정은 있으나 프로파일이 없으면 --profile 지정 여부와 무관하게 미적용 취급.
        assert_eq!(select_profile(&cfg, Some("x"), false).unwrap(), None);
    }

    #[test]
    fn select_profile_requested_found_and_missing() {
        let mut cfg = Config::default();
        cfg.profile.insert("local".to_string(), profile_with_db("local.db"));
        cfg.profile.insert("homelab".to_string(), profile_with_db("homelab.db"));

        let found = select_profile(&cfg, Some("homelab"), false).unwrap();
        assert_eq!(found.unwrap().db.as_deref(), Some("homelab.db"));

        let err = select_profile(&cfg, Some("nope"), false).unwrap_err();
        assert!(err.contains("nope"), "에러 메시지: {err}");
    }

    #[test]
    fn select_profile_uses_default_when_unspecified() {
        let mut cfg = Config::default();
        cfg.profile.insert("local".to_string(), profile_with_db("local.db"));
        cfg.profile.insert("homelab".to_string(), profile_with_db("homelab.db"));
        cfg.default_profile = Some("homelab".to_string());

        let picked = select_profile(&cfg, None, false).unwrap();
        assert_eq!(picked.unwrap().db.as_deref(), Some("homelab.db"));
    }

    #[test]
    fn select_profile_default_pointing_to_missing_profile_errors() {
        let mut cfg = Config::default();
        cfg.profile.insert("local".to_string(), profile_with_db("local.db"));
        cfg.default_profile = Some("ghost".to_string());

        let err = select_profile(&cfg, None, false).unwrap_err();
        assert!(err.contains("ghost"), "에러 메시지: {err}");
    }

    #[test]
    fn select_profile_single_profile_auto_selected() {
        let mut cfg = Config::default();
        cfg.profile.insert("only".to_string(), profile_with_db("only.db"));

        let picked = select_profile(&cfg, None, false).unwrap();
        assert_eq!(picked.unwrap().db.as_deref(), Some("only.db"));
    }

    #[test]
    fn select_profile_multiple_no_default_noninteractive_picks_first_alphabetically() {
        let mut cfg = Config::default();
        cfg.profile.insert("zeta".to_string(), profile_with_db("zeta.db"));
        cfg.profile.insert("alpha".to_string(), profile_with_db("alpha.db"));
        cfg.profile.insert("mid".to_string(), profile_with_db("mid.db"));

        let picked = select_profile(&cfg, None, false).unwrap();
        // 이름 정렬(alpha, mid, zeta) 후 첫 항목 = 결정적 non-interactive 규칙.
        assert_eq!(picked.unwrap().db.as_deref(), Some("alpha.db"));
    }

    #[test]
    fn match_profile_pick_by_number_and_name() {
        let names = vec!["alpha".to_string(), "homelab".to_string(), "zeta".to_string()];
        assert_eq!(match_profile_pick("2", &names).unwrap(), "homelab");
        assert_eq!(match_profile_pick(" alpha \n", &names).unwrap(), "alpha");
        assert!(match_profile_pick("0", &names).is_err());
        assert!(match_profile_pick("99", &names).is_err());
        assert!(match_profile_pick("nope", &names).is_err());
        assert!(match_profile_pick("   ", &names).is_err());
    }

    #[test]
    fn expand_home_variants() {
        // HOME을 건드리는 다른 테스트(merge_profile_into_fills_unset_fields_from_profile)와
        // 병렬 실행 시 레이스가 나서 ENV_LOCK으로 직렬화한다(poison은 무시하고 계속 진행).
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let orig_home = std::env::var_os("HOME");
        let orig_userprofile = std::env::var_os("USERPROFILE");

        // HOME이 있으면 HOME을 쓴다.
        // 단일 스레드 가정 하 unsafe 사용(이 테스트 안에서만 mutate+복구).
        unsafe {
            std::env::set_var("HOME", "/home/tester");
            std::env::remove_var("USERPROFILE");
        }
        assert_eq!(expand_home("~/.tunaround/local.db"), "/home/tester/.tunaround/local.db");
        // ~/ 접두 없으면 그대로.
        assert_eq!(expand_home("/abs/path.db"), "/abs/path.db");

        // HOME 없고 USERPROFILE만 있으면 그걸로 폴백.
        unsafe {
            std::env::remove_var("HOME");
            std::env::set_var("USERPROFILE", "C:/Users/tester");
        }
        assert_eq!(expand_home("~/.tunaround/local.db"), "C:/Users/tester/.tunaround/local.db");

        // 둘 다 없으면 원본 그대로.
        unsafe {
            std::env::remove_var("HOME");
            std::env::remove_var("USERPROFILE");
        }
        assert_eq!(expand_home("~/.tunaround/local.db"), "~/.tunaround/local.db");

        // 원래 값 복구.
        unsafe {
            match orig_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
            match orig_userprofile {
                Some(v) => std::env::set_var("USERPROFILE", v),
                None => std::env::remove_var("USERPROFILE"),
            }
        }
    }

    #[test]
    fn resolve_search_token_prefers_plain_then_env_then_none() {
        // 유일한 변수명이라 다른 테스트와 충돌은 없지만, 일관성을 위해 동일 락을 쓴다.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let mut p = Profile::default();
        assert_eq!(resolve_search_token(&p), None);

        p.search_token_env = Some("TUNAROUND_TEST_TOKEN_CFG_XYZ".to_string());
        // 단일 스레드 가정 하 unsafe 사용(이 테스트 안에서만 env mutate+복구). 유일한 이름이라 교차 테스트 충돌 없음.
        unsafe {
            std::env::set_var("TUNAROUND_TEST_TOKEN_CFG_XYZ", "from-env");
        }
        assert_eq!(resolve_search_token(&p), Some("from-env".to_string()));

        p.search_token = Some("plain-wins".to_string());
        assert_eq!(resolve_search_token(&p), Some("plain-wins".to_string()));

        unsafe {
            std::env::remove_var("TUNAROUND_TEST_TOKEN_CFG_XYZ");
        }
    }

    #[test]
    fn first_existing_picks_first_present_path() {
        let dir = std::env::temp_dir();
        let present = dir.join("tunaround_test_first_existing_present.toml");
        std::fs::write(&present, "").unwrap();
        let missing = dir.join("tunaround_test_first_existing_missing_xyz.toml");
        let present_str = present.to_string_lossy().into_owned();
        let missing_str = missing.to_string_lossy().into_owned();

        // 없는 경로가 후보 목록에서 먼저 와도 실제 존재하는 쪽을 고른다.
        let result = first_existing(&[missing_str.clone(), present_str.clone()]);
        assert_eq!(result, Some(present_str.clone()));

        std::fs::remove_file(&present).unwrap();
        assert_eq!(first_existing(&[missing_str]), None);
    }

    #[test]
    fn discover_config_path_explicit_found_and_missing() {
        let dir = std::env::temp_dir();
        let path = dir.join("tunaround_test_config_explicit.toml");
        std::fs::write(&path, "default_profile = \"x\"\n").unwrap();
        let path_str = path.to_string_lossy().into_owned();

        let found = discover_config_path(Some(&path_str)).unwrap();
        assert_eq!(found, Some(path_str.clone()));

        std::fs::remove_file(&path).unwrap();
        let missing = discover_config_path(Some(&path_str));
        assert!(missing.is_err());
    }

    #[test]
    fn load_config_file_roundtrip_and_parse_error() {
        let dir = std::env::temp_dir();
        let path = dir.join("tunaround_test_load_config_file.toml");
        std::fs::write(&path, "default_profile = \"local\"\n[profile.local]\ndb = \"x.db\"\n").unwrap();
        let path_str = path.to_string_lossy().into_owned();

        let cfg = load_config_file(&path_str).expect("파싱 성공");
        assert_eq!(cfg.default_profile.as_deref(), Some("local"));
        assert_eq!(cfg.profile.get("local").unwrap().db.as_deref(), Some("x.db"));

        std::fs::remove_file(&path).unwrap();
        assert!(load_config_file(&path_str).is_err());
    }

    #[test]
    fn merge_profile_into_none_profile_is_noop() {
        let cli = MergedSessionArgs { db: Some("cli.db".to_string()), pull_context: true, ..Default::default() };
        let merged = merge_profile_into(cli.clone(), None);
        assert_eq!(merged, cli);
    }

    #[test]
    fn merge_profile_into_cli_wins_over_profile() {
        let cli = MergedSessionArgs {
            db: Some("cli.db".to_string()),
            roster: Some("cli-roster.json".to_string()),
            recent_turns: Some(3),
            pull_context: false,
            session: Some("cli-session".to_string()),
            search_url: Some("http://cli/mcp".to_string()),
            search_token: Some("cli-token".to_string()),
        };
        let profile = Profile {
            db: Some("profile.db".to_string()),
            roster: Some("profile-roster.json".to_string()),
            recent_turns: Some(99),
            pull_context: Some(true),
            session: Some("profile-session".to_string()),
            search_url: Some("http://profile/mcp".to_string()),
            search_token: Some("profile-token".to_string()),
            search_token_env: None,
        };
        let merged = merge_profile_into(cli, Some(&profile));
        assert_eq!(merged.db.as_deref(), Some("cli.db"));
        assert_eq!(merged.roster.as_deref(), Some("cli-roster.json"));
        assert_eq!(merged.recent_turns, Some(3));
        assert_eq!(merged.session.as_deref(), Some("cli-session"));
        assert_eq!(merged.search_url.as_deref(), Some("http://cli/mcp"));
        assert_eq!(merged.search_token.as_deref(), Some("cli-token"));
        // pull_context는 예외: CLI false + 프로파일 true = OR로 true.
        assert!(merged.pull_context);
    }

    #[test]
    fn merge_profile_into_fills_unset_fields_from_profile() {
        // HOME을 건드리는 expand_home_variants와 병렬 실행 시 레이스가 나서 ENV_LOCK으로 직렬화한다.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let cli = MergedSessionArgs::default();
        let profile = Profile {
            db: Some("~/.tunaround/homelab.db".to_string()),
            roster: Some("~/.tunaround/roster.json".to_string()),
            recent_turns: Some(20),
            pull_context: Some(true),
            session: Some("s1".to_string()),
            search_url: Some("http://profile/mcp".to_string()),
            search_token: None,
            search_token_env: Some("TUNAROUND_TEST_TOKEN_MERGE_XYZ".to_string()),
        };
        // 단일 스레드 가정 하 unsafe 사용. 유일한 이름이라 교차 테스트 충돌 없음.
        unsafe {
            std::env::set_var("TUNAROUND_TEST_TOKEN_MERGE_XYZ", "resolved-token");
        }
        let orig_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", "/home/tester");
        }

        let merged = merge_profile_into(cli, Some(&profile));

        assert_eq!(merged.db.as_deref(), Some("/home/tester/.tunaround/homelab.db"));
        assert_eq!(merged.roster.as_deref(), Some("/home/tester/.tunaround/roster.json"));
        assert_eq!(merged.recent_turns, Some(20));
        assert!(merged.pull_context);
        assert_eq!(merged.session.as_deref(), Some("s1"));
        assert_eq!(merged.search_url.as_deref(), Some("http://profile/mcp"));
        assert_eq!(merged.search_token.as_deref(), Some("resolved-token"));

        unsafe {
            std::env::remove_var("TUNAROUND_TEST_TOKEN_MERGE_XYZ");
            match orig_home {
                Some(v) => std::env::set_var("HOME", v),
                None => std::env::remove_var("HOME"),
            }
        }
    }

    #[test]
    fn merge_profile_into_pull_context_or_semantics() {
        // CLI true + 프로파일 미설정 => true 유지.
        let cli_true = MergedSessionArgs { pull_context: true, ..Default::default() };
        let profile_unset = Profile::default();
        assert!(merge_profile_into(cli_true, Some(&profile_unset)).pull_context);

        // CLI false + 프로파일 false => false.
        let cli_false = MergedSessionArgs { pull_context: false, ..Default::default() };
        let profile_false = Profile { pull_context: Some(false), ..Default::default() };
        assert!(!merge_profile_into(cli_false, Some(&profile_false)).pull_context);
    }
}
