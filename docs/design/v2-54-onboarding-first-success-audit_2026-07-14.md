# v2-54 온보딩 첫 성공 감사: 현재 상태·갭·최소 변경 (2026-07-14)

> 조사 배경: 정본 확정(총감독=사람이 앉은 외부 Claude Code 세션 + tuna-broker MCP, chat REPL은 부수 기능).
> 첫 성공 정의 = **총감독 Claude Code 세션에서 로컬 Codex에게 대화를 보내고 응답을 받는 것**(로컬 전용).
> 방법: 5영역 병렬 코드 조사(전 주장 파일:라인 근거, claude CLI는 CLAUDE_CONFIG_DIR 격리 샌드박스 실측). 이 문서는 그 합성본.

## A. 현재 상태 표

| 첫 성공에 필요한 요소 | 현재 자동화됨? | 사용자 수동 작업 | 코드 근거 |
|---|---|---|---|
| tunaround 설치(serve·worker 포함) | 부분 | 릴리스 인스톨러 1줄(brew/irm/curl). ⚠ `cargo install`은 기본 피처에 serve/worker가 없어 함정 | dist-workspace.toml:35(풀피처) vs Cargo.toml:48(default=morphology+sqlite) |
| 설정 스캐폴드(node.toml·~/.tunaround/config) | O (`init`) | `--runner codex` 명시 필요(기본 탐지가 claude 우선) | src/cli_node.rs:79-168(파일 생성만), :90-96(claude→codex→opencode 첫 매치) |
| 브로커 기동 | O (`node`가 in-process 자동) | `tunaround node` 실행·유지(부팅 자동시작 없음) | src/cli_run.rs:352-397(core=self 전용 스레드 기동) |
| codex 워커 상주 | O (`node` 자동 레인) | 위와 동일 프로세스 | src/cli_run.rs:427-516; codex 실행=`codex exec --json --sandbox read-only -`(src/runner/codex.rs:66-94, app-server·승인 불요 확정) |
| 총감독 Claude에 MCP 등록 | **X (완전 수동)** | `claude mcp add --transport http ...` 직접 실행(토큰 argv 노출 방식만 문서화) | src 전체에 `claude mcp` 호출 0(grep 전수); 문서 안내만 docs/reference/a2a-usage.md:147-149 |
| MCP 등록 후 세션 반영 | X (구조적) | **Claude Code 재시작(새 세션)** | 공식 문서: 등록은 "세션 밖에서", 신규 서버 핫 리로드 부재(/mcp는 상태·인증·재접속용). 세션8 실측 기록과 일치(CLAUDE.md:93) |
| 토큰 | 로컬 무토큰 = **명시적 계약** | 현재 init은 그래도 토큰 개념을 노출(placeholder+안내 무조건 출력, listen 기본 0.0.0.0) | 계약 3중 근거: src/mcp/server.rs:963 주석("동일 계약")·회귀 테스트 http_mcp_no_token_allows_all(server.rs:2482-2527)·결정 기록(v2-40 핸드오프). init 노출: src/cli_node.rs:115-118(0.0.0.0 기본)·:153-162(토큰 안내) |
| 결과 수신 | O (MCP만으로 폐루프) | send_task→get_task 반복(추가 상주물·등록 0). completed면 같은 응답에 결과 전문 | src/mcp/tasks.rs:279-303, src/mcp/format.rs:341-399(⚠no-consumer 5분/⚠stuck 15분 신호 포함) |

**현재 첫 성공까지 실단계(신규 머신)**: ① 인스톨러 1줄 ② `init --runner codex` ③ 토큰 2곳 기입(config 파일+셸 env - `resolve_node_token`이 env만 읽음, src/config/node.rs:98-114) ④ `node` ⑤ `claude mcp add ...` 수동 ⑥ Claude 재시작 ⑦ send_task/get_task. **명령 5개 + 수동 편집 1~2회 + 재시작 1회.** ③은 로컬 계약을 쓰면 원리상 생략 가능한데 init이 강제 노출하는 상태.

## B. 목표 상태와의 갭

목표: `$ tunaround init` → (재시작 명시) → "codex한테 검토받아줘" → 응답.

