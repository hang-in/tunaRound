# tunaRound

![CI](https://github.com/hang-in/tunaRound/actions/workflows/ci.yml/badge.svg)
![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hang-in/tunaRound)

로컬 머신과 LAN 안에 열려 있는 터미널 에이전트 세션(Claude Code, Codex, OpenCode, 로컬 LLM)을 하나의 작업 흐름으로 묶는 **개인용 도구**입니다. 소스는 공개하지만 실제로 돌리는 서비스는 사적으로 운용합니다.

처음에는 구현 전에 여러 에이전트와 설계를 토론하는 REPL로 시작했고, 지금은 토론, 세션 분기, 검색과 기억, 원격 코어, A2A 작업 위임, 워커 노드까지 한 흐름으로 묶습니다. 사람이 목표와 판단을 쥐고, 에이전트 세션은 관측·호출·위임·수집되는 실행 단위로 참여합니다. 사람 없이 무한히 도는 swarm이 아니라, 항상 **사용자 주도**가 기본값입니다.

> 코드베이스를 AI 위키로 둘러보려면: [DeepWiki로 tunaRound 살펴보기](https://deepwiki.com/hang-in/tunaRound).

---

## 무엇인가

기본 단위는 단일 채팅이나 자동 토론 루프가 아니라 **사용자 주도 세션 오케스트레이션**입니다. CLI나 웹 대시보드에서 여러 세션을 보고, 역할을 부여하고, 작업을 맡기고, 결과를 다시 모아 판단합니다.

| 흐름 | 설명 |
| --- | --- |
| 세션 관측 | 로컬 머신과 LAN 안의 실행 중인 TUI 에이전트 세션을 확인합니다. |
| 설계 토론 | Claude Code, Codex, OpenCode 등에 서로 다른 역할을 주고 설계를 검토합니다. |
| 작업 위임 | 특정 세션이나 워커 노드에 구현·검토·정리 작업을 맡깁니다. |
| 결과 수집 | 토론 결론, 작업 결과, 문서, 로그를 모아 다음 판단 기준으로 씁니다. |
| 검색과 기억 | 긴 세션 전사와 문서를 색인하고 필요한 맥락만 다시 끌어옵니다. |

---

## 빠른 시작

`claude`와 `codex` CLI가 설치되고 인증까지 되어 있으면 바로 시작할 수 있습니다.

```bash
tunaround chat
```

```text
> 결제 모듈을 어떻게 설계할까?      # claude(제안자) + codex(리뷰어)가 응답
> @codex 이 설계에서 위험한 부분만 봐줘   # codex에게만 질문
> @codex! 이 함수 고쳐줘            # codex가 실제 파일을 수정
> /debate 3 이 방향 괜찮나          # 최대 3턴 제한 자동 토론
> /branches                        # 대화 분기 보기
> /conclude                        # 지금까지 토론 정리
> /search 인증 설계                 # 과거 대화와 문서 검색
> /save design.md                  # 토론 결과 저장
```

전사를 저장하고 `/search`를 쓰려면 DB를 지정합니다. 세션 이어가기·로스터 지정도 가능합니다.

```bash
tunaround chat --db tuna.db
tunaround chat session.json                 # 저장한 세션 이어가기
tunaround chat --roster examples/roster.json  # 역할·엔진 직접 지정
```

원격 코어에 붙거나 브로커·워커를 운용하는 셋업은 [온보딩 가이드](docs/reference/onboarding.md)를 참고하세요.

## 주요 명령

| 명령 | 설명 |
| --- | --- |
| `tunaround chat` | 기본 토론 REPL. |
| `tunaround core <addr>` | REPL과 in-process HTTP MCP 코어를 한 프로세스로. |
| `tunaround serve <addr>` | 헤드리스 코어(REPL 없음). 웹 대시보드도 여기서. |
| `tunaround join <url>` | 원격 코어 접속 프리셋. |
| `tunaround init` | 워커 노드 설정(`node.toml`) 생성(러너 자동 탐지). |
| `tunaround doctor` | 워커 노드 설정 진단(코어 도달·토큰·러너·경로). |
| `tunaround node` | 설정대로 브로커와 워커를 한 프로세스로 상주. |
| `tunaround work` | A2A 작업 위임 워커 데몬. |
| `tunaround reindex` | 검색 색인 재생성. |

옵션은 `tunaround <명령> --help`로 확인합니다. (`serve`/`worker` 피처가 없는 기본 빌드에는 `serve`/`work`/`node`/`doctor`가 없습니다. [온보딩 가이드](docs/reference/onboarding.md)의 피처 표를 참고하세요.)

---

## 핵심 기능

- **역할 기반 다중 에이전트 토론.** Claude는 제안자, Codex는 리뷰어처럼 자리마다 역할을 줍니다. 같은 레포를 봐도 역할이 다르면 다른 관점이 나옵니다. `/debate`로 제한된 자동 토론, `/branches`·`/checkout`으로 분기, `/conclude`·`/save`로 결론을 문서로 남깁니다.
- **같은 레포 직접 읽기.** 긴 컨텍스트를 복붙하지 않습니다. 각 에이전트가 자기 CLI로 현재 작업 디렉터리를 직접 읽고 판단합니다.
- **검색과 기억.** 대화·문서를 SQLite + FTS5로 색인하고 필요한 조각만 검색해 가져옵니다(pull 방식). 한국어 형태소 검색(Kiwi, 실패 시 lindera 폴백), 선택적 Ollama 의미 검색 + BM25 하이브리드, 유효성·최신성 반영 랭킹(`/reject`·`/supersede`). MCP를 켜면 에이전트가 토론 중 직접 `search_context`·`read_transcript`를 씁니다. 검색 백엔드 설정은 [온보딩 가이드](docs/reference/onboarding.md#검색-설정)에 있습니다.
- **웹 대시보드**(`--features dashboard`로 빌드한 뒤 `serve`의 `/dashboard`). 로컬/LAN 세션 로스터 + 라이브 task 피드 + 목표 제출을 한 화면에서 봅니다. 관제탑에 충실한 뷰로, 로컬(loopback)에서만 목표를 제출할 수 있고 원격 접속은 읽기 전용 관전입니다.
- **A2A 작업 위임과 워커 노드.** 코어가 제한된 범위("A2A 기반")의 작업 브로커로 동작합니다. 사람이 목표를 정하면 코어가 작업을 큐에 올리고, 워커 에이전트(Claude·Codex·로컬 LLM·외부 표준 A2A 에이전트)가 자기 앞 작업을 발견·처리합니다. 구조·역할과 표준 호환 범위는 [mesh 아키텍처](docs/reference/mesh-architecture.md), 명령 흐름은 [a2a-usage](docs/reference/a2a-usage.md)를 참고하세요.

자율 수준은 **semi-A2A**입니다. 사람이 목표를 정하고, 발견·실행·완료·통지는 기계끼리 처리합니다. 사람 없이 무한히 도는 자동 토론 루프는 의도적으로 두지 않습니다.

---

## 설치

**가장 쉬운 방법 - AI에게 맡기기**: [`docs/prompts/install-with-ai.md`](docs/prompts/install-with-ai.md)의 프롬프트를 새 머신의 Claude Code(또는 Codex)에 붙여넣으면 AI가 OS 감지·설치·`init`·`doctor`까지 대신합니다(사람은 역할과 코어 주소·토큰만). 아래는 직접 하는 경우입니다.

소스는 공개되어 있습니다(crates.io에는 게시하지 않습니다). 릴리스 바이너리 또는 소스 빌드 중 편한 쪽을 씁니다.

```bash
# Shell (macOS · Linux)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.sh | sh

# PowerShell (Windows)
irm https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.ps1 | iex

# Homebrew (macOS · Linux)
brew install hang-in/tap/tunaround

# Cargo (git에서 직접)
cargo install --git https://github.com/hang-in/tunaRound tunaround
```

> 릴리스 바이너리는 웹 대시보드 SPA를 포함하지 않습니다(플레이스홀더 페이지). 대시보드를 쓰려면 `frontend`를 빌드한 뒤 `--features dashboard`로 소스 빌드해야 합니다. 피처 조합·소스 빌드·개발 실행은 [온보딩 가이드](docs/reference/onboarding.md)와 [`docs/development/source-run.md`](docs/development/source-run.md)에 있습니다.

---

## 온보딩

셋업은 무엇을 하려는지에 따라 세 갈래입니다. 각 갈래의 단계와 알아둘 함정(피처 divergence, 설정 파일 3종, 토큰 로테이션)은 [온보딩 가이드](docs/reference/onboarding.md)에 정리했습니다.

- **로컬 1인**: `claude`/`codex` 인증 후 `tunaround chat`. 검색·기억은 `--db`.
- **브로커·대시보드 호스팅**: `serve`로 코어를 띄우고 대시보드를 엽니다(대시보드는 소스 빌드 필요).
- **머신 합류**: `init` → `doctor` → `node`로 워커 노드를 상주시키거나, `join`으로 REPL을 원격 코어에 붙입니다.

## 여러 머신에서 함께 운용하기

여러 컴퓨터가 하나의 코어에 붙어 같은 전사·검색 색인·작업 큐를 공유합니다. "공유 화이트보드 + 각자의 비서 + 작업 큐" 구조입니다. 전사는 복제하지 않고, 필요할 때 코어에 원격으로 물어보고 발언은 코어 전사에 기록합니다.

```bash
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>   # 코어를 띄우는 컴퓨터
tunaround join http://<코어-IP>:8770/mcp --token <토큰>       # 접속하는 컴퓨터
```

필요한 것은 코어 주소, bearer 토큰, 서로 닿는 네트워크입니다. 같은 공유기 안에서는 사설 IP, 외부에서는 Tailscale이나 SSH 터널을 씁니다. mesh 구성과 역할은 [mesh 아키텍처](docs/reference/mesh-architecture.md), 맥↔윈도우 실무는 [dev-mac-windows](docs/reference/dev-mac-windows.md)를 참고하세요.

---

## 왜 만들었나

코드를 바로 짜기 전에 설계를 먼저 검토하고 싶을 때가 있습니다. 또 여러 터미널과 여러 머신에 에이전트를 열어두면 각 세션의 상태·결정·작업 결과가 쉽게 흩어집니다. 혼자 생각하면 놓치는 부분이 있고, 한 에이전트에게만 물으면 답이 한 방향으로 굳습니다. 여러 에이전트를 따로 켜고 복붙으로 조율하면 토론·결정·구현 지시·결과가 분리됩니다.

tunaRound는 이 과정을 하나의 사용자 주도 세션 흐름 안에서 반복 가능하게 만듭니다. 목표는 에이전트가 사람을 대체하는 것이 아니라, 사용자가 여러 에이전트를 더 안정적으로 부리는 것입니다.

## 어디서 가져왔나

기존 프로젝트에서 검증한 기능들을 작게 묶은 도구입니다.

- 토론 흐름은 **tunapi**의 roundtable 구조에서 가져왔습니다.
- CLI 실행과 스트림 처리는 **tunaFlow**의 runner 경험을 바탕으로 했습니다.
- 세션 공유·관찰의 초기 아이디어는 **tunaSalon**에서 가져왔습니다(현재 구현은 SQLite 기반).
- 한국어 검색·하이브리드 검색은 **seCall**의 경험을 옮겨왔습니다.

## 기술 스택

Rust · tokio · JSON 세션 파일 · SQLite + FTS5 · Ollama 임베딩 · MCP · A2A-lite task broker · clap CLI · cargo-dist. 기본 UI는 가벼운 REPL이고, 웹 대시보드(React SPA)는 `dashboard` 피처로 임베드합니다.

---

## 다음

지금까지의 변경 내역은 [CHANGELOG](CHANGELOG.md)에 있습니다. 남은 방향은 다음과 같습니다.

- [ ] 세션을 넘나드는 프로젝트 기억
- [ ] 리치 TUI

---

## 문서

- 전체 문서 색인: [`docs/index.md`](docs/index.md)
- 현행 설계 스펙: [`docs/design/tunaRound-v1-design_2026-06-29.md`](docs/design/tunaRound-v1-design_2026-06-29.md)
- 온보딩: [`docs/reference/onboarding.md`](docs/reference/onboarding.md) · mesh 아키텍처: [`docs/reference/mesh-architecture.md`](docs/reference/mesh-architecture.md) · A2A 사용법: [`docs/reference/a2a-usage.md`](docs/reference/a2a-usage.md)
- 소스 빌드·개발 실행: [`docs/development/source-run.md`](docs/development/source-run.md) · 개발 규율: [`docs/reference/development-guidelines.md`](docs/reference/development-guidelines.md) · 버전 정책: [`docs/reference/versioning.md`](docs/reference/versioning.md)

## 라이선스

[AGPL-3.0](LICENSE) - GNU Affero General Public License v3.0
