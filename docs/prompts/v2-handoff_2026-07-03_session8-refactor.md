# tunaRound 핸드오프 - 2026-07-03 세션8 (후반: A2A 3자 리팩토링 도그푸딩)

> 콜드 스타트 가정. 이 문서 + `docs/plans/v2-refactor-from-reviews_2026-07-03.md`(리팩토링 계획) + `docs/reviews/`(제미나이·코덱스 원문) + `context-notes.md`(하단) + CLAUDE.md 현재상태로 이어갈 수 있게 씀. 이전: 같은 세션 전반은 CLAUDE.md 현재상태(세션8) 참조.

## ⓪ 가장 먼저

1. **`git pull --rebase origin main`** + `git fetch origin`. **origin/main**엔 세션8 전반(스트리밍 Phase 2·워커 데몬·outbound A2A) + 동구님 GPT의 README·a2a-usage 재작성이 있음. **리팩토링은 별도 브랜치 `refactor/reviews-2026-07-03`**(pushed)에 있음: `git checkout refactor/reviews-2026-07-03`로 그 위에서 이어감.
2. cargo는 **Bash 툴**로. 브랜치 검증: `cargo test --features "morphology mcp serve worker"` → **310 pass 기대**. 풀피처+worker+a2a-out은 그 이상.
3. 리팩토링 브랜치 head = **98b6298**. main과 겹치는 파일 0(브랜치=코드, main 새커밋=문서)이라 **머지 clean**.

## ① 이 세션 후반이 한 것 (한 줄)

제미나이·코덱스 리뷰를 삼분류해 리팩토링 계획을 세우고, **그 리팩토링을 tunaRound 자체 A2A 파트너 위임으로 3자(Opus 통합자 + Codex 워커 + Mac 워커)가 나눠 수행**했다(= 실제 코드 개선 + 워커 데몬·이기종 위임·크로스머신 도그푸딩 동시 실증).

## ② 리팩토링 진행 (브랜치 refactor/reviews-2026-07-03)

| 태스크 | 담당(백엔드) | 커밋 | 상태 |
|---|---|---|---|
| R4 --context-map parse→Result | Opus 직접 | a8b894e | ✅ |
| R1+R2 MCP 에러계약 + 저장소 조건부 상태전이 | Opus(Sonnet 서브) | b78df01 | ✅ (top, 이중claim/terminal덮어쓰기 차단) |
| R6 Embedder dim 동적화 | **Codex 워커(A2A)** | ced09e6 | ✅ |
| R5 save_session orphan 정리 | **Mac 워커/claude(A2A, LAN)** | d4b6815 | ✅ |
| R10 워커 MCP 세션 재연결 | Opus(Sonnet 서브) | c58df41 | ✅ (도그푸딩 finding fix) |
| R8 검색 폴백 tokenizer 일원화 | tunaLlama(→직접) | 4c27ab2 | ✅ |
| R3 watchdog 프로세스 트리 종료 | **Codex 워커(A2A)** | 98b6298 | ✅ (Win /T, Unix process_group) |
| **R7 retriever/reader Result 계약** | **Mac** | - | ⏳ 다음(큼, 신중 리뷰) |
| R9 A2A poll 견고화 | - | - | 옵션/후순위(현 구현 견고) |

**8/9 완료.** 각 커밋에 담당(Opus/Codex/Mac/plugin) 명기. 검증 310 pass.

## ③ 도그푸딩 findings (usecase 재료 - 중요)

1. **R10 = 도그푸딩이 찾은 실버그**: 워커 MCP 세션이 긴 러너 실행(codex/claude 수 분) 중 만료 → complete_task 404. 파일 편집은 landing되나 완료보고 유실. → R10로 자동 재연결(404 시 handshake 재수행+1회 재시도) 수정.
2. **complete 404여도 결과는 git으로**: 워커가 파일 편집 후 커밋+푸시하면 통합자가 pull. A2A 완료신호(SSE)가 정석이나 리팩토링은 결과물=코드라 git-watch로 감지(통합자가 브랜치 push를 auto-poll = 사람 릴레이 0).
3. **동시 워커는 워크트리 격리 필요**: R10/R8을 공유 워크트리에 동시에 띄웠더니 cargo 빌드가 서로 오염(크레이트 전체 컴파일이 미완성 편집 끌어들임). → 워커당 워크트리(Agent `isolation:worktree`) 또는 **태스크당 브랜치**.
4. **워커 패턴 = live vs headless**: 살아있는 Claude Code 세션을 워커로 쓰면 **handoff+/clear 반복**이 필요(맥락 축적). **헤드리스 `tunaround work` 데몬은 fresh claude spawn per task라 handoff·/clear 불요** = 워커 duty의 정답. 세션은 사람이 직접 운전할 때만.
5. **tunaLlama 플러그인은 config 필요**: `~/.tunallama/config.toml` 없으면 로컬 LLM 위임 못 하고 에이전트가 직접 처리. 진짜 로컬LLM 워커 쓰려면 config 세팅.
6. **방법론**: 우리 semi-a2a는 이미 **GitHub Flow**(dispatcher=배정, 워커=브랜치 작업, 통합자=PR 리뷰·머지)에 근접. A2A 큐=이슈트래커, git PR=코드통합. **도입 권장: (a) 태스크당 브랜치, (b) PR CI(GitHub Actions build+test+clippy)** = 에이전트 개발팀 완성.

## ④ 남은 일 (우선순위)

1. **R7**(retriever/reader Result 계약, 큼): Mac 워커에. 동구님은 맥에 **`tunaround work` 헤드리스 데몬 연속 루프**(--interval, --once 없이) 켜두면 됨(핸드오프·/clear 불요). Opus가 dispatch + 타이트 스펙 + 신중 리뷰. **먼저 브랜치 clean 상태에서 시작(지금 그러함).**
2. **브랜치 → main 머지**: 8태스크 검증 후. 겹치는 파일 없어 clean. 머지 시 checklist R1~R9 완료표시 + README 로드맵 갱신.
3. **방법론 도입**: PR CI 워크플로(.github/workflows) + 태스크당 브랜치 전환.
4. **usecase 문서 작성**: "에이전트 개발팀 = GitHub Flow + A2A 큐 + PR CI + 헤드리스 워커" + 위 findings. 동구님이 원한 usecase.
5. **정리(선택)**: `.gitattributes`(`* text=auto eol=lf`)로 CRLF 경고 제거(머지 경계에서 1회 renormalize) · `.omc/` gitignore · 데모 코어의 stuck task(R6/R3 working 잔여, 무해).

## ⑤ 인프라 상태

- **A2A 리팩토링 코어**: `serve 0.0.0.0:8770 --token REFACTOR`(윈도우 LAN 192.0.2.10)를 도그푸딩용으로 띄웠었음. 세션 끝에 정리(프로세스 kill). R7 재개 시 재기동.
- git: **LF-in-repo 확인**(i/lf, autocrlf=true) → 맥(LF) 충돌 없음. 담당 줄 분리 규약 유효.
- 검증 기대: 브랜치 `morphology mcp serve worker` 310 pass, clippy 클린.

## ⑥ 규율 (유지)

- 구현 위임=Sonnet 서브 or A2A 워커(codex/mac) + Opus 리뷰·독립검증. cargo=Bash. 한국어 마침표·새파일 역할주석·em-dash 금지.
- 굵직한 결정 재론 금지. 배포·릴리스는 비우선(비공개 레포, git 히스토리 IP 정리는 별개).
