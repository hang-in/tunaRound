---
title: tunaRound v1 설계 (사람 주도 2-에이전트 설계 토론)
type: design-spec
status: approved
priority: high
updated_at: 2026-06-29
owner: d9ng
summary: 터미널에서 사용자가 운전하는, 역할 부여된 2-에이전트(Claude Code·Codex) 착수 전 설계 토론 도구. 같은 레포 위에서 토론 후 결론을 결과 문서로 자동 기록. tunapi 토론코어 + tunaFlow 러너 포팅, Rust+tokio. Redis 멀티세션·N좌석·git-tree 분기는 v2.
supersedes: tunaRound-v1-design.md (2026-06-29 자동토론 spec)
---

# tunaRound v1 설계 (사람 주도 2-에이전트 설계 토론)

> 표기 규칙: 존댓말, em-dash 금지(일반 대시/콜론). 도메인/URL 비노출.
> 이 spec은 brainstorming(2026-06-29)에서 **사람 주도 피벗**으로 재설계되었습니다. 기존 자동토론 spec(`tunaRound-v1-design.md`)을 supersede합니다. 다음 단계는 writing-plans -> 구현입니다.
> status=approved: brainstorming 전 과정에서 사용자가 설계를 주도·수용했습니다. 2026-06-29 확정.

## 1. 한 줄 정의

터미널에서 **사용자가 운전하는, 역할을 부여한 2-에이전트(Claude Code·Codex) 착수 전 설계 토론** 도구. 같은 레포 위에서 토론하고, 결론을 **결과 문서로 자동 기록**해 실제 코딩으로 넘긴다.

## 2. 배경 / 왜 재설계인가 (피벗)

- **기존 spec(자동토론)**: 에이전트끼리 수렴까지 자동 토론, 사람은 관찰자. Redis 멀티세션이 v1 핵심.
- **피벗 동기**:
  - tunaFlow 사용자 평: "토론 기능은 좋은데 앱(데스크톱/Tauri)이 무겁고 어렵다." -> **토론 레이어만 얇게 추출.**
  - 사람이 관찰자가 아니라 **참가자**여야 한다. "지금 사용자와 Claude의 대화에 Codex가 끼는" 3자 대화. 사람이 운전석.
  - 핵심 가치는 **착수 전 설계 토론**(코딩 들어가기 *이전*).
- **출처 레포의 강·약 (실측 2026-06-29)**:
  - **tunapi**(tunaFlow 전전신, Python): roundtable 토론코어 강함(역할/순차-인지/follow-up/consensus). 멀티세션은 인메모리라 약함.
  - **tunaFlow**(Rust/Tauri): CLI 러너(claude/codex 구동·stream 파싱·hardening) 강함. 데스크톱 결합으로 무거움.
  - **tunaSalon**(Rust): Redis 멀티세션(streams/lease) 강함. 단 화자가 실제 CLI 에이전트가 아니고 presence/snapshot 미구현.
- **tunaRound = tunapi 토론코어 + tunaFlow 러너**(v1) **+ tunaSalon Redis 멀티세션**(v2). 터미널 네이티브.

## 3. 아키텍처 (유닛 경계 + 포팅 출처)

의도적으로 분리된 4개 유닛. 각각 독립 테스트 가능한 경계.

| 유닛 | 책임 | 인터페이스 | 포팅 출처 |
|---|---|---|---|
| **에이전트 러너** | claude/codex를 CLI(exec 모드)로 스폰, stream 파싱, hardening | `stream_run(input, on_progress, on_chunk, is_cancelled) -> RunOutput` | tunaFlow `claude.rs`/`codex.rs` 포팅(Rust) |
| **토론 오케스트레이터** | 사람 주도 턴, 순차-인지 프롬프트 조립, 역할 주입, follow-up·지목, consensus carry-forward | `RoundtableParticipant`, `run_followup_round(session, msg, seats_filter)`, 순수 `build_round_prompt(...)` | tunapi `core/roundtable/` 청사진 -> Rust 재구현 |
| **전사·영속** | 트리-ready 메시지 모델 + 결과 문서 | `id`+`parent_id` 메시지, 토론 로그 + 결과 문서 파일 | 신규(파일 또는 작은 rusqlite). tunapi store 약점은 의식적 비차용 |
| **프론트엔드** | 렌더 + 입력 | 헤드리스 코어와 **이벤트/커맨드 경계** | v1=thin REPL. tunaSalon `render_chat`(ratatui)는 v1.x 폴리시 |

