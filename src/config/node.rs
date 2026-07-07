// node.toml 워커 노드 설정(브로커 + 자동/감독 레인)의 파싱·검증·탐색. 세션 프로파일(config.rs)과 별개 도메인.

use serde::Deserialize;

// 경로 확장·후보 선택은 부모 모듈(config)의 공유 유틸을 재사용한다(자식 모듈은 부모의 private 항목 접근 가능).
use super::{expand_home, first_existing};

fn default_node_core() -> String {
    "self".to_string()
}
fn default_lane_runner() -> String {
    "claude".to_string()
}
fn default_lane_interval() -> u64 {
    20
}

/// node.toml 최상위. 한 머신을 A2A 워커 노드로 만드는 설정(브로커 + 자동 레인들).
#[derive(Debug, Clone, Deserialize)]
pub struct NodeConfig {
    /// "self"(이 머신이 브로커 호스팅) 또는 코어 `/mcp` URL.
    #[serde(default = "default_node_core")]
    pub core: String,
    /// core="self"일 때 브로커 바인드 주소(예: 0.0.0.0:8770).
    #[serde(default)]
    pub listen: Option<String>,
    /// bearer 토큰. "@env:NAME"이면 환경변수 참조(레포·설정에 평문 노출 회피).
    #[serde(default)]
    pub token: Option<String>,
    /// core="self"일 때 브로커 db 경로.
    #[serde(default)]
    pub db: Option<String>,
    /// 레인들(자동=헤드리스 워커 데몬, kind="supervised"=세션 부착 감독).
    #[serde(default)]
    pub lane: Vec<Lane>,
}

/// 레인 하나. work 서브커맨드 옵션을 config로 투영한 것.
#[derive(Debug, Clone, Deserialize)]
pub struct Lane {
    /// 이 레인의 agent id(이 앞 task만 처리).
    pub agent: String,
    /// 러너 종류(claude|codex|opencode|http|a2a).
    #[serde(default = "default_lane_runner")]
    pub runner: String,
    /// "read-only"(기본) | "write".
    #[serde(default)]
    pub mode: Option<String>,
    /// 러너 작업 디렉터리.
    #[serde(default)]
    pub project: Option<String>,
    /// 러너 모델(옵션).
    #[serde(default)]
    pub model: Option<String>,
    /// poll 간격 초(기본 20).
    #[serde(default = "default_lane_interval")]
    pub interval: u64,
    /// "supervised"면 세션 부착 감독 레인(node는 watcher 명령만 안내, 데몬화 안 함). 미지정=자동.
    #[serde(default)]
    pub kind: Option<String>,
    /// context_id -> project-path 매핑(work의 --context-map과 동일 형식).
    #[serde(default)]
    pub context_map: Option<String>,
    /// 로스터 발견용 태그(work의 --tags와 동일 형식 "k=v,k=v"). dispatcher가 to_selector로 이 레인
    /// 워커를 발견한다. 미지정이면 빈 태그로 등록(uuid/exact-id로만 라우팅 가능).
    #[serde(default)]
    pub tags: Option<String>,
    /// runner="http" 전용 base URL.
    #[serde(default)]
    pub http_base_url: Option<String>,
    /// runner="a2a" 전용 카드 URL.
    #[serde(default)]
    pub a2a_card: Option<String>,
    /// runner="a2a" 전용 외부 토큰.
    #[serde(default)]
    pub a2a_token: Option<String>,
}

impl Lane {
    /// 감독 레인(세션 부착 대상)인가. kind가 "supervised"면 true, 미지정 또는 "auto"면 false(자동 레인).
    pub fn is_supervised(&self) -> bool {
        self.kind.as_deref() == Some("supervised")
    }
    /// write 모드 여부(mode="write"만 true, 그 외/미지정=read-only).
    pub fn is_write(&self) -> bool {
        self.mode.as_deref() == Some("write")
    }
}

/// 토큰 문자열을 해석한다. "@env:NAME"이면 그 환경변수를, 아니면 평문을 그대로 쓴다. None이면 None.
pub fn resolve_node_token(token: Option<&str>) -> Option<String> {
    let raw = token?;
    match raw.strip_prefix("@env:") {
        Some(var) => std::env::var(var).ok(),
        None => Some(raw.to_string()),
    }
}

/// NodeConfig 의미 검증. 특히 `kind` 오타를 거부한다: 알 수 없는 값이 조용히 자동 레인으로
/// 강등되면(mode="write"와 겹칠 때) 사람 승인 게이트를 우회하므로, 모호하면 실행 대신 거부한다(fail-safe).
fn validate_node_config(cfg: &NodeConfig) -> Result<(), String> {
    for l in &cfg.lane {
        if let Some(k) = &l.kind
            && k != "supervised"
            && k != "auto"
        {
            return Err(format!(
                "lane '{}'의 kind '{}'가 유효하지 않습니다(auto|supervised만 허용). \
                 오타가 감독 레인을 자동 실행으로 강등시키는 사고를 막기 위해 거부합니다.",
                l.agent, k
            ));
        }
    }
    Ok(())
}

/// TOML 문자열을 NodeConfig로 파싱하고 의미 검증한다.
pub fn parse_node_config(text: &str) -> Result<NodeConfig, String> {
    let cfg: NodeConfig = toml::from_str(text).map_err(|e| format!("node 설정 파싱 실패: {e}"))?;
    validate_node_config(&cfg)?;
    Ok(cfg)
}

