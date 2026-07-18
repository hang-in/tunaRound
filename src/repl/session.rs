// 토론 세션 상태(대화 트리·러너 레지스트리)와 명령 실행 로직.

use super::*;

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
    /// pub(super): tests.rs(형제 모듈)가 기본값을 직접 검증한다(default_context_mode_is_push).
    pub(super) context_mode: ContextMode,
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
    /// pub(super): tests.rs(형제 모듈)에서 직접 호출한다.
    #[cfg(test)]
    pub(super) fn retrieve_for(&self, topic: &str) -> Vec<Utterance> {
        let active = self.active_path();
        self.retrieve_for_from_path(topic, &active)
    }

    /// 저장된 트리 상태(파일 또는 SQLite 세션 load_session)를 인메모리 세션에 주입한다.
    /// main이 --session/파일 재개 시 호출(v2-45 P7: Redis 스냅샷 경로 제거).
    pub fn seed_from(&mut self, snap: crate::types::ConversationSnapshot) {
        self.snapshot = snap;
    }

    /// 활성 경로(root->head) 전사를 반환한다.
    /// pub(super): tests.rs(형제 모듈)에서 직접 호출한다.
    pub(super) fn active_path(&self) -> Vec<Utterance> {
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
    /// 반환값 = (사용자 출력, append_failed). append_failed는 core-sync append가 실패해 발언이
    /// 권위 전사에 저장되지 못했음을 뜻한다(Debate 등 다회 루프가 DB 저장 실패에도 계속 도는 것을
    /// 막도록 노출한다 - gemini HIGH).
    fn finish_round(&mut self, round: Vec<Utterance>, err: Option<RunError>) -> (String, bool) {
        let append_warn = if round.is_empty() {
            None
        } else {
            self.append_round(&round)
        };
        let append_failed = append_warn.is_some();
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
        (out, append_failed)
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
                StepOutcome::Print(self.finish_round(round, err).0)
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
                StepOutcome::Print(self.finish_round(round, err).0)
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
                StepOutcome::Print(self.finish_round(round, err).0)
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
                StepOutcome::Print(self.finish_round(round, err).0)
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
                    let runner_error = err.is_some();
                    let (text, append_failed) = self.finish_round(round, err);
                    out.push_str(&format!("### 라운드 {}\n{text}\n\n", k + 1));
                    // 러너 실패뿐 아니라 core-sync 저장 실패에도 중단한다(권위 전사가 깨진 채 계속
                    // 토론하면 후속 라운드가 옛 head를 부모로 삼아 체인이 어긋난다, gemini HIGH).
                    if runner_error || append_failed {
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
