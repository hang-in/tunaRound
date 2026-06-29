---
title: 개발 행동 규율 (실험 적용)
type: reference
status: active
canonical: true
priority: high
updated_at: 2026-06-29
owner: d9ng
summary: LLM 코딩 흔한 실수를 줄이는 10개 행동 규율의 SSOT. 이 프로젝트에서만 실험 적용. CLAUDE.md는 이 문서로 링크만 두고 신규 규칙(#5/#6/#7)만 인라인.
---

# 개발 행동 규율 (이 프로젝트 실험 적용)

> 전역 규칙 아님. 이 레포에서만 **실험적으로** 적용한다. trivial 작업엔 판단 재량을 둔다.
> 효과 판정 기준은 diff의 불필요한 변경 감소, 과복잡으로 인한 재작성 감소, 구현 *전에* 질문이 나오는지다.

## COMMON.md와의 관계

10개 중 7개는 전역 `~/.config/agents/COMMON.md`가 이미 always-on으로 강제한다. 그래서 CLAUDE.md엔 중복하지 않고, 이 문서가 전문 SSOT 역할을 한다.

| 규율 | COMMON.md 대응 | CLAUDE.md 인라인 |
|---|---|---|
| #1 Think Before Coding | 핵심 태도 / 질문·기본값 | 아니오(중복) |
| #2 Simplicity First | 범위와 작업 크기 | 아니오(중복) |
| #3 Surgical Changes | Diff 규율 / 편집·안전 | 아니오(중복) |
| #4 Goal-Driven Execution | 작업 방식 / 검증 사다리 | 아니오(중복) |
| #5 No Closing Colons (Korean) | (없음, 신규) | **예** |
| #6 File Header Comments (Korean) | (없음, 신규) | **예** |
| #7 Plan + Checklist + Context Notes | (부분: 체크리스트 요약) | **예** |
| #8 Run Tests Before Complete | 검증 사다리 | 아니오(중복) |
| #9 Semantic Commits | 검증과 commit 분리 | 아니오(중복) |
| #10 Read Errors, Don't Guess | 실패 처리 | 아니오(중복) |

## 10개 규율 (전문)

**1. Think Before Coding.** 가정은 명시한다. 불확실하면 묻는다. 해석이 여럿이면 말없이 고르지 말고 제시한다. 더 단순한 길이 있으면 말하고 정당하면 반박한다. 막히면 멈추고 무엇이 불명확한지 짚고 묻는다.

**2. Simplicity First.** 문제를 푸는 최소 코드. 요청 안 한 기능·단일사용 추상화·요청 안 한 configurability·불가능 시나리오 에러처리 금지. 200줄이 50줄로 가능하면 다시 쓴다. "시니어가 과복잡이라 할까?" 그렇다면 단순화.

**3. Surgical Changes.** 건드릴 것만 건드린다. 인접 코드·주석·포맷 "개선" 금지, 안 깨진 것 리팩토링 금지, 기존 스타일을 따른다. 내 변경이 만든 orphan(미사용 import/변수/함수)만 제거하고 기존 dead code는 언급만 한다. 테스트는 바뀐 모든 줄이 요청에 직결되는가다.

**4. Goal-Driven Execution.** 작업을 검증 가능한 목표로 바꾼다. "검증 추가" -> "잘못된 입력 테스트 작성 후 통과", "버그 수정" -> "재현 테스트 작성 후 통과". 다단계는 짧은 plan(단계 -> 검증)을 명시한다. 강한 성공기준이 독립 루프를 가능케 한다.

**5. No Closing Colons (Korean).** 한국어 문장은 마침표로 끝낸다. 다음 줄이 리스트/예시여도 `:`로 끝내지 않는다. 콜론은 코드·key-value·라벨 안에서만 허용한다. 표기규칙(em-dash 대체)과 정합하면, 콜론은 문장 중간·라벨에만 쓰고 문장 끝은 `.`/`?`/`!` 로 둔다.

**6. File Header Comments in Korean.** 새 소스 파일 첫 줄은 역할을 적은 한국어 한 줄 주석이다. Rust 예: `// 토론 라운드 프롬프트를 조립하는 순수 함수`. 필수 지시문 바로 아래에 둔다. config 파일(`*.toml` 등)은 생략한다. 이유는 에이전트가 파일을 선택적으로 읽으므로 즉시 맥락을 주기 위함이다.

**7. Plan + Checklist + Context Notes.** 비trivial 작업 전 세 산출물을 만든다. Plan(무엇을 왜) / `checklist.md`(체크박스 task, 진행하며 체크) / `context-notes.md`(작업 중 결정·근거, 계속 append). plan만 주고 코딩 시작을 요청하면 멈추고 "checklist·context notes 먼저 만들까요?"라고 묻는다.

**8. Run Tests Before Marking Complete.** 코드를 건드렸으면 "완료" 전에 `cargo test`를 돌린다. 통과면 결과 보고, 실패면 고치고 재실행한다. 테스트가 없으면 최소 빌드/컴파일을 확인한다. 사용자가 "끝/완료" 신호를 보내기 *전에* 선제 실행한다.

**9. Semantic Commits.** 한 논리 변경이 끝나면 커밋한다(사용자 요청을 기다리지 않음). 테스트는 "한 문장으로 설명되나?"이고 아니면 분리한다. 좋음은 "auth 미들웨어 추가", 나쁨은 "auth 추가하고 UI도 고치고". solo 프로토타입은 느슨히 묶어도 되며 요점은 reversibility다.

**10. Read Errors, Don't Guess.** 실제 에러/로그 줄을 읽는다. 기억에서 패턴매칭하지 않는다. 풀 에러·스택트레이스를 확인하고, 원인 확인 전 "흔한 픽스"를 적용하지 않는다. 불명확하면 로그를 추가해 상태를 확인한 뒤 고친다.

## 위임 라우팅 (이 프로젝트)

- **구현(정확성 민감)** -> Sonnet 서브에이전트. tunaRound 코어 로직(러너·오케스트레이터)이 여기 해당.
- **벌크/초안(저비용)** -> tunaLlama(`tuna_generate_code`/`tuna_refactor_code`). 보일러플레이트·반복 코드·문서 초안·테스트 스캐폴드 1차 생성 후 Opus가 리뷰.
- **스펙·리뷰·검증** -> Opus(메인).
- `.tuna-docs/routing.json`으로 override 가능(`/tunaDocs:init` 시 생성).