**경계 원칙(중요):** 코어(러너+오케스트레이터)는 프론트엔드/transport를 import하지 않는다(framework-independent core, dsp_cad_gcs 규율 차용). 이 경계 덕분에:
- v1 프론트는 thin REPL이고,
- v1.x에 ratatui, v2에 web/멀티관찰을 **코어를 안 건드리고** 붙인다.
- v2의 Redis는 바로 이 이벤트/커맨드 경계 위에 얹힌다(tunaSalon `session_bus`/`ObservationSink` 패턴).

## 4. 핵심 결정 (brainstorming 2026-06-29, 승인됨)

| # | 결정 | 비고 |
|---|---|---|
| 상호작용 | **사람 주도 대화형**(Q1-a) | 사람이 말하면 두 자리가 응답, 사람이 방향. 에이전트끼리 자동 토론 아님 |
| 공유 컨텍스트 | **같은 레포 + 공유 문서**(Q2-a) | 컨텍스트팩 없음. 에이전트가 자기 도구로 레포를 직접 읽음(`--full-auto`/`--dangerously-skip-permissions`). 보조 컨텍스트는 대화에 얹혀 흐름 |
| 산출물 | **공유 문서 = 매개이자 결과물** | 결론이 결과 문서로 자동 기록(복붙 제거) |
| 쓰기 모델 | **읽기 전용 화자 + 사람이 쓰기 지목**(Q4) | 기본은 말만(로그 자동 기록). "X, 문서에 써" 지목 시 그 턴만 쓰기 ON. 한 번에 하나 -> 충돌 차단 |
| 턴 동역학 | **순차-인지 + 지목 가능**(Q6-a) | 뒤 자리가 앞 자리의 같은 턴 답을 봄. "코덱스만" 지목 가능 |
| 역할 | **자리마다 역할 주입(v1 포함)**(Q5) | 프롬프트 prefix(`## Your role`). 거의 공짜 |
| 좌석 수 | **v1=2자리 고정** (Claude, Codex) | N좌석 로스터는 v2 |
| 종료 | **사람이 끝냄 + consensus carry-forward**(Q7-b) | 합의된 건 "다시 논쟁 말고 전제로" 주입(맴돌이 방지). FlowMeter 자동종료는 v1.x 선택 |
| 분기 | **v1=데이터 모델만 트리-ready, 분기 UI는 v2**(Q8) | 복붙 고통은 v1의 결과문서 자동기록으로 해결. 풀 git-tree=v2(브랜치=세션) |
| 스택 | **Rust + tokio** | v1 의존: rust+tokio+rusqlite+thin REPL. Redis는 v2 |

## 5. 핵심 루프 (턴 흐름)

```
1. 사용자가 메시지 입력 (전체 자리 대상, 또는 "코덱스만" 지목)
2. 오케스트레이터가 각 자리 프롬프트 조립 (순차):
   [## Your role: <역할 지시>]
   [이미 합의된 사항 (전제로): ...]            # consensus carry-forward
   [이전 라운드 응답: [Claude]:..., [Codex]:...]
   [이번 라운드 다른 에이전트 답변: ...]        # 순차-인지: 앞 자리의 이번 턴 답
   ---
   위 의견들을 참고하여 답변해주세요: <사용자 메시지>
3. 러너가 해당 엔진 CLI를 exec 모드로 스폰 (같은 cwd=레포, 읽기 전용),
   stream 파싱 -> on_chunk로 REPL에 스트리밍
4. 응답을 토론 로그(전사)에 기록
5. 다음 자리도 동일(앞 자리 답을 보고) -> 한 턴(=라운드) 완료
6. 사용자의 다음 메시지 = 새 라운드(run_followup_round). 끝낼 때까지 반복
7. 사용자가 "X, 결론 정리해 써" -> 그 자리만 쓰기 ON,
   결과 문서(레포 파일)에 기록 = "merge back"
```

