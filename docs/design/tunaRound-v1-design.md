---
type: design-spec
status: superseded
updated_at: 2026-06-29
superseded_by: tunaRound-v1-design_2026-06-29.md
---

# tunaRound v1 설계 (spec) [SUPERSEDED]

> ⚠️ 이 문서는 자동토론 전제의 옛 설계입니다. 2026-06-29 사람 주도 피벗으로 [tunaRound-v1-design_2026-06-29.md](tunaRound-v1-design_2026-06-29.md)가 supersede했습니다. 현행 spec을 보세요.

> 표기 규칙: 존댓말, em-dash 금지(일반 대시/콜론). 이 spec은 brainstorming(2026-06-29)에서 사용자 승인된 설계입니다. 다음 단계는 writing-plans -> 구현입니다.

## 1. 한 줄 정의

터미널에서 **Codex CLI와 Claude Code가 구조화된 라운드로 토론**하고, 토론이 **수렴하면 결론**짓는 앱. 멀티세션은 Redis로 코디네이션. 최종 목표는 협업 코딩이고, v1은 그 substrate(에이전트 토론)를 증명합니다.

## 2. 배경 / 왜 새 앱인가

- **tunaFlow**(`~/privateProject/tunaFlow`): 데스크톱(Tauri) 다중 에이전트 오케스트레이터. 이미 Codex/Claude를 CLI로 구동하고 Roundtable 토론(Sequential/Deliberative)을 함. 단 단일 머신 + SQLite + 데스크톱 GUI, **Redis 멀티세션 없음**.
- **tunaSalon**(`~/privateProject/tunaSalon`): 대화 흐름 엔진(누가 언제 말할지) + **Redis 멀티세션(redis-bus)** + FlowMeter 수렴 감지. 단 화자는 LLM API(실제 CLI 에이전트 아님).
- **tunaRound**: 두 레포의 강점을 **터미널 네이티브 앱**으로 결합. tunaFlow의 에이전트 구동/Roundtable을 포팅하고, tunaSalon의 Redis 멀티세션 + 수렴 감지를 얹음.

## 3. 아키텍처 (레이어 + 출처)

| 레이어 | 책임 | 출처 / 포팅 대상 |
|---|---|---|
| 에이전트 러너 | Codex·Claude를 CLI로 구동(claude `-p --output-format stream-json`, codex), 턴마다 호출, stream 파싱 | tunaFlow `src-tauri/src/agents/`(claude.rs, codex.rs, codex_app_server.rs) 포팅 |
| 토론 진행 | 구조화된 라운드(순차/병렬)로 발언 순서 관리 | tunaFlow Roundtable(`execute_round`) 포팅 |
| 수렴 감지 | 토론이 같은 자리를 맴도는지 측정 -> 결론/종료 전환 신호 | tunaSalon `flow.rs`(FlowMeter, 토큰 중복 근사) 차용 |
| 멀티세션 버스 | 동시 다중 세션 + 멀티 관찰자/참여 + 세션 재개. command/event stream, owner lease, presence | tunaSalon `session_bus.rs`(redis-bus) 포팅 |
| 영속 | 세션 상태·전사·결론 저장(재개용) | SQLite (tunaSalon roomstore 패턴) |
| 터미널 UI | 전사 + 참가자 상태 + 입력. 멀티 관찰 | ratatui + crossterm (tunaSalon TUI 패턴) |

**의도적으로 분리된 유닛:** 에이전트 러너(에이전트가 어떻게 말하나) / 토론 진행(누가 언제 말하나) / 수렴 감지(언제 끝나나) / Redis 버스(세션 간 코디네이션) / UI(렌더). 각각 독립 테스트 가능한 경계.

## 4. 핵심 결정 (승인됨)

