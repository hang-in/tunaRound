# v2-45: mesh 영속·재생 아크 (2026-07-11, 세션21 설계 정본)

> 스코프 = 세션20 핸드오프 §3의 B 아크 5건 + 세션21 사용자 결정 1건(직접 제어 제거).
> 코드 근거 = 세션21 병렬 조사(7 에이전트, 파일:라인 전수 확보). 라인 번호는 main=050364c 기준.

## 0. 배경 (전부 실측·코드 확정)

- **watch-results 결함 2건**: ① 브로커 재기동 시 SSE 단절 → `run()` Err → 호출부가 `exit(1)` 즉시 종료(재접속 없음, src/watch_results.rs:103·105 + src/cli_daemons.rs:151). ② 인박스 다운 중 완료된 task 통지 영영 유실(이벤트 버스=인메모리 broadcast cap 256, DB 백필 없음). 세션20 실측 3회.
- **피드 리로드 전멸**: /dashboard/events가 라이브 버스 구독만(src/mcp/server.rs:253), 프론트 Feed.tsx도 초기 REST 없음(:95). tokio broadcast는 늦은 구독자에게 replay하지 않음.
- **★ 결함 2건**: (a) human_input_at이 인메모리 RefCell 전용(src/store/sqlite.rs:130) → 브로커 재기동마다 ★ 증발. (b) codex 세션은 human-ping 발신자가 없어(claude UserPromptSubmit 훅뿐) ★가 codex로 못 이동.
- **mesh 무기억**: 브로커 DB의 messages/FTS가 0행(실측) = search_context가 빈 코퍼스 검색 중. 종결 task 119건 무한 누적(DELETE 코드 없음).
- **Redis**: redis 크레이트는 무조건 dep(피처 게이트 없음), 사용처는 src/session_bus.rs 하나 + main.rs 3경로(--observe, --session 재개, 종료 flush). 동일 데이터를 SqliteIndexer가 매 라운드 이미 영속(load_session 드롭인 대체 존재).
- **직접 제어 잔재**: /dashboard/control + ControlForm = v2-46 relay 이전의 우회로. task 장부 우회(피드·결과 추적 없음) + 마커 없는 원문 주입이라 사람 턴 오인 구멍.

## 1. 방향 (사용자 확정, 2026-07-11 세션21)

- **대시보드 = 관제탑에 충실.** 뷰(로스터·피드) + 목표 제출(위임 티켓 발행, 장부 경유)만. 직접 제어 UX는 제거하고 다시 늘리지 않는다.
- **★ = TUI 자리 기준 유지.** 웹 goal 제출을 human 신호로 승격하지 않는다(웹=조종석 확장 비채택).
- **Redis = 완전 opt-out = 전삭제.** redis는 피처에 묶여 있지 않아 "피처 기본값에서 제거" 선택지 자체가 없음. 사용자 확정이 전삭제와 정합.
- **재생·스냅샷의 SoR = tasks 테이블(SQLite).** 버스/SSE Last-Event-ID 재생은 비채택(버스 cap 256 + Lagged 스킵 + 이벤트 무영속이라 구멍이 남음).

## 2. 비스코프

- 이벤트 자체의 영속(task별 중간 전이 이력 재구성). 스냅샷은 task별 최종 상태 1프레임이 상한 - 사용자 기대치로 명시.
- TaskEvent broadcast에 메시지 이벤트 variant 추가(--observe 대체용) - 비채택. exhaustive match 파급, 폴링으로 충분.
- 로스터 전체의 SQLite 영속화 - 비채택. 재기동 직후 죽은 세션 부활(유령 카드) = v2-46 tombstone 방향에 역행. human_input_at만 별도 테이블로.
- 종결 task의 행 삭제 - 비채택(이번 아크). get_task가 "task 없음"으로 조용히 소멸(세션8 죽은 id 폴링 마찰 재발). 슬림화까지만.

## 3. 공용 기반: task 재생 표면 (P2가 구축, P3·피드가 소비)

같은 tasks 테이블에서 "과거 task를 이벤트 envelope로 재구성"하는 로직을 한 곳에만 둔다(조사에서 질의 2개+엔드포인트 3개로 중복 설계된 것을 통일).

- **공용 store 질의 1개**: `list_tasks_replay(from_agent: Option<&str>, since: Option<&str>, states: &[&str], limit: Option<usize>)` - TASK_COLUMNS/task_row_from_sql 재사용, updated_at 오름차순.
- **envelope 재구성 헬퍼 1개**: state == "completed" → `{"event":"completed"}`, 그 외 전부 `{"event":"status"}`. 라이브 매핑과 동일(completed 상태는 try_complete 전이로만 도달하므로 state 기준 재구성 = 라이브와 일치). failed/canceled를 "completed"로 내보내는 실수 금지(조사 중 자기모순 있었음).
- **운반 = /dashboard/events SSE 선행 프레임(opt-in 쿼리 파라미터)**: subscribe 먼저 → 스냅샷 조회 → `stream::iter(snapshot).chain(live)`. 선례 = a2a_server.rs resubscribe_frame_json_stream(:530) + subscribe-먼저 순서(:478). 별도 REST는 비채택(프론트가 SSE/REST 병합 레이스를 짊어짐).
  - `?replay=N` (기본 0=현행 유지): 최근 N건, **전 상태 포함**(canceled·열린 task 포함 - 피드는 전 상태 뷰). 피드 전용.
  - `?since=TS[&dispatcher=X]`: TS 이후 **completed/failed만**(watch-results 인박스 의미론 - canceled는 총괄 자신의 취소가 대부분이라 통지 제외 유지, 설계 결정으로 명시). watch-results 전용.

