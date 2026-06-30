# Plan v2-25: Stage 3 상주 코어 + 네트워크 도달 (설계)

> (A) 코어-백엔드 Stage 3. docs/design/v2-A2A-core-backend_2026-06-30.md. **설계만**(구현 승인 후).
> half-a2a 완성 칸: 코어를 **네트워크 도달 가능한 상주 서비스**로 만들어 원격 프론트/에이전트가 접속. 단일 테넌트 자체호스팅(SaaS 아님).

## 실측 확정 가정 (규율 #10, 2026-06-30)

- **claude**: `claude mcp add --transport http <url>` / `--transport sse|http|stdio` + OAuth client-id·headers. 원격 HTTP MCP 접속 OK.
- **codex**: `codex mcp add --url <URL>`(streamable HTTP) + bearer token(env). 원격 HTTP MCP 접속 OK.
- **rmcp**: 현재 `transport-io`(stdio)만. streamable-http 서버 transport는 feature 추가로 가능(1.x 지원).
- 결론: "코어를 HTTP MCP 서비스로 상주 + 에이전트가 원격 접속 + bearer 인증"이 코드로 가능.

## 출발점: 지금 무엇이 이미 되나

Stage 0~2로 half-a2a 대부분이 섰다. **이미 됨**: 멀티프론트(Redis 미러, Plan 06) · 에이전트 pull(Stage 2) · 세션 스코핑(Stage 1 T2). **안 됨(=Stage 3 대상)**: 코어가 **네트워크 서비스가 아님**(MCP=stdio 로컬 spawn, Redis=로컬) → 원격 프론트/에이전트 접속 불가. 인증 없음. 에이전트 stateless.

## 핵심 전환 (린치핀)

현재: **에이전트마다 self-exe `--mcp-search` stdio MCP 서버를 로컬 spawn**(각자 로컬 DB 읽음). 
Stage 3: **코어가 HTTP MCP 서버 하나를 상주**시키고, 모든 에이전트(로컬·원격)가 그 URL에 접속. → DB 하나, 코어 하나 = "코어 상주 백엔드" 실현.

## 분해 (3a 린치핀 우선, 점진)

| 단계 | 내용 | 가치/위험 |
|---|---|---|
| **3a (린치핀)** | 코어가 MCP-over-HTTP 상주(rmcp streamable-http). 러너 MCP config를 stdio-spawn → URL 접속으로. opt-in `--serve-mcp <addr>`. | 코어 상주 실현. 위험=transport 신규 |
| **3b** | bearer 토큰 인증. claude headers / codex bearer env. | 원격 안전. 단일 테넌트라 토큰 1개로 충분 |
| **3c** | 원격 연결 = Tailscale 오버레이(코드 무관, 코어 URL=tailnet 주소). | NAT 통과·공개포트 0. ops |
| **3d** | post_turn(쓰기 도구) + get_roster. 원격 프론트/에이전트가 코어에 턴을 씀. | 분산 쓰기 권위. post_turn은 여기서 소비자 생김 |
| **3e (선택)** | 영속 에이전트(`codex app-server`/`mcp-server`, claude 영속). stateless spawn 대체. | 에이전트 세션 연속·spawn↓. 가장 큰 변경, 입증된 필요 약함(Stage 2가 비용 대부분 해소) → 보류 |

## 3a 상세 (린치핀 = 첫 구현 후보)

- **코어 모드**: `tunaround --serve-mcp <listen-addr> --db core.db [--token <tok>]`. rmcp streamable-http 서버로 search_context·read_transcript(+후속 post_turn) 노출. 기존 `TunaSearchServer` 핸들러 재사용, transport만 stdio→http.
- **러너 배선 전환**: `with_search_db`(로컬 spawn config) 외에 `with_search_url(url, token)` 추가. Some이면 MCP config가 stdio-spawn 대신 `{"type":"http","url":...,"headers":{"Authorization":"Bearer ..."}}`(claude) / codex `--url`+bearer. db-spawn과 택일.
- **누가 코어를 띄우나**: 최소안 = **프론트 프로세스가 곧 코어**(REPL + HTTP MCP 동시), 원격 에이전트/프론트가 그 HTTP에 접속. 완전 분리(데몬+thin front)는 후속. 단일 writer(코어 SQLite)가 분산 쓰기 직렬화.
- **테스트**: HTTP MCP 서버 기동 + JSON-RPC initialize/tools/call로 search_context·read_transcript 왕복(stdio 통합테스트 답습). 인증 거부(토큰 불일치) 테스트.

## 열린 결정 (착수 전)

- **3a 누가 코어인가**: 프론트=코어(최소) vs 분리 데몬(정석, 큼). 추천=프론트=코어 먼저.
- **전송**: streamable-http(권장, 양 CLI 지원) vs SSE. http 우선.
- **상태 공유 경로**: 원격 프론트의 전사 공유 = Redis-over-Tailscale(기존) vs MCP read_transcript/post_turn. 둘 공존 가능.
- **turn 구동**: HumanDriven 유지. 원격 프론트가 자기 좌석 구동, 코어가 전사 직렬화. (자율은 (B), 범위 밖.)
- **3e 영속 에이전트**: 입증된 필요 생길 때만(Stage 2가 재주입 비용 이미 해소). 지금 보류.

## 권고 순서

**3a(HTTP MCP 상주) → 3b(토큰) → 3c(Tailscale, ops) → 3d(post_turn/get_roster) → 3e(보류).** 3a가 모든 원격의 토대라 먼저. 3e(영속 에이전트)는 가치 입증 후.

## 성공 기준 (half-a2a 완성)

다른 머신(Tailscale)의 에이전트/프론트가 **코어 HTTP MCP에 인증 접속**해 전사를 read_transcript로 읽고(원하면 post_turn으로 쓰고) 토론에 참여. 사람이 여전히 운전. 이게 되면 half-a2a 완성.
