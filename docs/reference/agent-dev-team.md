# 유스케이스: 에이전트 개발팀 (GitHub Flow + A2A 큐 + PR CI + 헤드리스 워커)

> tunaRound의 semi-a2a를 "여러 에이전트가 한 코드베이스를 나눠 개발하는 팀"으로 굴리는 실전 패턴입니다. 2026-07-03 세션9에서 tunaRound 자체 리팩토링(R1-R10)을 이 방식으로 수행하며 실증한 내용을 정리했습니다. 통신 배선은 [a2a-usage](a2a-usage.md), 설계 정본은 [파트너 위임](../design/v2-a2a-partner-delegation_2026-07-02.md)·[워커 데몬](../design/v2-a2a-worker-daemon_2026-07-03.md)을 참고하세요.

## 0. 한 줄

작업을 **A2A 큐**로 배정하고, **워커 에이전트**(claude/codex/로컬LLM, 로컬·크로스머신)가 각자 브랜치에서 처리하며, **PR + CI**가 게이트를 서고, **통합자**가 리뷰·머지하는 구조입니다. 사람은 목표를 발행하고 감독할 뿐(semi-a2a HITL), 발견·실행·완료·검증은 기계끼리 돕니다.

## 1. 역할 맵

| 역할 | 하는 일 | 이번 세션 대응 |
| --- | --- | --- |
| **통합자(integrator)** | 스펙 확정, task dispatch, 결과 리뷰·독립검증, PR 관리, 머지 | Windows Opus |
| **워커(worker)** | 배정된 task를 자기 머신/러너로 수행, 브랜치에 커밋·푸시 | Mac claude(A2A), 로컬 Codex(A2A), Sonnet 서브 |
| **CI 게이트** | PR마다 다OS 빌드·테스트·clippy를 hermetic하게 검증 | GitHub Actions 3-OS 매트릭스 |
| **큐(broker)** | 작업 배정·상태 권위. `to_agent`로 라우팅 | `tunaround serve` 코어 |

A2A 큐 = 이슈 트래커, git PR = 코드 통합, CI = 게이트. 이미 GitHub Flow에 근접한 구조입니다.

## 2. 실제 흐름 (이번 세션 R7 예시)

1. **스펙**: 통합자가 R7(retriever/reader Result 계약)을 자기완결적 스펙으로 확정. **스펙은 커밋하지 않고 A2A task 메시지 본문으로 전달**합니다(헤드리스 워커는 `message.text`를 그대로 러너 프롬프트로 받음).
2. **dispatch**: `SendMessage`로 `to_agent=mac-worker`에 던짐 → `task_id` 획득(submitted).
3. **워커**: 맥 워커가 poll → claim(working) → 헤드리스 claude(`--write`)가 브랜치 checkout·pull·구현·테스트·커밋·푸시.
4. **완료 감지**: A2A `complete_task`(SSE로 통합자에 실시간) **또는** git 브랜치 push(통합자가 auto-poll). 결과물이 코드면 git이 진짜 완료 신호입니다.
5. **독립검증**: 통합자가 브랜치를 pull해 diff 리뷰 + `cargo test`·clippy 재실행. R7의 경우 의미론(폴백을 Err로 잘못 승격 안 했는지)까지 확인.
6. **PR + CI**: PR을 열면 CI가 다OS로 검증. 통과하면 머지(merge commit으로 태스크별 히스토리 보존).

R1-R10을 이렇게 3자(Opus 통합 + Codex 워커 + Mac 워커)가 나눠 처리하고 PR #1로 main에 머지했습니다.

## 3. 워커 형태: 데몬 vs 대화형 (토큰 비용이 핵심)

같은 코어에 두 종류 워커가 붙습니다. **선택 기준은 유휴 토큰 비용과 감독 필요성**입니다.

| | 복붙(사람 릴레이) | 유휴 폴링 토큰 | 실작업 토큰 | 감독(HITL) |
| --- | --- | --- | --- | --- |
| 수동 프롬프트 | 매 task | 0 | 실행 시 | 매번 |
| `/loop 30s` 대화형 | 없음 | **매 틱 소모** | 실행 시 | 세션에서 |
| **헤드리스 데몬** | 없음 | **0** | 실행 시만 | 없음(자율) |

- **데몬(`tunaround work`)**: 폴링이 순수 HTTP(Rust)라 유휴 0토큰, 실제 task가 있을 때만 fresh claude/codex 스폰. task마다 새 프로세스라 **handoff·/clear 불요** = 무인·반복 워커의 정답.
- **대화형 에이전트**: 살아있는 세션은 스스로 폴링을 안 해서 트리거가 필요합니다. `/loop`로 자동화하면 복붙은 없어지지만 **매 틱 모델 턴 = 유휴에도 토큰**(1시간이면 ~120턴 낭비). 지켜보며 승인할 때만 값어치가 있습니다.
- **2레인 패턴(권장)**: 한 머신이 **데몬(`mac-worker`, 자동 레인)** + **대화형(`mac-claude`, 감독 레인)**을 동시에 제공하고, dispatcher가 task 성격으로 `to_agent`를 골라 라우팅합니다. **agent id를 분리**해야 합니다(같은 id면 경합 claim). R2(조건부 상태전이)가 이중 실행은 막지만 누가 잡을지는 비결정적이라 id 분리가 깔끔합니다.

## 4. 크로스플랫폼: CI 매트릭스 vs 워커 플릿 (역할이 다름)

