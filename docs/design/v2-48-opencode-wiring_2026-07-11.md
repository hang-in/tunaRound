# v2-48: opencode 좌석 배선 (백로그, 2026-07-11 세션21 정찰 확정)

> 상태 = 백로그. **착수 = v2-45 아크 완료 후**(사용자 결정 2026-07-11 "권고대로 나중에 진행"). 정찰 근거 = 업스트림 공식 문서·소스(anomalyco/opencode v1.17.18, 2026-07-11 기준 - 레포가 sst에서 anomalyco로 이관됨). **착수 시점에 버전·스키마·API 재대조 필수**(아래 R2).
>
> **세션23 재대조·현황(2026-07-12)**: ① **워커 레인 = 이미 완성**(§2.1은 스테일). `src/runner/opencode.rs`(OpencodeRunner + `build_opencode_args` + `parse_opencode_stream`)가 커밋 7fedac2(2026-06-30)에 구현·배선(cli.rs `WorkRunner::Opencode`·cli_daemons 팩토리·roster·cli_node)·유닛테스트 완료. 세션23에 fixture 타임아웃 테스트 추가(형제 러너 동형). ② **CLI 재대조 = 드리프트 0**: 이 머신에 opencode **v1.17.18 설치**(정찰 버전과 동일), `opencode run [msg] --format json`(JSONL)·exit 1 계약 불변. ③ **감독 레인 = R2 확증, defer 유지**: opencode.db 마이그레이션 이슈가 1일 뒤에도 활성(세션 고아화·사용자 데이터손실·DB 손상 등 7+건) → 스캐너의 조용한-0 실패 위험 그대로. "몇 달 냉각 + 착수 시 재대조" 조건 미충족. ④ **잔여 폴리시(비착수)**: `RunMode::ReadOnly` 배선(opencode 안정 read-only 플래그 부재로 보류), 토큰 파싱 하드닝(본문은 text 이벤트에서 독립 누적돼 step_finish 드롭에 무손실이라 불요).

## 0. 결론 요약

- **워커 레인 = 난이도 낮음.** `opencode run`이 stdin 파이프·`--format json`·실패 exit 1을 갖춰 기존 러너와 동형 추가 가능.
- **감독 레인 = 난이도 중간, codex보다 유리.** 세션 주입이 app-server ws 우회가 아니라 1급 문서화 REST(`POST /session/:id/prompt_async`, `/tui/submit-prompt`).
- **랜덤 포트 함정 = 설정으로 해소**(후속 정찰로 확정). 남는 규약 = 머신당 포트 대역.
- **최대 리스크 = 세션 저장 계층**(JSON→SQLite 전환 직후, 스키마 미안정) - 착수를 늦추는 실질 근거.

## 1. 업스트림 사실 (전부 공식 문서·소스 출처 확정)

| 항목 | 판정 | 근거 |
|---|---|---|
| 헤드리스 실행 | 가능 | `opencode run [message..]`, `--format json`(이벤트 스트림), stdin 파이프(run.ts의 Bun.stdin), 실패 exit 1. docs/cli |
| 세션 주입 API | 가능 | `opencode serve`(기본 127.0.0.1:4096, `OPENCODE_SERVER_PASSWORD` Basic 인증). `POST /session/:id/message`(대기)·`POST /session/:id/prompt_async`(204 즉시)·`GET /session`·`GET /event` SSE. `/tui/append-prompt`·`/tui/submit-prompt` 등 TUI 제어도 1급 API(IDE 플러그인이 소비). docs/server |
| TUI 포트 고정 | **가능(정정)** | 기본 `opencode`(tui) 커맨드가 `--port`·`--hostname` 직접 수용(default 0=OS 랜덤이 랜덤 포트의 정체). `opencode.json`의 `server.port/hostname/mdns/cors`로도 고정. 전용 env 없음(config `{env:VAR}` 치환으로 우회). `--mini` 모드는 네트워크 플래그 차단. docs/cli·docs/config·network.ts |
| 외부 포트 발견 | 사실상 불가 | lockfile·상태 파일 없음, 미지정 랜덤이면 argv에도 안 남음. mDNS는 opt-in. → **고정이 정답**(발견 메커니즘 만들지 말 것) |
| serve→attach | 공식 지원 | `opencode attach [url]`(--session·--password 등). 문서가 지원 워크플로우로 예시. 포트 고정이 되므로 필수 아님(선택 운용) |
| 세션 영속 | 가능하되 주의 | `~/.local/share/opencode/opencode.db`(SQLite, **Windows도 %USERPROFILE%\.local\share** - APPDATA 아님). session 테이블 = id·project_id·directory(cwd)·title·time_created/updated → 스캐너가 SQL 한 방으로 열거 가능. 단 role은 message.data JSON 내부(컬럼 아님) |
| MCP 클라이언트 | 가능 | `opencode.json` mcp 필드 = `type: remote` + url + `headers: {Authorization: Bearer ...}` 공식 지원 → tuna-broker MCP native 로드 가능. docs/mcp-servers |
| 훅/플러그인 | 가능 | TS 플러그인(Bun): `~/.config/opencode/plugins/`(글로벌). `chat.message` 훅(sessionID·messageID 전달) + `event` 훅(session.idle 등) + Bun shell `$` 제공 = 외부 POST 가능. **chat.message가 사용자 제출 시에만 발화하는지는 라이브 검증 1회 필요** |

