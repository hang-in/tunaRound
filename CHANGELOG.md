# Changelog

사용자에게 영향을 주는 주요 변경을 버전별로 기록합니다.

- 가장 최근 개발 내용은 `Unreleased`에서 확인합니다.
- 설치와 사용법은 [README](README.md)와 [문서 색인](docs/index.md)을 참고하세요.
- 내부 구현 계획과 세션 기록은 이 문서에 넣지 않습니다.

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고, 버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 지향합니다.

## [Unreleased]

### Added

- mesh 토론: 쓰던 세션에서 `start_discussion`(tuna-broker MCP)으로 여러 머신의 에이전트에 역할을 나눠 라운드 토론을 시킵니다. 순차-인지 라운드 후 종합 발언이 생성되고, 전사는 `debate:<id>` 세션으로 저장되며 결과는 `watch-results --dispatcher debate:<id>`로 받습니다. `stop_discussion`으로 이후 라운드를 중단할 수 있고, 브로커 재기동 시 진행 중이던 토론은 실패로 정리되어 통지됩니다.
- mesh 토론 라운드 간 사람 승인 게이트(옵트인): `start_discussion(gate=true)`면 각 라운드 완료 시 다이제스트가 인박스로 오고, `continue_discussion(discussion_id, steer?, conclude?)`로 사람이 진행을 승인합니다. steer는 조향 지시로 다음 라운드에 주입되고, conclude는 남은 라운드를 건너뛰고 종합으로 직행합니다. 게이트 대기 중 브로커가 재기동되면 대기 표식 작업이 failed로 마감되어 인박스에 통지됩니다.
- 대시보드 피드: 카드 상세와 접힌 미리보기가 markdown으로 렌더됩니다. 토론 작업에는 전용 뱃지가 붙고, 발신(누가 보냈는지) 필터가 추가되었습니다.
- 대시보드 로스터의 동작 스피너가 A2A 작업 외에 "지금 대화 턴을 처리 중"인 세션에도 켜집니다(claude=턴 시작·종료 훅 쌍, codex=rollout 활동 신선도).
- `get_task`에 `wait_secs`(1~120) 옵션이 생겼습니다. 지정하면 작업이 끝날 때까지 서버가 대기(long-poll)했다가 반환하므로, 위임한 쪽이 폴링 간격을 관리할 필요가 없습니다.
- `tunaround node`/`doctor`가 node.toml의 `@env:TUNA_BROKER_TOKEN`을 환경변수에서 못 찾으면 `~/.tunaround/config`의 같은 키를 폴백으로 읽습니다. init이 안내하는 "config에 토큰 채우기"만으로 동작하고, 별도 export 단계가 필요 없습니다(비어있지 않은 환경변수가 있으면 그쪽이 우선).

### Fixed

- 한국어 등 CJK 문자·괄호에 붙은 굵게 표시(`**`)가 대시보드에서 렌더되지 않던 문제를 교정했습니다.


## [0.5.0] - 2026-07-15

### Added

- 대시보드에 세션 등장·소멸과 사람 입력 이력을 최신순으로 보여주는 presence 타임라인 패널을 추가했습니다.
- 토론 발언에 증류된 결정 요약과 검색 앵커를 남기는 `/annotate` 명령을 추가했습니다. 검색이 요약을 원문 앞에 얹어 보여주고 앵커가 맞으면 관련 발언을 위로 올립니다.
- 릴리스 바이너리에 대시보드 SPA가 포함됩니다. 설치본에서 별도 빌드 없이 `/dashboard`가 바로 동작합니다.
- 릴리스 아카이브에 서드파티 라이선스 고지(`THIRD-PARTY-NOTICES.html`)를 동봉합니다.
- `tunaround init`이 로컬(loopback) 기본으로 바뀌어 토큰 설정 없이 바로 시작할 수 있고, 성공하면 Claude Code에 tuna-broker MCP 서버를 자동 등록합니다(재시작 1회 필요). PATH의 claude·codex·opencode를 전부 탐지해 발견된 러너마다 워커 레인을 만듭니다.
- `tunaround node`가 기동 시 각 레인의 러너가 PATH에 없으면 경고하고, 브로커의 토큰 인증 사용 여부를 기동 로그에 남깁니다.
- 로스터에 지금 작업을 처리 중인 세션을 알리는 동작 스피너를 추가했습니다. 작업 이벤트를 실시간 반영하고, 멈춘 작업이 스피너를 계속 켜 두지 않습니다.
- 터미널에서 연 Codex 세션에 생존 마커를 붙이는 PATH 래퍼를 추가했습니다. 세션을 닫으면 로스터에서 즉시 사라지고, 오래 쉬어도 세션이 열려 있는 한 유지됩니다.
- 장기 작업의 lease를 러너 실행 중에도 자동 연장해, 실행 중인 작업이 다른 워커로 잘못 재배정되지 않습니다.

### Changed