## 4. 단계 분할 (P0~P7, 각각 1 PR)

- **P0 직접 제어 제거** (독립, 소수정): server.rs route(:121)+dashboard_control_handler(:558)+control 전용 SSRF 가드 제거(goal이 공유하는 CSRF 헬퍼는 유지) / ControlForm.tsx 삭제·App.tsx 마운트 제거·api.ts sendControl 제거·vite proxy 항목 제거 / npm build. codex_inject::run(Result<String>)은 relay가 쓰므로 유지.
- **P1 watch-results 재접속** (클라이언트 전용, 소수정): run()을 접속 1회분으로 분해 + 바깥 재접속 루프(지수 백오프 1s→30s 상한, 성공 시 리셋). seen·digest pending은 루프 바깥 소유. 청크 에러 경로에서도 pending flush. 연속 실패 N회(기본 20) 초과 시 exit 1 유지(Monitor 통지 보존 - 주소 오타 같은 영구 실패를 조용히 삼키지 않음).
- **P2 서버 재생 기반 + 피드 스냅샷**: §3 전체 + Feed 프론트(EventSource URL에 `?replay=50`, history 중복 가드 = 같은 task.id+updatedAt이면 history append 스킵). spawn_blocking + lock 짧게(roster 핸들러 패턴).
- **P3 watch-results 재생 클라이언트** (P1·P2 뒤): (재)접속 시 `?since=워터마크&dispatcher=X`로 구독 → 서버가 재생분+라이브를 한 스트림으로 줌 → 클라이언트 파서 무변경. 워터마크 = 서버가 준 updatedAt 최대값만 사용(로컬 시계 금지), 비교 >= + seen dedup 병행. 상태 파일 영속(`%LOCALAPPDATA%/tunaround/watch-results-{dispatcher}.since`) - 프로세스 재시작 유실까지 커버, 파일 없으면 재생 없이 라이브부터(과거 폭주 방지), `--since`로 수동 오버라이드.
- **P4 ★ human_input_at 영속** (스키마 v9): 신규 테이블 `agent_human_input(uuid TEXT PRIMARY KEY, at TEXT NOT NULL)`. mark_human_input = DB 먼저 write-through 후 인메모리. **미등록 핑 선기록 채택**(로스터에 없어도 기록+200 - 404 유실 창 제거, P5의 순서 제약도 함께 해소). register_agent/sync_presence 폴백 체인 = 인메모리 → 테이블 SELECT(재기동 후 스캐너 첫 보고 ≤15초에 ★ 자동 복원). GC = deregister뿐 아니라 **sync_presence stale 제거 루프에도 DELETE**(대부분의 세션 소멸이 deregister를 안 탐 - 조사 확정) + 7일 초과 행 주기 GC.
- **P5 codex 입력 신호** (P4 뒤 필수 - 영속 커버리지 비대칭 방지): presence 스캐너에 rollout tail 스캔 순수 함수 = `type=="event_msg" && payload.type=="user_message"` 줄의 top-level timestamp 최신값, **"브로커 task " prefix 시작 메시지는 제외**(relay 주입 실측). tail 256KB 역방향 + 전 주기 mtime 무변경 스킵. ISO Z → SQLite datetime 정규화 헬퍼('T' > ' ' 사전순 함정). 전달 = PresenceSessionInput/PresenceUpsert에 human_input_at 추가 → sync_presence를 preserve-only에서 **max-merge + 승자 write-through**로. uuid별 최신 rollout 파일 기준. relay build_inject_text prefix를 필터 계약으로 주석 고정.
- **P6 mesh 기억화** (2분할):
  - **6a 색인** (스키마 v10 tasks.indexed_at): MCP complete_task/fail_task 핸들러(전 production 종결 경로의 수렴 지점 - 워커·relay·CLI·세션 전부)에서 전이 성공 후 writer.append_turn으로 요청문+결과 색인. writer는 Option - None이면 명시적 no-op. a2a_store Mutex 해제 후 writer 호출(락 순서). best-effort(색인 실패 ≠ 종결 실패). 색인 성공 시 indexed_at 스탬프. 기동 시 미색인 terminal 백필 스캔(재기동·expire_stale_claims 유실 보완). "결과 있는 종결만 색인"(CancelTask·expire 격리는 비대상) 스코프 명시. 임베딩은 best-effort, 1단계는 FTS만으로 성립.
  - **6b retention** (P2·P3 머지 뒤에만): prune_terminal_tasks(보존기간 기본 30일) = indexed_at NOT NULL이고 기간 초과분을 슬림화 - history_json='[]' → (completed만) message_json=NULL. **artifacts_json과 failed의 message_json(실패 사유)은 행 수명 내내 보존**(get_task 재조회 계약 + watch-results 160자 절단 후 전문 재조회 창구). 행 삭제 없음. sweep에 PRAGMA wal_checkpoint 동반(수동 정리 실측 해소).
