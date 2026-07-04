# 세션11 핸드오프 (2026-07-04): 에이전트 레지스트리 구현·머지 + 다음 방향

> WIN(윈도우) 핸드오프. 세션11 = 에이전트 레지스트리(UUID 라우팅 + 태그 발견) 구현·머지(PR #5). 이전: 세션10(거버넌스·v0.2.2), [session9](v2-handoff_2026-07-03_session9.md).

## 이번 세션에 한 것

**에이전트 레지스트리(UUID+태그) Plan v2-34 T1-T5 전부 구현·검증·머지(PR #5 = `2bbf3d3` squash).**

어드레싱을 "사람이 손으로 맞추는 자유 문자열(`mac-claude`)"에서 **UUID(라우팅) + 태그(발견)**로 옮겼다. dispatcher가 `to_selector="runner=claude"`로 워커를 발견해 던지면 코어가 발송 시점에 concrete uuid로 해석한다.

- **T1** 인메모리 로스터 데이터레이어: `src/store/agents.rs`(AgentEntry, `parse_tags`/`selector_matches`(부분집합)/`is_online`(TTL 90s)). 로스터=`SqliteStore.agent_roster`(RefCell<HashMap>, 인메모리, `/a2a`·MCP 양 라우팅 경로가 같은 Arc 공유해 배선 0). register/heartbeat/list_agents/resolve_selector 메서드.
- **T2** 레지스트리 MCP 도구(register_agent/heartbeat/list_agents) + `send_task`에 `to_selector`. 다중매칭=후보 반환(사람선택). McpHttpClient 대칭 래퍼.
- **T3** `/a2a` SendMessage `toSelector`. 공유 헬퍼(validate_send_target/SendTarget/format_ambiguous_candidates)를 mcp.rs→`store/agents.rs`로 이동(serve↔mcp 피처 커플링 회피).
- **T4** 워커 `--tags`/`--agent` 자가 uuid(`generate_agent_uuid`) + 자동 register + 매 패스 heartbeat(코어 재기동 시 `needs_reregister`로 재등록).
- **T5** `a2a-usage.md` §9 등록·발견·셀렉터 레시피 + §0 태그 재프레이밍 + 라이브 스모크.

**품질 게이트**: 풀피처(`morphology mcp serve worker`) 414 pass / 0 fail, clippy 3조합 클린, 3-OS CI green(2회), CodeRabbit 4건(R1 에러계약: register/heartbeat/list_agents 실패를 isError로) 반영. **라이브 스모크 4/4**(단일매칭 라우팅 / 무매칭 no-consumer / 다중매칭 후보반환 / 부분집합 셀렉터). 하위호환 레거시 `to_agent` 문자열 exact-match 전 구간 불변.

**비범위(후속)**: agents 테이블 영속 · 브로커 자동배정(best-fit) · `/a2a` RegisterAgent JSON-RPC · 인증/권한 · 멀티브로커 gossip · node 레인 태그(T4에서 `None`으로 미룸).

## 다음 세션 첫 행동

1. `git pull --rebase origin main` + `cargo test --features "morphology mcp serve worker"`(베이스라인 414)로 상태 확인(cargo는 Bash 툴로).
2. **다음 방향 = D → B → C 순서**(사용자 확정). 각 세션 끝에 가벼운 close(CLAUDE.md 한 줄 + 핸드오프 갱신).

### D. doctor Stage 4 (온보딩 프리플라이트)
- claude/codex·Ollama·Kiwi·포트·코어 도달 프리플라이트. 정본 [배포·온보딩](../design/v2-deploy-onboarding_2026-07-02.md) Stage 4.

### B. 관측성·안전 보강 (agentgateway 권고 P1)
- **tasks flat trace 컬럼**(스키마 v8, additive): runner/session_id/started_at/completed_at. 현재 updated_at 하나로 claim↔complete 뭉갬. `create_task`/claim/complete 배선 + get_task/tasks 출력 노출.
- **쓰기 민감 path denylist**: `.env`/`secrets/**` 고정 denylist(규칙 엔진 아님). 러너 spawn 직전 순수 가드(worker.rs, `write_lane_disrupts_node`가 사는 곳).
- (문서) capability 예약 태그키 관례 명시 + README gateway 경계 한 문단.

### C. config 정리 (agentgateway 권고 v1 후)
- roster/SeatConfig에 `tags` 필드(config→런타임 태그 seed, node 레인 태그 배선 포함) → registry 3중화 방지.
- backend를 named seat로 tunaround.toml 프로파일에 은닉(`[backend.*]` 신설 금지).

### 채택 안 함 (경계 확정, agentgateway 검토)
- 정책 규칙 엔진(behavioral read-only 유지) · 별도 backend registry(runner=backend) · artifact lineage DAG(YAGNI) · 범용 gateway 전 영역. 정본 [agentgateway 선별 도입 검토](../design/v2-agentgateway-selective-adoption_2026-07-04.md).

## 규율 리마인더
- 구현=Sonnet 서브 + Opus 리뷰·독립검증. 태스크별 커밋 분리. GitHub Flow(PR + 3-OS CI) + CodeRabbit 리뷰 반영 후 머지. 커밋/검증/push 분리, push 전 `git pull --rebase origin main`.
- cargo는 Bash 툴, `CARGO_INCREMENTAL=0 cargo test -j 4`. 레포 PUBLIC이니 문서/코드에 LAN IP·토큰·사설호스트 평문 금지.