- 대시보드를 관제 중심 3층 레이아웃으로 재편했습니다(사이드바 로스터, 전폭 피드와 필터, 서버 요약 타일, 라이트·다크 토글, presence 로그).
- Codex 세션의 로스터 유지 기준을 "최근 사람 활동 시간창"으로 바꿨습니다. 닫힌 세션이 오래 남아 작업이 잘못 배달되던 문제를 줄이고, 래퍼 마커가 있는 세션은 시간창을 면제합니다.
- codex-relay를 비동기로 재설계했습니다. 긴 주입 중에도 생존 신호와 다른 작업 처리가 멈추지 않고, 도달할 수 없는 세션에는 주입을 시도하지 않습니다.
- README와 온보딩 가이드를 "첫 성공(내 Claude Code에서 로컬 Codex 왕복)" 경로 중심으로 재작성했습니다.

### Fixed

- 실패가 조용히 숨지 않습니다: 작업 실패 사유가 조회에 그대로 표시되고, 수신자 없음·정체 의심 신호가 함께 붙습니다.
- Windows에서 claude 러너 호출이 프롬프트의 개행에 깨지던 문제를 고쳤습니다(인자 대신 stdin으로 전달). 그 외 Windows 경로 이식성 문제들을 정리했습니다.
- `/clear`로 닫힌 세션의 수신 루프가 유령 프로세스로 남던 문제를 고쳤습니다(세션 마커가 사라지면 스스로 종료).
- 검색 정확성: 의미 검색 폴백의 순위 보존, 임베딩 모델·차원 불일치 필터링, 색인 중 검색 차단 해제를 고쳤습니다.
- 저장 무결성: 커밋 실패 시 롤백, 동시 기록 직렬화, 미래 스키마 버전 가드 등 SQLite 경계 문제들을 고쳤습니다.
- 대시보드: 목표 제출 오류 표시, 폴 응답 뒤섞임 방지, 알림 중복 발화, 접근성 문제를 정리했습니다.

### Security

- 토큰 비교를 상수시간으로 바꾸고, 비-loopback 주소에 토큰 없이 바인드하면 기동 시 경고합니다. HTTP 러너의 API 키를 브로커 토큰과 분리했습니다.
- CI 공급망을 강화했습니다: 서드파티 액션 커밋 고정, 최소 권한 토큰, 의존성 취약점 감사.

## [0.4.0] - 2026-07-11

0.3.0에서 추가한 mesh를 실제 운영에 쓸 수 있도록 세션 발견, Codex 수신, 상태 재생, 대시보드 관측 기능을 보강했습니다. 기존 설정과 명령은 대부분 그대로 사용할 수 있습니다.

### Added

- 머신마다 하나의 `presence-scan` 데몬이 실행 중인 Claude Code와 Codex 세션을 찾아 공용 로스터에 동기화합니다.
- `codex-relay`가 Codex 라이브 세션 앞으로 온 작업을 해당 thread에 직접 전달합니다.
- `watch-results`와 대시보드가 재접속 후 놓친 완료·실패 이벤트를 다시 불러옵니다.
- 종료된 A2A 작업의 요청과 결과를 별도 네임스페이스에 색인해 검색할 수 있습니다.
- 대시보드에 작업 상세, 필터, 브로커 상태, 알림, 위임 이력 검색, 모바일 레이아웃을 추가했습니다.
- OpenCode를 헤드리스 워커 러너로 사용할 수 있습니다.
- 장기 작업의 lease 자동 연장과 작업 취소 명령을 추가했습니다.
- `tunaround init`이 워커 설정과 mesh 환경 설정을 한 번에 생성합니다.
- 새 머신 설치를 Claude Code나 Codex에 맡길 수 있는 설치 프롬프트를 추가했습니다.

### Changed

- 로스터의 총감독 표시는 heartbeat가 아니라 가장 최근 사람 입력을 기준으로 판단합니다.
- 대시보드는 로스터와 작업 상태를 보는 관제 화면에 집중합니다.
- 새 워커 설정의 기본 토큰 환경변수를 `TUNA_BROKER_TOKEN`으로 통일했습니다.

### Removed

- Redis 기반 세션 버스와 관찰 경로를 제거했습니다. 세션과 작업 상태는 SQLite를 사용합니다.
- 대시보드의 직접 제어 엔드포인트를 제거했습니다. Codex 작업 전달은 `codex-relay`와 작업 장부를 거칩니다.

## [0.3.0] - 2026-07-06

작업 위임 대상을 이름으로 직접 맞추는 방식에서, 온라인 에이전트를 발견하고 선택할 수 있는 mesh 운영 구조로 확장했습니다.

### Added