- **흐름엔진 역할 = 수렴 감지만.** 턴 순서는 Roundtable 구조(순차/병렬)로 결정하고, tunaSalon의 Hawkes 발언-욕구 리듬은 **쓰지 않음**. FlowMeter의 "수렴도"만 가져와 "토론이 맴돈다 -> 결론으로 전환" 판단에 사용. (2명 과제 토론엔 유기적 리듬보다 구조 + 종료 판단이 적합.)
- **Redis 멀티세션 = 동시 세션 + 멀티 관찰자/참여 + 세션 재개.** 분산 에이전트(머신마다 다른 에이전트)는 v1 비목표 = 단일 머신.
- **에이전트 = 실제 CLI**(Codex CLI, Claude Code). LLM API 직접 호출 아님. (tunaFlow가 푼 부분을 포팅.)
- **스택 = Rust + tokio + ratatui + redis + rusqlite.**

## 5. v1 범위 (substrate + 토론 모드)

1. 두 에이전트(Codex·Claude)를 CLI로 구동, 한 쟁점/작업을 **구조 라운드로 토론**(예: 각자 입장 -> 반박 -> 보완).
2. **FlowMeter 수렴 감지**: 토론이 맴돌면 결론 리포트 생성 + 세션 종료.
3. **Redis 멀티세션**: 여러 세션 동시 진행, 여러 터미널이 한 세션을 관찰/참여, 끊긴 세션 재개.
4. 전사·결론을 터미널에 렌더(ratatui).

### v1 비목표 (명시적 제외)
- 공유 작업공간(git/파일) + 실제 코드 실행/검증 = 진짜 협업 코딩 -> **v2**.
- 분산(머신 간) 에이전트 -> 이후.
- 2명 초과 N 에이전트 -> substrate 검증 후.
- 웹/GUI -> 터미널 전용.

## 6. v2+ (다음 마일스톤, 참고)

- **협업 코딩**: 공유 작업공간(샌드박스 디렉토리/git 브랜치) + 에이전트가 실제 코드 편집/실행/테스트, 토론으로 결정 -> 구현 -> 리뷰 루프(tunaFlow Plan/Dev/Review 차용).
- N 에이전트(Gemini 등 추가), 분산 실행.

## 7. 주요 리스크 / 확인 필요 (다음 세션이 plan 단계에서 풀 것)

- **CLI 구동 안정성**: claude/codex의 비대화형 스트림 스키마가 버전마다 바뀜(tunaFlow에 hardening 이력 있음 - `claudeTransportFlipHardeningPlan` 참고). 포팅 시 그 견고화도 같이 가져올 것.
- **턴 컨텍스트 조립**: 각 에이전트 호출에 토론 히스토리를 어떻게 넣을지(tunaFlow ContextPack 참고, v1은 단순 전사 주입으로 시작).
- **수렴 임계값 튜닝**: 2-에이전트 토론에서 FlowMeter 임계값은 라이브로 잡아야 함.
- **Redis 스키마**: tunaSalon redis-bus의 command/event/lease/presence를 토론 세션 모델에 매핑.

## 8. 다음 단계

1. 이 spec 사용자 리뷰.
2. writing-plans 스킬로 구현 플랜 작성(레이어별 task 분해, tunaFlow/tunaSalon 포팅 출처 명시).
3. Sonnet 서브에이전트에 구현 위임, Opus가 스펙·리뷰·검증(tuna 생태계 관례).

## 9. 출처 레포 참조 (포팅 시 읽을 것)

- tunaFlow 에이전트 러너: `~/privateProject/tunaFlow/src-tauri/src/agents/{claude,codex,codex_app_server,mod}.rs`
- tunaFlow Roundtable: `~/privateProject/tunaFlow/docs/reference/architecture-detail.md`(RT 실행 흐름), `execute_round`/`start_roundtable_run`
- tunaSalon 흐름/수렴: `~/privateProject/tunaSalon/src/flow.rs`(FlowMeter)
- tunaSalon Redis 버스: `~/privateProject/tunaSalon/src/session_bus.rs`, `redis-bus` feature 사용처(web.rs)
- tunaSalon 영속/TUI 패턴: `roomstore.rs`, `tui.rs`, `chat.rs`
