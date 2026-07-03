소스에서 개발 실행하기

이 문서는 tunaRound를 릴리스 바이너리가 아니라 소스 코드에서 직접 빌드하고 실행하는 방법을 설명합니다.

일반 사용자는 README의 설치 방법을 사용하는 쪽이 낫습니다.
이 문서는 개발, 테스트, 디버깅, feature 조합 확인, 릴리스 전 도그푸딩을 위한 문서입니다.

요구사항

- Rust stable
- Cargo
- Git
- Claude Code CLI
- Codex CLI
- 선택: Redis
- 선택: Ollama
- 선택: Kiwi native library / model
- 선택: SQLite FTS5 지원 환경

기본 실행만 확인할 때는 Rust와 Cargo만 있으면 됩니다.
Claude/Codex 기반 실제 토론을 확인하려면 각 CLI가 설치되어 있고 인증까지 되어 있어야 합니다.

레포 받기

git clone https://github.com/hang-in/tunaround.git
cd tunaround

기본 빌드

cargo build

릴리스 빌드:

cargo build --release

빌드 결과는 다음 위치에 생깁니다.

target/debug/tunaround
target/release/tunaround

개발 모드 실행

인자 없이 실행하면 기본 REPL인 "chat"으로 들어갑니다.

cargo run

명시적으로 "chat"을 지정할 수도 있습니다.

cargo run -- chat

세션 파일을 지정해 실행합니다.

cargo run -- chat session.json

로스터 파일을 지정해 실행합니다.

cargo run -- chat --roster examples/roster.json

설치된 바이너리처럼 실행하기

개발 중에도 매번 "cargo run --"을 붙이기 싫다면 로컬 설치를 사용할 수 있습니다.

cargo install --path .

이후에는 일반 명령처럼 실행합니다.

tunaround chat

다시 빌드해 덮어쓰려면 같은 명령을 다시 실행합니다.

cargo install --path .

서브커맨드 확인

cargo run -- --help

각 명령의 옵션 확인:

cargo run -- chat --help
cargo run -- serve --help
cargo run -- join --help
cargo run -- work --help
cargo run -- reindex --help

로컬 설치 후에는 다음처럼 확인할 수 있습니다.

tunaround chat --help

테스트

전체 테스트:

cargo test

출력을 보면서 테스트:

cargo test -- --nocapture

특정 테스트만 실행:

cargo test <test_name>

feature를 켜고 테스트:

cargo test --features "semantic mcp"

기본 feature 실행

기본 기능만 실행합니다.

cargo run -- chat

SQLite DB를 지정합니다.

cargo run -- chat --db tuna.db

최근 N턴만 기본 컨텍스트로 넣습니다.

cargo run -- chat --db tuna.db --recent-turns 6

에이전트가 필요한 맥락을 직접 당겨오게 합니다.

cargo run -- chat --db tuna.db --pull-context

검색 feature 실행

의미 검색과 MCP를 함께 켭니다.

cargo run --features "semantic mcp" -- chat --db tuna.db

Ollama 임베딩 서버를 지정합니다.

export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11435
export TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b

cargo run --features "semantic mcp" -- chat --db tuna.db

모델을 바꾸려면 환경변수를 변경합니다.

export TUNAROUND_EMBED_MODEL=bge-m3
cargo run --features "semantic mcp" -- chat --db tuna.db

모델을 바꾸면 다음 색인 때 자동으로 재임베딩됩니다.

Redis 관찰 기능 실행

Redis URL을 지정합니다.

export TUNAROUND_REDIS_URL=redis://127.0.0.1:6379

세션 진행:

cargo run -- chat --session <id>

읽기 전용 관찰:

cargo run -- chat --observe <id>

Redis 기능은 여러 터미널에서 같은 세션을 보거나 관찰할 때 사용합니다.
머신 간 토론 공유는 "serve" / "join" 기반 코어 방식을 사용합니다.

코어 실행

코어를 띄웁니다.

cargo run -- serve 0.0.0.0:8770 --db shared.db --token <토큰>

다른 터미널이나 다른 머신에서 접속합니다.

cargo run -- join http://127.0.0.1:8770/mcp --token <토큰>

외부 머신에서 접속할 때는 "127.0.0.1" 대신 코어 머신의 IP나 도메인을 사용합니다.

cargo run -- join http://<코어-IP>:8770/mcp --token <토큰>

같은 공유기 안에서는 사설 IP를 사용할 수 있습니다.
외부에서는 Tailscale이나 SSH 터널을 사용할 수 있습니다.

A2A 작업 위임 개발 실행

코어를 작업 브로커로 띄웁니다.

