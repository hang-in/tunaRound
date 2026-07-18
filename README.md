# tunaRound

![CI](https://github.com/hang-in/tunaRound/actions/workflows/ci.yml/badge.svg)
![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hang-in/tunaRound)

쓰던 Claude Code에 tunaRound를 붙이면, 거기서 Codex와 다른 에이전트에게 일을 시키고 결과를 받습니다. 터미널을 옮겨다니며 복사·붙여넣기 하지 않아도 됩니다.

에이전트가 사람을 대체하는 도구가 아닙니다. 무엇을 시킬지, 결과를 받아들일지는 항상 당신이 결정합니다.

## 시작하기

`claude`와 `codex` CLI가 설치되고 로그인돼 있다면, 아래가 전부입니다.

```bash
# 설치 (한 줄)
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.sh | sh
# Windows PowerShell:  irm https://github.com/hang-in/tunaRound/releases/latest/download/tunaround-installer.ps1 | iex

# 초기화 + 상주
tunaround init
tunaround node
```

`init`이 설정을 만들고 Claude Code에 tunaround 연결을 등록합니다. 등록 후에는 **Claude Code를 재시작(새 세션)해야 합니다** - 이미 실행 중인 세션에는 새 연결이 로드되지 않습니다.

재시작한 Claude Code 세션에서 평소처럼 말하면 됩니다.

```text
> codex한테 이 설계 검토받아줘
```

Claude가 로컬 Codex에게 일을 맡기고, 끝나면 응답을 가져옵니다. 여기까지가 첫 성공입니다.

뭔가 안 되면 `tunaround doctor`가 설정을 점검해 줍니다.

<details>
<summary>다른 설치 방법 (Homebrew · cargo · AI에게 맡기기)</summary>

```bash
# Homebrew
brew install hang-in/tap/tunaround

# Cargo로 GitHub에서 직접 설치 - 피처를 명시해야 init·node가 포함됩니다
cargo install --git https://github.com/hang-in/tunaRound tunaround --features "mcp serve worker"
```

기본 `cargo install`(피처 미지정)로는 `init`·`node`가 없는 바이너리가 만들어지므로 릴리스 인스톨러를 권장합니다. crates.io에는 게시하지 않습니다.

설치 과정 자체를 에이전트에 맡기려면 [`docs/prompts/install-with-ai.md`](docs/prompts/install-with-ai.md)의 프롬프트를 붙여넣습니다.

</details>

## 무엇을 할 수 있나

| 상황 | tunaRound가 하는 일 |
| --- | --- |
| 다른 에이전트에게 일을 맡기고 결과를 받고 싶다 | 쓰던 Claude Code에서 바로 위임하고, 완료되면 결과를 받습니다. |
| 다른 머신의 에이전트에도 맡기고 싶다 | LAN으로 확장해 작업을 큐에 넣고 그쪽 머신이 받아 처리하게 합니다. |
| 긴 대화에서 이전 결정을 다시 찾고 싶다 | 전사와 문서를 색인해 필요한 맥락만 검색합니다. |
| 구현 전에 설계를 여럿에게 검토받고 싶다 | 쓰던 세션에서 `start_discussion`으로 mesh 토론을 시작합니다. 여러 머신의 에이전트가 역할을 나눠 라운드로 토론하고 합의문이 돌아옵니다. `gate=true`면 라운드 사이마다 멈춰 사람이 승인·조향합니다. |

완전 자율 swarm을 목표로 하지 않습니다. tunaRound의 기본값은 항상 **사용자 주도**입니다.

## 명령

첫 성공에 필요한 것은 두 개뿐입니다.

| 명령 | 용도 |
| --- | --- |
| `tunaround init` | 처음 한 번: 설정 생성, 로컬 에이전트 탐지, Claude Code 연결 등록. |
| `tunaround node` | 상주: 로컬 에이전트들이 맡긴 일을 받아 처리하게 유지. |

문제가 생기면 `tunaround doctor`로 점검합니다.

<details>
<summary>고급 명령 (직접 구성할 때만)</summary>

| 명령 | 용도 |
| --- | --- |
| `tunaround serve <addr>` | 헤드리스 코어(공유 전사·검색 색인·작업 큐)를 직접 실행합니다. |
| `tunaround work` | 작업을 처리하는 워커 데몬을 단독 실행합니다. |
| `tunaround join <url>` | 다른 머신의 코어에 REPL로 접속합니다. |
| `tunaround core <addr>` | REPL과 코어를 한 프로세스로 실행합니다. |
| `tunaround chat` | 설계 토론 REPL(부수 기능)을 시작합니다. |
| `tunaround reindex` | 검색 색인을 다시 만듭니다. |

세부 옵션은 `tunaround <명령> --help`. 일부 명령은 컴파일 피처가 켜진 빌드에서만 나타납니다.

</details>

## 다음 단계

첫 성공 이후, 필요해질 때 순서대로 깊어집니다.

1. **LAN·원격 머신 연결** - 다른 머신의 에이전트에게도 맡기려면 코어를 LAN에 열고 토큰을 설정합니다. 절차는 [온보딩 가이드의 "다른 머신에서 접속하기"](docs/reference/onboarding.md#5-다른-머신에서-접속하기) 절.
2. **워커 노드 조정** - `init`이 만든 `node.toml`의 lane(러너·권한·프로젝트)을 편집해 어떤 에이전트가 어떤 일을 받을지 조정합니다. [온보딩 가이드의 설정 파일](docs/reference/onboarding.md#6-설정-파일) 절 참고.
3. **검색과 기억** - 대화·문서를 SQLite + FTS5로 색인합니다. 한국어 형태소 검색, 선택적 의미 검색, 하이브리드 랭킹은 [온보딩 가이드의 검색 설정](docs/reference/onboarding.md#7-검색-설정) 절.
4. **웹 대시보드** - 세션·작업 상태·결과 피드를 한 화면(`/dashboard`)에서 관찰합니다. v0.5.0부터 릴리스 바이너리에 포함되어 별도 빌드 없이 바로 동작합니다.
5. **mesh 토론** - 쓰던 세션에서 tuna-broker MCP의 `start_discussion`으로 여러 머신의 에이전트에 역할을 주고 라운드 토론을 시킵니다. 전사가 저장되고 합의문이 인박스로 돌아옵니다. `gate=true`(옵트인)면 각 라운드 완료 시 다이제스트가 인박스로 오고, 사람이 `continue_discussion`으로 진행을 승인하거나 조향 지시(steer)를 주입하고, 충분하다 싶으면 종합으로 직행(conclude)합니다. 사용법은 [A2A 사용법의 mesh 토론](docs/reference/a2a-usage.md#8-mesh-토론) 절. 로컬 즉석 토론은 `tunaround chat`(설계 토론 REPL, 부수 기능)도 그대로 있습니다.

## 왜 만들었나

여러 에이전트를 따로 열어두면 설계 토론, 구현 지시, 진행 상태, 결과가 서로 다른 터미널과 머신에 흩어집니다. 결국 사람이 터미널을 오가며 내용을 복사해 붙여넣고, 세션마다 같은 설명을 반복하게 됩니다.

tunaRound는 그 복사·붙여넣기를 없애는 데서 출발했습니다. 당신이 앉아 있는 세션 하나가 지휘석이 되고, 나머지 에이전트는 거기서 맡긴 일을 받아 처리한 뒤 결과를 돌려줍니다. 에이전트가 사람을 대체하는 것이 아니라, 한 사람이 여러 에이전트를 더 안정적으로 다루기 위한 도구입니다.

처음에는 구현 전 설계 토론 REPL로 시작했습니다. 이후 세션 분기, 검색과 기억, 원격 코어, 작업 위임, 워커 노드가 추가되면서 사용자 주도 세션 오케스트레이터로 확장되었습니다.

## 문서

- [전체 문서 안내](docs/index.md)
- [온보딩 가이드](docs/reference/onboarding.md)
- [mesh 아키텍처](docs/reference/mesh-architecture.md)
- [A2A 작업 위임 사용법](docs/reference/a2a-usage.md)
- [macOS와 Windows 구성](docs/reference/dev-mac-windows.md)
- [소스 빌드와 개발 실행](docs/development/source-run.md)
- [개발 규칙](docs/reference/development-guidelines.md)
- [버전과 릴리스 정책](docs/reference/versioning.md)
- [v1 설계 기록](docs/design/tunaRound-v1-design_2026-06-29.md)
- [변경 내역](CHANGELOG.md)

코드베이스를 AI 위키 형태로 탐색하려면 [DeepWiki](https://deepwiki.com/hang-in/tunaRound)를 사용할 수 있습니다.

## 기술 스택

Rust · tokio · SQLite + FTS5 · Ollama 임베딩 · MCP · A2A 기반 작업 브로커 · clap CLI · React 대시보드 · cargo-dist

## 라이선스

[AGPL-3.0](LICENSE) — GNU Affero General Public License v3.0
