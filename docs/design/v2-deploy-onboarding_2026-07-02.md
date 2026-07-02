# tunaRound 배포·온보딩 설계 (2026-07-02)

> 결정 근거는 세션5 대화. 이 문서 = 배포 파이프라인 + 온보딩 UX 설계 정본. 구현 순서: clap 서브커맨드 → cargo-dist 릴리스 → 설정/프로파일.

## 0. 요약

- **배포**: cargo-dist(sshc와 동일 인프라) 재사용. mac/linux=Homebrew tap(`hang-in/homebrew-tap`), windows=powershell 원라이너. 풀 피처 단일 바이너리. Kiwi는 런타임 자동다운로드(번들 안 함).
- **온보딩**: 수동 arg 파싱 → clap 서브커맨드(`chat`/`core`/`serve`/`join`/`mcp-search`/`reindex`). `tunaround.toml` 프로파일 + 진입 시 선택(`--profile` 또는 대화형 픽커). doctor는 다음 단계.
- **보류**: scoop/winget(powershell로 충분), 분산 코어 홈랩 상주(별도 트랙, homelab-proxy 활용 가능).

## 1. 배포 파이프라인

### 사실
- 소스 공개/서비스 비공개. sshc가 이미 cargo-dist 0.31 사용(`dist-workspace.toml`, installers=shell/powershell/homebrew, tap=hang-in/homebrew-tap, targets mac/win/linux). tunapop는 Cask.
- cargo-dist 지원 인스톨러: shell·powershell·npm·homebrew·MSI. **scoop/winget 미지원**(요청 이슈 #521만).
- kiwi-rs 순수 Rust 빌드 → 크로스컴파일 무난. libkiwi는 런타임 자동다운로드(bab2min/Kiwi 릴리스, 맥 자산 존재).

### 설계
- `dist-workspace.toml`(또는 `[workspace.metadata.dist]`) 추가, sshc 패턴 답습.
  - `installers = ["shell", "powershell", "homebrew"]`
  - `targets = [aarch64-apple-darwin, x86_64-apple-darwin, aarch64-pc-windows-msvc, x86_64-pc-windows-msvc, aarch64-unknown-linux-gnu, x86_64-unknown-linux-gnu]`
  - `tap = "hang-in/homebrew-tap"`, `publish-jobs = ["homebrew"]`, `hosting = "github"`, `install-path = "CARGO_HOME"`.
  - **`features = ["semantic", "mcp", "serve"]`**(morphology·sqlite는 default) → 풀 기능 단일 바이너리. reqwest=rustls-tls라 OpenSSL 링크 회피.
- `.github/workflows/release.yml` 자동생성(sshc처럼 concurrency 블록 hand-edit 시 `allow-dirty=["ci"]`).
- 태그 푸시(`v0.x`) → CI 크로스빌드 → GitHub Release 첨부 + Homebrew formula tap에 발행.
- 사용자 설치: mac/linux `brew install hang-in/homebrew-tap/tunaround`, windows `irm <release>/tunaround-installer.ps1 | iex`.
- Kiwi: 번들 안 함. 첫 실행 자동다운로드(실패 시 lindera 폴백). 문서화.

### 보류(기록)
- **scoop**: cargo-dist 미지원. 원하면 별도 `hang-in/scoop-bucket` + 수동/CI 매니페스트. 현재는 powershell로 충분.
- **winget**: MSI 생성 후 winget-pkgs PR(수동/Komac). 마찰 커서 보류.
- **분산 코어 호스팅**: homelab-proxy(Caddy+WireGuard, n100)에 코어 상주 가능. 바이너리 배포와 별개 트랙. 착수 시 재설계.

## 2. 온보딩

### 통증(현재)
긴 `cargo run --features "..." -- --flags` 주문 / 플래그 혼동 / db·token·url·model 반복 / 분산 플래그 다수. main.rs 787줄 수동 파싱(모드 5개 cfg 분기).

### 설계 A: clap 서브커맨드 (토대)
현재 플래그 → 서브커맨드 매핑:
| 현행 | 서브커맨드 |
|---|---|
| (기본 REPL) `[state.json] --db --roster --recent-turns --pull-context --session --observe` | `tunaround chat` |
| `--core <addr>` | `tunaround core <addr>` |
| `--serve-mcp <addr> --token` | `tunaround serve <addr> --token` |
| (원격 프론트) `--search-url --search-token --pull-context` | `tunaround join <url> --token` (= chat + 원격코어 프리셋) |
| `--mcp-search` | `tunaround mcp-search` (⚠ 러너가 self-exe로 spawn — codex/claude 러너 args도 함께 갱신) |
| `--reindex` | `tunaround reindex` |
| (다음) | `tunaround doctor` |
- clap derive. feature별 서브커맨드는 `#[cfg]`로 게이트(mcp/serve 없으면 serve/core 숨김).
- ⚠ 내부 계약: `mcp-search`는 러너가 spawn하므로 codex.rs `build_mcp_wiring`·claude.rs args의 `--mcp-search`도 서브커맨드로 동시 교체(behavior-preserving 테스트로 가드).

### 설계 B: tunaround.toml + 프로파일 (진입 선택)
```toml
default_profile = "local"
[profile.local]                      # 단독
db = "~/.tunaround/local.db"
pull_context = false
[profile.homelab]                    # 홈랩 코어 프론트
search_url = "https://<도메인 설정파일에만>/mcp"
search_token_env = "TUNA_TOKEN"      # 토큰은 env 참조(레포·파일에 평문 금지)
pull_context = true
```
- 로드 우선순위: CLI 플래그 > 선택된 프로파일 > 기본값. (env 토큰은 `*_env` 키로 참조.)
- 진입 선택: `tunaround chat --profile homelab`. 프로파일 여럿 + 미지정이면 대화형 픽커.
- 위치: `./tunaround.toml` → `~/.config/tunaround/config.toml` 순 탐색. `--config <path>`로 지정.
- 도메인/토큰은 설정파일·env에만(소스·레포 미포함, 서비스 비공개 준수).

### 설계 C: doctor (다음 단계, 이번 범위 밖)
claude/codex CLI·인증, Ollama 도달, Kiwi 자동다운로드 성공, (core)포트, (front)코어 도달+bearer 프리플라이트.

## 3. 구현 순서 / 검증

1. **clap 서브커맨드**(설계 A): 최대 ROI 토대. 기존 모드 로직 본문은 유지하고 dispatch만 교체. mcp-search spawn 계약 동시 갱신. behavior-preserving 테스트.
2. **cargo-dist**(배포): dist-workspace.toml + release.yml. 태그로 첫 릴리스 스모크(맥 brew install + Kiwi 자동다운로드 실기 확인).
3. **tunaround.toml + 프로파일**(설계 B): 로드·머지·픽커.
4. (다음) doctor.

각 단계: cargo test(기본/features) + clippy, 커밋 분리. 위임 Sonnet + Opus 리뷰(규율).
