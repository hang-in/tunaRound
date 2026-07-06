# tunaRound - Claude Code Handoff

> 이 파일은 다음 세션이 이어가기 위한 핸드오프입니다. 제품/설계 전모는 [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md)(현행 spec).

## 표기 / 작업 규칙 (tuna 생태계 공통)

- 사용자 응답·문서는 **한국어 존댓말**. **em-dash 사용금지**(일반 대시 `-` 또는 콜론 `:`). ANSI 박스 드로잉 자제.
- 도메인 도착 URL/도메인은 비노출(소스공개, 서비스 비공개).
- 구현 위임 우선순위(2026-07-06 개정): **① tunaLlama(kimi-k2.7-code:cloud) → ② A2A codex 감독 → ③ Sonnet 서브에이전트.** 아키텍트·스펙·리뷰·검증은 **Opus**. (상위 순위가 스코프/컨텍스트 한계로 막히면 다음 순위로 폴백.)
- 한 세션 한 목적. 검증(build/test)과 commit/push는 분리.

## 맥↔윈도우 협업 규약 (git 교통정리, 2026-07-03)

> 충돌 발원지 = CLAUDE.md "현재 상태" 서술 + 진입점 포인터를 양쪽이 같은 줄로 다시 써서 부딪힘. 규약으로 같은 줄 경합을 없앤다.

- **"현재 상태" 서술 블록 = Windows(home) 단독 편집.** 맥은 서술을 건드리지 않는다. 맥 상태는 맥 자기 핸드오프 파일에 적고, CLAUDE.md 서술 갱신이 필요하면 Windows에 위임한다(또는 A2A task).
- **진입점 포인터 = 줄 분리.** `WIN 최신:` 줄은 윈도우만, `MAC 최신:` 줄은 맥만 편집한다. 서로 다른 줄이라 rebase가 깔끔히 병합된다.
- **세션별 상태·서술 = 각자 자기 핸드오프 파일**(`_session*.md`=윈도우 / `_mac-*.md`=맥). 한 파일을 양쪽이 동시에 열지 않는다.
- **rebase 위생(공통).** 세션 시작 + 매 push 직전 `git pull --rebase origin main`. 작은 커밋 자주 push. 공유 파일 편집을 핸드오프 너머로 들고 있지 않는다.

## 총괄/관리자/실무자 협업 위계 (멀티 에이전트 공유 레포, 2026-07-06)

> 여러 Opus/에이전트가 한 레포에서 동시에 일하며 충돌·중복이 생긴다(실측: 동시 버전 범프 위기, main 직접 편집, 코드 미커밋 방치, 스테일 브랜치). 역할별 권한·격리로 정리한다. **이미 만든 A2A 브로커가 조율 레이어다**(task 배정=선점).

- **총괄** (사람 자리 = win, 통합자): **main 머지 독점.** 방향 결정. **공유 파일 소유**(CLAUDE.md · README · Cargo.toml · CHANGELOG). 관리자/실무자 산출을 PR로 받아 CI green 후 머지한다.
- **관리자** (라이브 TUI 감독, role=supervised): 자기 **브랜치**에서 구현하고 **PR로 제출**한다. main 직접 push 금지(자기 핸드오프 포인터 줄 = 위 규약 예외만). 배정된 task 스코프 안에서만 움직인다.
- **실무자** (헤드리스 워커, role=worker): **1 task → 1 브랜치/worktree.** worktree로 물리 격리(병렬 충돌 원천 차단). 무관 파일 손대지 않고, 끝나면 `complete_task`(실패 시 `fail_task`)로 보고한다.

**핵심 규칙 5개**:
1. **사소하지 않은(non-trivial) 변경 = 브랜치.** main 직접 편집 금지, main = 총괄 머지 전용(CI green 전제).
2. **한 브랜치 한 에이전트.** 병렬 워커는 worktree로 격리(같은 브랜치 동시 편집 금지).
3. **작업 선점 = A2A task로.** 총괄이 goal/send_task로 배정 → 에이전트 claim = "누가 뭐 하는지" 직렬화(중복 작업 방지). 큰 작업 전 로스터/tasks로 겹침 확인.
4. **미커밋 들고 있지 않기.** 코드 변경은 곧 브랜치 커밋(작업 공간/실행에 반영됐다고 커밋된 것 아님). 스테일 브랜치는 정리한다.
5. **동기화 위생**(위 git 교통정리와 동일): 세션 시작·push 전 `pull --rebase`, 작은 커밋 자주, 공유 파일은 총괄 경유.

