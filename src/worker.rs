// A2A 워커 데몬: poll_tasks 텍스트 파싱 + poll->claim->runner.run->complete 루프.

use std::sync::Arc;
use std::time::Duration;

use crate::mcp_client::McpHttpClient;
use crate::runner::{RunInput, Runner};

// #138 B: 도메인별 서브모듈(순수 이동). parsing=task 파싱/디듑, guard=경로 라우팅·self-disruption
// 가드, run_loop=폴링 데몬 루프. 각 자식의 pub 항목은 아래 pub use로 기존 `crate::worker::X` 경로를
// 그대로 보존한다(외부 참조: cli_daemons.rs·cli_run.rs·codex_relay.rs·watch_results.rs).
mod guard;
mod parsing;
mod run_loop;

pub use guard::{
    context_map_disrupting_paths, parse_context_map, resolve_project_path, write_lane_disrupts_node,
};
pub use parsing::{ParsedTask, parse_open_tasks};
pub(crate) use run_loop::session_marker_terminated;
pub use run_loop::{generate_agent_uuid, needs_reregister, run_poll_loop, run_worker_loop};
// marker_gone은 원래 pub(crate) fn이라 test cfg 밖에서도 crate::worker::marker_gone 경로를 그대로
// 보존한다(현재 실사용은 tests.rs뿐이라 lib 단독 빌드에선 unused, 가시성 계약 보존이 우선).
#[allow(unused_imports)]
pub(crate) use run_loop::marker_gone;

// run_poll_loop(run_loop.rs)가 실행 시점에 쓰는 교차참조라 test cfg 밖에서도 필요하다.
use parsing::collect_new_submitted;

// 아래는 tests.rs(`use super::*;`)만 쓰는 교차참조라 test cfg 밖에서는 unused-imports가 뜬다(clippy
// -D warnings). 원래 private이던 항목이라 pub(super)로만 승격했고, 여기서도 pub use가 아닌 일반
// use로 받아 외부 공개 표면을 넓히지 않는다.
#[cfg(test)]
use guard::{normalize_lexically, paths_overlap};
#[cfg(test)]
use parsing::find_header_starts;
#[cfg(test)]
use run_loop::substitute_task_placeholders;

#[cfg(test)]
mod tests;
