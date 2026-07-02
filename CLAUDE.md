# tunaRound - Claude Code Handoff

> 이 파일은 다음 세션이 이어가기 위한 핸드오프입니다. 제품/설계 전모는 [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md)(현행 spec).

## 표기 / 작업 규칙 (tuna 생태계 공통)

- 사용자 응답·문서는 **한국어 존댓말**. **em-dash 사용금지**(일반 대시 `-` 또는 콜론 `:`). ANSI 박스 드로잉 자제.
- 도메인 도착 URL/도메인은 비노출(소스공개, 서비스 비공개).
- 구현 위임은 **Sonnet 서브에이전트**(codex 비사용), Opus가 스펙·리뷰·검증.
- 한 세션 한 목적. 검증(build/test)과 commit/push는 분리.

## 개발 행동 규율 (이 프로젝트 실험 적용, 2026-06-29)

> 전역 규칙 아님. 이 레포 실험 적용. **전문·근거·예시·위임 라우팅은 [docs/reference/development-guidelines.md](docs/reference/development-guidelines.md)**.
> 10개 중 #1·#2·#3·#4·#8·#9·#10은 전역 COMMON.md가 이미 always-on으로 강제하므로 여기 중복하지 않는다. 아래는 이 프로젝트 신규 3개만 둔다.

- **#5 한국어 문장 끝은 마침표.** 리스트/예시 앞이라도 `:`로 끝내지 않는다. 콜론은 라벨·key-value·문장 중간만.
- **#6 새 소스 파일 첫 줄 = 역할 한국어 한 줄 주석.** Rust 예: `// 토론 라운드 프롬프트를 조립하는 순수 함수`. config 파일 제외.
- **#7 비trivial 작업 전 plan + `checklist.md` + `context-notes.md`.** plan만 주고 코딩 요청 시 멈추고 checklist·notes 먼저 만들지 묻는다.

## 현재 상태 (2026-07-02, 세션 5, 맥 왕복 준비)

- **세션 5: 시간성·유효성 마무리(step 5c·6) + codex pull 활성화(behavioral) + 외래어 병기 색인 + 임베딩 기본 qwen3 + 배포(cargo-dist)·온보딩(clap 서브커맨드·tunaround.toml 프로파일) + AGPL-3.0 + 맥-윈도우 핸드오프.** 전부 origin/main 푸시(= c89da05).
- **이전 세션 4: Stage 3a-3(front=core) + Stage 3d(원격 쓰기 권위) + 시간성·유효성 로드맵 step 2~8.**
  - **3a-3**: `--core <addr>` 단일 프로세스(REPL+in-process HTTP MCP 코어). **서버=전용 OS 스레드 block_on**(공유 rt spawn은 유휴 중 간헐 신뢰불가). 라이브 e2e.
  - **3d(옵션 B front=core 병합)**: `append_turn`(증분·DB id 권위) + `post_turn`/`get_roster` MCP + REPL core-sync(adopt+append, 클로버 차단). 라이브 e2e: 원격 post_turn→흡수→claude 인용.
  - **로드맵(외부 memory 프레임워크 리뷰 후, SQLite-light·graph DB 비채택)**: step2 model_id 무효화키(실버그) · step3 retrieved 길이·세션 다양성 cap · step4 message_validity 테이블(스키마 v4) · step5 유효성 랭킹+/supersede·/reject · step5b 분기/세션 인지 랭킹 · step7 /explain 디버그 · step8 --reindex.