| 갭 | 난이도 | 제약/우회 |
|---|---|---|
| G1. MCP 자동 등록 부재 | **중** | `claude mcp add`는 http+`--header`+`--scope user` 전부 지원(이 머신 실측). **멱등 아님**(같은 scope·이름 재실행=exit 1 하드 에러, 샌드박스 실측+공식 문서) → `mcp get` 존재 확인 후 add(불일치 시 remove→add) 래핑 필요. local scope는 git-repo-루트 오귀속 실측(홈이 git repo인 머신) → **user scope 고정이 안전**. ~/.claude.json 직접 쓰기는 비권장 표면+라이브 세션 동시 쓰기 경합(조사 중 해시 변화 실측) → CLI 경유가 정석 |
| G2. 로컬 무토큰 경로를 init이 안 씀 | **하** | 계약은 이미 있음(A표). init에 `--local` 프리셋(또는 기본): listen=127.0.0.1 + 토큰 안내·placeholder 생략. **부수효과: G1의 토큰 평문(~/.claude.json)·argv 노출 문제가 로컬 첫 성공에선 통째로 소멸**(`--header` 자체 불요) |
| G3. 러너 탐지가 claude 우선 | 하 | 탐지된 러너 전부를 lane으로 스캐폴드하거나 codex 존재 시 안내. cli_node.rs:91 순서 문제 |
| G4. 토큰 이중 기입(비-local 경로) | 하 | node/doctor가 ~/.tunaround/config를 폴백으로 읽으면 1곳으로 압축(현재 env만: config/node.rs:98-114). local 프리셋에선 무관 |
| G5. 상주 수명 = node 프로세스 | 중 | 부팅 자동시작·서비스 등록 없음(grep 전수). 첫 성공엔 포그라운드 node로 충분, 상시 운용은 후속(restart 스크립트는 win 운영 머신 전제) |
| G6. 러너 사전 검증 부재 | 하 | codex 미설치·미로그인은 첫 task fail로야 표면화(worker.rs:792-797, 미로그인 401 실측). 기동 시 binary_on_path 경고 1줄 |
| G7. 완료 대기 MCP 도구 부재 | 중 | get_task는 즉시 반환뿐이라 총괄이 폴링 간격을 스스로 관리(턴 점유). `get_task(wait_secs=N)` 변형이면 UX 단순화. SSE SubscribeToTask는 /a2a 전용이라 MCP 클라이언트가 못 씀 |
| G8. watch-results dispatcher 불일치 함정 | 하 | 훅·운영 모두 `--dispatcher dashboard` 고정인데 필터는 fromAgent 완전일치(watch_results.rs:283-285) → 총괄이 자기 세션 id로 send하면 인박스 침묵. 첫 성공(동기 get_task) 범위 밖이지만 두 번째 마찰 지점 |

## C. 불가능한 것 (외부 의존)

1. **이미 열린 Claude Code 세션에 새 MCP 서버 핫 로드**: 공식 문서상 부재(project scope는 "재시작 필수" 명시, /mcp 명령은 신규 등록 리로드 기능 없음). → 목표 흐름의 "재시작이 필요하면 명시"는 **명시로 확정**하는 것이 정직한 설계. 우회는 헤드리스 한정(`--mcp-config` 퍼-런 주입 - src/runner/claude.rs:16·161에 기존 배선 / raw HTTP MCP 직접 호출 - 세션8 실증)이며 대화형 총감독 세션에는 적용 불가.
2. **user scope 헤더의 `${VAR}` env 확장**: 공식 문서가 `.mcp.json`에만 명시 - 확인 불가(실측은 등록 변경이라 금지). 토큰이 필요한 원격 구성에선 평문 저장을 전제해야 함.
3. **"보이는 codex 세션이 답하는" 경험**: app-server ws + codex-relay + codex config.toml MCP 배선 + 60분 human window의 다중 전제(상주 3개) - 첫 성공 정의(헤드리스 응답)와 분리 유지가 맞음. relay 경로 단순화는 codex app-server 프로토콜 의존이라 상.
4. codex의 응답 시간 상한: 코드상 보장 없음(러너 idle watchdog 600초만, codex.rs:115).

## D. 최소 변경 제안 (우선순위)

1. **[P0, 하] `init` 로컬 프리셋**: 기본(또는 `--local`)을 listen=127.0.0.1 + 무토큰(계약 활용)으로. 토큰 안내·placeholder는 `--listen`이 비-loopback일 때만 출력. → 사용자 어휘에서 "토큰" 소멸.
2. **[P0, 중] `init`의 MCP 자동 등록**: `claude mcp get tuna-broker` exit code로 존재 확인 → 없으면 `claude mcp add --transport http --scope user tuna-broker http://127.0.0.1:8770/mcp`(로컬 무토큰이라 헤더 불요) → 마지막 줄에 **"Claude Code를 재시작(새 세션)하면 tuna-broker 도구가 보입니다"** 명시. claude CLI 부재 시 수동 명령 출력으로 강등(fail-open).
3. **[P0, 하] 러너 탐지 개선**: PATH의 claude·codex·opencode 전부를 lane으로 스캐폴드(첫 매치 1개 → 전수). codex lane이 기본 포함되면 `--runner codex` 암기 불요.
4. **[P1, 하] `node` 기동 시 러너 사전 검증**: lane runner가 PATH에 없으면 기동 로그에 경고(첫 task fail 전에 표면화) + 기동 로그에 "토큰 인증: 사용/미사용" 1줄(env 조용한 픽업 비대칭 해소).
5. **[P1, 하] onboarding.md에 "로컬 첫 왕복" 절 신설**: `설치 → init → node → claude mcp add(자동화 전까지) → 재시작 → send_task/get_task` 최소 시나리오만. 훅·presence·대시보드·watch-results는 "다음 단계"로 격리.
6. **[P2, 중] `get_task`에 `wait_secs` 옵션**(long-poll): 총괄 폴링 UX 단순화.
7. **[P2, 하] node의 config 파일 토큰 폴백**(비-local 구성용) + watch-results dispatcher 규약 명문화(G8).

**목표 도달 후 흐름(P0 3건 반영 시)**: `설치 1줄 → tunaround init → tunaround node → (Claude 재시작 1회, 명시됨) → "codex한테 검토받아줘"`. 남는 어휘 = init·node 뿐(코어/워커/브로커/토큰/A2A 전부 비노출). node를 init이 detached로 띄우는 것(명령 1개화)은 서비스 수명 관리(G5)와 엮여 후속 판단.