- UUID 라우팅과 태그 검색을 지원하는 에이전트 레지스트리를 추가했습니다.
- `to_selector`로 조건에 맞는 온라인 워커를 찾아 작업을 보낼 수 있습니다.
- Claude Code와 Codex 라이브 세션을 감독 대상으로 등록하는 기능을 추가했습니다.
- 로스터와 작업 피드, 목표 제출을 제공하는 웹 대시보드를 추가했습니다.
- 워커 노드 진단에 형태소 백엔드와 Ollama 연결 상태 확인을 추가했습니다.
- 작업에 실행 러너 정보를 기록하고, 쓰기 워커의 민감 경로 접근을 제한했습니다.
- 토큰을 명령줄에 직접 쓰지 않도록 주요 데몬과 명령이 `TUNA_BROKER_TOKEN`을 읽습니다.

### Security

- 대시보드 쓰기 요청에 로컬 요청 검증을 추가했습니다.
- Codex 제어 WebSocket 대상은 loopback 주소로 제한했습니다.

## [0.2.2] - 2026-07-04

### Added

- 작업 도착 시 지정한 명령을 실행하는 `tunaround poll --on-task`를 추가했습니다.
- 워커가 작업을 선점한 뒤 종료되면 lease 만료 후 다시 대기 상태로 돌립니다.
- 재시도 상한을 넘은 작업은 무한 반복하지 않고 `failed`로 격리합니다.
- 늦게 살아난 이전 워커가 이미 완료된 결과를 덮어쓰지 못하도록 상태 전이 검사를 강화했습니다.

## [0.2.1] - 2026-07-04

### Changed

- 릴리스 바이너리에 워커, HTTP 러너, 외부 A2A 위임 기능을 포함했습니다.
- 설치본 하나로 `serve`, `node`, `poll`, `work`, `--runner http`, `--runner a2a`를 사용할 수 있습니다.

## [0.2.0] - 2026-07-04

작업 브로커와 워커 노드를 도입해 tunaRound를 여러 머신과 여러 종류의 에이전트가 참여하는 오케스트레이터로 확장했습니다.

### Added

- A2A 기반 작업 데이터 모델과 JSON-RPC 엔드포인트를 추가했습니다.
- 작업 등록, 확인, 선점, 완료, 실패를 위한 MCP 도구를 추가했습니다.
- SSE 기반 작업 상태 스트리밍을 추가했습니다.
- `tunaround work` 헤드리스 워커 데몬을 추가했습니다.
- Claude, Codex, OpenCode, HTTP LLM, 외부 A2A 에이전트를 워커 러너로 사용할 수 있습니다.
- `context_id`와 `--context-map`을 사용한 프로젝트별 작업 경로를 추가했습니다.
- `tunaround init`, `node`, `doctor`, `poll`을 추가했습니다.
- 미배달 작업과 고착 작업을 표시하는 운영 신호를 추가했습니다.

### Changed

- GitHub Flow와 PR 기반 CI를 도입했습니다.
- Linux, macOS, Windows에서 build, test, Clippy를 실행합니다.
- MCP 오류 응답, 작업 상태 전이, 프로세스 종료, 검색 오류 계약을 강화했습니다.
- 저장소를 공개로 전환하고 과거 히스토리의 시크릿을 정리했습니다.

### Fixed

- 워커 MCP 세션이 만료되었을 때 자동으로 다시 연결합니다.
- Unix에서 watchdog이 자식 프로세스 그룹을 제대로 종료하도록 수정했습니다.

## [0.1.0] - 2026-07-02

첫 공개 릴리스입니다.

### Added

- Claude Code와 Codex에 역할을 부여해 순차적으로 토론하는 기본 REPL을 추가했습니다.
- 특정 에이전트 지목, 제한된 자동 토론, 대화 분기, 결론 저장을 지원합니다.
- JSON 세션 저장과 SQLite 전사·검색 색인을 추가했습니다.
- Kiwi 기반 한국어 형태소 검색과 lindera 폴백을 추가했습니다.
- Ollama 임베딩과 BM25를 결합한 하이브리드 검색을 추가했습니다.
- 에이전트가 `search_context`와 `read_transcript`를 호출하는 MCP 검색을 추가했습니다.
- 유효성, 분기, 세션, 최신성을 반영한 검색 랭킹을 추가했습니다.
- `core`, `serve`, `join`을 통한 원격 코어 접속을 추가했습니다.
- `tunaround.toml` 프로파일과 cargo-dist 설치 파일을 추가했습니다.

### Known limitations

- Kiwi 네이티브 라이브러리나 모델을 불러오지 못하면 lindera로 폴백합니다. 검색은 계속 동작하지만 결과 품질은 달라질 수 있습니다.
- 의미 검색 서버가 없으면 형태소 FTS만 사용합니다.
- CLI 에이전트의 파일 쓰기 제한은 각 러너와 실행 모드의 제약을 함께 받습니다.

### License

- AGPL-3.0-only

[Unreleased]: https://github.com/hang-in/tunaRound/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.5.0
[0.4.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.4.0
[0.3.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.3.0
[0.2.2]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.2
[0.2.1]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.1
[0.2.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.0
[0.1.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0
