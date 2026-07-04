# tunaRound

![CI](https://github.com/hang-in/tunaRound/actions/workflows/ci.yml/badge.svg)
![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0-blue.svg)
![Rust](https://img.shields.io/badge/rust-edition%202024-orange.svg)
[![Ask DeepWiki](https://deepwiki.com/badge.svg)](https://deepwiki.com/hang-in/tunaRound)

구현 전에 Claude Code, Codex, OpenCode 같은 코딩 에이전트와 **설계를 먼저 토론하는** 터미널 도구입니다.

하나의 에이전트에게 바로 코드를 맡기기 전에, 여러 에이전트에게 서로 다른 역할을 줍니다. 한 에이전트는 설계를 제안하고, 다른 에이전트는 반박하거나 검토합니다. 사용자는 진행자이자 최종 결정자로 남습니다.

토론이 끝나면 결론을 `design.md` 같은 문서로 저장하고, 필요하면 그 자리에서 구현이나 작업 위임으로 이어갈 수 있습니다.

> 코드베이스 구조를 AI 위키로 둘러보려면: [DeepWiki로 tunaRound 살펴보기](https://deepwiki.com/hang-in/tunaRound).

---

## 언제 쓰나

- 기능을 바로 구현하기 전에 설계를 먼저 검토하고 싶을 때
- Claude와 Codex의 의견을 같은 레포 기준으로 비교하고 싶을 때
- 한 에이전트의 제안을 다른 에이전트에게 리뷰시키고 싶을 때
- 긴 토론 결과를 문서로 남기고 구현 기준으로 삼고 싶을 때
- 여러 머신의 에이전트에게 작업을 나눠 맡기고 싶을 때

## 기본 흐름

```
사용자가 주제를 던진다
  → Claude가 설계를 제안한다
  → Codex가 검토하거나 반박한다
  → 사용자가 방향을 정한다
  → 결론을 문서로 저장한다
  → 필요하면 에이전트에게 구현을 맡긴다
```

> tunaRound는 에이전트들이 사람 없이 무한히 대화하는 도구가 아닙니다.
> 기본값은 항상 **사용자 주도 토론**입니다.

---

## 설치

**Homebrew**

```bash
brew install hang-in/tap/tunaround
```

**PowerShell**

```powershell
# 릴리스 후 설치 스크립트 URL 확정
irm <install-script-url> | iex
```

**Cargo** (소스에서 직접 설치. crates.io에는 게시하지 않습니다)

```bash
cargo install --git https://github.com/hang-in/tunaRound tunaround
```

> 소스에서 직접 빌드하거나 개발 모드로 실행하려면 [`docs/development/source-run.md`](docs/development/source-run.md)를 참고하세요.

---

## 빠른 시작

`claude`와 `codex` CLI가 설치되어 있고 인증까지 되어 있다면 바로 시작할 수 있습니다.

```bash
tunaround chat
```

예시:

```
> 결제 모듈을 어떻게 설계할까?
> @codex 이 설계에서 위험한 부분만 봐줘
> /debate 3 이 방향 괜찮나
> /conclude
> /save design.md
```

## 주요 명령

| 명령 | 설명 |
| --- | --- |
| `tunaround chat` | 기본 REPL을 실행합니다. |
| `tunaround core <addr>` | 단일 프로세스 코어를 실행합니다. |
| `tunaround serve <addr>` | 헤드리스 코어를 실행합니다. |
| `tunaround join <url>` | 원격 코어에 접속합니다. |
| `tunaround init` | 워커 노드 설정(`node.toml`)을 만듭니다. |
| `tunaround node` | 설정대로 브로커와 워커를 한 프로세스로 상주시킵니다. |
| `tunaround doctor` | 워커 노드 설정을 진단합니다. |
| `tunaround work` | A2A 작업 위임 워커 데몬을 실행합니다. |
| `tunaround poll` | 새 작업이 오면 알리는 감시 전용 모드입니다. |
| `tunaround reindex` | 검색 색인을 다시 만듭니다. |

옵션은 다음 명령으로 확인합니다.

```bash
tunaround <명령> --help
```

## REPL 사용 예시

```
> 결제 모듈을 어떻게 설계할까?      # claude(제안자) + codex(리뷰어)가 응답
> @codex 이 부분만 봐줘            # codex에게만 질문
> @codex! 이 함수 고쳐줘            # codex가 실제 파일을 수정
> /debate 3 이 설계 괜찮나          # 최대 3턴 제한 자동 토론
> /branches                        # 대화 분기 보기
> /checkout 2                      # 특정 분기로 이동
> /conclude                        # 지금까지 토론 정리
> /search 인증 설계                 # 과거 대화와 문서 검색
> /save design.md                  # 토론 결과 저장
> /quit
```

세션을 이어서 쓰려면 상태 파일을 넘깁니다.

```bash
tunaround chat session.json
```

여러 역할과 엔진을 직접 정하려면 로스터 파일을 사용합니다.

```bash
tunaround chat --roster examples/roster.json
```

---

## 여러 머신에 작업 맡기기 (A2A 워커 노드)

토론을 넘어, 여러 대의 컴퓨터를 "작업을 받아 처리하는 워커"로 묶을 수 있습니다. 한 대에 설정 하나를 만들고 백그라운드에 데몬 하나만 띄우면, 그 기계가 대기하다가 던져진 작업을 스스로 처리하고 결과를 돌려줍니다.

```bash
tunaround init      # node.toml 생성 (설치된 러너 자동 탐지)
tunaround doctor    # 설정 진단 (코어 도달, 러너, 경로 점검)
tunaround node      # 브로커와 워커를 한 프로세스로 상주
```

목표는 사람이 쥡니다. 무엇을 할지는 사람이 정하고, 발견·실행·완료 통지는 에이전트들끼리 처리합니다(semi-a2a). 러너를 바꾸면 Claude, Codex, 로컬 LLM 등 서로 다른 파트너를 붙일 수 있습니다.

작업을 던지고 받는 전체 흐름과 설정은 [`docs/reference/a2a-usage.md`](docs/reference/a2a-usage.md), "에이전트 개발팀"으로 굴리는 방법은 [`docs/reference/agent-dev-team.md`](docs/reference/agent-dev-team.md)를 참고하세요.

---

## 핵심 기능

### 사용자 주도 설계 토론

사용자가 질문하거나 방향을 잡으면 에이전트들이 응답합니다. 에이전트끼리 끝없이 자동 대화하지 않습니다.

필요할 때만 `/debate`로 제한된 자동 토론을 실행할 수 있습니다.

```
> /debate 3 이 구조가 과한지 검토해줘
```

### 역할 기반 에이전트

Claude는 제안자, Codex는 리뷰어처럼 역할을 나눌 수 있습니다. 같은 레포를 보더라도 역할이 다르면 다른 관점이 나옵니다.

예시:

- **Claude** - 설계 제안자
- **Codex** - 코드 리뷰어
- **OpenCode** - 대안 구현 검토자
- **Local LLM** - 빠른 초안 생성자

### 같은 레포 직접 읽기

긴 컨텍스트를 복사해서 붙여넣지 않습니다. 각 에이전트가 자기 CLI로 현재 작업 디렉터리를 직접 읽고 판단합니다.

```
사용자
  → tunaRound
  → Claude Code / Codex / OpenCode
  → 현재 레포 직접 확인
  → 응답 또는 수정
```

### 문서 저장

토론 결과를 `design.md` 같은 문서로 저장할 수 있습니다.

```
> /conclude
> /save design.md
```

토론 따로, 구현 기준 따로 흩어지는 일을 줄입니다.

### 검색과 기억

긴 토론을 매번 전부 다시 넣지 않도록, 대화와 문서를 색인해두고 필요한 부분만 검색해서 가져옵니다.

```bash
tunaround chat --db tuna.db
```

검색 관련 기능은 필요할 때만 켤 수 있습니다.

```bash
tunaround chat --db tuna.db --features semantic
```

소스 빌드 환경에서는 다음 feature 조합을 사용할 수 있습니다.

```bash
cargo run --features "semantic mcp" -- chat --db tuna.db
```

---

## 검색 상세

### SQLite + FTS5

대화와 문서를 SQLite에 저장하고 FTS5로 검색합니다. `/search` 명령도 사용할 수 있습니다.

```
> /search 인증 설계
```

### 한국어 형태소 검색

한국어 검색을 위해 형태소 분석을 사용합니다. 예를 들어 "검색을" 같은 표현도 "검색"으로 잘 잡도록 돕습니다. Kiwi를 우선 사용하고, 실패하면 lindera로 자동 폴백합니다.

Kiwi 네이티브 라이브러리와 모델은 첫 실행 때 OS 캐시에 자동으로 내려받습니다. 자동 다운로드가 실패해도 lindera로 동작합니다.

다운로드가 막힌 환경에서는 다음 환경변수로 직접 지정할 수 있습니다.

```bash
export KIWI_RS_VERSION=<version>
export KIWI_LIBRARY_PATH=<path>
```

### 의미 검색

Ollama 임베딩으로 의미 검색을 추가합니다. 기본값은 다음과 같습니다.

```bash
TUNAROUND_OLLAMA_URL=http://127.0.0.1:11435
TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b
```

환경변수로 바꿀 수 있습니다.

```bash
export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11435
export TUNAROUND_EMBED_MODEL=bge-m3
```

모델을 바꾸면 다음 색인 때 자동으로 재임베딩됩니다.

### MCP 기반 맥락 검색

MCP 기능을 켜면 에이전트가 토론 중 직접 과거 맥락을 검색하고, 지금까지의 전사를 읽을 수 있습니다.

제공 도구:

- `search_context`
- `read_transcript`

검색 결과는 단순 관련도만 보지 않습니다. 유효성과 최신성도 함께 반영합니다.

- `/reject`로 무효화한 발언은 제외합니다.
- `/supersede`로 대체된 발언은 뒤로 내립니다.
- 서로 다른 세션에서 온 오래된 결과는 약하게 낮춥니다.
- 관련성이 높은 오래된 결정은 함부로 밀어내지 않습니다.

---

## 설정 프로파일

`--db`, `--roster`, `--search-url`처럼 반복되는 옵션은 `tunaround.toml`에 프로파일로 저장할 수 있습니다.

레포 루트의 `tunaround.toml.example`을 참고해 `tunaround.toml`로 복사한 뒤 값을 채웁니다. 이 파일은 gitignore 대상이라 사설 도메인이나 토큰을 넣어도 커밋되지 않습니다.

```toml
default_profile = "local"

[profile.local]
db = "~/.tunaround/local.db"
pull_context = false

[profile.homelab]
db = "~/.tunaround/homelab.db"
search_url = "https://your-core-host.example/mcp"
search_token_env = "TUNA_TOKEN"
pull_context = true
```

실행할 때 프로파일을 지정합니다.

```bash
tunaround chat --profile homelab
```

설정 파일은 다음 순서로 찾습니다.

1. `--config <경로>`
2. `./tunaround.toml`
3. `~/.config/tunaround/config.toml`

프로파일이 여러 개인데 `default_profile`도 `--profile`도 없으면 번호를 골라 선택하는 대화형 프롬프트가 뜹니다.

값 우선순위는 다음과 같습니다.

```
CLI 플래그 > 선택된 프로파일 > 기본값
```

토큰은 설정 파일에 평문으로 적을 수도 있지만, `search_token_env`로 환경변수 이름만 적어두는 쪽을 권장합니다.

> 프로파일 옵션은 `chat`, `core`, `join`에서만 적용됩니다. `serve`, `mcp-search`, `reindex`에는 적용되지 않습니다.

---

## 여러 터미널에서 같이 보기

Redis를 연결하면 여러 터미널에서 같은 세션을 공유할 수 있습니다.

```bash
export TUNAROUND_REDIS_URL=redis://127.0.0.1:6379

tunaround chat --session <id>   # 기존 세션 이어서 진행
tunaround chat --observe <id>   # 읽기 전용 관찰
```

이 기능은 한 터미널에서는 토론을 진행하고, 다른 터미널에서는 진행 상황만 볼 때 사용합니다.

---

## 여러 머신에서 함께 토론하기

두 사람이 각자 다른 컴퓨터에서 같은 설계 토론에 참여할 수 있습니다. 예를 들어 맥과 윈도우가 하나의 코어에 붙어 같은 전사를 공유할 수 있습니다.

원리는 "공유 화이트보드 + 각자의 비서"에 가깝습니다.

- **코어 = 공유 화이트보드**
  한 컴퓨터가 코어를 띄웁니다. 코어는 토론 전사와 검색 색인을 담은 유일한 원본입니다.
- **각 컴퓨터 = 각자의 비서**
  다른 컴퓨터는 코어에 접속합니다. Claude, Codex 같은 에이전트는 각 컴퓨터에서 로컬로 실행됩니다.
- **전사는 복제하지 않고 공유합니다.**
  각 머신이 DB 사본을 만들어 동기화하는 방식이 아닙니다. 필요할 때 코어에 원격으로 물어보고, 발언은 코어 전사에 기록합니다.
- **필요한 맥락만 당겨옵니다.**
  에이전트는 전체 전사를 매번 받지 않고 필요한 조각만 가져옵니다. 토론이 길어져도 프롬프트가 가볍게 유지됩니다.

코어를 띄우는 컴퓨터:

```bash
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>
```

접속하는 컴퓨터:

```bash
tunaround join http://<코어-IP>:8770/mcp --token <토큰>
```

필요한 것은 코어 주소, bearer 토큰, 서로 닿는 네트워크입니다. 같은 공유기 안에서는 사설 IP를 쓰면 되고, 외부에서는 Tailscale이나 SSH 터널을 사용할 수 있습니다.

> Redis 기반 관찰 기능은 이 구조와 별개입니다. Redis는 여러 터미널에서 같은 세션을 보는 용도이고, 머신 간 토론 공유는 코어 방식을 사용합니다.

---

## 고급 기능: A2A 작업 위임

tunaRound는 제한된 범위의 A2A 작업 브로커로도 동작합니다.

여기서 A2A는 "에이전트들이 사용자 없이 무한히 협업하는 swarm"이 아닙니다. 사용자가 목표를 정하고, 코어가 작업을 큐에 올리며, 워커 에이전트가 자기 앞으로 온 작업을 발견하고 처리하는 구조입니다.

### 호환 범위

tunaRound의 A2A 기능은 tunaRound 인스턴스끼리 작업을 위임하기 위한 내부 A2A-lite 구조입니다. A2A의 개념(Task lifecycle, Message, Artifact, Agent Card, JSON-RPC, SSE streaming)을 차용합니다.

호환은 방향으로 나눠 봐야 합니다.

- **Outbound (우리가 나감)**: `--runner a2a`로 외부 표준 A2A 에이전트에게 표준으로 위임하는 것은 지원·실증됐습니다(a2a-client 사용, 독립 표준 서버 상대 왕복 확인).
- **Inbound (남이 우리한테 던짐)**: 중앙 브로커 라우팅(`fromAgent`/`toAgent`)과 인증 게이트 카드가 있어, 임의의 제3자 표준 A2A 클라이언트가 우리한테 작업을 만드는 것은 비목표입니다(JSON-RPC envelope·`GetTask`는 호환, 카드 발견·`SendMessage`는 브로커 확장이라 미호환).

오픈소스라 우리 기능이 필요하면 레포를 쓰면 되고, 진짜 inbound 표준 게이트웨이가 필요해지면 별도 어댑터로 분리합니다.

### 동작 방식

- **코어 = 작업 큐 + A2A 서버**
  `serve`로 띄운 코어가 `/a2a`와 Agent Card를 노출합니다.
- **워커 = 자율 처리 데몬**
  `tunaround work`는 자기 앞으로 온 작업을 poll하고, claim하고, 지정한 runner로 실행한 뒤 complete 또는 failed로 전이합니다.
- **runner = 실제 실행 주체**
  같은 워커 데몬을 Claude, Codex, 로컬 LLM, OpenAI 호환 HTTP 모델, 외부 표준 A2A 에이전트(`--runner a2a`)로 실행할 수 있습니다.
- **SSE = 진행 상태 스트리밍**
  dispatcher는 `SendStreamingMessage`와 `SubscribeToTask`로 작업 상태 변화를 실시간으로 볼 수 있습니다.

코어 실행:

```bash
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>
```

Claude 워커 실행:

```bash
tunaround work --core http://<코어-IP>:8770/mcp --token <토큰> \
  --agent win-worker --runner claude
```

로컬 LLM 워커 실행:

```bash
tunaround work --core http://<코어-IP>:8770/mcp --token <토큰> \
  --agent llm-worker --runner http \
  --http-base-url http://127.0.0.1:11434 \
  --model qwen3.5:4b
```

자율 수준은 **semi-A2A**입니다. 사람이 목표를 정하고, 시스템은 발견·실행·완료·통지를 기계끼리 처리합니다. 사람 없이 무한히 도는 자동 토론 루프는 의도적으로 두지 않습니다.

자세한 사용법은 [`docs/reference/a2a-usage.md`](docs/reference/a2a-usage.md)를 참고합니다.

---

## 현재 상태

v1 본체와 v2 검색·맥락 기능이 대부분 들어왔습니다.

**토론과 실행**

- 역할을 나눈 다중 에이전트 토론
- Claude Code, Codex 기반 응답
- 특정 에이전트 지목
- 특정 에이전트에게 파일 수정 맡기기
- `/debate`를 통한 제한된 자동 토론
- 대화 분기와 체크아웃
- 세션 저장과 재개
- 긴 토론에서 오래된 발언을 요약해 다음 라운드로 넘기는 요약 이월

**검색과 맥락**

- SQLite + FTS5 검색
- 한국어 형태소 검색
- 외래어 병기 검색
- 의미 검색
- BM25 + 의미 검색 하이브리드
- 유효성과 최신성을 반영한 검색 랭킹
- `/reject`, `/supersede`, `/explain`
- MCP 기반 에이전트 직접 검색
- 에이전트 전사 읽기
- push 방식 대신 pull 방식 컨텍스트
- `--pull-context`
- `--recent-turns`

**분산 실행**

- Redis 기반 멀티세션 관찰
- 코어의 검색·전사를 HTTP MCP로 노출
- 원격 참가자 쓰기
- 로컬/원격 LLM 참가자
- Ollama, LM Studio, OpenAI 호환 HTTP 엔진
- OpenCode CLI 참가자

**A2A 작업 위임**

- `/a2a` 기반 작업 위임 브로커
- Agent Card
- `SendMessage`
- `GetTask`
- `CancelTask`
- `SendStreamingMessage`
- `SubscribeToTask`
- 자율 워커 데몬
- Claude, Codex, 로컬 LLM runner
- 외부 표준 A2A 에이전트 위임 (`--runner a2a`, outbound)
- 워커 실패 시 task `failed` 전이
- `context_id` 기반 프로젝트별 작업 라우팅
- `--context-map`
- 크로스머신 A2A 왕복과 SSE 스트리밍 스모크 확인

빌드는 macOS, Windows, Linux에서 순수 Rust로 됩니다.

Windows와 macOS aarch64에서 실제 `claude`, `codex` CLI로 동작을 확인했습니다. 빌드, 테스트, `cargo install`, 2에이전트 토론 도그푸딩, 크로스머신 A2A 읽기 스모크까지 확인했습니다.

macOS에서 Kiwi 네이티브 자동 다운로드가 막히면 lindera로 폴백해 그대로 동작합니다.

---

## 왜 만들었나

코드를 바로 짜기 전에 설계를 먼저 검토하고 싶을 때가 있습니다.

혼자 생각하면 놓치는 부분이 있고, 한 에이전트에게만 물어보면 답이 한 방향으로 굳기 쉽습니다. 그렇다고 여러 에이전트를 따로 켜고 복붙으로 조율하면 토론, 결정, 구현 지시가 쉽게 흩어집니다.

tunaRound는 이 과정을 터미널 안에서 반복 가능하게 만듭니다.

```
사용자가 방향을 잡는다.
  → 한 에이전트가 제안한다.
  → 다른 에이전트가 검토하거나 반박한다.
  → 결론을 문서로 남긴다.
  → 필요하면 작업으로 위임한다.
  → 결과를 다시 확인한다.
```

목표는 에이전트가 사람을 대체하는 것이 아니라, 사용자가 여러 에이전트를 더 안정적으로 부리는 것입니다.

## 어디서 가져왔나

tunaRound는 기존 프로젝트에서 검증한 기능들을 작게 묶은 도구입니다.

- 토론 흐름은 **tunapi**의 roundtable 구조에서 가져왔습니다.
- CLI 실행과 스트림 처리는 **tunaFlow**의 runner 경험을 바탕으로 했습니다.
- Redis 기반 세션 공유와 관찰 기능은 **tunaSalon**에서 가져왔습니다.
- 한국어 검색과 하이브리드 검색은 **seCall**의 경험을 옮겨왔습니다.

## 기술 스택

- Rust
- tokio
- JSON 세션 파일
- SQLite + FTS5
- Ollama 임베딩
- Redis
- MCP
- clap CLI
- cargo-dist

기본 UI는 가벼운 REPL입니다. TUI나 웹 UI는 이후 단계에서 붙일 예정입니다.

---

## 로드맵

**완료**

- [x] 여러 에이전트와 역할 설정
- [x] Redis 기반 세션 공유와 관찰
- [x] 에이전트에게 코드 수정 맡기기
- [x] `/debate` 자동 토론
- [x] 한국어 형태소 검색
- [x] Kiwi / lindera 폴백
- [x] SQLite + FTS5 검색
- [x] 과거 맥락 검색 주입
- [x] bge-m3 의미 검색
- [x] BM25 + 의미 검색 하이브리드
- [x] MCP `search_context` 도구
- [x] 가벼운 컨텍스트 주입
- [x] 로컬 LLM 참가자
- [x] OpenCode CLI 참가자
- [x] 요약 이월
- [x] 에이전트 전사 읽기 도구
- [x] push → pull 컨텍스트
- [x] 코어를 네트워크 HTTP MCP로 노출
- [x] 원격 참가자 쓰기
- [x] 유효성 인지 검색 랭킹
- [x] 최신성 인지 검색 랭킹
- [x] 외래어 병기 검색
- [x] Codex 전사 pull
- [x] 서브커맨드 CLI
- [x] `tunaround.toml` 프로파일
- [x] 배포 파이프라인 준비
- [x] 원격/분산 참가자 라이브
- [x] 맥 ↔ 윈도우 크로스머신 A2A 스모크
- [x] A2A 기반 작업 위임 브로커
- [x] A2A SSE 스트리밍
- [x] 자율 워커 데몬
- [x] 이기종 runner
- [x] 워커 실패 시 task `failed` 전이
- [x] `context_id` 기반 프로젝트별 작업 라우팅
- [x] outbound 표준 A2A 위임 (`--runner a2a`, 외부 표준 A2A 에이전트에 위임)
- [x] PR CI (linux·macOS·windows 3-OS 빌드·테스트·clippy 게이트) + GitHub Flow

**다음**

- [x] 공개 릴리스 (PUBLIC + SemVer + cargo-dist 릴리스)
- [x] 워커 고착 방지 (미배달/고착 표시 + write 워커 self-disruption 가드레일)
- [ ] claim 후 워커 사망 시 timeout / requeue
- [ ] 세션을 넘나드는 프로젝트 기억
- [ ] 리치 TUI
- [ ] 웹 UI

---

## 설계 문서

전체 설계는 아래 문서를 참고하세요.

- [`docs/design/tunaRound-v1-design_2026-06-29.md`](docs/design/tunaRound-v1-design_2026-06-29.md)
- [`docs/plans/index.md`](docs/plans/index.md)
- [`docs/development/source-run.md`](docs/development/source-run.md)

## 라이선스

[AGPL-3.0](LICENSE) - GNU Affero General Public License v3.0.