## 개발 행동 규율 (이 프로젝트 실험 적용, 2026-06-29)

> 전역 규칙 아님. 이 레포 실험 적용. **전문·근거·예시·위임 라우팅은 [docs/reference/development-guidelines.md](docs/reference/development-guidelines.md)**.
> 10개 중 #1·#2·#3·#4·#8·#9·#10은 전역 COMMON.md가 이미 always-on으로 강제하므로 여기 중복하지 않는다. 아래는 이 프로젝트 신규 3개만 둔다.

- **#5 한국어 문장 끝은 마침표.** 리스트/예시 앞이라도 `:`로 끝내지 않는다. 콜론은 라벨·key-value·문장 중간만.
- **#6 새 소스 파일 첫 줄 = 역할 한국어 한 줄 주석.** Rust 예: `// 토론 라운드 프롬프트를 조립하는 순수 함수`. config 파일 제외.
- **#7 비trivial 작업 전 plan + `checklist.md` + `context-notes.md`.** plan만 주고 코딩 요청 시 멈추고 checklist·notes 먼저 만들지 묻는다.

## 현재 상태 (2026-07-06, 세션 6~14. 세션14 = 총감독 대시보드 완성(목업 React SPA) + goal loopback + v2-40 세션버스 설계)

- **세션 14 (2026-07-06): roster 복구 → 총감독 대시보드 완성(목업 React 이식) → v2-40 설계.** ① roster 복구(win watcher `--tags` 재기동 + 맥 자율 재기동 = 3자 감독 online). ② **대시보드 v2-38 T1-T3**(라우트 + 전역 SSE 피드 + roster JSON + goal 폼) → **v2-39 DaleUI SPA(rust-embed 임베드, `dashboard` cargo feature)** → **Claude Design 목업을 plain React로 재이식**(DaleUI 제거, 번들 258→205KB; 통계타일·총감독 표식·heartbeat 애니·shields 값별뱃지·체크박스 멀티선택). ③ **goal 백엔드** `POST /dashboard/goal` = **loopback 무토큰 / 원격 403 read-only 관전**(ConnectInfo). 결정: 로컬=풀컨트롤, 원격=관전. ④ **디자인 반영**(사용자 피드백): 로스터-피드 레이아웃 통일 / 뱃지 값별색 / 아이콘 정돈 / 총감독=대등카드+★토글 지정. ⑤ README 최신화 + **v2-40 유니버설 세션버스 설계**([v2-40](docs/design/v2-40-universal-session-bus_2026-07-06.md): 임의 세션 A2A 주소화·발견·제어, 자동무장 SessionStart 훅) + **Planka 백로그 보드**. **다음=1) PR #12 머지 → 2) v2-40 S1(자동무장 훅).** 브랜치 `feat/orchestrator-dashboard`(bec79fe, PR #12). 진입점 [dashboard-v2-40 핸드오프](docs/prompts/v2-handoff_2026-07-06_dashboard-v2-40.md).

