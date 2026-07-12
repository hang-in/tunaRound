// 총괄(dispatcher) 결과 인박스: 내가 던진 task가 완료/실패하면 그 결과를 알린다(책임의 이전 = 결과 push).
// 브로커의 /dashboard/events SSE(무인증)를 구독해 fromAgent==dispatcher인 terminal 이벤트만 골라 stdout에
// 한 줄로 낸다. 총괄 세션이 이 프로세스를 Monitor로 감싸면 "던지고 자리 떠도 결과가 깨우는" 구조가 된다.

use std::collections::{HashSet, VecDeque};

/// terminal dedup 집합의 상한. 라이브 버스는 terminal 이벤트를 task당 한 번만 흘리므로 dedup은
/// 방어 장치다 - 최근 창만 유지하면 충분하고, 상한 없이는 장기 상주 시 무한 성장한다(리뷰 지적).
/// 주간 task 약 100건 실측 대비 수개월분 여유.
const SEEN_CAP: usize = 4096;

/// 상한이 있는 terminal dedup 집합: 초과 시 가장 오래 기억한 id부터 잊는다(FIFO).
struct SeenSet {
    set: HashSet<String>,
    order: VecDeque<String>,
}

impl SeenSet {
    fn new() -> Self {
        Self {
            set: HashSet::new(),
            order: VecDeque::new(),
        }
    }

    /// 새 id면 기억하고 true, 이미 본 id면 false. 상한 초과분은 오래된 것부터 방출한다.
    fn insert(&mut self, id: &str) -> bool {
        if !self.set.insert(id.to_string()) {
            return false;
        }
        self.order.push_back(id.to_string());
        while self.order.len() > SEEN_CAP {
            if let Some(old) = self.order.pop_front() {
                self.set.remove(&old);
            }
        }
        true
    }
}

/// 워터마크 상태 파일명(dispatcher별). 정제는 codex_inject의 thread 파일 정제 답습: 정상 id
/// (영숫자+`-`+`_`)는 불변, `/`·`..` 같은 경로 문자는 `_` 치환(네임스페이스 밖 탈출 방지).
/// 빈 dispatcher(전체 관측)는 "all".
fn watermark_file_name(dispatcher: &str) -> String {
    let safe: String = dispatcher
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    let safe = if safe.is_empty() {
        "all".to_string()
    } else {
        safe
    };
    format!("watch-results-{safe}.since")
}

/// 워터마크 상태 디렉터리. 설계 §4 P3 명시 경로 = `%LOCALAPPDATA%/tunaround`(win 브로커 db·bin과
/// 같은 안정 네임스페이스). LOCALAPPDATA가 없는 맥/리눅스는 기존 상태 파일 관례(`~/.tunaround`,
/// codex-sup thread 파일)로 폴백. 둘 다 없으면 None = 영속 불가(메모리 워터마크만으로 동작).
fn state_dir() -> Option<std::path::PathBuf> {
    if let Some(lad) = std::env::var_os("LOCALAPPDATA")
        && !lad.is_empty()
    {
        return Some(std::path::PathBuf::from(lad).join("tunaround"));
    }
    let home = crate::config::expand_home("~/.tunaround");
    if home.starts_with("~/") {
        return None; // HOME/USERPROFILE 미설정 = 확장 실패
    }
    Some(std::path::PathBuf::from(home))
}

/// DB datetime 포맷("YYYY-MM-DD HH:MM:SS" UTC, §5-3 고정 계약) 형태 검사. 워터마크는 서버가 준
/// updatedAt만 쓰지만, 상태 파일 오염·--since 오입력이 사전순 비교를 왜곡하지 않게 형태를 거른다.
fn is_db_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 19 {
        return false;
    }
    b.iter().enumerate().all(|(i, &c)| match i {
        4 | 7 => c == b'-',
        10 => c == b' ',
        13 | 16 => c == b':',
        _ => c.is_ascii_digit(),
    })
}

/// 쿼리 값 percent-encoding(RFC 3986 unreserved만 통과). since의 공백·콜론을 안전하게 실어 보낸다
/// (서버 percent_decode와 왕복 대칭).
fn encode_query_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// (재)접속 URL 조립(v2-45 P3). 워터마크가 있으면 `?since=`로 서버 재생을 요청해 다운 중 완료분이
/// 라이브 스트림 앞에 chain되어 온다(클라이언트 파서 무변경 원칙). 없으면(콜드스타트·상태 파일 없음)
/// 현행 무파라미터 구독 = 재생 없음(과거 폭주 방지). dispatcher가 빈 값이면 파라미터를 생략해
/// "전체 관측" 의미를 서버 질의까지 유지한다.
fn build_events_url(core: &str, dispatcher: &str, watermark: Option<&str>) -> String {
    let base = format!("{}/dashboard/events", core.trim_end_matches('/'));
    let Some(ts) = watermark else {
        return base;
    };
    let mut url = format!("{base}?since={}", encode_query_value(ts));
    if !dispatcher.is_empty() {
        url.push_str("&dispatcher=");
        url.push_str(&encode_query_value(dispatcher));
    }
    url
}

