// mesh 토론 driver: chat의 라운드 오케스트레이션(순차-인지·역할·종합)을 브로커 안의 결정적 코드로
// 수행한다(v2-56). 좌석 배달은 기존 A2A task 생명주기를 그대로 쓰고(1라운드 발언 = 1 task), 전사는
// TranscriptWriter(append_turn)로 `debate:<id>` 세션에 영속한다. 완료 대기는 인프로세스 순수 폴링
// (driver가 브로커 안에 있어 이벤트 버스 불필요). 타임아웃·중단 탈출구는 try_fail이다: canceled는
// watch-results가 배달하지 않아 인박스 무통지가 되므로 failed로 마감한다(적대 리뷰 major).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use crate::orchestrator::prompt::{PromptContext, build_round_prompt};
use crate::orchestrator::{Participant, TranscriptWriter};
use crate::store::a2a::{Artifact, Message, Part, TaskState};
use crate::store::sqlite::SqliteStore;
use crate::types::Utterance;

/// 토론 네임스페이스 프리픽스. from_agent와 전사 session_id가 같은 값을 쓴다
/// (`debate:<discussion_id>`). 기동 고아 sweep(`fail_orphan_debate_tasks`)의 LIKE 술어와 동기 유지.
pub const DEBATE_NS_PREFIX: &str = "debate:";

/// 게이트 다이제스트·대기 표식 task의 to_agent(좌석이 아니라 driver 자신이 발행·소비). 색인 제외
/// (mcp/indexing.rs: 다이제스트 요약은 이미 색인된 좌석 발언의 스니펫 중복)와 동기 유지.
pub const DEBATE_DRIVER_AGENT: &str = "debate-driver";

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
    /// 라운드 간 사람 승인 게이트(이슈 #131, 옵트인). true면 각 성공 라운드 뒤(종합 발행 직전 포함)
    /// 다이제스트를 인박스로 배달하고 continue_discussion까지 대기한다.
    pub gate: bool,
}

/// driver 대기 파라미터. 테스트가 짧은 값으로 주입한다.
#[derive(Debug, Clone)]
pub struct DriverConfig {
    /// 좌석 하나의 응답 대기 상한(초). lease(30분)보다 훨씬 짧게 두어 requeue 재배달(이중 발언)을 차단.
    pub seat_timeout_secs: u64,
    /// get_task 폴링 간격(밀리초).
    pub poll_interval_ms: u64,
    /// 게이트 대기 표식 task의 lease 연장 주기(초). 워커 자동연장 관례(5분)와 동일, lease 30분보다
    /// 짧다. 테스트가 0으로 주입해 연장 경로를 통과시킨다.
    pub sentinel_extend_secs: u64,
}

impl Default for DriverConfig {
    fn default() -> Self {
        Self {
            seat_timeout_secs: 600,
            poll_interval_ms: 3000,
            sentinel_extend_secs: 300,
        }
    }
}

/// 라운드 간 사람 승인 게이트 상태(이슈 #131). registry(ActiveDiscussion)와 driver가 Arc로 공유한다.
#[derive(Debug, Clone, PartialEq)]
pub enum GateState {
    /// 게이트 비대기(라운드 진행 중 또는 gate 미사용 토론).
    Idle,
    /// 라운드 완료 후 사람 승인 대기(round/total = 다이제스트·에러 문구 표기용).
    Waiting { round: u32, total: u32 },
    /// continue_discussion 수신분(driver가 회수하며 Idle로 되돌린다).
    Proceed {
        steer: Option<String>,
        conclude: bool,
    },
}

/// try_begin이 driver에 넘기는 제어 핸들(취소 플래그 + 게이트 셀).
#[derive(Debug)]
pub struct DiscussionControl {
    pub cancel: Arc<AtomicBool>,
    pub gate: Arc<Mutex<GateState>>,
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
    gate: Arc<Mutex<GateState>>,
}

