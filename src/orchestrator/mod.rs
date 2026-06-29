// 사람 주도 토론 오케스트레이터의 경계. 역할·프롬프트·라운드 구동.
pub mod roles;
pub mod prompt;

/// 토론 한 자리. 엔진(어떤 러너로 구동) + 역할 + 추가 지시.
#[derive(Debug, Clone)]
pub struct Participant {
    pub engine: String,
    pub role: Option<String>,
    pub instruction: String,
}

impl Participant {
    /// 전사 표기용 라벨(역할 있으면 "engine/role", 없으면 engine).
    pub fn label(&self) -> String {
        match &self.role {
            Some(r) => format!("{}/{}", self.engine, r),
            None => self.engine.clone(),
        }
    }
}

/// 한 발언. speaker=Participant.label(), content=응답 본문.
#[derive(Debug, Clone, PartialEq)]
pub struct Utterance {
    pub speaker: String,
    pub content: String,
}