## 2. 배선 설계 스케치

### 2.1 워커 레인 (`--runner opencode`)

- ~~`src/runner/opencode.rs` 신설~~ **(완료, 커밋 7fedac2)**: `opencode run` spawn + 프롬프트 **positional arg**(stdin 아님, 실측 정정) + `--format json` JSONL 파싱(text/step_finish → 본문·토큰) + exit 비0 = fail 전이. claude/codex 러너와 동형(argv 빌더 + 파서 순수부 + watchdog idle 타임아웃 + 가짜 CLI fixture 테스트). 배선 완료: `WorkRunner::Opencode`(cli.rs)·cli_daemons 팩토리·roster 좌석·cli_node.

### 2.2 감독 레인 (codex 선례와 동형 3요소)

- **발견**: presence 스캐너에 `enumerate_opencode_sessions` - opencode.db를 **read-only 모드**로 열어 session 테이블(id·directory·time_updated) 열거. 파서 보수 설계(읽기 실패 = 해당 머신 opencode 분만 제외, 스캔 루프 계속) + 지원 버전 상수 명시.
- **수신(택1, (a) 우선 검증)**: (a) native = 대상 opencode에 tuna-broker remote MCP(Bearer)를 config로 로드 → codex처럼 세션이 스스로 claim/complete. (b) relay형 = opencode-relay 데몬이 대리 claim 후 `POST /session/:id/prompt_async` 주입(고정 포트 필요). (a)가 데몬 하나를 아끼므로 먼저 검증, 승인 UX 마찰 있으면 (b).
- **human 신호(★)**: 글로벌 플러그인 `chat.message` 훅에서 `/dashboard/human-ping` POST. 주입 턴도 발화하면 ★ 오염이므로 "브로커 task " prefix 필터를 훅에 내장(v2-45 P5의 codex prefix 계약과 동일 패턴).
- **운용 규약**: 감독 대상 opencode TUI는 `--port` 고정(또는 opencode.json server.port). 머신당 포트 대역 규약 필요(예: 41xx, 다중 TUI 충돌 방지). `--mini` 모드는 감독 대상 제외.

## 3. 리스크와 완화

- **R1 랜덤 포트 = 해소(설정).** 잔여 과제는 포트 충돌뿐 - 머신당 대역 규약으로 처리. 외부 발견 메커니즘(netstat 매핑 등)은 만들지 않는다.
- **R2 세션 저장 스키마 미안정(최대 리스크, 착수 지연 근거).** opencode가 JSON 파일 저장을 SQLite로 갓 전환(v1.15 무렵). ① 업스트림에 마이그레이션 버그 이슈 다수(세션 고아화·메시지 파츠 소실) = 저장 계층 자체가 흔들리는 중. ② 거의 매일 릴리스라 내부 스키마 변경 가능 - 스캐너 파서가 에러 없이 0건을 반환하는 조용한 고장(codex rollout 파서와 동일 실패 모드). ③ 사용자 머신의 opencode 자동 업데이트로 배선 기준 버전과 어긋남. **완화 = 보수 파서 + 버전 핀 상수 + read-only 열기 + 몇 달 냉각 후 착수 + 착수 시 업스트림 재대조**(규율: 회귀 결론 전 업스트림 소스 대조).
- **R3 chat.message 발화 조건 불명.** 어시스턴트 턴·주입 턴에도 발화하면 ★ 오염. 라이브 검증 1회 + prefix 필터 내장이 전제.

## 4. 착수 조건·순서

1. v2-45 아크 완료 + opencode 스키마 냉각(마이그레이션 이슈 진정) 확인.
2. 착수 시 §1 표 전체를 당시 버전으로 재대조(포트 플래그·API 경로·db 스키마).
3. 순서 = 워커 러너(작음, 독립 PR) → 감독 레인(스캐너 열거 → 수신 (a) 검증 → human 신호 플러그인). 각 단계 라이브 스모크.
