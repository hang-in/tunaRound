# tunaRound

터미널에서 Claude Code, Codex 같은 코딩 에이전트와 함께 **설계를 먼저 토론하는 도구**입니다.

기능을 바로 구현하기 전에, 여러 에이전트에게 서로 다른 역할을 맡기고 같은 레포를 보게 합니다. 한 에이전트는 제안하고, 다른 에이전트는 검토합니다. 사용자는 진행자이자 최종 결정자로 남습니다.

토론이 끝나면 결론을 문서로 저장하고, 그 내용을 바탕으로 바로 구현을 시작할 수 있습니다.

## 무엇인가

- **사용자가 진행합니다.**  
  사용자가 질문하거나 방향을 잡으면 에이전트들이 응답합니다. 에이전트끼리 끝없이 자동으로 대화하지 않습니다. 필요할 때만 `/debate`로 제한된 자동 토론을 돌릴 수 있습니다.

- **에이전트마다 역할을 나눕니다.**  
  예를 들어 Claude는 제안자, Codex는 리뷰어처럼 둘 수 있습니다. 같은 레포를 보더라도 역할이 다르면 다른 관점이 나옵니다.

- **같은 레포를 직접 읽습니다.**  
  긴 컨텍스트를 복붙하지 않습니다. 각 에이전트가 자기 CLI로 현재 작업 디렉터리를 직접 읽고 판단합니다.

- **토론 결과를 문서로 남깁니다.**  
  대화 중 나온 결론을 `design.md` 같은 문서로 저장합니다. 토론 따로, 구현 따로 흩어지는 일을 줄입니다.

## 써보기

`claude`와 `codex` CLI가 설치되어 있고 인증까지 되어 있다면 바로 실행할 수 있습니다.

```bash
cargo run            # = cargo run -- chat (인자 없으면 기본 REPL)
```

서브커맨드: `chat`(기본 REPL) · `core <addr>`(단일 프로세스 코어) · `serve <addr>`(헤드리스 코어) · `join <url>`(원격 코어 접속) · `work`(A2A 위임 워커 데몬) · `reindex`. `tunaround <명령> --help`로 옵션 확인.

예시:

```text
> 결제 모듈을 어떻게 설계할까?      # claude(제안자) + codex(리뷰어)가 응답
> @codex 이 부분만 봐줘            # codex에게만 질문
> @codex! 이 함수 고쳐줘            # codex가 실제 파일을 수정
> /debate 3 이 설계 괜찮나          # 에이전트끼리 최대 3턴 토론
> /branches                        # 대화 분기 보기
> /checkout 2                      # 특정 분기로 이동
> /conclude                        # 지금까지 토론 정리
> /search 인증 설계                 # 과거 대화와 문서 검색
> /save design.md                  # 토론 결과 저장
> /quit
```

세션을 이어서 쓰려면 상태 파일을 넘깁니다.

```bash
cargo run -- chat session.json
```

여러 역할과 엔진을 직접 정하려면 로스터 파일을 사용합니다.

```bash
cargo run -- chat --roster examples/roster.json
```

## 여러 터미널에서 같이 보기

Redis를 연결하면 여러 터미널에서 같은 세션을 공유할 수 있습니다.

```bash
export TUNAROUND_REDIS_URL=redis://127.0.0.1:6379
```

```bash
cargo run -- chat --session <id>   # 기존 세션 이어서 진행
cargo run -- chat --observe <id>   # 읽기 전용으로 관찰
```

이 기능은 한 터미널에서는 토론을 진행하고, 다른 터미널에서는 진행 상황만 지켜볼 때 유용합니다.

## 검색과 기억

긴 토론을 매번 전부 다시 넣지 않아도 되도록, 대화와 문서를 색인해두고 필요한 부분만 검색해서 가져옵니다.

```bash
cargo run --features "semantic mcp" -- chat --db tuna.db   # 기본 빌드에 morphology+sqlite 포함
```

사용 가능한 기능:

- `sqlite`  
  대화와 문서를 SQLite에 저장하고 FTS5로 검색합니다. `/search` 명령도 사용할 수 있습니다.

