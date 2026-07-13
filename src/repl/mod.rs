// 터미널 REPL. 명령 파싱·렌더·세션 step. I/O는 main.rs.
use crate::orchestrator::{
    ContextMode, Participant, RoundInput, RunnerRegistry, Utterance, run_round,
};
use crate::runner::{RunError, RunMode};

/// 이월 요약 최대 바이트 수. 초과 시 최근 드롭 턴 우선 유지 + 생략 표기.
const MAX_CARRY: usize = 1500;

/// 발언 내용에서 첫 절을 추출한다. 첫 문장 종결('.', '。', 개행) 또는 첫 ~80자 중 짧은 쪽.
fn first_clause(content: &str) -> String {
    // 첫 문장 종결 위치(바이트 오프셋 + 문자 바이트 크기).
    let sentence_end = content
        .char_indices()
        .find(|(_, c)| *c == '.' || *c == '\u{3002}' || *c == '\n')
        .map(|(i, c)| i + c.len_utf8());
    let sentence = match sentence_end {
        Some(end) => &content[..end],
        None => content,
    };
    // 첫 ~80자(한국어 포함 문자 단위).
    let eighty: String = content.chars().take(80).collect();
    // 문자 수 기준 짧은 쪽 선택.
    if sentence.chars().count() <= eighty.chars().count() {
        sentence.trim().to_string()
    } else {
        eighty.trim().to_string()
    }
}

/// REPL 한 줄 입력의 해석 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Message(String),
    Save(Option<String>),
    Conclude(Option<String>),
    Only {
        engine: String,
        text: String,
    },
    Write {
        engine: String,
        text: String,
    },
    Debate {
        turns: usize,
        topic: String,
    },
    Search(String),
    /// 검색 디버그: 질의→토큰화→히트 bm25/유효성 표시.
    Explain(String),
    Branches,
    Checkout(u64),
    /// 발언을 superseded로 표시(선택적으로 대체 발언 id). 유효성 지정(HITL).
    Supersede {
        id: u64,
        by: Option<u64>,
    },
    /// 발언을 rejected로 표시(검색에서 제외).
    Reject(u64),
    /// 발언에 큐레이션(증류 요약 abstraction·검색 앵커 anchors)을 남긴다(둘 중 하나만도 허용).
    Annotate {
        id: u64,
        abstraction: Option<String>,
        anchors: Option<String>,
    },
    Help,
    Quit,
    Noop,
}

/// 큰따옴표를 존중해 공백 분리 토큰을 만든다(/annotate 인자 파싱용). 따옴표 안 공백은 보존한다.
/// 여는 따옴표는 값 경계로만 쓰고 토큰에는 포함하지 않는다. 빈 따옴표("")는 빈 토큰을 만든다.
fn split_quoted(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut has_token = false;
    for c in s.chars() {
        if c == '"' {
            in_quote = !in_quote;
            has_token = true; // "" 도 토큰으로 인정.
        } else if c.is_whitespace() && !in_quote {
            if has_token {
                out.push(std::mem::take(&mut cur));
                has_token = false;
            }
        } else {
            cur.push(c);
            has_token = true;
        }
    }
    if has_token {
        out.push(cur);
    }
    out
}

/// 한 줄을 명령으로 파싱한다. `/`로 시작하면 명령, 아니면 메시지, 공백이면 Noop.
pub fn parse_command(line: &str) -> Command {
    let line = line.trim();
    if line.is_empty() {
        return Command::Noop;
    }
    if let Some(rest) = line.strip_prefix('/') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let name = it.next().unwrap_or("");
        let arg = it.next().map(|s| s.trim().to_string());
        return match name {
            "quit" | "exit" | "q" => Command::Quit,
            "help" | "h" => Command::Help,
            "save" => Command::Save(arg.filter(|s| !s.is_empty())),
            "conclude" => Command::Conclude(arg.filter(|s| !s.is_empty())),
            "search" => match arg.filter(|s| !s.is_empty()) {
                Some(q) => Command::Search(q),
                None => Command::Message(line.to_string()),
            },
            "explain" => match arg.filter(|s| !s.is_empty()) {
                Some(q) => Command::Explain(q),
                None => Command::Message(line.to_string()),
            },
            "branches" | "tree" => Command::Branches,
            "checkout" | "co" => match arg.as_deref().and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Checkout(id),
                None => Command::Message(line.to_string()),
            },
            "supersede" => {
                // /supersede <id> [<by_id>]
                let mut toks = arg.as_deref().unwrap_or("").split_whitespace();
                match toks.next().and_then(|t| t.parse::<u64>().ok()) {
                    Some(id) => {
                        let by = toks.next().and_then(|t| t.parse::<u64>().ok());
                        Command::Supersede { id, by }
                    }
                    None => Command::Message(line.to_string()),
                }
            }
            "reject" => match arg.as_deref().and_then(|a| a.trim().parse::<u64>().ok()) {
                Some(id) => Command::Reject(id),
                None => Command::Message(line.to_string()),
            },
            "annotate" => {
                // /annotate <id> --abstraction "요약" --anchors "k1,k2" (둘 중 하나만도 허용).
                let toks = split_quoted(arg.as_deref().unwrap_or(""));
                match toks.first().and_then(|t| t.parse::<u64>().ok()) {
                    None => Command::Message(line.to_string()),
                    Some(id) => {
                        let mut abstraction = None;
                        let mut anchors = None;
                        // 플래그 값으로 소비할 다음 토큰. 다음 토큰이 `--`로 시작하면(=다음 플래그) 값이
                        // 없는 것으로 보고 삼키지 않는다(예 `--abstraction --anchors "x"`, CodeRabbit).
                        let take_value = |toks: &[String], i: usize| -> Option<String> {
                            toks.get(i + 1)
                                .filter(|s| !s.is_empty() && !s.starts_with("--"))
                                .cloned()
                        };
                        let mut i = 1;
                        while i < toks.len() {
                            match toks[i].as_str() {
                                "--abstraction" => {
                                    let v = take_value(&toks, i);
                                    let consumed = v.is_some();
                                    abstraction = v;
                                    i += if consumed { 2 } else { 1 };
                                }
                                "--anchors" => {
                                    let v = take_value(&toks, i);
                                    let consumed = v.is_some();
                                    anchors = v;
                                    i += if consumed { 2 } else { 1 };
                                }
                                _ => i += 1,
                            }
                        }
                        // 둘 다 비면 잘못된 사용 → 일반 메시지로 폴스루(기존 명령 패턴 답습).
                        if abstraction.is_none() && anchors.is_none() {
                            Command::Message(line.to_string())
                        } else {
                            Command::Annotate {
                                id,
                                abstraction,
                                anchors,
                            }
                        }
                    }
                }
            }
            "debate" => {
                const DEFAULT_TURNS: usize = 3;
                const MAX_TURNS: usize = 10;
                match arg.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
                    None => Command::Message(line.to_string()), // 주제 없음
                    Some(rest) => {
                        // 첫 토큰이 숫자면 turns, 나머지가 topic. 아니면 전체가 topic(기본 turns).
                        let mut it = rest.splitn(2, char::is_whitespace);
                        let first = it.next().unwrap_or("");
                        match first.parse::<usize>() {
                            Ok(n) => {
                                let topic =
                                    it.next().map(|s| s.trim().to_string()).unwrap_or_default();
                                if topic.is_empty() {
                                    Command::Message(line.to_string()) // 숫자만, 주제 없음
                                } else {
                                    Command::Debate {
                                        turns: n.clamp(1, MAX_TURNS),
                                        topic,
                                    }
                                }
                            }
                            Err(_) => Command::Debate {
                                turns: DEFAULT_TURNS,
                                topic: rest.to_string(),
                            },
                        }
                    }
                }
            }
            _ => Command::Message(line.to_string()),
        };
    }
    if let Some(rest) = line.strip_prefix('@') {
        let mut it = rest.splitn(2, char::is_whitespace);
        let mut engine = it.next().unwrap_or("").to_string();
        let text = it.next().map(|s| s.trim().to_string()).unwrap_or_default();
        let write = engine.ends_with('!');
        if write {
            engine.pop(); // trailing '!' 제거
        }
        if !engine.is_empty() && !text.is_empty() {
            return if write {
                Command::Write { engine, text }
            } else {
                Command::Only { engine, text }
            };
        }
        return Command::Message(line.to_string()); // "@codex"·"@codex!"만이면 일반 메시지
    }
    Command::Message(line.to_string())
}

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

const DEFAULT_SAVE_PATH: &str = "tunaround-discussion.md";

/// retrieve_for가 검색 시 끌어올 최대 슬라이스 수.
const RETRIEVE_K: usize = 5;

/// retrieved(검색) 주입의 누적 글자수 상한. 초과분 발언은 드롭(프롬프트 팽창 방지, carried MAX_CARRY 답습).
const MAX_RETRIEVED_CHARS: usize = 2000;

/// step이 active_path를 한 번만 계산해 파생하는 라운드 맥락 묶음.
struct RoundContext {
    /// run_round에 넘길 prior 슬라이스(recent_turns 적용 후).
    prior: Vec<Utterance>,
    /// 드롭된 옛 턴의 압축 요약.
    carried: String,
    /// 활성 경로 전체 발언 수(포인터 힌트용).
    transcript_len: usize,
    /// retrieve_for dedup 전용: 전체 활성 경로.
    full_path: Vec<Utterance>,
}

