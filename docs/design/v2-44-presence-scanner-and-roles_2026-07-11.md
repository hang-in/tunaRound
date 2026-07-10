// presence(존재)와 수신(wake)을 분리하는 머신 스캐너 데몬 + role 태그 체계 개편의 설계 정본.
# v2-44 presence 스캐너 + role 체계 개편

> 2026-07-11 세션18 후속. 세션18 핸드오프 §6 제안이 사용자 승인됨. 같은 자리에서 사용자 결정 두 개가 추가됨: **(a) sup은 사람이 직접 관리하는 감독이 아니라 "그 머신에 A2A 전달이 되는가" 확인용 인프라 인디케이터로 쓴다. (b) role에 따른 명칭을 전체적으로 정리한다.** 이 문서가 세 가지(스캐너·sup 재정의·role 개편)를 한 번에 확정한다. 상위 정본은 [v2-43](v2-43-target-model_2026-07-08.md)(불변), 이 문서는 그 중 presence 배선(§3)과 로스터 뷰(§2)를 대체·구체화한다.

## 0. 핵심 문장

**presence(로스터에 뜬다) / 수신(task에 깨어난다) / 뷰(어떻게 보여준다)는 서로 다른 문제다.** presence는 머신당 스캐너 데몬 1개가 ground truth(세션 파일 활동 + 프로세스 존재)로 일괄 보고하고, 수신은 세션이 원할 때만 자기 poll을 달고, 뷰에서 인프라 데몬은 세션 카드가 아니라 머신의 상태 도트다.

## 1. 배경 (왜 지금)