impl DiscussionRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// 새 토론을 점유한다. 이미 진행 중이면 Err(동시 1건 제한). 성공 시 driver 제어 핸들을 반환한다.
    pub fn try_begin(&self, id: &str) -> Result<DiscussionControl, String> {
        let mut active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(a) = active.as_ref() {
            // 게이트 대기는 타임아웃이 없어 잊힌 토론이 슬롯을 무기한 점유할 수 있다. 거부 문구에
            // 대기 상태와 해제 명령을 실어 발견·탈출 경로를 준다(설계 적대 검증 minor).
            let gate_hint = match *a.gate.lock().unwrap_or_else(|e| e.into_inner()) {
                GateState::Waiting { round, total } => format!(
                    " - 라운드 {round}/{total} 게이트 대기 중: continue_discussion 또는 stop_discussion으로 해제"
                ),
                _ => String::new(),
            };
            return Err(format!(
                "이미 진행 중인 토론이 있습니다: {}{gate_hint} (MVP=동시 1건, stop_discussion 후 재시도)",
                a.id
            ));
        }
        let cancel = Arc::new(AtomicBool::new(false));
        let gate = Arc::new(Mutex::new(GateState::Idle));
        *active = Some(ActiveDiscussion {
            id: id.to_string(),
            cancel: cancel.clone(),
            gate: gate.clone(),
        });
        Ok(DiscussionControl { cancel, gate })
    }

    /// 게이트 대기 중인 토론을 다음 단계로 진행시킨다(continue_discussion 경로). Waiting일 때만
    /// Proceed 전이하고 대기 중이던 (round, total)을 반환한다.
    pub fn continue_active(
        &self,
        id: &str,
        steer: Option<String>,
        conclude: bool,
    ) -> Result<(u32, u32), String> {
        let active = self.active.lock().unwrap_or_else(|e| e.into_inner());
        match active.as_ref() {
            Some(a) if a.id == id => {
                // stop 선행 후 continue가 오면: Proceed를 세워도 driver는 cancel을 먼저 보고 중단하므로
                // "진행됨" 성공 응답이 거짓이 된다. 여기서 거부한다(설계 적대 검증 minor).
                if a.cancel.load(Ordering::Relaxed) {
                    return Err("이미 중단 요청된 토론입니다(stop_discussion 선행)".to_string());
                }
                let mut gate = a.gate.lock().unwrap_or_else(|e| e.into_inner());
                match *gate {
                    GateState::Waiting { round, total } => {
                        *gate = GateState::Proceed { steer, conclude };
                        Ok((round, total))
                    }
                    _ => Err(
                        "게이트 대기 중이 아닙니다(라운드 진행 중이거나 gate 미사용 토론)"
                            .to_string(),
                    ),
                }
            }
            Some(a) => Err(format!("진행 중 토론 id 불일치: 현재={}", a.id)),
            None => Err(
                "진행 중인 토론이 없습니다(게이트 대기 중 브로커가 재기동되면 토론은 소멸합니다 - 재발의 필요)"
                    .to_string(),
            ),
        }
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

/// 게이트 다이제스트 본문(순수 함수, 문구 계약 테스트 대상). 선두 줄이 watch-results RESULT 프리뷰
/// (160자)에 그대로 실리므로 상태와 승인 주체를 앞세운다. 승인 주체=사람 문구는 계약이다: 인박스를
/// 소비하는 세션이 자율 규약(debate 발신=총괄발과 동일한 자율 수행)에 따라 스스로 continue를 부르면
/// 게이트가 무력화되므로, "사용자 지시가 있을 때만"을 본문에 못박는다(설계 적대 검증 major).
pub fn build_gate_digest(
    discussion_id: &str,
    round: u32,
    total: u32,
    same_round: &[Utterance],
) -> String {
    let mut out = format!(
        "[게이트] 라운드 {round}/{total} 완료·사람 승인 대기. 사용자에게 보고하고 지시가 있을 때만 continue_discussion(discussion_id=\"{discussion_id}\") 또는 stop_discussion을 호출하세요(자율 진행 금지)."
    );
    out.push_str("\n\n라운드 발언 요약:");
    for u in same_round {
        // 201자만 떠서 초과 여부까지 한 번의 순회로 판정한다(gemini: 이중 순회).
        let mut preview: String = u.content.chars().take(201).collect();
        let ellipsis = if preview.chars().count() > 200 {
            preview = preview.chars().take(200).collect();
            "…"
        } else {
            ""
        };
        out.push_str(&format!("\n- {}: {preview}{ellipsis}", u.speaker));
    }
    out.push_str(&format!(
        "\n\n조향: continue_discussion(steer=\"지시\")는 다음 라운드에 [사용자 조향 지시]로 주입 / conclude=true는 남은 라운드를 생략하고 종합 직행.\n전문: read_transcript(session_id=\"{DEBATE_NS_PREFIX}{discussion_id}\")"
    ));
    out
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

/// 좌석 task를 열린 채 두지 않기 위한 마감 전이(적대 리뷰: canceled는 watch-results가 배달하지
/// 않아 인박스 무통지가 되므로 **failed로 전이**한다 - terminal 가드라 늦은 완료 차단 효과는 동일).
/// try_fail이 지는 경합(그 사이 좌석이 완료)이면 완료 발언을 채택해 전사·인박스 모순을 없앤다.
async fn close_seat_task(
    store: &Arc<Mutex<SqliteStore>>,
    task_id: &str,
    reason: &str,
    marker: String,
) -> SeatOutcome {
    let tid = task_id.to_string();
    let reason_owned = reason.to_string();
    let failed = with_store(store, move |s| {
        let message_id = s.new_task_id()?;
        let msg = Message {
            message_id,
            role: "agent".to_string(),
            parts: vec![Part {
                text: Some(reason_owned),
                ..Default::default()
            }],
            task_id: Some(tid.clone()),
            context_id: None,
        };
        s.try_fail(&tid, Some(&msg), None)
    })
    .await;
    if failed.is_ok() {
        return SeatOutcome::Skipped(marker);
    }
    // 전이 불가 = 그 사이 terminal이 됨. completed였다면 발언을 채택(밀리초 경합 창).
    let tid = task_id.to_string();
    if let Ok(Some(task)) = with_store(store, move |s| s.get_task(&tid)).await
        && task.state == TaskState::Completed
    {
        let text = artifacts_text(&task);
        if !text.trim().is_empty() {
            return SeatOutcome::Utterance(text);
        }
    }
    SeatOutcome::Skipped(marker)
}

/// 게이트 대기의 산출(라운드 루프가 소비).
enum GateOutcome {
    /// continue_discussion 수신: steer(조향 지시)·conclude(남은 라운드 생략, 종합 직행).
    Proceed {
        steer: Option<String>,
        conclude: bool,
    },
    /// stop_discussion 수신(기존 중단 경로로 합류).
    Canceled,
    /// 다이제스트·표식 발행 실패(store 병증 클래스) - 전사 실패와 동급으로 토론을 중단시킨다.
    /// Waiting 무통지 대기는 금지이고, 자동 진행 폴백은 사용자가 명시한 승인 게이트를 조용히
    /// 제거하므로 채택하지 않는다(설계 적대 검증 major 판정).
    Abort(String),
}

/// driver 명의 task 발행 + 자가 claim(게이트 다이제스트·대기 표식 공용). from=debate:<id>,
/// to=DEBATE_DRIVER_AGENT라 watch-results 배달 대상이면서 좌석 소비 경로와는 무관하다.
async fn issue_driver_task(
    store: &Arc<Mutex<SqliteStore>>,
    ns: &str,
    text: &str,
) -> Result<String, String> {
    let from = ns.to_string();
    let body = text.to_string();
    with_store(store, move |s| {
        let message_id = s.new_task_id()?;
        let t = s.create_task_from_message(
            &from,
            DEBATE_DRIVER_AGENT,
            Message {
                message_id,
                role: "user".to_string(),
                parts: vec![Part {
                    text: Some(body),
                    ..Default::default()
                }],
                task_id: None,
                context_id: None,
            },
        )?;
        // claim 실패 시 방금 만든 submitted task를 정리하고 반환한다(안 하면 id를 잃은 열린 task가
        // 재기동 sweep까지 잔존 - CodeRabbit). 같은 락 안이라 경합 상대는 없고 store 병증 클래스다.
        if let Err(e) = s.try_claim(&t.id, Some(DEBATE_DRIVER_AGENT), Some(DEBATE_DRIVER_AGENT)) {
            let _ = s.try_fail(&t.id, None, None);
            return Err(format!("게이트 driver task claim 실패: {e}"));
        }
        Ok(t.id)
    })
    .await
}

/// driver 명의 task를 completed로 마감한다(artifact=본문 = watch-results RESULT 줄에 실림).
/// working이 아니면(lease 만료 requeue 등) 재claim 후 1회 재시도한다.
async fn complete_driver_task(
    store: &Arc<Mutex<SqliteStore>>,
    task_id: &str,
    artifact_text: String,
) -> Result<(), String> {
    let tid = task_id.to_string();
    with_store(store, move |s| {
        let artifact = Artifact {
            artifact_id: format!("gate-{tid}"),
            name: None,
            parts: vec![Part {
                text: Some(artifact_text),
                ..Default::default()
            }],
        };
        s.try_complete(
            &tid,
            std::slice::from_ref(&artifact),
            Some(DEBATE_DRIVER_AGENT),
        )
        .or_else(|_| {
            s.try_claim(&tid, Some(DEBATE_DRIVER_AGENT), Some(DEBATE_DRIVER_AGENT))?;
            s.try_complete(&tid, &[artifact], Some(DEBATE_DRIVER_AGENT))
        })
    })
    .await
}

/// driver 명의 task(표식·다이제스트)를 failed로 마감한다(중단·통지 실패 경로 = 인박스에 failed 통지).
/// failer=None(dispatcher 직접 경로)이라 lease 만료 requeue로 submitted가 된 task도 마감된다. 실패는
/// 로그만: 남은 열린 task는 다음 재기동의 고아 sweep이 정리한다.
async fn fail_driver_task(store: &Arc<Mutex<SqliteStore>>, task_id: &str, reason: &str) {
    let tid = task_id.to_string();
    let msg = reason.to_string();
    let r = with_store(store, move |s| {
        let message_id = s.new_task_id()?;
        s.try_fail(
            &tid,
            Some(&Message {
                message_id,
                role: "agent".to_string(),
                parts: vec![Part {
                    text: Some(msg),
                    ..Default::default()
                }],
                task_id: Some(tid.clone()),
                context_id: None,
            }),
            None,
        )
    })
    .await;
    if let Err(e) = r {
        eprintln!("[debate] 게이트 표식 실패 마감 실패(무시): {e}");
    }
}

/// 게이트 한 지점의 입력 묶음(gate_wait 인자 - clippy too_many_arguments 해소용 값 묶음).
struct GateRound {
    round: u32,
    total: u32,
    digest: String,
}

/// 라운드 간 사람 승인 게이트(#131). 순서가 계약이다: ① Waiting을 먼저 세워 "다이제스트를 보고 즉시
/// continue를 불렀는데 아직 대기 아님" 레이스를 제거 ② 전사 게이트 마커(다이제스트를 놓쳐도
/// read_transcript로 상태 재구성) ③ 대기 표식 task(자가 claim으로 working 유지 - 브로커 재기동 시
/// 고아 sweep이 failed로 마감 = 인박스 통지. 게이트 대기 중 재기동 침묵사를 기존 프리미티브로 봉합)
/// ④ 다이제스트 task(즉시 completed = watch-results push 통지) ⑤ continue/stop 폴링.
async fn gate_wait(
    store: &Arc<Mutex<SqliteStore>>,
    writer: &Arc<dyn TranscriptWriter>,
    ns: &str,
    gr: GateRound,
    control: &DiscussionControl,
    cfg: &DriverConfig,
) -> GateOutcome {
    // ns = `debate:<id>` 이므로 표기용 id는 프리픽스를 벗겨 얻는다(인자 수 절약).
    let discussion_id = ns.strip_prefix(DEBATE_NS_PREFIX).unwrap_or(ns);
    let GateRound {
        round,
        total,
        digest,
    } = gr;
    // Waiting을 먼저 세우므로 아래 발행 실패(Abort) 경로가 그 사이 도착한 Proceed를 덮어쓸 수 있다
    // (continue_discussion은 이미 성공을 반환한 상태). store 병증 + 밀리초 창의 동시 continue라는
    // 이중 조건이고 Abort 자체가 표식 failed·전사 마커로 통지되므로 수용하되, 폐기를 로그로 남긴다.
    let set_gate = |st: GateState| {
        let mut g = control.gate.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::mem::replace(&mut *g, st);
        if matches!(prev, GateState::Proceed { .. }) {
            eprintln!("[debate] 게이트 종료 경로가 도착해 있던 continue(Proceed)를 폐기합니다");
        }
    };
    set_gate(GateState::Waiting { round, total });
    if let Err(e) = append_transcript(
        writer,
        ns,
        "debate/driver",
        &format!("[게이트] 라운드 {round}/{total} 완료, 사람 승인 대기(continue_discussion)"),
    )
    .await
    {
        set_gate(GateState::Idle);
        return GateOutcome::Abort(format!("게이트 마커 기록 실패: {e}"));
    }
    // 문구 주의: 재기동만 단정하지 않는다 - 절전 등으로 lease 만료·재claim이 반복되면
    // expire_stale_claims의 attempt 상한 격리로도 이 본문이 실패 사유로 배달될 수 있다(코드 리뷰 minor).
    let sentinel_text = format!(
        "[게이트 대기 표식] 토론 {discussion_id} 라운드 {round}/{total} 사람 승인 대기. driver가 이 task를 더 유지하지 못하면(브로커 재기동 등) failed로 마감되어 인박스에 통지됩니다."
    );
    let sentinel = match issue_driver_task(store, ns, &sentinel_text).await {
        Ok(id) => id,
        Err(e) => {
            set_gate(GateState::Idle);
            return GateOutcome::Abort(format!("게이트 표식 발행 실패: {e}"));
        }
    };
    let digest_task = issue_driver_task(
        store,
        ns,
        &format!("[게이트 다이제스트] 라운드 {round}/{total}"),
    )
    .await;
    let digest_result = match &digest_task {
        Ok(id) => complete_driver_task(store, id, digest).await,
        Err(e) => Err(e.clone()),
    };
    if let Err(e) = digest_result {
        // 발행됐지만 완료 못한 다이제스트 task도 마감한다(안 하면 재기동 sweep까지 열린 채 잔존
        // = 헬스 노이즈, 코드 리뷰 minor).
        if let Ok(id) = &digest_task {
            fail_driver_task(store, id, "[debate] 게이트 다이제스트 완료 실패로 중단").await;
        }
        fail_driver_task(
            store,
            &sentinel,
            "[debate] 게이트 다이제스트 발행 실패로 중단",
        )
        .await;
        set_gate(GateState::Idle);
        return GateOutcome::Abort(format!("게이트 다이제스트 발행 실패: {e}"));
    }
    let mut last_extend = std::time::Instant::now();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(cfg.poll_interval_ms)).await;
        if control.cancel.load(Ordering::Relaxed) {
            fail_driver_task(store, &sentinel, "[debate] stop_discussion으로 게이트 중단").await;
            set_gate(GateState::Idle);
            return GateOutcome::Canceled;
        }
        let taken = {
            let mut g = control.gate.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*g, GateState::Proceed { .. }) {
                // Proceed 회수와 동시에 Idle 복귀(별도 reset 불요).
                Some(std::mem::replace(&mut *g, GateState::Idle))
            } else {
                None
            }
        };
        if let Some(GateState::Proceed { steer, conclude }) = taken {
            let note = if conclude {
                "[게이트 해제] 종합 직행"
            } else {
                "[게이트 해제] 진행"
            };
            // 표식 마감 실패는 로그만: 사람 승인이 이미 도착했으므로 토론은 계속한다(표식=통지 수단).
            if let Err(e) = complete_driver_task(store, &sentinel, note.to_string()).await {
                eprintln!("[debate {discussion_id}] 게이트 표식 마감 실패(무시): {e}");
            }
            return GateOutcome::Proceed { steer, conclude };
        }
        // 표식 lease 자동연장(워커 관례 답습). 만료 requeue로 submitted가 됐으면 재claim으로 복구.
        if last_extend.elapsed().as_secs() >= cfg.sentinel_extend_secs {
            last_extend = std::time::Instant::now();
            let sid = sentinel.clone();
            let r = with_store(store, move |s| {
                s.extend_lease(&sid, DEBATE_DRIVER_AGENT).or_else(|_| {
                    s.try_claim(&sid, Some(DEBATE_DRIVER_AGENT), Some(DEBATE_DRIVER_AGENT))
                })
            })
            .await;
            if let Err(e) = r {
                eprintln!("[debate {discussion_id}] 게이트 표식 lease 연장 실패(무시): {e}");
            }
        }
    }
}

