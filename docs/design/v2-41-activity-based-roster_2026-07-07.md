// 활동 기반 로스터↔발견 세션 모델(총감독 대시보드)의 설계 정본
# v2-41 활동 기반 로스터↔발견 모델

> 2026-07-07 세션17, 총괄(아키텍트)+사용자 대화로 확정. 기존 "무장/미무장" 이분법을 "활동/유휴 스펙트럼"으로 바꾼다. 관련: [v2-40 유니버설 세션 버스](v2-40-universal-session-bus_2026-07-06.md) · [orchestrator-dashboard](v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06.md).

## 1. 동기

세션16까지의 모델: **로스터**(register+heartbeat = poll watcher 살아있음) vs **발견됨/후보**(discover가 jsonl 스캔). 두 개의 분리된 신호. 문제:
- 총감독 자기 세션이 미무장이면 "채용 후보"로 뜬다(세션17 실측, 어색).
- "발견→감독으로 승격" 서사가 부정확: TUI 세션은 이미 감독(HITL)이고, claude 세션은 원격 arm 불가(발견≠제어).
- 무장/미무장이 활동을 반영 못 함: poll watcher가 돌면 사람이 그 TUI에 없어도 online.

사용자 재구성: **라이브 TUI 세션 = 감독 세션.** 배치는 **활동(jsonl 최신도)**이 결정한다. 활성=로스터, 유휴=발견됨. 재활동하면 로스터 복귀.

## 2. 모델

단일 축 = **활동 age**(마지막 jsonl 활동 이후 경과, discover가 `age_secs`로 계산).

| 상태 | 조건 | 위치 |
|------|------|------|
| 활성 | age < IDLE_SECS(3600 = 60분) | 관리자 로스터 |
| 유휴 | age >= IDLE_SECS | 발견됨 |
| 총감독 | 활성 세션 중 age 최소(사람이 방금 입력) | 로스터 ★(자동) |

**전이**: 유휴화 → 발견됨 강등. TUI에서 활동(`.` 한 글자라도) → jsonl mtime 갱신 → age 리셋 → 로스터 복귀. discover 폴 주기(30s) 지연.

**총감독 자동 감지**: 활성 세션 중 age 최소 = 사람이 지금 입력하는 세션(대개 안정적, 여러 세션 왕래하면 튀지만 사용자는 보통 하나만 씀). **수동 ★ override**로 고정 가능(localStorage). override 있으면 그것 우선, 없으면 자동최신.

## 3. 구현 (프론트 병합, 최소 리스크)

브로커는 이미 두 뷰를 노출(`/dashboard/roster`=armed agents+heartbeat, `/dashboard/candidates`=discover 세션+age+armed flag). **프론트가 session uuid로 병합**한다(백엔드/브로커 변경·재기동 불요, dist 새로고침 반영).

1. roster + candidates 둘 다 폴(App).
2. session uuid로 병합: candidate(uuid=세션id, age, armed, machine/runner/project) ↔ roster agent(uuid 또는 tags.session 매칭 → display_name, heartbeat, online).
3. 활동 age 산출:
   - candidate 매칭 있으면 그 age(로컬 jsonl).
   - 없으면(원격 agent = 다른 머신, 로컬 discover 커버 밖) **heartbeat 폴백**: online → 활성(age 0 취급), offline → 유휴. (한계: 원격은 jsonl 활동을 못 봐 heartbeat 프록시. 크로스머신 활동 정밀화는 각 머신 discover가 세션태그로 보고해야 - 후속.)
4. active = age < IDLE_SECS. 활성 → 로스터, 유휴 → 발견됨.
5. 총감독 = 활성 중 age 최소(자동) 또는 ★ override.

**armed**(poll 있음=A2A 수신 가능)는 **속성**(행에 표시)이지 배치 결정자가 아니다. 활성 세션엔 armed(주소화됨)와 미armed(발견만)가 섞일 수 있다.

## 4. 비범위 / 후속

- 크로스머신 활동 정밀화(각 머신 discover가 세션태그 포함 보고 → 원격도 jsonl age): 후속. 지금은 원격=heartbeat 프록시.
- 미무장 활성 세션의 자동 arm(원격 소켓 부재라 claude는 불가, 복붙 헬퍼 유지). discover=가시성, arm=수동.
- IDLE_SECS·boss 로직은 프론트 표시 정책이라 프론트에 둔다(코어 도메인 아님).