/// node.toml 탐색. 우선순위: 명시(`--config`) > `./tunaround.node.toml` > `~/.tunaround/node.toml`.
pub fn discover_node_config_path(explicit: Option<&str>) -> Result<Option<String>, String> {
    if let Some(p) = explicit {
        return if std::path::Path::new(p).is_file() {
            Ok(Some(p.to_string()))
        } else {
            Err(format!("node 설정 파일을 찾을 수 없습니다: {p}"))
        };
    }
    let candidates =
        vec!["tunaround.node.toml".to_string(), expand_home("~/.tunaround/node.toml")];
    Ok(first_existing(&candidates))
}

/// node 설정을 읽는다. 세션 프로파일과 달리 node는 설정이 필수라, 못 찾으면 안내 에러를 반환한다.
pub fn load_node_config(explicit: Option<&str>) -> Result<NodeConfig, String> {
    match discover_node_config_path(explicit)? {
        Some(path) => {
            let text = std::fs::read_to_string(&path)
                .map_err(|e| format!("node 설정 읽기 실패 ({path}): {e}"))?;
            parse_node_config(&text)
        }
        None => Err("node 설정이 없습니다. `tunaround init`으로 생성하거나 --config로 지정하세요.".to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // 부모 모듈(config)의 공용 ENV_LOCK을 공유해 profile 테스트와 함께 직렬화한다(set_var 전역 UB 방지, gemini 지적).
    use crate::config::ENV_LOCK;

    #[test]
    fn parse_node_config_full_with_lanes_and_defaults() {
        let toml_text = r#"
core = "self"
listen = "0.0.0.0:8770"
token = "@env:TUNAROUND_TOKEN"
db = "~/.tunaround/broker.db"

[[lane]]
agent = "mac-worker"
runner = "codex"
mode = "write"
project = "~/repos/x"
tags = "machine=mac,runner=codex,role=worker"

[[lane]]
agent = "mac-claude"
kind = "supervised"
"#;
        let cfg = parse_node_config(toml_text).expect("파싱 성공");
        assert_eq!(cfg.core, "self");
        assert_eq!(cfg.listen.as_deref(), Some("0.0.0.0:8770"));
        assert_eq!(cfg.lane.len(), 2);

        let auto = &cfg.lane[0];
        assert_eq!(auto.agent, "mac-worker");
        assert_eq!(auto.runner, "codex");
        assert!(auto.is_write());
        assert!(!auto.is_supervised());
        assert_eq!(auto.interval, 20, "interval 기본 20");
        assert_eq!(auto.tags.as_deref(), Some("machine=mac,runner=codex,role=worker"), "lane 태그 파싱");

        let sup = &cfg.lane[1];
        assert!(sup.is_supervised());
        assert!(!sup.is_write(), "mode 미지정 = read-only");
        assert_eq!(sup.runner, "claude", "runner 기본 claude");
        assert_eq!(sup.tags, None, "tags 미지정 = None");
    }

    #[test]
    fn parse_node_config_core_defaults_to_self() {
        // core 미지정이면 "self"(이 머신이 브로커).
        let cfg = parse_node_config("[[lane]]\nagent = \"w\"\n").expect("파싱 성공");
        assert_eq!(cfg.core, "self");
        assert_eq!(cfg.lane.len(), 1);
        assert_eq!(cfg.lane[0].agent, "w");
    }

    #[test]
    fn parse_node_config_missing_agent_errors() {
        // lane에 agent 없으면 파싱 에러(필수 필드).
        assert!(parse_node_config("[[lane]]\nrunner = \"claude\"\n").is_err());
    }

    #[test]
    fn parse_node_config_rejects_unknown_kind() {
        // 오타 kind는 거부(감독 레인이 조용히 자동 실행으로 강등되는 사고 방지, coderabbit 보안 지적).
        let err = parse_node_config("[[lane]]\nagent = \"w\"\nkind = \"supervized\"\n").unwrap_err();
        assert!(err.contains("kind"), "에러 메시지: {err}");
        // 정상 값은 통과.
        assert!(parse_node_config("[[lane]]\nagent = \"w\"\nkind = \"auto\"\n").is_ok());
        assert!(parse_node_config("[[lane]]\nagent = \"w\"\nkind = \"supervised\"\n").is_ok());
    }

    #[test]
    fn resolve_node_token_env_plain_and_none() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        assert_eq!(resolve_node_token(None), None);
        assert_eq!(resolve_node_token(Some("plain-token")), Some("plain-token".to_string()));

        unsafe {
            std::env::set_var("TUNAROUND_TEST_NODE_TOK_XYZ", "from-env-node");
        }
        assert_eq!(
            resolve_node_token(Some("@env:TUNAROUND_TEST_NODE_TOK_XYZ")),
            Some("from-env-node".to_string())
        );
        // 없는 env는 None(설정은 됐으나 값 없음).
        assert_eq!(resolve_node_token(Some("@env:TUNAROUND_TEST_NODE_TOK_ABSENT")), None);
        unsafe {
            std::env::remove_var("TUNAROUND_TEST_NODE_TOK_XYZ");
        }
    }
}
