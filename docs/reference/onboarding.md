# 온보딩 가이드

이 문서는 tunaRound를 처음 설치하거나 새 머신을 연결할 때 보는 안내서입니다.

먼저 자신이 하려는 일을 고른 뒤 해당 절차만 따라가면 됩니다.

| 하려는 일 | 필요한 구성 | 시작 명령 |
| --- | --- | --- |
| 처음 설치해서 로컬 왕복 확인(첫 성공) | 로컬 브로커·워커 | `init → node` |
| 한 머신에서 토론과 검색 | 로컬 REPL | `tunaround chat` |
| 한 머신을 공유 코어로 사용 | 코어·브로커 | `tunaround serve` |
| 다른 머신에서 원격으로 접속 | 원격 REPL | `tunaround join` |
| 다른 머신을 작업 노드로 상주시킴 | 워커 노드 | `init → doctor → node` |

전체 구조가 먼저 궁금하다면 [mesh 아키텍처](mesh-architecture.md)를, 실제 작업 위임 명령이 필요하다면 [A2A 작업 위임 사용법](a2a-usage.md)을 참고하세요.

## 1. 설치

설치 방법은 [README의 시작하기 절](../../README.md#시작하기)에 정리되어 있습니다.

가장 간단한 선택은 릴리스 바이너리입니다. 직접 빌드할 때만 Cargo 피처를 신경 쓰면 됩니다.

| 빌드 방식 | 포함되는 기능 |
| --- | --- |
| 기본 `cargo build` | REPL, SQLite, 형태소 검색 |
| 릴리스 바이너리 | 코어, 원격 접속, 워커, 의미 검색, HTTP 러너 |
| `dashboard` 피처 빌드 | 웹 대시보드 SPA 포함 |

컴파일 피처는 실행 옵션이 아닙니다.

```bash
# 잘못된 예
tunaround chat --features semantic

# 올바른 예
cargo run --features "semantic mcp" -- chat --db tuna.db
```

대시보드까지 빌드하려면 다음 순서로 실행합니다.

```bash
cd frontend
npm install
npm run build
cd ..
cargo build --release --features dashboard
```

소스 실행과 피처 조합은 [소스 빌드 가이드](../development/source-run.md)에 더 자세히 설명되어 있습니다.

## 2. 로컬 첫 왕복 (첫 성공)

신규 머신에서 가장 빠른 검증 경로입니다: 총감독 Claude Code 세션에서 로컬 Codex에게 위임하고 응답을 받는 것. 여기까지는 토큰·LAN 설정이 필요 없습니다.

```bash
tunaround init    # 설정 스캐폴드 + 러너 lane 전수 탐지 + Claude Code에 tuna-broker MCP 자동 등록
tunaround node    # 브로커(in-process) + 워커 lane 상주
```

- `init` 기본은 로컬 전용입니다(listen `127.0.0.1:8770`, 무토큰 계약). 토큰 없이 동작하고 LAN에 노출되지 않습니다. LAN 공유가 필요해지면 "다른 머신에서 접속하기" 절을 따르세요(`--listen 0.0.0.0:8770` 지정 시 토큰 안내가 다시 나타납니다).
- `init`이 PATH의 claude·codex·opencode를 전부 lane으로 스캐폴드합니다(예: `codex-worker`). claude CLI가 있으면 tuna-broker MCP를 user scope로 자동 등록하고, 이미 등록돼 있으면 보존합니다(`--no-mcp-register`로 옵트아웃).
- **Claude Code 재시작(새 세션) 필수**: 새로 등록된 MCP 서버는 이미 실행 중인 세션에 로드되지 않습니다.
- 재시작한 세션에서 자연어로 위임하면 Claude가 tuna-broker 도구로 왕복합니다. 수동으로 확인하려면 `send_task(from_agent="me", to_agent="codex-worker", text="...")` 후 `get_task(task_id)`를 반복 조회하세요 - `state=completed`면 같은 응답에 결과 전문이 옵니다(5분 미소비면 `⚠no-consumer?` 표시로 수신측 부재를 알 수 있습니다).
- 안 되면 `tunaround doctor`: 러너 PATH, 코어 도달, 설정을 점검합니다.

## 3. 한 머신에서 사용하기

브로커나 워커가 필요하지 않은 가장 단순한 구성입니다.

1. 사용할 CLI 에이전트를 설치하고 로그인합니다.
2. tunaRound를 실행합니다.
3. 대화 저장과 검색이 필요하면 DB를 지정합니다.

```bash
tunaround chat

# 대화 저장과 검색 포함
tunaround chat --db tuna.db
```

Claude Code나 Codex를 사용하지 않고 HTTP 기반 LLM 러너만 쓸 수도 있습니다.

의미 검색은 `semantic` 피처와 Ollama 호환 임베딩 서버가 있을 때만 동작합니다. 설정은 [검색 설정](#7-검색-설정)을 참고하세요.

## 4. 코어와 대시보드 띄우기

한 머신을 공유 코어로 정하면 다른 머신이 이 코어에 접속할 수 있습니다.

```bash
tunaround serve 0.0.0.0:8770 \
  --db shared.db \
  --token <토큰>
```

이 프로세스가 다음 상태를 보관합니다.

- 공유 전사
- 검색 색인
- A2A 작업 큐
- 세션과 워커 로스터
- 대시보드 이벤트

대시보드는 `/dashboard`에서 열립니다. 릴리스 바이너리는 대시보드 SPA를 포함하지 않으므로, 실제 화면이 필요하면 `dashboard` 피처로 소스 빌드해야 합니다.

토큰은 명령행 또는 `TUNA_BROKER_TOKEN` 환경변수로 전달할 수 있습니다. 실제 토큰은 문서, 저장소, 셸 기록에 남기지 않는 편이 안전합니다.

## 5. 다른 머신에서 접속하기

### 원격 REPL로 접속

다른 머신에서 기존 코어의 대화와 검색 기능을 사용할 때는 `join`을 씁니다.

```bash
tunaround join http://<코어-IP>:8770/mcp --token <토큰>
```

### 워커 노드로 상주

해당 머신이 작업을 자동으로 받아 처리하게 하려면 다음 순서로 설정합니다.

```bash
tunaround init \
  --core http://<코어-IP>:8770/mcp \
  --machine mac

tunaround doctor
tunaround node
```

각 명령의 역할은 다음과 같습니다.

| 명령 | 역할 |
| --- | --- |
| `init` | `node.toml`과 mesh 설정의 초안을 만듭니다. |
| `doctor` | 코어 연결, 토큰, 러너, 경로, 피처를 점검합니다. |
| `node` | 설정에 따라 브로커와 워커를 상주시킵니다. |

저수준 워커만 직접 실행할 수도 있습니다.

```bash
tunaround work \
  --core http://<코어-IP>:8770/mcp \
  --token <토큰> \
  --agent mac-worker \
  --runner claude
```

`doctor`가 성공해도 토큰이 비어 있으면 `node`는 인증에 실패할 수 있습니다. `doctor`의 WARN도 함께 확인해야 합니다.

## 6. 설정 파일

설정 파일은 세 종류지만 적용 대상이 다릅니다.

| 파일 | 사용하는 명령 | 역할 |
| --- | --- | --- |
| `tunaround.toml` | `chat`, `core`, `join` | 세션과 검색 프로파일 |
| `node.toml` | `init`, `doctor`, `node` | 워커 노드와 lane 구성 |
| `~/.tunaround/config` | mesh 스크립트와 훅 | 공통 `TUNA_*` 환경값 |

### `tunaround.toml`

세션 DB, 로스터, 검색 서버 같은 사용자 실행 설정을 담습니다.

조회 순서는 다음과 같습니다.

```text
--config
→ ./tunaround.toml
→ ~/.config/tunaround/config.toml
```

### `node.toml`

워커 노드가 어떤 코어에 연결되고 어떤 러너를 사용할지 정의합니다.

조회 순서는 다음과 같습니다.

```text
--config
→ ./tunaround.node.toml
→ ~/.tunaround/node.toml
```

### `~/.tunaround/config`

SessionStart 훅과 mesh 재기동 스크립트가 읽는 dotenv 형식의 파일입니다. `TUNA_BROKER_CORE`, `TUNA_BROKER_TOKEN`, `TUNA_MACHINE` 같은 값을 둡니다.

보통은 `tunaround init`이 `node.toml`과 `~/.tunaround/config` 초안을 함께 만들기 때문에, 최초 설정에서는 코어 주소와 토큰만 확인하면 됩니다.

## 7. 검색 설정

### 한국어 형태소 검색

Kiwi를 우선 사용하고, 초기화에 실패하면 lindera로 폴백합니다.

Windows에서 자동 설치가 불안정하면 다음 스크립트로 미리 설치합니다.

```bash
scripts/install-kiwi-windows.sh
```

자세한 내용은 [Windows Kiwi 설정](kiwi-windows-setup.md)을 참고하세요.

### 의미 검색

의미 검색에는 `semantic` 피처와 Ollama 호환 임베딩 서버가 필요합니다.

```bash
export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11435
export TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b
```

모델을 바꾸면 이후 재색인 과정에서 새 모델 기준으로 임베딩을 다시 만듭니다.

## 8. 토큰을 바꾼 뒤 해야 할 일

실행 중인 데몬은 시작할 때 읽은 환경변수를 계속 사용합니다. 토큰을 변경해도 이미 떠 있는 프로세스에는 자동으로 반영되지 않습니다.

토큰을 교체한 뒤에는 다음 프로세스를 모두 재기동합니다.

- `serve`
- `node`
- `work`
- presence 스캐너와 relay 같은 mesh 데몬

새 셸을 열거나 `setx`를 실행하는 것만으로 기존 프로세스의 토큰이 바뀌지는 않습니다.

## 9. 네트워크와 보안

같은 LAN에서는 코어 머신의 사설 IP를 사용할 수 있습니다.

외부 네트워크에서 연결할 때는 코어 포트를 인터넷에 그대로 공개하지 말고 Tailscale, WireGuard, SSH 터널 같은 별도 보안 경로를 사용합니다.

문서와 저장소에는 다음처럼 플레이스홀더만 남깁니다.

```text
<코어-IP>
<토큰>
@env:TUNA_BROKER_TOKEN
```

## 10. 다음에 읽을 문서

- 구조와 역할: [mesh 아키텍처](mesh-architecture.md)
- 작업 보내기와 받기: [A2A 작업 위임 사용법](a2a-usage.md)
- macOS와 Windows 구성: [dev-mac-windows](dev-mac-windows.md)
- 소스 빌드: [source-run](../development/source-run.md)