- **쓰기 하드 분리(리스크 항목):** 말 턴은 읽기 전용 호출, 쓰기 지목 턴만 쓰기 권한. 프롬프트로만 "쓰지 마"는 불충분(자율 모드가 파일을 건드릴 수 있음). 러너에 read-only/write 두 호출 모드 필요.

## 6. v1 범위 / 비목표

### v1 범위
1. 2-에이전트(Claude·Codex)를 CLI exec 모드로 구동, 같은 레포 위에서 사람 주도 설계 토론.
2. 순차-인지 턴 + 역할 주입 + 자리 지목 + 쓰기 지목.
3. consensus carry-forward(맴돌이 방지), 종료는 사람.
4. 결론을 결과 문서로 자동 기록. 토론 로그 + 결과 문서 영속(파일/rusqlite).
5. thin REPL 프론트(헤드리스 코어 경계 뒤).

### v1 비목표 (명시적 제외)
- Redis 멀티세션 / 멀티 관찰자 / 재개 -> v2.
- N>2 좌석 로스터(역할x엔진, 로컬LLM·opencode 좌석) -> v2.
- git-tree 다중 브랜치 + 트리 시각화 -> v2(데이터 모델만 트리-ready).
- 리치 TUI(ratatui) -> v1.x.
- 에이전트가 실제 코드 편집/실행(협업 코딩) -> v2.
- 웹/GUI / 분산.

## 7. v2+ 로드맵

1. **N좌석 로스터**: 역할x엔진 자유 조립(`build_participants_from_config`), 로컬LLM(tunaLlama 패턴)·opencode를 독립 좌석으로.
2. **Redis 멀티세션 = git-tree 다중 브랜치**: 동시 세션/관찰/재개. **브랜치 = 세션**(같은 개념의 두 각도). tunaSalon `session_bus` 포팅 + presence/snapshot 신규 구현(둘 다 tunaSalon 미구현).
3. **리치 프론트엔드**: ratatui(tunaSalon `render_chat` 포팅), 이후 web 스트리밍 게이트웨이(헤드리스 경계 위, 언어 무관).
4. **협업 코딩**: 사람 지목을 넘어 에이전트가 합의 기반으로 실제 편집/실행/테스트.
5. **FlowMeter 수렴도 인디케이터**(선택): tunaSalon `flow.rs` `measure()` 차용, 자동종료 아닌 조언.

## 8. 개발 규율 (TDD / DDD / 검증) - dsp_cad_gcs 차용

- **TDD**: 모든 신규/수정 로직에 테스트 동반, 가능하면 red->green. 분기·계산은 **순수함수로 추출해 단위테스트**(예: `build_round_prompt`, consensus 추출, stream 파싱). 빅뱅 금지.
- **DDD(점진)**: 코어는 framework-independent(프론트/transport import 0). 새 개념은 처음부터 타입으로. 거동 자체발명 금지(출처 답습).
- **검증(강제)**: `cargo build` + `cargo clippy` + `cargo test`. **검증과 commit/push 분리**(한 배치 금지). 커밋 후 origin 동기 재확인.
- **pre-push 훅**: push 전 빌드+clippy+테스트 자동, 실패 시 거부(dsp_cad_gcs `.githooks/pre-push` 패턴).
- **위임**: 구현은 Sonnet 서브에이전트(codex 비사용), Opus가 스펙·리뷰·검증(tunaRound CLAUDE.md).

## 9. 주요 리스크 / 확인 필요 (plan 단계에서 풀 것)