- `morphology`  
  한국어 검색을 위해 형태소 분석을 사용합니다. 예를 들어 “검색을” 같은 표현도 “검색”으로 잘 잡도록 돕습니다. 더 정확한 Kiwi를 우선 쓰고, 없으면 lindera로 자동 폴백합니다. 빌드는 순수 Rust라 macOS·Windows·Linux 모두 그대로 됩니다. Kiwi 네이티브 라이브러리(libkiwi)+모델은 첫 실행 때 OS 캐시에 자동으로 내려받습니다(실패해도 lindera로 동작). 자동 다운로드가 막히면 `KIWI_RS_VERSION`/`KIWI_LIBRARY_PATH` 환경변수로 지정하거나 캐시를 수동 설치합니다.

- `semantic`  
  Ollama 임베딩으로 의미 검색을 추가합니다. 기본 임베딩 서버는 `http://127.0.0.1:11435`, 기본 모델은 `qwen3-embedding:0.6b`입니다(실코퍼스에서 bge-m3보다 랭킹 우위).  
  서버는 `TUNAROUND_OLLAMA_URL`, 모델은 `TUNAROUND_EMBED_MODEL`(예: `bge-m3`)로 바꿀 수 있습니다. 모델을 바꾸면 다음 색인 때 자동 재임베딩됩니다.

- `mcp`  
  에이전트가 토론 중 직접 과거 맥락을 검색(`search_context`)하고, 지금까지의 전사를 읽어올(`read_transcript`) 수 있도록 도구를 붙입니다.

검색 결과는 단순 관련도만이 아니라 **유효성과 최신성**도 반영해 정렬합니다. `/reject`로 무효화한 발언은 제외하고, `/supersede`로 대체된 발언은 뒤로 내립니다. 서로 다른 세션에서 온 결과 중 오래된 것은 약하게 낮춥니다. 다만 관련성이 높은 오래된 결정은 함부로 밀지 않습니다.

기본 실행은 가볍게 유지하고, 검색·임베딩·MCP 같은 무거운 기능은 필요할 때만 켜는 구조입니다.

## 설정 프로파일

`--db`, `--roster`, `--search-url` 같이 반복되는 옵션을 매번 입력하지 않도록 `tunaround.toml`에 프로파일로 저장하고, 실행할 때 프로파일만 골라 쓸 수 있습니다.

레포 루트의 `tunaround.toml.example`을 참고해 `tunaround.toml`로 복사한 뒤 값을 채웁니다(이 파일은 gitignore 대상이라 사설 도메인·토큰을 넣어도 커밋되지 않습니다).

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

```bash
tunaround chat --profile homelab
```

설정 파일은 `--config <경로>`로 직접 지정하거나, 지정하지 않으면 `./tunaround.toml` -> `~/.config/tunaround/config.toml` 순서로 찾습니다. 프로파일이 여러 개인데 `default_profile`도 `--profile`도 없으면 번호를 골라 선택하는 대화형 프롬프트가 뜹니다.

값 우선순위는 **CLI 플래그 > 선택된 프로파일 > 기본값**입니다. 토큰은 설정 파일에 평문으로 적을 수도 있지만, `search_token_env`로 환경변수 이름만 적어두는 쪽을 권장합니다. 이 옵션들은 `chat`·`core`·`join`에서만 적용되고, `serve`·`mcp-search`·`reindex`는 쓰지 않습니다.

## 여러 머신에서 함께 토론하기 (원리)

두 사람이 각자 다른 컴퓨터(예: 맥과 윈도우)에서 **같은 설계 토론**에 참여할 수 있습니다. 원리는 "공유 화이트보드 + 각자의 비서"로 생각하면 쉽습니다.

- **하나의 코어 = 공유 화이트보드.** 한 컴퓨터가 코어를 띄웁니다(`serve <주소>` 또는 `core <주소>`). 코어는 **토론 전사와 검색 색인을 담은 유일한 원본**입니다.
- **각 컴퓨터 = 각자의 비서.** 다른 컴퓨터는 그 코어에 붙습니다(`join <url>`). claude·codex 에이전트는 **각 컴퓨터에서 로컬로** 돕니다.
- **읽고 쓰기.** 에이전트는 발언하기 전에 화이트보드를 **읽고**(전사·과거 맥락을 네트워크 너머로 당겨옴), 자기 발언을 화이트보드에 **적습니다**(코어 전사에 기록). 그래서 한쪽에서 한 말을 다른 쪽 에이전트가 이어받습니다.

