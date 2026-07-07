# 세션17 핸드오프 (2026-07-07~08): 대시보드 재설계 = heartbeat=presence + 사람입력 총감독

> 다음 세션 첫 행동: 이 문서 + [정본 타겟 모델 v2-43](../design/v2-43-target-model_2026-07-08.md) 읽기. **재센터링 결론**: A2A 워크플로우는 이미 완성돼 있고, 대시보드는 그 뷰일 뿐. 남은 건 새 설계가 아니라 **기존 프리미티브 배선**.

## 0. 한 줄 요약

세션16 후속(재부팅 재기동)으로 시작 → **대시보드 라이브 도그푸딩 2아크**: (A) PR #32 = 활동모델·codex관전결정·주입/네이밍/뱃지·봇수정. (B) PR #33 = **v2-42/43 재설계**(활동모델 결함 실측 → heartbeat=presence + 총감독=사람입력으로 정리). mac autoarm 전면화 협업. **정본 타겟 모델 v2-43 확정.**

## 1. PR 상태

- **PR #32 머지됨**(main=fc1f93e): 활동 기반 로스터(v2-41) + codex 관전 결정(§10) + 주입텍스트 절차/답변 분리 + 네이밍 `{machine}-{runner}-{project}` + 뱃지 값만·session별줄 + discover stale-mins 240 + 봇리뷰 8건.
- **PR #33 열림**(브랜치 `feat/heartbeat-presence-model`, HEAD=44ec2ed): **v2-42/43 heartbeat=presence 재설계**. CI 재검증 중(force-push로 일관상태 교정 완료). **머지 전 CI green + 봇리뷰 확인**(규율).

## 2. v2-42/43 재설계 (PR #33 내용)

**동기**: v2-41 활동모델(jsonl age)을 6실세션으로 도그푸딩 → 결함 3개: 총감독이 resume한 secall로 튐(jsonl mtime≠사람입력) / 유령 -B/-C(닫힌 jsonl이 stale창에 잔존) / resume 미무장(SessionStart 미포착).

**해결(정본 = 설계 v2-43)**:
- **총감독 = human_input_at 최신 online 세션**(사람 프롬프트만 신호). 브로커 AgentEntry.human_input_at + `/dashboard/human-ping`(loopback) + `mark_human_input`. UserPromptSubmit 훅 `tuna-session-ping.py`(무장보장+핑) + 공유 `tuna_arm.py`.
- **로스터 = online(heartbeat) 세션 전부.** 발견/유휴·discover·활동모델·FRESH_UNARMED **전부 제거**(전부 autoarm이라 불필요). `activity.ts=buildRoster(online만)`, `Candidates.tsx` 삭제, discover 프로세스 중단.
- **정렬**: 총괄→현재머신→원격. exit→offline→사라짐.

## 3. A2A 워크플로우 (이미 완성, 재발명 금지)

```
사람 ⟷ 총괄(아키텍트) ─①send_task/goal→ 감독/워커 ─②Monitor(poll) 자율수신→ complete_task
                          ↖──③ watch-results(총괄 인박스 SSE)로 결과가 총괄 깨움(PR #19) ──┘
       ④총괄이 사람에게 브리핑 + 다음 논의
```
받는 자리(감독/워커)는 **자율**이라 사람이 UX 안 봄 → Monitor "바쁨" UX는 논점 아님. 사람은 총괄에만 앉음. **①~④ 프리미티브 전부 있음.**

## 4. 남은 배선 (다음 세션, 새 설계 아님)

1. **env→설정파일**(최우선): 훅이 env 대신 `~/.tunaround/config`(AUTOARM·BIN·CORE·MACHINE·TOKEN) 읽기. **env가 터미널 launch에 고정**돼 훅이 no-op하는 문제(오늘 secall 실패·이 세션 ping 미작동 근본, 토큰로테이션과 같은 교훈). 설정파일이면 신뢰성 확보.
2. **수신 배선**: autoarm 훅 additionalContext에 "받으려면 `Monitor(poll --agent <id>)`" 안내 → 감독 패턴 일반화. 총괄은 watch-results 운용.
3. **codex arming**: codex-server 세션은 claude 훅이 안 잡음. codex용 poll 등록 별도 배선(임의 codex 세션 자동등록).
4. **워커 섹션**: role=worker 헤드리스만 별 섹션 "작업 중" 표시.
5. (후속) 크로스머신 boss-ping 토큰인증(현재 loopback=win boss만).

## 5. 재시작 방법 (사용자 검증)

- **재설치 불필요.** 브로커·win 바이너리(`TUNA_BIN=target\debug\tunaround.exe`)·훅(~/.claude/hooks/)·프론트(dist) 다 현재 상태. PR #33 머지해도 main=브랜치 코드=현 바이너리.
- **각 세션을 새 터미널 창(setx 이후)에서 시작** → env 상속 → autoarm 작동. 옛 터미널 재시작은 env 없어 no-op(오늘 실측).
- win claude: 새 터미널 시작 → heartbeat+ping 다 됨. mac claude: presence 됨(총괄은 win 전용). **codex-server: 자동등록 아직 안 됨**(배선 #3).
- 수용기준: 재시작 후 발견/유휴 없음 / 로스터=모든 열린 TUI / exit→사라짐(딜레이 OK).

## 6. 라이브 메시 (재부팅/exit 시 소멸, 상세=backend-private 세션17)

- broker `serve` PID **6092**(0.0.0.0:8770, v2-42 백엔드 포함 재빌드) · boss-poll(win-claude-tunaRound=이 세션) **32808** · watch-results **32596** · codex-sup **16152**. **discover 중단**(불필요).
- mac: mac-codex-sup online. mac-claude autoarm은 mac 세션 재시작 시.

## 7. 핵심 교훈

- **재발명 금지**: A2A 워크플로우는 완성. 대시보드=뷰. (오늘 이걸 자꾸 재설계하려다 꼬임 - 사용자 재센터링.)
- **env는 launch에 고정**: 훅/데몬이 env 의존이면 터미널·프로세스 신선도에 물림(오늘 secall·ping, 세션16 토큰로테이션). 근본=설정파일.
- **북극성**([[tunaround-north-star]]): 개인 악기, 경쟁렌즈 금지. 상시감시=HITL(토큰지옥 아님: Monitor(poll)=0토큰 대기).
- **codex ≠ claude 훅**: codex는 별 arming.

## 8. 다음 세션 첫 행동

1. PR #33 머지됐나 확인(안 됐으면 CI+봇 확인 후 머지). `git checkout main && git pull`.
2. 사용자가 전 세션 재시작함 → 로스터에 다 뜨는지 검증(개념 실증).
3. **배선 #1(env→설정파일)부터** = 신뢰성 근본. 그다음 #3(codex arming)·#2(수신안내)·#4(워커섹션).
4. `cargo test`(기본) + `--features "dashboard worker"`로 상태 확인(cargo는 Bash 툴로, 브로커 종료 후에만 bin 빌드 가능=exe 락).
