---
title: tunaRound 핸드오프 - 2026-07-08 맥 (자율 mesh 운영 + autoarm 전면화 + 제품철학 정립)
type: prompt
status: active
priority: P0
updated_at: 2026-07-08
owner: mac
summary: 맥 세션에서 win 위임 task 7건 자율 처리(char-패닉 실버그 수정 PR #21 머지, 거버넌스 정제 PR #31 채택, 아키텍처 리뷰, autoarm 전면화, discover 상주) + 사용자와 제품철학 정립(개인 악기/기억=moat/역할모델/계보, 메모리 3개). /exit 후 콜드 스타트 예정. autoarm 훅 설치돼 새 세션은 자동무장(presence). 라이브 감독은 Monitor 재-arm 필요.
---

# 맥 핸드오프 (2026-07-08)

> /exit로 종료 예정 = 다음 mac 세션은 콜드 스타트. 이 문서 + `CLAUDE.md`(현재상태 서술=Windows 편집분) + 최신 origin/main으로 시작한다. 라이브값(win 브로커 LAN IP)은 gitignored backend-private.md 참조 - 이 PUBLIC 문서엔 평문 금지.

## ⚠ 콜드 스타트 즉시

**보안/토큰**: 레포 PUBLIC. 토큰은 로컬 `~/.zshrc`의 `TUNA_BROKER_TOKEN` env. autoarm 설정은 `~/.zshenv`(`TUNA_AUTOARM=1`·`TUNA_BIN`·`TUNA_BROKER_CORE`=원격 win 브로커·`TUNA_MACHINE=mac`). 브로커=Windows 호스팅 LAN 포트 8770.

**/exit 후 데몬 상태**:
- 살아남음(nohup detached): `mac-codex-sup` poll(codex-inject narrate) / `discover`(--interval 30 --stale-mins 240 --machine mac) / codex `app-server`(ws 127.0.0.1:8790). 확인=`pgrep -fl "tunaround poll"`, `nc -z 127.0.0.1 8790`.
- **죽음**: 이 세션 Monitor watcher(세션 하네스 바운드) = mac-claude 자율 수신 정지.

**NEW - autoarm 전면화(이 세션에 설치)**: `~/.claude/settings.json` SessionStart 훅(`python3 $HOME/.claude/hooks/tuna-autoarm.py`)이 새 mac Claude 세션을 자동무장한다(detached poll = 브로커 로스터에 presence 등록, uuid=세션id, display=mac-claude-<project>). SessionEnd에 disarm. 즉 **새 세션은 시작만 해도 로스터에 online으로 뜬다**. 단 detached poll은 heartbeat만 하고 이 세션을 wake 안 함.

**라이브 감독(wake-on-task) 복구 레시피(콜드 스타트)**:
1. 새 세션은 autoarm으로 이미 presence 있음. 여기에 wake를 얹으려면 세션 하네스 Monitor로 `tunaround poll --core http://<win-LAN>:8770/mcp --agent <이 세션 session-id> --display-name mac-claude-tunaRound --tags "machine=mac,runner=claude,role=supervised,project=tunaround,session=<session-id>" --interval 15` 상주(토큰은 env 폴백). session-id = 자기 jsonl 파일명(`~/.claude/projects/-Users-d9ng-privateProject-tunaRound/<uuid>.jsonl`).
2. uuid=session-id + session 태그를 줘야 discover 후보와 armed-overlay 병합(중복 방지, #22). 고정 이름(mac-claude-sup) 쓰지 말 것.
3. **raw HTTP MCP(curl)로 claim/complete 권장**(MCP 등록·재시작 마찰 회피): POST initialize -> Mcp-Session-Id 헤더 캡처 -> notifications/initialized -> tools/call(poll_tasks/claim_task/complete_task), Authorization: Bearer 헤더. 이 세션의 curl 스크립트는 세션 스크래치패드에 있어 새 세션엔 없다 = 재작성 필요(mcp.rs get_task/claim/complete 인자: task_id, complete는 result 추가).

## ① 이 세션이 한 것 (win 위임 task 자율 처리 7건 + 실버그 수정)

- **char-경계 패닉 실버그 발견·수정(PR #21 머지)**: `worker.rs` find_header_starts가 한글 task 본문 `\n\n[작업]<한글>`을 32바이트로 슬라이스하다 char 경계 패닉 -> poll watcher exit 101 사망(모든 poll 워처 취약). `&s[..n]` -> `s.get(..n)` 경계안전 + 회귀테스트. 서비스 다운 긴급이라 직접 PR(관리자 예외).
- **거버넌스 정제 제안·채택(PR #31)**: 관리자를 '브랜치+PR 구현'에서 **'진단·리뷰·repro·스펙 산출 중심, 실제 코드변경은 총괄 or 총괄-dispatch 실무자 worktree로 집중'**으로 좁힘. 내 관찰(직접 PR이 규약은 지켜도 코드 분산)을 보스 채택.
- **아키텍처 리뷰(task c1a93ce)**: store->orchestrator Utterance 역결합, 토큰 전파 3층 분산+401 자가복구 부재, god파일 3개(mcp/sqlite/main), codex 감독 4부품 fragility -> work --runner codex 통합 제안. 보스가 오늘 대부분 반영(sqlite/mcp god분할, config 분리, Utterance 역결합 제거, SSRF 수정).
- **discover 진단 2건**: mac candidates 안 뜨던 근본원인 = discover가 옛 토큰 stale -> report 401 -> 0건. 재기동으로 해결. "10분 stale window" 가설은 반증(10->240 N=1 동일, 유휴 세션은 mtime 4h+).
- **codex app-server 재기동(task f2405a6)**: 옛 토큰 env -> tuna-broker MCP 401. 새 토큰 env로 8790 재기동, test 주입으로 list_agents 성공 검증(401 해소).
- **autoarm 전면화 setup(task 7a7bcbc5)**: 위 "콜드 스타트" 참조. mac 특이 3개 규명(TUNA_BROKER_CORE 원격 필수 / TUNA_MACHINE=mac / mac엔 python 없어 python3만). settings.json 기존 훅 전부 보존.
- **discover 상주(task 7b2cbd18)**: stale-mins 240 --machine mac 재기동. mac 유휴 세션이 대시보드 candidates에 age와 함께 뜸(활성<60분/유휴>=60분, v2-41).

## ② 제품 철학 정립 (이 세션 핵심 - 사용자와, 메모리화)

> Claude가 몇 턴 moat/경쟁 프레임으로 헤매다 사용자가 교정. **다음 세션은 이 프레임에서 시작할 것.**

- **개인 악기**(팔 제품 아님). 비슷한 문제의식 가진 사람이 잘 쓰면 보너스, 목표 아님. moat/경쟁 렌즈로 분석하면 틀린다(척도=personal-fit+장인정신).
- **바퀴 재발명 안 함**: 토론=tunaFlow(더 정교), 순수 오케=Hermes/OpenClaw. tunaRound=off-the-shelf에 없는 **연결조직만**(자기 레포+자기 기억 위 HITL 콕핏).
- **상시 모니터링=의도적 HITL affordance**: OpenClaw 상시(사람 빼기/도달, 대가 토큰지옥)와 반대로, 사람이 적시 개입하려는 것. human-enabling.
- **기억=핵심 투자**: 장기기억은 사람 불가·에이전트는 diary(DB)로 가능. secall 회상 품질이 유일한 복리 자리. **/clear(fresh context=drift 차단) + diary(외부화=amnesia 차단)** 가 트레이드오프 동시 해결. 좋은 기억->감시 필요↓->마이크로매니징을 적시 HITL로 전환.
- **역할 모델**: 총괄=사람과 대화하는 아키텍트 / 관리=헤드리스 부리거나 직접 하는 워커매니저(tunaLlama=값싼 워커) / 실무자=헤드리스.
- **계보**: tunaRound=튜나 시리즈 정점의 통합면(tunapi->orchestrator, tunaFlow->runner, tunaSalon->session_bus, secall->search, tunaLlama->위임, claude-mem->다이어리).
- 메모리: [[product-philosophy-personal-instrument]], [[token-rotation-frozen-env]], 기존 [[multiagent-structure-vs-autonomy]].

## ③ 기술 교훈

- **env-frozen 토큰(중요)**: 실행 중 프로세스 env는 launch 고정 -> 데몬 401 시 env 재로드 self-heal 무효(옛 토큰만 봄). 토큰 로테이션 = 모든 장수 데몬(poll·discover·app-server) 수동 재기동. 근본은 live 토큰 소스(토큰파일/레지스트리/브로커 grace-period dual-token) 별도 설계. [[token-rotation-frozen-env]].
- **discover liveness = jsonl mtime**: 열려있지만 유휴(입력 없음) 세션은 jsonl 안 써서 mtime stale -> 발견 불가. "열린 세션 N개" != "최근 mtime 세션 N개". stale-mins 240으로 유휴창 덮음.
- autoarm 훅 mac 필수 env(win 레시피엔 없음): TUNA_BROKER_CORE(원격, 기본 127.0.0.1 아님)·TUNA_MACHINE=mac(없으면 unix)·python3(mac엔 python 없음).

## ④ 미완 / 다음 후보

- **codex 감독 하이브리드(사용자 결정)**: app-server 유지 + 관전을 상주 ws thread(resume --remote)가 아니라 대시보드 SSE로. 총괄에 스펙 전달함(48a0dbb2), 구현 대기.
- **launchd 상주화**: discover·mac-codex-sup·app-server가 nohup이라 재부팅엔 안 살아남음. launchd plist가 남은 항목.
- **토큰 로테이션 근본해결**: 위 env-frozen 교훈, live 토큰 소스 설계.
- 자율 mesh 유지 여부: 이번 세션도 자율로 길어졌음(정당한 작업이라 낭비는 아니나, 상시 자율은 [[multiagent-structure-vs-autonomy]] 경계). 다음 세션이 판단.

## ⑤ 규율

- **관리자=진단·리뷰·스펙(새 거버넌스 #31)**. 코드변경은 총괄 or 총괄-dispatch 실무자 worktree로 집중. 직접 편집·PR은 서비스 다운 긴급 예외.
- main 직접 push 금지(MAC 최신 포인터 줄 + 자기 핸드오프 파일 = 규약 예외). git 교통정리: MAC 최신 줄만 맥 편집, 현재상태 서술·WIN 줄=Windows. push 전 `pull --rebase`.
- **PUBLIC 레포 = 토큰/LAN IP/사설호스트 평문 금지**(이 문서 포함 - win LAN은 backend-private.md). 검증·commit·push 분리. cargo는 Bash 툴로. 한국어 마침표(#5)·새파일 첫줄 역할주석(#6)·em-dash 금지.
