# Plan v2-40: 유니버설 세션 버스 (임의 세션 A2A 주소화·발견·제어)

> 설계 정본 [docs/design/v2-40-universal-session-bus_2026-07-06.md](../design/v2-40-universal-session-bus_2026-07-06.md).
> 단계 S1~S5. 이 계획은 **S1(자동무장 훅)**을 상세화한다. S2~S5는 설계 §4 골자만.

## S1: SessionStart 자동무장 훅 (opt-in)

> 목표: `TUNA_AUTOARM=1`인 Claude Code 세션이 시작 시 자동으로 로스터에 등록되어 총감독 대시보드에 등장한다.
> 전제: `tunaround poll`이 이미 register_agent + heartbeat를 내장(worker.rs run_poll_loop). 훅은 poll을 detached로 기동만 하면 된다. deregister 도구는 없으므로 정리는 TTL(90초) 소멸.

- [ ] S1a: `.claude/hooks/tuna-autoarm.py`(SessionStart). stdin JSON(session_id·cwd) 읽기 → `TUNA_AUTOARM=1` 게이트(아니면 no-op exit 0) → core/token/agent/tags 해석 → 중복 가드(pidfile 생존 시 skip) → detached `tunaround poll` 기동(등록·heartbeat) → pidfile 기록 → `hookSpecificOutput.additionalContext`로 무장 사실·수신법 주입.
- [ ] S1b: `.claude/hooks/tuna-disarm.py`(SessionEnd). pidfile의 poll 프로세스 kill + pidfile 제거. 로스터는 TTL 90초로 자연 offline.
- [ ] S1c: `.claude/settings.json` 두 훅 배선(env self-gate라 항상 등록해도 무장 안 하면 무해).
- [ ] S1d: 문서 - a2a-usage에 §11 자동무장 훅 사용법(env·LAN 복제·안전).
- [ ] S1e: 라이브 테스트 - mock stdin으로 autoarm 직접 실행 → `/dashboard/roster`에 신규 agent online 확인 → disarm으로 소멸 확인. (실제 세션 재시작 없이 훅 스크립트 계약 검증.)

### 설정 계약 (env)
- `TUNA_AUTOARM=1`: 마스터 opt-in. 없으면 훅 전부 no-op.
- `TUNA_BROKER_CORE`: 코어 `/mcp` URL(기본 `http://127.0.0.1:8770/mcp`).
- `TUNA_BROKER_TOKEN`: bearer 토큰(필수. 없으면 무장 skip + 경고 컨텍스트).
- `TUNA_AUTOARM_AGENT`: 로스터 agent id(기본 `<host>-claude-<session8>`). 총감독은 `win-opus-boss` 등 고정 지정.
- `TUNA_AUTOARM_ROLE`: role 태그(기본 `session`). 총감독은 `boss` 등.
- `TUNA_AUTOARM_PROJECT`: project 태그(기본 cwd basename).
- `TUNA_BIN`: tunaround 실행 경로(기본 PATH의 `tunaround`).

### 안전 (설계 §3)
- opt-in만(TUNA_AUTOARM=1). 모든 세션 조용히 붙이지 않음.
- 토큰·LAN IP 평문 커밋 금지(설정은 env/gitignored). `.claude/settings.json`엔 값 아닌 커맨드만.
- 중복 무장 가드(같은 세션 재-hook 시 pidfile 생존이면 skip).

## S2: 발견 리포터 (미무장 세션 가시화)

> 목표: 무장(S1) 안 한 세션도 대시보드에 후보(candidate)로 뜨게 한다. 각 머신이 로컬 세션을 열거해 브로커에 보고 → 브로커가 집계 → 대시보드 "발견된 세션" 패널(S3).
> MVP 스코프: **claude 세션 발견**(`~/.claude/projects/<mangled-cwd>/*.jsonl` recent mtime, 무의존). codex 프로세스 스캔은 후속(codex는 app-server 경로로 이미 armable + process→project 매핑 불안정).