- **P7 Redis 전삭제** (독립, diff 넓음): Cargo.toml redis dep 제거 + session_bus.rs 삭제(SessionBus trait 포함 - 구현체 없는 죽은 추상화 방지) + repl bus 필드·미러 블록 제거. main.rs = --observe를 SQLite 재작성(load_session 출력 + msg_id 커서 폴링 tail), --session 재개를 load_session→seed_from으로(--db 없으면 경고), owner lease 삭제(로컬 DB 단일머신 YAGNI - 결정 명시 기록), 종료 flush 삭제. --session 옵션·프로파일 session 키는 의미 전환 유지(SQLite 세션 id로도 이미 쓰임). 문서 개정(README·tunaround.toml.example·dev-mac-windows).

- **P8 유휴-열림 세션 로스터 유지** (백로그 C 승격, 2026-07-11 세션21): 마커 pid가 살아 있으면 mtime 창(240분)과 무관하게 로스터 유지. **3중 가드 필수** - ① pid 생존 + 그 프로세스가 claude/codex(이름 검증) ② 같은 살아있는 pid를 여러 마커가 가리키면 mtime 최신 세션만 인정 ③ 마커 없음 = 현행 창 폴백. codex 세션은 마커가 없어 rollout session_meta의 pid 유무를 정찰 후 범위 결정. 대안이었던 "총괄 주기 하트비트 주입"은 비채택(claude TUI 주입 채널 부재, 세션 wake 토큰 비용, 가짜 활동의 유휴/활동 신호 오염).

**순서**: P0·P1 즉시(상호 독립, 파일 겹침 없음) → P2 → P3. P4 → P5. P6a는 독립, P6b는 P2·P3 뒤. P7·P8은 순서 무관(리베이스 부담만 회피).

## 5. 고정 계약 (구현이 흔들리면 안 되는 것)

1. 재생 SoR = tasks 테이블. 버스/SSE id 기반 재생 금지.
2. envelope 매핑 = state가 completed일 때만 "completed", 그 외 전부 "status".
3. since/워터마크 포맷 = DB `datetime('now')` 포맷("YYYY-MM-DD HH:MM:SS" UTC) 그대로. ISO8601 금지(사전순 비교 왜곡). 클라이언트는 서버가 준 updatedAt만 워터마크로. 비교 >= + seen dedup 병행.
4. 스키마 버전 선점 = **v9 = agent_human_input(P4), v10 = tasks.indexed_at(P6a)**. P4가 먼저 머지. 순서가 바뀌어도 번호 스왑 금지(늦는 쪽이 다음 번호).
5. 데이터 수명 = retention 보존기간(30일) > 재생 지평선 + 피드 창(50건). artifacts_json·failed message_json은 행 수명 내내 보존.
6. 기계 주입 마커 = "브로커 task " prefix(build_inject_text). 문구 변경 = P5 필터 파손. 주석으로 계약 고정.
7. 색인 네임스페이스 = session_id `a2a:<task_id>`, speaker `a2a/<agent>`(요청=from, 결과=to 또는 runner). save_session(전량 교체)이 절대 건드리지 않는 전용 네임스페이스.
8. sync_presence 최종형(P4+P5 합산) = human_input_at을 max(인메모리 기존, 보고값, 영속 테이블) merge + 승자 write-through + stale 제거 시 영속 행 DELETE. 두 PR이 이 한 함수를 각자 다르게 고치지 않도록 이 형태를 정본으로.
9. 대시보드 = 뷰 + 목표 제출만. ★ = TUI 자리 기준(role!=worker/infra 중 human_input_at 최신).

## 6. 검증

- 각 PR: cargo test(기본 + 풀피처) + clippy(-D warnings) + 해당 라이브 스모크. 배포 전 `cargo build`(세션20 교훈: test/clippy는 bin을 안 만듦).
- 아크 통합 스모크(전 PR 후): 브로커 kill→재기동 시나리오에서 ① watch-results 프로세스 생존 + 다운 중 완료 task 재생 ② 대시보드 리로드 후 피드 유지 ③ ★ ≤15초 자동 복원 ④ codex TUI 입력 → ★ 이동(relay 주입으로는 이동 안 함 확인) ⑤ search_context로 과거 위임 이력 검색 성사.

## 7. 조사 원본

세션21 병렬 조사 결과(evidence 파일:라인 전수) = 워크플로우 wf_0a846d3e-325 journal. 요약은 context-notes.md 세션21 항목.