핵심은 **복사가 아니라 공유**라는 점입니다.

- 전사는 **코어 한 곳에만** 있습니다. 각 머신이 사본을 만들어 동기화하는 게 아니라(DB 복제가 아님), 필요할 때 코어에 원격으로 물어봅니다. 코어가 발언 순번의 유일한 권위라 여러 머신이 동시에 써도 충돌하지 않습니다.
- 에이전트는 전체를 다 받지 않고 **필요한 조각만 당겨옵니다**(pull). 그래서 토론이 길어져도 프롬프트가 가볍습니다.

연결에 필요한 것은 코어 주소 + 토큰(bearer 인증) + 서로 닿는 네트워크(같은 공유기면 사설 IP, 아니면 Tailscale/SSH 터널)뿐입니다.

```bash
# 코어를 띄우는 컴퓨터
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>

# 붙는 컴퓨터
tunaround join http://<코어-IP>:8770/mcp --token <토큰>
```

(참고: 위의 "여러 터미널에서 같이 보기"의 Redis 미러링은 이것과 별개인 관찰 전용 기능입니다. 머신 간 토론 공유는 여기 코어 방식을 씁니다.)

## 파트너 에이전트에게 작업 위임하기 (A2A)

토론과 별개로, 코어는 **A2A(Agent2Agent) 프로토콜을 기반으로 한** 작업 브로커로도 동작합니다. 한 쪽(dispatcher)이 다른 머신·다른 종류의 에이전트(worker)에게 작업을 맡기고, 그 진행과 결과를 받아볼 수 있습니다.

> **호환 범위(정직하게):** A2A의 구조(Task 생명주기·Message/Artifact·Agent Card·JSON-RPC·SSE)를 차용해 **tunaRound 인스턴스끼리** 위임하는 것이 목적입니다. 중앙-브로커 라우팅(`fromAgent`/`toAgent`)과 인증-게이트 카드 등 확장이 있어, **임의의 제3자 표준 A2A 클라이언트와의 완전 호환은 목표가 아닙니다**(JSON-RPC envelope·`GetTask` 레벨은 호환 확인, 카드 발견·`SendMessage`는 브로커 확장이라 미호환). 진짜 표준 게이트웨이가 필요해지면 별도 어댑터로 다룹니다.

- **코어 = 작업 큐 + A2A 서버.** `serve`로 띄운 코어가 `/a2a`(JSON-RPC: `SendMessage`/`GetTask`/`CancelTask` + 스트리밍 `SendStreamingMessage`/`SubscribeToTask`)와 Agent Card(`/.well-known/agent-card.json`)를 노출합니다.
- **자율 워커 데몬.** `tunaround work`는 자기 앞으로 온 작업을 스스로 발견(poll)하고, 착수(claim)하고, 지정한 러너로 실행한 뒤, 결과를 되돌립니다(complete). 사람이 "이제 처리해"라고 신호할 필요가 없습니다.
- **이기종 파트너.** 워커가 무엇으로 작업을 실행할지 `--runner`로 정합니다. 같은 데몬을 Claude로도, Codex로도, 로컬 LLM(Ollama 등 OpenAI 호환 HTTP)으로도 띄울 수 있습니다.
- **실시간 스트리밍.** dispatcher가 `SendStreamingMessage`로 던지면 작업의 상태 변화(submitted -> working -> 결과 artifact -> completed)를 SSE로 실시간 구독합니다. 폴링 없이 진행을 지켜봅니다.

```bash
# 코어(작업 브로커) 한 대
tunaround serve 0.0.0.0:8770 --db shared.db --token <토큰>

# 워커 데몬 - win-worker 앞 작업을 Claude로 자율 처리
tunaround work --core http://<코어-IP>:8770/mcp --token <토큰> \
  --agent win-worker --runner claude

# 로컬 LLM(Ollama)을 워커로 (이기종 파트너)
tunaround work --core http://<코어-IP>:8770/mcp --token <토큰> \
  --agent llm-worker --runner http --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b
```

