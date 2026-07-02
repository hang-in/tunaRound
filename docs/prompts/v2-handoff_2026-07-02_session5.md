# tunaRound v2 핸드오프 - 2026-07-02 세션 5 (Windows, 맥 왕복 준비)

> 이전: [session4](v2-handoff_2026-07-01_session4.md)(3a-3·3d·시간성유효성 step2~8). 이 세션 = **kimi 리뷰 자체검증 + 시간성·유효성 마무리(step 5c) + codex pull 활성화 + 실코퍼스 회귀(step 6) + 외래어 병기 색인 + 임베딩 기본 교체 + 배포/온보딩 설계·구현(clap·cargo-dist·프로파일) + AGPL-3.0 + 맥-윈도우 핸드오프**.
> 콜드 스타트 가정. 이 문서 + `checklist.md` + `context-notes.md`(하단 최신) + `docs/reference/dev-mac-windows.md`(왕복)로 이어갈 수 있게 씀.

## ⓪ 가장 먼저 (맥/윈도우 공통)

1. 이 문서 + `checklist.md` + `context-notes.md` + `docs/reference/dev-mac-windows.md`(크로스머신) 읽기.
2. **cargo는 Bash 툴로**: `cargo test`(기본) → **190개(184 lib + 6 main-cli) pass 기대**. `cargo test --features "semantic morphology mcp serve"` → **207개(198 lib + 9 cli) pass 기대**. `cargo clippy --features "..."` 클린.
3. `git log --oneline -12`로 이번 세션 커밋 확인. **origin/main = c89da05 (전부 푸시).** 워킹트리 클린(untracked `.omc/`·`docs/plans/v2-18~20.md`·`docs/*a2a*`는 세션2 잔여, 무관).

## ① 이 세션이 한 것 (한 줄)

시간성·유효성 로드맵을 완결(step 5c·6)하고, codex pull을 살리고(behavioral read-only), 외래어 검색 갭을 메우고, 임베딩 기본을 qwen3로 바꾸고, **배포(cargo-dist)와 온보딩(clap 서브커맨드 + tunaround.toml 프로파일)을 설계·구현**해 도그푸딩 직전까지 왔다. AGPL-3.0 확정.

## ② 커밋 요지 (순서대로, 전부 origin/main)