/// 워터마크 전진(사전순 max, DB datetime 포맷은 사전순=시간순). 서버가 준 포맷 그대로의 값만
/// 채택하고 형태가 다른 값은 버린다(사전순 비교 오염 방어). 로컬 시계는 절대 쓰지 않는다(§5-3).
fn advance_watermark(current: &mut Option<String>, candidate: &str) {
    if !is_db_datetime(candidate) {
        return;
    }
    match current {
        Some(cur) if cur.as_str() >= candidate => {}
        _ => *current = Some(candidate.to_string()),
    }
}

/// 워터마크 상태 파일(프로세스 재시작을 넘는 영속). 기록 시점 정책 = "pending까지 전부 stdout에
/// 나간 순간"만(persist_if_drained 참조) - 기록된 워터마크 뒤에 "아직 안 알린" 결과가 없다는
/// 불변식을 지켜, digest 묶음 중 크래시 시 유실 대신 중복 통지를 택한다(인박스 = at-least-once).
struct WatermarkFile {
    /// None = 영속 불가 환경(LOCALAPPDATA/HOME 없음) → 메모리 워터마크만으로 동작.
    path: Option<std::path::PathBuf>,
    /// 마지막 기록값(같으면 재기록 생략). 기록 빈도 근거: 종결 이벤트는 저빈도(주간 task 약 100건
    /// 실측)라 값이 바뀔 때마다 기록해도 IO 부담이 무시 가능하고, 유실 창 최소화가 우선이라
    /// 주기 배치·지연 기록은 두지 않는다.
    last_written: Option<String>,
}

impl WatermarkFile {
    fn at(path: Option<std::path::PathBuf>) -> Self {
        Self {
            path,
            last_written: None,
        }
    }

    /// 파일에서 워터마크를 읽는다. 없거나 형태 불량이면 None(재생 없이 라이브부터).
    fn load(&mut self) -> Option<String> {
        let path = self.path.as_ref()?;
        let raw = std::fs::read_to_string(path).ok()?;
        let ts = raw.trim().to_string();
        if is_db_datetime(&ts) {
            self.last_written = Some(ts.clone());
            Some(ts)
        } else {
            eprintln!(
                "[watch-results] 상태 파일 워터마크 형식 불량({}) - 무시하고 라이브부터",
                path.display()
            );
            None
        }
    }

    /// 워터마크를 기록한다. 임시 파일 write 후 rename(부분 쓰기로 파일이 깨지는 것 방지,
    /// mesh.pids rename-swap 답습). 실패는 best-effort로 삼킨다(기록 실패가 통지 기능을
    /// 멈출 이유는 아니고, 다음 전진 때 재시도된다). 영속 워터마크는 **단조**다: 마지막 기록값보다
    /// 새 값(사전순=시간순)만 쓴다. --since로 과거 값부터 시작해도 디스크의 더 새 워터마크를 뒤로
    /// 되감지 않는다(리뷰 이월: 재생은 인메모리 워터마크로 수행하고 영속 floor는 낮추지 않아 재시작
    /// 시 과거 재생 폭주를 방지).
    fn persist(&mut self, watermark: &Option<String>) {
        let (Some(path), Some(ts)) = (self.path.as_ref(), watermark.as_ref()) else {
            return;
        };
        // 마지막 기록값 이하(같거나 과거)면 재기록하지 않는다(단조 보장 + 같은 값 IO 절약).
        // is_some_and = 이 파일의 기존 관용구(connection_was_healthy와 동형, gemini 리뷰 반영).
        if self
            .last_written
            .as_deref()
            .is_some_and(|lw| ts.as_str() <= lw)
        {
            return;
        }
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let tmp = path.with_extension("since.tmp");
        if std::fs::write(&tmp, ts).is_ok() && std::fs::rename(&tmp, path).is_ok() {
            self.last_written = Some(ts.clone());
        }
    }
}

/// task 스냅샷에서 결과 텍스트를 뽑는다: completed=artifact 텍스트, 그 외=statusMessage 텍스트.
fn extract_result_text(task: &serde_json::Value) -> String {
    let from_artifact = task
        .get("artifacts")
        .and_then(|a| a.as_array())
        .and_then(|a| a.first())
        .and_then(|a| a.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str());
    let from_status = task
        .get("statusMessage")
        .and_then(|m| m.get("parts"))
        .and_then(|p| p.as_array())
        .and_then(|p| p.first())
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str());
    match from_artifact.or(from_status) {
        // \r도 제거한다(터미널에서 \r은 커서를 줄 앞으로 보내 기존 출력을 덮어쓴다).
        Some(t) => t
            .replace('\r', "")
            .replace('\n', " ")
            .chars()
            .take(160)
            .collect(),
        None => "(내용 없음)".to_string(),
    }
}

