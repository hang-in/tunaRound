# 소스에서 빌드하고 실행하기

이 문서는 tunaRound를 릴리스 바이너리가 아니라 소스 코드에서 직접 빌드하고 실행하는 개발자용 안내서입니다.

일반 사용자는 [README의 설치 방법](../../README.md#설치)을 사용하는 편이 간단합니다. 이 문서는 기능 개발, 테스트, 디버깅, 피처 조합 확인, 릴리스 전 검증을 다룹니다.

## 1. 준비물

기본 빌드에는 다음 도구가 필요합니다.

- Rust stable과 Cargo
- Git

실제 에이전트 실행까지 확인하려면 해당 머신에 Claude Code, Codex 또는 OpenCode CLI가 설치되고 로그인되어 있어야 합니다.

기능에 따라 다음 항목이 추가로 필요합니다.

| 기능 | 추가 요구사항 |
| --- | --- |
| 의미 검색 | Ollama 또는 호환 임베딩 서버 |
| 웹 대시보드 | Node.js와 npm |
| HTTP 러너 | OpenAI 호환 LLM 서버 |
| 외부 A2A 위임 | 접근 가능한 표준 A2A 에이전트 |

Redis는 더 이상 사용하지 않습니다. 세션, 작업 장부, 검색 색인은 SQLite를 사용합니다.

## 2. 레포 받기

```bash
git clone https://github.com/hang-in/tunaRound.git
cd tunaRound
```

## 3. 가장 빠른 개발 실행

기본 빌드와 테스트를 확인합니다.

```bash
cargo build
cargo test
```

기본 REPL을 실행합니다.

```bash
cargo run -- chat
```

전사 저장과 검색이 필요하면 DB를 지정합니다.

```bash
cargo run -- chat --db tuna.db
```

개발 중 `cargo run --`을 반복하고 싶지 않다면 로컬에 설치합니다.

```bash
cargo install --path .
tunaround chat
```

코드를 바꾼 뒤에는 같은 설치 명령을 다시 실행하면 덮어씁니다.

## 4. Cargo 피처

일부 명령은 컴파일 피처를 켜야 나타납니다.

| 목적 | 예시 |
| --- | --- |
| 기본 REPL과 SQLite 검색 | `cargo run -- chat --db tuna.db` |
| 의미 검색과 MCP | `cargo run --features "semantic mcp" -- chat --db tuna.db` |
| 코어 서버 | `cargo run --features serve -- serve 127.0.0.1:8770 --db shared.db` |
| 워커와 HTTP 러너 | `cargo run --features "serve worker engines" -- work ...` |
| 외부 A2A 러너 | `cargo run --features "serve worker a2a-out" -- work ...` |
| 웹 대시보드 포함 | 프런트엔드 빌드 후 `cargo build --features dashboard` |

피처는 Cargo에 전달하고, tunaRound 인자는 `--` 뒤에 둡니다.

```bash
cargo run --features "semantic mcp" -- chat --db tuna.db
```

다음처럼 런타임 인자로 쓰면 동작하지 않습니다.

```text
tunaround chat --features semantic
```

현재 지원하는 피처와 서브커맨드는 다음 명령으로 확인합니다.

```bash
cargo run -- --help
cargo run --features "serve worker engines" -- --help
```

## 5. 웹 대시보드 빌드

릴리스 바이너리에는 대시보드 SPA가 포함되지 않습니다. 소스 빌드에서는 먼저 프런트엔드를 만든 뒤 Rust 바이너리에 임베드합니다.

```bash
cd frontend
npm install
npm run build
cd ..

cargo build --release --features dashboard
```

실행합니다.

```bash
./target/release/tunaround serve 0.0.0.0:8770 \
  --db shared.db \
  --token <토큰>
```

Windows에서는 다음 경로를 사용합니다.

```powershell
.\target\release\tunaround.exe serve 0.0.0.0:8770 --db shared.db --token <토큰>
```

대시보드는 `/dashboard`에서 열립니다.

## 6. 검색 개발 실행

### 형태소 검색

기본 빌드에는 SQLite와 형태소 검색이 포함됩니다.

```bash
cargo run -- chat --db tuna.db
```

Kiwi를 불러오지 못하면 lindera로 폴백합니다. Windows에서 Kiwi를 미리 설치하려면 `scripts/install-kiwi-windows.sh`와 [Windows Kiwi 안내](../reference/kiwi-windows-setup.md)를 참고하세요.

### 의미 검색

Ollama 주소와 임베딩 모델을 지정합니다.

```bash
export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11434
export TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b

cargo run --features "semantic mcp" -- chat --db tuna.db
```

모델을 바꾸면 다음 색인 과정에서 필요한 항목을 다시 임베딩합니다.

색인을 전체 재생성하려면 다음 명령을 사용합니다.

```bash
cargo run --features "semantic mcp" -- reindex --db tuna.db
```

## 7. 코어와 원격 REPL 실행

코어를 띄웁니다.

```bash
cargo run --features serve -- serve 0.0.0.0:8770 \
  --db shared.db \
  --token <토큰>
```

다른 터미널이나 머신에서 접속합니다.

```bash
cargo run -- join http://<코어-IP>:8770/mcp --token <토큰>
```

같은 머신에서는 `<코어-IP>` 대신 `127.0.0.1`을 사용할 수 있습니다. 외부 네트워크에서는 코어 포트를 그대로 공개하지 말고 Tailscale이나 SSH 터널을 사용하세요.

## 8. A2A 워커 실행

Claude 워커 예시입니다.

```bash
cargo run --features "serve worker" -- work \
  --core http://127.0.0.1:8770/mcp \
  --token <토큰> \
  --agent claude-worker \
  --runner claude
```

HTTP 기반 로컬 LLM 워커 예시입니다.

```bash
cargo run --features "serve worker engines" -- work \
  --core http://127.0.0.1:8770/mcp \
  --token <토큰> \
  --agent llm-worker \
  --runner http \
  --http-base-url http://127.0.0.1:11434 \
  --model qwen3.5:4b
```

작업 등록, 태그 라우팅, SSE, Codex relay는 [A2A 작업 위임 사용법](../reference/a2a-usage.md)을 참고하세요.

## 9. 설정 프로파일

레포 루트의 `tunaround.toml.example`을 복사해 프로파일을 만들 수 있습니다.

```toml
default_profile = "local"

[profile.local]
db = "~/.tunaround/local.db"
pull_context = false

[profile.dev]
db = "./dev-tuna.db"
roster = "examples/roster.json"
pull_context = true
```

프로파일로 실행합니다.

```bash
cargo run -- chat --profile dev
```

설정 파일을 직접 지정할 수도 있습니다.

```bash
cargo run -- chat --config ./tunaround.toml --profile dev
```

값의 우선순위는 다음과 같습니다.

```text
CLI 플래그 > 선택한 프로파일 > 기본값
```

## 10. 테스트와 정적 검사

변경 범위에 맞는 검증부터 실행하고, 완료 전에는 전체 기본 검증을 통과시킵니다.

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

특정 피처만 확인할 수 있습니다.

```bash
cargo test --features "semantic mcp serve worker engines"
cargo clippy --features "semantic mcp serve worker engines"
```

특정 테스트만 실행합니다.

```bash
cargo test <test_name> -- --nocapture
```

외부 서버가 필요한 수동 테스트는 `#[ignore]`로 분리되어 있을 수 있습니다.

## 11. 로그와 디버깅

전체 디버그 로그를 켭니다.

```bash
RUST_LOG=debug cargo run -- chat
```

특정 크레이트 로그만 봅니다.

```bash
RUST_LOG=tunaround=debug cargo run -- chat
```

PowerShell에서는 환경변수를 먼저 지정합니다.

```powershell
$env:RUST_LOG="debug"
cargo run -- chat
```

## 12. 릴리스 빌드 확인

```bash
cargo build --release
./target/release/tunaround --help
```

cargo-dist 계획을 확인합니다.

```bash
dist plan
```

실제 릴리스 절차와 버전 규칙은 [버전 관리 정책](../reference/versioning.md)을 따릅니다.

## 13. 자주 겪는 문제

### Claude 또는 Codex를 찾지 못함

```bash
which claude
which codex
```

PowerShell에서는 다음 명령을 사용합니다.

```powershell
Get-Command claude
Get-Command codex
```

CLI 설치뿐 아니라 로그인도 완료되어야 합니다.

### 의미 검색 서버에 연결되지 않음

```bash
echo $TUNAROUND_OLLAMA_URL
```

실제 Ollama 포트가 `11434`인지 확인하세요. tunaRound는 서버가 없으면 형태소 FTS만 사용합니다.

### 원격 코어에 연결되지 않음

다음을 확인합니다.

1. 코어가 `0.0.0.0:<port>` 또는 접근 가능한 주소로 바인드되었는가
2. 방화벽에서 해당 포트를 허용했는가
3. 양쪽 토큰이 같은가
4. URL이 `/mcp`까지 포함하는가
5. 서로 다른 네트워크라면 터널이 연결되어 있는가

### 워커가 작업을 가져오지 않음

다음을 확인합니다.

1. `--core`와 `--token`이 맞는가
2. `--agent`가 작업의 대상과 같은가
3. 태그 셀렉터에 매칭되는 온라인 워커가 있는가
4. 러너 CLI가 설치되고 로그인되었는가
5. `--project-path` 또는 `--context-map`이 올바른가

상세 장애 진단은 [A2A 작업 위임 사용법](../reference/a2a-usage.md#5-운영과-장애-진단)을 참고하세요.
