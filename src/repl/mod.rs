// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.
use crate::orchestrator::{
    ContextMode, Participant, RoundInput, RunnerRegistry, Utterance, run_round,
};
use crate::runner::{RunError, RunMode};

mod command;
mod render;
mod session;

pub use command::{Command, parse_command};
pub use render::{StepOutcome, render};
pub use session::Session;

/// 이월 요약 최대 바이트 수. 초과 시 최근 드롭 턴 우선 유지 + 생략 표기.
const MAX_CARRY: usize = 1500;

#[cfg(test)]
mod tests;
