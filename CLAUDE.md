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

## 현재 상태 (2026-07-01, Windows 세션 4)

- **세션 4: Stage 3a-3(front=core) + Stage 3d(원격 쓰기 권위) + 시간성·유효성 로드맵 step 2~8.** 전부 origin/main 푸시(= 3071281).
  - **3a-3**: `--core <addr>` 단일 프로세스(REPL+in-process HTTP MCP 코어). **서버=전용 OS 스레드 block_on**(공유 rt spawn은 유휴 중 간헐 신뢰불가). 라이브 e2e.
  - **3d(옵션 B front=core 병합)**: `append_turn`(증분·DB id 권위) + `post_turn`/`get_roster` MCP + REPL core-sync(adopt+append, 클로버 차단). 라이브 e2e: 원격 post_turn→흡수→claude 인용.
  - **로드맵(외부 memory 프레임워크 리뷰 후, SQLite-light·graph DB 비채택)**: step2 model_id 무효화키(실버그) · step3 retrieved 길이·세션 다양성 cap · step4 message_validity 테이블(스키마 v4) · step5 유효성 랭킹+/supersede·/reject · step5b 분기/세션 인지 랭킹 · step7 /explain 디버그 · step8 --reindex.
- **v1 완료 + v2 Plan 01~20 + 3a-2 + 세션4(3a-3·3d·step2~8) 완성.** 검증: **기본 160 / `--features "semantic morphology mcp serve"` 174 pass, clippy 클린.** 스키마 v4.
- 현행 spec: [docs/design/tunaRound-v1-design_2026-06-29.md](docs/design/tunaRound-v1-design_2026-06-29.md). 진행: [docs/plans/index.md](docs/plans/index.md).
- **>>> 최신 핸드오프: [docs/prompts/v2-handoff_2026-07-01_session4.md](docs/prompts/v2-handoff_2026-07-01_session4.md) 먼저 읽기 <<<** (3a-3·3d·시간성유효성 + 서버 호스팅 교훈 + 남은 항목). 정본 방향: [A2A](docs/design/v2-A2A-core-backend_2026-06-30.md) + [시간성·유효성](docs/design/v2-temporal-validity-direction_2026-07-01.md). 이전: [session3](docs/prompts/v2-handoff_2026-06-30_session3.md)
- **⚠ 서버 호스팅 교훈**: `--core`는 메인이 동기 블로킹 REPL이라 서버를 **전용 스레드 block_on**으로 서빙해야 함(공유 rt spawn 신뢰불가). 라이브 e2e 디버깅 시 타이밍 함정(Kiwi ~3초 기동/FIFO 미flush/agent ~35초) 주의 → 준비 폴링 + 파이프 입력 + 넉넉한 타임아웃.
- **남은 항목**: step 6 실코퍼스 regression(실제 전사 코퍼스 확보 선행, 코드만으론 불가) · step 5c recency(messages에 created_at 컬럼) · abstraction/anchors 생성 파이프라인 · codex bearer-env·codex pull 활성화 · 잠재리뷰(unsafe Send Kiwi·session_bus unbounded).
- **검증/주의:** 임베딩=원격 Ollama(SSH `-p [사설포트]` 터널, dim 1024). 기본 모델 `qwen3-embedding:0.6b`(bge-m3보다 hybrid MRR 우위 측정), `TUNAROUND_EMBED_MODEL`로 교체. Redis 6379(3d/랭킹엔 불요).
- **Kiwi(정정 2026-07-02):** kiwi-rs 0.1.4는 **순수 Rust 빌드**(dep=regex만, build.rs·네이티브 링크 없음)라 **macOS/Win/Linux 모두 빌드됨**(kiwi cfg는 linux-aarch64만 제외). libkiwi(.dll/.dylib/.so)+모델은 **런타임에 bab2min/Kiwi 릴리스에서 다운로드**(캐시=OS cache dir 또는 `KIWI_LIBRARY_PATH`/`KIWI_MODEL_PATH`/`KIWI_RS_VERSION` env). 과거 "libkiwi 404"는 빌드가 아니라 런타임 자산 다운로드 실패(버전/자산)로, `scripts/install-kiwi-*.sh`가 캐시를 pre-seed해 우회. 실패해도 **lindera 자동 폴백**이라 빌드·실행 안 죽음. bab2min/Kiwi v0.22.2에 맥 자산 존재(`kiwi_mac_arm64`/`kiwi_mac_x86_64`).

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

1. **[docs/prompts/v2-handoff_2026-06-30_session2.md](docs/prompts/v2-handoff_2026-06-30_session2.md) 먼저 읽기** + `context-notes.md` + `docs/plans/index.md`. `cargo test`(기본) + `cargo test --features "semantic morphology mcp"`로 상태 확인(**cargo는 Bash 툴로**).
2. 검색/맥락 북극성은 1차 완결(Plan 09~19). 다음 = 핸드오프 ⑤의 남은 항목(opencode CLI 참가자 / 검색 품질 추가 개선 / ctx-handle 요약 / 리치 프론트 보류) 중 사용자 지정으로 착수. Kiwi는 v0.22.2 수동 설치(`scripts/install-kiwi-windows.sh`), 미설치 시 lindera 폴백. Ollama 터널 11435(SSH -p [사설포트]), Redis 6379.
3. 작업 추적 `checklist.md`·`context-notes.md`(규율 #7). 위임 Sonnet + Opus 리뷰. 굵직한 결정 재론 금지. 서브에이전트 진행 중 git add -A 레이스 주의.
