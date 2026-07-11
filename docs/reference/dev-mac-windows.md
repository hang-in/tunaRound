# macOS와 Windows에서 함께 운용하기

이 문서는 macOS와 Windows 두 머신에서 tunaRound를 개발하거나 mesh로 함께 운용할 때 필요한 차이만 정리합니다.

설치와 기본 온보딩은 [온보딩 가이드](onboarding.md), 작업 위임은 [A2A 사용법](a2a-usage.md)을 먼저 참고하세요.

## 1. 가장 중요한 원칙

- 한 레포를 두 머신에서 개발할 때는 이동 전 push, 이동 후 pull을 기본으로 합니다.
- 같은 브랜치를 두 머신에서 동시에 편집하지 않습니다.
- 병렬 작업은 브랜치와 worktree 또는 별도 클론으로 분리합니다.
- 토큰, 사설 IP, 호스트 이름은 레포 문서에 기록하지 않습니다.
- 코어와 워커가 읽는 토큰은 `TUNA_BROKER_TOKEN`으로 통일합니다.

## 2. 공통 준비

두 머신 모두 다음 도구를 준비합니다.

- Git
- Rust stable과 Cargo
- 사용할 에이전트 CLI와 로그인 상태
- 선택적으로 Ollama 또는 다른 임베딩 서버

레포를 받습니다.

```bash
git clone https://github.com/hang-in/tunaRound.git
cd tunaRound
cargo test
```

기본 REPL 실행은 두 플랫폼에서 같습니다.

```bash
cargo run -- chat --db tuna.db
```

## 3. 권장 구성

두 머신을 함께 쓸 때는 한 머신을 코어로 정하고 다른 머신을 워커 또는 원격 REPL로 붙이는 구성이 단순합니다.

```text
Windows 또는 macOS
└─ 코어: serve + SQLite + 대시보드

다른 머신
├─ 원격 REPL: join
├─ 헤드리스 워커: node 또는 work
└─ 라이브 세션 등록: presence-scan + 필요 시 codex-relay
```

어느 운영체제가 코어를 맡아도 구조는 같습니다. 항상 켜 두기 쉬운 머신을 코어로 정하세요.

## 4. 코어 머신

코어를 실행합니다.

```bash
tunaround serve 0.0.0.0:8770 \
  --db ~/.tunaround/broker.db \
  --token <토큰>
```

다른 머신에서 접근할 수 있도록 다음 항목을 확인합니다.

1. 코어가 `0.0.0.0:<port>`에 바인드되었는가
2. 운영체제 방화벽이 해당 포트를 허용하는가
3. 양쪽 머신이 같은 LAN이나 Tailscale 네트워크에 있는가
4. 토큰이 같은가

