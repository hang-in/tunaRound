// heartbeat=presence + UserPromptSubmit 기반 boss/무장 모델의 설계 정본
# v2-42 heartbeat=presence + 사람입력 기반 총감독

> 2026-07-08 세션17 후반. v2-41 활동 모델을 라이브 도그푸딩하다 jsonl 기반 신호의 한계를 실측하고 재설계. 관련: [v2-41 활동 로스터](v2-41-activity-based-roster_2026-07-07.md) · [v2-40 세션 버스](v2-40-universal-session-bus_2026-07-06.md).

## 1. v2-41 라이브 실측 결함 (재설계 동기)

6개 실세션(win 3 + mac 3, 전부 resume)으로 도그푸딩한 결과:

- **총감독이 엉뚱한 세션에**: 순수 자동=jsonl mtime 최소인데, resume/에이전트응답/tool이 전부 mtime을 건드려 "방금 resume한 secall"이 총감독으로 뽑힘. **jsonl mtime ≠ "사람이 입력 중"**.
- **유령 세션(-B/-C 중복)**: `--stale-mins 240`이 오늘 이전에 닫힌 jsonl까지 4시간 창 안이라 리포트 → mac-claude-tunaRound가 3개(live 1 + 유령 2). jsonl은 세션이 닫혀도 파일이 남아 유령이 된다.
- **resume했는데 미무장**: resume은 SessionStart를 (autoarm 무장이 걸릴 만큼) 안 띄운다(실측: 재개 세션의 autoarm pidfile 없음). autoarm이 새 세션만 커버.

**뿌리**: presence(살아있나)·activity(활동)·boss(사람 위치)를 전부 jsonl(discover)로 잡는데, jsonl은 노이즈가 크다(닫혀도 잔존·활동≠사람입력·resume 미포착).

## 2. 신호 재배치

| 개념 | 옛(jsonl) | 새 |
|------|-----------|-----|
| presence(살아있나) | discover jsonl mtime | **heartbeat**(autoarm poll). 닫히면 끊겨 유령 없음. |
| boss(사람 위치) | jsonl mtime 최소 | **마지막 사람 프롬프트**(UserPromptSubmit 훅 핑). |
| activity(활성/유휴) | discover age | heartbeat 존재 + (선택) discover age 보조. |

## 3. UserPromptSubmit 훅 = 열쇠

SessionStart는 새 세션만·resume 미포착. 반면 **UserPromptSubmit는 사람이 프롬프트 넣을 때마다** 발동(resume 세션 포함). 하나의 훅이 둘을 해결한다:

- **(a) 무장 보장**: 이 세션이 아직 로스터에 없으면(pidfile 없음) autoarm과 같은 detached poll을 띄운다(idempotent - 이미 있으면 no-op). → resume·재개 세션도 첫 입력에 무장.
- **(b) 총감독 핑**: 브로커에 "세션 X가 방금 사람 입력 받음(now)"을 기록. → 총감독 = 최신 사람입력 세션. resume/tool로 안 튄다(사람 입력만 신호).

SessionStart autoarm은 유지(새 세션 즉시 무장). UserPromptSubmit가 resume·boss를 보강.

## 4. 브로커 변경

- 에이전트 로스터 엔트리에 `human_input_at: Option<String>`(마지막 사람 프롬프트 시각) 추가.
- 무인증 loopback 핑 경로(`POST /dashboard/human-ping {agent}` 또는 MCP 툴 `mark_human_input(agent)`): 그 agent의 `human_input_at`을 now로 갱신. 원격은 read-only.
- `/dashboard/roster` JSON에 `human_input_at` 노출.

## 5. 프론트 변경

- **총감독 = armed 세션 중 `human_input_at` 최신.** jsonl age 대신 사람입력 시각. 없으면(아무도 입력 안 함) 폴백=heartbeat 최신 or 없음.
- **로스터 = armed(heartbeat) 세션 + 최근 미무장.** armed는 heartbeat=presence라 age 무관 항상 표시(active/idle은 discover age로). **미무장(armed 없는) discover 세션은 jsonl age < FRESH_UNARMED_SECS(600s=10분)일 때만 표시** - 그 이상 오래된 jsonl은 닫힌 세션 잔존(유령)이라 프론트에서 제외(mergeSessions). → 같은 세션 옛 jsonl -B/-C 중복 소멸.
- **discover `--stale-mins`는 240 유지**(되돌리지 않음). armed 세션의 활동 age 커버리지(active/idle 판정)에 240분이 필요하고, 유령(미무장 old)은 위 FRESH_UNARMED 프론트 필터가 제거하므로 discover를 줄일 필요 없다. 미무장 실세션은 타이핑하면 ping 훅이 무장 → armed로 상시 표시.

## 6. 단계

1. 브로커: `human_input_at` 필드 + 핑 경로 + roster 노출.
2. 훅 `tuna-session-ping.py`(UserPromptSubmit): 무장 보장(idempotent) + 핑. 전역 settings.json 등록(win python·mac python3).
3. 프론트: boss=human_input_at 최신, 로스터=armed 우선, 유령 제외.
4. discover stale-mins 원복(작은 창) + 발견됨=미무장 최근만.

## 7. 비범위 / 후속

- jsonl age는 보조 지표로만 남길지(활성/유휴 미세 표시) 혹은 제거할지는 구현하며 판단.
- 크로스머신 사람입력 핑도 각 머신 훅이 (원격) 브로커로 보내면 동일 동작(mac은 TUNA_BROKER_CORE 원격).
