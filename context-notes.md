# tunaRound 컨텍스트 노트

> 작업 중 결정과 근거. 계속 append. (규율 #7) 다음 세션이 결정을 재유도하지 않게.

## 2026-07-02 세션6(후반): semi-a2a 파트너 위임 설계 확정 (A2A 표준 채택)

- **경위**: Stage 3e(codex app-server, #24135) 논의가 동구님 질문들로 값이 해체 → **3e 킬**. 대신 진짜 값 = **크로스머신 앱-투-앱 semi-autonomous 위임**. 설계: docs/design/v2-a2a-partner-delegation_2026-07-02.md.
- **용어**: "half-a2a"→**"semi-a2a"**(자율수준=HITL, A2A는 진짜 성립; half=미완성 오독이라 폐기). 스펙트럼: 수동relay < semi < full-auto(AutoLoop=Stage 4 보류). README·CLAUDE.md 정정 후속.
- **경계(동구님)**: 토론=단일머신 헤드리스로 충분(크로스머신 불요). 개발협업=git+공유레포. 크로스머신 **헤드리스는 out**, **앱-투-앱 위임만 값**(#24135 무관=사람 승인).
- **표준 A2A 채택**(bespoke 아님). 이기종 파트너 interop이 A2A 존재이유. A2A(에이전트↔에이전트)+MCP(에이전트↔도구) 보완. 스펙 a2a-protocol.org v1.0(2026 LF): Task 8-state·SendMessage/GetTask·Agent Card·Part/Artifact.
- **worker=CLI 에이전트, 모델=config**(동구님 교정): headless 모델 어댑터 불필요. "Ollama 파트너"=Ollama 구동 CLI 에이전트(Codex 네이티브 OpenAI-compat, Claude Code는 프록시). agentic loop 때문에 raw 모델호출 아닌 CLI 에이전트. HTTP engine runner(Plan 17)=토론 좌석용 유지.
- **토폴로지=중앙 브로커**: 코어=A2A서버+task큐, worker=/loop+inbox MCP툴(poll/claim/complete) 폴링, dispatcher=A2A SendMessage/GetTask. 대화형 CLI가 per-agent 서버 못 띄워서. dispatch측 A2A 호환. SSE 후속.
- **합성**: task contextId↔session, worker가 read_transcript로 맥락 pull(또는 Message parts로 push=#24135 회피).
- **착수**: Phase 1 Task 1(A2A 데이터모델+tasks테이블 v6)부터, Sonnet 위임+Opus 리뷰.

## 2026-07-02 세션6: rc.1 CI green + Windows rc 아티팩트 검증

- **rc.1 CI 완전 green**(run 28564666085, 태그 v0.1.0-rc.1 = 19f3ce0): plan → 빌드 4타깃(mac arm64 3m45s / mac x86 8m39s / win x64 11m24s / **linux x64 7m4s**) → global artifacts → host 발행(28s) 전부 ✓. **크로스컴파일 리스크(ring C·rusqlite bundled)가 linux x64에서 실증 통과**(rc.1의 존재 이유였음). GitHub prerelease 발행됨(isDraft=false, isPrerelease=true), 아티팩트 15개 = 4바이너리 + sha256 + installer.ps1/sh + tunaround.rb(homebrew formula) + source.
- **Windows rc 아티팩트 검증(수동 다운로드+실행, CI 미접촉)**: sha256 발행값과 일치 · 번들(tunaround.exe 46MB + LICENSE(AGPL 전문) + README + CHANGELOG) · `--version` = 0.1.0-rc.1 · 전 서브커맨드(chat/core/serve/join/mcp-search/reindex)+플래그(--pull-context/--search-url/--config/--profile 등) 노출 = **semantic/mcp/serve/sqlite 피처가 릴리스 빌드에 실제 컴파일됨 확인**. win x64 바이너리 양호.
- **⚠ 실발견(공개 설치 경로 게이트)**: `tunaround-installer.ps1`(및 sh/homebrew) 익명 다운로드는 **레포가 private이라 404**(Net.WebClient·Invoke-WebRequest 둘 다, 릴리스 페이지 자체도 익명 404). `gh release download`(인증 토큰)만 성공. **스크립트·아티팩트 결함 아님** — 아티팩트명(tunaround-x86_64-pc-windows-msvc.zip)·URL·메커니즘 전부 정확. **함의: 공개 설치 경로(ps1/sh/brew) 전부 레포 public 전환 전엔 작동 불가**(릴리스 download URL을 익명으로 치기 때문). homebrew-tap이 public이어도 release asset이 private면 `brew install`도 404. 이건 설계 의도(소스공개=릴리스 행위)이자 **동구님 go/no-go**. **진짜 installer/brew 테스트는 레포 public 후에만 가능**, 그 전 Windows 최대 검증치 = 아티팩트 무결성+실행(= 통과).
- 릴리스 이름 `0.1.0-rc.1 - 미발행` = CHANGELOG.md 헤딩 플레이스홀더("미발행")를 cargo-dist가 릴리스 제목으로 상속. 기능 무관, 최종 v0.1.0 전 헤딩을 날짜로 정리 권장(cosmetic).
- 로컬 상태: main=origin/main(c59be32), src/ 무변경(c89da05 이후 전부 docs·CHANGELOG·Cargo.toml 버전/profile.dist·dist 설정) = 테스트 베이스라인 184+6 / 198+9 유효(재실행 불요). CI는 맥 주도 유지(윈도우 미개입).
- **사설 IP 전방 redact((나) 단계, 세션6)**: 홈 서버 공인 IP·DDNS 호스트명이 tracked 문서 4곳(context-notes·session2/3/4 핸드오프)+히스토리에 있던 것 발견. 트리에서 `[사설IP]`/`[사설호스트]` placeholder로 치환, 실값은 gitignored `docs/reference/backend-private.md`로 이관(+.gitignore 등록). 계정(`d9ng`)·포트(`2232`)는 non-secret라 유지(계정만으론 접속 불가, 보안=키). **전방 정리만** — 과거 히스토리엔 잔존(레포 private라 저위험). 공유/공개 결정 시에만 `git filter-repo` 히스토리 퍼지(맥 조율=rc.1 태그·발행 릴리스 재생성 동반). 배포 자체가 이 프로젝트 비우선(동구님).

## 2026-07-02 Stage 3 tunaround.toml + 프로파일 완료 (Sonnet5 구현, 미커밋)

- **설계 기준**: docs/design/v2-deploy-onboarding_2026-07-02.md §2 설계 B. checklist.md에 이미 스텁이 있던 항목이라 별도 plan 문서 없이 그 스펙(위임 프롬프트)을 그대로 plan으로 취급하고 착수(체크리스트·컨텍스트노트 규율 #7은 이미 만족된 상태로 판단).
- **신규 모듈 `src/config.rs`**: Config{default_profile, profile: HashMap<String,Profile>} / Profile(전부 Option: db·roster·recent_turns·pull_context·session·search_url·search_token·search_token_env). parse_config/load_config_file/discover_config_path(명시>./tunaround.toml>~/.config/tunaround/config.toml, 명시인데 없으면 Err)/load_config. expand_home(HOME 우선, 없으면 USERPROFILE, 외부crate 0). resolve_search_token(평문 우선, 없으면 *_env로 std::env::var).
- **select_profile 시그니처는 스펙 그대로**: `fn select_profile<'a>(cfg: &'a Config, requested: Option<&str>, interactive: bool) -> Result<Option<&'a Profile>, String>`. HashMap 순회 순서가 불안정하므로 "다중 프로파일" 케이스는 항상 이름 정렬 후 결정. interactive=false면 정렬된 첫 이름(테스트 결정적), interactive=true면 실제 stdin 픽커.
- **판단 갈린 지점 1(대화형 픽커 아키텍처)**: 스펙 문구("이 stdin 읽기는 select_profile 밖에서 하고, 선택 로직만 순수함수로")를 문자 그대로 읽으면 stdin이 main.rs에 있어야 하는데, select_profile의 반환타입(`Result<Option<&Profile>, String>`)엔 "대화형 필요" 시그널을 실어보낼 자리가 없어 물리적으로 불가능(콜백 파라미터도 스펙 시그니처엔 없음). **해석**: "밖에서"를 "핵심 결정 로직 바깥의 별도 함수로 분리"로 읽어, select_profile은 그대로 두고 내부에서 다중+비default+interactive 분기에서만 `prompt_profile_pick`(실제 stdin, println+read_line)을 호출하게 하고, 그 안에서 다시 순수 `match_profile_pick(input, names)`(번호/이름 매칭)를 호출하도록 3단 분리. 테스트는 select_profile의 결정적 케이스(설정없음/지정/default/단일/다중-비interactive) + match_profile_pick 자체(번호·이름·범위밖·빈입력)만 커버 = "대화형 stdin은 순수 선택 로직만 테스트" 요구를 문자 그대로 만족.
- **판단 갈린 지점 2(병합을 순수함수로 분리)**: main.rs 지역변수 직접 mutate로도 스펙을 만족하지만, "merge 우선순위(CLI>프로파일) 단위테스트" 요구를 main() 안에서 검증할 방법이 없어 `MergedSessionArgs`(db/roster/recent_turns/pull_context/session/search_url/search_token) + `merge_profile_into(cli, Option<&Profile>) -> MergedSessionArgs` 순수함수로 뽑음(선제설계 5원칙 #3 "분기·계산은 순수함수로"와도 정합). main.rs는 이 구조체를 조립→호출→분해만 한다. pull_context는 스펙대로 OR, 나머지는 `cli.is_none()`이면 프로파일 값(경로류는 expand_home 통과).
- **main.rs 배선**: CommonSessionArgs(chat+core가 flatten 공유)와 JoinArgs 양쪽에 `--config`/`--profile` 추가. 기존 `let db_path: Option<String>;`(단일 대입 관례)를 `let mut`으로 변경(병합 단계에서 2차 대입 필요). 신규 로컬 `profile_capable: bool`(Chat/Core/Join 분기에서만 true)로 병합 블록 전체를 게이트 → serve/mcp-search/reindex는 tunaround.toml이 cwd에 있어도 완전히 무시(스펙 요구 그대로, auto-discovery도 미적용).
- **에러 처리**: `--profile` 지정했는데 설정 자체가 없음 → 안내 후 exit(1). `--profile`이 맵에 없음 / `default_profile`이 가리키는 이름이 맵에 없음 → 둘 다 Err→exit(1)(default_profile 오탈자를 조용히 무시하지 않음, 스펙엔 명시 안 됐으나 안전한 기본값으로 판단).
- **테스트 전략(env/파일 I/O 안정성)**: `std::env::set_var`는 edition 2024라 `unsafe`(session_bus.rs 기존 컨벤션 그대로 답습, 동일 주석 문구). 파일 존재 탐색 테스트는 cwd(`./tunaround.toml`)를 직접 건드리지 않고 `std::env::temp_dir()`에 유니크 파일명으로 생성/삭제(CI·병렬테스트 안전). `discover_config_path`의 "명시 경로 없을 때 cwd/home 탐색" 분기 자체는 자동테스트 미커버(cwd 오염 리스크 회피 목적, first_existing 순수함수 테스트로 핵심 로직은 커버됨) — 코드리뷰로 갈음.
- **⚠ 실발견(레이스) + 수정**: 처음엔 "단일 테스트 함수 안에서 HOME을 저장→변경→복구"로 테스트 간 레이스를 피했다고 판단했으나, `expand_home_variants`와 `merge_profile_into_fills_unset_fields_from_profile` **두 개의 서로 다른 테스트 함수**가 각자 HOME을 건드려 cargo test 기본 병렬 실행(멀티스레드, 환경변수는 프로세스 전역)에서 실제로 레이스 발생(`cargo test --lib config::` 단독 실행 시 1/1 재현: "둘 다 없으면 원본" 케이스가 실제 Windows USERPROFILE 값을 봄). **수정**: `static ENV_LOCK: Mutex<()>`를 테스트 모듈에 추가하고 HOME을 건드리는 두 테스트(+ 일관성 위해 토큰-env 테스트도) 시작 시 `ENV_LOCK.lock()`으로 직렬화. 수정 후 5회 연속 + 전체 스위트 2회 연속 재실행으로 안정성 확인. **교훈**: env var를 건드리는 테스트가 파일 내 1개뿐일 때만 "단일 테스트 함수 내 저장/복구"로 충분하고, 2개 이상이면 처음부터 공유 락이 필요(session_bus.rs는 현재 1개뿐이라 우연히 안전했던 것).
- **문서**: `tunaround.toml.example`(레포 루트, 플레이스홀더 도메인/토큰) + `.gitignore`에 `/tunaround.toml` 추가(실값 커밋 방지, 서비스 비공개 원칙과 정합) + README "설정 프로파일" 섹션 + dev-mac-windows.md 경로 설명 갱신 + 상태 라인(Stage 3 구현완료·리뷰대기로 갱신).
- **검증**: 기본 184(lib)+6(main) / 풀피처 198(lib)+9(main) pass, 신규 실패 0. clippy 프로젝트 표준커맨드(기본/풀피처/no-default, `--all-targets` 없이) 0경고. `--all-targets`로 보면 claude.rs/repl-mod.rs에 기존 경고 2건이 뜨지만 이 세션 변경과 무관(사전 존재, 미접촉 파일).
- **미커밋**: Opus 리뷰 후 커밋 예정(지시 준수).

## 2026-07-01 step 8 완료: --reindex/lint (Plan 33)

- **`--reindex` 서브 모드**(sqlite): --db 필수. 모든 세션 load_session → save_session(현재 fts 토크나이저로 FTS 재생성) → index_vectors(semantic이면 재임베딩; step 2 model_id 키로 모델 교체 시 갱신). 전후 인덱스 stats 출력. 모델·토크나이저·스키마 교체 후 복구 경로.
- SqliteStore::list_sessions + index_stats(sessions/messages/fts/vectors/validity 카운트, lint 리포트).
- **검증**: 기본 160 pass, clippy 클린. list_sessions/stats + reindex FTS 재생성 테스트. 라이브 스모크(빈 DB stats, --db 없이 에러).
- **로드맵 완료(step 1~8, 5b 포함).** 남은 것: step 6(실코퍼스 regression - 실제 전사 코퍼스 확보 선행 필요) · 5c(recency, 메시지 타임스탬프 컬럼 필요) · abstraction/anchors 생성 파이프라인.

## 2026-07-01 step 7 완료: /explain 검색 디버그

- **ContextRetriever::debug_retrieve(query, limit, current_session) default 메서드**(기본은 결과 목록만). SqliteRetriever가 리치 버전: 질의→**토큰화(fts_query 결과)**→후보별 [msg_id, session, **bm25 점수**, valid_state, cur-session 표시] + 스니펫. 한국어 토큰화·랭킹 디버깅 가시성.
- REPL `/explain <질의>` 커맨드(--db 필요). /help 갱신.
- **검증**: 기본 158 pass, clippy 클린. debug_retrieve가 토큰화·bm25·유효성·현재세션 표시 확인.
- 다음 = step 8(reindex/lint 명령).

## 2026-07-01 step 5b 완료: 분기/세션 인지 랭킹 (Plan 32)

- **문제(아키텍트 리뷰 약점3)**: 검색이 분기 비인지 → checkout으로 버려진 분기 발언이 retrieve로 끌려옴.
- **수정**: ContextRetriever에 `retrieve_ctx(query, limit, current_session)` **default 메서드**(기본 retrieve 위임 → 다른 impl/MCP ripple 없음). SqliteRetriever가 penalty 기반 재랭크로 통합: rejected 드롭 / superseded·stale +2 / **현재 세션 off-branch +1**(활성경로 콘텐츠는 repl이 이미 제외하므로 남은 현재-세션 히트 ≈ 버려진 분기). 안정 정렬로 relevance 순서 보존. repl이 retrieve_ctx(topic, K, session_id) 호출.
- **검증**: 기본 157/features 167 pass, clippy 클린. 현재세션 off-branch가 타세션보다 뒤로, 컨텍스트 없는 retrieve 불변.
- **recency는 후속(5c)**: 메시지 타임스탬프 컬럼 없음(msg_id는 세션별이라 cross-session 비교 불가) → messages에 created_at 추가 필요. 다음 = step 7(/search --debug).

## 2026-07-01 step 5 완료: 유효성 인지 검색 랭킹 + 지정 커맨드 (Plan 31)

- **랭킹(SqliteRetriever)**: 후보에 rerank_by_validity 적용 - **rejected 드롭, superseded/stale은 active 뒤로 강등**(순서 보존), active/unknown/미설정은 유지. FTS단독·RRF·폴백 모두. 유효성 미설정 시 동작 불변.
- **커맨드(HITL)**: `/supersede <id> [<대체id>]` · `/reject <id>`. ValiditySink 트레잇 + SqliteValiditySink + Session.validity_sink, main이 --db로 배선. 미배선 시 안내. mark_validity가 발언 존재 확인 후 set_validity 호출.
- **범위**: valid_state 축만. recency/current-session/active-branch 가중은 retrieve에 컨텍스트 전달(트레잇 변경) 필요 → step 5b로 분리. abstraction/anchors 생성·활용도 후속.
- **검증**: 기본 156/features 166 pass, clippy 클린. 재랭크(rejected 제외·superseded 강등), 커맨드 파싱, sink 호출/미배선 안내 테스트.
- **시간성·유효성 흡수(step 4~5) 완료.** 사람이 /supersede·/reject로 옛/폐기 결정을 표시 → 검색이 자동으로 디프리오리티/제외. 다음 = step 6(실코퍼스 regression) 또는 사용자 지정.

## 2026-07-01 step 4 완료: 유효성 메타데이터 데이터 레이어 (Plan 30)

- **설계 판단**: messages/StoredMessage에 컬럼 추가는 모든 struct 리터럴 붕괴 + 직렬화 하위호환 문제 + Memora 철학(원문/메타 분리) 위배 → **별도 `message_validity` 테이블**로 레이어링. StoredMessage 불변.
- **스키마 v3→v4**: message_validity(session_id, msg_id, valid_state DEFAULT active, superseded_by_msg_id, abstraction, anchors, updated_at). 새 TABLE이라 migrate CREATE IF NOT EXISTS로 fresh·기존 처리.
- **API**: store::Validity 구조체. SqliteStore set_validity(valid_state/superseded, abstraction 보존) · set_annotation(abstraction/anchors 부분 갱신 COALESCE, valid_state 보존) · get_validity(없으면 None=기본 active).
- **검증**: 기본 151 pass, clippy 클린. 라운드트립 + 부분갱신 보존 테스트.
- **step 4 범위 = 데이터 레이어만.** step 5에서: 검색 랭킹 LEFT JOIN(non-active 디프리오리티) + REPL 커맨드(/supersede, /reject)로 사람이 유효성 지정 배선. abstraction/anchors 생성 파이프라인은 더 뒤(컬럼만 준비).
- 다음 = step 5.

## 2026-07-01 step 3 완료: retrieved 길이 cap + session diversity cap (Plan 29)

- **session diversity(SqliteRetriever)**: store.search/vector_search를 `limit*4` over-fetch → `cap_per_session_backfill(max_per_session=2, limit)`. 다중 세션이면 다양화, **단일 세션이면 backfill로 limit까지 채워 동작 불변**(under-fill 없음). FTS단독·RRF·폴백 경로 모두 적용.
- **retrieved 길이 cap(repl)**: `MAX_RETRIEVED_CHARS=2000`. retrieve_for_from_path에서 dedup 후 누적 글자수 초과 발언 드롭(최소 1건 보장, UTF-8 안전).
- **핵심 뉘앙스**: tunaRound 토론은 보통 단일 세션이라 무조건 세션 cap하면 손해 → backfill로 단일 세션 불변 보장.
- **검증**: 기본 149/features 159 pass, clippy 클린. 신규: cap_per_session_backfill(다중 다양성/단일 full-fill), 길이 cap(초과 드롭). eval 하네스는 store.search 직접 호출이라 무영향.
- 다음 = step 4(valid_state/superseded_by/abstraction/anchors 컬럼 = 시간성·유효성 흡수 시작).

## 2026-07-01 step 2 완료: 임베딩 무효화 키에 model_id (실버그 수정, Plan 28)

- **문제**: `index_vectors` 증분 가드가 content_hash(내용만)로 skip → 모델 교체 시 stale 벡터 유지(차원/공간 섞임, 조용한 저하).
- **수정**: Embedder 트레잇에 `model_id()`(Mock=`mock-{dim}`, Ollama=`ollama:{model}`). message_vectors에 `model_id TEXT`(스키마 v2→v3: CREATE에 추가 + migrate ALTER, column_exists 가드). skip은 (content_hash AND model_id) 일치 시만. 모델 바뀌면 재임베딩. 기존 v2 행은 model_id NULL → 다음 색인 때 자동 재임베딩.
- **검증**: 기본 146/features 156 pass, clippy 클린. 신규 테스트: model_id 표기, index_vectors 같은모델 skip/모델교체 재임베딩(카운팅 임베더), fresh DB 컬럼 존재, v2→v3 마이그레이션(수동 v2 스키마 → ALTER + 행 보존). behavior-preserving(모델 동일 시 기존과 동일).
- 다음 = step 3(retrieved 길이 cap + session diversity cap).

## 2026-07-01 Stage 3d 완료 (post_turn 쓰기 권위 + get_roster, 옵션 B front=core 병합)

- **4 태스크 커밋**: T1 `append_turn`(증분 INSERT, DB id 권위) + TranscriptWriter(`d90d867`). T2 MCP post_turn/get_roster 툴 + 서버 배선(`c28561d`). T3 REPL core-sync 병합(step adopt + append_turn, 전량 persist 생략 → 외부 쓰기 클로버 차단)(`f500840`). T4 main --core 배선 + 라이브 e2e(`8a80cfe`).
- **라이브 e2e 성공(결정적)**: 단일 `--core` 프로세스에서 원격 참가자가 실 HTTP MCP로 `post_turn`("추가됨 msg_id=1") → front=core REPL이 core-sync로 흡수 → **claude가 read_transcript로 그 발언을 그대로 인용**("...valid_state 가중...키워드 살구나무"). get_roster가 실 로스터(claude proposer/codex reviewer) 반환. = half-a2a 분산 쓰기 권위 끝까지 동작.
- **⚠ 중요 교훈(서버 호스팅)**: `--core`는 메인 스레드가 동기 블로킹 REPL(std stdin)이라, **공유 rt에 서버를 spawn하면 accept 루프가 유휴 중 간헐적으로만 구동돼 신뢰 불가**(실측: 유휴 4s UP, 6s/8s down). **해결 = 서버를 전용 OS 스레드의 자체 런타임 block_on으로 서빙**(헤드리스 --serve-mcp가 메인 block_on이라 되는 것과 동형). 라운드 중엔 메인이 서브프로세스 대기라 rt 워커가 돌아 에이전트는 작동했으나, 외부 curl/원격 클라이언트는 유휴 중 끊김. 전용 스레드가 둘 다 안정.
- **디버깅 함정 기록**: e2e 실패로 보였던 것들은 대부분 **타이밍/orchestration 아티팩트**였다. (1) Kiwi 토크나이저 init로 서버 기동이 ~3초 걸려 고정 `sleep 3` curl이 레이스. (2) FIFO `printf >&9`가 즉시 flush 안 돼 agent 라인이 close 시점까지 지연. (3) 2-에이전트 라운드가 ~35초라 짧은 타임아웃이 잘림. → 서버는 **준비 폴링** 후 호출, agent 라운드는 **파이프 입력** + 넉넉한 타임아웃(300s).
- **남은(3d 후속)**: codex bearer-env(원격 인증 접속), --core+resume 엣지(seed→DB 권위 반영은 구현했으나 미검증), post_turn 권한(현재 누구나 토큰만 있으면 씀).

## 2026-07-01 시간성·유효성 방향 확정 (외부 memory 프레임워크 검토 후)

- 외부 지형도(Zep/Graphiti·Mem0·Letta·Cognee·Memora·H-Mem/MemORAI/MRAgent) 검토. **결론: 인프라(graph DB·managed service) 안 감, 개념(시간성·유효성·provenance)만 SQLite 컬럼+랭킹가중치로 흡수.** SQLite-light·로컬-first 유지.
- 핵심 진단: provenance는 이미 있음(session_id·msg_id·parent_id·speaker·branch). 빠진 건 valid_from/until·superseded_by = **validity metadata**. 참고 1순위=Memora(원문/abstraction/anchors 분리, 인프라 안 바꾸고 흡수).
- **정본**: docs/design/v2-temporal-validity-direction_2026-07-01.md. 메모리: [[tunaround-temporal-validity-roadmap]].
- **확정 순서**: 1)3d 쓰기권위 2)embed 무효화키(model_id/dim/provider) 3)retrieved 길이·세션다양성 cap 4)valid_state/superseded_by/abstraction/anchors 컬럼 5)branch/session/recency/valid_state 랭킹가중 6)실코퍼스 regression 7)/search --debug 8)reindex/lint. Graphiti 1순위 구현은 과설계(지금 문제=검색 오염, graph traversal 아님).