| | 크로스플랫폼 **검증** | 크로스플랫폼 **수정(agentic)** |
| --- | --- | --- |
| **GitHub CI 매트릭스** | 정답(싸고 hermetic, PR 게이트) | 못 함(게이트일 뿐, 못 고침) |
| **semi-a2a 워커 플릿** | 가능하나 CI가 나음 | **존재 이유(실기에서 고쳐 되던짐)** |

- 단순 "리눅스/맥에서 빌드·테스트"는 **CI 매트릭스 한 줄**(`os: [ubuntu, macos, windows]`)이면 hermetic하게 됩니다. 자기 머신을 쓰는 워커 플릿은 non-hermetic(로컬 툴체인·상태)이라 영구 게이트로는 CI가 낫습니다.
- 워커 플릿이 빛나는 지점은 **CI가 못 하는 agentic 루프**입니다: CI 실패 → 그 실패를 실기 워커에 task로 던짐 → 재현·수정·푸시 → CI 재실행. 실제 플랫폼 자원(설치된 에이전트·로컬 GPU·플랫폼 API)이 필요한 수정은 hermetic CI로는 불가능합니다.
- **이상적 조합**: 검증은 CI 매트릭스, 실패하면 워커가 실기에서 고쳐 되던짐. 급하면 실기 워커로 CI보다 먼저(그리고 macOS runner 10배 분 없이) 선확인.

## 5. 이번 세션이 실증한 finding (증거 포함)

1. **CI가 로컬이 못 잡는 플랫폼 버그를 게이트로 막았다.** R3(watchdog 프로세스 트리 kill)의 Unix 경로 `kill -9 -PID`가 util-linux에서 음수 인자를 옵션으로 파싱해 no-op → 백그라운드 자식 생존. 이 테스트는 `#[cfg(unix)]`라 **Windows에서 작성한 워커도, Windows 로컬 통합자도 한 번도 실행하지 못했고**, Linux CI가 처음 돌려 잡았습니다. 수정 = `libc::kill(-pid, SIGKILL)`(c9905e8). macOS 실기 워커로도 통과 확인(0.23s). = **다OS 매트릭스가 R3류를 영구 게이트로 막는다.**
2. **데몬은 유휴 0토큰, `/loop`은 유휴에도 토큰.** 무인 워커는 데몬이 비용상 항상 유리합니다.
3. **스테일 데몬 = 이미 고친 버그 재현.** R7 처리 중 맥 데몬 바이너리가 R10(세션 재연결) 이전 빌드라 `complete_task` 404가 회복 불가. 결과물(편집·커밋·푸시)은 git으로 정상 landing, A2A 완료 신호만 유실. **교훈: 워커는 자기가 의존하는 수정이 포함된 최신 바이너리로 띄워야 한다.**
4. **complete 실패해도 git이 진짜 완료 신호.** 통합자가 브랜치 push를 auto-poll하면 A2A 완료신호 유실과 무관하게 결과를 회수합니다.
5. **동시 워커는 워크트리(또는 태스크당 브랜치) 격리 필요.** 공유 워크트리에 동시 실행하면 cargo 빌드가 서로 오염됩니다.
6. **docs-only 변경은 CI를 건너뛰게 한다.** 문서 커밋마다 3-OS 매트릭스(macOS 10배 분)를 돌리면 낭비라, `paths-ignore: ['**.md', 'docs/**']`로 스킵.

## 6. 안티패턴

- **스펙을 브랜치에 커밋해 전달하려 함**: 틀림. 헤드리스 워커는 A2A task 본문을 프롬프트로 받으므로 스펙은 **메시지 본문**에 실어야 합니다.
- **대화형 세션을 매 task 복붙으로 굴림**: `/loop`나 데몬으로 대체 가능. 반복이면 데몬.
- **플랫폼 분기 코드를 한 OS에서만 검증**: `#[cfg(unix)]`/`#[cfg(windows)]`는 그 OS(또는 CI 매트릭스)에서 반드시 돌려야 합니다.
- **cross-cutting task를 스펙 없이 위임**: R1·R2·R7 같은 넓은 변경은 통합자가 계획서보다 더 구체적인 스펙(전파 vs 흡수 경계 등)을 확정한 뒤 위임합니다.

## 7. 최소 재현 레시피

```bash
# 1) 통합자: 코어(큐) 기동
tunaround serve 0.0.0.0:8770 --token <TOK> --db ~/.tunaround/broker.db

# 2) 워커: 자동 레인 데몬(최신 바이너리로!)
tunaround work --core http://<코어IP>:8770/mcp --token <TOK> \
  --agent mac-worker --runner claude --write --interval 20 \
  --project-path <브랜치 체크아웃>

# 3) 통합자: task 본문에 자기완결 스펙 + git 워크플로를 실어 dispatch
curl -s -H "Authorization: Bearer <TOK>" -H "Content-Type: application/json" \
  -X POST http://<코어IP>:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"1","method":"SendMessage","params":{
        "message":{"messageId":"m1","role":"user","parts":[{"text":"<스펙+절차>"}]},
        "fromAgent":"integrator","toAgent":"mac-worker"}}'

# 4) 통합자: 브랜치 push 감지 -> pull -> 독립검증(cargo test/clippy) -> PR -> CI green -> 머지
```

PR CI(`.github/workflows/ci.yml`)는 push/PR to main에서 3-OS 빌드·테스트·clippy를 돌립니다. docs-only는 `paths-ignore`로 스킵됩니다.