사람은 목표를 한 번 발행할 뿐, 발견·실행·완료·통지는 기계끼리 처리합니다. 자율 수준은 **semi-a2a**(사람이 목표를 정하는 HITL)이며, 사람 없이 무한히 도는 자동 토론 루프는 의도적으로 두지 않았습니다. 작업을 던지는 dispatcher 쪽 요청·워커 실행·프로젝트별 라우팅 등 **구체적인 사용법은 [docs/reference/a2a-usage.md](docs/reference/a2a-usage.md)**를 참고하세요.

## 현재 상태

v1 본체와 v2 검색·맥락 기능이 대부분 들어왔습니다.

현재 가능한 것:

- 역할을 나눈 다중 에이전트 토론
- Claude Code, Codex 기반 응답
- 특정 에이전트 지목
- 특정 에이전트에게 파일 수정 맡기기
- `/debate`를 통한 제한된 자동 토론
- 대화 분기와 체크아웃
- 세션 저장과 재개
- Redis 기반 멀티세션 관찰
- SQLite + FTS5 검색
- 한국어 형태소 검색 (외래어 병기: 리프레시 ↔ refresh)
- 의미 검색 (Ollama 임베딩, 기본 `qwen3-embedding:0.6b`)
- BM25 + 의미 검색을 합친 하이브리드 검색
- 유효성과 최신성을 반영한 검색 랭킹 (`/reject`·`/supersede`·`/explain`)
- MCP 기반 에이전트 직접 검색과 전사 읽기 (Claude·Codex 둘 다 pull)
- `tunaround.toml` 프로파일로 반복 옵션 저장·진입 선택
- 긴 토론에서 오래된 발언을 요약해 다음 라운드로 넘기는 요약 이월
- 통째 주입 대신 에이전트가 맥락을 직접 당겨오는 방식 (push → pull, `--pull-context`)
- 코어의 검색·전사를 네트워크 HTTP MCP로 노출 (`--serve-mcp`, 원격 접속 토대)
- 로컬/원격 LLM 참가자 (ollama, lmstudio, openai 같은 HTTP 엔진, opencode CLI)
- A2A 기반 작업 위임: 파트너 에이전트에게 작업을 맡기고 결과를 받는 브로커 코어 (`/a2a`, Agent Card. tunaRound끼리 위임용, 제3자 표준 완전호환은 비목표)
- 자율 워커 데몬 (`tunaround work`: poll -> claim -> 러너 실행 -> complete, 사람 트리거 없이)
- 이기종 워커 러너 (Claude / Codex / 로컬 LLM을 `--runner`로 교체)
- A2A 작업 진행 실시간 SSE 스트리밍 (`SendStreamingMessage`/`SubscribeToTask`)

빌드는 macOS·Windows·Linux 모두 순수 Rust로 됩니다. Windows와 macOS(aarch64) 모두 실제 `claude`·`codex` CLI로 동작을 확인했습니다(빌드·테스트·`cargo install`·2에이전트 토론 도그푸딩, 크로스머신 A2A 읽기 스모크 포함). macOS에서 Kiwi 네이티브 자동다운로드가 막히면 lindera로 폴백해 그대로 동작합니다.

## 왜 만들었나

tunaRound는 새 에이전트 프레임워크를 만들려는 도구가 아닙니다.

이미 잘 동작하는 CLI 에이전트들을 터미널 안에서 함께 쓰기 쉽게 묶는 얇은 도구입니다.

코드를 바로 짜기 전에 설계를 먼저 검토하고 싶을 때가 있습니다.  
혼자 생각하면 놓치는 부분이 있고, 한 에이전트에게만 물어보면 답이 한 방향으로 굳기 쉽습니다.

tunaRound는 이 과정을 터미널 안에서 반복 가능하게 만듭니다.

- 한 에이전트는 제안합니다.
- 다른 에이전트는 반박하거나 검토합니다.
- 사용자는 중간에서 방향을 잡습니다.
- 결론은 문서로 남깁니다.
- 필요하면 그 자리에서 바로 구현으로 넘어갑니다.

## 어디서 가져왔나

