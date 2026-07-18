// write 모드 워커의 프로젝트 경로 라우팅과 self-disruption(node 실행 클론 훼손) 방지 가드.

/// task의 context_id를 `--context-map`에서 찾아 실행할 project-path를 정한다(순수 함수).
/// 매핑에 있으면 그 경로, 없거나 context_id가 없으면 기본 project-path로 폴백한다.
pub fn resolve_project_path(
    context_id: Option<&str>,
    context_map: &std::collections::HashMap<String, String>,
    default_path: Option<&str>,
) -> Option<String> {
    context_id
        .and_then(|c| context_map.get(c))
        .cloned()
        .or_else(|| default_path.map(|s| s.to_string()))
}

/// 두 경로가 겹치는지(같거나 한쪽이 다른 쪽의 조상) 판정한다(순수 함수, 파일시스템 접근 없음).
/// Path::starts_with는 컴포넌트 단위라 "/repo"와 "/repo2"를 오검출하지 않는다. write 워커의 작업
/// 디렉터리가 node 실행 클론과 겹치면 reset --hard 같은 write가 발밑을 갈아엎으므로, 그 판정의 핵심.
pub(super) fn paths_overlap(a: &std::path::Path, b: &std::path::Path) -> bool {
    a == b || a.starts_with(b) || b.starts_with(a)
}

/// 경로를 파일시스템 접근 없이 어휘적으로 절대·정규화한다(존재하지 않는 경로도 처리). 상대경로는 base에
/// 이어붙이고, `.`는 버리고 `..`는 직전 컴포넌트를 pop한다. canonicalize가 실패하는(=아직 없는) 경로의
/// overlap 판정 폴백으로 쓴다. 심볼릭 링크는 해석하지 않으므로 canonical과 완전 동치는 아니나, cwd 하위
/// 여부를 보수적으로 보는 데는 충분하다.
pub(super) fn normalize_lexically(
    p: &std::path::Path,
    base: &std::path::Path,
) -> std::path::PathBuf {
    use std::path::{Component, PathBuf};
    let combined: PathBuf = if p.is_absolute() {
        p.to_path_buf()
    } else {
        base.join(p)
    };
    let mut out = PathBuf::new();
    for comp in combined.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

/// write 모드 워커가 node 자신이 도는 클론을 갈아엎을(self-disruption) 위험이 있는지 판정한다.
/// project=None이면 러너가 node 실행 디렉터리(cwd)에서 돌아 위험(true). Some(p)이면 cwd와 겹치면
/// (같거나 한쪽이 조상) 위험. read-only 워커엔 호출하지 않는다(쓰기가 없어 무해). 2026-07-03 뱃지 task
/// self-disruption을 구조적으로 막는다.
///
/// 존재하는 경로는 canonical끼리 비교하고, 아직 없는 경로(canonicalize 실패)는 어휘 정규화로 폴백
/// 판정한다(gemini 리뷰: 러너가 실행 중 cwd 하위에 그 경로를 생성하면 뒤늦게 self-disruption 여지 -
/// 보수적으로 미리 겹침으로 본다).
pub fn write_lane_disrupts_node(
    project: Option<&std::path::Path>,
    node_cwd: &std::path::Path,
) -> bool {
    let Some(p) = project else {
        return true; // 작업 디렉터리 미지정 = node cwd에서 write = 위험.
    };
    match std::fs::canonicalize(p) {
        Ok(pc) => {
            let cwd = std::fs::canonicalize(node_cwd).unwrap_or_else(|_| node_cwd.to_path_buf());
            paths_overlap(&pc, &cwd)
        }
        Err(_) => {
            // 아직 없는 경로: canonical 대신 어휘 정규화로 양쪽을 같은 형태로 만들어 겹침을 본다
            // (Windows verbatim `\\?\` 접두 불일치를 피하려 cwd도 canonical 대신 어휘 정규화).
            let p_lex = normalize_lexically(p, node_cwd);
            let cwd_lex = normalize_lexically(node_cwd, node_cwd);
            // Windows는 경로 대소문자를 구분하지 않으므로(d:\... vs D:\...), 어휘 정규화 폴백에서만
            // 소문자로 맞춰 비교한다(canonicalize 성공 경로는 OS가 이미 실제 대소문자로 정규화해 주므로
            // 이 폴백과 무관). 그러지 않으면 대소문자만 다른 겹침 경로를 가드가 못 잡는다.
            #[cfg(windows)]
            {
                paths_overlap(&lowercase_path(&p_lex), &lowercase_path(&cwd_lex))
            }
            #[cfg(not(windows))]
            {
                paths_overlap(&p_lex, &cwd_lex)
            }
        }
    }
}

/// Windows 대소문자 무구분 경로 비교용 소문자 정규화(cfg(windows) 전용, write_lane_disrupts_node의
/// 어휘 정규화 폴백에서만 쓰인다).
#[cfg(windows)]
fn lowercase_path(p: &std::path::Path) -> std::path::PathBuf {
    std::path::PathBuf::from(p.to_string_lossy().to_lowercase())
}

/// write 모드 워커가 `--context-map`으로 매핑된 경로들 중 node_cwd와 겹치는(self-disruption) 것을
/// 찾는다(순수 함수). 기본 --project-path만 검사하던 write_lane_disrupts_node 가드는 context_id가
/// --context-map에 매핑돼 있을 때(resolve_project_path가 그 경로로 실행을 돌림) 우회됐다. 반환값은
/// "key=value" 표시용 문자열 목록(정렬, 빈 목록=안전).
pub fn context_map_disrupting_paths(
    context_map: &std::collections::HashMap<String, String>,
    node_cwd: &std::path::Path,
) -> Vec<String> {
    let mut bad: Vec<String> = context_map
        .iter()
        .filter(|(_, v)| write_lane_disrupts_node(Some(std::path::Path::new(v.as_str())), node_cwd))
        .map(|(k, v)| format!("{k}={v}"))
        .collect();
    bad.sort();
    bad
}

/// `--context-map` 문자열("k=v,k=v")을 context_id->project-path 맵으로 파싱한다(순수 함수).
/// 형식 오류(= 없음)·빈 key·빈 value·중복 key는 조용히 버리지 않고 Err로 거부한다. 오타 항목이
/// 조용히 사라져 기본 project-path로 폴백되면 --write 시 엉뚱한 레포를 고칠 수 있어서다. 완전히 빈
/// 항목(후행 콤마 등)만 무해하게 건너뛴다.
pub fn parse_context_map(spec: &str) -> Result<std::collections::HashMap<String, String>, String> {
    let mut map = std::collections::HashMap::new();
    for entry in spec.split(',') {
        let entry = entry.trim();
        if entry.is_empty() {
            continue;
        }
        let (k, v) = entry.split_once('=').ok_or_else(|| {
            format!("--context-map 항목이 'key=value' 형식이 아닙니다: {entry:?}")
        })?;
        let (k, v) = (k.trim(), v.trim());
        if k.is_empty() || v.is_empty() {
            return Err(format!(
                "--context-map 항목의 key 또는 value가 비어있습니다: {entry:?}"
            ));
        }
        if let Some(prev) = map.insert(k.to_string(), v.to_string()) {
            return Err(format!(
                "--context-map에 중복 key '{k}'가 있습니다(이전 값 {prev:?})"
            ));
        }
    }
    Ok(map)
}
