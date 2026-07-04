# Plan v2-34: 에이전트 레지스트리 (UUID + 태그)

> 설계 정본 [v2-agent-registry-uuid-tags_2026-07-04.md](../design/v2-agent-registry-uuid-tags_2026-07-04.md) §5 구현. 세션11(2026-07-04) 착수.
> 어드레싱을 "사람이 맞추는 자유 문자열"에서 "UUID(라우팅) + 태그(발견)"로 옮긴다. 하위호환(레거시 문자열 exact-match)은 전환기 유지.

## 코드 정찰 결론 (구현 전 확정 사실)

- `Arc<Mutex<SqliteStore>>`가 `/a2a`(SendMessage → `a2a_server::handle_send`)와 MCP 도구(`send_task`, inbox/dispatcher)에 **동일 인스턴스로 공유**된다(main.rs:1755 → build_router + with_a2a_store). 따라서 **로스터를 SqliteStore 인메모리 필드로 두면 양 경로가 자동으로 같은 로스터를 본다**(별도 배선 0).
- 로스터 = 인메모리 `HashMap`(설계 §5.1 얇은 시작). `event_bus: Option<broadcast::Sender>` 필드와 동형 패턴. 영속(agents 테이블)은 비범위(§6). 브로커 재기동 시 워커 재등록으로 복원.
- 라우팅 진입점: `create_task_from_message(from, to, msg)`가 유일한 task 생성 지점(handle_send·send_task_text 둘 다 위임). 셀렉터 해석은 이 호출 **직전**에 concrete uuid로 바꾸면 태스크는 항상 구체 to_agent로 남는다(설계 §5.3).
- 다중 매칭 정책 = **(a) dispatcher에게 후보 반환 후 사람이 선택**(기본, HITL·semi-a2a 부합). 브로커 자동배정(b)은 비범위.

## 설계 결정

1. **로스터 위치 = SqliteStore 인메모리 필드** `roster: HashMap<String, AgentEntry>`. register/heartbeat/list_agents/resolve는 conn을 건드리지 않는 순수 인메모리 메서드. Mutex는 기존 것 재사용(동시성 무료).
2. **online 판정 = heartbeat TTL**. `now - last_heartbeat <= AGENT_TTL_SECS`(기본 90초). age 계산은 기존 `a2a::age_secs`(SQL datetime 파서) 재사용.
3. **태그 = KV 맵**(`BTreeMap<String,String>`, 결정적 순회). 셀렉터 매칭 = 부분집합(셀렉터의 모든 k=v가 엔트리 태그에 존재). 표준키(machine/runner/role/project/mode)는 관례일 뿐 강제 없음.
4. **UUID = 워커 자가 발급**. `--agent`에 uuid를 주거나 미지정 시 `store.new_task_id()`와 같은 randomblob(16) hex로 자가 생성. 등록 라운드트립 불필요, 브로커는 register로 기록만.
5. **하위호환**: `to_agent`가 여전히 자유 문자열이면 exact-match 그대로 라우팅(레지스트리 우회). 셀렉터(`to_selector`)는 신규 경로로만 추가. 레거시 워커는 무변경.
6. **registration 표면 = MCP 도구 우선**. 워커는 이미 McpHttpClient(W1)로 poll/claim/complete를 MCP HTTP로 호출하므로 register/heartbeat/list_agents도 MCP 도구가 자연스러운 자리. `/a2a` JSON-RPC 신규 메서드(RegisterAgent 등)는 순정 A2A에 없어 비범위(후속). 단 **셀렉터 라우팅은 `/a2a` SendMessage(toSelector)에도 추가**(공유 resolve 재사용, 저비용).

## 태스크

### T1: 로스터 데이터 모델 + 인메모리 스토어 + 순수 함수 (신규 `src/store/agents.rs`)
- `AgentEntry { uuid, tags: BTreeMap<String,String>, display_name: Option<String>, last_heartbeat: String }`.
- 순수 함수: `parse_tags(&str) -> Result<BTreeMap, String>`("k=v,k=v", 빈/중복/형식오류 처리) · `selector_matches(tags, selector) -> bool`(부분집합) · `is_online(last_heartbeat, now, ttl) -> bool`(age_secs 기반).
- SqliteStore 필드 `roster: HashMap<String, AgentEntry>` + 메서드: `register_agent(uuid, tags, display, now)` · `heartbeat_agent(uuid, now) -> bool`(unknown=false) · `list_agents(selector, now, ttl) -> Vec<AgentEntry>`(online+매칭, uuid 정렬) · `resolve_selector(selector, now, ttl) -> Vec<String>`(매칭 online uuid, 정렬).
- 단위테스트: parse_tags(정상/빈값/등호없음), selector 부분집합(매칭/불매칭/빈셀렉터=전부), online TTL 경계, register→list 라운드트립, heartbeat 갱신/unknown, resolve 0/1/다중.
- 검증: 기본 + 풀피처 pass, clippy 클린. 신규 의존 0.

