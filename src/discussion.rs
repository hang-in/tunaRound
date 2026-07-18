// mesh 토론 driver: chat의 라운드 오케스트레이션(순차-인지·역할·종합)을 브로커 안의 결정적 코드로
// 수행한다(v2-56). 좌석 배달은 기존 A2A task 생명주기를 그대로 쓰고(1라운드 발언 = 1 task), 전사는
// TranscriptWriter(append_turn)로 `debate:<id>` 세션에 영속한다. 완료 대기는 인프로세스 순수 폴링
// (driver가 브로커 안에 있어 이벤트 버스 불필요, 타임아웃→try_cancel이 정본 탈출구).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::orchestrator::prompt::{PromptContext, build_round_prompt};
use crate::orchestrator::{Participant, TranscriptWriter};
use crate::store::a2a::{Message, Part, TaskState};
use crate::store::sqlite::SqliteStore;
use crate::types::Utterance;

/// 토론 네임스페이스 프리픽스. from_agent와 전사 session_id가 같은 값을 쓴다
/// (`debate:<discussion_id>`). 기동 고아 sweep(`fail_orphan_debate_tasks`)의 LIKE 술어와 동기 유지.
pub const DEBATE_NS_PREFIX: &str = "debate:";

/// discussion_id → from_agent(=전사 session_id) 네임스페이스 문자열.
pub fn debate_ns(discussion_id: &str) -> String {
    format!("{DEBATE_NS_PREFIX}{discussion_id}")
}

/// 토론 한 좌석. label은 전사 speaker(`debate/<label>`)와 프롬프트 화자 표기에 쓴다(uuid 원문 비노출).
#[derive(Debug, Clone)]
pub struct DiscussionSeat {
    /// 로스터 online agent uuid(task to_agent).
    pub agent: String,
    /// 표기용 라벨(로스터 display_name 또는 uuid 앞 8자).
    pub label: String,
    /// 토론 역할(roles::role_guidance가 아는 역할만 행동 지시문 주입, 그 외는 무시).
    pub role: Option<String>,
    /// 좌석별 추가 지시(자유 텍스트).
    pub instruction: String,
    /// 라이브 세션 좌석 여부(프리앰블 변형: 라이브=claim/complete 마감 지시, 헤드리스=출력만).
    pub live: bool,
}

/// 확정된 토론 사양(start_discussion 검증 통과분).
#[derive(Debug, Clone)]
pub struct DiscussionSpec {
    pub id: String,
    pub topic: String,
    pub seats: Vec<DiscussionSeat>,
    pub rounds: u32,
}

/// driver 대기 파라미터. 테스트가 짧은 값으로 주입한다.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    /// 좌석 하나의 응답 대기 상한(초). lease(30분)보다 훨씬 짧게 두어 requeue 재배달(이중 발언)을 차단.
    pub seat_timeout_secs: u64,
    /// get_task 폴링 간격(밀리초).
    pub poll_interval_ms: u64,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            seat_timeout_secs: 600,
            poll_interval_ms: 3000,
        }
    }
}

/// 진행 중 토론 레지스트리(동시 1건 제한, v2-56 MVP). 인메모리라 브로커 재기동 시 자연 소멸하고,
/// 남은 고아 task는 기동 sweep(fail_orphan_debate_tasks)이 실패 처리한다(별도 락 영속 없음 - Phase 0
/// 토론 합의: store-backed 락은 (a)/(b) 경계를 흐린다).
#[derive(Default)]
pub struct DiscussionRegistry {
    active: Mutex<Option<ActiveDiscussion>>,
}

struct ActiveDiscussion {
    id: String,
    cancel: Arc<AtomicBool>,
}

