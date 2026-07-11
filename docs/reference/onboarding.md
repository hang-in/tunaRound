# 온보딩 가이드

> tunaRound를 처음 세우는 세 갈래(로컬 1인 / 브로커·대시보드 호스팅 / 머신 합류)와, 알아두면 시간을 아끼는 함정 세 가지. 공개 레포이므로 토큰·IP·호스트는 전부 플레이스홀더(`<토큰>`·`<코어-IP>`·`@env:NAME`)로 적고 실제 값은 각 머신의 env·로컬 설정에만 둡니다.

> 가장 쉬운 길: AI에게 맡기기. [install-with-ai](../prompts/install-with-ai.md)의 프롬프트를 새 머신의 Claude Code/Codex에 붙여넣으면 아래 절차를 AI가 대신 수행합니다. 직접 하려면 계속 읽으세요.

## 0. 어떤 갈래인지 먼저 고르기

| 하려는 것 | 갈래 | 핵심 명령 |
| --- | --- | --- |
| 혼자 설계 토론·검색만 | 로컬 1인 | `tunaround chat` |
| 브로커 + 웹 대시보드 호스팅 | 호스팅(예: 윈도우) | `tunaround serve` (대시보드는 소스 빌드) |
| 다른 머신을 mesh에 합류 | 합류(예: 맥) | `tunaround join` 또는 `init` → `doctor` → `node` |

mesh 전체 구조와 역할은 [mesh 아키텍처](mesh-architecture.md), 작업 위임 명령 흐름은 [a2a-usage](a2a-usage.md)를 참고하세요.

## 1. 설치와 피처 (첫 번째 함정)

