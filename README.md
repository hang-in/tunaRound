# tunaRound

![CI](https://github.com/hang-in/tunaRound/actions/workflows/ci.yml/badge.svg)
![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hang-in/tunaRound)

tunaRound는 여러 터미널 에이전트 세션을 한곳에서 보고, 대화하고, 작업을 맡기고, 결과를 모으는 개인용 오케스트레이터입니다.

Claude Code, Codex, OpenCode, 로컬 LLM 세션을 로컬 머신이나 LAN 안에서 연결합니다. 사람이 목표와 최종 판단을 맡고, 에이전트는 토론·검토·구현·검색 같은 실행 단위로 참여합니다.

## 무엇을 할 수 있나

| 상황 | tunaRound가 하는 일 |
| --- | --- |
| 구현 전에 설계를 검토하고 싶다 | 여러 에이전트에 역할을 나눠 토론시킵니다. |
| 터미널 세션이 여러 개라 상태를 놓친다 | 로컬과 LAN의 세션을 한 화면에서 관찰합니다. |
| 다른 머신의 에이전트에 일을 맡기고 싶다 | 작업을 큐에 넣고 워커가 받아 처리하게 합니다. |
| 긴 대화에서 이전 결정을 다시 찾고 싶다 | 전사와 문서를 색인해 필요한 맥락만 검색합니다. |
| 작업 결과가 여러 곳에 흩어진다 | 결론, 로그, 문서, 작업 결과를 다시 모읍니다. |

완전 자율 swarm을 목표로 하지 않습니다. tunaRound의 기본값은 항상 **사용자 주도**입니다.

## 가장 빠르게 시작하기

`claude`와 `codex` CLI가 설치되고 로그인까지 되어 있다면 다음 명령으로 시작합니다.

```bash
tunaround chat
```

REPL에서는 일반 대화와 지목, 제한된 자동 토론, 분기, 검색을 함께 사용할 수 있습니다.

```text
> 결제 모듈을 어떻게 설계할까?
> @codex 이 설계에서 위험한 부분만 봐줘
> @codex! 이 함수 고쳐줘
> /debate 3 이 방향 괜찮나
> /branches
> /conclude
> /search 인증 설계
> /save design.md
```

대화를 저장하고 검색하려면 DB를 지정합니다.

```bash
tunaround chat --db tuna.db
```

저장한 세션을 이어가거나 역할 구성을 직접 지정할 수도 있습니다.

```bash
tunaround chat session.json
tunaround chat --roster examples/roster.json
```

여러 머신을 연결하거나 워커를 상주시킬 때는 [온보딩 가이드](docs/reference/onboarding.md)를 먼저 읽는 편이 빠릅니다.

## 설치

릴리스 바이너리 또는 소스 설치 중 편한 방법을 사용합니다. crates.io에는 게시하지 않습니다.

```bash
# macOS · Linux
curl --proto '=https' --tlsv1.2 -LsSf \
  https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.sh | sh

# Windows PowerShell
irm https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.ps1 | iex

# Homebrew
brew install hang-in/tap/tunaround

# Cargo로 GitHub에서 직접 설치
cargo install --git https://github.com/hang-in/tunaRound tunaround
```

설치 과정을 에이전트에 맡기려면 [`docs/prompts/install-with-ai.md`](docs/prompts/install-with-ai.md)의 프롬프트를 Claude Code나 Codex에 붙여넣습니다.

> 릴리스 바이너리에는 웹 대시보드 SPA가 포함되지 않습니다. 대시보드가 필요하면 프런트엔드를 빌드한 뒤 `dashboard` 피처로 소스 빌드해야 합니다. 자세한 절차는 [온보딩 가이드](docs/reference/onboarding.md)와 [소스 빌드 가이드](docs/development/source-run.md)에 있습니다.

## 주요 명령

| 명령 | 용도 |
| --- | --- |
| `tunaround chat` | 로컬 토론 REPL을 시작합니다. |
| `tunaround core <addr>` | REPL과 HTTP MCP 코어를 한 프로세스로 실행합니다. |
| `tunaround serve <addr>` | 헤드리스 코어와 웹 대시보드를 실행합니다. |
| `tunaround join <url>` | 다른 머신의 코어에 REPL로 접속합니다. |
| `tunaround init` | 워커 노드 설정을 생성하고 러너를 탐지합니다. |
| `tunaround doctor` | 코어, 토큰, 러너, 경로 설정을 점검합니다. |
| `tunaround node` | 브로커와 워커를 설정대로 상주시킵니다. |
| `tunaround work` | A2A 작업을 처리하는 워커 데몬을 실행합니다. |
| `tunaround reindex` | 검색 색인을 다시 만듭니다. |

세부 옵션은 `tunaround <명령> --help`에서 확인합니다. 일부 명령은 컴파일 피처가 켜진 빌드에서만 나타납니다.

## 사용 형태

### 1. 한 머신에서 토론과 검색

```bash
tunaround chat --db tuna.db
```

가장 단순한 형태입니다. 별도 브로커나 워커가 필요하지 않습니다.

### 2. 한 머신을 코어로 사용

```bash
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>
```

이 머신이 공유 전사, 검색 색인, 작업 큐를 보관합니다.

### 3. 다른 머신을 연결

```bash
tunaround join http://<코어-IP>:8770/mcp --token <토큰>
```

워커 노드로 상주시킬 때는 다음 흐름을 사용합니다.

```bash
tunaround init --core http://<코어-IP>:8770/mcp --machine mac
tunaround doctor
tunaround node
```

같은 LAN에서는 사설 IP를 쓰고, 외부 네트워크에서는 Tailscale이나 SSH 터널처럼 별도의 안전한 연결을 사용합니다.

## 핵심 기능

### 역할을 나눈 설계 토론

Claude는 제안자, Codex는 리뷰어처럼 서로 다른 역할을 줄 수 있습니다. `/debate`로 토론 길이를 제한하고, `/branches`와 `/checkout`으로 대화를 분기하며, `/conclude`와 `/save`로 결론을 남깁니다.

### 같은 작업 디렉터리 직접 사용

긴 컨텍스트를 매번 복사하지 않습니다. 각 에이전트가 자기 CLI를 통해 현재 작업 디렉터리를 직접 읽고 판단합니다.

### 검색과 기억

대화와 문서를 SQLite + FTS5로 색인합니다. 한국어 형태소 검색, 선택적 의미 검색, BM25 하이브리드 검색, 유효성·최신성 기반 랭킹을 지원합니다.

### 웹 대시보드

로컬과 LAN의 세션, 작업 상태, 결과 피드를 한 화면에서 봅니다. 로컬 접속에서는 목표를 제출할 수 있고, 원격 접속은 기본적으로 읽기 전용입니다.

### A2A 작업 위임

사람이 목표를 정하면 코어가 작업을 큐에 넣고 워커가 가져가 실행합니다. 작업 발견, 실행, 완료 통지는 자동화하지만, 무엇을 할지와 결과를 채택할지는 사람이 결정합니다.

구조는 [mesh 아키텍처](docs/reference/mesh-architecture.md), 실제 명령은 [A2A 작업 위임 사용법](docs/reference/a2a-usage.md)에 정리되어 있습니다.

## 왜 만들었나

여러 에이전트를 따로 열어두면 설계 토론, 구현 지시, 진행 상태, 결과가 서로 다른 터미널과 머신에 흩어집니다. 결국 사람이 내용을 복사하고 세션마다 같은 설명을 반복하게 됩니다.

tunaRound는 이 과정을 하나의 작업 흐름으로 묶습니다. 에이전트가 사람을 대체하는 것이 아니라, 한 사람이 여러 에이전트를 더 안정적으로 다루기 위한 도구입니다.

처음에는 구현 전 설계 토론 REPL로 시작했습니다. 이후 세션 분기, 검색과 기억, 원격 코어, A2A 작업 위임, 워커 노드가 추가되면서 사용자 주도 세션 오케스트레이터로 확장되었습니다.

## 문서

- [전체 문서 안내](docs/index.md)
- [온보딩 가이드](docs/reference/onboarding.md)
- [mesh 아키텍처](docs/reference/mesh-architecture.md)
- [A2A 작업 위임 사용법](docs/reference/a2a-usage.md)
- [macOS ↔ Windows 구성](docs/reference/dev-mac-windows.md)
- [소스 빌드와 개발 실행](docs/development/source-run.md)
- [개발 가이드](docs/reference/development-guidelines.md)
- [버전 정책](docs/reference/versioning.md)
- [현행 설계 스펙](docs/design/tunaRound-v1-design_2026-06-29.md)
- [변경 내역](CHANGELOG.md)

코드베이스를 AI 위키 형태로 탐색하려면 [DeepWiki](https://deepwiki.com/hang-in/tunaRound)를 사용할 수 있습니다.

## 기술 스택

Rust · tokio · SQLite + FTS5 · Ollama 임베딩 · MCP · A2A 기반 작업 브로커 · clap CLI · React 대시보드 · cargo-dist

## 라이선스

[AGPL-3.0](LICENSE) — GNU Affero General Public License v3.0