impl DiscussionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 새 토론을 점유한다. 이미 진행 중이면 Err(동시 1건 제한). 성공 시 취소 플래그를 반환한다.
    pub fn try_begin(&self, id: &str) -> Result<Arc<AtomicBool>, String> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = active.as_ref() {
            return Err(format!(
                "이미 진행 중인 토론이 있습니다: {} (MVP=동시 1건, stop_discussion 후 재시도)",
                a.id
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        *active = Some(ActiveDiscussion {
            id: id.to_string(),
            cancel: cancel.clone(),
        });
        Ok(cancel)
    }

    /// 진행 중 토론의 이후 라운드 발행을 중단시킨다(이미 실행 중인 좌석 러너는 중단 불가).
    pub fn cancel(&self, id: &str) -> Result<(), String> {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        match active.as_ref() {
            Some(a) if a.id == id => {
                a.cancel.store(true, Ordering::Relaxed);
                Ok(())
            }
            Some(a) => Err(format!("진행 중 토론 id 불일치: 현재={}", a.id)),
            None => Err("진행 중인 토론이 없습니다".to_string()),
        }
    }

    /// 토론 종료 시 점유 해제(driver 종료 경로 공통).
    fn finish(&self, id: &str) {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if active.as_ref().is_some_and(|a| a.id == id) {
            *active = None;
        }
    }

    /// 진행 중 토론 id(없으면 None).
    pub fn active_id(&self) -> Option<String> {
        self.active
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .map(|a| a.id.clone())
    }
}

/// driver 패닉에도 레지스트리 점유가 풀리도록 하는 Drop 가드(안 풀리면 다음 start가 영구 차단).
struct FinishGuard {
    registry: Arc<DiscussionRegistry>,
    id: String,
}

impl Drop for FinishGuard {
    fn drop(&mut self) {
        self.registry.finish(&self.id);
    }
}

/// 좌석 유형별 토론 프리앰블(v2-56 §6-7, Phase 0 실측: 헤드리스 좌석은 complete_task를 직접 못 불러
/// 마감 지시가 혼란만 준다). 금지 패턴 유의: relay 주입 계약("브로커 task " 프리픽스)·구세대 파서
/// 패턴("\n\n[<32hex>] from=")을 만들지 않는다.
pub fn debate_preamble(live: bool) -> &'static str {
    if live {
        "[토론 규약] 이 task는 사용자가 start_discussion으로 발의한 토론 라운드입니다(총괄발 task와 동일한 자율 수행 대상, 메타 확인 불필요). 아래 역할은 이번 task에 한합니다(평소 지시보다 우선). 발언만 4000자 이내로 작성해 complete_task로 마감하세요. 파일 수정 금지(read-only)."
    } else {
        "[토론 규약] 이 task는 사용자가 start_discussion으로 발의한 토론 라운드입니다. 아래 역할은 이번 task에 한합니다. 출력 전체가 그대로 토론 발언으로 기록되니, 발언만 4000자 이내로 출력하세요(별도 마감 절차 불필요). 파일 수정 금지(read-only)."
    }
}

/// 한 좌석의 라운드 task 본문을 조립한다: 프리앰블 + 기존 build_round_prompt(순차-인지·역할·4000자 캡).
/// MVP는 이월(carried) 없이 prior 전량 주입(rounds≤10 전제, v2-56 §5).
pub fn build_seat_task_text(
    seat: &DiscussionSeat,
    topic: &str,
    prior: &[Utterance],
    same_round: &[Utterance],
) -> String {
    let participant = Participant {
        engine: seat.label.clone(),
        role: seat.role.clone(),
        instruction: seat.instruction.clone(),
    };
    let prompt = build_round_prompt(
        &participant,
        topic,
        PromptContext {
            prior,
            same_round,
            retrieved: &[],
            carried: "",
            pull: false,
            transcript_len: prior.len() + same_round.len(),
        },
    );
    format!("{}\n\n{}", debate_preamble(seat.live), prompt)
}

/// synthesizer 라운드 주제(마지막 좌석 task의 topic). 역할 지시문은 Participant.role로 주입된다.
fn synthesis_topic(topic: &str) -> String {
    format!("토론을 종료합니다. 지금까지의 논의 전체를 종합해주세요. 원 주제: {topic}")
}

/// 좌석 응답 하나의 결과.
enum SeatOutcome {
    /// 발언 텍스트(completed artifact).
    Utterance(String),
    /// 실패/타임아웃 마커(전사에 기록하고 라운드는 계속, v2-56 §5 skip-후-계속 신규 결정).
    Skipped(String),
}