tunaRound는 기존 프로젝트에서 검증한 기능들을 가져와 작게 묶은 도구입니다.

- 토론 흐름은 `tunapi`의 roundtable 구조에서 가져왔습니다.
- CLI 실행과 스트림 처리는 `tunaFlow`의 러너 경험을 바탕으로 했습니다.
- Redis 기반 세션 공유와 관찰 기능은 `tunaSalon`에서 가져왔습니다.
- 한국어 검색과 하이브리드 검색은 `seCall`의 경험을 옮겨왔습니다.

## 기술 스택

- Rust
- tokio
- JSON 세션 파일
- SQLite + FTS5
- Ollama 임베딩 (기본 `qwen3-embedding:0.6b`)
- Redis
- MCP
- clap CLI / cargo-dist 배포

기본 UI는 가벼운 REPL입니다.  
TUI나 웹 UI는 이후 단계에서 붙일 예정입니다.

## 로드맵

완료:

- [x] 여러 에이전트와 역할 설정
- [x] Redis 기반 세션 공유와 관찰
- [x] 에이전트에게 코드 수정 맡기기
- [x] `/debate` 자동 토론
- [x] 한국어 형태소 검색 (Kiwi / lindera)
- [x] SQLite + FTS5 검색
- [x] 과거 맥락 검색 주입
- [x] bge-m3 의미 검색
- [x] BM25 + 의미 검색 하이브리드
- [x] MCP `search_context` 도구
- [x] 가벼운 컨텍스트 주입 (최근 N턴 + 검색 결과, `--recent-turns`)
- [x] 로컬 LLM 참가자 (ollama, lmstudio, openai 같은 HTTP 엔진)
- [x] opencode CLI 참가자
- [x] 요약 이월 (긴 토론의 오래된 발언을 요약해 다음 라운드로)
- [x] 에이전트 전사 읽기 도구 (`read_transcript`)
- [x] push → pull 컨텍스트 (에이전트가 전사를 직접 당겨와 프롬프트 경량화, `--pull-context`)
- [x] 코어를 네트워크 HTTP MCP로 노출 (`--serve-mcp`, 원격 접속 토대)
- [x] 원격 참가자 쓰기 (원격에서 코어 전사에 자기 턴 기록, `post_turn`/`--core`)
- [x] 유효성 인지 검색 랭킹 (`/reject` 제외, `/supersede` 강등, `/explain` 디버그)
- [x] 최신성 인지 검색 랭킹 (세션 간 오래된 결과 약한 강등)
- [x] 외래어 병기 검색 (한글 외래어 ↔ 영어 원어)
- [x] Codex 전사 pull (behavioral read-only)
- [x] 서브커맨드 CLI (`chat`/`core`/`serve`/`join`/`work`/`reindex`) + `tunaround.toml` 프로파일
- [x] 배포 파이프라인 준비 (cargo-dist, Homebrew/powershell)
- [x] 원격/분산 참가자 라이브 (맥 ↔ 윈도우 크로스머신 A2A, 양방향 왕복 + SSE 스트리밍 스모크)
- [x] A2A 기반 작업 위임 브로커 (`/a2a` SendMessage/GetTask/CancelTask + Agent Card, tunaRound끼리)
- [x] A2A SSE 스트리밍 (`SendStreamingMessage`/`SubscribeToTask`)
- [x] 자율 워커 데몬 (`tunaround work`) + 이기종 러너 (Claude/Codex/로컬 LLM)
- [x] 워커 실패 시 task `failed` 전이 (`fail_task`, completed와 구분)
- [x] context_id 기반 프로젝트별 작업 라우팅 (`--context-map`, 데몬 하나가 여러 프로젝트 배분)

다음:

- [ ] 공개 릴리스 (도그푸딩 후 태그)
- [ ] 워커 고착(claim 후 죽음) 방지 (타임아웃/재큐)
- [ ] 세션을 넘나드는 프로젝트 기억
- [ ] 리치 TUI / 웹 UI

## 설계 문서

전체 설계는 아래 문서를 참고하세요.

- [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md)
- [docs/plans/index.md](docs/plans/index.md)

## 라이선스

[AGPL-3.0](LICENSE) (GNU Affero General Public License v3.0).