/// parse_result_line의 산출: stdout 한 줄 + digest 분기 정보 + 워터마크 후보.
struct ResultLine {
    line: String,
    /// failed 여부(--digest 분기용: failed=즉시 알림 / completed=묶음 가능, v2-44 W5).
    is_failed: bool,
    /// 서버가 준 task.updatedAt(워터마크 후보, §5-3: 이 값만 워터마크로 쓴다 - 로컬 시계 금지).
    updated_at: Option<String>,
}

/// SSE data 한 줄(`{"event":..,"task":{..}}`)을 파싱해, dispatcher가 던진 terminal(completed/failed) task면
/// [`ResultLine`]을 만든다. 이미 본 task(seen)·비-terminal·다른 dispatcher는 None.
/// dispatcher가 빈 문자열이면 fromAgent 필터를 끈다(전체 완료 관측).
fn parse_result_line(data: &str, dispatcher: &str, seen: &mut SeenSet) -> Option<ResultLine> {
    let v: serde_json::Value = serde_json::from_str(data.trim()).ok()?;
    let task = v.get("task")?;
    let state = task.get("state")?.as_str()?;
    if state != "completed" && state != "failed" {
        return None;
    }
    let from = task.get("fromAgent").and_then(|x| x.as_str()).unwrap_or("");
    if !dispatcher.is_empty() && from != dispatcher {
        return None;
    }
    let id = task.get("id")?.as_str()?;
    if !seen.insert(id) {
        return None; // 같은 task terminal은 한 번만
    }
    let to = task.get("toAgent").and_then(|x| x.as_str()).unwrap_or("?");
    let short: String = id.chars().take(8).collect();
    Some(ResultLine {
        line: format!(
            "RESULT {short} {state} <- {to} :: {}",
            extract_result_text(task)
        ),
        is_failed: state == "failed",
        updated_at: task
            .get("updatedAt")
            .and_then(|x| x.as_str())
            .map(str::to_string),
    })
}

/// 재접속을 포기하기 전까지 허용하는 연속 실패 횟수. 초과 시 run()이 Err를 반환해 호출부가
/// exit(1)하게 한다(주소 오타 같은 영구 실패를 조용히 삼키지 않고 Monitor가 죽음을 알게 하는 정책).
const MAX_CONSECUTIVE_FAILURES: u32 = 20;

/// 재접속 지수 백오프 대기 시간(초): 연속 실패 1회=1s, 이후 2배씩(2→4→8→16), 상한 30s.
fn backoff_secs(consecutive_failures: u32) -> u64 {
    // 2^5=32는 상한 30을 넘으므로 지수를 5에서 멈추고 min으로 자른다(0회는 방어적으로 1s).
    let exp = consecutive_failures.saturating_sub(1).min(5);
    (1u64 << exp).min(30)
}

/// 실패 연쇄 리셋에 필요한 접속 최소 생존 시간(초). 2xx 수립만으로 리셋하면 "수립 직후 즉시
/// 닫히는" 브로커(크래시루프, 200 후 빈 바디를 주는 오설정 엔드포인트)가 카운터를 영원히
/// 리셋해 포기(exit 1)가 불가능해지므로, 이 시간 이상 살았던 접속만 건강했던 것으로 본다.
const MIN_HEALTHY_SECS: u64 = 30;

/// 실패 연쇄 리셋 판정: 2xx 스트림 수립 후(None=수립 실패) 최소 생존 시간을 넘긴 접속만 "건강했다".
/// 생존 시간은 수립 시점부터 잰다(접속 수립에 쓴 핸드셰이크 시간을 생존으로 오산하지 않게, 리뷰 반영).
fn connection_was_healthy(lived_after_connect: Option<std::time::Duration>) -> bool {
    lived_after_connect
        .is_some_and(|lived| lived >= std::time::Duration::from_secs(MIN_HEALTHY_SECS))
}

/// 재접속을 넘어 유지되는 인박스 상태(재접속 루프 바깥 소유): terminal dedup(seen)·digest 묶음(pending)·
/// flush 예정 시각·워터마크. 접속이 끊겨도 "이미 알린 task"와 "아직 못 알린 묶음"을 잃지 않는다.
struct InboxState {
    seen: SeenSet,
    pending: Vec<String>,
    flush_at: Option<tokio::time::Instant>,
    /// 이 프로세스가 통지한 task들의 서버 updatedAt 최대값(v2-45 P3). 재접속 URL의 since로 쓴다.
    /// 서버 질의는 `updated_at >= since`(P2 확정)라 경계 task가 재전달되는데, 프로세스 생존 중엔
    /// seen이 dedup하고, 재시작 후엔 seen이 비어 경계 1건(같은 초의 여러 건 포함)이 한 번 더
    /// 통지될 수 있다 - 인박스 특성상 무해(유실보다 중복 우선). 억제하려면 상태 파일에 경계
    /// task id들을 동봉하면 되지만 단순성 우선으로 비채택.
    watermark: Option<String>,
    /// 워터마크 영속(상태 파일). 기록 시점은 persist_if_drained가 관장한다.
    file: WatermarkFile,
}