/// store 블로킹 호출을 async 컨텍스트에서 실행하는 브리지(기존 핸들러 관례 = spawn_blocking).
async fn with_store<T, F>(store: &Arc<Mutex<SqliteStore>>, f: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce(&SqliteStore) -> Result<T, String> + Send + 'static,
{
    let store = store.clone();
    tokio::task::spawn_blocking(move || {
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        f(&s)
    })
    .await
    .map_err(|e| format!("spawn_blocking: {e}"))?
}

/// 전사 append 브리지(writer도 SQLite 블로킹).
async fn append_transcript(
    writer: &Arc<dyn TranscriptWriter>,
    session: &str,
    speaker: &str,
    content: &str,
) -> Result<u64, String> {
    let writer = writer.clone();
    let (session, speaker, content) = (
        session.to_string(),
        speaker.to_string(),
        content.to_string(),
    );
    tokio::task::spawn_blocking(move || writer.append_turn(&session, &speaker, &content))
        .await
        .map_err(|e| format!("spawn_blocking: {e}"))?
}

/// 좌석 하나에 라운드 task를 발행하고 terminal까지 폴링 대기한다. 타임아웃이면 try_cancel 후
/// Skipped(늦은 완료는 canceled 가드에 막혀 무해 = 이중 발언 차단).
async fn run_seat(
    store: &Arc<Mutex<SqliteStore>>,
    from_agent: &str,
    seat: &DiscussionSeat,
    text: String,
    cfg: &DriverConfig,
) -> SeatOutcome {
    let from = from_agent.to_string();
    let to = seat.agent.clone();
    let created = with_store(store, move |s| {
        let message_id = s.new_task_id()?;
        s.create_task_from_message(
            &from,
            &to,
            Message {
                message_id,
                role: "user".to_string(),
                parts: vec![Part {
                    text: Some(text),
                    ..Default::default()
                }],
                task_id: None,
                context_id: None,
            },
        )
    })
    .await;
    let task_id = match created {
        Ok(t) => t.id,
        Err(e) => return SeatOutcome::Skipped(format!("[무응답: task 발행 실패 - {e}]")),
    };

    let deadline = std::time::Duration::from_secs(cfg.seat_timeout_secs);
    let started = std::time::Instant::now();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(cfg.poll_interval_ms)).await;
        let tid = task_id.clone();
        let task = match with_store(store, move |s| s.get_task(&tid)).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                return SeatOutcome::Skipped(format!("[무응답: task {task_id} 소실]"));
            }
            Err(e) => {
                // 일시 DB 오류는 다음 폴에서 재시도(타임아웃이 상한).
                eprintln!("[debate] get_task 오류(재시도): {e}");
                if started.elapsed() >= deadline {
                    return SeatOutcome::Skipped(format!("[무응답: 폴링 실패 - {e}]"));
                }
                continue;
            }
        };
        match task.state {
            TaskState::Completed => {
                let text: String = task
                    .artifacts
                    .iter()
                    .flat_map(|a| a.parts.iter())
                    .filter_map(|p| p.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    return SeatOutcome::Skipped("[무응답: 빈 발언(artifact 없음)]".to_string());
                }
                return SeatOutcome::Utterance(text);
            }
            TaskState::Failed => {
                let reason = task
                    .status_message
                    .as_ref()
                    .and_then(|m| m.parts.first())
                    .and_then(|p| p.text.as_deref())
                    .unwrap_or("사유 없음");
                return SeatOutcome::Skipped(format!("[무응답: 좌석 실패 - {reason}]"));
            }
            TaskState::Canceled => {
                return SeatOutcome::Skipped("[무응답: task 취소됨]".to_string());
            }
            _ => {
                if started.elapsed() >= deadline {
                    // 재배달(이중 발언) 차단: 취소를 시도하고 skip. 이미 실행 중인 러너는 중단되지
                    // 않지만 늦은 complete는 canceled 가드(try_complete state='working')에 막힌다.
                    let tid = task_id.clone();
                    let _ = with_store(store, move |s| s.try_cancel(&tid)).await;
                    return SeatOutcome::Skipped(format!(
                        "[무응답: {}초 타임아웃]",
                        cfg.seat_timeout_secs
                    ));
                }
            }
        }
    }
}