- **세션 13 (2026-07-06): 4자 감독 무-셔틀 mesh 라이브 + 통합 총감독 대시보드 착수.** v2-37 codex 라이브 감독(PR #9=`252e09e`: `codex app-server ws`+`codex-inject` turn/start 주입, 헤드리스 exec 대체)·heartbeat(PR #10=`d7deae3`: poll에 register/heartbeat+`--tags`) 머지. **4자 감독**(win-codex-sup/mac-claude-sup/mac-codex-sup + **win-opus-boss 허브**) 크로스머신 자율 수신·3~4자 티키타카 실증(사람 릴레이 0, 허브도 **Monitor 인박스 자동수신**). 동적 총감독(자리=역할, hydration ritual)·codex narrate 수정(과정 가시)·hydration으로 roster/ack 갭 empirical 발견·정리 실증. **구현 위임 규율 개정**: ①tunaLlama(kimi-k2.7-code:cloud) ②A2A codex ③Sonnet, 아키텍트=Opus. **통합 총감독 대시보드** 설계 정본([v2-orchestrator-dashboard](docs/design/v2-orchestrator-dashboard-and-dynamic-boss_2026-07-06.md))+계획 [v2-38](docs/plans/v2-38-orchestrator-dashboard.md), **T1**(`/dashboard` 라우트+스켈레톤, tunaLlama 생성→Opus 리뷰→적용, 라이브 200) 완료(브랜치 `feat/orchestrator-dashboard`, main rebase). **다음=1) roster 복구(heartbeat로 to_selector 복구) → 2) 대시보드 T2/T3(tunaLlama).** 진입점 [orchestrator-dashboard 핸드오프](docs/prompts/v2-handoff_2026-07-06_orchestrator-dashboard.md).

- **세션 12 (2026-07-04): agentgateway 선별 도입 검토 → D·B·C 3개 PR 머지.** 검토 결론=capability는 태그로 이미 됨 + 값싼 trace/denylist만 취하고 정책엔진·별도 backend registry·lineage DAG는 비채택([검토 노트](docs/design/v2-agentgateway-selective-adoption_2026-07-04.md)). **D**(doctor Stage 4, PR #6=`89cdbf2`): Kiwi/형태소 백엔드 + Ollama 도달 진단. **B**(trace+guard, PR #7=`27f04e6`): tasks `runner` 컬럼(스키마 v8, claim 시 기록) + 쓰기 민감 path 가드(WRITE_GUARD_DIRECTIVE, behavioral). **C**(node태그, PR #8=`5f3ec50`): node.toml lane `tags` 배선(node 워커도 셀렉터 발견) + doctor/node 태그형식 검증. 4개 PR 전부 3-OS CI green + CodeRabbit 반영. 스키마 **v8**(tasks.runner). **위임 vs 회담 정리**: A2A=task 위임(1 task→1 워커, 다중매칭=사람선택), 라이브 다자 토론은 단일머신 `chat --roster`(크로스머신 토론은 비목표). 태그 셀렉터로 프로젝트별 격리(project= 태그 규율 전제). **다음**: Mac 노드 태그 등록 온보딩(프롬프트 준비됨) / R9 poll 견고화(옵션) / 릴리스 cadence.

- **세션 11 (2026-07-04): 에이전트 레지스트리(UUID 라우팅 + 태그 발견) 구현 완료·머지(PR #5=`2bbf3d3`).** 어드레싱을 자유 문자열에서 UUID(라우팅)+태그(발견)로. 로스터=`SqliteStore.agent_roster`(RefCell 인메모리, `/a2a`·MCP 양 경로 공유). T1 데이터레이어(`src/store/agents.rs` parse_tags/selector_matches/is_online) / T2 MCP 도구 register_agent·heartbeat·list_agents + send_task `to_selector` / T3 `/a2a` SendMessage `toSelector`(공유 헬퍼 store/agents.rs로 이동) / T4 워커 `--tags`·자가 uuid·자동 register/heartbeat(재기동 시 재등록) / T5 문서+라이브 스모크 4/4. 다중매칭=후보반환(사람선택). 하위호환 레거시 `to_agent` 불변. CodeRabbit 4건(R1 에러계약) 반영. 풀피처 414 pass, 3-OS CI green. 정본 [레지스트리 설계](docs/design/v2-agent-registry-uuid-tags_2026-07-04.md) · 계획 [v2-34](docs/plans/v2-34-agent-registry.md) · 사용법 [a2a-usage §9](docs/reference/a2a-usage.md). **부수**: [agentgateway 선별 도입 검토](docs/design/v2-agentgateway-selective-adoption_2026-07-04.md)(결론: capability=태그(됨)+값싼 trace/denylist만 취함, 정책엔진·별도 backend registry·lineage DAG 비채택). **다음 후보(권고 순): D doctor Stage 4 → B(tasks flat trace 컬럼+쓰기 민감 path denylist) → C(config→런타임 태그 seed·backend를 named seat 은닉).**

- **세션 10 (2026-07-04): 브로커 거버넌스 #1~#5 구현 + v0.2.0→0.2.2 릴리스(cargo-dist 6타깃·brew).** 세션9 두 실패(no-consumer·self-disruption)를 구조적으로 제거. #1 네이밍 컨벤션 문서 / #2 빌드 피처 광고(Agent Card `buildFeatures`) / #3+#4 고착·no-consumer 표시 신호 + `tasks` 조망 MCP 도구 / #5 워커 격리 가드레일(`write_lane_disrupts_node`). + `poll --on-task`(0토큰 감독, PR #3) + claim-후-워커사망 자동 requeue(lease 기반, PR #4). PR #1~#4 전부 3-OS CI green 머지. 스키마 **v7**(tasks lease 컬럼). ⚠ Cargo.toml `version="0.2.2"`(더 이상 rc 아님, cargo-dist+brew 정식 배포 중).

- **세션 9 (2026-07-03): R1-R10(PR #1, CI가 R3 Unix버그 포착)·poll 감시자(PR #2·3)·node 고도화 init/node/doctor(PR #3·4·5, 리뷰 8건) + 크로스머신 node 양방향 실증 → 공개 준비.** **filter-repo로 히스토리 272커밋 시크릿 퍼지(사설호스트·SSH·LAN IP·토큰 placeholder화) → 새 PUBLIC `hang-in/tunaRound`(히스토리 보존, 시크릿 0), 옛 것 `-private` rename**(공개 못했던 이슈 해소). README 뱃지·명령 갱신. 발견 2건(win-opus=dispatcher는 폴러 아님 / 워커가 자기 node 클론 write=self-disruption 자살)을 거버넌스로 정리. **다음 세션 = 거버넌스 구현.** 정본 [브로커 거버넌스](docs/design/v2-broker-governance_2026-07-03.md) · [node 온보딩](docs/design/v2-node-onboarding_2026-07-03.md) · [usecase](docs/reference/agent-dev-team.md). ⚠ 레포 PUBLIC이니 문서/코드에 LAN IP·토큰·사설호스트 평문 금지.

- **세션 6: rc.1 CI green 확인 + Windows 아티팩트 검증 + 사설 IP 전방 redact(backend-private.md 패턴) + Stage 3e 킬 -> semi-a2a 파트너 위임(A2A 표준) 설계·Phase 1 코드(Task 1~4 = 데이터레이어·`/a2a` 엔드포인트·worker inbox·dispatcher 툴) 완성·푸시 + Task 5 라이브 크로스머신 도그푸딩 착수.** 검증 기본 209 / 풀피처 262 lib pass. 스키마 **v6**(tasks). 정본 [semi-a2a 파트너 위임](docs/design/v2-a2a-partner-delegation_2026-07-02.md).

- **세션 7 (2026-07-03): Task 5 크로스머신 왕복 성공 = semi-a2a Phase 1 완료.** 윈도우 코어(`serve 0.0.0.0:8770`, LAN 192.0.2.10) `/a2a` SendMessage(win-claude→mac-claude) → 맥 worker poll/claim/complete → 윈도우 GetTask=completed+artifact, 소스 교차검증 통과(task_id 83f0e576, 19:11→19:17 맥 HITL 포함). + **맥↔윈도우 git 교통정리 규약**(단일 통합자 + 진입점 포인터 WIN/MAC 줄 분리, `37a8ee1`).

- **세션 8 (2026-07-03): mac→win 역방향 왕복 성사 = 크로스머신 양방향 다 실증.** 재부팅으로 이전 background 코어(+temp db)가 죽어 옛 task(`907f5c82`) 소멸 -> Windows 코어 재기동(`serve 0.0.0.0:8770`, 안정 db=LOCALAPPDATA) -> 맥이 새 task `76ea0b9c` 재디스패치(같은 주소/토큰, MCP 재등록 불요) -> **win-claude가 raw curl MCP**(claude mcp 등록·세션 재시작 없이 initialize->notifications/initialized->poll_tasks->claim_task->complete_task)로 처리 -> get_task=completed+artifact 자기검증. **교훈 2개**: (a) 워커는 raw HTTP MCP 직접 호출로 "MCP 등록+세션 재시작" 2세션 온보딩 마찰(#1)을 회피 가능(대가=대화형 도구승인 UX 없음). (b) 코어 리셋 시 옛 task_id는 조용히 "task 없음"으로 소멸하고 리셋 신호가 없다 -> dispatcher가 죽은 id를 계속 폴링(discovery/상태변화 통지 채널 공백 = 마찰 #3와 동근). 맥 반영=`e073329`(`_mac-rc1.md ⑦`).

- **세션 8 (2026-07-03, 후반): A2A 스트리밍(SSE) Phase 2 완료 = 표준 A2A 서버로 스트리밍 지원.** "복붙 UX면 A2A 왜?"(복붙=트리거 릴레이, 코어는 이미 poll/get_task로 노출) 논의 -> 정찰은 이미 끝나 있었고(스펙 §9.1·§5.3 인용, `SubscribeToTask` 명명, 카드 streaming 플래그 존재) `§65 "SSE 후속" 유예를 interop·호기심 근거로 재개. 정본 [스트리밍 설계](docs/design/v2-a2a-streaming_2026-07-03.md). T1 이벤트 버스(store 계층 broadcast, /a2a·MCP 두 경로 자동 커버, `785fb25`) / T2 스트리밍 serde 타입+매핑(`25619c4`) / T3 `SendStreamingMessage` SSE(subscribe-before-create, `9ed6380`) / T4 `SubscribeToTask` 재구독(`ea3e855`) / T5 카드 `streaming:true`(`2bc5437`) / T6 **라이브 데모 성공(복붙 0)**: boss SSE 개방 -> 워커 MCP claim/complete -> 같은 버스로 submitted->working->artifact->completed(final) 실시간 스트림 후 종료. 검증 기본 218 / 풀피처 279 lib pass. **스코프 경계**: 워커 방향 push는 미구현(브로커 폴링 유지), push_notifications(webhook)·discovery는 후속(YAGNI).

- **세션 8 (2026-07-03, 후반2): 크로스머신 SSE 스트리밍 스모크 성공 + A2A 자율 워커 데몬(a·b) 완료.** 크로스머신 스모크: 맥=원격 dispatcher가 SendStreamingMessage를 LAN 너머 SSE로 개방 -> Windows worker claim/complete -> 맥 SSE에 4프레임 실시간 도착(SSE-over-LAN 실증). "복붙 아직 맞지?" 논의 = SSE가 데이터/완료통지는 제거, 남은 트리거 릴레이는 **워커 auto-poll로 제거** -> 자율 워커 데몬 착수. 정본 [워커 데몬](docs/design/v2-a2a-worker-daemon_2026-07-03.md). W1 프로덕션 MCP HTTP 클라(`ad5ca38`) / W2+W3 `tunaround work` 서브커맨드+자율 루프(poll->claim->Runner.run->complete, `60364d8`) / W4 **로컬 데모(사람 트리거 0)**: dispatcher SSE + `work --once --runner claude`가 자율 발견·claude 실행·완료, SSE에 claude 실답변 artifact 실시간. **(b) 이기종**: `--runner codex`로 **Codex가 워커**(GetTask=completed+codex 답변) = **(a)=(b), --runner만 교체=파트너 교체**(`a94809b`). Ollama-http(`--runner http`)는 코드완성이나 로컬 GPU OOM으로 라이브 미검증(후속). 검증 기본 218 / 풀피처+worker 286 lib pass. 스코프: opt-in 데몬, read-only 기본, dispatcher 사람 목표발행(semi-a2a HITL).

- **세션 8 (2026-07-03, 후반3): 워커 후속(fail전이·프로젝트라우팅·Ollama) + A2A interop 방향 확정 + outbound 표준 A2A 위임 실증.** (1) Ollama-http 워커 라이브 성공(GPU 언로드 후, `8c9f6d6` reqwest::blocking을 std::thread로 = 3번째 파트너 로컬LLM). (2) **fail 전이**(`abc4f1e`): 러너 실패 시 completed 위장 대신 `fail_task`로 state=failed(dispatcher가 성패 구분). (3) **context_id 프로젝트 라우팅**(`bc70e29`): poll에 ctx= 노출 + `--context-map`으로 데몬 하나가 여러 프로젝트 배분. (4) **A2A interop 스모크**(독립 a2a-client로 우리 코어 외부검증): GetTask/envelope는 호환이나 Agent Card(인증게이트+구식스키마)·SendMessage(브로커 fromAgent/toAgent 필수)는 제3자 미호환 = "표준 A2A" 문구 정직화(`e922534`, "A2A 기반"으로). (5) **방향 결정**(동구님): inbound(제3자가 우리한테) 폐기(오픈소스라 레포 쓰면 됨), **outbound(우리가 표준으로 던짐)만 구축**. (6) **A2ARunner**(`6399443`, WA1+WA2): a2a-client로 외부 표준 A2A 에이전트에 위임 = 4번째 러너 `--runner a2a`. **WA3 outbound interop 성공**: 독립 표준 서버(radkit) 상대 왕복 실증(카드발견->SendMessage->task완료->artifact). 검증 기본 218 / +worker+a2a-out 304 lib pass. 정본 [outbound 러너](docs/design/v2-a2a-outbound-runner_2026-07-03.md) · 사용법 [a2a-usage](docs/reference/a2a-usage.md).

- **세션 8 (2026-07-03, 후반4): 제미나이+코덱스 리뷰 기반 리팩토링을 A2A 3자 도그푸딩으로 수행(브랜치 `refactor/reviews-2026-07-03`, 8/9 완료).** 리뷰 삼분류→계획([리팩토링 계획](docs/plans/v2-refactor-from-reviews_2026-07-03.md), 원문 docs/reviews/). **3자 분담**: Opus 통합자(R4·R1R2·R10 = Sonnet 서브) / **Codex 워커 A2A**(R6·R3) / **Mac 워커 A2A LAN**(R5) / tunaLlama→직접(R8). 실버그 4개 잡음(R1 MCP성공위장·R2 이중claim·R3 부모만kill·R5 orphan). **도그푸딩 findings**: (a) R10=워커 세션만료 404 발견·수정(`c58df41`), (b) 동시 워커는 워크트리 격리 필요, (c) 워커=헤드리스 데몬(fresh spawn)이 handoff·/clear 불요, (d) tunaLlama는 config 필요, (e) 방법론=GitHub Flow+PR CI가 semi-a2a에 적합, (f) 통합자가 브랜치 push를 git-watch로 auto-poll=사람 릴레이 0. **남음**: R7(Mac, 큼)·브랜치 머지·PR CI 도입·usecase 문서. 브랜치 310 pass. **진입점=[session8-refactor 핸드오프](docs/prompts/v2-handoff_2026-07-03_session8-refactor.md).**

- **세션 5: 시간성·유효성 마무리(step 5c·6) + codex pull 활성화(behavioral) + 외래어 병기 색인 + 임베딩 기본 qwen3 + 배포(cargo-dist)·온보딩(clap 서브커맨드·tunaround.toml 프로파일) + AGPL-3.0 + 맥-윈도우 핸드오프.** 전부 origin/main 푸시(= c89da05).
- **이전 세션 4: Stage 3a-3(front=core) + Stage 3d(원격 쓰기 권위) + 시간성·유효성 로드맵 step 2~8.**
  - **3a-3**: `--core <addr>` 단일 프로세스(REPL+in-process HTTP MCP 코어). **서버=전용 OS 스레드 block_on**(공유 rt spawn은 유휴 중 간헐 신뢰불가). 라이브 e2e.
  - **3d(옵션 B front=core 병합)**: `append_turn`(증분·DB id 권위) + `post_turn`/`get_roster` MCP + REPL core-sync(adopt+append, 클로버 차단). 라이브 e2e: 원격 post_turn→흡수→claude 인용.
  - **로드맵(외부 memory 프레임워크 리뷰 후, SQLite-light·graph DB 비채택)**: step2 model_id 무효화키(실버그) · step3 retrieved 길이·세션 다양성 cap · step4 message_validity 테이블(스키마 v4) · step5 유효성 랭킹+/supersede·/reject · step5b 분기/세션 인지 랭킹 · step7 /explain 디버그 · step8 --reindex.
- **v1 + v2 검색/맥락 로드맵(step 2~8) + Stage 3a~3d + codex pull(behavioral) + 실코퍼스 회귀(step6) + 외래어 병기 + 임베딩 qwen3 + 배포/온보딩(clap·cargo-dist·프로파일) 완성.** 검증: **기본 184 lib+6 cli / `--features "semantic morphology mcp serve"` 198 lib+9 cli pass, clippy 클린(no-default 포함).** 스키마 **v5**(created_at).
- 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md). 진행: [docs/plans/index.md](docs/plans/index.md).
- **>>> 진입점 먼저 읽기 (각 줄은 해당 머신만 편집) <<<**
  - **WIN 최신**: [dashboard-v2-40](docs/prompts/v2-handoff_2026-07-06_dashboard-v2-40.md) - **총감독 대시보드 완성(목업 React SPA + goal loopback) → PR #12**. 다음=1) PR #12 머지 → 2) v2-40 유니버설 세션버스 S1(자동무장 훅). 작업 브랜치 `feat/orchestrator-dashboard`. 라이브값=gitignored backend-private.md. (이전: [orchestrator-dashboard](docs/prompts/v2-handoff_2026-07-06_orchestrator-dashboard.md) / [supervised-a2a](docs/prompts/v2-handoff_2026-07-04_supervised-a2a.md).)
  - **MAC 최신**: [mac-a2a-supervisor](docs/prompts/v2-handoff_2026-07-06_mac-a2a-supervisor.md) - 맥: 4자 크로스머신 자율 A2A 감독 mesh + 양방향 무-셔틀 실증 + v2-40 discover 배포. **/exit 콜드스타트용**: 세션-바운드 mac-claude-sup Monitor watcher 재-arm 필요(nohup 데몬=codex sup·discover·app-server는 생존). 설계 교훈=구조>자율(GN)·동적 총감독+hydration. (이전 [mac-rc1](docs/prompts/v2-handoff_2026-07-03_mac-rc1.md).)
- 이전 [session5](docs/prompts/v2-handoff_2026-07-02_session5.md). 맥↔윈도우 왕복은 [docs/reference/dev-mac-windows.md](docs/reference/dev-mac-windows.md). 협업 규약은 위 "맥↔윈도우 협업 규약" 섹션.
- **Cargo.toml `version="0.2.2"`**(세션10부터 정식 배포, 더 이상 rc 아님). cargo-dist 6타깃 + homebrew-tap 발행 중(cargo-release로 bump). 릴리스 교훈=[dev-mac-windows §6](docs/reference/dev-mac-windows.md). 정본 방향: [배포·온보딩](docs/design/v2-deploy-onboarding_2026-07-02.md) + [A2A](docs/design/v2-A2A-core-backend_2026-06-30.md) + [시간성·유효성](docs/design/v2-temporal-validity-direction_2026-07-01.md).
- **⚠ 서버 호스팅 교훈**: `--core`(=`core` 서브커맨드)는 메인이 동기 블로킹 REPL이라 서버를 **전용 스레드 block_on**으로 서빙(공유 rt spawn 신뢰불가). 라이브 e2e 타이밍 함정(Kiwi ~3초/FIFO 미flush/agent ~35초) → 준비 폴링 + 파이프 입력 + 넉넉한 타임아웃.
- **남은 항목**: 공개 릴리스=**`v0.1.0-rc.1` 먼저**(맥 도그푸딩 판정: 6타깃 CI 미검증이라 rc로 CI 검증 후 최종 태그. 상세 [release-readiness](docs/reference/release-readiness-v0.1.0_2026-07-02.md)) · 온보딩 Stage 4 doctor · abstraction/anchors 생성 파이프라인(보류=YAGNI) · **분산 크로스머신 스모크=claude leg 통과**(맥.184→윈도우.179 read_transcript 실증 2026-07-02), codex leg는 승인취약(#24135)→app-server(3e) 후속 · 홈랩 코어 호스팅(보류) · opencode 검색 배선.
- **맥 검증(2026-07-02, 완료)**: 맥 aarch64 빌드·테스트·`cargo install`·2에이전트 도그푸딩 전부 통과(크로스컴파일 이슈 없음). Kiwi 자산 404→lindera 폴백 정상.
- **완료된 이전 남은항목**(참고): step 5c·6·codex pull·codex bearer-env·잠재리뷰(bounded bus/snapshot log/Kiwi 주석) 전부 이번 세션에 처리됨.
- **검증/주의:** 임베딩=원격 Ollama(SSH `-p [사설포트]` 터널, dim 1024). 기본 모델 `qwen3-embedding:0.6b`(bge-m3보다 hybrid MRR 우위 측정), `TUNAROUND_EMBED_MODEL`로 교체. Redis 6379(3d/랭킹엔 불요).
- **Kiwi(정정 2026-07-02):** kiwi-rs 0.1.4는 **순수 Rust 빌드**(dep=regex만, build.rs·네이티브 링크 없음)라 **macOS/Win/Linux 모두 빌드됨**(kiwi cfg는 linux-aarch64만 제외). libkiwi(.dll/.dylib/.so)+모델은 **런타임에 bab2min/Kiwi 릴리스에서 다운로드**(캐시=OS cache dir 또는 `KIWI_LIBRARY_PATH`/`KIWI_MODEL_PATH`/`KIWI_RS_VERSION` env). 과거 "libkiwi 404"는 빌드가 아니라 런타임 자산 다운로드 실패(버전/자산)로, `scripts/install-kiwi-windows.sh`(Windows 전용)가 캐시를 pre-seed해 우회(맥/리눅스 전용 스크립트는 없음 - 자동다운로드 또는 lindera 폴백). 실패해도 **lindera 자동 폴백**이라 빌드·실행 안 죽음. **맥(aarch64) 실측 2026-07-02: libkiwi 0.23.1 dylib 로드 실패 + kiwi_mac_arm64_v0.23.2.tgz 자산 404 -> lindera 폴백으로 정상 동작.** bab2min/Kiwi v0.22.2에 맥 자산 존재(`kiwi_mac_arm64`/`kiwi_mac_x86_64`).

## 무엇을 만드나 (요약)

터미널에서 **사용자가 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론** 도구. 같은 레포 위에서 사람 주도로 토론하고, 결론을 **결과 문서로 자동 기록**해 구현으로 넘긴다.

**핵심 결정(brainstorming 2026-06-29):** 사람 주도 대화형 / 공유 컨텍스트 = 같은 레포+공유 문서(컨텍스트팩 없음) / 읽기 전용 화자 + 사람이 쓰기 지목 / 순차-인지 턴 / 자리마다 역할 주입 / v1=2자리 고정 / consensus carry-forward(종료는 사람) / 스택 Rust+tokio.

**레이어(출처):** 에이전트 러너(tunaFlow `claude.rs`/`codex.rs` 포팅) + 토론 오케스트레이터(tunapi `core/roundtable/` 청사진 -> Rust 재구현) + 전사·영속(파일/rusqlite, 트리-ready) + 프론트(thin REPL).

**v1 비목표 -> v2:** Redis 멀티세션 = git-tree 다중 브랜치 / N>2 좌석 로스터(로컬LLM·opencode) / 리치 TUI(ratatui)·웹 / 협업 코딩.

## 출처 레포 (포팅 시 읽기)

- **tunapi**(전전신, Python): `~/privateProject/tunapi/src/tunapi/core/roundtable/` - 토론 오케스트레이터 청사진(`orchestrator.py`/`prompt.py`/`rt_participant.py`/`session.py`). 역할·순차-인지·follow-up·consensus.
- **tunaFlow**(Rust): `~/privateProject/tunaFlow/src-tauri/src/agents/{claude,codex}.rs` - CLI 러너(`stream_run`) + hardening.
- **tunaSalon**(Rust, v2용): `src/session_bus.rs`(Redis), `src/chat.rs`의 `render_chat`(ratatui), `src/flow.rs`(FlowMeter, 선택).

## 다음 세션 첫 행동

1. **[docs/prompts/v2-handoff_2026-07-03_session6.md](docs/prompts/v2-handoff_2026-07-03_session6.md) 먼저 읽기** + `context-notes.md`(상단 세션6) + `checklist.md`("semi-a2a 파트너 위임 Phase 1" 섹션) + 설계 정본 [파트너 위임](docs/design/v2-a2a-partner-delegation_2026-07-02.md). `cargo test`(기본 209) + `cargo test --features "semantic morphology mcp serve"`(262)로 상태 확인(**cargo는 Bash 툴로**).
2. **다음 세션 우선순위 = Task 5 라이브 도그푸딩 재개**(핸드오프 §3~④): 윈도우 코어 재기동(`target\debug\tunaround.exe serve 0.0.0.0:8770 --token [REDACTED_TOKEN] --db <임의 temp>`, PowerShell background) -> 맥 worker(mac-claude, `docs/prompts/a2a-dogfood-mac-worker_2026-07-03.md`) 붙었나 확인 -> `/a2a` SendMessage로 test task 던지고 -> GetTask로 **state=completed+artifact** 검증 = **왕복 1회 = Task 5 성공.** 이후 Phase 2(이기종 파트너 Codex-on-Ollama·A2A interop 갭·SSE). 릴리스(v0.1.0)·IP 히스토리 filter-repo 퍼지는 **배포 비우선**(핸드오프 §6).
   - **A2A 성숙도(정직)**: 현재=공유맥락(데이터평면)+**사람 오케스트레이션(HITL)** = **semi-a2a**(자율수준이 "semi"=HITL, A2A 통신은 진짜 성립). 스펙트럼: 수동relay < semi-a2a < full-auto(AutoLoop=Stage4 미구현, 의도적 보류). 크로스머신 앱-투-앱 위임 설계=docs/design/v2-a2a-partner-delegation_2026-07-02.md.
3. 작업 추적 `checklist.md`·`context-notes.md`(규율 #7). 위임 Sonnet + Opus 리뷰. 굵직한 결정 재론 금지. 서브에이전트 진행 중 파일 레이스 주의. 배포 전 도그푸딩.