/// 한 토론 세션. 참가자 + 대화 트리 스냅샷(ConversationSnapshot) + 러너 레지스트리를 보유한다.
/// v2-52 ⑤: 과거 messages:Vec<StoredMessage>+head 직보유를 중립 snapshot으로 대체(스키마 누수 차단).
pub struct Session {
    participants: Vec<Participant>,
    snapshot: crate::types::ConversationSnapshot,
    registry: Box<dyn RunnerRegistry>,
    session_id: String,
    indexer: Option<Box<dyn crate::store::indexer::MessageIndexer>>,
    retriever: Option<Box<dyn crate::orchestrator::ContextRetriever>>,
    recent_turns: Option<usize>,
    /// 컨텍스트 전달 모드. 기본 Push(=현행 동작 불변), Pull은 --pull-context 플래그로 활성화.
    context_mode: ContextMode,
    /// front=core 병합 경계(Plan 27 옵션 B). Some이면 DB가 권위: 매 라운드 adopt + append_turn 쓰기.
    /// None(기본)이면 기존 인메모리 트리 + indexer 전량 persist(동작 불변).
    core_sync: Option<Box<dyn crate::orchestrator::CoreSync>>,
    /// 유효성 지정 sink(/supersede·/reject, step 5). None(--db 없음)이면 안내만.
    validity_sink: Option<Box<dyn crate::orchestrator::ValiditySink>>,
    /// 큐레이션 지정 sink(/annotate, v2-51). None(--db 없음)이면 안내만.
    annotation_sink: Option<Box<dyn crate::orchestrator::AnnotationSink>>,
}

impl Session {
    pub fn new(participants: Vec<Participant>, registry: Box<dyn RunnerRegistry>) -> Self {
        Self {
            participants,
            snapshot: crate::types::ConversationSnapshot::new(),
            registry,
            session_id: "default".to_string(),
            indexer: None,
            retriever: None,
            recent_turns: None,
            core_sync: None,
            validity_sink: None,
            annotation_sink: None,
            context_mode: ContextMode::Push,
        }
    }

    /// indexer 배선 생성자. SQLite 색인 활성화용.
    pub fn new_with_indexer(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        session_id: String,
        indexer: Option<Box<dyn crate::store::indexer::MessageIndexer>>,
    ) -> Self {
        Self {
            participants,
            snapshot: crate::types::ConversationSnapshot::new(),
            registry,
            session_id,
            indexer,
            retriever: None,
            recent_turns: None,
            core_sync: None,
            validity_sink: None,
            annotation_sink: None,
            context_mode: ContextMode::Push,
        }
    }

    /// retriever를 설정하는 빌더 메서드(단일 적용, self를 소비 후 반환).
    pub fn with_retriever(
        mut self,
        retriever: Option<Box<dyn crate::orchestrator::ContextRetriever>>,
    ) -> Self {
        self.retriever = retriever;
        self
    }

    /// recent_turns를 설정하는 빌더 메서드(None=기본, 현행 통째 재주입 유지).
    pub fn with_recent_turns(mut self, n: Option<usize>) -> Self {
        self.recent_turns = n;
        self
    }

    /// context_mode를 설정하는 빌더 메서드. Pull이면 MCP 가능 좌석에 포인터 프롬프트를 사용한다.
    pub fn with_context_mode(mut self, m: ContextMode) -> Self {
        self.context_mode = m;
        self
    }

    /// front=core 병합 경계를 설정하는 빌더 메서드(--core 전용). None=기존 동작 불변.
    pub fn with_core_sync(mut self, cs: Option<Box<dyn crate::orchestrator::CoreSync>>) -> Self {
        self.core_sync = cs;
        self
    }

    /// 유효성 지정 sink를 설정하는 빌더 메서드(--db 시 배선). None이면 /supersede·/reject 안내만.
    pub fn with_validity_sink(
        mut self,
        sink: Option<Box<dyn crate::orchestrator::ValiditySink>>,
    ) -> Self {
        self.validity_sink = sink;
        self
    }

    /// 큐레이션 지정 sink를 설정하는 빌더 메서드(--db 시 배선). None이면 /annotate 안내만.
    pub fn with_annotation_sink(
        mut self,
        sink: Option<Box<dyn crate::orchestrator::AnnotationSink>>,
    ) -> Self {
        self.annotation_sink = sink;
        self
    }

    /// core-sync 모드에서 코어 DB(권위)의 최신 트리를 인메모리에 채택한다(외부 post_turn 흡수).
    /// core_sync 미연결이면 no-op(기존 동작 불변). DB에 세션 없으면 그대로 둔다.
    fn adopt_from_core(&mut self) {
        let Some(cs) = &self.core_sync else { return };
        if let Some(snap) = cs.load_session(&self.session_id) {
            self.snapshot = snap;
        }
    }

    /// recent_turns를 적용해 prior 슬라이스를 반환한다. path를 받으므로 active_path 재호출 없음.
    fn prior_from_path(&self, path: Vec<Utterance>) -> Vec<Utterance> {
        match self.recent_turns {
            Some(n) if path.len() > n => path[path.len() - n..].to_vec(),
            _ => path,
        }
    }

    /// run_round에 넘길 prior 슬라이스. recent_turns Some(n)이면 활성 경로 마지막 n턴만, 아니면 전체.
    pub fn prior_for_prompt(&self) -> Vec<Utterance> {
        let path = self.active_path();
        self.prior_from_path(path)
    }

    /// carry_forward_digest의 path 파라미터 버전. active_path 재호출 없이 호출 가능.
    fn carry_forward_digest_from_path(&self, path: &[Utterance]) -> String {
        let dropped: &[Utterance] = match self.context_mode {
            // Pull: prior를 통째로 안 넣으니 전사 전체가 요약 대상이다.
            ContextMode::Pull => path,
            // Push: recent_turns 밖만 드롭. 미캡(None)이거나 path<=n이면 드롭 없음.
            ContextMode::Push => match self.recent_turns {
                Some(n) if path.len() > n => &path[..path.len() - n],
                _ => return String::new(),
            },
        };
        if dropped.is_empty() {
            return String::new();
        }
        // 드롭 턴마다 한 줄: "- [speaker] {first_clause}".
        let lines: Vec<String> = dropped
            .iter()
            .map(|u| format!("- [{}] {}", u.speaker, first_clause(&u.content)))
            .collect();

        let full = lines.join("\n");
        if full.len() <= MAX_CARRY {
            return full;
        }

        // 최근 드롭 턴 우선으로 예산 안에 유지. 마커 여유분 50바이트 확보.
        const MARKER_RESERVE: usize = 50;
        let budget = MAX_CARRY.saturating_sub(MARKER_RESERVE);
        let mut kept: Vec<&str> = Vec::new();
        let mut used: usize = 0;
        for line in lines.iter().rev() {
            let extra = if kept.is_empty() {
                line.len()
            } else {
                1 + line.len()
            };
            if used + extra > budget {
                break;
            }
            kept.push(line.as_str());
            used += extra;
        }
        kept.reverse();

        let omitted = lines.len() - kept.len();
        let marker = format!("(이전 {}턴 생략)", omitted);
        if kept.is_empty() {
            marker
        } else {
            let body = kept.join("\n");
            let result = format!("{}\n{}", marker, body);
            // 최종 바이트 캡 안전망.
            if result.len() <= MAX_CARRY {
                result
            } else {
                marker
            }
        }
    }

    /// 프롬프트에서 빠진 전사를 결정적 압축 요약으로 반환한다(LLM·임베더 미사용).
    /// Pull 모드: 전사 전체가 프롬프트에서 빠지므로 전체를 요약한다(안전망, MAX_CARRY로 평평하게 캡).
    /// Push 모드: recent_turns 밖으로 드롭된 옛 턴만 요약(None이거나 path<=n이면 드롭 없음 → 빈 문자열).
    /// 초과 시 최근 턴 우선 유지, 맨 앞에 "(이전 N턴 생략)" 표기.
    pub fn carry_forward_digest(&self) -> String {
        let path = self.active_path();
        self.carry_forward_digest_from_path(&path)
    }

    /// step에서 active_path를 한 번만 계산해 prior·carried·transcript_len을 파생하는 헬퍼.
    fn round_context(&self) -> RoundContext {
        let full_path = self.active_path(); // 유일한 active_path 호출.
        let carried = self.carry_forward_digest_from_path(&full_path);
        let transcript_len = full_path.len();
        let prior = self.prior_from_path(full_path.clone());
        RoundContext {
            prior,
            carried,
            transcript_len,
            full_path,
        }
    }

