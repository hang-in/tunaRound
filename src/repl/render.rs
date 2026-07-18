// 발언 목록과 step 결과를 터미널 표시용 문자열로 렌더링한다.

use crate::orchestrator::Utterance;

/// step 결과. I/O(출력·파일쓰기·종료)는 main이 수행한다.
#[derive(Debug)]
pub enum StepOutcome {
    Print(String),
    Save { path: String, markdown: String },
    Exit,
    Noop,
}

/// 한 발언 목록을 터미널 표시용 문자열로.
/// 큐레이션 abstraction(v2-51)이 있으면(검색 결과 등) 원문 앞에 증류 요약을 표면화한다.
/// 라운드 출력 발언은 abstraction=None이라 기존 표시 동작 불변.
pub fn render(round: &[Utterance]) -> String {
    round
        .iter()
        .map(|u| {
            let body = match &u.abstraction {
                Some(a) if !a.trim().is_empty() => format!("[요약] {}\n{}", a.trim(), u.content),
                _ => u.content.clone(),
            };
            format!("## {}\n{}", u.speaker, body)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}
