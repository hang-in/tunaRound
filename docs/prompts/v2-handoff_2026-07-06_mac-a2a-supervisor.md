---
title: tunaRound 핸드오프 - 2026-07-06 맥 A2A 감독 세션 (4자 자율 mesh + v2-40 배포)
type: prompt
status: active
priority: P0
updated_at: 2026-07-06
owner: mac
summary: 맥 세션에서 크로스머신 4자 자율 A2A 감독 mesh 구축·양방향 무-셔틀 실증 + v2-40 discover 크로스머신 배포. /exit로 새 세션 시작 예정. ⚠ 세션-바운드 Monitor watcher는 /exit로 죽음(자율 수신 정지), nohup 데몬(codex sup·discover·app-server)은 생존. 콜드 스타트 시 mac-claude-sup Monitor 재-arm 필요.
---

# 맥 A2A 감독 세션 핸드오프 (2026-07-06)

> 이 세션은 /exit로 종료 예정 = 다음 mac 세션은 **완전 콜드 스타트**. 이 문서 + `CLAUDE.md`(현재 상태 서술=Windows 편집분) + 최신 origin/main으로 시작한다.

## ⚠ 콜드 스타트 즉시 (실행 인프라 + /exit로 죽는 것)

**보안 먼저**: 레포 PUBLIC. 브로커 LAN endpoint·토큰을 문서/코드에 평문 커밋 금지. 토큰은 로컬 `~/.zshrc`의 `TUNA_BROKER_TOKEN` env에 있음(이 세션이 넣음). 브로커=Windows 호스팅 LAN(포트 8770). 이 세션 중 토큰이 여러 번 로테이트됐고, context-notes.md 토큰 평문 커밋 사고로 feat 브랜치 히스토리를 force-push로 정리한 적 있음 - **토큰은 항상 env·A2A 메시지로만**.

**/exit 후 데몬 상태**:
- 살아남음(nohup detached, init reparent): `mac-codex-sup` poll watcher(codex-inject narrate) / `discover` 데몬(--interval 30) / codex `app-server`(ws 127.0.0.1:8790). → `pgrep -fl tunaround`, `nc -z 127.0.0.1 8790`로 확인.
- **죽음**: `mac-claude-sup` Monitor watcher(세션 하네스 바운드) → **mac-claude-sup 자율 수신 정지**. 새 세션이 재-arm해야 함.

**mac-claude-sup 자율 수신 복구(콜드 스타트)**:
1. `export TUNA_BROKER_TOKEN=<env에 이미 있음>`; 브로커 도달 확인(`tunaround doctor` 또는 agent-card curl).
2. 세션 하네스 Monitor로 `tunaround poll --core <브로커/mcp> --token $TUNA_BROKER_TOKEN --agent mac-claude-sup --tags "machine=mac,runner=claude,role=supervised,project=tunaround" --interval 15` 상주. task 도착 시 세션 자동 wake → claim/complete.
3. MCP 툴은 세션 시작 때만 로드 = **raw HTTP MCP(curl로 initialize→notifications/initialized→tools/call) 권장**(등록·재시작 마찰 회피). 세션 만료 404는 재핸드셰이크로 자가복구(수동 R10).

## ① 이 세션이 한 것

- **4자 크로스머신 자율 A2A 감독 mesh 구축**: win-opus-boss(허브,claude) / win-codex-sup(codex) / mac-claude-sup(claude,나) / mac-codex-sup(codex). 전부 자율 수신(watcher가 task 자동 처리, 사람 복붙 0). **양방향 무-셔틀 실증**(Windows↔mac 던지면 양쪽 자동 처리).
- **감독 = Monitor watcher(claude) 또는 poll --on-task + codex app-server(codex, v2-37)**. heartbeat(#10)로 상시 online + to_selector 라우팅.
- **v2-40 discover 크로스머신 배포**: 로컬 Claude 세션을 브로커 candidate로 리포트 → win 대시보드 /dashboard/candidates 크로스머신 노출. discover가 claude-mem observer 세션(~/.claude-mem/, 2572개)을 false-positive로 잡던 걸 조사·보고 → 필터 반영(후보 6→1).
- **정직한 실패 규율 실증**: discover 미구현 브랜치에서 fake success 대신 `fail_task`로 정직 보고 → 디스패처가 S2 구현 → 재시도 성공.

## ② 레포 상태

- **main = `83d3ef5`**(PR #13, v2-40 유니버설 세션 버스·자동무장·발견·후보패널·codex 제어 + 대시보드 확장). 로컬 동기 완료, feature 브랜치 전부 머지·삭제됨(origin엔 main만).
- 설치 바이너리 `tunaround` = v0.2.2(main 기준). Cargo.toml=0.2.2.
- discover 데몬은 옛 feat 빌드(./target/debug)로 도는 중이나 discover는 main에도 동일(머지됨) - 재기동 시 main에서 재빌드 가능.

## ③ 설계 교훈 (다음 세션이 꼭 볼 것)

- **GN "LLM Wiki 편집국" 글**([[multiagent-structure-vs-autonomy]] 메모리): 자유 자율 멀티에이전트는 토큰↑·컨텍스트 손실·가짜 완료. 해법=구조·규칙·컨텍스트 격리·단일 판정자. **오늘 만든 자율 mesh는 배관은 인상적이나 이 글이 경고한 패턴으로 drift**(이 세션이 수 시간 자율 task 처리로 길어짐). 다음 세션은 자율 mesh를 유지할지, tunaRound의 원래 human-driven·구조화 코어로 회귀할지 판단할 것.
- **동적 총감독(사용자 채택)**: 총감독은 고정 자리가 아니라 **역할** - 사람이 말 거는 세션이 그때그때 총감독. 상태(roster·task·transcript)가 broker.db에 외부화돼 있어 어느 세션이든 hydration ritual(list_agents + open tasks + 최근 transcript)로 이어받을 수 있음. **하지만 "결정/종합"이 1급 외부화 안 돼 아직 자동 아님** - 이걸 `tunaround hydrate` 서브커맨드 + decision 로그로 구현하면 진짜 "어느 세션이든 알잘딱깔센"이 됨.
- **동적 총감독 라운드1 desk 판정**: 소프트 advisory orchestrator-lease(하드 fencing 아님 - 직렬 큐가 부패 이미 막음, 2-3머신 규모엔 과공학). 실버그=허브가 task큐를 메시지채널로 쓰며 complete ack 안 해 submitted 쌓임 → hydration open-tasks 오염(받은 task ack 강제 또는 채널 분리).

## ④ 미완 / 다음 후보

- **동적 총감독 hydration 구현**(`tunaround hydrate` + 1급 decision 로그) = 이 세션 핵심 설계 산출.
- discover 데몬을 main 빌드로 재기동 + launchd 상주화(세션·재부팅 생존)는 후속.
- 4자 감독의 launchd/Windows Service 상주화(세션-바운드 해소, 티키타카 결론).
- 자율 mesh 유지 여부 = 다음 세션 판단(위 설계 교훈).

## ⑤ 규율

`checklist.md`·`context-notes.md`(#7). cargo=Bash. 한국어 마침표(#5)·새파일 첫줄 역할주석(#6)·em-dash 금지. **PUBLIC 레포=토큰/LAN IP/사설호스트 평문 금지**. git 교통정리: CLAUDE.md 현재상태 서술·WIN 포인터=Windows 단독, MAC 최신 줄=맥만. 검증·commit·push 분리, push 전 pull --rebase.