설치 채널은 README [설치](../../README.md#설치)에 있습니다(shell/powershell/homebrew/cargo). 현재 버전은 `0.3.0`, crates.io에는 게시하지 않습니다.

**피처가 무엇을 켜는지 알아야 합니다.** 서브커맨드는 컴파일 피처로 게이트됩니다. 피처가 없으면 그 서브커맨드는 아예 존재하지 않습니다(`node`/`doctor`/`serve`가 "command not found"로 보이는 흔한 원인).

| 빌드 | 켜지는 것 |
| --- | --- |
| 기본 `cargo build` | `default = ["morphology","sqlite"]` = REPL + 형태소 FTS 검색. `serve`/`work`/`node`/`doctor` 없음. |
| 릴리스 바이너리(shell/ps/brew) | `semantic mcp serve worker engines a2a-out` = 코어·워커·원격까지. **`dashboard`는 없음**(대시보드는 플레이스홀더 페이지). |
| 대시보드 필요 시 | `cd frontend && npm run build` 후 `cargo build --release --features dashboard`. |

`--features`는 **컴파일 타임 cargo 플래그**입니다. `tunaround chat --features ...` 같은 런타임 플래그가 아닙니다. 소스에서 특정 조합을 쓰려면 `cargo run --features "semantic mcp" -- chat --db tuna.db` 형태입니다. 자세한 소스 빌드는 [source-run.md](../development/source-run.md).

## 2. 로컬 1인 (REPL·검색)

mesh·브로커가 필요 없습니다.

1. `claude`와 `codex` CLI를 설치하고 **로그인**합니다(에이전트는 tunaRound가 도는 그 머신에서 subprocess로 실행됩니다). LLM 러너만 쓸 거면 선택입니다.
2. `tunaround chat`. 전사 저장·`/search`가 필요하면 `tunaround chat --db tuna.db`.
3. 의미 검색을 추가하려면 Ollama 서버 + `semantic` 피처 빌드가 필요합니다([검색 설정](#검색-설정)).

`doctor`는 이 갈래와 무관합니다(워커 노드 전용). Kiwi는 첫 실행에 자동 다운로드되거나 조용히 lindera로 폴백합니다.

## 3. 브로커 · 대시보드 호스팅

한 머신이 코어를 띄우고 나머지가 붙습니다.

1. 대시보드가 필요하면 먼저 소스 빌드: `cd frontend && npm run build` → `cargo build --release --features dashboard`. (릴리스 바이너리엔 SPA가 없다는 점이 최대 함정입니다.)
2. 코어 실행:
   ```bash
   tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>
   ```
   토큰은 `--token` 또는 `TUNA_BROKER_TOKEN` env로 줍니다. 대시보드는 `/dashboard`에 뜹니다(loopback=풀컨트롤, 원격=읽기 전용 관전).
3. 개인 mesh 전체(codex-relay·presence·결과 인박스)를 상시화하려면 `~/.tunaround/config`를 채우고([설정 파일 3종](#5-설정-파일-3종-두-번째-함정) 참고) 재기동 스크립트를 씁니다(윈도우 `scripts/restart-win-mesh.ps1`).

최소 구성은 2번 하나(린 빌드면 대시보드는 플레이스홀더)입니다.

## 4. 머신 합류 (워커 노드 / 원격 REPL)

1. 워커 가능한 바이너리를 설치합니다(릴리스 바이너리엔 `worker` 포함. 소스면 `cargo install --features "semantic mcp serve worker"`). 프론트엔드는 필요 없습니다.
2. **원격 REPL로 붙기**: `tunaround join http://<코어-IP>:8770/mcp --token <토큰>`.
3. **워커 노드로 상주**:
   ```bash
   tunaround init --core http://<코어-IP>:8770/mcp --machine mac
   #  → node.toml + ~/.tunaround/config(mesh·훅용)를 한 번에 스캐폴드. 러너 자동 탐지(claude→codex→opencode).
   #  → ~/.tunaround/config 의 TUNA_BROKER_TOKEN 을 편집기로 채운다(데몬·훅·restart 스크립트가 읽음).
   #  → node/doctor를 직접 실행하려면 같은 토큰을 env로도: 셸에 타이핑 말고 프로파일(~/.zshrc 등)에
   #    편집기로 export TUNA_BROKER_TOKEN=... 추가(히스토리 유출 방지). restart 스크립트로 띄우면 파일에서 상속.
   tunaround doctor    # 코어 도달·토큰·러너 PATH·경로 프리플라이트
   tunaround node      # 브로커(self 또는 원격) + 워커 레인 상주
   ```
   `init`은 기존 `~/.tunaround/config`(실토큰 보유 가능)는 `--force` 없이 덮지 않고, 토큰은 placeholder만 넣습니다. node.toml만 원하면 `--no-mesh-config`. 저수준은 `tunaround work --core http://<코어-IP>:8770/mcp --token <토큰> --agent <이름> --runner claude`.

`doctor`는 워커 노드 실패 모드(설정 파싱·코어 도달·토큰·러너 PATH·피처 가용성·프로젝트 경로·태그 형식·형태소 백엔드)를 잘 짚습니다. 다만 (1) 기본 빌드엔 `doctor`가 없고, (2) 토큰 부재는 WARN이라 doctor가 통과해도 `node`가 인증 실패할 수 있으며, (3) 로컬 REPL 경로·훅/mesh 계층은 검사하지 않습니다.

## 5. 설정 파일 3종 (두 번째 함정)

역할이 다른 설정 파일이 세 개라 헷갈립니다. 스코프가 다릅니다.

| 파일 | 적용 대상 | 내용 | 조회 순서 |
| --- | --- | --- | --- |
| `tunaround.toml` | `chat`·`core`·`join`만 | 세션 프로파일(`db`·`roster`·`search_url`·`search_token_env` 등) | `--config` > `./tunaround.toml` > `~/.config/tunaround/config.toml` |
| `node.toml` | `node`·`doctor`·`init` | 워커 노드(`core`·`listen`·`token`·`[[lane]]`) | `--config` > `./tunaround.node.toml` > `~/.tunaround/node.toml` |
| `~/.tunaround/config` | SessionStart 훅 · mesh 스크립트 | dotenv형 `TUNA_*`(BROKER_CORE·BROKER_TOKEN·MACHINE·BIN·AUTOARM) | 파일 우선 > env > 기본값 |

`tunaround.toml` 프로파일은 `serve`/`mcp-search`/`reindex`에는 적용되지 않습니다. 값 우선순위는 `CLI 플래그 > 선택된 프로파일 > 기본값`이고, 토큰은 평문 대신 `search_token_env`(또는 node.toml의 `@env:NAME`)로 env 이름만 두는 쪽을 권장합니다. 예시는 레포 루트 `tunaround.toml.example`.

**`tunaround init`이 node.toml + `~/.tunaround/config`를 한 번에 스캐폴드**합니다(위 §4). node.toml의 토큰 env 이름을 데몬·훅과 같은 `TUNA_BROKER_TOKEN`으로 통일하므로, mesh 쪽(`node.toml`·`~/.tunaround/config`)은 **토큰 하나(TUNA_BROKER_TOKEN)** 만 채우면 됩니다(`tunaround.toml`은 검색 전용이라 별개). 즉 "3종"이지만 최초 셋업은 init 한 번 + 토큰 한 번입니다.

## 6. 토큰 로테이션과 env (세 번째 함정)

Rust 데몬은 기동 시점의 env를 고정합니다. 그래서 **토큰을 로테이션하면 이미 떠 있는 데몬은 옛 토큰으로 계속 돌다 실패**합니다. `setx`/새 셸 export도 기존 터미널엔 닿지 않습니다. `~/.tunaround/config` 파일은 매 훅 호출마다 다시 읽혀 이 문제를 훅·스크립트 한정으로 우회하지만, 데몬은 프로세스 시작 때 env를 읽습니다. **토큰을 바꾸면 데몬을 전부 재기동**하세요(재기동 스크립트가 `~/.tunaround/config`에서 새 값을 읽어 자식에 상속).

## 검색 설정

- **한국어 형태소(Kiwi)**: 첫 실행에 네이티브 라이브러리+모델을 OS 캐시로 자동 다운로드하고, 실패하면 lindera로 폴백합니다. 다운로드가 막히면 `KIWI_RS_VERSION`/`KIWI_LIBRARY_PATH`로 직접 지정합니다. 윈도우는 자동 다운로드가 불안정해 `scripts/install-kiwi-windows.sh`로 pre-seed합니다([kiwi-windows-setup](kiwi-windows-setup.md)).
- **의미 검색(Ollama)**: `semantic` 피처 빌드 + Ollama 서버가 필요합니다. 기본값은 아래이고 env로 바꿉니다. 모델을 바꾸면 다음 색인에 자동 재임베딩됩니다.
  ```bash
  export TUNAROUND_OLLAMA_URL=http://127.0.0.1:11435
  export TUNAROUND_EMBED_MODEL=qwen3-embedding:0.6b   # 예: bge-m3
  ```

## 7. 맥 ↔ 윈도우 차이

윈도우가 브로커+대시보드를 호스팅하고 맥이 워커/감독으로 붙는 구성을 기준으로, 맥 온보딩의 주의점입니다.

- **원격 `TUNA_BROKER_CORE` 필수.** 브로커가 윈도우에 있으므로 맥의 코어 URL은 원격이어야 합니다(기본 `http://127.0.0.1:8770`은 맥에서 틀림).
- **`TUNA_MACHINE=mac`** 필수(없으면 머신 태그가 `unix`로 등록).
- **`python`이 아니라 `python3`.** 맥 SessionStart 훅은 `python3`로 호출합니다.
- **대시보드 빌드는 윈도우 몫.** 맥은 `cargo install --features "semantic mcp serve worker"`로 충분(프론트엔드 툴체인 불필요).
- 데몬 수명: 맥은 `nohup` 분리 데몬(재부팅 시 죽음, launchd는 TODO), 윈도우는 `restart-win-mesh.ps1` + `mesh.pids` 선별 종료.

맥↔윈도우 왕복 개발 실무는 [dev-mac-windows](dev-mac-windows.md)에 있습니다.
