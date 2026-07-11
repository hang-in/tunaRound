# Changelog

이 프로젝트의 주요 변경을 기록합니다. 형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고, [Semantic Versioning](https://semver.org/lang/ko/)을 지향합니다.

## [Unreleased]

> 0.3.0 이후 mesh를 관측·영속·자동수신 쪽으로 굳혔다(presence 스캐너, codex-relay, 재생·기억화, 대시보드 관제탑 고도화). 전부 하위호환 추가이며 스키마는 v8→v10(컬럼·테이블 추가)로 확장됐다. 아직 릴리스 태그를 붙이지 않았다.

### 추가 (Added)

- **presence 스캐너 (v2-44)**: 머신당 데몬 1개가 라이브 세션(claude·codex)을 스캔해 로스터에 전집합을 일괄 동기화(`report_presence`). 세션 수신 자동 가동 SessionStart 훅. role 태그에 `infra`(머신 상주 데몬) 추가.
- **codex-relay (v2-46)**: 로스터에 보이는 codex 세션 thread로 task를 직접 배달(app-server ws 주입, 대리 claim). codex는 스스로 수신 메커니즘이 없어 relay가 대신 받는다.
- **mesh 영속·재생 (v2-45)**: `watch-results` 재접속 + since 워터마크 재생, 대시보드 피드 초기 스냅샷(`?replay`), 총감독 ★(`human_input_at`) 영속, codex 입력 신호 추출, 종결 task 요청·결과 색인(`a2a:*` 네임스페이스 → `search_context` 검색), 종결 task retention 슬림화, 유휴-열림 세션 로스터 유지.
- **대시보드 관제탑 고도화 (v2-47)**: task 카드 상세 펼침 + 필터 칩, 브로커 헬스 패널(`GET /dashboard/health`, 미배달·고착·스캐너 도달성 + 브로커 uptime·WAL 크기), 브라우저 알림 옵트인, 위임 이력 검색(`GET /dashboard/search`, a2a 스코프), 로스터 ★ recency 표시·관전 뱃지·모바일 반응형.
- **opencode 워커 러너**: `--runner opencode`(`opencode run --format json`, 기존 러너와 동형).
- **task lease 자동연장 + 취소 (v2-49)**: 워커가 실행 중 자기 task의 lease를 주기 연장(`extend_task_lease`, 상한 후 중단해 고착은 회수)해 30분 넘는 장기 task가 실행 중 requeue되지 않게. `cancel_task` MCP 도구 + `tunaround task cancel` CLI(잘못 보냈거나 불필요한 열린 task를 canceled로).
- **원커맨드 온보딩**: `tunaround init`이 `node.toml` + `~/.tunaround/config`(mesh·훅용 dotenv)를 한 번에 스캐폴드(`--machine` OS 감지, 유닉스 0600 권한, 기존 config는 --force 없이 보존). AI 설치 안내 프롬프트(`docs/prompts/install-with-ai.md`)로 새 머신 설치를 에이전트에 위임.

### 변경 (Changed)

- **총감독 판정 = 사람 입력 최신 세션**(heartbeat=presence 재설계, v2-42/43). 로스터=online 세션 전부, 발견/유휴·discover 모델 제거.
- **대시보드 = 관제탑(read-only 뷰)로 수렴**: 뷰(로스터·피드) + 목표 제출만. 직접 제어 UX 비확장.
- **토큰 env 이름 통일**: `tunaround init` 기본 `token_env`를 `TUNAROUND_TOKEN` → `TUNA_BROKER_TOKEN`으로(node.toml·데몬·훅·`~/.tunaround/config` 공용 = 토큰 env가 둘이던 혼란 제거). 기존 node.toml은 불변, 새 init에만 적용.

### 제거 (Removed)

- **Redis 전삭제 (v2-45 P7)**: 관찰·세션 버스의 Redis 의존 제거. 관찰(`--observe`)·재개는 SQLite DB 공유로.
- **대시보드 직접 제어 제거 (v2-45 P0)**: `/dashboard/control` 제거. codex 제어는 task 장부 경유 codex-relay가 대체.

## [0.3.0] - 2026-07-06

> 0.2.2 이후 오케스트레이션 레이어가 크게 늘었다(레지스트리·감독·대시보드·세션 버스). 전부 하위호환 추가이며 스키마는 v6→v8(컬럼 추가)로 확장됐다.

### 추가 (Added)

- **에이전트 레지스트리 (UUID 라우팅 + 태그 발견)**: `register_agent`/`heartbeat`/`list_agents` + `send_task`/`SendMessage`의 `to_selector`(태그 셀렉터). 로스터 인메모리(heartbeat TTL 90초). 워커/세션이 자기 uuid로 등록하고 dispatcher가 태그로 발견한다.
- **감독(관리자) 모드**: watcher가 도는 동안 heartbeat로 상시 online 로스터 유지(`poll --tags`). **codex 라이브 감독**: `codex app-server --listen ws://` + `tunaround codex-inject`(turn/start 외부 주입)로 라이브 thread를 외부에서 wake.
- **총괄 웹 대시보드** (`serve --features dashboard`의 `/dashboard`): 로스터 + 라이브 task 피드(task별 카드·이력 펼침) + 목표 제출(loopback) + codex 직접 제어(`/dashboard/control`). Vite+React SPA를 rust-embed로 임베드.
- **유니버설 세션 버스 (v2-40)**: SessionStart 자동무장 훅(opt-in `TUNA_AUTOARM=1`, register+poll) + `tunaround discover`(로컬 Claude Code 세션 발견 리포터, machine 속성, claude-mem·secall automation 노이즈 필터) + 대시보드 "발견된 세션" 후보 패널(cross-machine, armed overlay) + "연결"(arm 프롬프트 안내).
- **워커 노드 진단 doctor Stage 4**: 형태소 백엔드 이름 + Ollama 도달 진단. node lane `tags` 배선.
- **task 트레이스**: `tasks.runner` 컬럼(claim 시 기록) + 쓰기 민감 path 가드.
- **serve/poll/discover `--token` env 폴백**: `TUNA_BROKER_TOKEN` env를 읽어 argv에 토큰을 노출하지 않고 인증한다.

### 보안 (Security)

- 대시보드 write 엔드포인트(`/dashboard/goal`·`/dashboard/control`) local CSRF 방어(`Sec-Fetch-Site`) + control ws 대상 loopback 제한(SSRF 방어).

## [0.2.2] - 2026-07-04

### 추가 (Added)

- `tunaround poll --on-task '<cmd>'`: task 도착 시 명령을 실행한다(`{id}` 치환 + `TUNAROUND_TASK_ID`/`TUNAROUND_TASK_MSG` 환경변수). Monitor가 없는 하네스(codex 등)의 0토큰 감독 레인 wake 글루. 예: codex는 `--on-task 'codex exec resume --last "task {id} 처리"'`로 세션을 이어받아 처리(idle 0토큰, 문맥 보존).
- **claim-후-워커사망 자동 requeue**(거버넌스 §6): lease 기반. claim 시 `lease_expires_at`/`claimed_by`/`attempt_count` 기록(스키마 v7), poll 경로 지연 sweep(`expire_stale_claims`)이 lease(기본 30분) 만료된 `working`을 `submitted`로 회수하고 `attempt_count`가 상한(3) 초과면 `failed`로 격리(무한 requeue 차단). `complete`는 first-completer-wins 가드(되살아난 stale 워커의 뒤늦은 덮어쓰기 거부). 재배달 시 지시문(status_message) 보존. 별도 타이머·하트비트 없음(YAGNI).

## [0.2.1] - 2026-07-04

### 변경 (Changed)

- 릴리스 바이너리(cargo-dist)에 `worker`/`engines`/`a2a-out` 피처 포함. 이제 설치본 하나로 코어(`serve`)뿐 아니라 워커 노드(`tunaround node`/`poll`/`work`), http 러너(`--runner http`), 외부 표준 A2A 위임(`--runner a2a`)까지 동작한다. (v0.2.0 프리빌트는 `serve`만 있어 워커 서브커맨드가 없었다.)

## [0.2.0] - 2026-07-04

### 추가 (Added)

- **semi-A2A 파트너 위임(Phase 1)**: A2A Task 데이터모델(Task/TaskState/Message/Part/Artifact, `tasks` 테이블 스키마 v6) + JSON-RPC 엔드포인트(`SendMessage`/`GetTask`/`CancelTask`, `/.well-known/agent-card.json`) + inbox MCP 도구(`poll_tasks`/`claim_task`/`complete_task`/`fail_task`) + dispatcher 도구(`send_task`/`get_task`). 크로스머신 왕복 실증.
- **A2A 스트리밍(Phase 2, SSE)**: `SendStreamingMessage`/`SubscribeToTask` + store 이벤트 버스. Agent Card `capabilities.streaming=true` 광고.
- **A2A 자율 워커 데몬**: `tunaround work`(poll->claim->러너 실행->complete 자율 루프). 러너 교체로 이기종 파트너(`--runner claude|codex|opencode|http|a2a`). `context_id` 프로젝트 라우팅(`--context-map`). 러너 실패 시 `fail_task` 전이(completed 위장 안 함).
- **A2A outbound 러너**: `--runner a2a`로 외부 표준 A2A 에이전트에 표준 위임(a2a-client, `a2a-out` 피처).
- **워커 노드 온보딩**: `tunaround init`/`node`/`doctor` + `NodeConfig`(config 1개 + 데몬 하나 = 워커 노드). 감시 전용 `tunaround poll`(감독 레인 유휴 0토큰 wake).
- **브로커 거버넌스**: 네이밍 규약(`to_agent`는 워커만) · Agent Card `buildFeatures` 능력 광고 · 미배달/고착 표시(`⚠no-consumer?`/`⚠stuck?`) + 전역 조망 `tasks` 도구 · write 워커 self-disruption 방지 가드레일.

### 변경 (Changed)

- **개발 방법론**: GitHub Flow + PR CI(3-OS 매트릭스: ubuntu/macOS/windows, build+test+clippy) 도입.
- **리팩토링(리뷰 기반 R1-R10)**: MCP 에러 계약 정직화(실패를 isError로) · A2A 상태머신 조건부 전이(이중 claim/종료 덮어쓰기 차단) · watchdog 프로세스 트리 종료 · retriever/reader Result 계약 · Embedder dim 동적화 등.
- **저장소 공개**: `hang-in/tunaRound` PUBLIC 전환(히스토리 시크릿 퍼지).

### 고침 (Fixed)

- 워커 MCP 세션 만료 시 자동 재연결(404 -> handshake 재수행).
- `watchdog`의 Unix 프로세스 그룹 종료 이식성(외부 `kill -9 -PID` no-op -> `libc::kill`).

## [0.1.0] - 2026-07-02

첫 공개 릴리스(도그푸딩 후 태그 예정). 터미널에서 사람이 운전하는 역할 부여 2-에이전트(Claude Code · Codex) 착수 전 설계 토론 도구.

### 추가 (Added)

- **토론 코어**: 역할 주입 + 순차-인지 라운드(`run_round`), thin REPL(`chat`). 러너는 Codex·Claude(공통 `Runner` trait, 읽기/쓰기 하드 분리).
- **REPL 커맨드**: `@engine`(자리 지목) · `@engine!`(쓰기 턴, 협업 코딩) · `/debate <n>`(N턴 자동 교환) · `/conclude`(종합) · `/branches`·`/checkout`(분기 트리) · `/save` · `/supersede`·`/reject`·`/explain`(유효성·검색 디버그).
- **영속·세션**: SQLite 시스템 오브 레코드(스키마 v5, `created_at`) + in-store 트리(브랜치=세션). 멀티세션 관찰/재개(Redis, 선택).
- **한국어 검색/맥락**: 형태소 FTS(Kiwi 메인 + lindera 폴백, POS keep-tags) + 외래어 음역 병기 색인 + 벡터 RRF + RAG 주입 + MCP 능동검색(`search_context`/`read_transcript`) + 유효성·세션·recency 인지 랭킹.
- **semi-A2A 코어 백엔드**: `core`(단일 프로세스 REPL+in-process HTTP MCP) · `serve`(헤드리스 코어) · `join`(원격 코어 접속). `post_turn`/`get_roster` + core-sync(증분 append, DB id 권위). bearer 인증.
- **온보딩·배포**: clap 서브커맨드(`chat`/`core`/`serve`/`join`/`mcp-search`/`reindex`) · `tunaround.toml` 프로파일 · cargo-dist(6타깃, homebrew/shell/powershell 인스톨러).
- **임베딩**: 원격 Ollama HTTP(기본 `qwen3-embedding:0.6b`, dim 1024, `TUNAROUND_EMBED_MODEL`로 교체) + 결정적 MockEmbedder 폴백.

### 알려진 제약 (Known limitations)

- Kiwi 네이티브 라이브러리·모델은 첫 실행 시 [bab2min/Kiwi](https://github.com/bab2min/Kiwi) 릴리스에서 자동 다운로드하며, 실패하면 lindera로 자동 폴백합니다(검색은 동작, 품질만 소폭 차이). macOS(aarch64)에선 현재 자산 태그 이슈로 lindera 폴백 상태입니다.
- 의미검색(semantic)은 Ollama 엔드포인트(기본 `http://127.0.0.1:11435`)가 있어야 하며, 없으면 FTS 단독으로 폴백합니다.
- codex 좌석의 원격 MCP pull은 환경에 따라 불안정할 수 있습니다(claude pull은 안정). read-only는 behavioral로 보장합니다.

### 라이선스

- AGPL-3.0-only.

[Unreleased]: https://github.com/hang-in/tunaRound/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.3.0
[0.2.2]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.2
[0.2.1]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.1
[0.2.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.0
[0.1.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0