## 2026-06-30 (세션4) Stage 3a-3 front=core 착수 (Plan 26)

- **목표**: 3a-2의 2프로세스(`--serve-mcp` 코어 + REPL `--search-url`)를 1프로세스로 통합. `--core <addr>` = REPL이 자기 안에서 HTTP MCP 코어를 띄우고 로컬 좌석이 거기에 HTTP pull. 원격 프론트/에이전트도 같은 주소 공존.
- **왜 가벼운가**: 배선 전부 기존. `with_search_url`(3a-2)·`build_registry` 4-arg·`serve_http_mcp_on_listener`(3a-1) 다 있음. 러너 `run` 동기 + REPL 동기라 HTTP 서버를 rt 워커에 spawn하고 메인 블로킹 루프와 공존 가능(확인). 작업 = main.rs `--core` 분기 + `core_local_url` 순수함수.
- **결정**: (1) `--serve-mcp`(헤드리스 순수 서버)는 불변, `--core`(REPL+서빙) 신규 = 의미 분리. (2) bind 동기 선행(rt.block_on bind)으로 포트 경합 fail-fast 후 spawn. (3) 로컬 좌석 URL = addr의 0.0.0.0/[::]→127.0.0.1 + /mcp. (4) `--core`+명시 `--search-url` 동시면 명시 우선(경고). (5) `--db` 필수.
- **동시성 근거**: Runtime::new=multi-thread. rt.spawn(server)=워커 스레드, 메인=블로킹 REPL. REPL indexer가 core.db 쓰고 HTTP reader가 core.db 읽음 = WAL 동시(2프로세스 e2e와 동일, 동일 프로세스 2커넥션). 루프 중 block_on 없음(runner subprocess 동기).
- **구현 완료**: main.rs `--core <addr>` 분기(bind 동기 선행 fail-fast → rt.spawn 서빙 → search_url/token 자동 배선 → REPL). serve 두 분기(`--serve-mcp`/`--core`)가 `build_http_mcp_backends(ctx, db)` 헬퍼 공유(중복 제거). mcp.rs `core_local_url`(0.0.0.0/[::]→127.0.0.1+/mcp) 순수함수+단위테스트. 곁다리: 기존 `mcp_session_id` 미사용 경고(mcp 없는 기본빌드)를 mcp 게이트로 정리.
- **검증**: 기본 137 / serve 146 pass, clippy 클린(기본+serve), 경고 0. **스모크 e2e**: `--core 127.0.0.1:8788 --db core.db --token TOK123 --pull-context` 단일 프로세스가 HTTP MCP 코어 바인드(`서버 기동 127.0.0.1:8788`) + 로컬좌석 자동배선(`http://127.0.0.1:8788/mcp`) + REPL 동시 구동, bearer 인증(no-token 401 / token 200) 확인. 서버 future=rt워커 / REPL=메인 블로킹 공존 실증.
- **풀 라이브 e2e 통과(결정적)**: 단일 `--core 127.0.0.1:8790 --db e2e.db --token TOKE2E --pull-context`로 실 claude+codex 2턴. turn1=사용자가 "이벤트소싱 vs CRUD, 근거=감사추적" 제시(claude+codex 응답). turn2=`@claude`(Only, pull). **claude가 프롬프트 604자(포인터만, 전사 인라인 없음)인데 "방금 전사를 확인했습니다"며 자기 turn1 발언을 verbatim 인용**("감사 추적만으로는...80% 확보됩니다") = 604자로는 불가 → **in-process 코어에 read_transcript 호출해 당긴 것 확정**. [ctx]: claude pull 513/604(평평) vs codex push 1511(전사 인라인). "권한 막힘/취소" 0회(bearer+allowedTools 정상). **3a-3 = half-a2a 척추가 단일 front=core 프로세스로 라이브 동작.**
- **알려진 사소점**: `serve_http_mcp_on_listener` 기동 로그가 `[serve-mcp]` 프리픽스 고정이라 `--core`에서도 그렇게 찍힘(기능 무관, 공용 fn 시그니처 보존 위해 방치).

## 2026-06-29 실행 준비

