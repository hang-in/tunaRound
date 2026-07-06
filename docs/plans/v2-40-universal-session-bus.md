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

## S2~S5 (설계 §4 골자, 후속)
- S2 발견 리포터: `tunaround discover`(로컬 세션 열거 → 브로커 candidate 보고) + candidate 저장/조회.
- S3 대시보드 "발견된 세션" 패널 + arm 액션.
- S4 codex 직접 제어(app-server candidate turn/start) + busy/available/consent 스코핑.
- S5 검증: tunaRound→secall 세션 왕복 + 크로스머신 발견/arm.
