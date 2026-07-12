<!-- v2-52 ④ task wire 프로토콜(문자열→JSON) 리팩토링의 착수 전 고정 계약(스코프·마이그레이션·하위호환). -->

# v2-52 ④ Task wire 프로토콜 구조화 (문자열 → JSON): 착수 전 계약 (2026-07-12)

> 정본 배경 = [v2-52 리팩토링 백로그 §2 "Task wire format을 구조화"](v2-52-refactoring-backlog_2026-07-12.md). 라이브 mesh 동작 변경이라 하위호환 다단계 마이그레이션으로 진행합니다.

## 1. 스코프 (실측)

- **파싱되는 문자열 프로토콜은 poll_tasks 응답 하나뿐입니다.** 생산=`mcp/format.rs::format_open_tasks`(`[{id}] from=... state=...{annotation} ctx=... msg={msg}` 블록을 `\n\n`로 조인), 소비=`worker.rs::parse_open_tasks`(문자열 surgery로 `ParsedTask{id,state,context_id,msg}` 복원).
- **다른 응답은 워커가 파싱하지 않습니다**: claim/complete/fail/extend/cancel은 워커가 `mcp_client`의 isError만 보고(본문 미파싱), get_task/tasks/format_agents는 사람/운영자용(워커 비소비). 따라서 ④의 대상은 `format_open_tasks ↔ parse_open_tasks` 한 쌍입니다.
- **취약성 근거**: 문자열 파서가 실측 패닉(mac-claude-sup poll이 본문 `\n\n[한글]` 슬라이스 중 exit 101, worker.rs `parse_open_tasks_korean_bracket_after_blank_line_does_not_panic` 회귀 테스트).

## 2. 라이브 mesh 하위호환 (핵심 제약)

브로커와 워커가 **독립적으로 업데이트**되므로 롤아웃 중 4조합이 모두 동작해야 합니다:

| broker | worker | 요구 |
|--------|--------|------|
| 구(문자열) | 구(문자열) | 현행 |
| 구(문자열) | 신(JSON우선) | 신 워커가 문자열로 폴백 |
| 신(JSON+문자열) | 구(문자열) | 구 워커가 JSON 프리픽스를 무시하고 문자열 파싱 |
| 신 | 신 | 신 워커가 JSON 파싱(견고) |

## 3. 설계: JSON 프리픽스 라인 + 문자열 블록 병존

`format_open_tasks` 출력 = **`TASKS_JSON <compact-json>` 한 줄 + `\n\n` + 기존 human 블록**.
- JSON = `[{"id","state"(clean),"context_id":Option,"msg"}]`(워커가 필요로 하는 `ParsedTask` 필드와 1:1). state는 **주석 없는 clean state**(`t.state.as_str()`).
- **구 워커 안전 근거**: `parse_open_tasks`의 `find_header_starts`는 `[<32hex>] from=` 헤더만 찾고 **첫 헤더 앞 내용은 어떤 블록에도 포함하지 않습니다**. JSON 프리픽스는 첫 human 헤더 **앞**에 오고, compact JSON은 실제 개행 없이(msg 내 `\n\n`도 `\\n\\n`로 이스케이프) 한 줄이라 `find_header_starts`가 그 안에서 거짓 헤더를 못 찾습니다 → 구 워커가 프리픽스를 무시하고 human 블록만 파싱.
- **신 워커**: 첫 줄이 `TASKS_JSON ` 프리픽스면 JSON 디코드(견고), 아니면 기존 문자열 파싱으로 폴백.
- **공유 DTO**: `crate::a2a_wire`(신규 무-게이트 crate 루트 모듈, serde만 의존 = mcp·worker·경량 워커 빌드 모두 접근. `store::a2a`는 sqlite-gated라 worker 단독 빌드가 못 써서 여기로 분리)에 `PollTaskDto` + `POLL_JSON_PREFIX` + `encode_poll_json`/`decode_poll_json` 정의(생산·소비 단일 소스).
- **빈 목록**: 기존 `"{agent} 앞 열린 task 없음"` 유지(프리픽스 없음). 신 워커는 프리픽스 없으면 문자열 폴백 → `contains("앞 열린 task 없음")` → 빈 Vec. 구 워커도 동일.

## 4. 이 세션 범위 = 계약 ①②③ (Stage 1)

- ① JSON 응답 추가(format_open_tasks 프리픽스) + ② worker 구조화 우선(parse_open_tasks JSON 먼저) + ③ 문자열 형식 하위호환 유지(human 블록 계속 emit)를 **한 번에** 달성합니다. 셋이 병존 설계라 분리 커밋이 무의미합니다.
- **④ 문자열 parser 제거는 defer**(post-rollout): human 블록 제거 + `parse_open_tasks` 문자열 경로 삭제는 mesh 전체가 신 바이너리로 롤아웃되고 도그푸딩으로 확인된 뒤에만 안전합니다. 이 세션은 하위호환 병존까지.

## 5. 고정 계약 (공개 API·테스트)

- **신규 공개**: `a2a_wire::PollTaskDto{id,state,context_id:Option<String>,msg}`(serde), `POLL_JSON_PREFIX`, `encode_poll_json(&[PollTaskDto])->Option<String>`(직렬화 실패 시 None→프리픽스 생략), `decode_poll_json(&str)->Option<Vec<PollTaskDto>>`. **context_id "-"는 DTO에서 None으로 정규화**(문자열 경로 패리티).
- **불변**: `format_open_tasks` 시그니처(추가 출력만), `parse_open_tasks` 시그니처(`&str -> Vec<ParsedTask>`), `ParsedTask`, poll_tasks MCP 툴 계약. 기존 문자열 파싱 테스트 전부 green 유지(폴백 경로).
- **테스트**: encode/decode 라운드트립 / parse_open_tasks가 JSON 프리픽스 우선 / 프리픽스 없으면 문자열 폴백(기존 테스트) / 구 워커가 JSON 프리픽스를 무시하고 human 블록 파싱(프리픽스+블록 혼합 입력) / msg 내 `\n\n`·한글·브래킷이 JSON 경로에서 무손실 / 빈 목록.

## 6. 위험

- **프리픽스 오염**: JSON이 반드시 compact 단일 라인(실개행 없음)이어야 구 워커의 `find_header_starts`가 그 안에서 거짓 헤더를 안 만든다. `serde_json::to_string`(pretty 아님)이 개행을 이스케이프하므로 보장됨. 테스트로 잠금.
- **state 오염**: JSON state는 clean(`as_str()`)이라 주석 스트립 불요. human 블록의 `state={}{annotation}`은 그대로(구 워커가 `state_token`으로 스트립).
- **사람 가독성**: poll 출력 최상단에 JSON 한 줄이 붙어 다소 노이즈. poll_tasks 직독은 드물고(사람은 대시보드), 주 소비자는 워커라 수용. (④ stage에서 human 블록 제거 시 자연 해소.)