cargo run -- serve 0.0.0.0:8770 --db shared.db --token <토큰>

Claude 워커:

cargo run -- work --core http://127.0.0.1:8770/mcp --token <토큰> \
  --agent claude-worker --runner claude

Codex 워커:

cargo run -- work --core http://127.0.0.1:8770/mcp --token <토큰> \
  --agent codex-worker --runner codex

HTTP 기반 로컬 LLM 워커:

cargo run -- work --core http://127.0.0.1:8770/mcp --token <토큰> \
  --agent llm-worker --runner http \
  --http-base-url http://127.0.0.1:11434 \
  --model qwen3.5:4b

A2A 관련 상세 사용법은 다음 문서를 봅니다.

docs/reference/a2a-usage.md

설정 프로파일 개발 확인

레포 루트에 "tunaround.toml"을 둡니다.

default_profile = "local"

[profile.local]
db = "~/.tunaround/local.db"
pull_context = false

[profile.dev]
db = "./dev-tuna.db"
roster = "examples/roster.json"
pull_context = true

프로파일로 실행합니다.

cargo run -- chat --profile dev

설정 파일을 직접 지정합니다.

cargo run -- chat --config ./tunaround.toml --profile dev

값 우선순위는 다음과 같습니다.

CLI 플래그 > 선택된 프로파일 > 기본값

색인 재생성

기본 재색인:

cargo run -- reindex --db tuna.db

feature 조합을 켜고 재색인:

cargo run --features "semantic mcp" -- reindex --db tuna.db

로그 확인

Rust 로그를 켭니다.

RUST_LOG=debug cargo run -- chat

더 자세히 봅니다.

RUST_LOG=trace cargo run -- chat

특정 모듈만 보고 싶으면 모듈 경로를 지정합니다.

RUST_LOG=tunaround=debug cargo run -- chat

Windows PowerShell에서는 다음처럼 지정합니다.

$env:RUST_LOG="debug"
cargo run -- chat

포맷과 린트

포맷 확인:

cargo fmt -- --check

포맷 적용:

cargo fmt

Clippy:

cargo clippy --all-targets --all-features

경고를 에러로 봅니다.

cargo clippy --all-targets --all-features -- -D warnings

릴리스 빌드 확인

cargo build --release

릴리스 바이너리 실행:

./target/release/tunaround chat

Windows PowerShell:

.\target\release\tunaround.exe chat

cargo-dist 확인

cargo-dist 설정을 확인합니다.

cargo dist plan

릴리스 산출물 생성을 확인합니다.

cargo dist build

실제 배포 전에는 태그, GitHub Actions, Homebrew tap, PowerShell 설치 스크립트 URL을 함께 확인해야 합니다.

자주 겪는 문제

"claude" 또는 "codex"를 찾지 못함

CLI가 설치되어 있는지 확인합니다.

which claude
which codex

Windows PowerShell:

Get-Command claude
Get-Command codex

인증도 미리 끝나 있어야 합니다.

Ollama 임베딩 서버에 연결되지 않음

Ollama 서버 주소를 확인합니다.

echo $TUNAROUND_OLLAMA_URL

기본값은 다음입니다.

http://127.0.0.1:11435

사용 중인 Ollama 서버가 "11434"라면 환경변수를 바꿉니다.

export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11434

Kiwi 다운로드 실패

Kiwi 자동 다운로드가 막혀도 lindera로 폴백합니다.
Kiwi를 꼭 써야 한다면 다음 환경변수를 지정합니다.

export KIWI_RS_VERSION=<version>
export KIWI_LIBRARY_PATH=<path>

원격 코어에 접속되지 않음

확인할 것:

- 코어가 "0.0.0.0:<port>"로 떠 있는가
- 방화벽에서 포트가 열려 있는가
- 토큰이 같은가
- URL 경로가 "/mcp"까지 포함되어 있는가
- 같은 네트워크가 아니라면 Tailscale이나 SSH 터널이 연결되어 있는가

예시:

cargo run -- join http://<코어-IP>:8770/mcp --token <토큰>

워커가 작업을 가져오지 않음

확인할 것:

- "--core" 주소가 맞는가
- "--token"이 맞는가
- "--agent" 이름과 작업의 target agent가 맞는가
- context 라우팅을 쓰는 경우 "--context-map"이 맞는가
- runner CLI가 설치되어 있는가

개발 실행 원칙

README의 기본 경로는 릴리스 바이너리 기준입니다.

tunaround chat

개발 문서에서만 "cargo run"을 사용합니다.

cargo run -- chat

이 구분을 유지해야 README가 사용자 문서처럼 보이고, 개발 실행법은 필요한 사람만 볼 수 있습니다.