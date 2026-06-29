---
title: "tunaRound v2 설계: 에이전트 쓰기 지목 (협업 코딩)"
type: design
status: needs-decision
priority: P1
updated_at: 2026-06-29
owner: shared
summary: v1은 모든 턴 읽기 전용. 이 설계는 사람이 특정 자리에 "쓰기 턴"을 지목해 에이전트가 실제로 레포를 편집하게 한다(토론 도구 -> 협업 코딩 도약). 안전 민감(에이전트가 레포에 직접 쓰기)이라 착수 전 사용자 결정 3건 필요. 자율 진행에서 제외하고 설계만 준비.
---

# tunaRound v2 설계: 에이전트 쓰기 지목 (협업 코딩)

> **상태: 착수 전 사용자 결정 필요.** 이 문서는 접근과 결정 지점을 정리한 설계안이다. 결정이 끝나면 v2 Plan으로 분해해 구현한다.

## 왜 자율 진행에서 뺐나

에이전트가 레포에 **직접 파일을 쓰는** 기능이라 안전 민감(데이터 변경)이다. 특히 claude 쓰기 모드의 현재 인자가 `--dangerously-skip-permissions`(모든 권한 우회)라, 자율로 켜는 건 위험하다. COMMON.md "데이터 손실·보안 가능성 있으면 먼저 확인" 규칙에 해당한다.

## 현재 상태 (실측)

- `RunMode{ReadOnly, Write}` 타입은 v1부터 존재. 쓰기 하드 분리도 러너 인자에 구현됨:
  - claude Write: `--dangerously-skip-permissions` (= bypassPermissions, 모든 권한 우회).
  - codex Write: `--sandbox workspace-write` (워크스페이스 한정 샌드박스).
- 그러나 `run_round`은 **모든 턴을 `RunMode::ReadOnly`로 하드코딩**(orchestrator/mod.rs L84~88). REPL에 쓰기 턴 지목 경로 없음.
- `RunInput.project_path`는 run_round에서 `None`(= 현재 작업 디렉토리에서 실행).

즉 "쓰기 인프라는 있으나 행사 경로가 없다." 이 작업은 그 경로를 연다.

## 제안 접근

### 명령 경로
기존 `@engine <msg>`(읽기 지목)와 평행하게 **쓰기 지목 명령**을 추가한다. run_round에 mode 인자를 받게 확장(현재 ReadOnly 고정 -> 호출자가 지정). 후보 문법:
- `@engine! <msg>` : bang = 쓰기 턴. 간결하고 기존 `@engine`과 대칭. (추천)
- `/write @engine <msg>` : 명시적이지만 장황.

### 코드 변경 (개략)
- `orchestrator::run_round`에 `mode: RunMode` 파라미터 추가(기존 호출부는 `RunMode::ReadOnly` 전달 = 무변경 동작). RunInput.mode를 그 값으로.
- `repl::Command`에 `Write { engine, text }` variant + `parse_command`에 `@engine!` 분기.
- `Session::step`에 Write 분기: 해당 자리만 `run_round(..., RunMode::Write)`. 실행 후 working tree 변경을 사용자에게 알림(예: `git status --short` 요약을 출력에 첨부).
- 쓰기 턴은 1자리만(여러 자리 동시 쓰기 = 충돌 위험, 비포함).

### 전사 기록
쓰기 턴의 에이전트 출력(무엇을 왜 바꿨는지)을 전사에 기록. 실제 파일 변경은 git working tree에 남으므로, 선택적으로 `git diff --stat` 요약을 발언에 덧붙여 추적성↑.

## 착수 전 결정 필요 (사용자)

1. **claude 쓰기 권한 수위.** 현재 `--dangerously-skip-permissions`(전권 우회, bash 포함). 더 안전한 대안은 `--permission-mode acceptEdits`(파일 편집만 자동 승인, 임의 bash는 차단). 협업 코딩 편의 vs 안전의 트레이드오프.
   - 옵션 A: `acceptEdits`로 낮춤(추천, 안전). 옵션 B: 현행 `--dangerously-skip-permissions` 유지(편의, 위험). codex는 `workspace-write` 샌드박스라 상대적으로 안전.
2. **쓰기 대상 디렉토리.** 현재 cwd(= tunaRound 레포 자신 또는 실행 위치)에 쓴다. 의도대로인가, 아니면 `--project <path>`로 대상 레포를 따로 지정할 것인가. v1 설계는 "같은 레포 위에서"였다.
3. **실행 전 확인 프롬프트.** 사람이 명시적으로 쓰기 명령을 친 것이지만, 실행 직전 1회 확인(`정말 쓰기 턴? [y/N]`)을 넣을지. 비대화형 스모크와의 호환도 고려.

## 비포함 (후속)

- 여러 자리 동시 쓰기(충돌·머지), 자동 커밋, 쓰기 결과 자동 리뷰 라운드, 변경 롤백 명령.

## 다음

위 결정 3건이 정해지면 v2 Plan(예: `v2-03-write-delegation.md`)으로 분해 -> TDD 구현(파싱·run_round mode 파라미터·step 분기) -> 실 에이전트 쓰기 스모크(샌드박스 확인). 결정 전까지 보류.
