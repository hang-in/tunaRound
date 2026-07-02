# tunaRound v2 핸드오프 - 2026-07-03 세션6 (semi-a2a Phase 1 + 도그푸딩 진행)

> 이전: [session5](v2-handoff_2026-07-02_session5.md). 이 세션 = rc.1 CI green 확인 + Windows 아티팩트 검증 + 사설 IP 전방 redact + **Stage 3e 킬 -> semi-a2a 파트너 위임(A2A 표준) 설계·Phase 1 코드(Task 1~4) 완성·푸시** + Task 5 라이브 도그푸딩 착수.
> 콜드 스타트 가정. 이 문서 + checklist.md + context-notes.md(세션6·세션6후반) + docs/design/v2-a2a-partner-delegation_2026-07-02.md(정본, §12 레시피)로 이어감.

## ⓪ 가장 먼저
1. 이 문서 + checklist.md("semi-a2a 파트너 위임 Phase 1" 섹션) + context-notes.md 상단(세션6 A2A 결정) + 설계 정본 읽기.
2. **cargo는 Bash로**: `cargo test` 기본 **lib 209** / `cargo test --features "semantic morphology mcp serve"` **lib 262** pass 기대. clippy 클린.
3. `git log --oneline -8`. **origin/main = ae2fc71 (전부 푸시).** 워킹트리 클린(untracked `.omc/`·`docs/plans/v2-18~20`·a2a-comm-layer 노트는 세션2 잔여, 무관).

## ① 커밋 (순서, 전부 origin)
- `4633d99` chore(security): 사설 IP·호스트명 전방 redact + backend-private.md(gitignore) 패턴
- `fefff51` feat(a2a): 설계 + Phase 1 Task 1 데이터 레이어
- `ef6e993` docs: half-a2a -> semi-a2a 용어 정정
- `6c522e2` feat(a2a): Task 2 A2A 서버 엔드포인트
- `b9cae15` feat(a2a): Task 3 worker inbox 툴(poll/claim/complete)
- `b1ba880` feat(a2a): Task 4 dispatcher 툴(send/get) + §12 레시피
- `ae2fc71` docs: 맥 worker 도그푸딩 핸드오프

## ② semi-a2a Phase 1 상태
- **Task 1~4 완성·검증·푸시.** tasks 테이블 v6 / A2A `/a2a`(SendMessage·GetTask·CancelTask + Agent Card `/.well-known/agent-card.json`) / worker inbox(poll_tasks·claim_task·complete_task) / dispatcher(send_task·get_task) + create_task_from_message DRY 헬퍼.
- **Task 5(라이브 크로스머신 e2e) = 진행 중. <- 다음 세션 여기부터.**
- 핵심 결정(context-notes 세션6후반): A2A 표준 채택(이기종 interop). **중앙 브로커 토폴로지**(코어=A2A서버+task큐 / worker=MCP inbox 폴링 / dispatcher=MCP send·get). worker=CLI 에이전트(모델=config). `/a2a` JSON-RPC는 Phase 2 외부 interop용. 메서드=ADR-001 PascalCase(SendMessage/GetTask/CancelTask). wire camelCase(TaskState만 snake).

## ③ Task 5 도그푸딩 재개 (다음 세션 핵심)
> **✅ 완료(2026-07-03 세션7): 크로스머신 왕복 1회 성공 = semi-a2a Phase 1 완료.** win-claude `/a2a` SendMessage → mac-claude poll/claim/complete → 윈도우 GetTask=completed+artifact(소스 교차검증 통과, task_id 83f0e576, 19:11→19:17 맥 HITL 포함). 정정: background 코어(Start-Process)는 세션 종료 후에도 생존해 재기동 불필요했음. 아래는 당시 재개 절차(기록 보존).
> **⚠ 이 세션의 background 코어·맥 연결은 세션 종료로 끊김. 아래로 재개.** 빌드는 이미 됨(target/debug).

1. **코어 재기동**(윈도우, PowerShell background):
   `.\target\debug\tunaround.exe serve 0.0.0.0:8770 --token [REDACTED_TOKEN] --db <임의 temp>\dogfood.db`
   - 포트 8770 already in use면 옛 tunaround 프로세스 kill 후 재기동. 빌드 필요 시 `cargo build --features "mcp serve"`(Bash).
   - 로컬 스모크(PowerShell): `/a2a` agent-card(GET, Bearer) + SendMessage(POST, to=smoke-verify) + no-token 401 확인. (이 세션 스모크 통과: agent-card·SendMessage·GetTask·401 전부 OK.)
2. **맥 worker 확인**: 맥이 `docs/prompts/a2a-dogfood-mac-worker_2026-07-03.md` 따라 붙었나. 코어 등록 = `http://192.0.2.10:8770/mcp` + 토큰. worker agent id = **mac-claude**. `poll_tasks agent=mac-claude`가 도는지.
3. **dispatch**(윈도우=Opus, `/a2a` SendMessage curl/PowerShell): mac-claude 앞으로 가벼운 test task. 예 message text = "src/store/a2a.rs의 TaskState enum을 한 줄로 요약해줘". -> 반환 task_id 기록.
4. **검증**: 맥이 claim_task -> 수행 -> complete_task(result) -> Opus가 GetTask(task_id)로 **state=completed + artifact 결과** 확인. **왕복 1회 성공 = Task 5 완료.**

## ④ 라이브 상태값
- 윈도우 LAN IP = **192.0.2.10**(이더넷). 방화벽 8770 인바운드 규칙 존재(`tunaround-smoke-8770`).
- 토큰(throwaway) = **[REDACTED_TOKEN]**.
- 코어 DB = 임의 temp(throwaway, 세션마다 새로).
- dispatch PowerShell 패턴: `Invoke-RestMethod $base/a2a -Method Post -Headers @{Authorization="Bearer <tok>"} -Body (@{jsonrpc="2.0";id=1;method="SendMessage";params=@{message=@{messageId="..";role="user";parts=@(@{text=".."})};fromAgent="win-claude";toAgent="mac-claude"}}|ConvertTo-Json -Depth 8) -ContentType application/json`. GetTask는 method="GetTask", params=@{id=".."}.

## ⑤ Phase 2 후속 (도그푸딩 성공 후)
- 이기종 파트너: Codex-on-Ollama 등 worker 등록, Agent Card로 skills 광고.
- A2A interop 갭(설계 §10 리뷰포인트): Agent Card 최소필드->supportedInterfaces / Agent Card 공개(현 bearer 뒤) / TaskState snake->SCREAMING_SNAKE(TASK_STATE_*).
- SSE SubscribeToTask(스트리밍), historyLength 반영. 통합테스트 reqwest 결합 정리(tower oneshot).

## ⑥ 잔여(비A2A)
- **릴리스**: rc.1 CI green·prerelease 발행됨. 최종 v0.1.0 = 레포 public 전환(**IP 히스토리 filter-repo 퍼지 선행**) + tap/시크릿 후 동구님 판단. **배포 비우선.**
- **사설 IP 히스토리**: 전방 redact 완료(HEAD 청정, backend-private.md=gitignore). 과거 히스토리엔 잔존(private라 저위험). 공개 시 filter-repo 퍼지 맥 조율(rc.1 태그·릴리스 재생성 동반).

## ⑦ 규율/방식
- 구현=Sonnet 위임(이 세션 Task 1~4 전부 Sonnet), Opus 스펙·리뷰(정독+독립 테스트)·검증. cargo=Bash. 커밋은 Opus 리뷰 후, 논리 단위. 푸시=task별. 한국어 마침표(#5)·새파일 첫줄 역할주석(#6)·em-dash 금지.