impl InboxState {
    /// "알릴 것을 전부 알렸다"가 성립하는 순간에만 워터마크를 영속한다. pending이 남아 있으면
    /// (digest 묶음 대기 중) 기록을 미뤄서, 그 사이 크래시해도 재시작 재생이 미출력 구간을 다시
    /// 가져오게 한다(기록된 워터마크 뒤에 미통지 결과가 없다는 불변식 = 유실 대신 중복).
    fn persist_if_drained(&mut self) {
        if self.pending.is_empty() {
            self.file.persist(&self.watermark);
        }
    }
}

/// digest로 묶인 completed 라인들을 한 번에 stdout으로 내보낸다(출력 burst 1회 = 총괄 wake 1회).
fn flush_pending(pending: &mut Vec<String>) {
    use std::io::Write;
    if pending.is_empty() {
        return;
    }
    for line in pending.drain(..) {
        println!("{line}");
    }
    let _ = std::io::stdout().flush();
}

/// SSE 접속 1회분: 접속해 끊길 때까지 이벤트를 처리하고, 단절 사유를 돌려준다. 서버가 잘린
/// 재생 스냅샷 후 스트림을 정상 종료하는 catch-up 경로(P3 서버 계약)도 "스트림 종료" 단절로
/// 돌아와 호출부 재접속 루프가 전진한 워터마크로 이어받는다.
/// 2xx 스트림 수립에 성공하면 *connected_at=수립 시점(호출부가 순수 생존 시간으로 실패 카운터
/// 리셋을 판정할 근거), 결과를 1건이라도 통지하면 *progressed=true(catch-up 연쇄 판정 근거).
/// state(seen·pending·flush_at·watermark)는 호출부(재접속 루프) 소유라 재접속을 넘어 유지된다.
async fn run_once(
    client: &reqwest::Client,
    url: &str,
    dispatcher: &str,
    digest_secs: u64,
    state: &mut InboxState,
    connected_at: &mut Option<tokio::time::Instant>,
    progressed: &mut bool,
) -> String {
    use futures_util::StreamExt;
    use std::io::Write;
    let resp = match client.get(url).send().await {
        Ok(r) => r,
        Err(e) => return format!("SSE 접속 실패({url}): {e}"),
    };
    if !resp.status().is_success() {
        return format!("SSE 상태 {}", resp.status());
    }
    *connected_at = Some(tokio::time::Instant::now());
    eprintln!("[watch-results] {url} 구독 시작 (dispatcher={dispatcher}, digest={digest_secs}s)");
    // 버퍼는 Vec<u8>로 유지한다. 청크마다 UTF-8 변환하면 멀티바이트 문자(한글 등)가 청크 경계에서
    // 깨지므로(U+FFFD 영구 손실), 개행(\n=ASCII)으로 완결된 라인만 변환한다.
    // 접속마다 새로 시작한다(끊긴 접속의 반쪽 라인을 새 스트림에 이어 붙이면 오염).
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = resp.bytes_stream();
    loop {
        tokio::select! {
            // digest 마감: 묶인 completed를 한 번에 내보낸다(출력 burst 1회 = 총괄 wake 1회).
            _ = async { tokio::time::sleep_until(state.flush_at.unwrap()).await }, if state.flush_at.is_some() => {
                flush_pending(&mut state.pending);
                state.flush_at = None;
                state.persist_if_drained(); // 묶음이 전부 나간 순간 = 워터마크 영속 시점
            }
            chunk = stream.next() => {
                let Some(chunk) = chunk else {
                    return "SSE 스트림이 종료됨".to_string();
                };
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => return format!("스트림 오류: {e}"),
                };
                buf.extend_from_slice(&chunk);
                while let Some(pos) = buf.iter().position(|&b| b == b'\n') {
                    let line_bytes: Vec<u8> = buf.drain(..=pos).collect();
                    // 라인은 \n에서 끝나므로 완결된 UTF-8(문자 중간에서 안 잘림) → lossy여도 손실 없음.
                    let line = String::from_utf8_lossy(&line_bytes);
                    let Some(data) = line.trim_end().strip_prefix("data: ") else {
                        continue;
                    };
                    let Some(result) = parse_result_line(data, dispatcher, &mut state.seen) else {
                        continue;
                    };
                    *progressed = true;
                    // 워터마크는 파싱 즉시 전진(재접속 URL용 - 단절 시 호출부가 pending을 먼저
                    // flush하므로 "전진한 워터마크로 재접속" 시점엔 전부 출력돼 있다). 영속은
                    // persist_if_drained가 별도로 관장한다(출력 완료 시점만).
                    if let Some(ts) = result.updated_at.as_deref() {
                        advance_watermark(&mut state.watermark, ts);
                    }
                    if digest_secs > 0 && !result.is_failed {
                        state.pending.push(result.line);
                        if state.flush_at.is_none() {
                            state.flush_at = Some(tokio::time::Instant::now() + std::time::Duration::from_secs(digest_secs));
                        }
                    } else {
                        println!("{}", result.line);
                        let _ = std::io::stdout().flush();
                        state.persist_if_drained();
                    }
                }
            }
        }
    }
}

