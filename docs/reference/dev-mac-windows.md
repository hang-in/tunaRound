# 맥 - 윈도우 왕복 개발 핸드오프

> 두 머신(맥/윈도우)을 오가며 tunaRound를 개발할 때의 환경·재개 가이드. 세션별 핸드오프가 아니라 **상시 참조용**. 사설 도메인/토큰은 레포 문서에 넣지 않는다(각 머신의 env·로컬 설정에만).

## 0. 왕복 규칙 (제일 중요)

- **머신을 옮기기 전 항상 push**, 옮긴 뒤 항상 `git pull`. 작업은 `main`에 올린다(현재 정책).
- 검증(build/test)과 commit/push는 분리. cargo는 **Bash 툴**로(PowerShell 아님).
- 진행 상태·결정은 레포의 `checklist.md` + `context-notes.md`에 남긴다 = 두 머신이 공유하는 단일 진실.

## 1. 공통 전제 (양 머신)

- Rust 툴체인(rustup).
- `claude` CLI + `codex` CLI 설치 + **로그인**(에이전트는 tunaRound가 도는 그 머신에서 subprocess로 실행됨).
- git.
- (선택) 원격 Ollama 접근(의미검색), Redis(멀티세션 관찰).

## 2. 레포 동기 / 실행

```bash
git clone https://github.com/hang-in/tunaRound   # 처음
git pull                                          # 이어받기
cargo run -- chat                                 # 기본 REPL (인자 없으면 자동 chat)
```
- 서브커맨드: `chat`(기본) · `core <addr>` · `serve <addr>` · `join <url>` · `reindex`. `cargo run -- <명령> --help`.
- 검색·의미검색·MCP까지: `cargo run --features "semantic mcp" -- chat --db tuna.db` (morphology+sqlite는 default 포함).
- 설치형: `cargo install --path .` → 이후 `tunaround`.

## 3. 플랫폼 차이 (왕복 시 주의)

### 형태소 분석기(Kiwi/lindera)
- 빌드는 순수 Rust라 양쪽 동일하게 됨. Kiwi 네이티브(libkiwi)+모델은 **런타임 자동 다운로드**(OS 캐시), 실패하면 lindera 폴백.
- **Windows**: 이미 `%LOCALAPPDATA%\kiwi`에 pre-seed됨(자동 다운로드 성공 이력).
- **맥**: 첫 실행 시 자동 다운로드 예상(bab2min/Kiwi에 `kiwi_mac_arm64`/`x86_64` 자산 존재). 안 되면 `KIWI_RS_VERSION`/`KIWI_LIBRARY_PATH` env 또는 수동 캐시. 그래도 안 되면 lindera로 동작(검색 품질 측정도 lindera 기준이라 무방).

### 줄바꿈(CRLF/LF)
- Windows에서 커밋 시 "LF will be replaced by CRLF" 경고가 뜨지만 **레포엔 LF로 저장**되므로 맥(LF)과 충돌 없음. 대량 diff가 보이면 `git config core.autocrlf` 확인. 필요하면 `.gitattributes`(`* text=auto eol=lf`)로 정규화(1회 renormalize 커밋 발생하니 작업 경계에서).

### 경로
- DB/스크래치 경로는 OS마다 다름. `--db`는 상대·절대 경로 자유. `tunaround.toml` 프로파일(`--config`/`--profile`, `tunaround.toml.example` 참고)을 쓰면 `db`/`roster`의 선행 `~/`를 홈 디렉터리로 자동 확장하므로 `~/.tunaround/…` 같은 홈 기준 경로를 권장.

## 4. 백엔드 / 환경변수 (양 머신에서 동일 이름)

- `TUNAROUND_OLLAMA_URL` — 의미검색 임베딩 서버(기본 `http://127.0.0.1:11435`). 원격 Ollama는 각자 SSH 터널로 로컬 포트에 연결(호스트·포트는 개인 설정, 레포에 미기재).
- `TUNAROUND_EMBED_MODEL` — 기본 `qwen3-embedding:0.6b`(bge-m3보다 랭킹 우위 측정). `bge-m3`로 교체 가능. 모델 바꾸면 다음 색인 때 자동 재임베딩.
- `TUNAROUND_REDIS_URL` — 멀티세션 관찰(선택, 기본 미사용).
- 원격 코어 접속용 도메인/bearer 토큰은 **env·로컬 설정파일에만**(레포 미포함, 서비스 비공개 원칙).

## 5. 검증 루틴 (커밋 전, Bash 툴)

```bash
cargo test                                             # 기본(morphology+sqlite)
cargo test --features "semantic morphology mcp serve"  # 풀 피처
cargo clippy --features "semantic morphology mcp serve"
```
- 의미검색/모델 비교 등 수동 테스트는 `#[ignore]`(Ollama 터널 필요): `cargo test --features "semantic morphology" --test embed_model_compare -- --ignored --nocapture`.

## 6. 배포 도구(cargo-dist)

- `dist`(cargo-dist 0.31.0)로 릴리스. 로컬 설치는 각 머신에서(Windows는 `D:\.cargo\bin`에 설치됨). 미설치 머신은 powershell/shell 인스톨러로 `v0.31.0` 설치.
- 설정: `dist-workspace.toml`(+ `.github/workflows/release.yml`). 드라이런: `dist plan`.
- **실제 릴리스는 도그푸딩 후 태그 푸시**: `git tag v0.1.0 && git push origin v0.1.0` → 공개 Release + `hang-in/homebrew-tap` 발행. 라이선스=AGPL-3.0.
- ⚠ 크로스컴파일 리스크(rusqlite bundled C·reqwest rustls·axum, 특히 aarch64-linux)는 첫 릴리스 CI에서 확인.

## 7. 분산(맥 ↔ 윈도우) 실행

- 한 머신이 코어(`tunaround serve <addr> --token <T>` 헤드리스, 또는 `core <addr>` 단일프로세스), 다른 머신이 프론트(`tunaround join <url> --token <T>`)로 접속해 전사·검색 공유. 양쪽 다 자기 claude/codex 필요.
- 네트워크는 Tailscale/LAN/SSH. 코어 바인드는 `0.0.0.0:<port>`, 인증은 bearer.
- 상세·검증범위: `docs/design/v2-deploy-onboarding_2026-07-02.md`, `docs/design/v2-A2A-core-backend_2026-06-30.md`.

## 8. 현재 상태 / 재개 포인터 (2026-07-02 기준)

- **완료**: v1 + v2 검색/맥락 로드맵(step 2~8) + Stage 3a-3d + codex pull(behavioral) + 실코퍼스 회귀(step 6) + 외래어 병기 색인 + 임베딩 기본 qwen3. 배포 온보딩 **Stage 1(clap 서브커맨드)·Stage 2(cargo-dist 설정)·Stage 3(tunaround.toml 프로파일)** 구현 완료(Stage 3은 리뷰·커밋 대기).
- **대기/다음**: 배포 실릴리스(도그푸딩 후 태그) · 온보딩 Stage 4 doctor · abstraction/anchors(보류) · 분산 코어 홈랩 호스팅(보류).
- **재개 시 읽을 것**: `checklist.md`(단계별 체크+커밋해시) · `context-notes.md`(최근 노트) · `docs/plans/index.md` · `docs/design/v2-deploy-onboarding_2026-07-02.md` · 최신 세션 핸드오프(`docs/prompts/`).
- 검증 기대치(참고): 기본 166+6 / 풀피처 180+9 테스트 pass, clippy 클린(세션마다 갱신).
