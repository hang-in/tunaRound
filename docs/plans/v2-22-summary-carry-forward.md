# Plan v2-22: 요약 carry-forward (Stage 0 항목2)

> (A) 코어-백엔드 Stage 0의 둘째 슬라이스. docs/design/v2-A2A-core-backend_2026-06-30.md.
> Memora(github.com/microsoft/Memora) 차용: 요약(abstraction)만 이월, 원문(value)은 전사/색인에 남겨 검색으로 확장. ChromaDB/GRPO 등 무거운 스택은 비차용(YAGNI).

## 문제

Plan 16 `--recent-turns N`은 활성 경로 마지막 N턴만 프롬프트에 넣고 **드롭된 옛 턴은 프롬프트에서 사라진다**(RAG 검색만이 복구). 검색은 pull이라 에이전트가 질의해야 하고, 초기 합의/결정의 gist가 매 턴 프롬프트에서 빠진다. carry-forward = 드롭된 턴을 **압축 요약으로 항상 이월**해 연속성 유지(push지만 압축).

## 설계

- `build_round_prompt`에 **예약 슬롯**이 이미 있음(prompt.rs:31, "이미 합의된 사항(전제)"). 여기에 이월 요약 섹션을 꽂는다.
- 섹션 순서: **carried(요약·전제) → retrieved(검색) → prior(최근 N턴) → same_round(이번 라운드) → topic**. 넓은 토대 → 검색 → 최근 → 현재.
- **opt-in, behavior-preserving**: `--recent-turns` None(미캡)이면 드롭 없음 → 요약 빈 문자열 → 섹션 없음 → 프롬프트 불변. 별도 플래그 없이 recent-turns에 종속.
- **요약 생성 = 결정적·무LLM(v1)**: 드롭 턴마다 `- [speaker] {첫 절}` 추출, 총 길이 캡. 추가 비용/지연/의존성 0, 테스트 가능. **LLM 증류(Memora식)·에피소드 분할은 문서화된 후속 업그레이드**(품질 부족 입증 시).

## Task 1: carry-forward 요약 이월 (Sonnet 위임)

### 1a. `Session::carry_forward_digest(&self) -> String` (src/repl/mod.rs)
- `recent_turns`가 `Some(n)`이고 `active_path().len() > n`일 때만 드롭 prefix(`path[..len-n]`)를 요약. 아니면 `""`.
- 형식: 드롭 턴마다 한 줄 `- [{speaker}] {first_clause}`. first_clause = 첫 문장(`.`/`。`/개행 기준) 또는 첫 ~80자 중 짧은 쪽.
- 총 길이 캡(상수, 예 `MAX_CARRY = 1500`). 초과 시 **최근 드롭 턴 우선**으로 예산 내 유지하고 맨 앞에 `(이전 N턴 생략)` 표기.
- 결정적(LLM·임베더 미사용).

### 1b. `build_round_prompt` 시그니처에 `carried: &str` 추가 (src/orchestrator/prompt.rs)
- 비어있지 않으면 예약 슬롯(line 31 위치)에 `이전 논의 요약(이월):\n\n{carried}` 섹션을 **retrieved보다 앞에** push.
- 빈 문자열이면 섹션 없음(behavior-preserving).

### 1c. 배선
- `run_round`(src/orchestrator/mod.rs)에 `carried: &str` 파라미터 추가 → build_round_prompt에 전달.
- repl/mod.rs의 step 5개 호출부(Message/Only/Write/conclude/debate)에서 `let carried = self.carry_forward_digest();` 계산 후 run_round에 전달. (debate 루프는 매 라운드 재계산 - 라운드가 쌓이며 드롭이 늘 수 있음.)
- main.rs는 별도 변경 불필요(--recent-turns 이미 배선됨).

### 테스트
- `carry_forward_digest` = "" when recent_turns None(미캡).
- `carry_forward_digest` = "" when path.len() <= n.
- path.len() > n이면 비어있지 않고 **드롭된 턴의 speaker/gist 포함**.
- 길이 캡 초과 시 `(이전 N턴 생략)` 포함 + 총 길이 ≤ MAX_CARRY.
- prompt 레벨: carried 비면 "이전 논의 요약" 섹션 없음(기존 테스트 무영향), carried 있으면 섹션 + 순서(요약이 검색/prior보다 앞).

### 검증
- cargo는 **Bash 툴**. `cargo test`(기본) + `cargo test --features "sqlite morphology"` 통과, `cargo clippy` 클린. 기존 통과 수 유지(시그니처 추가로 호출부·테스트 갱신 필요).
- 커밋 금지(Opus 리뷰 후).

## Task 2: 리뷰 + 측정 (Opus)
프롬프트가 캡 상태에서 유계인지(요약+최근N+검색이 통째 재주입보다 작은지) 확인. 연속성(초기 결정 gist 유지) 점검. 회귀 가드.

## 비포함(후속)
- LLM 증류 요약(Memora abstraction 고품질화) - 비용 측정 후.
- 에피소드/주제 분할 요약(현재 chronological flat).
- 요약 자체를 색인(abstraction 인덱싱) + 원문 비색인 - Stage 2 push→pull과 합류.