### T2: MCP 도구(register_agent/heartbeat/list_agents) + send_task to_selector
- 신규 MCP 도구 3개(McpHttpClient 래퍼도 대칭 추가): `register_agent(uuid, tags?, display_name?)` · `heartbeat(uuid)` · `list_agents(selector?)`(사람이 읽는 텍스트: uuid·태그·online).
- `send_task`에 `to_selector: Option<String>`(k=v,k=v) 추가. 셀렉터 있으면: resolve → 0개=no-consumer 안내(생성 안 함), 1개=그 uuid로 create, 2개+=후보 목록 반환(생성 안 함, dispatcher가 골라 to_agent로 재호출). `to_agent`/`to_selector` 상호배타(둘 다면 에러 안내).
- 순수 `*_text` 함수 분리(SQLite 없이 테스트) + HTTP e2e(register→list_agents online→send_task selector 라우팅→get_task).
- 검증: mcp 피처 pass, clippy 클린.

### T3: `/a2a` SendMessage toSelector 지원 (공유 resolve 재사용)
- `SendParams`에 `to_selector: Option<String>`(camelCase `toSelector`) 추가, `to_agent`를 `Option`화(하위호환: 없으면 셀렉터 필수). `handle_send`가 셀렉터를 resolve해 concrete to_agent로 create. 다중=Err(후보 나열, dispatch가 -32602/Internal로 응답). 0개=no-consumer Err.
- 단위테스트(in-memory store): selector 단일 라우팅, 다중 거부, 레거시 to_agent 불변.
- 검증: serve 피처 pass, clippy 클린.

### T4: 워커 CLI --agent/--tags + 자가 uuid + 자동 register/heartbeat
- `work` 서브커맨드(WorkArgs)에 `--tags k=v,k=v` 추가. `--agent` 미지정 시 자가 uuid 생성(현재는 필수였는지 확인 후). 워커 루프 시작 시 1회 register(uuid+tags), 매 poll 직전 heartbeat. McpHttpClient 사용.
- node 레인(config)도 `tags` 필드 수용(있으면 배선, 없으면 무변경) — 단 범위 크면 워커 CLI만 하고 node는 후속으로 분리.
- 검증: worker 피처 pass, clippy 클린. `work --help`에 --tags 노출.

### T5: 문서 + 하위호환 확인 + 라이브 스모크
- `docs/reference/a2a-usage.md`에 등록·발견·셀렉터 라우팅 레시피 추가. 네이밍 규약(거버넌스 #1)을 "태그 관례"로 재프레이밍(machine/runner/role 태그).
- 하위호환: 레거시 문자열 to_agent 경로가 기존 테스트로 불변임을 확인.
- 라이브 스모크: 워커 자동 register → dispatcher `list_agents`로 발견 → `send_task --to-selector runner=claude` → 매칭 uuid로 라우팅 → 워커 claim/complete → get_task=completed. (단일 머신 로컬로 충분, 크로스머신은 후속.)

## 실행 규율
- 구현 = Sonnet 서브에이전트, Opus(나) 스펙·리뷰·독립검증. 태스크별 커밋 분리.
- cargo = Bash 툴, `CARGO_INCREMENTAL=0 cargo test -j 4`. 검증 = 기본 + `--features "morphology mcp serve worker"`(베이스라인 377).
- clippy 3조합(기본/풀피처/no-default) 0경고. push는 rebase 위생(pull --rebase origin main) 후, 사용자 확인.

## 비범위(이 플랜)
- agents 테이블 영속(§6) · 브로커 자동배정(b, best-fit) · `/a2a` RegisterAgent JSON-RPC 메서드 · 인증/권한(누가 어떤 태그로 등록) · 멀티브로커 gossip.