/// 토론 driver 본체. start_discussion이 tokio::spawn으로 띄운다. 모든 종료 경로에서 레지스트리 점유를
/// 해제한다(FinishGuard=패닉 포함). 전사 append 실패는 치명(전사가 정본)이라 토론을 중단한다.
pub async fn run_discussion(
    spec: DiscussionSpec,
    store: Arc<Mutex<SqliteStore>>,
    writer: Arc<dyn TranscriptWriter>,
    registry: Arc<DiscussionRegistry>,
    cancel: Arc<AtomicBool>,
    cfg: DriverConfig,
) {
    let ns = debate_ns(&spec.id);
    let _guard = FinishGuard {
        registry,
        id: spec.id.clone(),
    };
    eprintln!(
        "[debate {}] 시작: 좌석 {}석, {}라운드, 주제={}",
        spec.id,
        spec.seats.len(),
        spec.rounds,
        spec.topic.chars().take(60).collect::<String>()
    );
    if let Err(e) = append_transcript(&writer, &ns, "debate/user", &spec.topic).await {
        eprintln!("[debate {}] 전사 기록 실패로 중단: {e}", spec.id);
        return;
    }

    let mut prior: Vec<Utterance> = Vec::new();
    let mut canceled = false;
    let mut aborted = false;
    'rounds: for round in 1..=spec.rounds {
        let mut same_round: Vec<Utterance> = Vec::new();
        let mut failures = 0usize;
        for seat in &spec.seats {
            if cancel.load(Ordering::Relaxed) {
                canceled = true;
                break 'rounds;
            }
            let text = build_seat_task_text(seat, &spec.topic, &prior, &same_round);
            let speaker = format!("debate/{}", seat.label);
            let content = match run_seat(&store, &ns, seat, text, &cfg).await {
                SeatOutcome::Utterance(t) => t,
                SeatOutcome::Skipped(marker) => {
                    failures += 1;
                    eprintln!("[debate {}] r{round} {}: {marker}", spec.id, seat.label);
                    marker
                }
            };
            if let Err(e) = append_transcript(&writer, &ns, &speaker, &content).await {
                eprintln!("[debate {}] 전사 기록 실패로 중단: {e}", spec.id);
                return;
            }
            same_round.push(Utterance::new(speaker, content));
        }
        if failures == spec.seats.len() {
            aborted = true;
            prior.extend(same_round);
            break;
        }
        prior.extend(same_round);
        eprintln!("[debate {}] 라운드 {round}/{} 완료", spec.id, spec.rounds);
    }

    if canceled || aborted {
        let marker = if canceled {
            "[토론 중단: stop_discussion]"
        } else {
            "[토론 중단: 라운드 전 좌석 실패]"
        };
        let _ = append_transcript(&writer, &ns, "debate/driver", marker).await;
        eprintln!("[debate {}] {marker}", spec.id);
        return;
    }

    // synthesizer 라운드: 첫 좌석(v2-56 §8-5 기본값)에게 종합을 위임한다. driver는 러너가 없어 LLM
    // 생성을 스스로 못 한다. 실패 시 합의문 없이 종료(부분 전사가 남고, failed terminal이 곧 통지).
    let synth_seat = DiscussionSeat {
        role: Some("synthesizer".to_string()),
        ..spec.seats[0].clone()
    };
    let text = build_seat_task_text(&synth_seat, &synthesis_topic(&spec.topic), &prior, &[]);
    let speaker = format!("debate/{}", synth_seat.label);
    match run_seat(&store, &ns, &synth_seat, text, &cfg).await {
        SeatOutcome::Utterance(t) => {
            let _ = append_transcript(&writer, &ns, &speaker, &t).await;
            eprintln!("[debate {}] 종합 완료(전사={ns})", spec.id);
        }
        SeatOutcome::Skipped(marker) => {
            let _ = append_transcript(
                &writer,
                &ns,
                "debate/driver",
                &format!("[종합 실패: {marker}]"),
            )
            .await;
            eprintln!("[debate {}] 종합 실패: {marker}", spec.id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seat(agent: &str, label: &str, role: Option<&str>, live: bool) -> DiscussionSeat {
        DiscussionSeat {
            agent: agent.to_string(),
            label: label.to_string(),
            role: role.map(String::from),
            instruction: String::new(),
            live,
        }
    }

    #[test]
    fn registry_enforces_single_active_discussion() {
        let reg = DiscussionRegistry::new();
        let cancel = reg.try_begin("d1").expect("첫 점유는 성공");
        assert!(reg.try_begin("d2").is_err(), "동시 1건 제한");
        assert_eq!(reg.active_id().as_deref(), Some("d1"));
        // 다른 id cancel은 거부, 일치 id는 플래그 설정.
        assert!(reg.cancel("d2").is_err());
        reg.cancel("d1").expect("일치 id cancel");
        assert!(cancel.load(Ordering::Relaxed));
        // finish 후 재점유 가능.
        reg.finish("d1");
        assert!(reg.active_id().is_none());
        reg.try_begin("d2").expect("해제 후 점유");
    }

    #[test]
    fn registry_cancel_without_active_is_error() {
        let reg = DiscussionRegistry::new();
        assert!(reg.cancel("dx").is_err());
    }

    #[test]
    fn finish_guard_releases_on_drop() {
        let reg = Arc::new(DiscussionRegistry::new());
        reg.try_begin("d1").unwrap();
        {
            let _g = FinishGuard {
                registry: reg.clone(),
                id: "d1".to_string(),
            };
        }
        assert!(reg.active_id().is_none(), "Drop이 점유를 해제");
    }

    #[test]
    fn preamble_varies_by_seat_type_and_avoids_forbidden_patterns() {
        let live = debate_preamble(true);
        let headless = debate_preamble(false);
        assert!(live.contains("complete_task"));
        assert!(
            !headless.contains("complete_task"),
            "헤드리스는 마감 지시 없음"
        );
        for p in [live, headless] {
            assert!(!p.starts_with("브로커 task "), "relay 주입 프리픽스 금지");
            assert!(!p.contains("] from="), "구세대 파서 패턴 금지");
        }
    }

    #[test]
    fn seat_task_text_carries_role_prior_and_same_round() {
        let s = seat("uuid-1", "mac-claude", Some("reviewer"), true);
        let prior = vec![Utterance::new("debate/win-worker", "이전 라운드 발언")];
        let same = vec![Utterance::new("debate/mac-claude", "같은 라운드 앞 발언")];
        let text = build_seat_task_text(&s, "주제문", &prior, &same);
        assert!(text.starts_with("[토론 규약]"));
        assert!(text.contains("## Your role"), "역할 지시 주입");
        assert!(text.contains("이전 라운드 발언"));
        assert!(text.contains("같은 라운드 앞 발언"));
        assert!(text.contains("주제문"));
    }

    #[test]
    fn debate_ns_matches_sweep_predicate() {
        // 기동 sweep의 LIKE 'debate:%' 술어와 동기(문자 그대로 프리픽스).
        assert_eq!(debate_ns("abc123"), "debate:abc123");
        assert!(debate_ns("x").starts_with(DEBATE_NS_PREFIX));
    }

    // --- driver 통합 테스트: 인메모리 store + 가짜 좌석 워커 + 캡처 writer ---

    struct CapturingWriter(Mutex<Vec<(String, String, String)>>);

    impl TranscriptWriter for CapturingWriter {
        fn append_turn(
            &self,
            session_id: &str,
            speaker: &str,
            content: &str,
        ) -> Result<u64, String> {
            let mut v = self.0.lock().unwrap();
            v.push((
                session_id.to_string(),
                speaker.to_string(),
                content.to_string(),
            ));
            Ok(v.len() as u64)
        }
    }

    /// 가짜 좌석: agent 앞 open task를 폴링해 claim→complete(에코)한다. 받은 본문을 기록한다.
    fn spawn_fake_seat(
        store: Arc<Mutex<SqliteStore>>,
        agent: &'static str,
        reply_prefix: &'static str,
        received: Arc<Mutex<Vec<String>>>,
    ) -> tokio::task::JoinHandle<()> {
        use crate::store::a2a::Artifact;
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let s = store.lock().unwrap_or_else(|e| e.into_inner());
                let open = s.list_open_tasks_for(agent).unwrap_or_default();
                for t in open {
                    if t.state != TaskState::Submitted {
                        continue;
                    }
                    let msg = t
                        .status_message
                        .as_ref()
                        .and_then(|m| m.parts.first())
                        .and_then(|p| p.text.clone())
                        .unwrap_or_default();
                    if s.try_claim(&t.id, Some(agent), Some("fake")).is_ok() {
                        received.lock().unwrap().push(msg.clone());
                        let n = received.lock().unwrap().len();
                        let artifact = Artifact {
                            artifact_id: format!("art-{agent}-{n}"),
                            name: None,
                            parts: vec![Part {
                                text: Some(format!("{reply_prefix} 발언 {n}")),
                                ..Default::default()
                            }],
                        };
                        s.try_complete(&t.id, &[artifact], Some(agent))
                            .expect("가짜 좌석 complete");
                    }
                }
            }
        })
    }

    fn test_cfg() -> DriverConfig {
        DriverConfig {
            seat_timeout_secs: 3,
            poll_interval_ms: 20,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_runs_rounds_sequential_aware_and_synthesizes() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let cancel = registry.try_begin("t1").unwrap();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let recv_b = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        let seat_b = spawn_fake_seat(store.clone(), "agent-b", "B", recv_b.clone());

        let spec = DiscussionSpec {
            id: "t1".to_string(),
            topic: "테스트 주제".to_string(),
            seats: vec![
                seat("agent-a", "seat-a", Some("proposer"), false),
                seat("agent-b", "seat-b", Some("reviewer"), false),
            ],
            rounds: 2,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            cancel,
            test_cfg(),
        )
        .await;
        seat_a.abort();
        seat_b.abort();

        // 순차-인지: 좌석 B의 첫 수신 본문에 좌석 A의 같은 라운드 발언이 포함된다.
        let b_first = recv_b.lock().unwrap()[0].clone();
        assert!(
            b_first.contains("A 발언 1"),
            "seat-b 프롬프트에 seat-a 발언이 주입돼야 함: {b_first}"
        );
        // 전사: user + (2좌석 × 2라운드) + synthesizer(seat-a의 3번째 발언) = 6항목.
        let rows = cap.0.lock().unwrap();
        assert_eq!(rows.len(), 6, "전사 항목 수: {rows:?}");
        assert!(rows.iter().all(|(s, _, _)| s == "debate:t1"));
        assert_eq!(rows[0].1, "debate/user");
        assert_eq!(rows[1].1, "debate/seat-a");
        assert_eq!(rows[2].1, "debate/seat-b");
        assert_eq!(rows[5].1, "debate/seat-a"); // synthesizer=첫 좌석
        assert!(rows[5].2.contains("A 발언 3"));
        // synthesizer task 본문에 종합 지시가 갔는지(seat-a 3번째 수신).
        let a_third = recv_a.lock().unwrap()[2].clone();
        assert!(a_third.contains("종합해주세요"));
        // 레지스트리 해제.
        assert!(registry.active_id().is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_skips_dead_seat_with_timeout_and_cancels_task() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let cancel = registry.try_begin("t2").unwrap();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        // agent-dead는 아무도 처리하지 않는다(타임아웃 경로).

        let spec = DiscussionSpec {
            id: "t2".to_string(),
            topic: "타임아웃 테스트".to_string(),
            seats: vec![
                seat("agent-a", "seat-a", None, false),
                seat("agent-dead", "seat-dead", None, false),
            ],
            rounds: 1,
        };
        let cfg = DriverConfig {
            seat_timeout_secs: 1,
            poll_interval_ms: 20,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            cancel,
            cfg,
        )
        .await;
        seat_a.abort();

        let rows = cap.0.lock().unwrap();
        // 죽은 좌석은 [무응답] 마커로 skip되고 토론은 계속(synthesizer=seat-a까지 진행).
        let dead_row = rows.iter().find(|(_, sp, _)| sp == "debate/seat-dead");
        assert!(
            dead_row.is_some_and(|(_, _, c)| c.contains("[무응답")),
            "타임아웃 마커: {rows:?}"
        );
        assert!(rows.iter().any(|(_, sp, _)| sp == "debate/seat-a"));
        // 죽은 좌석의 task는 canceled로 전이됐다(재배달 차단).
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        let open = s.list_open_tasks_for("agent-dead").unwrap();
        assert!(open.is_empty(), "고아 task가 canceled로 닫혀야 함");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_stops_issuing_after_cancel() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let cancel = registry.try_begin("t3").unwrap();
        cancel.store(true, Ordering::Relaxed); // 시작 전 취소 = 첫 좌석 발행 전 중단.

        let spec = DiscussionSpec {
            id: "t3".to_string(),
            topic: "취소 테스트".to_string(),
            seats: vec![
                seat("agent-a", "seat-a", None, false),
                seat("agent-b", "seat-b", None, false),
            ],
            rounds: 3,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            cancel,
            test_cfg(),
        )
        .await;

        let rows = cap.0.lock().unwrap();
        // user + 중단 마커만. synthesizer 없음.
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert!(rows[1].2.contains("stop_discussion"));
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        assert!(s.list_open_tasks_for("agent-a").unwrap().is_empty());
    }
}