- **v1 + v2 검색/맥락 로드맵(step 2~8) + Stage 3a~3d + codex pull(behavioral) + 실코퍼스 회귀(step6) + 외래어 병기 + 임베딩 qwen3 + 배포/온보딩(clap·cargo-dist·프로파일) 완성.** 검증: **기본 184 lib+6 cli / `--features "semantic morphology mcp serve"` 198 lib+9 cli pass, clippy 클린(no-default 포함).** 스키마 **v5**(created_at).
- 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md). 진행: [docs/plans/index.md](docs/plans/index.md).
- **>>> 최신 핸드오프: [docs/prompts/v2-handoff_2026-07-03_mac-rc1.md](docs/prompts/v2-handoff_2026-07-03_mac-rc1.md) 먼저 읽기 <<<**(맥: v0.1.0-rc.1 발행 + 티키타카). 이전 [session5](docs/prompts/v2-handoff_2026-07-02_session5.md). 맥↔윈도우 왕복은 [docs/reference/dev-mac-windows.md](docs/reference/dev-mac-windows.md).
- **⚠️ Cargo.toml `version="0.1.0-rc.1"`**(rc 발행용). **최종 v0.1.0 태그 전 `0.1.0`으로 되돌릴 것.** 프리릴리스 v0.1.0-rc.1 live(CI green, 4타깃). 릴리스 교훈=[dev-mac-windows §6](docs/reference/dev-mac-windows.md). 정본 방향: [배포·온보딩](docs/design/v2-deploy-onboarding_2026-07-02.md) + [A2A](docs/design/v2-A2A-core-backend_2026-06-30.md) + [시간성·유효성](docs/design/v2-temporal-validity-direction_2026-07-01.md). 이전: [session4](docs/prompts/v2-handoff_2026-07-01_session4.md)
- **⚠ 서버 호스팅 교훈**: `--core`(=`core` 서브커맨드)는 메인이 동기 블로킹 REPL이라 서버를 **전용 스레드 block_on**으로 서빙(공유 rt spawn 신뢰불가). 라이브 e2e 타이밍 함정(Kiwi ~3초/FIFO 미flush/agent ~35초) → 준비 폴링 + 파이프 입력 + 넉넉한 타임아웃.
- **남은 항목**: 공개 릴리스=**`v0.1.0-rc.1` 먼저**(맥 도그푸딩 판정: 6타깃 CI 미검증이라 rc로 CI 검증 후 최종 태그. 상세 [release-readiness](docs/reference/release-readiness-v0.1.0_2026-07-02.md)) · 온보딩 Stage 4 doctor · abstraction/anchors 생성 파이프라인(보류=YAGNI) · **분산 크로스머신 스모크=claude leg 통과**(맥.184→윈도우.179 read_transcript 실증 2026-07-02), codex leg는 승인취약(#24135)→app-server(3e) 후속 · 홈랩 코어 호스팅(보류) · opencode 검색 배선.
- **맥 검증(2026-07-02, 완료)**: 맥 aarch64 빌드·테스트·`cargo install`·2에이전트 도그푸딩 전부 통과(크로스컴파일 이슈 없음). Kiwi 자산 404→lindera 폴백 정상.
- **완료된 이전 남은항목**(참고): step 5c·6·codex pull·codex bearer-env·잠재리뷰(bounded bus/snapshot log/Kiwi 주석) 전부 이번 세션에 처리됨.
- **검증/주의:** 임베딩=원격 Ollama(SSH `-p [사설포트]` 터널, dim 1024). 기본 모델 `qwen3-embedding:0.6b`(bge-m3보다 hybrid MRR 우위 측정), `TUNAROUND_EMBED_MODEL`로 교체. Redis 6379(3d/랭킹엔 불요).
- **Kiwi(정정 2026-07-02):** kiwi-rs 0.1.4는 **순수 Rust 빌드**(dep=regex만, build.rs·네이티브 링크 없음)라 **macOS/Win/Linux 모두 빌드됨**(kiwi cfg는 linux-aarch64만 제외). libkiwi(.dll/.dylib/.so)+모델은 **런타임에 bab2min/Kiwi 릴리스에서 다운로드**(캐시=OS cache dir 또는 `KIWI_LIBRARY_PATH`/`KIWI_MODEL_PATH`/`KIWI_RS_VERSION` env). 과거 "libkiwi 404"는 빌드가 아니라 런타임 자산 다운로드 실패(버전/자산)로, `scripts/install-kiwi-windows.sh`(Windows 전용)가 캐시를 pre-seed해 우회(맥/리눅스 전용 스크립트는 없음 - 자동다운로드 또는 lindera 폴백). 실패해도 **lindera 자동 폴백**이라 빌드·실행 안 죽음. **맥(aarch64) 실측 2026-07-02: libkiwi 0.23.1 dylib 로드 실패 + kiwi_mac_arm64_v0.23.2.tgz 자산 404 -> lindera 폴백으로 정상 동작.** bab2min/Kiwi v0.22.2에 맥 자산 존재(`kiwi_mac_arm64`/`kiwi_mac_x86_64`).

## 무엇을 만드나 (요약)

터미널에서 **사용자가 운전하는 역할 부여 2-에이전트(Claude Code·Codex) 착수 전 설계 토론** 도구. 같은 레포 위에서 사람 주도로 토론하고, 결론을 **결과 문서로 자동 기록**해 구현으로 넘긴다.

**핵심 결정(brainstorming 2026-06-29):** 사람 주도 대화형 / 공유 컨텍스트 = 같은 레포+공유 문서(컨텍스트팩 없음) / 읽기 전용 화자 + 사람이 쓰기 지목 / 순차-인지 턴 / 자리마다 역할 주입 / v1=2자리 고정 / consensus carry-forward(종료는 사람) / 스택 Rust+tokio.

**레이어(출처):** 에이전트 러너(tunaFlow `claude.rs`/`codex.rs` 포팅) + 토론 오케스트레이터(tunapi `core/roundtable/` 청사진 -> Rust 재구현) + 전사·영속(파일/rusqlite, 트리-ready) + 프론트(thin REPL).

**v1 비목표 -> v2:** Redis 멀티세션 = git-tree 다중 브랜치 / N>2 좌석 로스터(로컬LLM·opencode) / 리치 TUI(ratatui)·웹 / 협업 코딩.

## 출처 레포 (포팅 시 읽기)

- **tunapi**(전전신, Python): `~/privateProject/tunapi/src/tunapi/core/roundtable/` - 토론 오케스트레이터 청사진(`orchestrator.py`/`prompt.py`/`rt_participant.py`/`session.py`). 역할·순차-인지·follow-up·consensus.
- **tunaFlow**(Rust): `~/privateProject/tunaFlow/src-tauri/src/agents/{claude,codex}.rs` - CLI 러너(`stream_run`) + hardening.
- **tunaSalon**(Rust, v2용): `src/session_bus.rs`(Redis), `src/chat.rs`의 `render_chat`(ratatui), `src/flow.rs`(FlowMeter, 선택).

## 다음 세션 첫 행동

1. **[docs/prompts/v2-handoff_2026-07-02_session5.md](docs/prompts/v2-handoff_2026-07-02_session5.md) 먼저 읽기** + `context-notes.md`(하단) + `checklist.md` + `docs/plans/index.md`. 맥↔윈도우 왕복이면 [docs/reference/dev-mac-windows.md](docs/reference/dev-mac-windows.md)도. `cargo test`(기본) + `cargo test --features "semantic morphology mcp serve"`로 상태 확인(**cargo는 Bash 툴로**).
2. **다음 세션 우선순위(2026-07-02 오후 확정)**: (a) **rc.1 CI green 확인**(맥이 잡는 중 - aarch64 크로스 ring C 실패로 4타깃 축소·버전·profile.dist 수정. **윈도우 미개입**) → green 시 최종 v0.1.0 태그·tap 발행 판단. (b) **codex 먼저 해결**(크로스머신 codex leg가 #24135 승인취약으로 실패 = app-server(3e) 또는 대화형 승인). (c) codex 풀리면 **맥이랑 크로스머신 스모크 이어가기**(codex leg·양방향 post_turn). (d) 선택: 진짜 A2A=AutoLoop(Stage4, 미구현·모더레이터 에이전트). 실행은 `cargo run -- chat`(서브커맨드). Kiwi 자동다운로드(실패 시 lindera). Ollama 11435, 기본 qwen3-embedding:0.6b, Redis 6379.
   - **A2A 성숙도(정직)**: 현재=공유맥락(데이터평면)+**사람 오케스트레이션(HITL)** = **semi-a2a**(자율수준이 "semi"=HITL, A2A 통신은 진짜 성립). 스펙트럼: 수동relay < semi-a2a < full-auto(AutoLoop=Stage4 미구현, 의도적 보류). 크로스머신 앱-투-앱 위임 설계=docs/design/v2-a2a-partner-delegation_2026-07-02.md.
3. 작업 추적 `checklist.md`·`context-notes.md`(규율 #7). 위임 Sonnet + Opus 리뷰. 굵직한 결정 재론 금지. 서브에이전트 진행 중 파일 레이스 주의. 배포 전 도그푸딩.
