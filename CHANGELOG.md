# Changelog

사용자에게 영향을 주는 주요 변경을 버전별로 기록합니다.

- 가장 최근 개발 내용은 `Unreleased`에서 확인합니다.
- 설치와 사용법은 [README](README.md)와 [문서 색인](docs/index.md)을 참고하세요.
- 내부 구현 계획과 세션 기록은 이 문서에 넣지 않습니다.

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/)를 따르고, 버전은 [Semantic Versioning](https://semver.org/lang/ko/)을 지향합니다.

## [Unreleased]

### Added

- 대시보드에 세션 등장·소멸과 사람 입력 이력을 시간순으로 보여주는 presence 타임라인 패널을 추가했습니다.
- 토론 발언에 증류된 결정 요약과 검색 앵커를 남기는 `/annotate` 명령을 추가했습니다. 검색이 요약을 원문 앞에 얹어 보여주고 앵커가 맞으면 관련 발언을 위로 올립니다.

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

[Unreleased]: https://github.com/hang-in/tunaRound/compare/v0.4.0...HEAD
[0.4.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.4.0
[0.3.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.3.0
[0.2.2]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.2
[0.2.1]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.1
[0.2.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.2.0
[0.1.0]: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0