대시보드를 사용하려면 소스에서 프런트엔드를 빌드해야 합니다. 자세한 과정은 [소스 빌드 안내](../development/source-run.md#5-웹-대시보드-빌드)를 참고하세요.

## 5. 합류하는 머신

가장 간단한 원격 REPL 접속입니다.

```bash
tunaround join http://<코어-IP>:8770/mcp --token <토큰>
```

워커 노드로 상주시킵니다.

```bash
tunaround init --core http://<코어-IP>:8770/mcp --machine mac
# 또는 --machine win

tunaround doctor
tunaround node
```

`init`이 만든 `~/.tunaround/config`에서 다음 값을 확인합니다.

```dotenv
TUNA_BROKER_CORE=http://<코어-IP>:8770/mcp
TUNA_BROKER_TOKEN=<토큰>
TUNA_MACHINE=mac
```

Windows에서는 `TUNA_MACHINE=win`을 사용합니다.

## 6. 플랫폼별 차이

### 6.1 경로

홈 디렉터리 기준 경로를 사용하면 설정을 두 플랫폼에서 비슷하게 유지할 수 있습니다.

```text
~/.tunaround/broker.db
~/.tunaround/node.toml
~/.tunaround/config
```

`tunaround.toml`과 `node.toml`은 `~/`를 홈 디렉터리로 확장합니다. 셸 스크립트나 외부 도구에서는 플랫폼별 경로 처리가 다를 수 있으므로 확인이 필요합니다.

### 6.2 실행 파일

macOS와 Linux에서는 보통 확장자 없는 실행 파일을 사용합니다.

```bash
./target/release/tunaround
```

Windows에서는 `.exe`, CLI 래퍼는 `.cmd`일 수 있습니다.

```powershell
.\target\release\tunaround.exe
Get-Command claude
Get-Command codex
```

Rust 러너는 Windows의 `.cmd`와 `.bat` 실행을 처리하지만, 셸 스크립트 예시는 Git Bash 또는 WSL이 더 편할 수 있습니다.

### 6.3 환경변수

macOS의 zsh 예시입니다.

```bash
export TUNA_BROKER_TOKEN=<토큰>
export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11434
```

PowerShell 예시입니다.

```powershell
$env:TUNA_BROKER_TOKEN="<토큰>"
$env:TUNAROUND_OLLAMA_URL="http://127.0.0.1:11434"
```

환경변수를 영구 변경해도 이미 실행 중인 데몬에는 반영되지 않습니다. 토큰이나 서버 주소를 바꾸면 관련 프로세스를 재기동하세요.

### 6.4 줄바꿈

레포 파일은 LF를 기준으로 유지합니다. Windows에서 대량 줄바꿈 diff가 생기면 다음 설정을 확인합니다.

```bash
git config core.autocrlf
```

단순 문서 수정에서 파일 전체가 변경된 것처럼 보이면 커밋하기 전에 중단하고 줄바꿈을 먼저 바로잡습니다.

### 6.5 Kiwi 형태소 분석기

Kiwi 네이티브 라이브러리나 모델을 불러오지 못하면 lindera로 자동 폴백합니다. 검색 자체는 계속 동작합니다.

Windows에서는 자동 다운로드가 불안정할 수 있으므로 [Windows Kiwi 설정](kiwi-windows-setup.md)을 참고하세요. macOS에서는 자동 다운로드가 실패하면 로그를 확인하고, 필요한 경우 `KIWI_LIBRARY_PATH`를 지정합니다.

## 7. 라이브 세션 등록

각 머신에서 `presence-scan`을 하나씩 실행하면 해당 머신의 Claude Code와 Codex 세션이 공용 로스터에 나타납니다.

```bash
tunaround presence-scan --machine mac
```

Windows에서는 다음과 같이 실행합니다.

```powershell
tunaround presence-scan --machine win
```

스캐너는 세션 존재만 보고합니다. 실제 작업 수신은 별도입니다.

- Claude 세션은 Monitor 또는 task CLI로 수신합니다.
- Codex 라이브 세션은 `codex app-server`와 `tunaround codex-relay`가 필요합니다.

세부 설정은 [A2A 사용법의 라이브 세션 절](a2a-usage.md#4-라이브-세션을-mesh에-연결하기)을 참고하세요.

## 8. Codex 세션을 다른 머신에서 관전하기

Codex app-server는 기본적으로 localhost에 바인드합니다. 다른 머신에서 같은 thread를 열어 볼 때만 SSH 포트 포워딩을 사용합니다.

```bash
ssh -L <로컬-포트>:127.0.0.1:<원격-포트> <원격-머신>
```

포워딩된 주소로 접속합니다.

```bash
codex resume <threadId> --remote ws://127.0.0.1:<로컬-포트>
```

이 포워딩은 원격 관전용입니다. 일반적인 작업 위임과 결과 회수는 코어 브로커를 통하므로 Codex WebSocket을 다른 머신에 직접 공개할 필요가 없습니다.

## 9. 검색 서버 공유

임베딩 서버는 각 머신에서 따로 실행해도 되고 한 머신의 서버를 네트워크로 공유해도 됩니다.

```bash
export TUNAROUND_OLLAMA_URL=http://<임베딩-서버-IP>:11434
export TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b
```

외부 네트워크에 Ollama 포트를 직접 공개하지 마세요. LAN, Tailscale 또는 SSH 터널 안에서만 접근하도록 구성합니다.

## 10. 개발 검증

두 플랫폼 모두 기본 검증은 같습니다.

```bash
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
```

한 플랫폼에서만 통과했다고 끝내지 말고, 플랫폼 의존 변경은 GitHub Actions의 macOS와 Windows 결과까지 확인합니다.

특히 다음 변경은 양쪽 확인이 필요합니다.

- 프로세스 실행과 종료
- 경로와 홈 디렉터리 처리
- `.cmd`, `.bat`, `.exe` 탐색
- 파일 잠금과 SQLite
- 네이티브 라이브러리 로딩

## 11. 연결이 안 될 때

| 증상 | 먼저 확인할 것 |
| --- | --- |
| `join` 실패 | 코어 주소의 `/mcp`, 토큰, 방화벽 |
| 워커가 안 보임 | `node` 또는 `work` 실행 여부, `doctor` 결과 |
| 세션이 로스터에 안 보임 | 각 머신의 `presence-scan` 실행 여부 |
| Codex task가 전달되지 않음 | app-server thread와 `codex-relay` 상태 |
| 의미 검색 실패 | `TUNAROUND_OLLAMA_URL`, 서버 포트, 모델 존재 여부 |
| 토큰 변경 후 인증 실패 | 모든 코어·워커·relay 프로세스 재기동 |

실제 IP, 토큰, 호스트 별칭은 각 머신의 로컬 설정에서 관리하고 문서나 커밋에 포함하지 않습니다.