    /// retrieve_for의 path 파라미터 버전. active_path 재호출 없이 호출 가능.
    fn retrieve_for_from_path(&self, topic: &str, active: &[Utterance]) -> Vec<Utterance> {
        let Some(r) = &self.retriever else {
            return Vec::new();
        };
        // retrieve_ctx Err(1차 검색 경로 DB 장애, R7)는 "조용히 무시"가 아니라 "보이게 무시": stderr로 알리고
        // 빈 컨텍스트로 degrade한다(REPL은 계속 진행). 활성 경로 중복은 제외(분기 인지 디프리오리티).
        let hits = match r.retrieve_ctx(topic, RETRIEVE_K, &self.session_id) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("[tunaRound] 맥락 검색 실패(빈 컨텍스트로 진행): {e}");
                return Vec::new();
            }
        };
        let deduped = hits
            .into_iter()
            .filter(|u| !active.iter().any(|a| a.content == u.content));
        // 누적 글자수가 MAX_RETRIEVED_CHARS를 넘으면 이후 발언 드롭(최소 1건은 보장, UTF-8 안전).
        let mut used = 0usize;
        let mut out: Vec<Utterance> = Vec::new();
        for u in deduped {
            let len = u.content.chars().count();
            if used + len > MAX_RETRIEVED_CHARS && !out.is_empty() {
                break;
            }
            used += len;
            out.push(u);
        }
        out
    }

    /// topic으로 retriever를 검색하고 활성 경로 중복을 제외한 슬라이스를 반환한다.
    /// retriever 없으면 빈 Vec(기존 동작 불변). 테스트 전용(step은 retrieve_for_from_path를 직접 사용).
    #[cfg(test)]
    fn retrieve_for(&self, topic: &str) -> Vec<Utterance> {
        let active = self.active_path();
        self.retrieve_for_from_path(topic, &active)
    }

    /// 저장된 트리 상태(파일 또는 SQLite 세션 load_session)를 인메모리 세션에 주입한다.
    /// main이 --session/파일 재개 시 호출(v2-45 P7: Redis 스냅샷 경로 제거).
    pub fn seed_from(&mut self, snap: crate::types::ConversationSnapshot) {
        self.snapshot = snap;
    }

    /// 활성 경로(root->head) 전사를 반환한다.
    fn active_path(&self) -> Vec<Utterance> {
        self.snapshot.active_path()
    }

    /// round 발언들을 head에서 시작하는 체인으로 트리에 append하고 head를 옮긴다.
    /// core-sync 모드에서 발언별 append 실패 시 그 지점에서 후속 append를 중단해(체인 무결성),
    /// 실패 사실과 저장되지 않은 발언을 사용자에게 보이는 경고 문자열로 반환한다(결함 #3).
    /// 성공(또는 non-core 모드)이면 None.
    fn append_round(&mut self, round: &[Utterance]) -> Option<String> {
        // core-sync 모드: DB가 id 권위. append_turn으로 쓰고 DB 트리를 adopt(외부 post_turn 흡수).
        // 전량 persist(indexer)를 생략해 외부 쓰기 클로버를 구조적으로 차단한다(Plan 27 옵션 B).
        if let Some(cs) = &self.core_sync {
            let mut warning: Option<String> = None;
            for (i, u) in round.iter().enumerate() {
                if let Err(e) = cs.append_turn(&self.session_id, &u.speaker, &u.content) {
                    eprintln!("[core-sync] append 실패: {e}");
                    let unsaved: Vec<&str> =
                        round[i..].iter().map(|u| u.speaker.as_str()).collect();
                    warning = Some(format!(
                        "[core-sync 경고] 발언 저장 실패({}): {e}. 저장되지 않은 발언: {}",
                        u.speaker,
                        unsaved.join(", ")
                    ));
                    break; // 체인 무결성: 실패 지점 이후 후속 append를 중단한다.
                }
            }
            // 쓰기 후 DB 권위 트리를 채택(이번 라운드 성공분 + 사이에 들어온 외부 post 포함).
            self.adopt_from_core();
            return warning;
        }

        for u in round {
            self.snapshot.append(u.speaker.clone(), u.content.clone());
        }
        if let Some(idx) = &self.indexer {
            idx.persist(&self.session_id, &self.snapshot);
        }
        None
    }

    /// run_round 결과(부분 성공 발언 + 선택적 에러)를 append하고 사용자 출력 문자열로 합친다(결함 #6).
    /// 좌석 실패 시에도 이미 완료된 발언을 폐기하지 않고 append + 표면화하며, append_round 자체가
    /// 실패(결함 #3)해도 그 경고를 같은 출력에 병합한다.
    fn finish_round(&mut self, round: Vec<Utterance>, err: Option<RunError>) -> String {
        let append_warn = if round.is_empty() {
            None
        } else {
            self.append_round(&round)
        };
        let mut out = render(&round);
        if let Some(w) = append_warn {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&w);
        }
        if let Some(e) = err {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&format!(
                "[에러] 일부 좌석 응답 실패(완료된 발언은 보존됨): {e:?}"
            ));
        }
        out
    }

    /// 활성 경로의 발언 수를 반환한다(선형 사용 시 기존 transcript.len()과 동일).
    pub fn transcript_len(&self) -> usize {
        self.active_path().len()
    }

    /// 트리 전체 메시지 수를 반환한다(분기 포함).
    pub fn message_count(&self) -> usize {
        self.snapshot.node_count()
    }

    /// 활성 경로를 마크다운 결과 문서로 직렬화.
    pub fn transcript_markdown(&self) -> String {
        let mut out = String::from("# tunaRound 토론 기록\n\n");
        out.push_str(&render(&self.active_path()));
        out.push('\n');
        out
    }

    /// 테스트 전용: active_path를 공개 접근한다(외부 노출 목적 아님).
    #[cfg(test)]
    pub fn active_path_pub_for_test(&self) -> Vec<Utterance> {
        self.active_path()
    }

    /// 현재 트리를 상태 파일(JSON)로 저장한다.
    pub fn save_state(&self, path: &str) -> std::io::Result<()> {
        crate::store::save_session(&self.snapshot, path)
    }

    /// 현재 인메모리 대화 트리 스냅샷 참조(--core seed를 코어 DB에 권위로 반영할 때 사용). 호출부가
    /// StoredSession::from(&snapshot)으로 변환하므로 소유권이 불요 → 복제 없이 참조 반환(gemini 리뷰).
    pub fn snapshot(&self) -> &crate::types::ConversationSnapshot {
        &self.snapshot
    }

    /// 상태 파일에서 트리를 로드해 세션을 복원한다. 레거시 bare-array 포맷도 지원한다.
    pub fn resume(
        participants: Vec<Participant>,
        registry: Box<dyn RunnerRegistry>,
        path: &str,
    ) -> std::io::Result<Self> {
        let snapshot = crate::store::load_session(path)?;
        Ok(Self {
            participants,
            snapshot,
            registry,
            session_id: "default".to_string(),
            indexer: None,
            retriever: None,
            recent_turns: None,
            core_sync: None,
            validity_sink: None,
            annotation_sink: None,
            context_mode: ContextMode::Push,
        })
    }

    /// 한 입력을 처리한다. run_round 호출 등 로직만; 실제 I/O는 호출자(main).
    pub fn step(&mut self, cmd: Command) -> StepOutcome {
        // core-sync 모드: 명령 처리 전 코어 DB 권위 트리를 채택(외부 post_turn을 이번 라운드 prior에 반영).
        self.adopt_from_core();
        match cmd {
            Command::Quit => StepOutcome::Exit,
            Command::Noop => StepOutcome::Noop,
            Command::Help => StepOutcome::Print(
                "메시지를 입력하면 두 에이전트가 응답합니다. @engine 메시지로 한 자리만 지목(읽기), @engine! 메시지로 쓰기 턴(에이전트가 레포 편집), /debate [n] <주제>로 에이전트 N턴 자동 교환(기본 3, 최대 10), /conclude [engine] 종합, /save [경로] 결과 저장, /search <질의>로 인덱스 검색(--db 필요), /explain <질의>로 검색 디버그(토큰화·bm25·유효성), /branches 트리 목록, /checkout <id> 분기 전환, /supersede <id> [<대체id>] 발언을 대체됨으로 표시, /reject <id> 발언을 기각으로 표시(검색 제외), /annotate <id> --abstraction \"요약\" --anchors \"키워드1,키워드2\" 발언에 큐레이션 남기기(요약 표면화·앵커 부스트, 둘 중 하나만도 허용), /quit 종료.".into(),
            ),
            Command::Save(path) => StepOutcome::Save {
                path: path.unwrap_or_else(|| DEFAULT_SAVE_PATH.to_string()),
                markdown: self.transcript_markdown(),
            },
            Command::Message(text) => {
                // active_path를 1회만 계산하고 prior·carried·tlen·dedup 모두 재사용.
                let ctx = self.round_context();
                let retrieved = self.retrieve_for_from_path(&text, &ctx.full_path);
                let input = RoundInput { prior: &ctx.prior, retrieved: &retrieved, carried: &ctx.carried, ctx_mode: self.context_mode, transcript_len: ctx.transcript_len };
                let (round, err) = run_round(&self.participants, &text, self.registry.as_ref(), RunMode::ReadOnly, input);
                StepOutcome::Print(self.finish_round(round, err))
            }
            Command::Only { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                let ctx = self.round_context();
                let retrieved = self.retrieve_for_from_path(&text, &ctx.full_path);
                let input = RoundInput { prior: &ctx.prior, retrieved: &retrieved, carried: &ctx.carried, ctx_mode: self.context_mode, transcript_len: ctx.transcript_len };
                let (round, err) = run_round(&seats, &text, self.registry.as_ref(), RunMode::ReadOnly, input);
                StepOutcome::Print(self.finish_round(round, err))
            }
            Command::Write { engine, text } => {
                let seats: Vec<Participant> =
                    self.participants.iter().filter(|p| p.engine == engine).cloned().collect();
                if seats.is_empty() {
                    return StepOutcome::Print(format!("그런 자리가 없습니다: {engine}"));
                }
                let ctx = self.round_context();
                let retrieved = self.retrieve_for_from_path(&text, &ctx.full_path);
                let input = RoundInput { prior: &ctx.prior, retrieved: &retrieved, carried: &ctx.carried, ctx_mode: self.context_mode, transcript_len: ctx.transcript_len };
                let (round, err) = run_round(&seats, &text, self.registry.as_ref(), RunMode::Write, input);
                StepOutcome::Print(self.finish_round(round, err))
            }
            Command::Conclude(engine) => {
                let eng = engine.or_else(|| self.participants.first().map(|p| p.engine.clone()));
                let Some(eng) = eng else {
                    return StepOutcome::Print("종합할 참가자가 없습니다.".into());
                };
                let synth = vec![Participant {
                    engine: eng,
                    role: Some("synthesizer".into()),
                    instruction: String::new(),
                }];
                let ctx = self.round_context();
                let retrieved = self.retrieve_for_from_path("지금까지의 토론을 종합해 결론을 정리해줘.", &ctx.full_path);
                let input = RoundInput { prior: &ctx.prior, retrieved: &retrieved, carried: &ctx.carried, ctx_mode: self.context_mode, transcript_len: ctx.transcript_len };
                let (round, err) = run_round(&synth, "지금까지의 토론을 종합해 결론을 정리해줘.", self.registry.as_ref(), RunMode::ReadOnly, input);
                StepOutcome::Print(self.finish_round(round, err))
            }
            Command::Debate { turns, topic } => {
                let mut out = String::new();
                for k in 0..turns {
                    let round_topic = if k == 0 {
                        topic.clone()
                    } else {
                        "지금까지의 논의를 이어서, 앞 발언에 반박하거나 더 깊이 들어가줘. 새 주제를 꺼내지 말고 수렴을 시도해줘.".to_string()
                    };
                    // 매 라운드마다 새 발언이 추가되므로 active_path 재계산은 불가피.
                    let ctx = self.round_context();
                    // 검색 질의는 진행용 지시문(round_topic)이 아니라 원래 topic을 쓴다(결함 #5:
                    // 고정 지시문 문자열이 FTS/시맨틱 질의로 새어 들어가 무관한 히트를 끌어오는 것 방지).
                    let retrieved = self.retrieve_for_from_path(&topic, &ctx.full_path);
                    let input = RoundInput { prior: &ctx.prior, retrieved: &retrieved, carried: &ctx.carried, ctx_mode: self.context_mode, transcript_len: ctx.transcript_len };
                    let (round, err) = run_round(&self.participants, &round_topic, self.registry.as_ref(), RunMode::ReadOnly, input);
                    let had_error = err.is_some();
                    out.push_str(&format!("### 라운드 {}\n{}\n\n", k + 1, self.finish_round(round, err)));
                    if had_error {
                        break;
                    }
                }
                StepOutcome::Print(out)
            }
            Command::Search(q) => {
                const SEARCH_K: usize = 10;
                match &self.retriever {
                    None => StepOutcome::Print("검색이 비활성화돼 있습니다. --db <경로>로 실행하면 인덱스를 검색할 수 있습니다.".into()),
                    Some(r) => match r.retrieve(&q, SEARCH_K) {
                        // R7: 검색 실패(DB 장애)는 "결과 없음"으로 위장하지 않고 사용자에게 표시(REPL 계속).
                        Err(e) => StepOutcome::Print(format!("검색 실패: {e}")),
                        Ok(hits) if hits.is_empty() => StepOutcome::Print(format!("검색 결과 없음: {q}")),
                        Ok(hits) => {
                            StepOutcome::Print(format!("검색 결과({}건):\n\n{}", hits.len(), render(&hits)))
                        }
                    },
                }
            }
            Command::Explain(q) => {
                const EXPLAIN_K: usize = 10;
                match &self.retriever {
                    None => StepOutcome::Print("검색이 비활성화돼 있습니다. --db <경로>로 실행하세요.".into()),
                    Some(r) => StepOutcome::Print(r.debug_retrieve(&q, EXPLAIN_K, &self.session_id)),
                }
            }
            Command::Branches => StepOutcome::Print(self.snapshot.tree_summary()),
            Command::Checkout(id) => {
                // core-sync 모드: 매 명령 시작에 adopt_from_core가 DB 권위 head로 스냅샷을 통째
                // 교체하므로 여기서 head를 옮겨도 다음 step에서 바로 덮인다(결함 #2: 조용한 무력화
                // 대신 명시적으로 미지원임을 안내하고 head를 건드리지 않는다).
                if self.core_sync.is_some() {
                    StepOutcome::Print("core 모드에서는 분기(checkout)를 지원하지 않습니다(DB head가 매 명령마다 채택되어 분기가 무력화됩니다).".into())
                } else if self.snapshot.checkout(id) {
                    StepOutcome::Print(format!("checkout #{id} (현재 분기 전환). 이어서 메시지를 보내면 분기됩니다."))
                } else {
                    StepOutcome::Print(format!("그런 메시지가 없습니다: #{id}"))
                }
            }
            Command::Supersede { id, by } => self.mark_validity(id, "superseded", by, "대체됨"),
            Command::Reject(id) => self.mark_validity(id, "rejected", None, "기각됨"),
            Command::Annotate { id, abstraction, anchors } => {
                self.mark_annotation(id, abstraction.as_deref(), anchors.as_deref())
            }
        }
    }

    /// 큐레이션 지정 공용 처리: sink 미배선/발언 없음 안내 + 성공/실패 메시지.
    fn mark_annotation(
        &self,
        id: u64,
        abstraction: Option<&str>,
        anchors: Option<&str>,
    ) -> StepOutcome {
        let Some(sink) = &self.annotation_sink else {
            return StepOutcome::Print("큐레이션 지정은 --db <경로>로 실행해야 합니다.".into());
        };
        if !self.snapshot.contains(id) {
            return StepOutcome::Print(format!("그런 발언이 없습니다: #{id}"));
        }
        match sink.set_annotation(&self.session_id, id, abstraction, anchors) {
            Ok(()) => {
                let mut parts = Vec::new();
                if abstraction.is_some() {
                    parts.push("요약");
                }
                if anchors.is_some() {
                    parts.push("앵커");
                }
                StepOutcome::Print(format!(
                    "#{id} 큐레이션 저장({}). 이후 검색에서 요약이 표면화되고 앵커가 순위를 부스트합니다.",
                    parts.join("·")
                ))
            }
            Err(e) => StepOutcome::Print(format!("[큐레이션 지정 실패] {e}")),
        }
    }

    /// 유효성 지정 공용 처리: sink 미배선/발언 없음 안내 + 성공/실패 메시지.
    fn mark_validity(&self, id: u64, state: &str, by: Option<u64>, label: &str) -> StepOutcome {
        let Some(sink) = &self.validity_sink else {
            return StepOutcome::Print("유효성 지정은 --db <경로>로 실행해야 합니다.".into());
        };
        if !self.snapshot.contains(id) {
            return StepOutcome::Print(format!("그런 발언이 없습니다: #{id}"));
        }
        // 대체 발언(by)도 존재 검증한다(결함 #7: 오타 id가 검증 없이 저장되던 것 방지).
        if let Some(b) = by
            && !self.snapshot.contains(b)
        {
            return StepOutcome::Print(format!("대체 발언이 없습니다: #{b}"));
        }
        match sink.set_validity(&self.session_id, id, state, by) {
            Ok(()) => {
                let extra = by.map(|b| format!(" (대체: #{b})")).unwrap_or_default();
                StepOutcome::Print(format!(
                    "#{id} {label}{extra}. 이후 검색에서 디프리오리티/제외됩니다."
                ))
            }
            Err(e) => StepOutcome::Print(format!("[유효성 지정 실패] {e}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::orchestrator::MapRegistry;
    use crate::runner::{RunError, RunInput, RunOutput, Runner};
    use crate::store::{StoredMessage, StoredSession}; // 테스트 fixture(FakeCoreSync·seed_from)가 조립에 사용.

    struct FakeRunner {
        reply: String,
    }
    impl Runner for FakeRunner {
        fn run(&self, _i: &RunInput) -> Result<RunOutput, RunError> {
            Ok(RunOutput {
                content: self.reply.clone(),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    fn session_with_two_seats() -> Session {
        let mut reg = MapRegistry::new();
        reg.insert(
            "claude",
            Box::new(FakeRunner {
                reply: "제안".into(),
            }),
        );
        reg.insert(
            "codex",
            Box::new(FakeRunner {
                reply: "리뷰".into(),
            }),
        );
        let participants = vec![
            Participant {
                engine: "claude".into(),
                role: Some("proposer".into()),
                instruction: String::new(),
            },
            Participant {
                engine: "codex".into(),
                role: Some("reviewer".into()),
                instruction: String::new(),
            },
        ];
        Session::new(participants, Box::new(reg))
    }

    /// 공유 트리를 흉내내는 가짜 CoreSync(외부 post_turn 시뮬레이션용). DB id 권위를 모사.
    #[derive(Clone)]
    struct FakeCoreSync {
        db: std::sync::Arc<std::sync::Mutex<StoredSession>>,
    }
    impl FakeCoreSync {
        fn new() -> Self {
            Self {
                db: std::sync::Arc::new(std::sync::Mutex::new(StoredSession {
                    messages: vec![],
                    head: None,
                })),
            }
        }
        fn append_inner(&self, speaker: &str, content: &str) -> u64 {
            let mut db = self.db.lock().unwrap();
            let new_id = db.messages.iter().map(|m| m.id).max().unwrap_or(0) + 1;
            let parent = db.head;
            db.messages.push(StoredMessage {
                id: new_id,
                parent_id: parent,
                speaker: speaker.into(),
                content: content.into(),
            });
            db.head = Some(new_id);
            new_id
        }
        /// 다른 프론트/에이전트의 post_turn을 흉내낸다(REPL 밖에서 DB에 직접 추가).
        fn external_post(&self, speaker: &str, content: &str) -> u64 {
            self.append_inner(speaker, content)
        }
        fn len(&self) -> usize {
            self.db.lock().unwrap().messages.len()
        }
    }
    impl crate::orchestrator::CoreSync for FakeCoreSync {
        fn load_session(&self, _sid: &str) -> Option<crate::types::ConversationSnapshot> {
            let db = self.db.lock().unwrap();
            if db.messages.is_empty() {
                None
            } else {
                Some(db.clone().into())
            }
        }
        fn append_turn(&self, _sid: &str, speaker: &str, content: &str) -> Result<u64, String> {
            Ok(self.append_inner(speaker, content))
        }
    }

    /// core-sync append 실패를 흉내내는 가짜(결함 #3 테스트용). 지정한 순번(1-based)에서 실패한다.
    struct FailingCoreSync {
        fail_at: usize,
        calls: std::sync::Mutex<usize>,
    }
    impl crate::orchestrator::CoreSync for FailingCoreSync {
        fn load_session(&self, _sid: &str) -> Option<crate::types::ConversationSnapshot> {
            None
        }
        fn append_turn(&self, _sid: &str, _speaker: &str, _content: &str) -> Result<u64, String> {
            let mut c = self.calls.lock().unwrap();
            *c += 1;
            if *c == self.fail_at {
                Err("의도된 실패".into())
            } else {
                Ok(*c as u64)
            }
        }
    }

    fn core_sync_session(cs: FakeCoreSync) -> Session {
        let mut reg = MapRegistry::new();
        reg.insert(
            "claude",
            Box::new(FakeRunner {
                reply: "제안".into(),
            }),
        );
        reg.insert(
            "codex",
            Box::new(FakeRunner {
                reply: "리뷰".into(),
            }),
        );
        let participants = vec![
            Participant {
                engine: "claude".into(),
                role: Some("proposer".into()),
                instruction: String::new(),
            },
            Participant {
                engine: "codex".into(),
                role: Some("reviewer".into()),
                instruction: String::new(),
            },
        ];
        Session::new(participants, Box::new(reg)).with_core_sync(Some(Box::new(cs)))
    }

    #[test]
    fn parses_validity_commands() {
        assert_eq!(
            parse_command("/supersede 3"),
            Command::Supersede { id: 3, by: None }
        );
        assert_eq!(
            parse_command("/supersede 3 7"),
            Command::Supersede { id: 3, by: Some(7) }
        );
        assert_eq!(parse_command("/reject 4"), Command::Reject(4));
        assert_eq!(
            parse_command("/explain 검색 질의"),
            Command::Explain("검색 질의".into())
        );
        assert_eq!(
            parse_command("/explain"),
            Command::Message("/explain".into())
        );
        // 인자 없으면 일반 메시지로 폴스루.
        assert_eq!(
            parse_command("/supersede"),
            Command::Message("/supersede".into())
        );
        assert_eq!(
            parse_command("/reject x"),
            Command::Message("/reject x".into())
        );
    }

    #[test]
    fn parses_annotate() {
        // 둘 다 지정(따옴표 안 공백·콤마 보존).
        assert_eq!(
            parse_command("/annotate 3 --abstraction \"핵심 결정 텍스트\" --anchors \"검색,랭킹\""),
            Command::Annotate {
                id: 3,
                abstraction: Some("핵심 결정 텍스트".into()),
                anchors: Some("검색,랭킹".into()),
            }
        );
        // abstraction만.
        assert_eq!(
            parse_command("/annotate 5 --abstraction \"요약만\""),
            Command::Annotate {
                id: 5,
                abstraction: Some("요약만".into()),
                anchors: None
            }
        );
        // anchors만.
        assert_eq!(
            parse_command("/annotate 7 --anchors \"a,b\""),
            Command::Annotate {
                id: 7,
                abstraction: None,
                anchors: Some("a,b".into())
            }
        );
        // 따옴표 없는 단일 토큰 값도 허용.
        assert_eq!(
            parse_command("/annotate 9 --anchors kiwi"),
            Command::Annotate {
                id: 9,
                abstraction: None,
                anchors: Some("kiwi".into())
            }
        );
        // id 없음 / 플래그 없음 / 빈 값은 일반 메시지로 폴스루.
        assert_eq!(
            parse_command("/annotate"),
            Command::Message("/annotate".into())
        );
        assert_eq!(
            parse_command("/annotate 3"),
            Command::Message("/annotate 3".into())
        );
        assert_eq!(
            parse_command("/annotate x --abstraction \"y\""),
            Command::Message("/annotate x --abstraction \"y\"".into())
        );
        assert_eq!(
            parse_command("/annotate 3 --abstraction \"\""),
            Command::Message("/annotate 3 --abstraction \"\"".into())
        );
        // 값 없는 --abstraction 뒤에 --anchors가 바로 오면, --anchors를 값으로 삼키지 않고 정상 파싱.
        assert_eq!(
            parse_command("/annotate 3 --abstraction --anchors \"x\""),
            Command::Annotate {
                id: 3,
                abstraction: None,
                anchors: Some("x".into())
            }
        );
        // 양쪽 다 값 없으면 일반 메시지로 폴스루(삼킴 없음).
        assert_eq!(
            parse_command("/annotate 3 --abstraction --anchors"),
            Command::Message("/annotate 3 --abstraction --anchors".into())
        );
    }

    /// set_validity 캡처 튜플: (session_id, msg_id, state, by).
    type ValidityCapture = (String, u64, String, Option<u64>);

    /// set_validity 호출을 캡처하는 가짜 sink.
    struct CapturingSink {
        last: std::sync::Mutex<Option<ValidityCapture>>,
    }
    impl crate::orchestrator::ValiditySink for CapturingSink {
        fn set_validity(
            &self,
            sid: &str,
            msg_id: u64,
            state: &str,
            by: Option<u64>,
        ) -> Result<(), String> {
            *self.last.lock().unwrap() = Some((sid.to_string(), msg_id, state.to_string(), by));
            Ok(())
        }
    }

    #[test]
    fn supersede_command_calls_sink_for_existing_message() {
        let sink = std::sync::Arc::new(CapturingSink {
            last: std::sync::Mutex::new(None),
        });
        // 메시지 2건 있는 세션 구성(sink 배선). by(#2)도 존재해야 검증(결함 #7)을 통과한다.
        let mut s =
            session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
        s.seed_from(
            StoredSession {
                messages: vec![
                    StoredMessage {
                        id: 1,
                        parent_id: None,
                        speaker: "claude".into(),
                        content: "x".into(),
                    },
                    StoredMessage {
                        id: 2,
                        parent_id: Some(1),
                        speaker: "codex".into(),
                        content: "y".into(),
                    },
                ],
                head: Some(2),
            }
            .into(),
        );
        let out = s.step(Command::Supersede { id: 1, by: Some(2) });
        assert!(matches!(out, StepOutcome::Print(_)));
        let cap = sink.last.lock().unwrap().clone();
        assert_eq!(
            cap,
            Some(("default".into(), 1, "superseded".into(), Some(2)))
        );
    }

    #[test]
    fn supersede_missing_message_does_not_call_sink() {
        let sink = std::sync::Arc::new(CapturingSink {
            last: std::sync::Mutex::new(None),
        });
        let mut s =
            session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
        let _ = s.step(Command::Reject(99)); // 없는 id.
        assert_eq!(
            sink.last.lock().unwrap().clone(),
            None,
            "없는 발언은 sink 미호출"
        );
    }

    #[test]
    fn supersede_missing_by_is_rejected_and_does_not_call_sink() {
        // by(대체 발언 id)가 존재하지 않으면 대상 id와 같은 방식으로 거부돼야 한다(결함 #7).
        let sink = std::sync::Arc::new(CapturingSink {
            last: std::sync::Mutex::new(None),
        });
        let mut s =
            session_with_two_seats().with_validity_sink(Some(Box::new(SinkHandle(sink.clone()))));
        s.seed_from(
            StoredSession {
                messages: vec![StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "claude".into(),
                    content: "x".into(),
                }],
                head: Some(1),
            }
            .into(),
        );
        match s.step(Command::Supersede {
            id: 1,
            by: Some(99),
        }) {
            StepOutcome::Print(t) => assert!(t.contains("99"), "없는 by id 안내 불일치: {t}"),
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(
            sink.last.lock().unwrap().clone(),
            None,
            "존재하지 않는 by는 sink 미호출"
        );
    }

    #[test]
    fn validity_command_without_sink_guides() {
        let mut s = session_with_two_seats(); // sink 미배선.
        match s.step(Command::Reject(1)) {
            StepOutcome::Print(t) => assert!(t.contains("--db"), "안내 불일치: {t}"),
            _ => panic!("Print 기대"),
        }
    }

    /// Arc<CapturingSink>를 Box<dyn ValiditySink>로 넘기기 위한 얇은 래퍼.
    struct SinkHandle(std::sync::Arc<CapturingSink>);
    impl crate::orchestrator::ValiditySink for SinkHandle {
        fn set_validity(
            &self,
            sid: &str,
            msg_id: u64,
            state: &str,
            by: Option<u64>,
        ) -> Result<(), String> {
            self.0.set_validity(sid, msg_id, state, by)
        }
    }

    /// set_annotation 캡처 튜플: (session_id, msg_id, abstraction, anchors).
    type AnnotationCapture = (String, u64, Option<String>, Option<String>);

    /// set_annotation 호출을 캡처하는 가짜 sink(session_id, msg_id, abstraction, anchors).
    struct CapturingAnnotationSink {
        last: std::sync::Mutex<Option<AnnotationCapture>>,
    }
    impl crate::orchestrator::AnnotationSink for CapturingAnnotationSink {
        fn set_annotation(
            &self,
            sid: &str,
            msg_id: u64,
            abstraction: Option<&str>,
            anchors: Option<&str>,
        ) -> Result<(), String> {
            *self.last.lock().unwrap() = Some((
                sid.to_string(),
                msg_id,
                abstraction.map(str::to_string),
                anchors.map(str::to_string),
            ));
            Ok(())
        }
    }
    /// Arc<CapturingAnnotationSink>를 Box<dyn AnnotationSink>로 넘기기 위한 얇은 래퍼.
    struct AnnotationSinkHandle(std::sync::Arc<CapturingAnnotationSink>);
    impl crate::orchestrator::AnnotationSink for AnnotationSinkHandle {
        fn set_annotation(
            &self,
            sid: &str,
            msg_id: u64,
            abstraction: Option<&str>,
            anchors: Option<&str>,
        ) -> Result<(), String> {
            self.0.set_annotation(sid, msg_id, abstraction, anchors)
        }
    }

    #[test]
    fn annotate_command_calls_sink_for_existing_message() {
        let sink = std::sync::Arc::new(CapturingAnnotationSink {
            last: std::sync::Mutex::new(None),
        });
        let mut s = session_with_two_seats()
            .with_annotation_sink(Some(Box::new(AnnotationSinkHandle(sink.clone()))));
        s.seed_from(
            StoredSession {
                messages: vec![StoredMessage {
                    id: 1,
                    parent_id: None,
                    speaker: "claude".into(),
                    content: "x".into(),
                }],
                head: Some(1),
            }
            .into(),
        );
        let out = s.step(Command::Annotate {
            id: 1,
            abstraction: Some("요약".into()),
            anchors: Some("검색,랭킹".into()),
        });
        assert!(matches!(out, StepOutcome::Print(_)));
        let cap = sink.last.lock().unwrap().clone();
        assert_eq!(
            cap,
            Some((
                "default".into(),
                1,
                Some("요약".into()),
                Some("검색,랭킹".into())
            ))
        );
    }

    #[test]
    fn annotate_missing_message_does_not_call_sink() {
        let sink = std::sync::Arc::new(CapturingAnnotationSink {
            last: std::sync::Mutex::new(None),
        });
        let mut s = session_with_two_seats()
            .with_annotation_sink(Some(Box::new(AnnotationSinkHandle(sink.clone()))));
        let _ = s.step(Command::Annotate {
            id: 99,
            abstraction: Some("요약".into()),
            anchors: None,
        });
        assert_eq!(
            sink.last.lock().unwrap().clone(),
            None,
            "없는 발언은 sink 미호출"
        );
    }

    #[test]
    fn annotate_command_without_sink_guides() {
        let mut s = session_with_two_seats(); // sink 미배선.
        match s.step(Command::Annotate {
            id: 1,
            abstraction: Some("요약".into()),
            anchors: None,
        }) {
            StepOutcome::Print(t) => assert!(t.contains("--db"), "안내 불일치: {t}"),
            _ => panic!("Print 기대"),
        }
    }

    /// 긴 발언 여러 개를 반환하는 가짜 retriever(길이 cap 테스트용).
    struct LongRetriever;
    impl crate::orchestrator::ContextRetriever for LongRetriever {
        fn retrieve(&self, _q: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
            Ok((0..3)
                .map(|i| Utterance::new(format!("s{i}"), "가".repeat(1200)))
                .collect())
        }
    }

    #[test]
    fn retrieved_injection_is_capped_by_chars() {
        // 1200자 발언 3개(총 3600 > MAX_RETRIEVED_CHARS 2000) → 누적 초과 전까지만(1건).
        let s = session_with_two_seats().with_retriever(Some(Box::new(LongRetriever)));
        let got = s.retrieve_for("주제");
        assert_eq!(got.len(), 1, "글자수 cap으로 초과 발언 드롭(최소 1건 보장)");
    }

    #[test]
    fn core_sync_round_writes_through_to_db() {
        // core-sync 모드: 라운드 발언이 DB(CoreSync)에 append되고 인메모리도 그걸 채택.
        let cs = FakeCoreSync::new();
        let mut s = core_sync_session(cs.clone());
        let _ = s.step(Command::Message("설계 논의".into()));
        // 2좌석 응답 2건이 DB에 기록.
        assert_eq!(cs.len(), 2, "라운드 발언이 DB에 써져야 함");
        assert_eq!(s.message_count(), 2, "인메모리도 DB를 채택");
    }

    #[test]
    fn core_sync_adopts_external_post_and_does_not_clobber() {
        // 외부 post_turn(다른 프론트)이 들어와도 REPL이 다음 step에서 흡수하고, REPL 턴이 덮지 않는다.
        let cs = FakeCoreSync::new();
        let mut s = core_sync_session(cs.clone());

        // 1라운드: REPL 발언 2건(DB id 1,2).
        let _ = s.step(Command::Message("첫 주제".into()));
        assert_eq!(cs.len(), 2);

        // 외부 참가자가 post_turn으로 발언 추가(DB id 3).
        cs.external_post("remote/agent", "외부에서 추가한 발언");
        assert_eq!(cs.len(), 3);

        // 2라운드: step 시작에 adopt → 외부 발언이 prior에 들어오고, REPL 2건이 더해짐(id 4,5).
        let _ = s.step(Command::Message("이어서".into()));
        assert_eq!(cs.len(), 5, "외부 발언 보존 + REPL 2건 추가(클로버 없음)");
        // 인메모리 트리에 외부 발언이 포함되어야 한다.
        let path = s.active_path();
        assert!(
            path.iter().any(|u| u.content == "외부에서 추가한 발언"),
            "외부 post_turn이 활성 경로에 흡수되어야 함: {:?}",
            path.iter().map(|u| u.content.as_str()).collect::<Vec<_>>()
        );
    }

    #[test]
    fn core_sync_append_failure_halts_chain_and_surfaces_warning() {
        // 2번째 append_turn 호출(codex 좌석)에서 실패 -> 1번(claude) 발언은 화면에 남고, 실패가
        // StepOutcome에 표면화되며, 3번째 이후 append는 시도되지 않는다(체인 무결성, 결함 #3).
        let cs = FailingCoreSync {
            fail_at: 2,
            calls: std::sync::Mutex::new(0),
        };
        let mut reg = MapRegistry::new();
        reg.insert(
            "claude",
            Box::new(FakeRunner {
                reply: "제안".into(),
            }),
        );
        reg.insert(
            "codex",
            Box::new(FakeRunner {
                reply: "리뷰".into(),
            }),
        );
        let participants = vec![
            Participant {
                engine: "claude".into(),
                role: Some("proposer".into()),
                instruction: String::new(),
            },
            Participant {
                engine: "codex".into(),
                role: Some("reviewer".into()),
                instruction: String::new(),
            },
        ];
        let mut s = Session::new(participants, Box::new(reg)).with_core_sync(Some(Box::new(cs)));
        match s.step(Command::Message("주제".into())) {
            StepOutcome::Print(t) => {
                assert!(
                    t.contains("제안"),
                    "먼저 완료된 발언은 화면에 남아야 함: {t}"
                );
                assert!(
                    t.contains("실패"),
                    "실패가 사용자 출력에 표면화돼야 함: {t}"
                );
            }
            other => panic!("expected Print, got {other:?}"),
        }
    }

    #[test]
    fn parses_commands() {
        assert_eq!(parse_command("/quit"), Command::Quit);
        assert_eq!(parse_command("/help"), Command::Help);
        assert_eq!(
            parse_command("/save notes.md"),
            Command::Save(Some("notes.md".into()))
        );
        assert_eq!(parse_command("/save"), Command::Save(None));
        assert_eq!(
            parse_command("이 설계 어떤가요?"),
            Command::Message("이 설계 어떤가요?".into())
        );
    }

    #[test]
    fn blank_is_noop() {
        assert_eq!(parse_command("   "), Command::Noop);
    }

    #[test]
    fn parses_debate() {
        assert_eq!(
            parse_command("/debate 3 이 설계 괜찮나"),
            Command::Debate {
                turns: 3,
                topic: "이 설계 괜찮나".into()
            }
        );
        // 숫자 생략 -> 기본 3턴
        assert_eq!(
            parse_command("/debate 주제만"),
            Command::Debate {
                turns: 3,
                topic: "주제만".into()
            }
        );
        // 상한 clamp(최대 10)
        assert_eq!(
            parse_command("/debate 50 큰주제"),
            Command::Debate {
                turns: 10,
                topic: "큰주제".into()
            }
        );
        // 주제 없음 -> 일반 메시지로 폴스루
        assert_eq!(parse_command("/debate"), Command::Message("/debate".into()));
        assert_eq!(
            parse_command("/debate 3"),
            Command::Message("/debate 3".into())
        ); // 숫자만, 주제 없음
    }

    #[test]
    fn render_formats_speaker_and_content() {
        let utts = vec![Utterance {
            speaker: "claude/proposer".into(),
            content: "제안".into(),
            abstraction: None,
        }];
        let out = render(&utts);
        assert!(out.contains("claude/proposer"));
        assert!(out.contains("제안"));
    }

    #[test]
    fn step_message_runs_round_and_prints() {
        let mut s = session_with_two_seats();
        match s.step(Command::Message("이 설계?".into())) {
            StepOutcome::Print(text) => {
                assert!(text.contains("제안"));
                assert!(text.contains("리뷰"));
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 2);
    }

    #[test]
    fn parses_conclude() {
        assert_eq!(parse_command("/conclude"), Command::Conclude(None));
        assert_eq!(
            parse_command("/conclude claude"),
            Command::Conclude(Some("claude".into()))
        );
    }

    #[test]
    fn step_conclude_runs_synthesizer_and_grows_transcript() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제?".into())); // 전사 2개 채움
        let before = s.transcript_len();
        match s.step(Command::Conclude(None)) {
            StepOutcome::Print(text) => assert!(text.contains("제안")), // 기본 엔진=claude FakeRunner reply
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), before + 1); // 종합 1발언 추가
    }

    #[test]
    fn step_quit_help_save() {
        let mut s = session_with_two_seats();
        assert!(matches!(s.step(Command::Quit), StepOutcome::Exit));
        assert!(matches!(s.step(Command::Help), StepOutcome::Print(_)));
        assert!(matches!(s.step(Command::Noop), StepOutcome::Noop));
        match s.step(Command::Save(Some("x.md".into()))) {
            StepOutcome::Save { path, .. } => assert_eq!(path, "x.md"),
            other => panic!("expected Save, got {other:?}"),
        }
    }

    #[test]
    fn parses_at_engine_target() {
        assert_eq!(
            parse_command("@codex 이거 봐줘"),
            Command::Only {
                engine: "codex".into(),
                text: "이거 봐줘".into()
            }
        );
        // @만 있고 메시지 없으면 일반 메시지로 취급
        assert_eq!(parse_command("@codex"), Command::Message("@codex".into()));
    }

    #[test]
    fn step_only_targets_single_seat() {
        let mut s = session_with_two_seats();
        match s.step(Command::Only {
            engine: "codex".into(),
            text: "리뷰만".into(),
        }) {
            StepOutcome::Print(text) => {
                assert!(text.contains("리뷰")); // codex FakeRunner reply
                assert!(!text.contains("제안")); // claude는 응답 안 함
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 1);
    }

    #[test]
    fn step_only_unknown_engine_errors() {
        let mut s = session_with_two_seats();
        match s.step(Command::Only {
            engine: "gemini".into(),
            text: "?".into(),
        }) {
            StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
            other => panic!("expected Print, got {other:?}"),
        }
    }

    struct ModeEchoRunner;
    impl Runner for ModeEchoRunner {
        fn run(&self, i: &RunInput) -> Result<RunOutput, RunError> {
            Ok(RunOutput {
                content: format!("mode={:?}", i.mode),
                input_tokens: 0,
                output_tokens: 0,
            })
        }
    }

    fn session_with_mode_echo() -> Session {
        let mut reg = MapRegistry::new();
        reg.insert("codex", Box::new(ModeEchoRunner));
        let participants = vec![Participant {
            engine: "codex".into(),
            role: Some("coder".into()),
            instruction: String::new(),
        }];
        Session::new(participants, Box::new(reg))
    }

    #[test]
    fn parses_at_engine_bang_as_write() {
        assert_eq!(
            parse_command("@codex! 이 함수 고쳐줘"),
            Command::Write {
                engine: "codex".into(),
                text: "이 함수 고쳐줘".into()
            }
        );
        // 읽기 지목은 그대로
        assert_eq!(
            parse_command("@codex 봐줘"),
            Command::Only {
                engine: "codex".into(),
                text: "봐줘".into()
            }
        );
        // bang만 있고 메시지 없으면 일반 메시지
        assert_eq!(parse_command("@codex!"), Command::Message("@codex!".into()));
    }

    #[test]
    fn step_write_uses_write_mode_on_single_seat() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Write {
            engine: "codex".into(),
            text: "고쳐줘".into(),
        }) {
            StepOutcome::Print(text) => assert!(text.contains("Write"), "got: {text}"),
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 1);
    }

    #[test]
    fn step_only_stays_readonly() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Only {
            engine: "codex".into(),
            text: "봐줘".into(),
        }) {
            StepOutcome::Print(text) => assert!(text.contains("ReadOnly"), "got: {text}"),
            other => panic!("expected Print, got {other:?}"),
        }
    }

    #[test]
    fn step_write_unknown_engine_errors() {
        let mut s = session_with_mode_echo();
        match s.step(Command::Write {
            engine: "gemini".into(),
            text: "x".into(),
        }) {
            StepOutcome::Print(text) => assert!(text.contains("자리가 없")),
            other => panic!("expected Print, got {other:?}"),
        }
    }

    #[test]
    fn parses_branches_and_checkout() {
        assert_eq!(parse_command("/branches"), Command::Branches);
        assert_eq!(parse_command("/checkout 3"), Command::Checkout(3));
        assert_eq!(
            parse_command("/checkout"),
            Command::Message("/checkout".into())
        ); // 인자 없으면 일반 메시지
    }

    #[test]
    fn checkout_then_message_creates_branch() {
        let mut s = session_with_two_seats(); // claude=제안, codex=리뷰
        let _ = s.step(Command::Message("주제".into())); // msg 1,2 (head=2)
        // head를 1로 옮기고 새 메시지 -> 분기(2의 sibling)
        match s.step(Command::Checkout(1)) {
            StepOutcome::Print(t) => assert!(t.contains("1")),
            other => panic!("got {other:?}"),
        }
        let _ = s.step(Command::Message("다른 방향".into())); // msg 3,4 (parent=1, 분기)
        // 트리에 4개 메시지(2개 분기), active path는 1->3->4 (길이 3)
        assert_eq!(s.message_count(), 4);
        assert_eq!(s.transcript_len(), 3);
    }

    #[test]
    fn step_debate_runs_n_rounds_and_grows_tree() {
        let mut s = session_with_two_seats(); // claude="제안", codex="리뷰" (FakeRunner)
        match s.step(Command::Debate {
            turns: 2,
            topic: "주제".into(),
        }) {
            StepOutcome::Print(text) => {
                assert!(text.contains("라운드 1"));
                assert!(text.contains("라운드 2"));
                assert!(text.contains("제안") && text.contains("리뷰"));
            }
            other => panic!("expected Print, got {other:?}"),
        }
        // 2턴 x 2자리 = 메시지 4개(트리), active path 길이 4
        assert_eq!(s.message_count(), 4);
        assert_eq!(s.transcript_len(), 4);
    }

    /// 검색 질의를 캡처하는 가짜 retriever(결함 #5 테스트용).
    struct CapturingRetriever {
        queries: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    }
    impl crate::orchestrator::ContextRetriever for CapturingRetriever {
        fn retrieve(&self, q: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
            self.queries.lock().unwrap().push(q.to_string());
            Ok(Vec::new())
        }
    }

    #[test]
    fn debate_uses_original_topic_for_retrieval_not_round_directive() {
        // 2라운드 이상에서도 검색 질의는 진행용 고정 지시문(round_topic)이 아니라 원래 topic이어야
        // 한다(결함 #5: 지시문이 FTS/시맨틱 질의로 새어 들어가 무관한 히트를 끌어오는 것 방지).
        let queries = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut s = session_with_two_seats().with_retriever(Some(Box::new(CapturingRetriever {
            queries: queries.clone(),
        })));
        let _ = s.step(Command::Debate {
            turns: 3,
            topic: "원래 주제".into(),
        });
        let qs = queries.lock().unwrap();
        assert_eq!(qs.len(), 3, "라운드마다 검색 호출: {qs:?}");
        for q in qs.iter() {
            assert_eq!(
                q, "원래 주제",
                "매 라운드 검색 질의는 원래 topic 고정: {qs:?}"
            );
        }
    }

    #[test]
    fn step_debate_stops_on_error() {
        // 첫 라운드는 OK, 이후 에러나는 시나리오는 FakeRunner로 만들기 번거로우니
        // 최소: turns=1도 정상 동작(라운드 1만)
        let mut s = session_with_two_seats();
        match s.step(Command::Debate {
            turns: 1,
            topic: "주제".into(),
        }) {
            StepOutcome::Print(text) => {
                assert!(text.contains("라운드 1") && !text.contains("라운드 2"))
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.message_count(), 2);
    }

    #[test]
    fn checkout_unknown_id_errors() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제".into()));
        match s.step(Command::Checkout(99)) {
            StepOutcome::Print(t) => assert!(t.contains("없")),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn checkout_is_refused_in_core_sync_mode() {
        // core-sync 모드에서는 adopt_from_core가 매 명령마다 DB head로 스냅샷을 통째 교체하므로
        // checkout이 실제로는 무력하다. 조용히 "분기됩니다"라고 안내하는 대신 명시적으로 거부해야
        // 한다(결함 #2). head도 옮기지 않아야 한다.
        let cs = FakeCoreSync::new();
        let mut s = core_sync_session(cs.clone());
        let _ = s.step(Command::Message("주제".into())); // DB에 발언 2건(claude, codex).
        let path_before = s.active_path();

        match s.step(Command::Checkout(1)) {
            StepOutcome::Print(t) => {
                assert!(
                    t.contains("지원하지 않"),
                    "core 모드 미지원 안내가 있어야 함: {t}"
                );
                assert!(
                    !t.contains("분기 전환"),
                    "실제 분기 전환처럼 보이면 안 됨: {t}"
                );
            }
            other => panic!("expected Print, got {other:?}"),
        }

        // head가 그대로 유지되어 다음 메시지가 checkout(1) 기준이 아니라 기존 head 기준으로 이어진다.
        let _ = s.step(Command::Message("이어서".into()));
        let path_after = s.active_path();
        assert_eq!(
            path_after.len(),
            path_before.len() + 2,
            "checkout이 무시되고 기존 head에서 정상 진행돼야 함: {path_after:?}"
        );
    }

    struct FakeRetriever {
        results: Vec<Utterance>,
    }
    impl crate::orchestrator::ContextRetriever for FakeRetriever {
        fn retrieve(&self, _query: &str, _limit: usize) -> Result<Vec<Utterance>, String> {
            Ok(self.results.clone())
        }
    }

    #[test]
    fn retrieve_for_deduplicates_active_path_content() {
        let mut s = session_with_two_seats(); // claude="제안", codex="리뷰"
        let _ = s.step(Command::Message("초기 주제".into()));
        // 활성 경로에 "제안", "리뷰" 두 발언이 있다.
        let active = s.active_path();
        let dup_content = active[0].content.clone(); // "제안" - 활성경로 중복

        let retriever = FakeRetriever {
            results: vec![
                Utterance {
                    speaker: "past/speaker".into(),
                    content: dup_content,
                    abstraction: None,
                },
                Utterance {
                    speaker: "past/other".into(),
                    content: "고유 맥락 발언".into(),
                    abstraction: None,
                },
            ],
        };
        let s = s.with_retriever(Some(Box::new(retriever)));

        let retrieved = s.retrieve_for("테스트 쿼리");
        // 활성경로 중복("제안")은 제외하고 신규("고유 맥락 발언")만 남아야 한다.
        assert_eq!(retrieved.len(), 1, "dedup 후 1개여야 함: {:?}", retrieved);
        assert_eq!(retrieved[0].content, "고유 맥락 발언");
    }

    #[test]
    fn retrieve_for_returns_empty_without_retriever() {
        let s = session_with_two_seats(); // retriever 없음
        let result = s.retrieve_for("어떤 주제");
        assert!(result.is_empty(), "retriever 없으면 빈 결과");
    }

    /// 큐레이션(v2-51) 회귀 방지: annotation(abstraction)이 달린 현재-세션 active-path 발언이
    /// 검색 히트로 돌아와도, content(raw)가 활성 경로와 일치하면 dedup으로 제외돼 **이중 주입되지 않아야** 한다.
    /// (표면화를 retriever content에 하면 content가 변형돼 dedup이 깨졌던 실회귀를 못박는다.)
    #[test]
    fn annotated_active_path_hit_is_deduped_not_double_injected() {
        struct AnnotatedRetriever {
            dup: String,
        }
        impl crate::orchestrator::ContextRetriever for AnnotatedRetriever {
            fn retrieve(&self, _q: &str, _l: usize) -> Result<Vec<Utterance>, String> {
                // finish가 실어 보내는 것과 동형: content=raw(활성 경로와 동일), abstraction=Some.
                Ok(vec![Utterance {
                    speaker: "past/speaker".into(),
                    content: self.dup.clone(),
                    abstraction: Some("증류 요약".into()),
                }])
            }
        }
        let mut s = session_with_two_seats(); // claude="제안", codex="리뷰"
        let _ = s.step(Command::Message("초기 주제".into())); // 활성 경로에 "제안","리뷰"
        let active = s.active_path();
        let dup_content = active[0].content.clone(); // "제안"(활성 경로 발언)
        let s = s.with_retriever(Some(Box::new(AnnotatedRetriever { dup: dup_content })));
        let retrieved = s.retrieve_for("테스트 쿼리");
        assert!(
            retrieved.is_empty(),
            "annotation 달린 active-path 발언이 dedup되지 않아 이중 주입됨: {retrieved:?}"
        );
    }

    #[derive(Default)]
    struct IdxCalls {
        persists: usize,
        last_session: String,
        last_len: usize,
    }
    struct FakeIndexer(std::sync::Arc<std::sync::Mutex<IdxCalls>>);
    impl crate::store::indexer::MessageIndexer for FakeIndexer {
        fn persist(&self, session_id: &str, snap: &crate::types::ConversationSnapshot) {
            let mut c = self.0.lock().unwrap();
            c.persists += 1;
            c.last_session = session_id.to_string();
            c.last_len = snap.node_count();
        }
    }

    #[test]
    fn round_persists_to_indexer_when_present() {
        let calls = std::sync::Arc::new(std::sync::Mutex::new(IdxCalls::default()));
        let mut reg = MapRegistry::new();
        reg.insert(
            "claude",
            Box::new(FakeRunner {
                reply: "제안".into(),
            }),
        );
        let participants = vec![Participant {
            engine: "claude".into(),
            role: Some("proposer".into()),
            instruction: String::new(),
        }];
        let mut s = Session::new_with_indexer(
            participants,
            Box::new(reg),
            "sess-i".into(),
            Some(Box::new(FakeIndexer(std::sync::Arc::clone(&calls)))),
        );
        let _ = s.step(Command::Message("주제".into()));
        let c = calls.lock().unwrap();
        assert_eq!(c.persists, 1);
        assert_eq!(c.last_session, "sess-i");
        assert_eq!(c.last_len, 1); // 1자리 1발언
    }

    #[test]
    fn no_indexer_means_normal_behavior() {
        let mut s = session_with_two_seats(); // indexer 없음
        let _ = s.step(Command::Message("주제".into()));
        assert_eq!(s.transcript_len(), 2); // 기존 동작 불변
    }

    #[test]
    fn parses_search() {
        assert_eq!(
            parse_command("/search 검색 시스템"),
            Command::Search("검색 시스템".into())
        );
        // 인자 없으면 일반 메시지로 폴스루(기존 명령 패턴)
        assert_eq!(parse_command("/search"), Command::Message("/search".into()));
    }

    #[test]
    fn step_search_without_retriever_explains() {
        let mut s = session_with_two_seats(); // retriever 없음
        match s.step(Command::Search("아무거나".into())) {
            StepOutcome::Print(t) => assert!(t.contains("검색") && t.contains("--db")),
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn step_search_with_retriever_renders_hits() {
        // FakeRetriever(고정 Utterance 반환)로 검색 결과 렌더 확인.
        struct FakeRetriever(Vec<Utterance>);
        impl crate::orchestrator::ContextRetriever for FakeRetriever {
            fn retrieve(&self, _q: &str, _l: usize) -> Result<Vec<Utterance>, String> {
                Ok(self.0.clone())
            }
        }
        let hits = vec![Utterance {
            speaker: "claude/proposer".into(),
            content: "검색 시스템 설계".into(),
            abstraction: None,
        }];
        let mut s = session_with_two_seats().with_retriever(Some(Box::new(FakeRetriever(hits))));
        match s.step(Command::Search("검색".into())) {
            StepOutcome::Print(t) => {
                assert!(t.contains("검색 시스템 설계"));
                assert!(t.contains("claude/proposer"));
            }
            other => panic!("got {other:?}"),
        }
    }

    #[test]
    fn prior_for_prompt_uncapped_by_default() {
        let mut s = session_with_two_seats();
        let _ = s.step(Command::Message("주제1".into())); // 발언 2개
        let _ = s.step(Command::Message("주제2".into())); // 총 4개
        // 기본(None) = prior_for_prompt가 활성 경로 전체와 길이 동일.
        assert_eq!(s.prior_for_prompt().len(), s.transcript_len());
    }

    #[test]
    fn prior_for_prompt_caps_to_recent_n() {
        let mut s = session_with_two_seats().with_recent_turns(Some(2));
        let _ = s.step(Command::Message("주제1".into()));
        let _ = s.step(Command::Message("주제2".into())); // 활성 경로 4턴
        let prior = s.prior_for_prompt();
        assert_eq!(prior.len(), 2); // 최근 2턴만 재주입
        // 마지막 발언이 활성 경로 전체의 마지막 발언과 동일해야 한다.
        let full = s.active_path_pub_for_test();
        assert_eq!(
            prior.last().map(|u| &u.content),
            full.last().map(|u| &u.content)
        );
    }

    // --- carry_forward_digest 테스트 ---

    #[test]
    fn carry_forward_digest_empty_when_no_cap() {
        // recent_turns None(기본) -> 드롭 없음 -> 빈 문자열.
        let s = session_with_two_seats();
        assert_eq!(s.carry_forward_digest(), "");
    }

    #[test]
    fn carry_forward_digest_empty_when_path_not_exceeded() {
        // recent_turns=Some(4), 발언 2개(path 2) -> path<=n -> 빈 문자열.
        let mut s = session_with_two_seats().with_recent_turns(Some(4));
        let _ = s.step(Command::Message("주제".into())); // path 길이 2
        assert_eq!(s.carry_forward_digest(), "");
    }

    #[test]
    fn carry_forward_digest_includes_dropped_speaker_and_gist() {
        // recent_turns=Some(2), 두 번 Message -> path=4, 드롭=2(path[..2]).
        // 드롭된 발언의 speaker와 gist가 다이제스트에 포함돼야 한다.
        let mut s = session_with_two_seats().with_recent_turns(Some(2));
        let _ = s.step(Command::Message("주제1".into())); // path 2
        let _ = s.step(Command::Message("주제2".into())); // path 4, 드롭 2
        let digest = s.carry_forward_digest();
        assert!(!digest.is_empty(), "드롭 존재 -> 비어있으면 안 됨");
        // claude/proposer="제안", codex/reviewer="리뷰" 중 하나는 포함돼야 한다.
        assert!(
            digest.contains("claude/proposer") || digest.contains("codex/reviewer"),
            "speaker 없음: {digest}"
        );
        assert!(
            digest.contains("제안") || digest.contains("리뷰"),
            "gist 없음: {digest}"
        );
    }

    #[test]
    fn with_context_mode_pull_does_not_break_step() {
        // with_context_mode(Pull) 후 step이 정상 동작하는지(스모크). FakeRunner 엔진이므로 동작 동일.
        let mut s =
            session_with_two_seats().with_context_mode(crate::orchestrator::ContextMode::Pull);
        match s.step(Command::Message("테스트".into())) {
            StepOutcome::Print(text) => {
                assert!(
                    text.contains("제안") || text.contains("리뷰"),
                    "출력 없음: {text}"
                );
            }
            other => panic!("expected Print, got {other:?}"),
        }
        assert_eq!(s.transcript_len(), 2);
    }

    #[test]
    fn default_context_mode_is_push() {
        // 기본(미설정) context_mode는 Push여야 한다.
        let s = session_with_two_seats();
        assert_eq!(s.context_mode, crate::orchestrator::ContextMode::Push);
    }

    #[test]
    fn carry_forward_digest_caps_at_max_carry() {
        // 긴 응답을 내는 러너로 캡 초과 시나리오 구성.
        // recent_turns=Some(1), 10번 Message -> path=20, 드롭=19 -> 각 라인 ~100자 합계 ~1900 > 1500.
        let mut reg = MapRegistry::new();
        let long_reply = "A".repeat(200);
        reg.insert(
            "claude",
            Box::new(FakeRunner {
                reply: long_reply.clone(),
            }),
        );
        reg.insert("codex", Box::new(FakeRunner { reply: long_reply }));
        let parts = vec![
            Participant {
                engine: "claude".into(),
                role: Some("proposer".into()),
                instruction: String::new(),
            },
            Participant {
                engine: "codex".into(),
                role: Some("reviewer".into()),
                instruction: String::new(),
            },
        ];
        let mut s = Session::new(parts, Box::new(reg)).with_recent_turns(Some(1));
        for _ in 0..10 {
            let _ = s.step(Command::Message("주제".into()));
        }
        let digest = s.carry_forward_digest();
        assert!(digest.contains("이전"), "생략 표기 없음: {digest}");
        assert!(
            digest.len() <= super::MAX_CARRY,
            "MAX_CARRY 초과: {} > {}",
            digest.len(),
            super::MAX_CARRY
        );
    }

    #[test]
    fn carry_forward_digest_pull_summarizes_whole_path() {
        // Pull 모드: recent_turns 없이도 전사 전체를 요약(안전망). Push 기본과 대비.
        use crate::orchestrator::ContextMode;
        let mut s = session_with_two_seats().with_context_mode(ContextMode::Pull);
        let _ = s.step(Command::Message("주제1".into())); // path 2
        let digest = s.carry_forward_digest();
        assert!(!digest.is_empty(), "pull 모드는 전사 전체를 요약해야 함");
        assert!(
            digest.contains("claude/proposer") || digest.contains("codex/reviewer"),
            "speaker 없음: {digest}"
        );
        // 같은 전사라도 Push(미캡)면 빈 문자열이어야 한다(대조).
        let mut s2 = session_with_two_seats();
        let _ = s2.step(Command::Message("주제1".into()));
        assert_eq!(s2.carry_forward_digest(), "", "push 미캡은 빈 요약");
    }
}
