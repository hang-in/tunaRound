# tunaRound

터미널에서 **사용자가 주도하는 N-에이전트 설계 토론** 도구입니다.

기능이나 서비스를 구현하기 전에, Claude Code와 Codex에게 서로 다른 역할을 부여하고 같은 레포 위에서 설계를 토론하게 합니다. 사용자는 진행자이자 최종 결정자로 남고, 두 에이전트는 각자의 CLI로 레포를 읽으며 의견을 냅니다. 토론이 끝나면 결론을 문서로 남겨 곧바로 구현으로 넘어갑니다.

## 무엇인가

- **사용자가 진행을 잡는다.** 사용자가 질문하거나 방향을 제시하면 에이전트들이 응답합니다. 에이전트끼리 자동으로 끝까지 토론하지 않습니다(`/debate`로 바운드된 자동 교환은 선택).
- **역할을 나눈다.** 한 자리는 제안자(proposer), 한 자리는 리뷰어(reviewer)처럼 역할을 주면 같은 레포를 보더라도 다른 관점이 나옵니다.
- **같은 레포를 직접 본다.** 에이전트들은 같은 작업 디렉터리를 각자의 CLI로 읽습니다. 컨텍스트를 길게 복붙하지 않습니다.
- **결론을 문서로 남긴다.** 토론을 결과 문서로 저장해, 곁가지에서 나온 결론을 다시 옮기는 단절을 줄입니다.

## 써보기

claude · codex CLI가 설치·인증돼 있으면:

```
cargo run
> 결제 모듈을 어떻게 설계할까?      # claude(제안자) + codex(리뷰어)가 응답
> @codex 이 부분만 봐줘            # 한 자리만 지목(읽기 전용)
> @codex! 이 함수 고쳐줘            # 쓰기 턴: 지목한 에이전트가 레포를 편집
> /debate 3 이 설계 괜찮나          # 에이전트끼리 N턴 자동 교환(기본 3, 최대 10)
> /branches                        # 분기 트리 보기
> /checkout 2                      # 특정 메시지로 분기 전환
> /conclude                        # 지금까지 토론을 종합
> /search 인증 설계                 # 색인된 과거 맥락을 직접 검색(--db 필요)
> /save design.md                  # 토론을 결과 문서로 저장
> /quit
```

세션을 이어가려면 상태 파일을 넘깁니다: `cargo run -- session.json`(시작 시 이어받고 종료 시 저장).

자리 구성을 바꾸려면 로스터를 줍니다: `cargo run -- --roster examples/roster.json`(역할 × 엔진 N자리).

## 멀티세션 (Redis, 선택)

Redis가 있으면 여러 프로세스가 한 세션을 공유합니다. `TUNAROUND_REDIS_URL`을 설정한 뒤:

- `cargo run -- --session <id>`: 스냅샷에서 세션을 재개합니다(owner lease 포함).
- `cargo run -- --observe <id>`: 라이브로 관찰합니다(읽기 전용 구독).

## 검색과 맥락 (선택, 빌드 피처)

긴 토론이나 프로젝트 맥락을 매번 통째로 재주입하지 않고, 검색해서 관련 부분만 끌어 씁니다. 한국어는 형태소 분석으로 선-토크나이즈해 색인합니다("검색을"을 "검색"으로 잡습니다). `--db <path>`를 주면 라운드마다 메시지가 색인됩니다.

- `--features morphology`: 한국어 형태소 토크나이저(Kiwi 메인 + lindera 폴백).
- `--features sqlite`: SQLite 시스템오브레코드 + FTS5 선-형태소화 색인 + 검색 주입(RAG). 라운드 프롬프트에 관련 과거 맥락(다른 분기·과거 세션)을 검색해 덧붙입니다. `/search`로 직접 검색도 가능합니다.
- `--features semantic`: bge-m3 임베딩(원격 Ollama, reqwest) + 벡터 검색 + 하이브리드(BM25 + 의미, RRF 융합). 임베딩 엔드포인트는 `TUNAROUND_OLLAMA_URL`(기본 `http://127.0.0.1:11435`).
- `--features mcp`: 에이전트가 토론 중 스스로 검색하는 MCP 도구(`search_context`). 러너가 claude에 검색 서버를 물려 자율 호출하게 합니다.
- 예: `cargo run --features "sqlite morphology semantic mcp" -- --db tuna.db`

## 상태

**v1 완료 + v2 검색/맥락 스택 완결.** 위 v1 본체(역할 부여·순차 토론·결과 문서·세션 재개)에 더해 N자리 로스터, 협업 코딩(`@engine!` 쓰기 턴), Redis 멀티세션(`--observe`·`--session`), `/debate` 바운드 자동 교환, 그리고 검색/맥락 전 계층 - 형태소 FTS, 라이브 색인, 검색 주입(RAG), `/search`, 벡터·하이브리드, 에이전트 MCP 검색 도구 - 이 들어왔습니다. 실 claude/codex로 Windows에서 동작 검증됐습니다. 전체 설계는 아래 spec을 참고하세요.

## 왜

tunaRound는 새 에이전트 프레임워크라기보다, 이미 검증된 구성요소를 터미널 네이티브로 얇게 묶는 도구입니다.

- 토론 진행(역할·순차 응답)은 tunapi의 roundtable 코어에서 가져왔습니다.
- 에이전트 CLI 실행·스트림 파싱·러너 견고화는 tunaFlow의 러너 경험에서 가져왔습니다.
- 멀티세션·관찰·재개(Redis)는 tunaSalon에서 가져왔습니다.
- 한국어 형태소 검색(FTS·벡터·하이브리드)은 secall에서 포팅합니다.

## 스택

Rust + tokio. 영속은 JSON 파일(전사·세션 상태)이고, 검색 인덱스는 SQLite + FTS5 + 벡터(BLOB)입니다. 의미 임베딩은 원격 Ollama(bge-m3), 에이전트 검색 도구는 MCP(rmcp), 멀티세션은 Redis로 둡니다. 무거운 의존(SQLite·임베딩·MCP)은 빌드 피처로 분리해 기본 빌드는 가볍게 유지합니다. 첫 UI는 thin REPL이고 ratatui·웹 UI는 이후 단계입니다.

## v2 로드맵

- [x] N자리 로스터: 역할 × 엔진 조합
- [x] Redis 멀티세션(관찰·재개) = git-tree 기반 분기 토론
- [x] 에이전트가 직접 코드를 편집하는 협업 코딩(`@engine!`)
- [x] `/debate` 바운드 자동 교환
- [x] 한국어 형태소 검색(SQLite + FTS5) + 라이브 색인
- [x] 검색 주입(RAG): 관련 과거 맥락을 라운드 프롬프트에 + `/search`
- [x] 벡터 임베딩(bge-m3) + 하이브리드(BM25 + 의미, RRF) 검색
- [x] 에이전트 능동 검색 도구(MCP `search_context`)
- [ ] 재주입 축소(통째 대신 최근 N턴 + 검색 슬라이스), 세션 간 프로젝트 기억
- [ ] 리치 TUI·웹 UI, 로컬 LLM·opencode 좌석

## 설계 문서

전체 설계는 [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md)를 참고하세요. 진행 현황은 [docs/plans/index.md](docs/plans/index.md)에 있습니다.
