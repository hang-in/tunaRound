# Plan v2-24: Stage 2 push→pull (crux, 설계)

> (A) 코어-백엔드 Stage 2. docs/design/v2-A2A-core-backend_2026-06-30.md. **이 문서는 설계만**(구현은 사용자 승인 후).
> (A)의 진짜 페이오프를 증명하는 칸: 에이전트가 맥락을 **도구로 당겨오게(pull)** 해서, 통째 재주입(push) 대비 **재전송 토큰 감소를 품질 손실 없이** 실측한다.

## 출발점: push는 이미 상당히 줄었다

현 `build_round_prompt` push 구성: carried(요약) + retrieved(검색 슬라이스) + prior(최근 N턴) + same_round + topic. Plan 16(--recent-turns) + carry-forward + RAG(Plan 11)로 통째 재주입은 이미 축소됨. Stage 2는 **한 걸음 더**: 푸시하던 전사(최근 N턴·검색 슬라이스)를 **얇은 포인터**로 바꾸고, 에이전트가 `read_transcript`/`search_context`로 직접 읽게 한다.

## 설계

### 컨텍스트 모드 (opt-in, behavior-preserving)
- **Push**(기본, 현행): 지금처럼 전사/검색을 프롬프트에 주입.
- **Pull**(신규, `--pull-context`): 전사·검색 슬라이스를 빼고 포인터만. 기본 미설정이면 Push라 불변.

### Pull 모드 프롬프트 구성
- 유지: role 지시 + **carried(요약)** + **same_round** + topic.
  - carried = 항상 푸시(싼 오리엔테이션 + 통제 약화 완충). same_round = **반드시 푸시**(이번 라운드 앞 발언은 아직 전사에 영속 안 됨 → pull 불가).
- 제거: prior(최근 N턴) + retrieved(검색).
- 추가: **포인터 섹션** -
  > "이전 토론 전사는 read_transcript(session_id, max_turns?)로, 관련 과거 맥락은 search_context(query)로 직접 읽을 수 있다. 전사 약 N턴. 답변 전 필요한 만큼 읽어라."

### 게이팅: MCP 가능 좌석만 pull
- pull은 read_transcript/search_context 도구가 있어야 함 → **claude/codex(--mcp-search 배선) 좌석만**. HTTP/opencode 좌석은 도구가 없어 **Push로 폴백**. 즉 모드는 **좌석 능력별**(혼합 로스터 = 혼합 모드).

### 통제 약화 리스크 완화 (Stage 2의 핵심 위험)
- "그 턴에 무엇을 봤나"가 에이전트 주도로 바뀜 → (1) carried 요약을 항상 푸시해 baseline 연속성 유지, (2) 포인터에 **당길 수 있는 범위(전사 N턴)** 명시, (3) 에이전트의 도구 호출은 로그로 관측 가능.

## 측정 (이 Stage의 존재 이유 = 페이오프 증명)
- **계측**: 턴별 프롬프트 char/token 크기 로깅.
- **비교**: 동일 토론을 Push vs Pull로 돌려 평균 프롬프트 크기 + 누적 토큰 비교. 전사가 길어질수록 Pull << Push 기대.
- **품질**: Pull에서 토론이 일관되게 이어지나(에이전트가 당겨와 앞 맥락을 정확히 참조하나) 라이브 관측.

## Tasks (승인 후)
- **Task 1**: 컨텍스트 모드(Push/Pull enum) + build_round_prompt pull 분기(포인터, prior/retrieved 생략) + `--pull-context` + 좌석 능력 게이트(비MCP→push 폴백) + 프롬프트 크기 계측. 테스트: pull 프롬프트에 포인터 있고 prior 없음 / push 불변 / 비MCP 좌석 폴백.
- **Task 2**: 라이브 측정(push vs pull 토큰 크기 + 일관성). claude/codex + read_transcript(Stage1 Task2) 필요. 기록.
- **Task 3(정밀화)**: 포인터 문구·max_turns 힌트·"답변 전 당겨라" 역할 지시 튜닝.

## 리스크
- **게으른 pull(에이전트가 도구를 안 부름) → 품질 하락.** 1순위 위험. 완화: 역할 지시에 "비trivial 턴은 먼저 read_transcript" 명시 + 측정.
- mid-round same_round는 영속 전이라 반드시 push.
- 비MCP 좌석은 pull 불가 → push 폴백(혼합 로스터 = 혼합 모드).
- 통제/관측: 본 슬라이스 선택이 에이전트 주도 → 도구 호출 로깅으로 보완.

## 성공 기준
전사가 길어질 때 Pull 프롬프트 토큰이 Push 대비 유의하게 작고(예: 긴 토론에서 50%+ 감소), 토론 일관성이 Push와 동등. 이 둘이 동시 성립해야 (A)의 push→pull 전환이 정당화된다. 안 되면(게으른 pull 등) Push 유지가 옳다는 데이터.