1. **CLI stream 스키마 견고화**: claude/codex 비대화형 출력은 버전마다 바뀜. tunaFlow hardening(아래 §10)을 같이 포팅. NDJSON robust 파싱(미파싱 라인 fallback) 필수.
2. **쓰기 하드 분리**: 말 턴(read-only) vs 쓰기 지목 턴(write) 두 호출 모드. 프롬프트 디스플린만으론 불충분.
3. **턴 컨텍스트 토큰 비용**: 각 자리가 같은 레포를 재독(자기 토큰 소비). 설계 토론엔 감수. 전사 truncate(tunapi `_MAX_ANSWER_LENGTH=4000`) 차용.
4. **consensus 추출**: tunapi `consensus.py`의 합의 마커/추출 로직을 어떻게 Rust로 옮길지(synthesizer 필요 여부).
5. **트리-ready 영속 스키마**: v2 분기/세션이 rewrite 없이 얹히도록 `id`+`parent_id` 설계.
6. **레포 격리**: 에이전트가 같은 cwd에서 읽되 v1은 쓰기 금지(지목 제외). 실수로 코드 건드리는 것 방지.

## 10. 출처 레포 참조 (실측 2026-06-29, 포팅 시 읽을 것)

### 에이전트 러너 — tunaFlow `~/privateProject/tunaFlow/src-tauri/src/agents/`
- `claude.rs`: `stream_run(input, on_progress, on_chunk, is_cancelled) -> RunOutput`. Tauri 의존 거의 0, `AppError`+`NoConsole`만 필요.
- 스폰: `claude -p <prompt> --output-format stream-json --verbose --dangerously-skip-permissions [--model]`. codex: `codex exec --json --skip-git-repo-check --color=never --full-auto [--model]`, prompt는 stdin.
- hardening(같이 포팅): `claudeTransportFlipHardeningPlan` T1(rate_limit 라인) T2(stale resume 자동 1회 재시도) T7(에러 분류), `insightStabilityPlan` INV-3(top/nested 토큰 fallback) INV-4(idle watchdog 10분). v1에서 rate_limit/fresh_fallback은 None 가능, robust 파싱/watchdog은 필수.
- v1 단순화: Codex AppServer 스킵(exec 모드만), `stream_run` 1개만.

### 토론 오케스트레이터 — tunapi `~/privateProject/tunapi/src/tunapi/core/roundtable/`
- `rt_participant.py:15` `RoundtableParticipant{engine, role, instruction, order, model_override}` — engine+role 분리. `:43` `build_participants_from_config`(v2 N좌석 청사진).
- `orchestrator.py:271` `run_roundtable`, `:306` `parallel = parallel_first_round and round_num == 1`(기본 False=순차), `:336` `run_followup_round(session, topic, engines_filter)`(사람 follow-up + 지목).
- `prompt.py:11` `_build_round_prompt`: consensus(`:29-34`) / 이전 라운드(`:37-46`) / **이번 라운드 같은 자리 답(`:48-60`, 순차-인지)** / 역할 지시(`:62,70`). 순수 함수 -> 테스트 용이.
- `session.py:67` `RoundtableStore`: 인메모리 dict + **완료분만** JSON 스냅샷(`:89,96`) -> 진행 중 세션 소실. **이 약점은 v1에서 비차용**, v2 Redis가 해결.

### Redis 멀티세션 (v2) — tunaSalon `~/privateProject/tunaSalon/src/session_bus.rs`
- 재사용 6함수: `submit_command`/`publish_event`/`read_commands`/`subscribe_events`/`try_acquire_owner`/`refresh_owner`. 의존: redis 0.32 + tokio + futures.
- **presence/hot_snapshot은 키만 정의·미구현** -> v2 신규.

### 프론트엔드 (v1.x) — tunaSalon `~/privateProject/tunaSalon/src/`
- `chat.rs` `render_chat(...)` 순수 함수(LiveSession 무관) + 틱/렌더/50ms 폴 루프. ratatui 0.30/crossterm 0.29.

## 11. 다음 단계

1. 사용자 written-spec 리뷰 -> status approved.
2. 기존 `tunaRound-v1-design.md` supersede 표시.
3. CLAUDE.md 핸드오프 갱신(현재 상태 = 사람 주도 피벗 + 스택 확정).
4. writing-plans로 v1 구현 플랜 작성(레이어별 task, §9 리스크 반영, TDD red->green).