/// 브로커 SSE를 구독해 dispatcher의 완료/실패 결과를 stdout으로 흘린다. 단절(접속 실패·비2xx·
/// 스트림 종료·청크 오류) 시 pending을 flush한 뒤 지수 백오프(1s→30s 상한)로 재접속한다
/// (브로커 재기동을 넘어 생존, v2-45 P1). 연속 MAX_CONSECUTIVE_FAILURES회 초과 실패 시에만
/// Err로 종료해 호출부가 exit(1)하게 한다(주소 오타 같은 영구 실패는 Monitor가 죽음으로 알게).
/// digest_secs>0이면 completed는 그 구간 동안 묶어 한 번에 낸다(총괄 wake 절약, v2-44 W5).
/// failed는 digest와 무관하게 즉시 낸다(막힌 task는 총괄 판단이 급하다).
///
/// v2-45 P3: (재)접속 때 워터마크가 있으면 `?since=`로 구독해 인박스 다운 중 완료된 task를
/// 서버 재생으로 선행 수신한다(통지 유실 해소). 워터마크 초기값 = --since 오버라이드 > 상태 파일 >
/// 없음(재생 없이 라이브부터).
pub async fn run(
    core: &str,
    dispatcher: &str,
    digest_secs: u64,
    since_override: Option<&str>,
) -> Result<(), String> {
    // connect timeout만 둔다(SSE 바디는 무한정 열려 있어야 하므로 전체 요청 timeout은 두지 않는다).
    // TCP는 붙었는데 응답이 없는 상황(방화벽 drop)에서 send가 무한 대기하는 것을 막는다.
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| format!("watch-results: 클라이언트 구성 실패: {e}"))?;
    let mut file = WatermarkFile::at(state_dir().map(|d| d.join(watermark_file_name(dispatcher))));
    if file.path.is_none() {
        eprintln!(
            "[watch-results] 상태 디렉터리 없음(LOCALAPPDATA/HOME 미설정) - 워터마크는 메모리에만 유지"
        );
    }
    let watermark = match since_override {
        // --since 수동 오버라이드(상태 파일보다 우선). 'T'/'Z'는 서버 파서와 같은 규칙으로 정규화.
        Some(raw) => {
            let norm = raw.replace('T', " ").trim_end_matches('Z').to_string();
            if !is_db_datetime(&norm) {
                return Err(format!(
                    "watch-results: --since 형식은 \"YYYY-MM-DD HH:MM:SS\"(UTC, 'T' 구분자 허용)이어야 합니다: {raw:?}"
                ));
            }
            // 오버라이드는 인메모리 재생 시작점으로만 쓴다. 디스크 워터마크를 읽어 영속 floor
            // (last_written)만 채워, 단조 persist가 더 새 저장값을 오버라이드로 되감지 않게 한다.
            let _ = file.load();
            Some(norm)
        }
        None => file.load(),
    };
    // seen(dedup)·digest pending·워터마크는 재접속을 넘어 유지한다(루프 바깥 소유, v2-45 P1/P3).
    let mut state = InboxState {
        seen: SeenSet::new(),
        pending: Vec::new(),
        flush_at: None,
        watermark,
        file,
    };
    let mut consecutive_failures: u32 = 0;
    loop {
        // 매 접속마다 현재 워터마크로 URL을 다시 조립한다(재접속 = 전진한 워터마크부터 재생).
        let url = build_events_url(core, dispatcher, state.watermark.as_deref());
        let mut connected_at: Option<tokio::time::Instant> = None;
        let mut progressed = false;
        let reason = run_once(
            &client,
            &url,
            dispatcher,
            digest_secs,
            &mut state,
            &mut connected_at,
            &mut progressed,
        )
        .await;
        // 모든 단절 경로(접속 실패·비2xx·스트림 종료·청크 오류)에서 pending을 먼저 flush한다
        // (digest 묶음 유실 방지). flush했으니 예정 시각도 지우고 워터마크를 영속한다.
        flush_pending(&mut state.pending);
        state.flush_at = None;
        state.persist_if_drained();
        // 실패 연쇄 리셋: 최소 생존을 넘긴 접속 외에, 결과를 1건이라도 실어 나른 접속도 건강으로
        // 본다(P3 catch-up 연쇄: 서버가 잘린 스냅샷 후 정상 종료 → 30초 미만 접속이 반복되는데,
        // 진전이 있는 한 실패 예산을 태우지 않고 백오프도 1s로 유지해 빠르게 따라잡는다).
        // 진전 없는 단절은 기존 판정 그대로 = 영구 장애 포기(exit 1)는 여전히 가능.
        if connection_was_healthy(connected_at.map(|at| at.elapsed())) || progressed {
            consecutive_failures = 0;
        }
        consecutive_failures += 1;
        if consecutive_failures > MAX_CONSECUTIVE_FAILURES {
            return Err(format!(
                "watch-results: 연속 {consecutive_failures}회 접속 실패, 재접속 포기(마지막 사유: {reason})"
            ));
        }
        let wait = backoff_secs(consecutive_failures);
        // 재접속 시도·사유는 stderr에만 기록한다(stdout은 RESULT 라인 계약 전용).
        eprintln!(
            "[watch-results] 단절: {reason} → {wait}s 후 재접속 (연속 실패 {consecutive_failures}/{MAX_CONSECUTIVE_FAILURES})"
        );
        tokio::time::sleep(std::time::Duration::from_secs(wait)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(state: &str, from: &str, to: &str, id: &str, artifact: Option<&str>) -> String {
        let art = match artifact {
            Some(t) => serde_json::json!([{ "parts": [{ "text": t }] }]),
            None => serde_json::json!([]),
        };
        serde_json::json!({
            "event": if state == "completed" { "completed" } else { "status" },
            "task": { "id": id, "state": state, "fromAgent": from, "toAgent": to, "artifacts": art,
                      "updatedAt": "2026-07-11 09:00:00" }
        })
        .to_string()
    }

    #[test]
    fn completed_from_dispatcher_yields_result() {
        let mut seen = SeenSet::new();
        let r = parse_result_line(
            &ev(
                "completed",
                "dashboard",
                "mac-claude-sup",
                "abc12345xyz",
                Some("발견 6건"),
            ),
            "dashboard",
            &mut seen,
        );
        let r = r.unwrap();
        assert!(r.line.contains("RESULT abc12345"));
        assert!(r.line.contains("completed"));
        assert!(r.line.contains("mac-claude-sup"));
        assert!(r.line.contains("발견 6건"));
        assert!(!r.is_failed, "completed는 digest 묶음 대상");
        assert_eq!(
            r.updated_at.as_deref(),
            Some("2026-07-11 09:00:00"),
            "서버 updatedAt = 워터마크 후보"
        );
    }

    #[test]
    fn failed_yields_result_with_status_text() {
        let mut seen = SeenSet::new();
        let data = serde_json::json!({
            "event": "status",
            "task": { "id": "f00d", "state": "failed", "fromAgent": "dashboard", "toAgent": "mac-claude-sup",
                      "artifacts": [], "statusMessage": { "parts": [{ "text": "BLOCKED: discover 없음" }] } }
        }).to_string();
        let r = parse_result_line(&data, "dashboard", &mut seen).unwrap();
        assert!(r.line.contains("failed"));
        assert!(r.line.contains("BLOCKED"));
        assert!(r.is_failed, "failed는 digest 무관 즉시 알림");
        assert_eq!(
            r.updated_at, None,
            "updatedAt 없는 이벤트는 워터마크 후보 없음(전진 안 함)"
        );
    }

    #[test]
    fn non_terminal_and_other_dispatcher_filtered() {
        let mut seen = SeenSet::new();
        assert!(
            parse_result_line(
                &ev("working", "dashboard", "x", "1", None),
                "dashboard",
                &mut seen
            )
            .is_none()
        );
        assert!(
            parse_result_line(
                &ev("completed", "other", "x", "2", None),
                "dashboard",
                &mut seen
            )
            .is_none()
        );
    }

    #[test]
    fn same_task_terminal_reported_once() {
        let mut seen = SeenSet::new();
        let e = ev("completed", "dashboard", "x", "dup1", Some("r"));
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_some());
        assert!(parse_result_line(&e, "dashboard", &mut seen).is_none()); // 두 번째는 dedup
    }

    #[test]
    fn backoff_grows_exponentially_to_cap() {
        // 계약(v2-45 P1): 1s → 2 → 4 → 8 → 16 → 30 상한, 이후 30 유지.
        assert_eq!(backoff_secs(1), 1);
        assert_eq!(backoff_secs(2), 2);
        assert_eq!(backoff_secs(3), 4);
        assert_eq!(backoff_secs(4), 8);
        assert_eq!(backoff_secs(5), 16);
        assert_eq!(backoff_secs(6), 30);
        assert_eq!(backoff_secs(7), 30);
        assert_eq!(backoff_secs(u32::MAX), 30); // 오버플로 없이 상한 유지
    }

    #[test]
    fn backoff_zero_failures_is_defensive_min() {
        // 0회는 호출부에서 오지 않지만(항상 실패 후 호출) 방어적으로 최소값 1s.
        assert_eq!(backoff_secs(0), 1);
    }

    #[test]
    fn healthy_connection_needs_establishment_and_min_lifetime() {
        use std::time::Duration;
        // 수립 실패(None)는 생존 시간 개념 자체가 없다 = 실패 연쇄 유지.
        assert!(!connection_was_healthy(None));
        // 수립했어도 즉시 닫히면(크래시루프 브로커) 건강 아님 = 카운터가 계속 쌓여 포기 가능.
        assert!(!connection_was_healthy(Some(Duration::from_secs(1))));
        assert!(!connection_was_healthy(Some(Duration::from_secs(
            MIN_HEALTHY_SECS - 1
        ))));
        // 수립 시점부터 잰 순수 생존이 최소치를 넘긴 접속만 리셋 근거.
        assert!(connection_was_healthy(Some(Duration::from_secs(
            MIN_HEALTHY_SECS
        ))));
        assert!(connection_was_healthy(Some(Duration::from_secs(3600))));
    }

    #[test]
    fn seen_set_dedups_and_evicts_oldest_beyond_cap() {
        let mut seen = SeenSet::new();
        assert!(seen.insert("a"));
        assert!(!seen.insert("a"), "같은 id는 dedup");
        // 상한을 넘기면 가장 오래된 id부터 잊는다(무한 성장 방지, 리뷰 반영).
        for i in 0..SEEN_CAP {
            seen.insert(&format!("id-{i}"));
        }
        assert!(seen.set.len() <= SEEN_CAP, "상한 유지");
        assert!(
            seen.insert("a"),
            "방출된 가장 오래된 id는 다시 새 것으로 취급"
        );
        assert!(
            !seen.insert(&format!("id-{}", SEEN_CAP - 1)),
            "최근 id는 여전히 dedup"
        );
    }

    #[test]
    fn flush_pending_drains_all_lines() {
        let mut pending = vec!["a".to_string(), "b".to_string()];
        flush_pending(&mut pending);
        assert!(pending.is_empty(), "flush 후 pending은 비어야 한다");
        flush_pending(&mut pending); // 빈 상태 재호출도 안전(no-op)
        assert!(pending.is_empty());
    }

    // --- v2-45 P3: 워터마크·재생 구독 순수부 ---

    #[test]
    fn watermark_file_name_sanitizes_and_defaults() {
        // 정상 id는 불변.
        assert_eq!(
            watermark_file_name("win-opus-boss"),
            "watch-results-win-opus-boss.since"
        );
        // 빈 dispatcher(전체 관측)는 "all"(파일명 성립).
        assert_eq!(watermark_file_name(""), "watch-results-all.since");
        // 경로 문자·비ASCII는 '_' 치환(네임스페이스 탈출 방지).
        assert_eq!(watermark_file_name("../a/b"), "watch-results-___a_b.since");
        assert_eq!(watermark_file_name("총괄"), "watch-results-__.since");
    }

    #[test]
    fn is_db_datetime_accepts_only_db_format() {
        assert!(is_db_datetime("2026-07-11 09:00:00"));
        assert!(
            !is_db_datetime("2026-07-11T09:00:00"),
            "ISO 'T' 구분자는 거부(사전순 왜곡)"
        );
        assert!(!is_db_datetime("2026-07-11 09:00:00Z"));
        assert!(!is_db_datetime("2026-07-11 09:00"));
        assert!(!is_db_datetime(""));
        assert!(!is_db_datetime("어제쯤"));
    }

    #[test]
    fn encode_query_value_escapes_space_and_colon() {
        assert_eq!(
            encode_query_value("2026-07-11 09:00:00"),
            "2026-07-11%2009%3A00%3A00"
        );
        assert_eq!(
            encode_query_value("win-opus-boss"),
            "win-opus-boss",
            "unreserved는 불변"
        );
    }

    #[test]
    fn build_events_url_variants() {
        // 콜드스타트(워터마크 없음) = 현행 무파라미터 구독(재생 없음 = 과거 폭주 방지).
        assert_eq!(
            build_events_url("http://127.0.0.1:8770/", "dashboard", None),
            "http://127.0.0.1:8770/dashboard/events"
        );
        // 워터마크 + dispatcher = 재생 구독.
        assert_eq!(
            build_events_url(
                "http://127.0.0.1:8770",
                "dashboard",
                Some("2026-07-11 09:00:00")
            ),
            "http://127.0.0.1:8770/dashboard/events?since=2026-07-11%2009%3A00%3A00&dispatcher=dashboard"
        );
        // 빈 dispatcher는 파라미터 생략(전체 관측 의미를 서버 질의까지 유지).
        assert_eq!(
            build_events_url("http://127.0.0.1:8770", "", Some("2026-07-11 09:00:00")),
            "http://127.0.0.1:8770/dashboard/events?since=2026-07-11%2009%3A00%3A00"
        );
    }

    #[test]
    fn advance_watermark_takes_lexicographic_max_and_rejects_malformed() {
        let mut w: Option<String> = None;
        advance_watermark(&mut w, "2026-07-11 09:00:00");
        assert_eq!(w.as_deref(), Some("2026-07-11 09:00:00"), "첫 값 채택");
        advance_watermark(&mut w, "2026-07-11 08:00:00");
        assert_eq!(
            w.as_deref(),
            Some("2026-07-11 09:00:00"),
            "과거 값으로 후퇴 금지"
        );
        advance_watermark(&mut w, "2026-07-11 09:00:00");
        assert_eq!(w.as_deref(), Some("2026-07-11 09:00:00"), "같은 값은 유지");
        advance_watermark(&mut w, "2026-07-11 10:30:00");
        assert_eq!(
            w.as_deref(),
            Some("2026-07-11 10:30:00"),
            "미래 값으로 전진"
        );
        advance_watermark(&mut w, "2026-07-11T23:59:59");
        advance_watermark(&mut w, "garbage");
        assert_eq!(
            w.as_deref(),
            Some("2026-07-11 10:30:00"),
            "포맷 불량은 무시(비교 오염 방어)"
        );
    }

    #[test]
    fn watermark_file_persists_loads_and_rejects_corruption() {
        let dir = std::env::temp_dir().join(format!("tuna-wr-since-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(watermark_file_name("test-disp"));
        let _ = std::fs::remove_file(&path);

        // 파일 없음 = None(재생 없이 라이브부터).
        let mut file = WatermarkFile::at(Some(path.clone()));
        assert_eq!(file.load(), None);

        // persist → load 왕복.
        file.persist(&Some("2026-07-11 09:00:00".to_string()));
        let mut reread = WatermarkFile::at(Some(path.clone()));
        assert_eq!(reread.load().as_deref(), Some("2026-07-11 09:00:00"));

        // None 워터마크는 기록하지 않는다(기존 값 보존).
        file.persist(&None);
        let mut reread = WatermarkFile::at(Some(path.clone()));
        assert_eq!(reread.load().as_deref(), Some("2026-07-11 09:00:00"));

        // 오염된 내용은 무시(None = 라이브부터).
        std::fs::write(&path, "not-a-datetime").unwrap();
        let mut corrupt = WatermarkFile::at(Some(path.clone()));
        assert_eq!(corrupt.load(), None);

        // path=None(영속 불가 환경)은 전부 no-op(패닉 없음).
        let mut nofile = WatermarkFile::at(None);
        assert_eq!(nofile.load(), None);
        nofile.persist(&Some("2026-07-11 09:00:00".to_string()));

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn watermark_file_persist_is_monotonic_no_rewind() {
        // 단조 영속(리뷰 이월): --since로 과거 값부터 시작해도 디스크의 더 새 워터마크를 되감지
        // 않는다. persist는 마지막 기록값보다 새 값(사전순=시간순)만 쓴다.
        let dir = std::env::temp_dir().join(format!("tuna-wr-mono-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(watermark_file_name("mono-disp"));
        let _ = std::fs::remove_file(&path);

        let mut file = WatermarkFile::at(Some(path.clone()));
        file.persist(&Some("2026-07-11 10:00:00".to_string()));
        // 과거 값(오버라이드 되감기 시나리오)은 무시 - 파일은 더 새 값을 유지한다.
        file.persist(&Some("2026-07-11 08:00:00".to_string()));
        let mut reread = WatermarkFile::at(Some(path.clone()));
        assert_eq!(
            reread.load().as_deref(),
            Some("2026-07-11 10:00:00"),
            "과거 값으로 되감기지 않음"
        );
        // 더 새 값은 정상 전진 기록.
        file.persist(&Some("2026-07-11 11:00:00".to_string()));
        let mut reread = WatermarkFile::at(Some(path.clone()));
        assert_eq!(
            reread.load().as_deref(),
            Some("2026-07-11 11:00:00"),
            "새 값은 전진 기록"
        );

        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_if_drained_defers_while_pending_remains() {
        let dir = std::env::temp_dir().join(format!("tuna-wr-drain-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let path = dir.join(watermark_file_name("drain-disp"));
        let _ = std::fs::remove_file(&path);

        let mut state = InboxState {
            seen: SeenSet::new(),
            pending: vec!["RESULT ...".to_string()],
            flush_at: None,
            watermark: Some("2026-07-11 09:00:00".to_string()),
            file: WatermarkFile::at(Some(path.clone())),
        };
        // pending이 남아 있으면 기록을 미룬다(크래시 시 미출력 구간을 재생이 다시 가져오게).
        state.persist_if_drained();
        assert!(!path.exists(), "pending 잔존 중엔 워터마크 미기록");
        // pending이 비면 기록된다.
        state.pending.clear();
        state.persist_if_drained();
        assert_eq!(
            std::fs::read_to_string(&path).unwrap().trim(),
            "2026-07-11 09:00:00"
        );

        let _ = std::fs::remove_file(&path);
    }
}