- 스택 Rust+tokio 확정. 단 Plan 01 러너는 **동기 `std::process`**(v1 순차)라 tokio 미사용. tokio는 concurrency가 실제로 필요할 때 도입(YAGNI).
- **Codex 러너 먼저**(Plan 01), Claude 러너는 Plan 02. `codex exec --json` 파싱이 claude stream-json보다 단순.
- 러너는 `Runner` trait 경계. 오케스트레이터가 concrete 엔진에 안 묶이게(선제 설계 #2).
- `RunMode{ReadOnly,Write}`를 처음부터 타입으로(선제 설계 #1). spec §5 쓰기 하드 분리.
- **미확인:** codex 샌드박스 read-only 플래그. 본 plan은 `--sandbox read-only`(read) / `--full-auto`(write) 가정. Task 4 Step 1에서 `codex exec --help`로 확인 후 진행(규율 #10).
- 실행 방식: subagent-driven (Sonnet per task, Opus 리뷰). tunaRound 관례("구현=Sonnet, Opus 리뷰·검증").
- push는 천천히(개인 프로젝트). 커밋은 논리 단위로 진행.

## 실행 중 교정

- **Plan 01 Task 1 컴파일 순서 버그 교정.** plan 원안은 Task 1 `runner/mod.rs`에 `pub mod codex;`를 두고 lib.rs를 Task 5에 도입했는데, codex.rs가 없는 Task 1에서 `cargo build`가 깨진다. 교정: **lib.rs를 Task 1부터** 두고(통합테스트가 `tunaround::` 접근), `pub mod codex;` 선언은 codex.rs가 생기는 **Task 2로** 미룸. plan 문서는 실행 후 동기.
- 구현은 feature 브랜치 `feat/v1-agent-runner`에서 진행(main 직접 구현 금지).
- **Codex 샌드박스 플래그 실측 교정(Task 4, #10).** `codex exec --help` 결과 plan 가정 `--full-auto`는 **실재하지 않음**. 실제는 `-s/--sandbox <read-only|workspace-write|danger-full-access>`. 채택: **Write=`--sandbox workspace-write`**(레포 쓰기 허용), **ReadOnly=`--sandbox read-only`**(말하기 턴). plan 문서의 `--full-auto`는 Plan 01 종료 시 동기 필요.
- 미확인: `--color=never`(=형) vs `--color never`(공백형). codex가 = 형도 통상 허용. 실제 통합 실행 시 확인.

## Plan 01 완료 (2026-06-29)

- 러너 레이어 완료. 브랜치 `feat/v1-agent-runner`, 커밋 5330063~e7949f9. 전체 10 테스트 green, `cargo build`/`clippy` 클린.
- parse의 중첩 if를 let-chain으로 정리(edition 2024). dead_code 경고 전부 해소.
- 다음: Plan 02(Claude 러너, stream-json NDJSON, StreamLine 파싱, INV-3 토큰 fallback, idle watchdog). 그 전에 브랜치 마감(merge/PR) 결정 필요.

## Plan 02 완료 (2026-06-29)

- Claude 러너 완료. 브랜치 `feat/v1-claude-runner`(80ca2cb~2b18382) -> main 머지. 전체 17 테스트 green, build/clippy 클린.
- `claude --help` 실측으로 가정 플래그 전부 확인(교정 불필요). `RunError::Agent` 변형 추가(in-band 에러).
- 러너 레이어 완결(Codex + Claude, 둘 다 `Runner` trait). 다음: Plan 03 토론 오케스트레이터(두 러너를 trait로 주입, build_round_prompt 순수함수, 드라이빙 루프, consensus, 자리/쓰기 지목). idle watchdog은 hardening plan.

## Plan 03 완료 (2026-06-29)

- 오케스트레이터 완료. 브랜치 `feat/v1-orchestrator`(3a13954~c9af140) -> main. 24 테스트 green, build/clippy 클린.
- `src/orchestrator/`: roles(역할 지시문) + prompt(build_round_prompt 순차-인지) + mod(Participant/Utterance/RunnerRegistry/MapRegistry/run_round). Runner trait 경계만 의존(concrete 러너 미임포트).
- run_round는 사람 메시지=라운드. 모든 턴 ReadOnly(쓰기 지목 mode 분기는 Plan 05 REPL). consensus 자동추출은 주석 seam만.
- 사용자 지시 "플랜3까지". 여기서 정지. 남은: Plan 04(영속 트리-ready), Plan 05(thin REPL), Hardening(idle watchdog + consensus + 실 CLI 스모크).

## Plan 05 완료 (2026-06-29) — 돌아가는 앱

- "계속 진행해" 지시로 Plan 05(REPL)를 Plan 04보다 먼저(돌아가는 앱 우선). 브랜치 `feat/v1-repl`(e35683d~10dda04) -> main. `cargo run` 구동, 비대화형 스모크(배너/help/save/quit) 통과, 29 테스트 green.
- `src/repl/`: Command·parse_command·render·StepOutcome·Session. main.rs가 실 CodexRunner/ClaudeRunner를 MapRegistry로 묶음. 기본 2자리 claude=proposer, codex=reviewer. v1 에이전트 읽기 전용, 결과 문서는 /save가 전사에서 저장(에이전트 파일쓰기=v2).
- **현재 상태: 토론 코어(runner+orchestrator) + 돌아가는 REPL 완성.** 남은: Plan 04(전사 영속 트리-ready, resume), Hardening(idle watchdog + consensus 합성/conclude + 자리/쓰기 지목 + 실 CLI 통합 스모크).

## 실 에이전트 스모크 통과 (2026-06-29) — 핵심 가설 실증

- `cargo run`에 메시지 한 줄 -> 실 claude(제안자)+codex(리뷰어)가 정상 응답, exit 0, 출력 안 깨짐. fake로 못 본 실 CLI 통합 검증됨.
- 역할 주입·순차-인지·읽기전용 레포 접근(claude가 실제 README 인용) 전부 실증. **v1 핵심 가설(Claude↔Codex 구조 토론이 가치 있나)이 실 에이전트로 증명됨.**
- 주의: claude는 read-only 모드에서 레포를 자율 탐색함(읽기만). 토론 턴 후 `git status` 깨끗(레포 미변경) 확인.

## Plan 04 완료 (2026-06-29) — v1 본체 완성

- 전사 영속 완료. 브랜치 `feat/v1-store`(21dbfc5~1cc75bf) -> main. 33 테스트 green, resume 스모크 통과(저장 -> 이어받기).
- `src/store/`: StoredMessage(id/parent 트리-ready) + JSON save/load. Session.save_state/resume + main `cargo run -- state.json`(시작 resume, 종료 save). v1은 JSON, SQLite는 v2.
- **v1 본체 완성: 러너(Codex+Claude) + 오케스트레이터 + REPL + 영속.** 돌아가고, 저장/재개되고, 실 에이전트로 검증됨. 남은 건 hardening(idle watchdog, consensus /conclude, 자리/쓰기 지목).

## Plan 06 Hardening 완료 (2026-06-29) — v1 완료

- `/conclude`(synthesizer 종합) + `@engine`(자리 지목). 브랜치 `feat/v1-hardening`(464bf37, 0c4b282) -> main. 38 테스트 green. 둘 다 run_round 재사용, additive.
- **v1 완료.** 본체 + hardening. idle watchdog · 에이전트 쓰기 지목(RunMode::Write 행사) · Redis 멀티세션=git-tree 분기 · N좌석 로스터 · ratatui/web 는 v2.
- 다음 세션 = v2. 핸드오프는 docs/prompts/.

## v2 착수 (2026-06-29) — brainstorming으로 우선순위 확정

- 사용자가 "v2 끝까지 자율 진행"(특별한 결정만 확인) 지시. brainstorming으로 v2 첫 수 = **idle watchdog**(신뢰성 먼저) 확정.
- v2 우선순위: (1) idle watchdog [P0, 진행중] (2) 에이전트 쓰기 지목=협업코딩 (3) N좌석 로스터 (4) Redis 멀티세션=git-tree [신규 인프라, 착수 전 결정 필요 - 자율 진행에서 제외] (5) 리치 프론트.
- 근거: 나머지 4개는 "앱을 더 많이/오래 쓴다"는 전제 -> 신뢰성이 토대. idle watchdog은 작고 자기완결적.

## v2 Plan 01 idle watchdog 설계 결정 (2026-06-29)

- **공유 헬퍼 `src/runner/exec.rs`**로 추출(양 러너의 spawn->read->wait 동일, watchdog 단일 출처). 범위 결정 = watchdog + stderr 동시 배수(pipe-buffer 데드락도 제거).
- 출처 = tunaFlow `claude.rs` L429~629 검증 패턴. **race 수정**: watchdog_done AtomicBool + RAII WatchdogGuard(trailing-kill 차단, tunaFlow 2026-04-29 버그 반영). timed_out을 종료코드 검사보다 먼저 확인.
- **신규 의존성 0**: parking_lot 안 씀, std::sync로 충분. tokio도 불필요(동기 러너).
- kill = **단일 PID**(tunaFlow와 동일). 고아 grandchild+pipe 드문 경우는 후속 프로세스-그룹 kill로(위험 섹션). 테스트는 `exec sleep`로 단일 프로세스 보장.
- 기본 idle_timeout=600s(INV-4), 러너 필드 + `with_idle_timeout`로 테스트 주입. RunError::Timeout 추가(additive, exhaustive match 없음 확인).

## v2 Plan 01 idle watchdog 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-idle-watchdog`(a142c11 docs, 3414cf2, 78dd033) -> main. 43 테스트 green, build/clippy 클린.
- Sonnet 서브에이전트 구현 + Opus 리뷰. 계획서와 정확히 일치(벗어남 없음). 러너 타임아웃 테스트 안정적이라 `#[ignore]` 불필요.
- `src/runner/exec.rs`: run_with_watchdog(공유) = spawn -> stdin주입 -> stderr 동시배수 -> stdout 라인읽기(타이머리셋) -> watchdog 스레드 -> timed_out 먼저검사 -> 분류. WatchdogGuard(RAII)로 trailing-kill race 차단.
- 다음: v2 Plan 02(설정 구동 N좌석 로스터, docs/plans/v2-02-roster.md 작성됨). 오케스트레이터 N-ready라 main.rs + 신규 roster 로더만. 신규 의존성 0.

## v2 Plan 02 N좌석 로스터 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-roster`(af69db9, bb23e22) -> main. 48 테스트 green, build/clippy 클린, 스모크 3종 통과.
- `src/roster.rs`: JSON 로스터(Roster/SeatConfig serde) -> build_participants(_checked) + build_registry(엔진별 1러너, claude/codex만, 미지 엔진 에러). `src/main.rs` `--roster <path>` 수동 파싱(positional state backward compat 유지). `examples/roster.json`.
- 같은 엔진 다중 좌석 OK(registry 엔진별 1러너 공유, run_round이 자리별 프롬프트 분기). per-seat model·신규 엔진 러너는 후속.

## v2 자율 세션 종료 지점 (2026-06-29) — 결정 대기

- idle watchdog + N좌석 로스터까지 자율 완료(전부 main, 미푸시). 남은 v2는 전부 "특별한 결정" 필요라 자율 진행 멈춤:
  - **협업 코딩(쓰기 지목):** 설계안 docs/design/v2-write-delegation-design_2026-06-29.md. 결정 3건 = (1)claude 쓰기 권한 수위(--dangerously-skip vs --permission-mode acceptEdits) (2)쓰기 대상 디렉토리 (3)실행 전 확인 프롬프트.
  - **Redis 멀티세션=git-tree:** 신규 인프라(Redis) 결정 필요.
  - **리치 프론트(ratatui/web):** 신규 의존성 결정 필요.
- 사용자가 돌아오면 위 결정부터 받고 이어간다.

## v2 Plan 03 협업 코딩 착수 (2026-06-29) — 결정 확정

- 사용자 결정: (1) claude 쓰기 권한 **현행 `--dangerously-skip-permissions` 유지**(수개월 무사고) (2) 쓰기 대상 **cwd 레포** (3) 실행 전 확인 프롬프트 **없음**(역할 분리로 동시 같은 파일 경합 없음, 한 번에 한 자리만 쓰기).
- 설계: `@engine!`로 쓰기 턴 지목. run_round에 mode 파라미터 추가(기존 호출 ReadOnly=동작보존), Command::Write + step 분기. 쓰기 인프라(러너 인자)는 v1 구현 재사용.
- main 푸시 시작함(이 시점 origin 동기화, 8bc3bea..240cd83). 이후 논리 단위로 푸시.

## v2 Plan 03 협업 코딩 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-write-delegation`(ee96a53 docs, 9c55b97, 1ae8b49) -> main. 52 테스트 green, build/clippy 클린.
- `@engine! <msg>`로 한 자리를 쓰기 턴 지목 -> 그 자리만 RunMode::Write로 cwd 레포 편집. run_round에 mode 파라미터(기존 호출 ReadOnly=동작보존). 쓰기 인프라는 v1 재사용.
- **이제 tunaRound는 토론 + 실제 협업 코딩 도구.** 남은 v2(전부 인프라/의존성 결정 필요): Redis 멀티세션=git-tree / 리치 프론트 ratatui·web / 신규 엔진 러너(tunaLlama·opencode 좌석).
- 후속(쓰기 관련): git diff 자동 요약, 자동 커밋, 쓰기 결과 리뷰 라운드 - 필요 시.

## v2 멀티세션 착수 + 교정 (2026-06-29)

- **교정:** 내가 "Redis가 정말 필요한가"로 멀티세션 아키텍처를 재론해 사용자가 제지("이미 다 결정했는데 뒤집지 마라, claude-mem 활용해라"). 설계문서 L33·L108·L144-145가 이미 확정: **Redis 멀티세션=git-tree 분기, tunaSalon session_bus 포팅, 브랜치=세션, presence/snapshot 신규**. 메모리 [[no-relitigating-decisions]] 추가. 앞으로 v2 착수 전 design 문서 v2 섹션 + claude-mem 먼저.
- 분해 3플랜으로 진행(사용자 GO): **Plan 04 session_bus 포팅(격리 토대)** -> Plan 05 세션모델(브랜치=세션) -> Plan 06 REPL통합+presence/snapshot.
- async 경계 결정(내가 정함): tokio/async는 bus 레이어에만, 동기 코어 유지, block_on 브리지는 Plan 06. 신규 의존성 tokio/redis 0.32/futures-util(설계문서 L145 승인).

## v2 Plan 04 session_bus 포팅 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-session-bus`(c0ee2bb docs, 0783179, 86aa482, 11e1f52 주석정리) -> main. 56 테스트(49 pass + 2 ignored 라이브 Redis + 5 통합), build/clippy 클린.
- `src/session_bus.rs`: tunaSalon 포팅. SessionBus trait + RedisBus(6함수 async) + RedisBusHandle + RedisSessionKeys/RedisStreamMessage. 키 prefix `session:{id}:...`, env `TUNAROUND_REDIS_URL`. redis 0.32 API 출처와 동일(조정 불필요).
- 완전 격리: 기존 동기 코드 미접촉, main.rs 런타임 미도입. 평소 cargo test는 Redis 없이 green.
- 다음: Plan 05 세션 모델(브랜치=세션, store parent_id 실사용). 착수 전 design 문서 + claude-mem에서 분기/세션 결정 확인할 것([[no-relitigating-decisions]]).

## v2 Plan 05 세션 모델 착수 (2026-06-29)

- 설계문서가 "분기 UI는 v2(Q8)"로 남긴 미결 항목 -> 사용자에게 구체 모델만 확인(재론 아님). **확정: in-store 논리 트리(옵션 A).** git 브랜치 백업/세션파일 복사는 기각.
- 설계: Session이 선형 transcript -> 트리(messages: Vec<StoredMessage> + head). 라운드마다 active path(root->head)를 run_round에 넘기고 반환 round를 head 분기로 append. `/branches`(tree_summary)+`/checkout <id>`(head 이동). run_round/러너 무변경(트리 로직 = store 순수함수 + Session 격리).
- 저장 포맷 StoredSession{messages, head}, load_session은 레거시 bare-array 폴백(head=마지막 id). Redis/presence/멀티프로세스는 Plan 06.

## v2 Plan 05 세션 모델 완료 (2026-06-29)

- 구현 완료. 브랜치 `feat/v2-session-model`(7ded26d, c9510fe, 5b25827) -> main. 63 테스트(61 pass + 2 ignored), build/clippy 클린.
- Session: 선형 transcript -> 트리(messages+head). active_path(root->head)를 run_round에 넘기고 반환 round를 head 분기로 append(이중 append 없음 검증). `/branches`(tree_summary)+`/checkout <id>`. 저장 StoredSession+레거시 폴백.
- **단일 프로세스 분기 토론 동작.** 멀티세션 04 토대+05 트리모델 done. 남은 Plan 06 = Redis 통합(각 분기 session_id)+presence/snapshot 신규+block_on 브리지(멀티프로세스 동시 세션). Plan 06은 async<->sync 브리지·net-new presence라 가장 무거움, 착수 전 설계 필요.

## v2 Plan 06 Redis 통합 착수 (2026-06-29)

- 사용자 확정: 미러 + observe + resume **전부 한 플랜**(read 쪽 첫 동작 질문에 "둘 다").
- 설계 핵심: write path는 sync(SessionBus fire-and-forget mpsc -> 백그라운드 tokio), read path(observe/resume 일회성 GET·subscribe 루프)만 main에서 block_on. payload=store 타입 재사용(snapshot=StoredSession, event=이번 라운드 Vec<StoredMessage>). owner lease=process id, 경고만(강제차단 아님). bus=None이면 기존 동작 불변.
- **검증 한계(정직):** observe/resume 라이브는 라이브 Redis + 2 터미널 필요 -> 수동/#[ignore]. 자동 테스트는 fake bus write-path + 파싱만. 자동 green이 라이브 동작 보장 안 함.
- Task: 1 session_bus snapshot / 2 Session 미러(fake bus 테스트) / 3 main.rs 런타임+observe+resume(수동 스모크).

## v2 Plan 06 Redis 통합 완료 (2026-06-29) — 멀티세션 완성

- 구현 완료. 브랜치 `feat/v2-redis-integration`(e72c867, c46121c, eb470b8, 389fe09 정리) -> main. 66 테스트(63 pass + 3 ignored), build/clippy 클린. Redis 없이 cargo run /quit 정상(bus=None 불변).
- session_bus snapshot(set/get + fire-and-forget) + Session 미러(append_round 후 event=새메시지/snapshot=전체트리) + main.rs tokio 런타임 + `--observe`(구독 루프)/`--session`(snapshot seed+owner lease+refresh).
- 리뷰 정리: --session 재개의 중복 RedisBusHandle spawn 제거(bus_boxed 재사용, 389fe09).
- **검증 한계:** observe/resume 라이브는 라이브 Redis+2터미널 필요라 자동검증 불가(수동 1회 확인 필요). 자동 테스트는 FakeBus write-path+파싱만.
- **멀티세션 완성(04 토대+05 트리+06 통합).** v2 설계문서 멀티세션 로드맵 끝. 남은 v2 백로그(결정 필요): 리치 프론트 ratatui·web / 신규 엔진 러너 좌석(tunaLlama·opencode).

## v2 Plan 06 라이브 검증 + 버그 수정 (2026-06-30)

- 로컬 Redis(brew 8.8.0) 설치 후 실 검증. bus 3 #[ignore] / resume(--session) / observe(--observe) / 실 3라운드 컨텍스트 유지 전부 통과. git 청결(ReadOnly).
- **실 라운드로만 잡힌 버그:** mirror가 fire-and-forget이라 /quit 시 마지막 라운드 snapshot 유실(events=3, snapshot=2). resume 정확성 결함.
- **수정:** 종료 직전 `session.snapshot_json()`을 동기 `set_snapshot`으로 1회 기록(Session::snapshot_json 추가). 1라운드 재검증 통과(snapshot이 라운드 보존). 브랜치 `fix/v2-06-snapshot-flush`(50edea4). 잔여: per-round 이벤트는 best-effort(관찰자는 라운드 중 수신), resume는 종료 flush로 보장.
- redis-server는 검증 후에도 기동 상태(brew 설치). 끄려면 `redis-cli shutdown nosave`.

## 설계 방향 리뷰 (2026-06-30) — 결정 보류

- 사용자가 "로컬 멀티세션 broker(PTY로 claude/codex 라이브 세션 + Redis stream + 수동 라우팅)" 설계 대화를 검토 요청. 내 결론: **이건 tunaRound 현재 one-shot 재주입 모델과 다른 패러다임**(PTY 라이브 세션 = 컨텍스트가 에이전트 내부). PTY는 tunaRound가 의도적으로 피한 파싱 복잡도(턴 종료 판정 등)를 되살림. local-first·manual-first·ctx-handle 순서는 옳음. **추천 = 하이브리드**(one-shot 유지 + ctx-handle + side-by-side UI + draft→approve relay, PTY 없이). "tunaRound 피벗 vs 별도 tunaSalon v0" 결정 필요. 아직 미결.

## 설계 방향 수렴 (2026-06-30)

- 사용자 비전: 다른 터미널의 claude/codex가 A2A로 토론·대화·협업. **PTY 아니라 A2A(MCP 도구로 서로 메시지, 버스=Redis)가 맞는 길**로 합의(터미널 스크래핑 회피).
- 핵심 난제 = **turn-triggering**: 인터랙티브 CLI는 메시지 와도 스스로 안 깨어남(데몬 모드 없음). 그래서 자율 두-터미널 대화는 큰 과제.
- **트리거 UX 통찰:** 토론은 분리 터미널이 트리거를 *더* 애매하게 만듦(수동 핑퐁). 단일 REPL 오케스트레이션이 이미 깔끔(한 곳 입력 -> 둘 순차). "사람 발화 1회 -> 둘이 N턴 자동 교환 -> 복귀"로 바운드하면 트리거 명확 + 폭주 방지.
- **결정: 두 갈래.** (1) 토론/대화 = 오케스트레이션 + 바운드 자동 교환(Plan 07, 지금) (2) 프로젝트 협업 = 분리 터미널 A2A(MCP+버스, 자율 핸드오프, turn-triggering 해결 필요 = 백로그). #2의 "codex 리뷰 -> claude에 전달 -> claude 응답" 예시 상당 부분은 #1 토대(방향 지정 라운드)로도 표현됨. **#1부터.**

## v2 Plan 07 바운드 자동 교환 착수 (2026-06-30)

- `/debate <n> <주제>`: 사람 발화 1개 -> run_round을 N회 반복(라운드1=주제, 라운드2~N=연속 프롬프트 반박/심화/수렴) -> 누적 출력. 기본 3턴, 최대 10 clamp(비용 폭주 방지). 각 라운드는 기존 append_round(트리·Redis 미러 그대로). 새 인프라 0. fake 러너 TDD.
- **완료(2026-06-30):** 브랜치 `feat/v2-bounded-debate`(c5b9339, 01b8860) -> main. 69 테스트(66+3 ignored), build/clippy 클린.

## 북극성: 계층형 공유 맥락 + 능동 검색 (2026-06-30)

- 사용자 핵심 요구: 에이전트가 서로 맥락을 **능동적으로 기억·검색**, 단기(세션)~프로젝트 모든 층. Redis·SQLite 적극, 필요시 vector DB. 설계문서 `docs/design/v2-context-memory-direction_2026-06-30.md`.
- 핵심 전환: "전사 통째 재주입" -> "검색해 관련 슬라이스만 주입(RAG)". 현재 build_round_prompt가 통째 재주입 = 스케일 병목.
- **재주입 vs Redis 구분(사용자 질문):** Redis(Plan 06)는 cross-process 전송/캐시 + 라이브일 뿐, 프롬프트 조립을 안 바꿈 = 재주입 자체를 안 줄임. 재주입 감소 = (a)handle(참조 전달, 온디맨드 expand: Redis 강점) + (b)관련성 검색(vanilla Redis는 전문/의미검색 없음 -> SQLite FTS / vector / RediSearch). 둘 다 아직 미구현(현 Redis는 미러/observe/resume만).
- 저장소 계층: Redis=핫+handle+pubsub / SQLite=시스템오브레코드+FTS 백본 / vector=의미검색.

## 한국어 검색 정답 = secall 포팅 (2026-06-30)

- tuna 단골 "한국어 FTS 형태소" 정답이 secall 코드에 있음. 재발명 금지, 포팅. 메모리 [[korean-search-port-secall]].
- 해법: 형태소 분석기로 선-토크나이즈 -> FTS5(unicode61)에 공백조인 저장("검색을"->"검색"). keep-tags NNG/NNP/NNB/VV/VA/SL(외국어=영어·코드 살림). + BGE-M3 벡터 + 하이브리드(BM25+ANN).
- **Kiwi 메인(품질)**, lindera 폴백. lindera 폴백은 secall 초기 Mac kiwi 컴파일 이슈 잔재(현재 mac에서 Kiwi 동작). tunaSalon은 lindera-only lift라 그것만 보였던 것.
- **임베딩 = 원격 Ollama(로컬 ORT 대체):** SSH 터널 `-L 11435:127.0.0.1:11434` -> `/api/embed` model bge-m3. Embedder=reqwest HTTP + MockEmbedder 폴백. 무거운 ONNX 의존 제거. 터널 떠 있어야 동작, 원격 bge-m3 pull 필요.
- 진화: tunaFlow(vector_search) -> secall(hybrid 정본) -> tunaSalon(lindera+BGE-M3 경량). 설계 v2-context-memory-direction_2026-06-30.md.

## v2 Plan 08 한국어 토크나이저 착수 (2026-06-30)

- secall `tokenizer.rs` 포팅(Tokenizer trait + Kiwi + lindera + factory), String 에러/eprintln 적응(anyhow/tracing 미도입), `morphology` feature 게이트(기본 빌드 무영향). Task 1=lindera(안전), Task 2=Kiwi(컴파일 risk-gate). 격리 모듈, 미배선. 다음=SQLite FTS 선-형태소화.
- **완료(2026-06-30):** 브랜치 `feat/v2-ko-tokenizer`(74f8771, 1059be8) -> main. 기본 66/morphology 72 pass, clippy 클린. kiwi-rs **컴파일 성공**(mac aarch64, 과거 이슈 해소).
- **⚠️ Kiwi 런타임 부트스트랩 실패:** 라이브 테스트에서 libkiwi.dylib 로드 실패 + auto-download 404(`kiwi_mac_arm64_v0.23.2.tgz` 없음). kiwi-rs 0.1.4가 libkiwi v0.23.2 받으려다 upstream 에셋 부재 -> **create_tokenizer("kiwi")가 lindera 폴백**. Kiwi 메인 코드는 준비됐으나 실효는 lindera. 후속: kiwi-rs 버전 핀/libkiwi 수동 설치/upstream 확인. **Windows에선 Kiwi cfg 제외 = lindera만이라 무관.**

## 맥 세션 종료 + Windows 이관 (2026-06-30)

- 사용자: 다음 작업은 **Windows로 이관**(완전 새 세션, /clear 아님). 맥 작업 여기까지. 상세 핸드오프 작성됨(docs/prompts/).
- 정리: redis-server 내림(`redis-cli shutdown nosave`), SSH 터널(2232) 종료, observer 프로세스 종료. (brew redis 설치는 남음 - 필요시 `brew uninstall redis`.)
- **Windows 주의:** (1) Kiwi cfg 제외 -> 토크나이저 = lindera(정상). (2) Redis 라이브 검증은 Windows에 redis 필요(WSL/Memurai/Docker). (3) 원격 Ollama 터널: Windows ssh도 `-p [사설포트] -L 11435:127.0.0.1:11434`(bge-m3 검증됨 dim 1024). (4) claude/codex CLI 경로/실행이 Windows에서 다를 수 있음(러너 spawn 확인).

## Windows 첫 세션: 빌드 검증 + Plan 09 착수 (2026-06-30)

- **빌드 검증(맥 패리티 달성):** 기본 `cargo test` 66/morphology 72 pass, build/clippy 클린. 처음엔 러너 timeout/spawn 픽스처 4건 Windows 실패(`#!/bin/sh`를 bin으로 직접 spawn -> Windows 직접 실행 불가). 수정: OS별 픽스처(Unix=.sh, Windows=무출력 .cmd, Rust 1.77.2+가 .cmd를 cmd.exe 래핑). 커밋 `3f44a48`(미푸시). cfg(unix) 게이트 안 하고 양 OS 커버리지 유지.
- **⚠️ 남은 리스크(gotcha #4, 미검증):** 프로덕션 러너는 `Command::new("claude")`/`("codex")`(확장자 없음)로 spawn. Windows 실제는 `claude.cmd`(npm shim)일 수 있고 .cmd 자동 래핑은 **이름이 .cmd로 끝날 때만** -> 실 에이전트 스모크 전 러너 실행파일 해석 점검 필요(tunaFlow wrap_windows_script 참고).
- **전역 설정 gotcha #0:** Windows엔 `~/.config/agents/COMMON.md` 없음(`~/.claude/CLAUDE.md`가 `@RTK.md`만 import, COMMON 미로드). 단 Windows 자체 CLAUDE.md가 공통 계약(결론우선/findings-first/검증사다리/보안/한국어)을 자체 포함 -> 치명적 공백 아님. 일원화하려면 COMMON.md 복원 + import 틸드 경로(별 트랙).
- **Plan 09 결정(사용자 확정):** 다음 = SQLite 시스템오브레코드 + FTS5(선-형태소화). 범위 = **격리 모듈 우선**(store/sqlite.rs + 테스트만, REPL/main JSON 미접촉). 의존성 = **새 sqlite feature**(rusqlite 0.31 bundled optional). 스토어는 토크나이저 비의존(선-토크나이즈 텍스트 주입), morphology는 통합 테스트에서만 결합. 출처 답습 = secall `store/schema.rs`(FTS5 unicode61 + UNINDEXED 역참조) + `search/bm25.rs`. 출처 레포 D드라이브 확인(`D:/privateProject/seCall`, `tunaSalon`). plan = docs/plans/v2-09-sqlite-fts.md.
- **Plan 09 완료(2026-06-30):** Task 1 `c61cf11`(Sonnet 위임: 스키마/마이그레이션/save_session/load_session) + Task 2 `181f46a`(Opus: FTS 검색 테스트). **Windows rusqlite bundled 컴파일 OK(21초, MSVC C:\BuildTools 자동탐지).** sqlite 68/sqlite+morphology 75 pass, 기본 61 불변, clippy 양 조합 클린. **핵심 실증**: `morpheme_indexing_matches_inflected_form` 통과 = "검색을" 형태소 색인 -> "검색" 쿼리 매칭(Windows lindera 경로). 미푸시.
- **잠재 와트(기록):** `exec.rs` 러너 테스트는 `bin:"sh"` 의존 -> Git Bash(sh on PATH)에선 green, 순수 PowerShell(sh 미발견)에선 spawn 실패. 정본 검증은 Bash 경유. 서브에이전트가 PowerShell로 돌려 "2 fail" 오인했던 원인.
- **Plan 09 다음 슬라이스:** (a) 영속을 SQLite로 전환(시스템오브레코드, REPL/main + Redis 스냅샷 조정) (b) `build_round_prompt` RAG화(통째 재주입 -> 검색 슬라이스) (c) 벡터(원격 Ollama bge-m3 dim 1024) -> 하이브리드.

## Plan 10 SQLite 라이브 배선 완료 (2026-06-30)

- **방식:** 기존 SessionBus 미러 패턴 답습. `MessageIndexer` trait(비게이트) + `SqliteIndexer`(sqlite feature, `Mutex<SqliteStore>` + tokenize closure 주입) + Session `indexer: Option<Box<dyn ...>>` 필드 + `append_round` 훅 + main `--db <path>`. 추가적(JSON save/load·Redis 미접촉), sqlite off/--db 없음=None=기존 동작 불변.
- **커밋:** Task 1 `e21cf43`(trait+indexer+Session, Sonnet) + Task 2 `5d79a0a`(main --db 3분기 배선 + roundtrip 테스트, Sonnet). sqlite 74/sqlite+morphology 81 pass, clippy 3조합 클린, 스모크 OK. **origin 푸시됨**(README `5c31a1d`와 함께, 63fc071..5c31a1d).
- **이탈(타당):** Send+Sync 위해 Rc→Arc<Mutex> / Connection !Sync라 Mutex<SqliteStore> / 통합테스트는 indexer.rs 단위테스트로(FakeRunner cross-crate 불가 회피) / `--db` 변수 cfg(sqlite) 게이트(unused 경고 억제).
- **절차 교훈:** README를 쓰는 사이 Task 2가 커밋돼 `git push`(README만 의도)가 Task 1·2 코드까지 함께 올림. 또 Task 2를 리뷰 전 푸시 -> 사후 독립검증(빌드/테스트/clippy)으로 그린 확인. **다음부턴 서브에이전트 진행 중 푸시 자제 또는 완료·리뷰 후 푸시.**
- **다음 = Plan 11 검색 주입(RAG):** `build_round_prompt`가 통째 재주입 대신 SqliteStore.search로 관련 슬라이스만 주입. 북극성 핵심. 인덱스는 Plan 10으로 라이브로 채워짐.
- **검색 토크나이저(서브에이전트 보고):** `cargo`는 Bash 툴로 돌릴 것(Git Bash sh 있어 exec.rs sh 테스트 통과; PowerShell이면 2건 거짓 실패).

## Plan 11 검색 주입(RAG) 완료 (2026-06-30)

- **방식(추가적):** prior 통째 재주입은 그대로 두고, 활성 경로 **밖**의 관련 맥락(다른 분기·과거 세션)을 topic으로 검색해 "참고할 만한 과거 맥락(검색)" 섹션으로 **추가** 주입. 검증된 단일세션 품질 보존하면서 능동 검색 기둥만 세움. prior 캡(재주입 토큰 축소)은 품질 측정 후 별 슬라이스(설계 원칙: 검색가능->주입->측정->필요시 축소).
- **구조:** `ContextRetriever` trait(orchestrator, 비게이트) + `build_round_prompt`/`run_round`에 retrieved 슬롯(Task 1, 동작 불변) + `SqliteRetriever`(sqlite, SqliteStore 읽기 + tokenize closure) + `Session.retriever`(with_retriever 빌더) + `retrieve_for`(활성 경로 content dedup, K=5) + main `--db`로 indexer와 별개 읽기 연결(WAL 동시 reader). retriever 없으면 retrieved=&[] = 동작 불변.
- **커밋:** Task 1 `b0dd7bd`(orchestrator 슬롯) + Task 2 `4643977`(SqliteRetriever+Session+main). sqlite 76/sqlite+morphology 83 pass, clippy 3조합 클린, 스모크 OK. **cross-session 검색 단위 테스트 통과 = 능동 검색 실연.** 미푸시.
- **다음 = Plan 12 벡터/하이브리드:** 어휘(FTS)만으론 동의어·의미 약함. 원격 Ollama bge-m3(dim 1024, SSH -p [사설포트] 터널) reqwest 임베더 + MockEmbedder 폴백 + ANN(usearch 또는 cosine) + 하이브리드(BM25+벡터).

## 원격 Ollama Windows 검증 + 벡터/정렬 결정 (2026-06-30)

- **검증됨(Windows):** `ssh -p [사설포트] -o BatchMode=yes [사설계정]@[사설IP] 'curl 127.0.0.1:11434/api/...'` 작동(키 인증, 무비번). `/api/tags`=bge-m3:latest + gemma4:e2b/e4b. `/api/embed` model bge-m3 -> **dim 1024 확인.** 사용자가 안내한 **포트 22는 타임아웃, 실제 포트=2232**(핸드오프 일치). 호스트=[사설IP](이제 세션에 공개됨). 터널형: `ssh -N -p [사설포트] -L 11435:127.0.0.1:11434 [사설계정]@[사설IP]`.
- **벡터 라이브 블로커 해소.** 단 **설계 YAGNI 게이트(FTS 부족 입증 시에만, 마지막)는 여전히 유효** -> 사용자 결정=벡터 보류, 정렬 슬라이스(/search)부터.
- **Plan 12 재정의 = /search 명령**(사람이 인덱스 직접 검색, FTS 품질 관측 -> 벡터 도입 근거 수집). plan=docs/plans/v2-12-search-command.md. 기존 Session.retriever 재사용, 신규 의존성 0. 벡터(원안)는 /search로 품질 관측 후.
- **Plan 12 /search 완료(2026-06-30):** `bc2f359`(Sonnet). Command::Search 파싱 + step 핸들러(retriever 재사용, 없으면 --db 안내, 빈 결과 안내, 있으면 render). 기본 70/sqlite 79/sqlite+morphology 86 pass, clippy 3조합 클린. 미푸시.
- **벡터(Plan 원안) 재개 조건:** 라이브 블로커 해소됨(2232/bge-m3 dim 1024). 남은 게이트=YAGNI(FTS 부족 입증). 재개 시 Embedder trait + MockEmbedder + reqwest Ollama(엔드포인트 http://127.0.0.1:11435, 터널 -p [사설포트]) + message_vectors BLOB(dim 1024) + cosine/ANN + 하이브리드(BM25+벡터). semantic feature 게이트.

## Plan 13 벡터/하이브리드 완료 (2026-06-30, 사용자 요청으로 YAGNI 우회 진행)

- **사용자가 벡터 진행 지시**(Ollama 호스트 제공 + "2,3 가자"). 블로커 해소돼 원안대로 구축.
- **구조:** `semantic = ["sqlite","dep:reqwest"]`(reqwest blocking, rustls-tls). `store/embedding.rs`=Embedder trait + MockEmbedder(결정적, sqlite) + OllamaEmbedder(semantic, `{model:bge-m3,input:[..]}`->`{embeddings}`). `sqlite.rs`=message_vectors(schema v2, f32 LE BLOB, content_hash 증분 가드) + index_vectors(Embedder 주입) + vector_search(brute-force cosine) + get_message. `store/mod.rs`=reciprocal_rank_fusion(k=60, secall 답습). SqliteIndexer/SqliteRetriever에 `Option<Box<dyn Embedder>>` - 있으면 벡터색인/RRF 하이브리드, 없으면 FTS 단독(불변). main semantic 시 OllamaEmbedder 2개(indexer/retriever, env TUNAROUND_OLLAMA_URL/기본 11435).
- **커밋:** 1ad8881(embedder) + 30efa51(vectors+cosine) + 8920027(RRF+배선). sqlite 86/semantic 86 pass, clippy 클린, 스모크 OK.
- **라이브 검증:** `ollama_embed_live_dim_1024 ... ok`(로컬 11435 터널 -> 원격 bge-m3, dim 1024). reqwest 클라이언트 end-to-end 동작 확인.
- **한계:** ANN 미도입(brute-force cosine, 규모 시 usearch). 라이브 의미 품질(벡터가 recall 개선하는지)은 실사용 측정 영역. embedder 2중 생성(Arc 공유 후속). reqwest blocking은 Session.step이 block_on 밖이라 안전.
- **다음 = item 3 폴리시:** load_session .ok() 보정 + 토크나이저/embedder Arc 공유.

## Plan 14 에이전트 능동 검색 MCP (2026-06-30, 사용자 선택)

- **방식:** secall rmcp(1.3.0->1.8.0) 답습. `src/mcp.rs` TunaSearchServer = 단일 툴 `search_context(query,limit)`가 기존 SqliteRetriever(하이브리드) 호출 -> Content. `main --mcp-search --db`로 stdio MCP 서버 기동. claude 러너가 `--mcp-config`(serde_json, command=self-exe args=[--mcp-search,--db,path])로 이 서버를 물려 에이전트가 토론 중 자율 호출. `mcp = ["sqlite","dep:rmcp","dep:schemars"]` 피처.
- **커밋:** Task 1 `a65feba`(서버+stdio) + Task 2 `a5a185d`(claude 배선). mcp 89 pass, 기본 71 불변, clippy 클린. ContextRetriever에 `Send+Sync` bound 추가(기존 구현 충족). rmcp Windows 빌드 OK(10초).
- **Task 3 라이브 대기:** 실 claude가 search_context를 실제로 부르는지 = 토큰 소모, 사용자 확인 후. **codex는 gotcha #4로 막힘**(codex.exe 없음, npm shim codex.cmd만 -> Command::new("codex") spawn 실패). codex 능동검색은 gotcha #4(러너 Windows CLI 해석) 수정 후.
- **CLI MCP 설정 실측:** claude `--mcp-config <JSON>`(인라인/파일)+`--strict-mcp-config`. codex는 퍼-런 플래그 없음(`codex mcp add` 영속 or `-c` 오버라이드).
- **gotcha #4 정밀 진단:** `claude`=claude.exe(spawn OK), `codex`=codex/codex.cmd/codex.ps1만(codex.exe 없음). Rust Command::new는 .exe만 덧붙여 찾고 .cmd는 이름이 .cmd로 끝날 때만 -> codex spawn 실패. 수정=러너가 Windows에서 .cmd 해석(tunaFlow wrap_windows_script).

## Plan 15(gotcha #4) + Plan 14 Task 3 라이브 검증 완료 (2026-06-30)

- **Plan 15 `8d02088`:** `exec.rs resolve_bin` - Windows에서 확장자 없는 bin을 PATH에서 .exe/.cmd/.bat/.com 풀경로화(Rust가 .cmd를 cmd.exe 자동 래핑). `run_with_watchdog` spawn 전 호출. 비Windows·확장자/경로 있는 bin은 no-op(기존 .cmd/.sh 픽스처 테스트 무영향). 기본 74/전체 99 pass.
- **라이브 검증(실 에이전트, 토큰 사용):** `printf '...자기 역할...' | tunaround --db smoke.db`(mcp 빌드) -> **claude/proposer + codex/reviewer 둘 다 실제 응답**(gotcha #4 수정으로 codex.cmd spawn 성공 = 라이브 입증). smoke.db에 색인됨.
- **MCP 검증(무토큰):** `tunaround --mcp-search --db smoke.db`에 JSON-RPC initialize+tools/call 직접 전송 -> rmcp 1.8.0 정상, `search_context("발제자")` -> 실 색인된 claude 발언 반환. **MCP 검색 전 체인 입증.**
- **남은 것(모델 행동):** 에이전트가 토론 중 search_context를 자율 호출할지는 모델 판단. 툴 배선·서버·검색은 입증됨. 품질은 `--features "mcp morphology semantic"`(형태소 FTS + bge-m3 벡터)로 빌드 시 ↑.
- **검색 스택 전체 완성:** 형태소 FTS(Plan 8,9) + 라이브 색인(10) + RAG 주입(11) + /search(12) + 벡터/하이브리드(13) + 에이전트 MCP 도구(14) + Windows 러너(15). v2 검색/맥락 북극성 1차 완결.

## 검색 품질 측정 + Plan 17~19 + Kiwi 활성화 (2026-06-30)

- **검색 품질 실측(중요):** tests/search_quality.rs(#[ignore], 통제 코퍼스+Ollama 임베딩) 게이지로 측정. 발견: lindera가 **외래어를 문맥에서 누락**("벡터 임베딩을"→"임베딩" 탈락, "인증을"→"인증"은 정상). 형태소 굴절은 OK, 외래어가 구멍. 벡터는 소규모 코퍼스에서 노이즈 큼. => **기계 동작만 검증했지 품질은 평범**이었음을 인정하고 실측으로 전환.
- **Plan 17 `e1373f9`:** OpenAI 호환 HTTP 엔진 러너(runner/http.rs, engines feature). 한 러너로 ollama/lmstudio/openai/cloud. 로스터 base_url/model/api_key_env. 라이브: Ollama gemma4:e2b /v1/chat/completions 응답.
- **Plan 18 `45cf0c8`:** FTS 리콜 보강 - 색인=형태소+raw 토큰(fts_index), 질의=prefix-AND(fts_query). 외래어 누락 메움(재측정서 "임베딩" #3 히트). index/query 클로저 분리. 기본 feature=morphology+sqlite(4441a18).
- **Plan 19 `fe0ec71` Kiwi 활성화(중요, 재현법):** Kiwi가 Windows에서 막혔던 진짜 원인 = (1) kiwi-rs 0.1.4 auto-download 깨짐(GITHUB_TOKEN 무관, release_json/curl 실패) (2) **latest Kiwi v0.23.2는 kiwi-rs 0.1.4 바인딩과 ABI 불일치 → native ACCESS_VIOLATION**. 해법 = **Kiwi v0.22.2**(0.1.4 README가 겨냥) 수동 설치. `Kiwi::init()`이 discovery(KIWI_LIBRARY_PATH/KIWI_MODEL_PATH 또는 **%LOCALAPPDATA%\kiwi** 기본)를 bootstrap보다 먼저 봄 → 수동 배치로 깨진 다운로드 우회. **설치:** `gh release download v0.22.2 --repo bab2min/Kiwi`로 kiwi_win_x64_v0.22.2.zip(→lib/kiwi.dll) + kiwi_model_v0.22.2_base.tgz(→models/cong/base)를 %LOCALAPPDATA%\kiwi에 추출(`scripts/install-kiwi-windows.sh`). env 불필요. 미설치 시 lindera 폴백. 문서 docs/reference/kiwi-windows-setup.md. **주의: v0.23.2 쓰지 말 것(crash).** Kiwi keep-tags는 base 매칭(VA-I/VV-I 변종). Kiwi도 외래어 음절분할하나 Plan 18 raw+prefix가 FTS 커버.
- **README:** 사용자가 깃헙에서 전면 리라이트(어투 개선) → 로컬 분기와 충돌 → merge에서 사용자 리라이트 채택 + 로드맵 정정·"좌석"→"참가자"·Kiwi 안내만 재적용(`5b8cd36`). origin 동기화됨. "좌석"은 코드(SeatConfig)·일부 plan 문서엔 잔존(내부라 미변경).
- **미반영 후속:** 검색 품질 추가 개선(현실 코퍼스 측정) · 요약 carry-forward(enhancement; 온디맨드 확장은 MCP search_context가 이미 커버) · 예시 로스터 확장 · 리치 프론트(보류).
- **Plan 20 opencode CLI 러너 done(`7fedac2`):** `opencode run --format json` JSONL(text.part.text=본문, step_finish.part.tokens=토큰) 파싱 + 로스터 engine "opencode"(seat.model). 신규 의존성 0, gotcha #4 resolve_bin이 opencode.cmd spawn. **ollama cloud가 opencode에 안정**(Cerebras/짧은 타임아웃은 cold start로 hang). 모델 예: `ollama-cloud/gemma3:4b`. 신규 엔진 = HTTP(17) + opencode(20) 완성.
- **검토할 아키텍처 방향(사용자 제기 2026-06-30): 코어-백엔드 + 에이전트-클라이언트(A2A).** 현재=tunaRound가 매 라운드 에이전트 stateless spawn(-p). 제안=코어(오케스트레이션+검색/메모리) 백엔드 상주 + 에이전트는 MCP 클라이언트로 접속. **이미 씨앗=`--mcp-search`(검색/메모리를 백엔드로 노출)**. 확장=오케스트레이션 툴(read_transcript/post_turn) 추가. **난점=분산 turn-triggering(A2A 백로그 난제) + 컨텍스트 통제 약화.** 두 모델 공존·점진 권고. 큰 포크라 별도 설계 세션. 상세 핸드오프 ⑧-A.

## 2026-06-30 (A) 코어-백엔드 설계 확정 (사용자 결정)

- **A2A를 둘로 분해(설계 흔들림 방지):** (A) 아키텍처 A2A = 코어 상주 백엔드 + 에이전트 접속 클라이언트, **사람이 운전자**(= 가치, 채택). (B) 자율 A2A = 에이전트가 다음 화자 스스로 결정·서로 트리거(= 미래 명시 opt-in, 지금 X). 사용자 확정: **(A)**.
- **(B) 경제 논리(사용자 직관, 기록):** 자율 루프가 비싼 진짜 이유 = 토큰이 아니라 **탐색 공간**. 사람 마이크로매니징 = 매 턴 **가지치기** = 라운드 수↓ = 품질↑·비용↓. (B)의 경제가 뒤집히는 조건 = (1) 토큰 단가 충분히 하락 or (2) 과제가 **검증 가능**(테스트/컴파일/실측 기계 판정)해 사람 없이 수렴. 그 전엔 사람-주도가 싸고 좋다. → (B)는 조건부 옵션, 디폴트는 영원히 사람-주도.
- **핵심 솔기 = turn-policy:** "다음 턴 누가 정하나"를 코어 명시 정책으로 분리. `HumanDriven`(디폴트·유일 구현) / `AutoLoop`(미래 (B), 같은 백엔드 위 정책만 교체). 이 솔기로 (B)는 포크가 아닌 **플러그인 1개**, 켜기 전 비용 0.
- **본질 전환:** push(맥락을 prompt에 통째 밀어넣기) → pull(코어가 전사·검색·요약을 서비스로 노출, 에이전트가 필요분만 도구로 당김). `--recent-turns`(Plan 16)·`--mcp-search`(Plan 14)가 이미 그 씨앗.
- **단계:** Stage 0(검색품질+요약 carry-forward, 코어 경화) → 1(오케스트레이션 툴 read_transcript/get_roster) → 2(주입 push→pull, 재전송량 감소 **실측**=crux) → 3(코어 데몬 분리) → 4(범위 밖=(B)). Stage 1~2는 **에이전트 여전히 stateless spawn**(저위험), 영속 프로세스는 Stage 3 이후.
- **리스크:** codex MCP **도구 실호출** 미검증(Plan 14 T4는 `-c mcp_servers` 인자 수용만 확인) = Stage 1 통과 기준. Stage 2는 통제 약화 위험(포인터에 당길 범위 명시로 완화).
- **정본 문서:** docs/design/v2-A2A-core-backend_2026-06-30.md. **이번 세션 = Stage 0 + (A)설계 병렬.**

## 2026-06-30 검색 품질 트랙 결정 (Memora 참고 후)

- **Stage 0 항목1(검색품질) 완료·커밋**: `581eaa2`(하네스+FTS AND→OR), `30543fb`(precision@k). R@5 0.55→0.90, MRR 0.60→0.90, P@5 0.727. K=5 정당화. 진짜 천장 = Q6 어휘공백(재주입↔재전송) = 벡터/확장 근거점.
- **ChromaDB 비도입(확정)**: ANN=근사라 exact cosine보다 품질 동급↓, 우리 규모(수천 턴) brute-force 충분·정확. 이득은 스케일/운영뿐. 사용자 여러 프로젝트 공통 SQLite 고수(메모리 [[prefer-sqlite-over-vector-db]]).
- **GRPO 비도입(확정)**: RL 정책학습 = 라벨데이터·인프라 필요(우리 없음), 측정 불가, Memora도 experimental.
- **채택(사용자 승인, 무거워도 품질이면 OK)**: cross-encoder 리랭커(secall `model_manager`/`hybrid` 씨앗) + 쿼리 확장(secall `query_expand.rs`, Q6류 어휘공백). 단 리랭커는 임베딩/CE 모델 필요 → **Ollama 터널 의존(현재 DOWN)**.
- **품질 트랙 전략(사용자 문답): 측정-증분, 심판자 우선.** "기능 다 깔고 데이터로 수정"(A)은 귀속불가·비용낭비·락인이라 기각. 순서 = (0) **eval 코퍼스 확대 먼저**(현실 라벨 케이스, 터널 불필요·결정적 FTS로 지금 측정) → (1) 기능 한 개씩 측정·유지/폐기(동시투입 금지) → (2) 프로덕션 데이터는 기능 맹목수정 아니라 **실패 케이스를 eval에 흡수**(hard negative). 기능 "완성(배선+단위테스트)"과 "튜닝(데이터 필요)"은 다른 축. 얇은 eval(10질의)에 튜닝 = 과적합이라 eval 확대가 0번 스텝.
- **다음 품질 슬라이스**: eval 코퍼스 확대(Plan 21 코퍼스 확장판) → 리랭커(터널 복구 후) → 쿼리 확장.

## 2026-06-30 eval 확대 측정 + 리랭커/벡터 분리 (중요)

- **eval 확대 완료(미커밋→커밋예정)**: tests/search_recall.rs 코퍼스 20→40발언, 질의 10→21(어휘·의미공백 질의 추가). 측정 **R@5 0.857 / P@5 0.592 / MRR 0.833**(easy 0.90보다 낮음=변별력↑). floor r5≥0.85, p5≥0.58.
- **핵심 발견 - 두 레버는 다른 문제**:
  - **리콜 공백(FTS 0건/부분)**: Q6 재주입(0.0), Q16 원격접속인증→33 누락(신원확인=어휘 0겹침), Q17 코어백엔드호스팅→35 누락, Q21 오래기억(0.0, '기억' 부재). → **리랭커로 불가**(재정렬은 가져온 것만; recall=0이면 무력). = **벡터(Plan 13 기존)+쿼리확장**의 일.
  - **정밀도/랭킹(가져왔지만 noise)**: Q8 로컬LLM좌석 P@5 0.25, Q19 동의어질의확장 P@5 0.25("확장에"가 msg1 끌어옴). → **cross-encoder 리랭커**의 일.
- **로드맵 정밀화(측정-증분)**: "리랭커부터"가 아니라 **"기존 하이브리드 벡터가 리콜 공백을 메우나" 먼저 측정**(이미 가진 기능, Ollama 터널 필요). 회복되면 쿼리확장 YAGNI. 그 뒤 리랭커=정밀도용(직교). 리랭커 로컬 가능 확인(RTX 3060 Ti 8GB, ~3.7GB 여유; TEI/Infinity 무료; 터널 불요).

## 2026-06-30 벡터 측정 완료 → 쿼리확장·리랭커 둘 다 보류 (측정-우선의 값)

- **터널**: known_hosts에서 2232 호스트 찾아 직접 기동([사설호스트]=[사설IP], d9ng). 모델 bge-m3/gemma4 확인. 하네스: tests/search_recall.rs에 `vector_hybrid_recall`(#[ignore], semantic) 추가, QUERIES 모듈 공용화(FTS/벡터 같은 gold).
- **측정(21질의/40발언)**: FTS R@5 0.857 → **벡터 0.952 / 하이브리드 0.952**, **벡터 MRR 0.976** / 하이브리드 MRR 0.921. FTS 공백 회복: Q16 0.5→1.0, Q17 0.5→1.0, Q6 0→0.667, Q21 0→0.333.
- **결론(측정이 취소시킨 것)**: (1) **쿼리확장 YAGNI 확정** - 벡터가 어휘공백 메움. (2) **리랭커 보류** - 벡터 MRR 0.976(gold 거의 1순위)이라 재정렬 한계이득 미미. 측정 한 번이 두 기능을 안 짓게 막음.
- **단서**: 깨끗한 소코퍼스라 bge-m3에 쉬움. 프로덕션 전사(길고 노이즈·문서多)는 더 어려워 갭 재개 가능 → 그때 리랭커 재검토(로컬 GPU 가능). **하이브리드 MRR < 벡터**: RRF 어휘 arm이 가끔 gold 끌어내림(이 코퍼스선 순수 벡터가 깔끔).
- **검색 품질 트랙 = 현 eval 기준 충분.** 다음 = Stage 1(A2A 오케스트레이션 툴). 검색은 프로덕션 코퍼스 확보 후 재측정.

## 2026-06-30 Stage 2(push->pull) 라이브 측정 - 페이오프 증명 + 권한 블로커 발견

- **Task 1 done(f15911b)**: ContextMode(Push/Pull) + is_mcp_capable + build_round_prompt pull 분기(포인터, prior/retrieved 생략) + --pull-context(--db 없으면 경고+Push) + [ctx] 프롬프트 크기 계측. behavior-preserving. 기본 118/mcp+sqlite 124.
- **Task 2 라이브 측정(실 claude/codex, 3턴, --db, --recent-turns 미설정이라 carried도 빈값)**:
  - **토큰 페이오프 증명**: push는 전사 누적에 선형 증가(claude 284->5184->9770, codex 2453->7623->12489). pull은 평평(claude 433->431->429, codex 2413->2307->2417). claude 95%↓, codex 81%↓. **프롬프트가 전사 길이와 탈동조** = (A) 핵심 페이오프.
  - **블로커 발견(중요)**: pull에서 read_transcript가 **헤드리스 `claude -p` 권한모드서 차단**. claude 응답에 "read_transcript 권한이 막혀 직전 4턴 전사 대신 이전 결론 메모를 근거로" 명시. 게으른 pull 아니라 **하드 권한 블록**. 에이전트는 레포(cwd)+사전지식으로 보충 → 그럴듯하나 **전사 grounding 아님**(예 "상주코어<->접속" = 레포 설계문서에서 읽음). coherence 부분 착시.
  - **결론**: 토큰 감소 실재, 단 현 spawn 설정선 pull 무효. **Task 3 = 러너 spawn에 MCP 도구 권한 자동허용**(claude --allowedTools 또는 permission-mode로 tuna-search 승인, codex 대응) 후 재측정. 측정-우선이 조용한 품질저하를 사전 차단.

## 2026-06-30 Stage 2 Task 3(권한 블로커 해소) - 작동 검증 완료

- **claude 수정(claude.rs build_claude_args)**: ReadOnly + mcp_config 있으면 `--allowedTools mcp__tuna-search__search_context,mcp__tuna-search__read_transcript` 추가. 쓰기 차단(--disallowedTools Write,Edit,Bash) 유지 = read-only 안전. fail-safe(도구명 틀려도 보안구멍 없이 막힐 뿐). 실측 #10: `claude --help`에 --allowedTools(가산 허용)·--disallowedTools·--permission-mode 확인. permission-mode bypass는 fail-unsafe라 미채택.
- **codex는 수정 불필요**: codex exec=비대화형이라 MCP 도구 자동 승인(claude -p와 다름). 재측정서 codex도 전사 인용 확인.
- **codex 서버 모드(사용자 지적)**: codex는 `codex mcp-server`(codex를 MCP 서버로), `codex app-server`(영속 데몬, experimental), `codex mcp`(외부 MCP 관리)가 있음. 우리 러너는 `codex exec`(stateless, codex=우리 tuna-search의 MCP 클라이언트). **app-server/mcp-server = (A) Stage 3 영속 에이전트 세션 경로**(매턴 stateless exec 대신 영속 데몬). 후속 검토.
- **재측정 검증(실 claude/codex, pull, 3턴, 3턴째=합의요약 과제)**: "권한 막힘" 0회. 두 에이전트가 **전사 특정 내용 정확 인용**(codex "전사와 관련 맥락을 확인해" + env_clear/close-on-exec/Tailscale 등 앞턴 결정 요약; claude가 transcript.read scope 등 참조). 프롬프트 평평(claude 433/431/441, codex 2401/3307/2132) vs push(9770/12489). **토큰 80~95%↓ + grounding 유지 = (A) push→pull 페이오프 실증.**
- **Stage 2 작동 검증 완료.** half-a2a 척추 동작. 남은 폴리시: --recent-turns+carried 병행 시 동작, 포인터 문구 튜닝, pull 기본화 결정(품질 더 측정 후). get_roster/post_turn은 Stage 1 후속.

## 2026-06-30 Stage 2 pull 하더닝 - carried 요약 항상 켜기(안전망)

- **변경(repl/mod.rs carry_forward_digest)**: 모드 분기. Pull이면 전사 전체를 요약 대상(prior 통째 안 넣으니), Push면 기존(recent_turns 밖 드롭만). MAX_CARRY=1500 캡 그대로라 평평. Push 기본 동작 불변(기존 4테스트 통과 + 신규 pull 테스트 1).
- **이유**: 직전 측정은 carried 빈 상태라 pull이 순수 에이전트 당김에만 의존(강한 모델이라 됐으나 안전망 0). 이제 pull은 요약 baseline 항상 보유 → 게으른 pull에도 연속성.
- **재측정(실 claude/codex, pull, 4턴)**: claude 435→566→696→855(요약 턴당 ~130자 누적, 1500서 캡 예정=유계). codex 2262~3283(same_round 변동). push(9770/12489 무한증가) 대비 여전히 평평/유계. grounding 유지(앞턴 결정 참조). **안전망 + 토큰 페이오프 동시 성립.**
- 남은 폴리시: 더 다양/긴 시나리오·약한 좌석 측정 후 pull 기본화 결정. 포인터 문구 튜닝. 이후 Stage 3(영속 에이전트=codex app-server, 원격/토폴로지, post_turn/get_roster 흡수).

## 2026-06-30 코드리뷰 + Batch A + codex pull 권한 버그 발견(이전 판단 정정)

- **코드 전반 리뷰(리뷰 에이전트 + Opus 검증)**: 확정 버그 4 + 잠재 3 + 반증 2. 상세는 세션 로그.
- **Batch A 커밋(1b13ac7)**: (1) get_message `.ok()`→QueryReturnedNoRows만 None·실에러 전파 (2) path_to_root id→msg HashMap(O(N²)→O(N))+visited 순환가드 (3) codex TOML single→double-quote basic+이스케이프(toml_basic, 주입 차단) (4) DefaultHasher→FNV-1a 인라인(버전무관 결정적). 신규 5테스트, 기본 123/전체 129.
- **⚠ codex pull 권한 버그 발견(Stage 2 정정)**: codex 변경 라이브 검증 중, **codex exec도 MCP 도구 호출을 승인 막음**(read_transcript "사용자 취소" 3회). **이전 "codex는 exec 비대화형이라 자동승인=무수정" 판단은 틀림.** Task 3 재측정의 codex 응답은 same_round+레포 보충이었고 실제 pull 아니었음. 즉 **pull은 현재 claude만 작동, codex 좌석은 전사 못 당김.**
- **codex 권한 후속 수정 후보**: `codex exec`에 세밀 승인은 `--dangerously-bypass-approvals-and-sandbox`(샌드박스까지 제거=ReadOnly 부적합)뿐 보이고, `-c approval_policy="never"`(승인 안 물음, 샌드박스는 유지) 추정 → 검증 필요. ReadOnly 좌석은 MCP 허용+쓰기 차단 동시 필요. Batch B(리팩토링) 전 우선 처리 권고(shipped 기능 버그).

## 2026-06-30 codex pull 권한 = claude 전용 pull로 결정(검증)

- **시도**: codex.rs에 `-c approval_policy="never"` 추가 → **라이브에서 codex read_transcript 여전히 "사용자 취소"**. approval_policy가 codex MCP 도구 승인을 관장 안 하거나 값이 다름. codex exec엔 -a/--ask-for-approval 플래그 없음(-c config만), granular MCP 승인 키 미발견. 추정 되돌림.
- **결정(정직한 폴백)**: `is_mcp_capable` → **claude 전용**. codex는 pull 모드에서 push 폴백(전사 전체 주입=grounded). 혼합 모드(claude pull + codex push) 라이브 검증: claude 432(pull) / codex 2024→6015(push), codex가 "첫 주제" 정확 답변=grounded, "사용자 취소" 사라짐.
- **codex pull 활성화 = 후속**: codex 승인 설정 심층 조사(mcp 서버 trust? config 키?) 또는 Stage 3e 영속 codex(app-server, 승인 모델 다를 수 있음). 그 전엔 claude만 pull 이득, codex는 정확하지만 push.
- **신규 테스트**: orchestrator pull_capable_is_claude_only.

## 2026-06-30 Stage 3a-2 remote core e2e 검증 + 핸드오프(세션3)

- **3a-2(502e458)**: 러너 with_search_url(url,token). claude=HTTP config(type:http+url+Bearer 헤더, serde_json), codex=-c mcp_servers.tuna-search.url(bearer env는 ExecSpec env 필드 필요해 TODO). main --search-url/--search-token, roster build_registry 4인자. search_url 미설정 시 stdio 불변.
- **라이브 e2e(성공)**: 코어 `--serve-mcp 127.0.0.1:8766 --db shared2.db --token TOK123`(serve feature) 상주 → 별도 REPL이 `--db shared2.db --search-url http://127.0.0.1:8766/mcp --search-token TOK123 --pull-context`. claude(pull, 439/646자)가 **원격 HTTP MCP(bearer)에서 read_transcript 호출 → 별도 프로세스가 쓴 전사 정확 인용**("전사를 확인했습니다. 첫 주제는 이벤트소싱 vs CRUD..."). 인증: no-token 401, with-token 통과. **remote core = half-a2a 네트워크 실증.**
- **세션3 핸드오프**: docs/prompts/v2-handoff_2026-06-30_session3.md. README·CLAUDE.md(상태 세션3)·핸드오프 갱신.
- **3a 잔여**: 3a-3 front=core(REPL+HTTP MCP 단일 프로세스, 현재는 serve+REPL 2프로세스로 e2e), 3d post_turn/get_roster, codex bearer-env(ExecSpec env), 영속 에이전트(3e 보류).

## 2026-07-01 세션5: step 5c recency 랭킹 설계 노트(착수 전)

- **목표**: cross-session 최신성을 랭킹에 약하게 반영. msg_id는 세션별이라 세션간 비교 불가 → messages에 created_at(절대 타임스탬프) 필요.

- **⚠ 함정 1 (save_session 타임스탬프 리셋)**: save_session은 세션을 **전량 DELETE+INSERT**한다(sqlite.rs:206~). INSERT에서 created_at=datetime('now')를 쓰면 **스냅샷 저장할 때마다 모든 메시지의 created_at이 now로 덮어써져** recency 신호가 무의미(전부 마지막 저장 시각)해진다. StoredMessage엔 created_at 필드 없음(step 4에서 리터럴붕괴·직렬화 하위호환 회피로 원문/메타 분리한 방침 유지). **해결=save_session 트랜잭션 안에서 DELETE 전에 기존 (msg_id→created_at) 맵을 SELECT해두고 재INSERT 시 COALESCE(기존값 있으면 유지, 없으면 now).** append_turn은 순수 증분이라 그냥 now.

- **⚠ 함정 2 (ALTER 비상수 default 불가)**: SQLite는 `ALTER TABLE ADD COLUMN`에 비상수 default(`datetime('now')`/`CURRENT_TIMESTAMP`)를 금지한다. → created_at은 **nullable(DEFAULT 없음)** 컬럼으로 추가하고 값은 INSERT에서 명시. model_id(v3) 마이그레이션과 동일 패턴. 기존 행은 NULL → 랭킹에서 "가장 오래됨"으로 관대 처리.

- **결정 필요 (recency 강도 정책, precision 트레이드오프)**: rerank는 현재 정수 penalty 버킷+안정정렬이라 penalty가 relevance보다 우선. recency를 penalty에 더하면 **relevance보다 강해져** 매우 관련성 높은 오래된 발언이 강등될 위험(OR-query precision 트레이드오프와 동종). 두 정책:
  - **정책 A(보수, 추천)**: valid_state가 이미 명시적 노후화 채널(supersede/reject/stale)이므로 recency는 그 위에 약하게만. 다른 세션의 낡은 후보에만 소폭 penalty(예: 최신 후보 대비 age 임계 초과 시 +1), 현재 세션·active·관련성 높은 건 보존. 설계 토론 도구 특성상 "오래됐지만 관련 높은 초기 결정"을 못 찾으면 손해.
  - **정책 B(적극)**: 낡음을 시간으로 확실히 강등(age 버킷 penalty를 validity와 합산). 최신성 강하게 반영하나 precision 훼손 가능.
  - Opus 추천=**A**. 이유: 유효성 랭킹이 노후화를 이미 담당, recency는 동률대 보조로 충분. **사용자 확정=A(2026-07-01).**
- **정책 A 구현 확정**: recency penalty `+1`을 (현재 세션 아님) && (created_at 존재) && (후보집합 max created_at 대비 임계 초과) 교집합에만. off-branch/superseded와 동급 최소 강등. **NULL created_at=판단 유보 penalty 0**(마이그레이션 기존행 관대 처리, "가장 오래됨"보다 보수적). 임계는 후보 상대 기준(비결정 now 회피=테스트 가능), 타임스탬프 단조 파싱은 월별 일수 근사 허용(약한 신호).

- **구현 완료(미커밋)**: 스키마 v5. sqlite.rs=CREATE_MESSAGES created_at TEXT + migrate ALTER(column_exists 가드) + save_session 보존(DELETE 전 msg_id→created_at 맵 SELECT, INSERT는 COALESCE(?6, datetime('now'))) + append_turn datetime('now') + get_created_at/set_created_at. retriever.rs=rerank 2-pass(1차 validity/분기 penalty+created_at 수집+max_ts, 2차 다른세션 낡은 히트 +1) + parse_ts_approx + RECENCY_STALE_SECS=7일. let-chain으로 중첩 if 병합(clippy). 신규 테스트 3(migration_v4→v5·save_session 보존·recency 강등). **기본 163/features 177 pass, clippy 클린.** 커밋 메시지 후보: `feat(store): cross-session recency 랭킹 + created_at 컬럼 (로드맵 step 5c)`.
- **⚠ 라이브 미검증**: created_at이 실제 REPL 경로(save_session/append_turn)에서 채워지는지, recency 강등이 실 다중세션에서 체감되는지는 미검증(단위·통합 테스트만). step 6 실코퍼스 확보 시 함께 라이브 확인 권장.

## 2026-07-01 세션5: step 5c 라이브 검증(/explain 확장)

- **결정**: 라이브 검증을 옵션2(창구 확장+합성 aging)로. 이유: recency는 세션 간 7일 초과 간격이 있어야 발동 → 새 라이브는 전부 오늘 타임스탬프라 유기적 관찰 불가(그건 step 6 실코퍼스 몫). `--mcp-search`는 stdio 서버라 원샷 조회 아님. `/explain`(debug_retrieve)이 REPL의 검증 창구인데 created_at/recency를 안 보여줬음.
- **변경**: `debug_retrieve`에 `created=<날짜>` + `recency↓`(다른세션 && 후보최신 대비 7일 초과) 표시 추가. rerank와 동일 규칙(parse_ts_approx·RECENCY_STALE_SECS 공유). 신규 테스트 `debug_retrieve_marks_stale_cross_session_recency` + 기존 debug 테스트에 created= 확인 추가.
- **라이브 결과(실 라이브러리 코드, 임시 example로 seed+8일aging 후 삭제)**: (1) plumbing=save_session이 created_at을 실 타임스탬프("2026-07-01 03:16:42")로 채움 확인. (2) /explain 출력에 aged 세션만 `created=2026-01-01 recency↓`, 최신 세션은 무표시. (3) retrieve 순서=최신("재설계")이 낡은 것("설계")보다 앞. **step 5c 랭킹·plumbing 라이브 확정.**
- **미검증 잔여**: sqlite3 CLI 부재로 파일 직접 aging은 불가했고 example 경유. 유기적(며칠 간격 실 다세션) 관찰은 step 6로 이월. `/explain`이 이제 recency 검증 상시 창구.

## 2026-07-01 세션5: 잔여 항목 배치(안전성 + codex bearer-env)

- **A. 안전성/견고성 배치**:
  - #1 KiwiWrapper `unsafe impl Send`: 코드 유지, SAFETY 주석만 강화(항상 Mutex 보관=동시접근 없음이 근거, 잔여 리스크=libkiwi 내부 TLS/전역 스레드정체성 미확인, thread_local 대안은 인스턴스당 init 비용으로 비채택, Windows는 Kiwi 제외=비활성 경로). morphology 피처.
  - #2 session_bus `unbounded_channel`→`channel(1024)`: `enqueue()` 헬퍼가 `try_send`(Full=drop+경고 로그, Closed=무시). REPL 동기 스레드가 Redis 지연에 블로킹 안 되도록 try_send만. fire-and-forget·non-blocking API 불변, 무한 증가(OOM) 방지.
  - #3 `Session::snapshot_json` `unwrap_or_default`→실패 시 eprintln 후 빈 문자열. 빈 스냅샷을 조용히 발행해 크로스터미널 상태 덮어쓰는 것 방지(직렬화 실패 확률은 낮지만 로그로 가시화).
- **B. codex bearer-env(3d 후속 TODO 제거)**:
  - `ExecSpec.env: Vec<(String,String)>` 필드 추가 + `run_with_watchdog`가 `cmd.env(k,v)` 적용. claude/opencode/exec-test는 `env: Vec::new()`.
  - codex `run()`의 mcp_args 조립을 `build_mcp_wiring()` 메서드로 추출(spawn과 분리=테스트 가능, `(mcp_args, child_env)` 반환).
  - search_url+search_token 둘 다 Some이면 `-c mcp_servers.tuna-search.bearer_token_env_var="TUNA_SEARCH_TOKEN"`(config엔 변수명만) + `child_env`에 실제 토큰(자식 프로세스 env로만 전달=argv/config 비노출). 상수 `BEARER_TOKEN_ENV="TUNA_SEARCH_TOKEN"`.
  - 단위테스트 3: 토큰이 env로 가고 argv엔 없음 / 토큰 없으면 bearer 배선 없음 / url 우선. 기존 url 테스트도 build_mcp_wiring 직접 호출로 강화.
  - **⚠ 라이브 미검증 + 한계**: 이전 결정대로 codex exec는 MCP 도구 호출 승인이 막혀 pull=claude 전용. bearer는 codex의 원격 서버 인증 배선을 완결하나, codex가 실제로 search_context/read_transcript를 호출하려면 승인 문제가 별도로 풀려야 함. 즉 이 커밋은 "인증 준비 완료"지 "codex 원격 pull 작동"은 아님. 라이브 e2e는 승인 해소 후.
- **abstraction/anchors(C)**: 별도 세션. message_validity 컬럼은 있으나 채우는 로직(에이전트 요약/앵커 추출) 설계 필요. set_annotation은 준비됨.

## 2026-07-01 세션5: codex 실행 모드 조사(pull=claude-only 원인 확정, 업스트림 근거)

- **배경**: 동구님이 "codex는 -p 외 다른 모드가 있고 tunaFlow가 그걸 쓴다, 업스트림 참고하라" 지적. codex pull=claude-only 결정 재검토.
- **확인(설치 codex-cli 0.142.5)**:
  - `-p`는 codex에선 `--profile`(config profile)이지 claude의 print 아님. codex 비대화형=`exec`. 우리 러너는 `codex exec --json --sandbox <mode> -`(정상적인 모드 선택).
  - codex 서브커맨드: exec / app-server[experimental] / exec-server[EXPERIMENTAL] / mcp / mcp-server / remote-control. `--full-auto`는 0.142.5 exec엔 없음(구 tunaFlow가 쓰던 것, 게다가 full-auto=workspace-write라 read-only 아님).
  - 승인 모델=프로젝트 `[projects."path"] trust_level` + `approval_policy` + `sandbox`. `-c approval_policy="never"`는 예전 시도서 MCP 도구 "사용자 취소" 못 막음.
- **업스트림 확정(핵심)**: openai/codex **issue #24135** = "codex exec: MCP 도구 호출을 --dangerously-bypass-approvals-and-sandbox 없이 비대화형 허용 불가". `approval_policy=never`·`mcp_approval_policy=never`·`tools_require_approval=false`·`trusted_mcp_servers` 전부 무효. 유일 우회=`--dangerously-bypass-approvals-and-sandbox`(샌드박스까지 제거). **즉 우리 "codex exec에서 pull 불가"는 우리 버그 아니라 codex 제약이 맞았음.**
- **tunaFlow의 실제 방식**: (1) 구 exec `--full-auto`, (2) 현 `codex app-server`(영속 WS/JSON-RPC v2, codex_app_server.rs). **둘 다 read-only 유지 안 함**: app-server도 `thread/start`에 `approvalPolicy:"never" + sandbox:"danger-full-access"`(claude --dangerously-skip-permissions 등가) 사용. 즉 tunaFlow는 codex에 풀 액세스를 주는 트레이드오프를 택함.
- **우리 함의(결정 필요)**: codex pull 활성화 = read-only 포기(빠름·불안전) 또는 app-server + 선택적 승인 구현(정석·큼=Stage 3e).
  - 옵션A: exec에 `--dangerously-bypass-approvals-and-sandbox`(또는 app-server danger-full-access). ~1줄로 오늘 pull 작동하나 **codex ReadOnly 좌석이 파일편집·쉘 가능=read-only fail-safe 붕괴**(우리 제품 핵심 가치와 충돌). 비권장.
  - 옵션B: `codex app-server` 영속 러너 포팅 + `approvalPolicy=on-request`류로 per-tool 승인 이벤트를 받아 **MCP 읽기 도구만 프로그래밍적 승인, 쓰기/쉘은 거부**. read-only 의도 보존. 상당한 신규 작업(WS 클라이언트+프로토콜+승인 이벤트 루프), = Stage 3e 영속 codex. tunaFlow는 이 선택적 승인까진 안 하고 그냥 never+full-access.
  - **결론(현시점)**: codex pull=claude-only 유지가 정당(문서화됨). 옵션B(app-server 선택적 승인)를 Stage 3e로 스케줄. bearer-env 커밋은 exec에선 무의미하나 app-server 원격 인증에 재사용되므로 forward-useful.

## 2026-07-01 세션5: codex pull 활성화(behavioral read-only) 구현

- **결정(동구님)**: codex는 규칙 준수가 강해(Claude/Gemini와 달리 명시적 unlock 요청도 안 어김) read-only를 샌드박스가 아니라 지시로 강제 가능. → 옵션A를 채택하되 안전하게: (1)강제수단=프롬프트 지시 주입, (2)발동=pull+ReadOnly+MCP일 때만.
- **구현**: is_mcp_capable=claude|codex. RunInput에 `pull: bool`(+Default 파생, RunMode Default=ReadOnly)로 run_round가 per-seat pull(ctx_mode==Pull && is_mcp_capable)을 러너까지 전파. codex `build_codex_args(input, mcp_args, bypass)`: `ReadOnly && input.pull && (search_url|search_db)`이면 `--dangerously-bypass-approvals-and-sandbox`로 `--sandbox read-only` 대체(exec의 MCP 승인 우회 유일 수단). 같은 조건에서 프롬프트에 `READONLY_DIRECTIVE`(편집·변경성 명령 금지, 예외 없음) 접두. Write=workspace-write, 비pull ReadOnly=read-only 유지(불변).
- **트레이드오프(수용)**: bypass는 fs/네트워크/셸 전부 개방 → read-only가 하드(샌드박스)→소프트(codex 규칙 준수)로 하강. pull+ReadOnly+MCP로 발동 범위 최소화. 잔여 리스크=codex가 규칙 무시 시 실제 편집 가능(관측상 안 일어남 전제).
- **검증**: 기본 161 / features 175 pass, clippy 클린. 신규 테스트 args_readonly_bypass_replaces_sandbox + is_mcp_capable(claude|codex).
- **⚠ 라이브 e2e 통과(2026-07-01)**: 실 codex 0.142.5. 구성=`--serve-mcp 127.0.0.1:8791`(seed.db 세션 default에 "이벤트소싱 채택, 코드명 PELICAN" 1턴) + 별도 codex-only 로스터 pull REPL(`--search-url .../mcp --search-token --pull-context`). 결과: codex가 **tuna-search MCP 도구를 실제 호출**(예전 "사용자 취소" 사라짐)→전사를 정확 인용("코드명 PELICAN"은 프롬프트에 없고 전사에만=진짜 pull 증거) + "읽기 전용으로 전사만 확인" 명시하며 **파일 변경 0**(bypass여도 규칙 준수). `[ctx] seat=codex mode=pull` 확인. **behavioral read-only + codex pull 실증 완료.** (임시 seed_e2e.rs 예제·target/e2e 아티팩트는 검증 후 삭제.)
- **관련**: 이번에 codex도 pull 가능해지며 세션4의 bearer-env(원격 HTTP MCP 인증)가 비로소 exec 경로에서도 의미. 단 원격 코어+bearer 조합 라이브는 별도.

- **범위**: 스키마 v5 + INSERT 2경로 created_at + rerank recency + created_at 읽기 + 테스트(마이그레이션·save_session created_at 불변·recency 동작·기존 랭킹 불변). 외부 백엔드/코퍼스 불요, 자체 완결.

## 2026-07-01 세션5: step 6 실코퍼스 regression (seCall 복구 후)

- **배경**: 앞서 seCall이 semantic 다운 + 한국어 keyword 비동작으로 막혔으나, 재시도 시 **복구됨(v0.6.4, 3142세션/53726턴, semantic+한국어 BM25 동작)**. project=tunaRound 세션들이 뜸(이 세션 dff85fb8 포함, 06-30 아키텍처리뷰 6274470d 722턴 등).
- **코퍼스**: seCall 실 턴에서 발췌한 18발언(출처 주석 session:turn). 6274470d:175(대형 아키텍처 리뷰=형태소FTS·RRF·임베딩무효화·recency·분기·retrieved cap·kiwi unsafe 등 다논점)·37b034cb:2(캐시 content-addressed/gen-stamp)·6274470d:89(HTTP코어 bearer)·dff85fb8(codex #24135/behavioral·recency v5·debug_retrieve). 1발언=1논점으로 분해.
- **질의**: 12개, 코퍼스 원문과 다른 표현(굴절·동의어·외래어)로 변형해 형태소 검색 실난이도 측정. tests/real_corpus_recall.rs(search_recall.rs 패턴, lindera 결정적, 하드코딩).
- **측정 결과**: **mean R@5 0.958 / P@5 0.621 / MRR 1.000**(n=12). 합성 확장셋(0.857/0.592)보다 높고, MRR 1.0=모든 질의 첫 히트가 gold. 유일 약점=Q2 "모델 바꾸면 재색인"(재색인↔무효화 동의어 갭, R@5 0.5, 3은 찾고 11 놓침). 결론: **검색 스택이 실 한국어 설계토론 어휘(굴절·외래어·코드용어 BM25/codex/pull/bearer)에서도 품질 유지 실증.** 합성 코퍼스 대표성 우려 해소.
- **회귀 가드**: floor R@5>=0.85, P@5>=0.55. 새 파일 clippy 클린(기존 테스트 4경고는 --tests 전용·범위밖).
- **한계(정직)**: 18발언 소규모(검정력 제한), 라벨=Opus 도메인 판단(주관성), 발언이 주로 assistant 턴이라 문체 동질. recency 유기 검증은 step 5c 라이브로 이미 실증해 별도 테스트 생략(실 날짜 코퍼스라 향후 확장 여지).

## 2026-07-02 세션5: 임베딩 모델 비교 bge-m3 vs qwen3-embedding:0.6b

- **동기**: qwen3-embedding:0.6b가 bge-m3보다 낫다는 얘기 확인 요청. 실측으로 판단.
- **셋업**: Ollama 터널(11435)에 두 모델 존재, 둘 다 dim 1024(드롭인 교체 가능). tests/embed_model_compare.rs(#[ignore] 수동): real corpus 18발언에 각 모델로 색인→vec-only/hybrid의 R@5·MRR을 나란히.
- **결과(12질의)**: vecR@5·hybR@5는 둘 다 1.000(recall 포화=코퍼스가 작고 쉬움). 차이는 MRR: bge-m3 vecMRR 0.903/hybMRR 0.917 vs **qwen3 vecMRR 0.958/hybMRR 1.000**. qwen3가 12질의 전부 gold를 1순위에 놓음. bge-m3는 "풀 모드 토큰 절감"·"검색 디버그 창구"에서 1순위 놓침. **결론: qwen3-embedding:0.6b 랭킹 우위 실증(단, recall 포화라 차이는 MRR에만, 소코퍼스 검정력 한계, qwen3 색인 체감 느림).**
- **적용**: OllamaEmbedder::from_env() 추가(TUNAROUND_OLLAMA_URL + TUNAROUND_EMBED_MODEL, DEFAULT_MODEL=qwen3-embedding:0.6b). main.rs 4곳 하드코딩 "bge-m3"→from_env()로 DRY. bge-m3 복귀=TUNAROUND_EMBED_MODEL=bge-m3. 모델 교체 시 model_id 무효화 키가 재임베딩 자동 처리(step 2 인프라 덕). README·CLAUDE.md 갱신. 기본 161 pass, clippy 클린.

## 2026-07-02: step 6 실코퍼스 확장 (seCall 패치 후) + 외래어 갭 발견

- **seCall 패치**: 세션 3142→6352, 턴 53726→255769 대규모 재수집. 한국어 멀티텀 keyword는 여전히 co-occurrence 의존(특정 질의 0), semantic이 다양성 우위(단 재수집분 벡터 40581로 뒤처짐).
- **확장**: 패치 재수집이 드러낸 **실제 설계토론 세션**(e5a848d3 06-30, proposer/reviewer 역할 + 리프레시 토큰 회전 논쟁 = auth/보안 도메인, 비메타 세션)에서 5발언 추가. 코퍼스 18→23발언(검색인프라+auth 2도메인), 질의 12→15. 현재세션 쏠림·문체 동질 한계 완화.
- **재측정**: mean R@5 0.878 / P@5 0.494 / MRR 0.900 (18발언 때 0.958/0.621/1.0보다 하락). floor R@5>=0.80, P@5>=0.42로 조정.
- **⚠ 실발견(가치)**: 질의 "리프레시 토큰 어디 저장"(gold=발언20) **R@5 0.0 완전 누락**. 원인=질의는 한국어 외래어 "리프레시", 발언은 영어 "refresh" → **FTS 형태소가 외래어 음역(리프레시↔refresh) 갭을 못 이음**. 외래어 표기 정규화(romanize/음역 매핑) 미구현. 쉬운 합성/소코퍼스가 숨긴 실패모드를 확장 실코퍼스가 노출 = step 6의 본래 목적 달성. **개선 후보**: 토크나이저에 외래어 음역 정규화 또는 영한 병기 색인. auth 질의가 검색-인프라 발언 distractor 유입으로 P 하락도 관측.
- **결론**: 검색 스택은 도메인 내 어휘엔 강하나(R@5 0.88, MRR 0.9), 외래어 한↔영 음역 경계에 실갭 존재. 실코퍼스가 이를 정직하게 드러냄.

## 2026-07-02: FTS vs 하이브리드 실측 - 로안워드 갭은 임베딩으로도 안 메워짐(반증)

- **가설**: 외래어 음역 갭(리프레시↔refresh)은 FTS-only 한계고 다국어 임베딩(qwen3/bge-m3) 하이브리드가 메울 것.
- **실측(real_corpus_hybrid_recall, #[ignore], qwen3-embedding)**: 하이브리드 mean R@5 0.933/MRR 0.933 (FTS-only 0.878/0.900보다 상승). 동의어 갭 회복 확인: "모델 바꾸면 재색인" 0.5→1.0, "토큰 회전 필요한가" 0.667/0.5→1.0/1.0. **그러나 "리프레시 토큰 어디 저장"은 하이브리드도 R@5 0.0**(발언20=access메모리/refresh keychain/SPA httpOnly, 영어 조밀). 관련 refresh-토큰 발언(19/21/23, 한국어 조밀)이 위로, 정작 storage 답(20)은 top5 밖.
- **결론**: 다국어 임베딩은 **동의어·의역**은 잘 잇지만, **한국어 로안워드 질의 ↔ 영어term 조밀 발언**의 교차스크립트+코드믹싱은 못 이음(FTS·벡터 둘 다). → 이 케이스엔 **어휘층 병기(alias) 색인**이 직접적. ES synonym 필터 패턴을 토크나이저에 이식(리프레시↔refresh 등 개발 외래어 사전, 양방향 index+query 확장). 결정적·고정밀, 라이브러리 불요(음역 자동화=Knight&Graehl/NEWS 태스크지만 개인도구엔 과함, 로마자변환 라이브러리는 리프레시→ripeuresi라 refresh와 무관=해법 아님).

## 2026-07-02: 외래어 음역 병기 색인 구현 (로안워드 갭 해소)

- **구현**: search/mod.rs LOANWORD_GROUPS(음역 페어 32그룹: refresh↔리프레시, embedding↔임베딩 등, 모호단음절 풀/락/큐 제외) + loanword_aliases(token). tokenizer.rs fts_query(default trait)가 질의 토큰별 alias를 사후 추가(index 무변경=재색인 불요, 모든 백엔드 공유). main.rs 비morphology fallback 2곳도 동일 확장. 번역(검색↔search)은 제외=임베딩 담당(noise 회피).
- **효과(실측)**: 목표 갭 해소 - "리프레시 토큰 어디 저장" R@5 0.0→1.0(질의 리프레시→refresh 확장이 영어 조밀 발언20과 이어짐). FTS mean R@5 0.878→**0.944**, P@5 0.494→0.508. 하이브리드 R@5 0.933→**0.978**. 대가=MRR 소폭↓(FTS 0.900→0.869, hyb 0.933→0.883): OR 확장이 상위 랭크 흔듦, top-k 주입 용도엔 수용(recall 우선). **합성 코퍼스(search_recall) R@5 0.857 불변**=기존 훼손 없음. 기본 164 pass(loanword 단위 3 추가), clippy 클린.
- **설계 판단**: 음역만 겨냥(의역은 임베딩이 이미 처리 실측). 질의확장 방식(index 불변). 자동 음역모델(Knight&Graehl/NEWS) 비채택=개인도구 과함. 흔한 공통어(토큰/token) alias는 noise 대가 있으나 소코퍼스 과적합 회피 위해 유지(향후 실사용서 재튜닝 여지). floor R@5>=0.88, P@5>=0.45로 상향.

## 2026-07-02: 온보딩 Stage 1 clap 서브커맨드 (Sonnet5 위임 + Opus 리뷰)

- **위임**: main.rs 787줄 수동 arg 파싱 → clap 서브커맨드 리팩터를 Sonnet5 서브에이전트에 위임(규율: 구현=Sonnet, Opus 리뷰). 정밀 스펙(서브커맨드 매핑·러너 spawn 계약·feature 게이트·검증) 제공.
- **결과**: `Cli { command: Option<Commands> }`(None→Chat 기본, 하위호환). Commands=Chat/Core/Serve/Join/McpSearch/Reindex, Core/Serve=serve·McpSearch=mcp·Reindex=sqlite로 cfg 게이트. CommonSessionArgs(db/roster/recent-turns/pull-context/session/search-url/search-token) flatten. match가 서브커맨드→기존 지역변수로 매핑하고 **모드 본문(tokio rt 이후)은 원본 불변**=behavior-preserving. join=chat+search_url/pull_context 프리셋. db_path lazy-init(모든 컴파일 분기가 채움, serve→mcp→sqlite 의존이라 unconditional 대입 안전) + fn main #[allow(unused_assignments)](no-default-features dead-store만, 문서화).
- **러너 계약**: codex build_mcp_wiring·claude build_mcp_config(신규 추출)의 self-exe spawn 첫 인자 `--mcp-search`→`mcp-search`(서브커맨드). 회귀 가드 테스트 추가.
- **Opus 독립 검증**: 기본 test 166lib+6cli, features 180lib+9cli, clippy 클린(features·no-default), 빌드 3조합 클린. 보고와 일치. main.rs diff 정독=매핑 정확·본문 불변 확인.
- **의도된 파괴변경**: bare `tunaround file.json`(서브커맨드 없이 positional)은 이제 clap 에러. `chat file.json` 필요(설계문서 명시). README 예시 전부 서브커맨드형으로 갱신.
- **다음**: Stage 2 cargo-dist(dist-workspace.toml + release.yml, homebrew+powershell, features semantic/mcp/serve) → Stage 3 tunaround.toml 프로파일.

## 2026-07-02: 배포 Stage 2 cargo-dist 설정 (릴리스 미발행)

- **설치**: cargo-dist(dist) 0.31.0 로컬 설치(sshc와 동일 버전, D:\.cargo\bin). powershell 인스톨러.
- **설정**: `dist-workspace.toml` 작성(sshc 답습 + `features=["semantic","mcp","serve"]`=풀기능 단일바이너리). Cargo.toml에 description/repository/homepage 추가(dist가 repository 요구, formula용). `dist generate`로 `.github/workflows/release.yml`(14.5KB, 앱-불특정, CI가 런타임에 dist plan/host). 
- **검증(dry-run, 릴리스 안 함)**: `dist generate --check` 동기 OK, `dist plan`이 v0.1.0에 6타깃(mac arm64/x86, win arm64/x86 msvc, linux arm64/x86) 바이너리 + shell/powershell/homebrew installer + tunaround.rb formula + 체크섬을 경고 없이 announce. cargo build 클린.
- **미결/리스크**: (1) **license 미정** = 동구님 결정(dist는 강제 안 하나 정식 릴리스엔 필요, sshc는 MIT). (2) **크로스컴파일 리스크**: sshc(순수 TUI)와 달리 tunaRound는 rusqlite bundled(C 컴파일)·reqwest rustls(ring/aws-lc C)·axum이 있어 특히 aarch64-linux 크로스에서 실패 가능. 첫 릴리스 CI에서 확인, 실패 시 해당 타깃 제외 또는 zigbuild 조정.
- **방침**: 태그 미푸시 = 릴리스 안 나감. 도그푸딩 후 동구님 승인 시 `git tag v0.1.0` 푸시([[dogfood-before-release]]).

## 맥 왕복 검증 + 릴리스 도그푸딩 (2026-07-02, 맥 aarch64)

- 윈도우 개발분 pull(HEAD 7428fd7→d4526a7). 맥에서 **빌드·테스트·설치 전부 통과**: `cargo build`(기본/풀피처), `cargo test` 195/212, clippy 클린, `cargo install --features "semantic mcp serve"`(release) → `~/.cargo/bin/tunaround v0.1.0`. **크로스플랫폼 컴파일 이슈 없음**(rusqlite bundled·rustls·axum·kiwi-rs·lindera 맥 OK).
- **E2E 도그푸딩**: tunaround chat로 "v0.1.0 릴리스 준비" 토론 → claude+codex 정상 라운드, 결과문서·DB 생성. graceful 저하 확인: Kiwi→lindera(자산404), semantic→FTS(터널없음), 미설치CLI→`[에러] Spawn`(패닉X). **판정=v0.1.0-rc.1 먼저**(6타깃 CI 미검증).
- **크로스머신 A2A 스모크(맥→윈도우 코어 192.0.2.10:8770)**: 네트워크 401/200 ✅, **claude가 원격 전사 ALBATROSS 인용 = half-A2A 읽기 실증 ✅**. 단 codex read_transcript 취소(codex pull 취약, 기존 한계). 임시 핸드오프 `docs/prompts/smoke-cross-machine_2026-07-02.md`(완료 후 삭제 예정, codex leg 남아 보류).
- **릴리스-준비 배치 처리**: README macOS 상태 갱신(확인됨) · CLAUDE.md `install-kiwi-*.sh`→windows 하나 정정 + 맥 Kiwi 실측 · `CHANGELOG.md` 최소본 · `dist plan` 6타깃 유효 · `docs/reference/release-readiness-v0.1.0_2026-07-02.md`(도그푸딩+검증+체크리스트).

## 2026-07-02 오후: A2A 성숙도 정직화 (용어 정렬) + rc.1 CI + 크로스머신 스모크

- **크로스머신 스모크(claude ✅ / codex 실패)**: 윈도우 serve 코어(.179:8770, 시드 ALBATROSS) ← 맥 join(.184). 맥 claude가 원격 read_transcript로 ALBATROSS 인용 = **half-A2A 읽기 크로스머신 실증**. codex leg는 "사용자 취소"(#24135) — 윈도우 loopback(PELICAN)은 됐으나 맥-원격 실패 = 환경 의존 취약. codex 후속(app-server or 대화형 승인).
- **rc.1 CI(맥 주도)**: 도그푸딩 판정으로 v0.1.0-rc.1 먼저. CI가 우리 예측 리스크를 실제로 노출 - **aarch64 크로스(arm64-win/linux) ring C 크로스컴파일 실패** → 맥이 4타깃(mac arm64/x86, win x64, linux x64)으로 축소 + 버전=태그 일치 + [profile.dist] 추가. 최신 run 진행중. **CI는 맥이 잡음, 윈도우 미개입.**
- **A2A 용어 정렬(동구님과 합의)**: 코드/커밋의 "half-a2a"는 **데이터 평면(공유 전사 pull/post)만** 뜻함. **제어 평면(누가·언제·왜·종료)은 사람** = 현재는 "공유맥락 + 사람 오케스트레이션". 동구님이 목표로 말한 "진짜 A2A"는 **자율 제어 평면=AutoLoop(Stage 4, 미구현)**: 모더레이터 에이전트가 턴·종료 자율 결정 + 합의/교착 감지 + (선택)영속 에이전트. 설계가 "사람 주도(종료는 사람)"로 일부러 뺐고 "경제 조건 입증 시에만". 최소 경로=/debate 고정N→LLM 모더레이터. **비명시적 AutoLoop 없음이 맞음.**
- **부수 통찰**: 코어=범용 공유토론 MCP서버. 대화형 Claude/Codex 터미널 2개에 코어를 MCP 등록하면 공유토론 가능(대화형 codex는 사람 승인→#24135 우회). 협업 크로스머신=git + 핸드오프 문서(맥락 운반), 전사는 코어공유면 live. 사람 relay 없애기=gh watch(CI)·/loop git-fetch·푸시알림. 순차 solo면 크로스머신 이득 얇음, 병렬일 때 값.
## rc.1 릴리스 CI 성공 (2026-07-02, 맥에서 태그·수리)

- 동구님 지시로 `v0.1.0-rc.1` 태그. **rc.1이 CI 전용 버그 3개를 순차로 잡음(로컬 미검출)** → 4회차만에 성공:
  1. **버전=태그 불일치**: cargo-dist 태그버전=Cargo.toml버전 요구 → `version="0.1.0-rc.1"`(c20267f).
  2. **`[profile.dist]` 누락** → 추가 + 로컬 `--profile dist` 검증(59e0c74).
  3. **aarch64 ring 크로스컴파일 실패**(`/imsvc`, arm64-win xwin 난제) → **arm64-win/linux 제외, 4타깃**(19f3ce0).
- **최종 성공**: run 28564666085 all-green, **프리릴리스 v0.1.0-rc.1 생성**(15 assets: 4타깃+sha256, sh/ps1 인스톨러, tunaround.rb, source). **homebrew publish=prerelease라 skipped 확정**(tap 불요).
- 교훈은 [[dev-mac-windows §6]]에 영속 기록. **최종 v0.1.0**: Cargo.toml `0.1.0` 되돌림 + `git tag v0.1.0`(동구님, rc 아티팩트 설치검증 + tap/시크릿 후). **주의: gh run watch --exit-status의 exit code 신뢰 불가**(실패해도 0), 잡 결론 직접 확인.

## 2026-07-03 세션8: 크로스머신 양방향 왕복 완료 + A2A 스트리밍(Phase 2) 설계 착수

- **역방향(mac->win) 왕복 성사 = 크로스머신 양방향 다 실증.** 재부팅으로 이전 코어+temp db 소멸(옛 task 907f5c82 유실) -> Windows 코어 재기동(안정 db=LOCALAPPDATA) -> 맥이 새 task 76ea0b9c 재디스패치 -> **win-claude가 raw curl MCP**(등록·세션재시작 없이 initialize->poll->claim->complete)로 처리 -> get_task 자기검증. 교훈: (a) 워커 2세션 온보딩 마찰(#1)은 raw HTTP로 회피 가능(대가=승인 UX 없음), (b) 코어 리셋 시 옛 task_id 조용히 소멸+리셋 통지 없음(마찰 #3 동근). CLAUDE.md 현재상태 세션8 반영(2bb51dd), 맥은 _mac-rc1 ⑦(e073329).
- **"복붙 UX면 A2A 왜?" 통찰(동구님)**: 지금 복붙이 나르는 건 작업이 아니라 **트리거(알림)**다. 작업내용·결과 artifact는 코어로 이미 자동 흐름(get_task). 복붙되는 "이제 poll해"는 코어가 poll_tasks/get_task로 노출하는 걸 사람이 대신 폴링하는 것 = 마찰 #2(사람릴레이)/#3(push부재). 도그푸딩이라 손으로 한 홉씩 밟은 것. 런타임 UX가 복붙이면 안 됨.
- **트리거 자동화 두 단계**: (지금·코어수정0) 백그라운드 poll-watcher가 이벤트 시에만 에이전트 깨움 = 복붙0. (제대로·Phase 2) SSE push. **단, 우리 클라가 에이전트라 push든 poll이든 결국 bg가 깨우는 형태 = UX 동일. SSE 실익=폴링overhead·지연 절감뿐, 우리 스케일(분단위·에이전트)엔 거의 0.** 그래서 "복붙 죽이기"만이면 watcher가 ROI 압승.
- **그런데 ROI가 최상위 가치는 아님(동구님 압박, 수용)**: 이 프로젝트는 A2A가 논지 자체 + privateProject(호기심 1급 사유) + AGPL 공개. **스트리밍의 진짜 값 = interop(외부 스펙 에이전트)·스펙준수·학습·서사.** 우리 UX 이득은 modest. 그래서 스트리밍은 "UX 고치기"가 아니라 "표준 A2A 시민 되기"로 프레이밍.
- **정찰 = 이미 끝나 있었음(동구님 기억 맞음)**: a2a_server.rs가 스펙 §9.1(PascalCase)·§5.3(Method Mapping) 인용, 스트리밍 메서드명 SubscribeToTask 확정, Agent Card에 streaming 플래그 존재(false). **게다가 partner-delegation §65가 "SSE는 후속"으로 명시 유예한 결정이었음** -> 이번은 그 유예를 호기심·interop 근거로 **재개**(잊은 것 아님). 유일 공백=SSE 이벤트 스키마였고 스펙에서 당김(SendStreamingMessage/SubscribeToTask, StreamResponse 래퍼=task|message|statusUpdate|artifactUpdate, TaskStatusUpdateEvent{taskId,contextId,status,final,metadata}/TaskArtifactUpdateEvent{...,append,lastChunk}).
- **설계 crux = 이벤트 버스를 store 계층에 둠**: 모든 task 변이가 SqliteStore 세 메서드(create_task_from_message/update_task_state/complete_task) 통과 -> 거기서 broadcast emit하면 /a2a·MCP 두 경로 자동 커버. broadcast::send는 sync라 rusqlite 동기 경로 OK. 정본=docs/design/v2-a2a-streaming_2026-07-03.md. checklist T1~T6. 미착수(설계 리뷰 대기).
- **스코프 경계**: 워커 방향 push(코어->워커 inbound 알림)는 이번 스코프 아님(브로커 폴링 유지, Phase 1 결정과 동일 근거). 스트리밍=dispatcher-facing 실시간 읽기 + 외부 interop만.

## 2026-07-03 세션8(후반): A2A 스트리밍(SSE) Phase 2 T1~T6 완료 + 라이브 데모

- **전체 완료**(설계 docs/design/v2-a2a-streaming_2026-07-03.md, checklist T1~T6). Sonnet 위임 + Opus 정독리뷰·독립검증, 태스크별 커밋.
- **T1**(785fb25) 이벤트 버스: SqliteStore가 Option<broadcast::Sender<TaskEvent>> 보유, 세 변이(create/update_state/complete) 커밋 후 emit -> /a2a·MCP 두 경로 자동 커버(store 단일 지점). bus=None no-op.
- **T2**(25619c4) 스트리밍 타입: TaskStatus/TaskStatusUpdateEvent(final rename)/TaskArtifactUpdateEvent(lastChunk)/StreamResponse 래퍼 + 순수 task_event_to_frames. TaskState snake_case 재사용(unary 일관).
- **T3**(9ed6380) SendStreamingMessage SSE: subscribe-before-create, task_id 필터, testable string 스트림 분리, main.rs serve store에 with_task_events 배선(MCP claim/complete와 버스 공유). 버스없음=-32004.
- **T4**(ea3e855) SubscribeToTask 재구독: 스냅샷 먼저->terminal이면 최종프레임 종료/아니면 라이브 chain(Box<dyn Stream+Send> 통일), subscribe-후-get_task, 없는id=-32001.
- **T5**(2bc5437) Agent Card streaming:true 플립(두 메서드 동작하니 정직). push_notifications=false 유지.
- **T6 라이브 데모 성공(복붙 0)**: 로컬 코어(with_task_events) + boss가 SendStreamingMessage로 SSE 개방 -> 워커가 MCP(/mcp) claim/complete -> **같은 store 버스 통해** SSE(/a2a)에 task(submitted)->statusUpdate(working,final:false)->artifactUpdate(lastChunk:true)->statusUpdate(completed,final:true) 실시간 도착 후 종료. keep-alive `:` 확인. agent-card `"streaming":true,"pushNotifications":false` 라이브. = 사람 릴레이 없이 boss가 위임 생명주기 실시간 관찰("복붙 왜?"의 코드 답).
- **검증 총계**: 기본 218 / 풀피처(morphology mcp serve) 279 lib pass, clippy 클린(기존 무관 경고 2개만). a2a_server 22 tests.
- **스코프 경계 유지**: 워커 방향 push(코어->워커 inbound 알림)는 미구현(브로커 폴링 유지). 스트리밍=dispatcher-facing 실시간 읽기 + 외부 A2A interop. push_notifications(webhook)·discovery·다중auth는 후속(YAGNI). 우리 자신 UX 이득은 modest, 값=interop/스펙준수/학습.

## 2026-07-03 세션8: 크로스머신 SSE 스트리밍 스모크 성공 + "복붙 잔존" 정직화

- **스모크 성공**: Windows 코어 LAN 호스팅(192.0.2.10:8770, with_task_events, agent-card streaming:true) + **맥=원격 dispatcher**가 SendStreamingMessage를 SSE로 LAN 너머 개방 -> Windows=worker가 poll_tasks(win-claude) 발견->claim->complete -> **맥 SSE에 submitted->(heartbeat)->working(final:false)->artifactUpdate(lastChunk:true)->completed(final:true) 4프레임 실시간 도착 후 정상 종료**(task 53806631, artifact caa49a9e). = SSE-over-LAN 실증. 레시피 docs/prompts/a2a-stream-smoke-mac-dispatcher_2026-07-03.md.
- **동구님 정곡: "아직은 내가 복붙하는게 맞지?" = 맞다, 단 복붙의 내용이 줄었다.** SSE가 제거한 것 = (1) 작업 결과/프레임 relay(맥이 프레임을 붙여넣지 않음, SSE가 나름), (2) dispatcher의 "다 됐나?" 폴링(SSE가 completed를 push = 마찰 #3의 dispatcher-notify 절반 해소). **아직 사람이 나르는 것 = 트리거/조정 신호**("SSE 열었다"->윈도우가 처리 시작, "처리 완료"->맥이 확인). 근데 이 트리거도 제거 가능: 워커가 auto-poll 루프면 "처리 시작" 릴레이 불요, dispatcher는 이미 SSE로 완료를 받으니 "처리 완료" 릴레이는 애초에 redundant였음(SSE가 이미 알림). **결론: 워커 auto-poll + dispatcher SSE = 사람은 목표를 dispatcher에 1회만 말하고, 기계끼리 트리거+데이터+완료통지 자율.** 마찰 #3 = dispatcher-notify(SSE로 해소)/worker-discovery(아직 폴링=워커 auto-poll로 해소) 두 절반.
- 남은 마지막 조각 = **워커 auto-poll 루프**(background poll_tasks -> task 뜨면 claim/처리/complete). 이게 붙으면 사람 트리거 릴레이가 사라진다. 이기종 파트너(Codex-on-Ollama worker)는 그 위에.

## 2026-07-03 세션8: A2A 자율 워커 데몬 설계 착수 (a=워커 auto-poll, b=이기종 파트너 통합)

- 동구님 "a 먼저(중요!) 하고 b". **통찰: (a)=(b)** - 워커 데몬이 어느 Runner/model로 task 실행하냐가 곧 이기종 파트너. 신규 어댑터 불필요, 기존 Runner(claude/codex/opencode/http) 재사용.
- 설계 정본 docs/design/v2-a2a-worker-daemon_2026-07-03.md. `tunaround work` 서브커맨드 = 헤드리스 자율 워커: poll_tasks(agent)->claim->RunInput{prompt=task text}->runner.run()->complete. claim/complete가 코어 버스로 dispatcher SSE에 실시간 흐름(스트리밍과 자동 결합) = 사람 트리거 0.
- 재사용 조각: Runner trait(RunInput/RunOutput), MCP inbox 툴. **W1 관건 = 프로덕션 MCP HTTP 클라이언트**(현재 mcp.rs 테스트 코드에만 있는 handshake+tools/call+SSE파싱을 추출). 태스크 W1~W4(checklist).
- 스코프: opt-in 데몬, read-only 기본(behavioral), dispatcher-side 사람이 목표 발행(semi-a2a HITL 유지). debate AutoLoop(Stage4)와 다름=위임 task 워커 자율.

## 2026-07-03 세션8: A2A 자율 워커 데몬(W1~W4) 완료 - "복붙 제거" 실증

- **W1**(ad5ca38) 프로덕션 MCP HTTP 클라이언트 McpHttpClient(connect handshake + call_tool SSE파싱 + poll/claim/complete 래퍼), worker feature=dep:reqwest async. serve 하네스로 왕복 테스트.
- **W2+W3**(60364d8) parse_open_tasks(견고 블록 파싱, msg 개행 허용, 단위테스트 5) + run_worker_loop(poll->submitted만 claim->spawn_blocking runner.run->complete, --once/interval, task별 에러격리) + Work 서브커맨드/러너 factory(claude/codex/opencode/http).
- **W4 로컬 데모 성공(사람 트리거 0)**: 코어(with_task_events) + dispatcher SendStreamingMessage SSE 개방(win-worker 앞 task submitted) + `tunaround work --once --agent win-worker --runner claude`가 **자율로** poll->발견->claim->**claude 실제 spawn 실행**->complete. dispatcher SSE에 submitted->working->artifactUpdate(**claude 실제 답변** "...다중 소비자 팬아웃 채널입니다.")->completed(final) 실시간(claim 22:56:24->complete 22:56:58 = claude ~34s, 그동안 SSE heartbeat 유지). = 사람이 목표 1회 발행(SSE 개방)만 하고 워커가 발견+실행+완료 자율, dispatcher가 전 과정 실시간 관찰. **"복붙"의 마지막 조각(트리거 릴레이) 제거 실증.**
- 검증: 기본 218 / 풀피처+worker 286 lib pass, clippy 클린.
- **(a)=(b) 확인**: 워커 데몬의 --runner가 파트너 종류. (b) 이기종 = `--runner http`(OpenAiChatRunner, engines)를 Ollama OpenAI-compat(/v1/chat/completions)에 붙이면 로컬LLM 워커 = 다음(W4b). Ollama chat endpoint/model 필요(사용자 원격 Ollama 터널).
- 스코프: opt-in 데몬, read-only 기본, dispatcher-side 사람 목표 발행(semi-a2a HITL). 러너 실패 시 task 'working' 고착(requeue/timeout 후속).

## 2026-07-03 세션8: (b) 이기종 파트너 실증 (Codex 워커) + Ollama-http 코드완성/GPU블록

- **(b) 성공(codex 러너)**: 같은 `tunaround work` 데몬을 `--runner codex`로 띄우니 **Claude 아닌 Codex가 워커**로 자율 처리. dispatcher SendStreamingMessage(to=codex-worker) -> 데몬이 poll->claim(SSE working 도착)->codex exec 실행->complete. GetTask=completed + codex 생성 artifact("A2A 프로토콜의 목적은 서로 다른 AI 에이전트가 표준화된 방식으로...협업하도록..."). **(a)=(b) 실증: 같은 데몬, --runner만 교체 = 파트너 종류 교체.** (codex는 단순 prompt라 #24135 무관 - MCP 툴 승인 불필요.)
- SSE 꼬리 유실: codex가 curl --max-time(150s) 넘겨 completed 프레임은 SSE 대신 GetTask로 확인(codex 느림, ~2분). working까지는 SSE 실시간, 완료는 GetTask. 실전엔 dispatcher가 SSE를 넉넉/무한 유지하거나 재구독(SubscribeToTask)하면 됨.
- **Ollama-http 경로**: `--runner http`(OpenAiChatRunner, engines) **코드 완성**. 로컬 Ollama(11434) chat 모델(qwen3.5:4b/gemma4:e4b) 있으나 **GPU OOM**으로 라이브 실패(qwen3.5:4b가 5.4GB 점유, gemma4=CUDA OOM+CLIP 로드실패). 인프라 이슈, GPU 정리하면 됨. **minor 코드**: main.rs http factory가 `--token`(코어 bearer)을 러너 api_key로 넘김 - Ollama는 무시라 무해하나, 별도 --http-api-key로 분리하는 게 정직(후속).
- 정리: 셸 taskkill이 간헐 행(tasklist 지연) -> PowerShell Stop-Process로 코어 종료 확인.

## 2026-07-03 세션8: Ollama-http 워커 라이브 성공 + reqwest::blocking 버그픽스

- **(b) 3번째 파트너 실증**: `--runner http --http-base-url http://127.0.0.1:11434 --model qwen3.5:4b`로 **로컬 Ollama LLM이 워커**. GetTask=completed + qwen3.5:4b 생성 답변. Claude/Codex/로컬LLM 셋 다 같은 데몬 --runner 교체로 실증(a=b 완결).
- **버그(수정됨, 8c9f6d6)**: http 러너(OpenAiChatRunner)의 reqwest::blocking이 tokio spawn_blocking 스레드에서 "error sending request"로 즉시 실패. 원인 = spawn_blocking 스레드는 Handle::current()가 살아 있어 reqwest::blocking이 "런타임 내 blocking 불가"로 거부. **수정 = 러너를 순수 std::thread + oneshot에서 실행**(런타임 핸들 없음). subprocess 러너(claude/codex)는 원래 무관했음. 교훈: sync 러너를 async에서 돌릴 때 spawn_blocking은 reqwest::blocking과 안 맞음, std::thread 필요.
- **초기 Ollama 실패는 인프라**(GPU 좀비): gemma4 CUDA OOM 크래시가 llama-server를 좀비로 만들어 qwen3.5:4b 요청이 행. `curl /api/generate -d '{"model":..,"keep_alive":0}'`로 언로드 후 재호출하니 정상(33s 콜드로드). 동구님 "가벼운거 하나 호출" 제안이 정확했음.

## 2026-07-03 세션8: A2A interop 스모크 (독립 a2a-client로 외부검증) - 갭 3개 발견

- 독립 크레이트 `a2a-client 0.2` + `a2a-types 0.2`(throwaway)를 우리 코어(/a2a, /.well-known)에 붙여 외부검증. **자기검증(우리 curl)으론 안 보이던 실제 표준 interop 갭 3개 발견**(스모크의 값).
- **(c) GetTask = 완전 호환 ✅**: a2a-client가 보낸 method `GetTask`, params `id`, JSON-RPC envelope 우리 서버와 일치. 없는 id로 `-32001 Task not found`(역직렬화 에러 아닌 정상 앱 에러) = envelope/method 레벨 interop 실증. **method 명명 PascalCase는 a2a-client와 우연히 일치**(단 둘 다 공식 스펙의 slash `message/send`/`tasks/get`과는 다름 - 명명 컨벤션은 여전히 미해결 여지).
- **(a) Agent Card 발견 = 실패 ✗ (2원인)**: (1) 우리 `/.well-known/agent-card.json`이 bearer 게이트(무인증 401). A2A는 카드=신뢰수립 전 공개발견 원칙인데 우리는 /a2a와 같은 auth에 묶임. (2) 스키마 구식: a2a-types는 단일 `url` 아닌 `supported_interfaces: Vec<AgentInterface>`(멀티전송 url+protocol_binding+protocol_version) 요구 + protocolVersion/preferredTransport 부재 -> serde deny_unknown_fields로 `url` 파싱 실패. build_agent_card(a2a_server.rs:187) 구버전 스타일.
- **(b) SendMessage = 구조적 실패 ✗**: 우리 SendParams가 tunaRound 브로커 확장 `fromAgent`/`toAgent`를 **필수**로 요구(a2a_server.rs:87). 표준 a2a-types SendMessageRequter엔 그 개념 자체가 없어 표준 클라가 채울 방법이 없음 -> `-32602 missing field fromAgent`. 우리 중앙-브로커 라우팅의 구조적 대가.
- **정직한 결론**: "표준 A2A 서버" 주장은 **envelope/GetTask 레벨만 참**이고, **Agent Card(공개성+스키마)와 SendMessage(브로커 필드)는 표준 클라와 interop 안 됨**. 우리끼리(tunaRound↔tunaRound)는 되지만 제3자 표준 클라는 못 붙음. 고칠 지점: (1) 카드 무인증 공개 + supported_interfaces 스키마로 재구성, (2) toAgent를 URL 경로/헤더로 옮기거나 optional+default로 - fromAgent는 인증주체에서 유도. README의 "표준 A2A" 문구는 이 한계를 반영하는 게 정직.