/// 좌석 발언 상한(자). 프롬프트 조립 캡(prompt.rs MAX_ANSWER_LEN=4000)과 동치. 프리앰블이 이 상한을
/// 좌석에 지시하지만, 폭주 출력이 전사를 비대하게 만들지 않도록 회수 시점에도 강제한다(CodeRabbit).
const SEAT_UTTERANCE_MAX_CHARS: usize = 4000;

/// completed task의 artifact 텍스트를 모아 발언으로 만든다(상한 초과분은 절단 마커와 함께 잘라낸다).
fn artifacts_text(task: &crate::store::a2a::Task) -> String {
    let text = task
        .artifacts
        .iter()
        .flat_map(|a| a.parts.iter())
        .filter_map(|p| p.text.as_deref())
        .collect::<Vec<_>>()
        .join("\n");
    if text.chars().count() <= SEAT_UTTERANCE_MAX_CHARS {
        return text;
    }
    let mut capped: String = text.chars().take(SEAT_UTTERANCE_MAX_CHARS).collect();
    capped.push_str("\n[발언 상한 4000자 초과로 절단됨]");
    capped
}

/// 좌석 하나에 라운드 task를 발행하고 terminal까지 폴링 대기한다. 타임아웃·중단 시 task를 failed로
/// 마감(close_seat_task)해 열린 task를 남기지 않고, failed terminal이 인박스(watch-results) 통지가 된다.
async fn run_seat(
    store: &Arc<Mutex<SqliteStore>>,
    from_agent: &str,
    seat: &DiscussionSeat,
    text: String,
    cancel: &Arc<AtomicBool>,
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
        // stop_discussion 반응성: 폴 회차마다 확인해 좌석 타임아웃 상한이 아니라 폴 간격 안에
        // 점유가 풀리게 한다(적대 리뷰: stop 후 즉시 재시작이 최대 600초 차단되던 결함).
        if cancel.load(Ordering::Relaxed) {
            return close_seat_task(
                store,
                &task_id,
                "[debate] stop_discussion으로 중단",
                "[무응답: stop_discussion 중단]".to_string(),
            )
            .await;
        }
        let tid = task_id.clone();
        let task = match with_store(store, move |s| s.get_task(&tid)).await {
            Ok(Some(t)) => t,
            Ok(None) => {
                return SeatOutcome::Skipped(format!("[무응답: task {task_id} 소실]"));
            }
            Err(e) => {
                // 일시 DB 오류는 다음 폴에서 재시도(타임아웃이 상한). 상한 도달 시에도 task를 열린
                // 채 두지 않는다(열린 채 skip하면 다음 라운드와 이중 발언 경로가 생긴다).
                eprintln!("[debate] get_task 오류(재시도): {e}");
                if started.elapsed() >= deadline {
                    return close_seat_task(
                        store,
                        &task_id,
                        "[debate] driver 폴링 실패",
                        format!("[무응답: 폴링 실패 - {e}]"),
                    )
                    .await;
                }
                continue;
            }
        };
        match task.state {
            TaskState::Completed => {
                let text = artifacts_text(&task);
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
                    // 재배달(이중 발언) 차단 + 인박스 통지: failed로 마감한다. 이미 실행 중인
                    // 러너는 중단되지 않지만 늦은 complete는 terminal 가드에 막힌다.
                    return close_seat_task(
                        store,
                        &task_id,
                        &format!("[debate] 좌석 타임아웃 {}초", cfg.seat_timeout_secs),
                        format!("[무응답: {}초 타임아웃]", cfg.seat_timeout_secs),
                    )
                    .await;
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
    control: DiscussionControl,
    cfg: DriverConfig,
) {
    let cancel = control.cancel.clone();
    let ns = debate_ns(&spec.id);
    let _guard = FinishGuard {
        registry,
        id: spec.id.clone(),
    };
    // MCP 계층(resolve_seats·rounds clamp)이 보장하지만 driver는 pub API라 방어한다: 빈 좌석은
    // synthesizer의 seats[0] 인덱스 패닉 전에, 0라운드는 빈 종합 전에 조기 반환(봇 리뷰. 가드가
    // 점유를 해제한다).
    if spec.seats.is_empty() || spec.rounds == 0 {
        eprintln!(
            "[debate {}] 좌석 {}석·{}라운드: 시작 불가",
            spec.id,
            spec.seats.len(),
            spec.rounds
        );
        return;
    }
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
            let content = match run_seat(&store, &ns, seat, text, &cancel, &cfg).await {
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
        // 다이제스트는 same_round가 prior로 흡수되기 전에 조립한다(라운드 발언 요약이 내용).
        let digest = spec
            .gate
            .then(|| build_gate_digest(&spec.id, round, spec.rounds, &same_round));
        prior.extend(same_round);
        eprintln!("[debate {}] 라운드 {round}/{} 완료", spec.id, spec.rounds);
        // 라운드 간 사람 승인 게이트(#131, 옵트인). 전 좌석 실패(aborted) 경로는 위에서 이미 중단하므로
        // 성공 라운드 뒤에만 선다. 마지막 라운드 뒤에도 서서 synthesizer 발행을 사람이 승인한다
        // ("synthesizer도 라운드"라는 stop 계약과 일관).
        if let Some(digest) = digest {
            match gate_wait(
                &store,
                &writer,
                &ns,
                GateRound {
                    round,
                    total: spec.rounds,
                    digest,
                },
                &control,
                &cfg,
            )
            .await
            {
                GateOutcome::Proceed { steer, conclude } => {
                    if let Some(s) = steer {
                        // 조향은 전사 debate/user 턴 + prior 포함(순차-인지 그대로). 프리픽스로 피어
                        // 발언이 아니라 지시임을 명시한다(설계 적대 검증: prior는 화자 권위 구분이 없다).
                        let content = format!("[사용자 조향 지시] {s}");
                        if let Err(e) =
                            append_transcript(&writer, &ns, "debate/user", &content).await
                        {
                            eprintln!("[debate {}] 전사 기록 실패로 중단: {e}", spec.id);
                            return;
                        }
                        prior.push(Utterance::new("debate/user", content));
                    }
                    if conclude {
                        eprintln!("[debate {}] conclude: 남은 라운드 생략, 종합 직행", spec.id);
                        break 'rounds; // canceled/aborted 아님 → synthesizer로 진행.
                    }
                }
                GateOutcome::Canceled => {
                    canceled = true;
                    break 'rounds;
                }
                GateOutcome::Abort(reason) => {
                    let marker = format!("[토론 중단: 게이트 통지 실패 - {reason}]");
                    if let Err(e) = append_transcript(&writer, &ns, "debate/driver", &marker).await
                    {
                        eprintln!("[debate {}] 중단 마커 기록 실패(무시): {e}", spec.id);
                    }
                    eprintln!("[debate {}] {marker}", spec.id);
                    return;
                }
            }
        }
    }

    // stop이 마지막 좌석 실행 중에 들어온 경우: 좌석 루프는 자연 종료되므로 synthesizer 발행 전에
    // 재확인한다("이후 라운드는 발행되지 않습니다" 계약 - synthesizer도 라운드다. 적대 리뷰 major).
    if !canceled && cancel.load(Ordering::Relaxed) {
        canceled = true;
    }
    if canceled || aborted {
        let marker = if canceled {
            "[토론 중단: stop_discussion]"
        } else {
            "[토론 중단: 라운드 전 좌석 실패]"
        };
        if let Err(e) = append_transcript(&writer, &ns, "debate/driver", marker).await {
            eprintln!("[debate {}] 중단 마커 기록 실패(무시): {e}", spec.id);
        }
        eprintln!("[debate {}] {marker}", spec.id);
        return;
    }

    // synthesizer 라운드: 첫 좌석(v2-56 §8-5 기본값)에게 종합을 위임한다. driver는 러너가 없어 LLM
    // 생성을 스스로 못 한다. 실패 시 합의문 없이 종료(부분 전사가 남고, failed terminal이 곧 통지).
    // instruction은 비운다: 좌석별 입장 지시(예: 반대 견지)가 종합에 상속되면 합의문이 편향된다.
    let synth_seat = DiscussionSeat {
        role: Some("synthesizer".to_string()),
        instruction: String::new(),
        ..spec.seats[0].clone()
    };
    let text = build_seat_task_text(&synth_seat, &synthesis_topic(&spec.topic), &prior, &[]);
    let speaker = format!("debate/{}", synth_seat.label);
    match run_seat(&store, &ns, &synth_seat, text, &cancel, &cfg).await {
        SeatOutcome::Utterance(t) => match append_transcript(&writer, &ns, &speaker, &t).await {
            Ok(_) => eprintln!("[debate {}] 종합 완료(전사={ns})", spec.id),
            // 전사 실패해도 합의문 자체는 synthesizer task의 artifact로 남는다(get_task로 회수 가능).
            Err(e) => eprintln!(
                "[debate {}] 종합 전사 기록 실패: {e} (합의문은 task artifact로만 존재)",
                spec.id
            ),
        },
        SeatOutcome::Skipped(marker) => {
            if let Err(e) = append_transcript(
                &writer,
                &ns,
                "debate/driver",
                &format!("[종합 실패: {marker}]"),
            )
            .await
            {
                eprintln!("[debate {}] 종합 실패 마커 기록 실패(무시): {e}", spec.id);
            }
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
        let ctl = reg.try_begin("d1").expect("첫 점유는 성공");
        assert!(reg.try_begin("d2").is_err(), "동시 1건 제한");
        assert_eq!(reg.active_id().as_deref(), Some("d1"));
        // 다른 id cancel은 거부, 일치 id는 플래그 설정.
        assert!(reg.cancel("d2").is_err());
        reg.cancel("d1").expect("일치 id cancel");
        assert!(ctl.cancel.load(Ordering::Relaxed));
        // finish 후 재점유 가능.
        reg.finish("d1");
        assert!(reg.active_id().is_none());
        reg.try_begin("d2").expect("해제 후 점유");
    }

    #[test]
    fn registry_continue_requires_waiting_gate() {
        let reg = DiscussionRegistry::new();
        // 진행 중 토론 없음.
        assert!(reg.continue_active("dx", None, false).is_err());
        let ctl = reg.try_begin("d1").unwrap();
        // Idle(라운드 진행 중) = 거부.
        let err = reg.continue_active("d1", None, false).unwrap_err();
        assert!(err.contains("게이트 대기 중이 아닙니다"), "{err}");
        // id 불일치 = 거부.
        assert!(reg.continue_active("d2", None, false).is_err());
        // Waiting = Proceed 전이 + (round, total) 반환.
        *ctl.gate.lock().unwrap() = GateState::Waiting { round: 2, total: 3 };
        let (r, t) = reg
            .continue_active("d1", Some("조향".to_string()), true)
            .unwrap();
        assert_eq!((r, t), (2, 3));
        assert_eq!(
            *ctl.gate.lock().unwrap(),
            GateState::Proceed {
                steer: Some("조향".to_string()),
                conclude: true
            }
        );
        // 이미 Proceed(연타) = 거부.
        assert!(reg.continue_active("d1", None, false).is_err());
    }

    #[test]
    fn registry_continue_rejected_after_stop_and_begin_error_carries_gate_hint() {
        let reg = DiscussionRegistry::new();
        let ctl = reg.try_begin("d1").unwrap();
        *ctl.gate.lock().unwrap() = GateState::Waiting { round: 1, total: 2 };
        // 게이트 대기 중 새 토론 시작 거부 문구에 대기 상태·해제 경로 명시(발견성).
        let err = reg.try_begin("d2").unwrap_err();
        assert!(err.contains("게이트 대기 중"), "{err}");
        assert!(err.contains("continue_discussion"), "{err}");
        // stop 선행 후 continue = 거부(모순 응답 방지).
        reg.cancel("d1").unwrap();
        let err = reg.continue_active("d1", None, false).unwrap_err();
        assert!(err.contains("중단 요청"), "{err}");
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
            // 0 = 게이트 대기 매 폴 회차마다 표식 lease 연장 경로를 통과시킨다(코드 리뷰: 상수라
            // 테스트 도달 불가였던 분기).
            sentinel_extend_secs: 0,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_runs_rounds_sequential_aware_and_synthesizes() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("t1").unwrap();

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
            gate: false,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            control,
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
        let control = registry.try_begin("t2").unwrap();

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
            gate: false,
        };
        let cfg = DriverConfig {
            seat_timeout_secs: 1,
            poll_interval_ms: 20,
            sentinel_extend_secs: 0,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            control,
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
        // 죽은 좌석의 task는 failed로 마감됐다(열린 task 없음 + watch-results 배달 대상 = 인박스 통지).
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        let open = s.list_open_tasks_for("agent-dead").unwrap();
        assert!(open.is_empty(), "고아 task가 닫혀야 함");
        use crate::store::sqlite::ReplayLimit;
        let failed = s
            .list_tasks_replay(Some("debate:t2"), None, &["failed"], ReplayLimit::All)
            .unwrap();
        assert_eq!(failed.len(), 1, "타임아웃 좌석 task는 failed여야 함");
        let reason = failed[0]
            .status_message
            .as_ref()
            .and_then(|m| m.parts.first())
            .and_then(|p| p.text.as_deref())
            .unwrap_or_default();
        assert!(reason.contains("타임아웃"), "실패 사유 명시: {reason}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_rejects_empty_seats_without_panic() {
        // MCP 계층이 2~6석을 보장하지만 driver 단독 호출(pub API) 방어(gemini 리뷰).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("t5").unwrap();
        let spec = DiscussionSpec {
            id: "t5".to_string(),
            topic: "x".to_string(),
            seats: vec![],
            rounds: 1,
            gate: false,
        };
        run_discussion(spec, store, writer, registry.clone(), control, test_cfg()).await;
        assert!(
            cap.0.lock().unwrap().is_empty(),
            "빈 좌석은 전사 기록 없이 종료"
        );
        assert!(registry.active_id().is_none(), "점유 해제");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_rechecks_cancel_before_synthesizer() {
        // 마지막 좌석 실행 중 stop_discussion 시나리오: 좌석 완료 직후 cancel이 세워지면
        // synthesizer task를 발행하지 않고 중단 마커로 끝나야 한다(계약: 이후 라운드 발행 금지).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("t4").unwrap();

        let store2 = store.clone();
        let cancel2 = control.cancel.clone();
        let received = Arc::new(Mutex::new(0usize));
        let received2 = received.clone();
        let seat_task = tokio::spawn(async move {
            use crate::store::a2a::Artifact;
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                let s = store2.lock().unwrap_or_else(|e| e.into_inner());
                for t in s.list_open_tasks_for("agent-a").unwrap_or_default() {
                    if t.state == TaskState::Submitted
                        && s.try_claim(&t.id, Some("agent-a"), None).is_ok()
                    {
                        *received2.lock().unwrap() += 1;
                        let artifact = Artifact {
                            artifact_id: "art".to_string(),
                            name: None,
                            parts: vec![Part {
                                text: Some("마지막 발언".to_string()),
                                ..Default::default()
                            }],
                        };
                        s.try_complete(&t.id, &[artifact], Some("agent-a")).unwrap();
                        // 완료 직후 stop_discussion이 들어온 상황 재현.
                        cancel2.store(true, Ordering::Relaxed);
                    }
                }
            }
        });

        let spec = DiscussionSpec {
            id: "t4".to_string(),
            topic: "취소 재확인 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 1,
            gate: false,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_task.abort();

        let rows = cap.0.lock().unwrap();
        // user + seat-a 발언 + 중단 마커 = 3. synthesizer 미발행(수신 task 1건).
        assert_eq!(rows.len(), 3, "{rows:?}");
        assert!(rows[2].2.contains("stop_discussion"), "{rows:?}");
        assert_eq!(
            *received.lock().unwrap(),
            1,
            "synthesizer task가 발행되면 안 됨"
        );
    }

    #[test]
    fn gate_digest_wording_contract() {
        // 선두 줄 = watch-results RESULT 프리뷰(160자)에 실리는 계약: 상태·승인 주체(사람)·해제 명령.
        // "자율 진행 금지"는 인박스 소비 세션의 자율 규약(debate 발신=자율 수행)이 게이트를 무력화하지
        // 않게 하는 문구 계약이다(설계 적대 검증 major).
        let long = "발언".repeat(150); // 200자 초과 → 절단 마커.
        let same = vec![Utterance::new("debate/seat-a", long)];
        let d = build_gate_digest("abc123def456", 2, 3, &same);
        let first = d.lines().next().unwrap();
        assert!(
            first.chars().count() <= 160,
            "선두 줄 {}자 > 160",
            first.chars().count()
        );
        assert!(first.contains("라운드 2/3"));
        assert!(first.contains("사용자"));
        assert!(first.contains("자율 진행 금지"));
        assert!(first.contains("continue_discussion(discussion_id=\"abc123def456\")"));
        assert!(d.contains("debate/seat-a"));
        assert!(d.contains("…"), "200자 초과 절단 마커");
        assert!(d.contains("read_transcript(session_id=\"debate:abc123def456\")"));
        assert!(d.contains("conclude=true"));
    }

    /// 게이트 승인자 역할: gate 셀이 지정 라운드의 Waiting이 될 때까지 폴링한 뒤 continue_active를
    /// 부른다(공개 경로 = continue_discussion 툴과 동일).
    fn spawn_gate_approver(
        registry: Arc<DiscussionRegistry>,
        gate: Arc<Mutex<GateState>>,
        id: &'static str,
        round: u32,
        steer: Option<&'static str>,
        conclude: bool,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                let waiting = matches!(
                    *gate.lock().unwrap_or_else(|e| e.into_inner()),
                    GateState::Waiting { round: r, .. } if r == round
                );
                if waiting {
                    registry
                        .continue_active(id, steer.map(String::from), conclude)
                        .expect("continue_active");
                    return;
                }
            }
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_waits_and_plain_continue_proceeds_to_synthesizer() {
        // 마지막 라운드 뒤에도 게이트가 서고(synthesizer도 라운드), plain continue가 종합 발행으로
        // 이어진다(적대 검증: conclude 직행 테스트로 대체 불가한 별도 경로).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g1").unwrap();
        let gate_cell = control.gate.clone();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        let approver = spawn_gate_approver(registry.clone(), gate_cell, "g1", 1, None, false);

        let spec = DiscussionSpec {
            id: "g1".to_string(),
            topic: "게이트 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 1,
            gate: true,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_a.abort();
        approver.abort();

        let rows = cap.0.lock().unwrap();
        // user + r1 발언 + 게이트 마커 + 종합 = 4행.
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(
            rows[2].2.contains("[게이트] 라운드 1/1"),
            "게이트 마커: {rows:?}"
        );
        assert_eq!(rows[3].1, "debate/seat-a", "종합 발행됨");
        assert_eq!(recv_a.lock().unwrap().len(), 2, "r1 + 종합");
        // 게이트 task 2건(다이제스트+표식) 전부 completed로 마감, 열린 task 없음.
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        use crate::store::sqlite::ReplayLimit;
        let done = s
            .list_tasks_replay(Some("debate:g1"), None, &["completed"], ReplayLimit::All)
            .unwrap();
        let driver_tasks: Vec<_> = done
            .iter()
            .filter(|t| t.to_agent == DEBATE_DRIVER_AGENT)
            .collect();
        assert_eq!(driver_tasks.len(), 2, "다이제스트+표식: {driver_tasks:?}");
        let texts: Vec<String> = driver_tasks.iter().map(|t| artifacts_text(t)).collect();
        assert!(
            texts.iter().any(|t| t.contains("자율 진행 금지")),
            "다이제스트 문구 계약: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("[게이트 해제]")),
            "표식 해제 마감: {texts:?}"
        );
        assert!(
            s.list_open_tasks_for(DEBATE_DRIVER_AGENT)
                .unwrap()
                .is_empty(),
            "게이트 task 전부 마감"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_steer_lands_in_transcript_and_next_round_prompt() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g2").unwrap();
        let gate_cell = control.gate.clone();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        let ap1 = spawn_gate_approver(
            registry.clone(),
            gate_cell.clone(),
            "g2",
            1,
            Some("근거를 더 구체적으로"),
            false,
        );
        let ap2 = spawn_gate_approver(registry.clone(), gate_cell, "g2", 2, None, false);

        let spec = DiscussionSpec {
            id: "g2".to_string(),
            topic: "조향 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 2,
            gate: true,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_a.abort();
        ap1.abort();
        ap2.abort();

        let rows = cap.0.lock().unwrap();
        // user + r1 + 게이트1 + 조향 + r2 + 게이트2 + 종합 = 7행.
        assert_eq!(rows.len(), 7, "{rows:?}");
        assert_eq!(
            rows[3].1, "debate/user",
            "조향은 debate/user 화자: {rows:?}"
        );
        assert_eq!(rows[3].2, "[사용자 조향 지시] 근거를 더 구체적으로");
        let recv = recv_a.lock().unwrap();
        assert!(
            recv[1].contains("[사용자 조향 지시] 근거를 더 구체적으로"),
            "r2 프롬프트에 조향 주입: {}",
            recv[1]
        );
        assert!(
            recv[2].contains("[사용자 조향 지시]"),
            "종합 프롬프트에도 prior로 포함"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_conclude_skips_remaining_rounds_to_synthesis() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g3").unwrap();
        let gate_cell = control.gate.clone();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        let ap = spawn_gate_approver(
            registry.clone(),
            gate_cell,
            "g3",
            1,
            Some("이대로 충분, 종합"),
            true,
        );

        let spec = DiscussionSpec {
            id: "g3".to_string(),
            topic: "종합 직행 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 3,
            gate: true,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_a.abort();
        ap.abort();

        // 라운드 2·3 생략: 좌석 수신 = r1 + 종합 2건뿐.
        let recv = recv_a.lock().unwrap();
        assert_eq!(recv.len(), 2, "남은 라운드 생략");
        assert!(recv[1].contains("종합해주세요"), "종합 직행: {}", recv[1]);
        assert!(
            recv[1].contains("[사용자 조향 지시] 이대로 충분, 종합"),
            "steer+conclude 동시: 종합 프롬프트에 조향 반영"
        );
        // 전사: user + r1 + 게이트 마커 + 조향 + 종합 = 5행.
        let rows = cap.0.lock().unwrap();
        assert_eq!(rows.len(), 5, "{rows:?}");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_stop_during_wait_cancels_and_fails_sentinel() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g4").unwrap();
        let gate_cell = control.gate.clone();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        let reg2 = registry.clone();
        let stopper = tokio::spawn(async move {
            loop {
                tokio::time::sleep(std::time::Duration::from_millis(15)).await;
                let waiting = matches!(
                    *gate_cell.lock().unwrap_or_else(|e| e.into_inner()),
                    GateState::Waiting { .. }
                );
                if waiting {
                    reg2.cancel("g4").expect("stop");
                    return;
                }
            }
        });

        let spec = DiscussionSpec {
            id: "g4".to_string(),
            topic: "게이트 중 중단 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 2,
            gate: true,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_a.abort();
        stopper.abort();

        // 좌석 수신 1건(r1)뿐, 종합 미발행. 전사 = user + r1 + 게이트 마커 + 중단 마커.
        assert_eq!(recv_a.lock().unwrap().len(), 1);
        let rows = cap.0.lock().unwrap();
        assert_eq!(rows.len(), 4, "{rows:?}");
        assert!(rows[3].2.contains("stop_discussion"), "{rows:?}");
        // 표식은 failed로 마감(인박스에 중단 통지), 열린 task 없음.
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        use crate::store::sqlite::ReplayLimit;
        let failed = s
            .list_tasks_replay(Some("debate:g4"), None, &["failed"], ReplayLimit::All)
            .unwrap();
        assert_eq!(failed.len(), 1, "표식 1건만 failed: {failed:?}");
        let reason = failed[0]
            .status_message
            .as_ref()
            .and_then(|m| m.parts.first())
            .and_then(|p| p.text.as_deref())
            .unwrap_or_default();
        assert!(reason.contains("게이트 중단"), "{reason}");
        assert!(
            s.list_open_tasks_for(DEBATE_DRIVER_AGENT)
                .unwrap()
                .is_empty(),
            "게이트 task 전부 마감"
        );
        assert!(registry.active_id().is_none(), "점유 해제");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_stands_after_partial_failure_round_with_marker_in_digest() {
        // 일부 좌석만 실패한 라운드는 성공 라운드다(전 좌석 실패만 중단). 게이트가 서고 다이제스트에
        // [무응답] 마커 발언이 요약으로 실린다(코드 리뷰: 커버리지 공백).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g5").unwrap();
        let gate_cell = control.gate.clone();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());
        // agent-dead는 아무도 처리하지 않는다(타임아웃 skip 경로).
        let ap = spawn_gate_approver(registry.clone(), gate_cell, "g5", 1, None, false);

        let spec = DiscussionSpec {
            id: "g5".to_string(),
            topic: "부분 실패 게이트 테스트".to_string(),
            seats: vec![
                seat("agent-a", "seat-a", None, false),
                seat("agent-dead", "seat-dead", None, false),
            ],
            rounds: 1,
            gate: true,
        };
        let cfg = DriverConfig {
            seat_timeout_secs: 1,
            poll_interval_ms: 20,
            sentinel_extend_secs: 0,
        };
        run_discussion(spec, store.clone(), writer, registry.clone(), control, cfg).await;
        seat_a.abort();
        ap.abort();

        // 전사: user + seat-a + seat-dead([무응답]) + 게이트 마커 + 종합 = 5행.
        let rows = cap.0.lock().unwrap();
        assert_eq!(rows.len(), 5, "{rows:?}");
        assert!(rows[3].2.contains("[게이트] 라운드 1/1"), "{rows:?}");
        assert_eq!(rows[4].1, "debate/seat-a", "종합까지 진행");
        // 다이제스트 요약에 [무응답] 마커가 실린다.
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        use crate::store::sqlite::ReplayLimit;
        let done = s
            .list_tasks_replay(Some("debate:g5"), None, &["completed"], ReplayLimit::All)
            .unwrap();
        let digest = done
            .iter()
            .filter(|t| t.to_agent == DEBATE_DRIVER_AGENT)
            .map(artifacts_text)
            .find(|txt| txt.contains("[게이트]"))
            .expect("다이제스트 존재");
        assert!(
            digest.contains("[무응답"),
            "다이제스트에 skip 마커: {digest}"
        );
    }

    /// fail_from(0-기반) 번째 append부터 실패하는 writer(게이트 Abort 경로 검증용).
    struct FailingWriter {
        inner: Mutex<Vec<(String, String, String)>>,
        fail_from: usize,
    }

    impl TranscriptWriter for FailingWriter {
        fn append_turn(
            &self,
            session_id: &str,
            speaker: &str,
            content: &str,
        ) -> Result<u64, String> {
            let mut v = self.inner.lock().unwrap();
            if v.len() >= self.fail_from {
                return Err("주입된 전사 실패".to_string());
            }
            v.push((
                session_id.to_string(),
                speaker.to_string(),
                content.to_string(),
            ));
            Ok(v.len() as u64)
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn gate_marker_write_failure_aborts_without_leaks() {
        // 게이트 마커 기록 실패 = store 병증 클래스 → 토론 중단(Waiting 무통지 대기 금지 정책,
        // 자동 진행 폴백 비채택). 점유 해제·열린 task 무누수까지 확인(코드 리뷰: Abort 경로 미검증).
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        // append 순서: user(0)·r1 발언(1) 성공, 게이트 마커(2)부터 실패.
        let cap = Arc::new(FailingWriter {
            inner: Mutex::new(Vec::new()),
            fail_from: 2,
        });
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("g6").unwrap();

        let recv_a = Arc::new(Mutex::new(Vec::new()));
        let seat_a = spawn_fake_seat(store.clone(), "agent-a", "A", recv_a.clone());

        let spec = DiscussionSpec {
            id: "g6".to_string(),
            topic: "게이트 Abort 테스트".to_string(),
            seats: vec![seat("agent-a", "seat-a", None, false)],
            rounds: 2,
            gate: true,
        };
        run_discussion(
            spec,
            store.clone(),
            writer,
            registry.clone(),
            control,
            test_cfg(),
        )
        .await;
        seat_a.abort();

        // r1 이후 중단: 좌석 수신 1건(라운드 2·종합 미발행), 전사는 실패 전 2행뿐.
        assert_eq!(recv_a.lock().unwrap().len(), 1);
        assert_eq!(cap.inner.lock().unwrap().len(), 2);
        // 점유 해제(FinishGuard) + 게이트 마커 실패는 표식·다이제스트 발행 전이라 열린 task 없음.
        assert!(registry.active_id().is_none(), "점유 해제");
        let s = store.lock().unwrap_or_else(|e| e.into_inner());
        assert!(
            s.list_open_tasks_for(DEBATE_DRIVER_AGENT)
                .unwrap()
                .is_empty(),
            "게이트 task 무누수"
        );
        assert!(s.list_open_tasks_for("agent-a").unwrap().is_empty());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn driver_stops_issuing_after_cancel() {
        let store = Arc::new(Mutex::new(SqliteStore::open_memory().unwrap()));
        let cap = Arc::new(CapturingWriter(Mutex::new(Vec::new())));
        let writer: Arc<dyn TranscriptWriter> = cap.clone();
        let registry = Arc::new(DiscussionRegistry::new());
        let control = registry.try_begin("t3").unwrap();
        control.cancel.store(true, Ordering::Relaxed); // 시작 전 취소 = 첫 좌석 발행 전 중단.

        let spec = DiscussionSpec {
            id: "t3".to_string(),
            topic: "취소 테스트".to_string(),
            seats: vec![
                seat("agent-a", "seat-a", None, false),
                seat("agent-b", "seat-b", None, false),
            ],
            rounds: 3,
            gate: false,
        };
        run_discussion(
            spec,
            store.clone(),
            writer.clone(),
            registry.clone(),
            control,
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