**검색/맥락 마무리**
- `f8a7a15` fix(mcp): search_context blocking retrieve를 spawn_blocking(async executor 비차단). kimi 리뷰 자체검증 결과 유일한 실유효 지적.
- `1aa0661` feat(store): cross-session recency 랭킹 + `created_at` 컬럼(스키마 **v5**). 정책 A(보수): 다른 세션의 후보 최신 대비 7일 초과만 소폭 강등.
- `101b6be` feat(search): /explain에 created_at·recency 표시 + step 5c 라이브 검증.
- `1491293` fix(safety): bounded bus 채널(unbounded→channel(1024)+try_send) + snapshot 실패 로그 + Kiwi unsafe Send 주석 강화.
- `ed535b4` feat(runner): codex 원격 HTTP MCP bearer 인증 배선(ExecSpec env + bearer_token_env_var, argv 미노출).
- `ef33a6a`/`46411c7` feat(runner): **codex pull 활성화**. codex exec는 read-only 유지한 채 MCP 승인 불가(업스트림 #24135) → `--dangerously-bypass-approvals-and-sandbox` + READONLY_DIRECTIVE(behavioral). is_mcp_capable=claude|codex, RunInput.pull 전파. **라이브 e2e 통과**(codex가 MCP 호출→전사 인용→파일 변경 0).
- `adc5bf0`/`9c55a0a` test(search): **실코퍼스 회귀(step 6)**. seCall tunaRound 실 턴 코퍼스(18→23발언, 검색인프라+auth 2도메인) + 12→15질의. FTS R@5 0.878→0.944, 하이브리드 0.978. **발견**: 외래어 음역 갭(리프레시↔refresh)은 하이브리드도 못 메움.
- `93a2481` feat(search): **외래어 음역 병기 색인**. `loanword_aliases`(음역 32그룹) + fts_query 질의확장. 리프레시 질의 R@5 0→1.0. 합성 코퍼스 불변.
- `e5f28c7` feat(embed): **기본 임베딩 qwen3-embedding:0.6b**(bge-m3보다 hybrid MRR 우위 측정) + `OllamaEmbedder::from_env`(TUNAROUND_EMBED_MODEL). 둘 다 dim 1024.

**배포/온보딩**
- `b718de3` docs: 배포·온보딩 설계문서([docs/design/v2-deploy-onboarding_2026-07-02.md]).
- `4ac327f` feat(cli): **clap 서브커맨드**(chat/core/serve/join/mcp-search/reindex). 수동 파싱 교체, 본문 불변. 러너 self-exe `--mcp-search`→`mcp-search`. ⚠파괴변경: bare `tunaround file.json`→`chat file.json`.
- `9ce3a23` build(dist): **cargo-dist 설정**(0.31.0, homebrew+powershell, 6타깃, features semantic/mcp/serve, tap hang-in/homebrew-tap). 태그 미푸시=릴리스 미발행.
- `6f946a8` docs: **AGPL-3.0**(Cargo.toml + LICENSE 전문) + `docs/reference/dev-mac-windows.md`.
- `c89da05` feat(config): **tunaround.toml 프로파일**(`src/config.rs`). --config/--profile, 진입 선택(default/단일/다중 픽커), CLI>프로파일 우선, ~ 확장, search_token_env. + README 홀리스틱 정리.

## ③ 현재 상태 / 검증

- **기본 184 lib + 6 cli / features(semantic morphology mcp serve) 198 lib + 9 cli pass, clippy 클린(no-default 포함).** 스키마 **v5**(created_at).
- 라이브 검증: codex pull e2e, step 5c recency(/explain), 임베딩 qwen3 vs bge-m3 비교(수동), 실코퍼스 회귀(lindera 결정적).
- 배포: `dist plan` dry-run OK(v0.1.0 6바이너리+installer+formula), **릴리스 미발행**.

## ④ 남은 항목

- **공개 릴리스**: 도그푸딩 후 동구님이 `git tag v0.1.0 && git push origin v0.1.0`. 크로스컴파일 리스크(rusqlite bundled C·rustls ring, aarch64-linux)는 첫 CI서 확인.
- **맥 실행 확인**: 맥에서 git pull→build/run, Kiwi 자동다운로드 여부(안 되면 lindera 폴백).
- **온보딩 Stage 4 doctor**: claude/codex·Ollama·Kiwi·포트·코어도달 프리플라이트(미착수).
- **abstraction/anchors 생성 파이프라인**: 컬럼·set_annotation만 있고 생성/소비 로직 없음. 결정적=carry_forward와 중복 저가치, 에이전트 요약이 진짜 가치나 큼 → **보류(YAGNI)**. 실사용서 "retrieved 주입 무겁다" 신호 시 착수.
- **분산 라이브(맥↔윈도우)**: serve/join + bearer + 네트워크. loopback까진 검증, 크로스머신 스모크 필요. 코어 홈랩(homelab-proxy) 호스팅은 별도 트랙(보류).
- step 5c recency 유기 검증(며칠 실데이터) · opencode 검색 배선.

## ⑤ 방향 정본 / 참고

- 배포·온보딩: [docs/design/v2-deploy-onboarding_2026-07-02.md]. 크로스머신: [docs/reference/dev-mac-windows.md].
- 검색/맥락: [docs/design/v2-temporal-validity-direction_2026-07-01.md] + [docs/design/v2-A2A-core-backend_2026-06-30.md]. spec: [docs/design/tunaRound-v1-design_2026-06-29.md].
- 규율: 비trivial 전 plan+checklist·notes(#7). 구현 위임=Sonnet, Opus 리뷰(단 Sonnet5 토큰비용 Opus급이라 대량정독·격리·병렬 이득 큰 것만 선별). 검증과 commit 분리. cargo=Bash. 한국어 마침표(#5)·새파일 첫줄 역할주석(#6)·em-dash 금지. 배포 전 도그푸딩.

## ⑥ 핵심 파일 지도 (이 세션 신규/변경)

- `src/config.rs`(신규): tunaround.toml 파싱·프로파일 선택·병합.
- `src/main.rs`: clap Cli/Commands + 서브커맨드 매핑 + 프로파일 병합(profile_capable 게이트).
- `src/search/mod.rs`: `loanword_aliases` + LOANWORD_GROUPS. `src/search/tokenizer.rs`: fts_query 음역 확장.
- `src/store/sqlite.rs`: 스키마 v5(created_at) + get/set_created_at + save_session created_at 보존. `src/store/retriever.rs`: rerank recency(정책 A) + debug_retrieve created_at/recency.
- `src/store/embedding.rs`: `OllamaEmbedder::from_env`(기본 qwen3). `src/runner/{codex,claude}.rs`: mcp-search 서브커맨드 spawn + codex bypass/bearer.
- `dist-workspace.toml`·`.github/workflows/release.yml`·`Cargo.toml`(메타+license)·`LICENSE`·`tunaround.toml.example`.
