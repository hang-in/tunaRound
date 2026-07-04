# tunaRound v2: node 고도화 + 온보딩 + doctor (설계)

> 2026-07-03 세션9. 세션 중 프리미티브(serve/work/poll/a2a)를 바닥부터 빠르게 쌓아 UX 표면이 복잡해진 것을, **config 1개 + 백그라운드 데몬 1개**로 캡 씌우는 고도화. 원칙: **얇은 래퍼**(기존 검증된 프리미티브 조합만, 재작성 금지). 기존 온보딩(clap 서브커맨드=Stage1, tunaround.toml 프로파일=Stage3)의 **Stage 4 doctor**를 포함해 완성한다.

## 0. 북극성

```
tunaround init      # 온보딩 1회 → node.toml 생성
tunaround node      # 백그라운드 데몬 하나(브로커+자동워커) → 끝(set-and-forget)
tunaround doctor    # 진단
tunaround send/watch/tasks   # dispatch 편의(raw curl 대체)
```

**"데몬 하나 = 다 됨"의 범위(정직)**: 자율 **워커 노드**(받아서 처리)는 `node` 하나로 완결. 두 가지는 본질상 데몬 밖: (1) **dispatch**(던지기)=명령 1회(`send`), (2) **감독 레인**(지켜보며 승인)=살아있는 세션 부착(모델을 백엔드로 못 돌림). 둘 다 옵션.

## 1. config 스키마 (node.toml)

경로 우선순위: `--config <경로>` > `./tunaround.node.toml` > `~/.tunaround/node.toml`. (기존 tunaround.toml 프로파일과 별개 파일 - 프로파일=토론 세션용, node.toml=워커 노드용. 혼동 방지 위해 분리.)

```toml
core  = "self"                             # "self"=이 머신이 브로커 호스팅 / 또는 "http://IP:8770/mcp"
listen = "0.0.0.0:8770"                    # core="self"일 때만. 브로커 바인드 주소.
token = "@env:TUNAROUND_TOKEN"             # bearer. "@env:NAME"=환경변수 참조(레포에 토큰 노출 금지).
db    = "~/.tunaround/broker.db"           # core="self"일 때 브로커 db(안정 경로 권장).

[[lane]]                                   # 자동 레인(헤드리스 워커 데몬). 복수 가능.
agent   = "mac-worker"
runner  = "claude"                         # claude|codex|opencode|http|a2a
mode    = "read-only"                      # read-only(기본)|write
project = "~/privateProject/tunaRound"
interval = 20
tags    = "machine=mac,runner=claude,role=worker"  # 로스터 발견용(dispatcher가 to_selector로 발견). 옵션.
# model, context-map, http-base-url, a2a-card 등 work의 옵션을 그대로 투영(옵션).
```

- `tags`(옵션): 자동 레인 워커가 뜰 때 이 태그로 로스터에 자기 등록해, dispatcher가 `to_selector`(예: `runner=claude`)로 발견·라우팅할 수 있다(에이전트 레지스트리, a2a-usage §9). 미지정이면 빈 태그로 등록돼 uuid/exact-id로만 라우팅된다. work의 `--tags`와 동일 형식. **backend(러너+접속)는 별도 registry를 두지 않고 이 lane 정의가 곧 named backend다**(runner/model/http-base-url/a2a-card 필드로 표현, 프로파일로 재사용).

- 감독 레인은 config에 `kind="supervised"`로 선언만 해두고, `node`가 그 watcher 실행 명령(`tunaround poll ...`)을 **출력**해준다(세션에 붙이라고). 데몬이 직접 세션을 못 여니까.
- 파싱: `src/config.rs`에 `NodeConfig` 추가(기존 config 로더 패턴 재사용). `@env:` 치환 + `~` 확장.

## 2. `tunaround node`

한 프로세스가 config를 읽어:
1. `core="self"`면: 기존 serve 경로 재사용해 **브로커를 전용 스레드 block_on**으로 기동(Stage 3a 교훈: 공유 rt spawn 신뢰불가). ready 폴링으로 바인드 확인.
2. 각 `[[lane]]`(자동)마다 `run_worker_loop`를 tokio task로 실행. core="self"면 `http://127.0.0.1:<listen포트>/mcp`를, remote면 그 URL을 대상으로.
3. 감독 레인이 있으면 그 `poll` watcher 실행 명령을 stderr에 안내 출력.
4. SIGINT까지 실행(백그라운드로 띄우면 상주).

= `serve` + N×`work`를 한 방에. **신규 로직 최소**(오케스트레이션만).

## 3. `tunaround doctor`

체크리스트(각 pass/warn/fail + 한 줄 사유):
- config 파일 발견 + 파싱 OK + `@env:` 참조 변수 존재.
- core: "self"면 listen 주소 바인드 가능 / remote면 agent-card GET 200(토큰 포함).
- 토큰: 설정됐나(없으면 warn).
- 러너 바이너리: 각 lane의 runner(claude/codex/opencode)가 PATH에 있나. http면 base-url 도달. a2a면 card 도달.
- project 경로 존재.
- (있으면) 브로커 db 쓰기 가능.
종료코드: fail 있으면 non-zero.

## 4. `tunaround init`

- 러너 자동 탐지(which claude/codex/opencode) → 기본 runner 제안.
- core self/remote, token(env 이름), agent id, project path를 플래그 또는 최소 프롬프트로 수집.
- `~/.tunaround/node.toml` 작성(있으면 덮어쓰기 확인). 다음 단계(`export TUNAROUND_TOKEN=...`, `tunaround doctor`, `tunaround node`) 안내 출력.
- 최소판: 플래그 주도(`init --core self --agent X --project P ...`)로 시작, 대화형 프롬프트는 후속.

## 5. `send`/`watch`/`tasks` (dispatch 편의)

- `send --core <url|config> --token .. --to <agent> [--from <me>] [--context <id>] "<msg>"` → task_id 출력. (config에서 core/token 읽으면 인자 생략 가능.)
- `watch <task_id>` → 완료까지 상태 추적 + 최종 artifact 출력(내부적으로 GetTask 폴링). `--stream`이면 SSE.
- `tasks [--agent X]` → 열린 task 목록.
= `/a2a` SendMessage/GetTask의 얇은 CLI. raw curl 박멸.

## 6. 구현 순서(이번 세션)

1. **config**: `NodeConfig` 스키마 + 로더(`@env:`/`~` 처리) + 단위테스트.
2. **`node`**: serve+work 오케스트레이션(자동 레인). 로컬 e2e(node self + 자기 앞 task 던져 처리).
3. **`doctor`**: 체크리스트.
4. **`send`/`watch`/`tasks`**: dispatch CLI.
5. **`init`**: config 생성(플래그 주도).
각 단계 커밋 분리 + 테스트 + PR CI. 감독레인 대화형은 코드 아님(안내 출력만).

## 7. 비범위

- 대화형 온보딩 위저드(프롬프트 UX) = 후속(최소판은 플래그 주도).
- 감독 레인을 백엔드화(불가 = 세션 부착 본질).
- 프로세스 관리(재시작/데몬화)는 OS(systemd/nohup/작업스케줄러)에 위임. tunaround는 foreground 상주만 제공.