- **presence 결함 실측 누적**: 세션이 로스터에서 사라짐 / 유령 poll 부활(#40) / 무장 경합(#42) / ★ 탈취(#44) / codex는 래퍼 PATH shim 의존(#38). 전부 "세션마다 poll 데몬 + 훅 + 래퍼"라는 분산 presence 구조의 파생 결함. 개별 패치는 끝났지만(세션18) 구조 원인은 남아 있다.
- **sup 정체 혼란(2026-07-11 사용자 지적)**: win-codex-sup이 project=tunaRound 태그로 세션들과 대등한 카드로 뜨는데, 실체는 특정 프로젝트 소속 세션이 아니라 그 머신의 codex 주입 경로 데몬이다. display_name 없음·구식 고정 문자열 uuid까지 겹쳐 로스터를 오염시킨다.
- **role 명칭 불일치(세션15부터 백로그)**: UI 용어는 총괄/관리자/실무자인데 태그값은 supervised/worker(+총괄 태그 없음). supervised는 "감독당하는"으로 읽혀 의미도 어긋난다.

## 2. 세 축의 분리

```text
presence  = 머신당 스캐너 데몬 1개 (스캔 → 일괄 보고, 세션 존재 = ground truth)
수신      = 세션이 원할 때만 자기 poll + Monitor (현행 유지, opt-in)
뷰        = 대시보드: role=session 카드 + 머신 헤더 인프라 도트 + worker 별도 섹션(#37)
총괄 ★   = human-ping(UserPromptSubmit 훅) 유지 (v2-42/43 불변)
```

- 세션별 presence poll 데몬(autoarm detached poll·codex 래퍼 shim)은 **폐지 대상**. 유령·경합·PATH 의존이 구조적으로 사라진다.
- 수신용 poll(Monitor에 감싸는 것)은 그대로다. presence와 무관해지므로 "무장됨 ≠ 수신중" 혼동도 해소된다(§5 도트 참고).

## 3. 스캐너 데몬 (`tunaround presence-scan`)

- **머신당 1개**, 주기 15초(=현행 heartbeat 간격), 브로커 토큰은 env/설정파일 폴백(현행 관례).
- **스캔 소스**(discover.rs 스캔 코드 용도 변경 - project_from_cwd·parse_cwd_from_jsonl_line·is_internal_cwd·age_secs_since 재사용):
  - claude: `~/.claude/projects/*/*.jsonl` mtime+cwd. 활동 신선도 임계 이내 = 라이브 세션.
  - codex: `~/.codex/sessions/**/rollout-*.jsonl` mtime + codex 프로세스 존재. **래퍼 불요**.
  - 프로세스 테이블 교차 확인(파일만 남고 프로세스 죽은 경우 제외). claude-mem 등 내부 자동화 cwd 필터는 기존 is_internal_cwd 유지.
- **보고 = 일괄(batch)**: 브로커에 "이 머신의 라이브 세션 전집합"을 upsert. 브로커는 그 머신의 role=session 항목 중 보고에 없는 것을 제거한다(전집합 보고라 diff 제거가 안전 = 유령 원천 차단, TTL은 스캐너 자체가 죽었을 때의 폴백).
- **스캐너 자신의 heartbeat = 머신 도달성 신호**. 이게 사용자가 요구한 "맥/윈에 A2A 전달이 되는가" 확인의 1차 답이다.
- SessionStart/SessionEnd 훅의 등록·해제 책임은 스캐너로 이관(훅은 human-ping과 수신 안내 additionalContext만 남음). resume·/clear 재무장 경합 자체가 없어진다.

## 4. role 태그 체계 (확정)

| 태그값 | 의미 | 등록 주체 | 뷰 |
|---|---|---|---|
| `role=session` | 라이브 TUI 세션(claude·codex 공통) | 스캐너가 보고 | 로스터 메인 카드 |
| `role=worker` | 헤드리스 워커(work 데몬) | 자기 register/heartbeat(현행) | 별도 "작업 중" 섹션(#37) |
| `role=infra` | 머신 상주 데몬(presence 스캐너·codex 주입 watcher 등) | 자기 register/heartbeat | 카드 없음. 머신 그룹 헤더의 상태 도트 |
| (총괄) | 태그 아님. online 세션 중 human_input_at 최신 = ★ | human-ping 파생 | ★ 표식(현행) |

- **`role=supervised`는 폐기**하고 `role=infra`로 개명한다. 어색한 명칭 해소 + 실체(머신 인프라)와 일치.
- infra 항목은 **project 태그를 갖지 않는다**(machine 스코프). machine·runner·purpose(예: `purpose=codex-inject` / `purpose=presence`)만.
- UI 한국어 용어 최종 매핑: 총괄(★) / 세션 카드(관리자 후보 = 던질 수 있는 자리) / 실무자(worker) / 인프라(도트). "관리자"는 태그가 아니라 "감독으로 쓰는 세션"을 가리키는 말로만 남긴다.

## 5. sup 재정의 + 로스터 뷰

- **sup(win-codex-sup 등) = role=infra, purpose=codex-inject.** 그 머신의 라이브 codex에 task를 주입하는 경로 데몬(v2-37 배선 불변).
- **어드레싱은 불변**: infra 데몬도 여전히 poll/claim 하는 에이전트다. `to_selector "role=infra,purpose=codex-inject,machine=mac"`으로 던지면 지금과 똑같이 동작한다. 바뀌는 것은 뷰뿐(대시보드=뷰, 메커니즘 아님 - v2-43 §2).
- **머신 그룹 헤더 도트**(PR #46 머신그룹 분리 위에 얹음):
  - `presence ✓/✗` = 스캐너 heartbeat = 이 머신에 A2A가 닿는가.
  - `codex 주입 ✓/✗` = codex-inject watcher online = 이 머신 codex에 던지면 받는가.
  - (선택) 세션 카드에 `수신중` 뱃지 = 스캐너가 프로세스 테이블에서 그 세션 uuid의 poll 존재를 관측(백로그 "무장됨≠수신중 표시"가 공짜로 해결).

## 6. 보고 API

- 신규 MCP 도구 `report_presence(machine, sessions: [{uuid, project, runner, display_name, age_secs}])` (HTTP `/mcp` 경유, 기존 register_agent와 동거).
- 브로커 처리: 해당 machine의 role=session을 보고 전집합으로 동기화(upsert+제거). human_input_at은 human-ping이 계속 소유(스캐너가 덮지 않음).
- 기존 `register_agent`/`heartbeat`는 worker·infra용으로 존치. `report_candidates`(discover 후보 리포터)는 스캐너로 대체 후 제거 후보(T5).

## 7. 마이그레이션 경로

셀렉터 하위호환: 브로커가 register와 selector 양쪽에서 `supervised`를 `infra`로 정규화(alias). 한 단계 유예 후 T5에서 제거.

1. **T1 브로커**: report_presence + machine 동기화 로직 + supervised→infra alias + 스캐너 순수부(discover.rs 용도 변경, 단위테스트).
2. **T2 win 라이브**: `presence-scan` 데몬 기동(안정 바이너리 경로) + autoarm 훅 간소화(detached poll 제거, human-ping·수신 안내만 유지) + codex 래퍼 PATH shim 제거 + win-codex-sup를 infra 태그(purpose=codex-inject, project 제거)로 재기동.
3. **T3 mac 배포**: A2A task로 위임(스캐너 기동 + restart-mac-mesh.sh 갱신 + mac-codex-sup 재태깅). 민감 task이므로 로컬 운영자 게이트 규약 적용.
4. **T4 대시보드 뷰**: 머신 헤더 인프라 도트 + infra 카드 제거 + (선택) 수신중 뱃지. **PR #46 머지 후** 그 위에 얹는다(#46 스코프는 불변).
5. **T5 정리**: alias 제거, discover 후보 리포터·report_candidates 제거, 문서 일괄 갱신.

**갱신 대상 목록**(T2~T5에 분배): `.claude/hooks/tuna_arm.py`·`tuna-disarm.py` / `scripts/codex`(래퍼, 제거) / codex-sup-handle 기동 스크립트(win) / `restart-mac-mesh.sh`(mac) / goal 폼 프리셋("모든 감독 role=supervised") / `docs/reference/a2a-usage.md` §9·§10 / `tunaround-config.example` / README 로스터 설명.

## 8. 수용 기준

- 작업 중 세션이 로스터에서 사라지지 않는다(스캔 ground truth). exit 시 다음 스캔 주기(±15초) 내 사라진다.
- codex 세션이 래퍼 없이(새 터미널에서 plain `codex`) 로스터에 뜬다.
- sup 카드가 로스터에 없고, 머신 헤더 도트로 "win/mac에 A2A 전달 가능"을 한눈에 확인할 수 있다.
- `to_selector role=supervised`(구)와 `role=infra`(신)가 유예 기간 동안 같은 결과를 준다.
- role 태그값이 코드·문서·UI에서 일치한다(supervised 잔존 0, T5 완료 시점).

## 9. 비범위

- 스캐너의 OS 서비스 상주화(launchd/작업 스케줄러). nohup/detached로 시작하고 상주화는 별도 후속.
- 리치 TUI. 같은 roster API를 소비한다는 계약만 명시.
- 워커 자동배정·capability 라우팅(기존 비범위 유지).