### 데이터 계약
- CandidateEntry: `{uuid(=세션 id), runner, project, source, age_secs, reported_at}`. `armed`는 저장하지 않고 브로커가 조회 시 online roster 소속 여부로 overlay 계산.
- stale: reported_at 기준 TTL(기본 CANDIDATE_TTL_SECS, 예 180초)로 자연 제외(리포터 죽으면 후보에서 사라짐).

- [ ] S2a (Opus 직접, 4파일 교차배선): 브로커 candidate 저장 + 조회 + overlay.
  - `src/store/candidates.rs`(신규): CandidateEntry 모델 + 순수 함수(is_fresh 등) + 단위테스트.
  - `sqlite.rs`: `candidate_pool: RefCell<HashMap<String,CandidateEntry>>` 필드 + `report_candidates(reporter_key, Vec<CandidateEntry>, now)`(리포터별 전체 교체) + `list_candidates(now, ttl)`(fresh만).
  - `mcp.rs`: MCP 도구 `report_candidates`(candidates 배열)/`list_candidates` + `GET /dashboard/candidates`(무인증 outer, armed overlay=uuid∈online roster). 스키마/툴 안내 갱신.
  - `mcp_client.rs`: `report_candidates`/`list_candidates` 타입 래퍼(register_agent 패턴).
- [ ] S2b (tunaLlama 위임 + Opus 리뷰): `tunaround discover` 열거.
  - 순수 함수: mangled-cwd 디렉토리명 → project 추정, jsonl stem → 세션 uuid, mtime window(--stale-mins)로 활동 세션 필터. 단위테스트.
  - CLI `discover --core --token [--interval N] [--stale-mins M] [--once]`: 열거 → CandidateEntry 빌드 → `client.report_candidates`. --interval 데몬/--once.
- [ ] S2c: 테스트 + 라이브 스모크 - 이 머신 discover → 내 세션(4a46a380..)이 후보 등장 → `/dashboard/candidates`에 armed overlay(win-opus-boss 무장분은 armed=true, 미무장 세션은 armed=false) 확인. 3-OS CI(discover 순수부 + 임베드 무관).

### 안전
- candidate 보고에 토큰·LAN IP 평문 금지(uuid·project·runner·age만). read(발견) 무인증 로컬, report는 토큰(브로커 write 경계).
- project 태그로 스코프 격리(엉뚱 프로젝트 세션 노출 방지는 S3 필터).

## S4: codex 직접 제어 (app-server turn/start 주입)

> 대시보드에서 codex app-server 세션에 turn/start를 직접 주입해 제어한다(v2-37 codex-inject 재사용).
> **MVP 트림**: codex 프로세스 자동발견(cmdline 스캔)은 기존 프로세스-열거 인프라(sysinfo) 없어 취약 → 후속.
> **수동 ws 제어부터 실증.** 로컬(loopback) 전용, 브로커 in-process codex_inject(worker 피처).

- [x] S4a: `codex_inject::run`이 최종답 반환(Result<String>, PrintText 누적) + 브로커 `POST /dashboard/control`(loopback만, worker 게이트): `{ws,text,agent?,timeout?}` → in-process turn/start 주입 → `{answer}`. **check(worker 유무 양쪽)·clippy 클린, codex_inject 23 pass.**
- [x] S4b: 대시보드 ControlForm.tsx(ws 입력 + 지시 텍스트 → POST /dashboard/control, 응답 pre 표시). 원격=관전 안내. api.ts sendControl. App mount + CSS. **npm build 211KB.**
- [ ] S4c 라이브 스모크: 대시보드 제어 폼 → codex app-server(ws://8790)에 주입 → 응답 확인(단 codex 사용량/모델 외부요인 가능).
- [ ] S4d(후속): codex 세션 자동발견(프로세스/rollout) → 후보 패널 편입 + busy/consent 스코핑.

## S5 (설계 §4, 후속)
- S5 검증: tunaRound→secall 세션 왕복 + 크로스머신 발견/arm.
