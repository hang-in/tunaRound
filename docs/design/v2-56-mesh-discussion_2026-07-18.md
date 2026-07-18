# v2-56: mesh 토론 - 토론(chat)을 멀티세션 A2A mesh 위로 (설계 스케치)

> 지위: 착수 전 스케치(2026-07-18, 사용자 발의 "지금 토론 할려면 TUI를 따로 열어야 하잖아? 멀티세션 구조로 올리면 안될까?"). 3렌즈 적대 검증(코드 계약·기존 결정·스코프) 1회 통과분 반영. 착수 세션에서 §8 열린 결정을 확정한 뒤 정본으로 승격한다.
>
> 번호 주의: v2-55는 `src/mcp/server.rs:596` 주석이 "스피너 신선도 게이트(이슈 #94)" 의미로 이미 선점하고 있어 건너뛴다. (코드 주석의 v2-54=스피너도 문서 v2-54=온보딩 감사와 겹쳐 있다. 착수 PR에서 주석을 이슈 번호 지칭으로 정리한다, §8-6.)

## 0. 기존 결정과의 관계 (개정 선언)

**개정하는 결정 2건.** 둘 다 이번 사용자 발의(2026-07-18)에 근거한다.

1. **"크로스머신 토론 비목표"(세션6 결정, 세션12 재문서화).** 원문(docs/prompts/v2-handoff_2026-07-04_session12.md:15-18)은 "라이브 다자 토론(회담)은 단일머신 chat --roster. 크로스머신 토론은 비목표. 브로드캐스트/팬아웃 없음"을 재유도 금지로 못박았다. 개정 근거: 결정 당시(세션6~12)에는 mesh가 없었고, 지금은 라이브 세션 수신·헤드리스 워커·relay·watch-results 인박스가 완성되어 크로스머신 토론의 한계비용이 다르다. 팬아웃 프리미티브는 이번에도 만들지 않는다(driver가 좌석별 개별 send_task, §5). 착수 시 CLAUDE.md의 세션12 요약 줄("위임 vs 회담") 갱신이 필요하다(총괄 소유 공유 파일, §8-7).
2. **자동 종합(synthesizer) 라운드.** v1 원칙은 "자동 수렴 점수로 끝내지 않고 사용자가 충분하다고 판단할 때 결론을 정리"(tunaRound-v1-design:101-103)이고, REPL 선례도 종합은 별도 사람 명령 `/conclude`다(src/repl/mod.rs:718-733). mesh 토론에서는 rounds 소진 후 synthesizer 라운드 1회를 자동 실행한다. 이는 v1 원칙의 부분 개정이며, 근거는 인박스 배달 모델이다: 사람이 자리에 없는 것이 전제이므로 "종합해서 가져와라"까지가 위임 단위다. 사람 주도는 시작·좌석·rounds 지정과 합의문 수신 후 재발의로 유지된다.

**유지하는 결정들.**
- **유한 라운드.** 자동 반복은 `/debate n` 선례(기본 3, 최대 10: src/repl/mod.rs:189-211)의 유한 rounds 안에서만 돈다. 에이전트끼리 무한 토론하지 않는다.
- **대시보드 = 관제탑.** 정확한 원문은 "뷰(로스터·피드) + 목표 제출(위임 티켓 발행, 장부 경유)만. 직접 제어 UX는 제거하고 다시 늘리지 않는다"(v2-45:17)이다. 토론 시작·중단은 세션 MCP 도구로만 한다. 대시보드 goal 폼처럼 토론 시작 폼을 넣는 것도 원칙상 가능하나 이번 범위에서는 선택하지 않는다(별도 결정, 후속).
- **재발명 금지.** 신규 프리미티브 없이 기존 task 생명주기·전사 append 계약 위에서 조립한다(§3).

## 1. 문제

- 토론(chat)은 별도 REPL 프로세스다. 토론을 하려면 터미널에서 `tunaround chat`을 따로 열어야 하고, 지금 앉아 있는 총괄 세션에서는 시작할 수 없다.
- chat 좌석은 무상태 fresh spawn 헤드리스 러너뿐이라, mesh에 이미 존재하는 자원(다른 머신의 워커 lane, 라이브 세션)이 토론에 참여할 수 없다.
- mesh(A2A)는 위임 시맨틱(1 task → 1 응답 → terminal)뿐이라 라운드 구조·순차-인지·역할 주입이 없다. 다만 "그 구조를 손으로 재연하면 셔틀"이라는 프레임은 절반만 맞다: 재연 주체가 사람이 아니라 총괄 세션(LLM)이면 기존 프리미티브만으로 라운드를 자동 재연할 수 있다. 이것이 §4 Phase 0이다.

## 2. 경로: Phase 0(코드 0) → Phase 1(driver 최소본)

적대 검증(스코프 렌즈)의 반증을 수용해 2단계를 재구성했다.

- **Phase 0 (운영 레시피, 신규 코드 0)**: 총괄 세션이 기존 send_task/get_task/watch-results로 라운드를 자동 재연하는 절차를 문서화하고 1회 이상 도그푸딩한다. 검증 대상 = "크로스머신 토론이 실제로 가치 있나"라는 핵심 가설 + 라운드-over-mesh의 실물 마찰(지연·좌석 부재·발언 품질).
- **Phase 1 (브로커 discussion driver, 중형)**: Phase 0에서 가치가 확인되면 착수. 총괄 세션 토큰을 태우던 오케스트레이션을 브로커의 결정적 코드로 내린다.
- **비채택: chat에 mesh 좌석(구 Stage 1).** MeshSeatRunner·roster 확장은 Phase 1에서 하나도 재사용되지 않는 폐기성 코드이고, 발의 문제(TUI 따로 열기)를 해결하지 못하며, 검증 목적은 Phase 0이 코드 0으로 대체한다. "로컬 chat REPL에 원격 좌석 혼합"이 독립적으로 필요해지면 별도 건으로 재발의한다(부록 A).

## 3. 재사용하는 기존 계약 (적대 검증 통과)

| 계약 | 위치 | 재사용 방식 |
|---|---|---|
| 라운드 프롬프트 조립(순차-인지·역할) | `build_round_prompt` src/orchestrator/prompt.rs:51-115 (순수 함수), `role_guidance` src/orchestrator/roles.rs | driver가 그대로 호출. 발언별 MAX_ANSWER_LEN=4000자 캡 내장 |
| task 생성 | `create_task_from_message`(send_task/SendMessage 수렴) src/store/sqlite/tasks/state.rs:56-71 | 라운드 발언 1건 = task 1건, to_agent 직지정 |
| claim/complete/fail + lease | src/store/sqlite/tasks/lease.rs (lease 30분, attempt≤3 requeue) | 좌석 소비 경로 무변경(워커·라이브 poll·relay 그대로) |
| 늦은 완료 무해화 | `try_complete`의 `WHERE state='working'` 가드 src/store/sqlite/tasks/state.rs:207-237, `try_cancel` :286-304 | driver 타임아웃 시 cancel → 이후 도착하는 완료는 가드에 막힘 |
| 전사 영속 | `append_turn`(BEGIN IMMEDIATE, 세션별 MAX(msg_id)+1 = DB id 권위) src/store/sqlite/messages.rs:167-248 | 토론 발언을 `debate:<id>` 세션에 증분 추가. save_session(전량 교체) 금지 |
| 세션 네임스페이스 선례 | mesh 기억화 `a2a:<task_id>`·speaker `a2a/<agent>` src/mcp/indexing.rs:61-138 | `debate:<discussion_id>`·speaker `debate/<seat-label>` 로 답습 |
| 인박스 | watch-results(fromAgent==dispatcher 필터, digest) src/watch_results.rs:273-304 | 라운드 결과·합의문 배달(§8-1) |
| 좌석 수신 3경로 | 워커 lane(src/worker.rs:680-810), 라이브 claude(poll Monitor+자가 claim), 라이브 codex(relay 대리 claim+thread 주입 src/codex_relay.rs:136-308) | 무변경. 토론 task도 일반 task로 소비됨 |

## 4. Phase 0: 운영 레시피 (총괄 세션이 라운드 재연)

총괄 세션에 다음 절차를 지시한다(레시피는 착수 시 docs/reference/a2a-usage.md에 수록).

1. 사용자에게 주제·좌석(agent uuid 2~3개)·rounds(기본 2)를 받는다.
2. 라운드 r, 좌석 순서대로: 프롬프트 = 토론 프리앰블(§6-7) + 역할 지시 + 이전 라운드 전 발언 + 이번 라운드 앞 좌석 발언 + 주제 → `send_task(to_agent=<uuid>)` → `get_task` 폴링으로 완료 대기 → 발언 확보.
3. rounds 소진 후 총괄 자신이 합의/이견/미결을 종합해 사용자에게 보고한다.
4. 기억 복리는 부분 성립: 각 발언 task가 mesh 기억화로 `a2a:<task_id>`에 자동 색인된다(연속 전사는 없음, Phase 1의 몫).

한계(= Phase 1의 존재 이유): 총괄 컨텍스트·토큰을 소모하고, 라운드 조립이 모델 재량이라 비결정적이며, 총괄이 그 시간 동안 다른 일을 못 한다. 이 한계가 실사용에서 아프면 Phase 1로 간다.

## 5. Phase 1: 브로커 discussion driver + 총괄 MCP 도구

- **MCP 도구 2개**(tuna-broker 서버에 추가. get_discussion은 YAGNI로 배제: 진행 상태는 인박스 RESULT 줄 + read_transcript(`debate:<id>`)로 충분).
  - `start_discussion(topic, seats, rounds, dispatcher)` : seats = `[{agent(uuid), label?, role?, instruction?, live?}]` (2~6석), rounds 기본 3·상한 10. label 생략 시 로스터 display name으로 해석(speaker 가독성, uuid 원문 금지). 반환 = discussion_id. **동시 토론 1건 제한**(진행 중이면 거부, MVP).
  - `stop_discussion(discussion_id)` : 이후 라운드 발행 차단(인메모리 취소 플래그, 라운드 루프가 좌석 발행 전마다 확인). 이미 claim된 task의 러너 실행은 중단 불가(app-server 취소 API 부재, 세션28 #9)를 반환문에 명시.
- **driver = 브로커 프로세스 안의 tokio task** (discussion 1건당 1 spawn). **store 접근(create_task_from_message·append_turn·get_task)은 전부 동기(rusqlite)이므로 기존 핸들러 관례대로 `spawn_blocking` 경유**(src/mcp/tasks.rs:36-39, src/mcp/server.rs:599-603 답습). 완료 대기는 **인프로세스 순수 폴링**(2~5초 간격 get_task + 좌석 타임아웃 기본 600초): driver는 브로커 안에 있어 이벤트 버스가 불필요하고, 버스를 쓰지 않으면 sweep 무이벤트·Lagged 리스크가 아예 발생하지 않는다(적대 검증 수용).
- **라운드 루프.**
  1. 좌석 순서대로(순차-인지): `build_round_prompt`로 본문 조립 → 토론 프리앰블(§6-7) prepend → task 발행(from_agent=`debate:<id>`, to_agent=좌석 uuid) → 폴링 대기.
  2. terminal 도착 즉시 `append_turn("debate:<id>", "debate/<label>", 발언)` → 다음 좌석의 same_round로 주입.
  3. 좌석 타임아웃 = `try_cancel` 시도 후 전사에 `[무응답: <사유>]` 기록하고 skip(늦은 완료는 canceled 가드에 막혀 무해). **skip-후-계속은 신규 결정이다**(REPL /debate 선례는 임의 실패 즉시 중단: src/repl/mod.rs:749-756. mesh는 좌석 부재가 일상이라 정책을 달리한다). 한 라운드에서 전 좌석이 실패하면 토론 중단.
  4. rounds 소진 후 synthesizer 라운드 1회(§0 개정 2). **실행 주체 = start_discussion에서 지정한 synthesizer 좌석(기본 = 첫 좌석)에게 task로 위임**(driver는 러너가 없어 LLM 생성을 스스로 못 한다). synthesizer 좌석 실패 시 합의문 없이 부분 전사 포인터만 인박스로 배달(토론 유실 방지).
  5. 종료(완주·중단·실패 공통): 결과 요약 1건을 인박스로 배달(§8-1).
- **이월(carried) 없음(MVP).** rounds≤10이라 prior 전량 주입으로 시작한다(발언별 4000자 캡은 build_round_prompt 내장). REPL의 `carry_forward_digest_from_path`(src/repl/mod.rs:416-473)는 ReplSession 프라이빗이라 추출 리팩토링이 선행돼야 하며, 이는 필요가 실측된 뒤 후속으로 한다.
- **브로커 재기동 = 진행 중 토론 실패 처리.** driver 상태가 인메모리이므로 "실패 처리"가 공짜가 아니다(적대 검증 수용). 방식은 §8-4에서 확정: 권고 = 기동 시 `from_agent LIKE 'debate:%'`인 open task를 fail 전이시키는 고아 sweep(신규 store 질의 1개). watch-results는 failed를 배달하므로 dispatcher 감시가 살아 있으면 통지도 성립한다. discussion 레코드 영속(신규 테이블·마이그레이션)은 MVP 비채택.

## 6. 모델링 결정

1. **1라운드 발언 = 1 task.** task 내 멀티턴 프리미티브가 없다(input_required 전이·append_history의 프로덕션 경로 부재). 라운드 맥락은 매 task 본문에 재조립해 싣는다. 좌석이 무상태여도 성립한다.
2. **팬아웃 없음.** to_selector 다중 매칭은 의도적으로 에러(사람 선택)이므로 좌석은 start 시 agent uuid로 확정하고 driver가 좌석별 개별 send. 세션12 결정의 "팬아웃 프리미티브 없음"은 유지된다.
3. **본문은 단일 text part.** poll의 본문 추출은 `parts[0]`의 text다(첫 part가 text가 아니면 본문 없음 처리, src/mcp/format.rs:82-88). 역할·라운드 메타를 다중 part로 나누지 않고 전부 본문 텍스트에 인코딩한다.
4. **전사 정본 = messages 트리(`debate:<id>`).** 라운드 task들은 mesh 기억화로 `a2a:<task_id>`에도 개별 색인된다(중복 수용). 정본을 debate 세션으로 두는 근거는 파편화다: task별 색인은 라운드 문맥 연결이 없고, prune이 요청 지시문(history/message_json)을 슬림화한다(artifact=발언 자체는 보존되지만 그 발언을 낳은 라운드 맥락이 사라진다). 연속 전사만이 토론을 토론으로 재독 가능하게 한다.
5. **좌석 기본 = 토론 전용 헤드리스 lane.** node.toml lane(전용 agent id + `purpose=debate` 태그)으로 성립하고 lease 자동연장(워커 5분 주기)도 공짜로 얻는다. 단 "즉시"는 아니다: 머신별 node.toml 편집 + 재부팅 복구 스크립트(restart-win-mesh.ps1·restart-mac-mesh.sh)에 lane 반영이 온보딩 절차다(§9). **일반 위임 lane과 분리할 것**: 워커는 task를 순차 처리하므로(src/worker.rs:699) 토론 좌석 lane을 일반 위임과 공유하면 head-of-line blocking으로 좌석 타임아웃이 연쇄한다.
6. **라이브 세션 좌석은 opt-in**(`live: true` 명시, §8-3). 사람이 쓰는 세션의 컨텍스트를 소모하고, 라이브 claude는 lease 자동연장이 없으며(30분 초과 시 requeue 재배달 = 이중 발언 위험, driver 타임아웃 600초로 실질 차단), 라이브 codex 중 **래퍼 마커(#119) 없는 세션**(VS Code 자체 기동 등)은 사람활동 60분 게이트 밖으로 빠질 수 있다(래퍼 경유 세션은 마커 면제라 게이트 리스크 없음).
7. **토론 프리앰블 신규 정본화.** 위임 프리앰블 템플릿(구 a2a-usage §12d)은 2026-07-12 문서 재작성(e801a3b)에서 소실되어 git 구버전(2e2ce60)에만 있다(CLAUDE.md 포인터 스테일). 착수 시 토론용 프리앰블을 a2a-usage에 신규 수록한다. 골자: "[토론 규약] 이 task는 사용자가 start_discussion으로 발의한 토론 라운드다(총괄발 task 자율 수행 규약의 적용 대상 - `debate:<id>` 발신자 클래스를 규약에 명시). 이번 task에 한한 역할이다(평소 지시보다 우선). 발언만 4000자 이내로 반환하고 complete_task로 마감. 파일 수정 금지(read-only)." 금지 패턴 2개: `브로커 task ` 프리픽스(relay 주입 계약과 충돌), `\n\n[<32hex>] from=` 패턴(구세대 문자열 파서 오분리).
8. **발언 길이.** 프롬프트 조립 캡은 MAX_ANSWER_LEN=4000자 유지. artifact 자체엔 코드상 상한이 없으나 전송 계층 암묵 상한이 미실측이므로 좌석 지시에 4000자 이내를 명시한다.

## 7. 함정·리스크 (적대 검증 정정 반영)

1. **driver 타임아웃이 정본 탈출구다.** lease 만료 sweep은 poll 경로 의존 지연 sweep이라(src/mcp/format.rs:70-74가 유일 트리거) get_task 폴링은 terminal 전이를 "기다리는" 수단이 못 될 수 있다(좌석 앞으로 poll하는 소비자가 없으면 working으로 영원히 남음). driver 자체 타임아웃(600초) → try_cancel → skip이 유일하게 신뢰 가능한 탈출구다.
2. **at-least-once 재배달 = 이중 발언 위험.** lease 만료 requeue 시 같은 라운드 지시문이 재배달된다. 완화: driver 타임아웃(600초) ≪ lease(30분) + 타임아웃 시 cancel(재배달 자체를 차단).
3. **취소는 상태 전이일 뿐.** 실행 중인 러너·codex 턴은 중단되지 않는다. stop_discussion 반환문에 명시.
4. **다중 writer 규율.** 토론 전사 쓰기는 append_turn만 사용한다. save_session(전량 교체)은 클로버를 만든다.
5. **대시보드 노출의 실효 범위.** /dashboard/search는 `a2a/*` 화자만 통과시키므로 `debate/*` 전사는 검색에 안 나온다. 단 이것이 막는 것은 "과거 검색성"뿐이다: 발언 전문 자체는 라운드 task로서 기존 a2a task와 동일하게 무인증 피드·SSE·replay에 이미 표출되는 노출 클래스다. §8-2는 이 정직한 범위 위에서 결정한다.
6. **비용·지연 추정(순차-인지의 대가).** 3좌석 3라운드 + synthesizer = task 10건 직렬. 헤드리스 좌석은 fresh spawn마다 풀 컨텍스트 재수신(claude spawn 실측 약 35초)이라 정상 경로 대략 10~20분, 좌석 1개가 타임아웃까지 가면 +10분. 토론은 "던져놓고 인박스로 받는" 비동기 작업이지 실시간 관전물이 아니다. 이 전제를 사용자 표면(도구 설명)에 명시한다.
7. **#115 메이저 범프와 파일 겹침.** rmcp 2.2(MCP tool 표면)는 v2-56 Phase 1이 만질 층과 겹친다. 전용 세션 순서를 정하고 동시 진행하지 않는다.

## 8. 열린 결정 (착수 전 사용자 확정)

1. **인박스 배달 방식.** 후보 (a) from_agent=`debate:<id>` 유지 + 총괄이 `watch-results --dispatcher debate:<id>`를 추가로 띄움(Monitor 1개 추가, 라운드별 RESULT 줄 + 종료 요약) / (b) from_agent를 시작 총괄의 dispatcher로 발행(Monitor 추가 없음, 기존 인박스에 라운드 노이즈 유입·digest로 완화). 권고 = (a).
2. **대시보드 검색에 `debate/*` 노출 여부.** §7-5의 실효 범위(검색성만 차이) 전제 하에 결정. 권고 = MVP 비노출(결정 유보 비용이 0).
3. **라이브 좌석 opt-in UX.** `live: true` 명시 플래그 요구 권고(실수로 사람 세션을 끌어들이는 것 방지).
4. **재기동 실패 처리 방식.** (a) 고아 task 기동 sweep(`from_agent LIKE 'debate:%'` open → fail, 신규 store 질의 1개, 권고) / (b) discussion 레코드 영속(신규 테이블·마이그레이션, MVP 과투자). 재기동 후 진행 중이던 discussion_id 문의에 대한 계약(존재 안 함)도 함께.
5. **synthesizer 좌석 규칙.** start_discussion 지정(기본 첫 좌석) 권고. 실패 시 부분 전사 포인터 배달로 합의.
6. **v2-55 번호 주석 정리.** src/mcp/server.rs:596 등 코드 주석의 v2-54/v2-55 지칭을 이슈 번호(#94)로 바꿔 충돌 해소. 권고 = 착수 PR 동승(1줄).
7. **CLAUDE.md 갱신(총괄 경유).** 세션12 "위임 vs 회담" 요약 줄과 스테일 포인터(a2a-usage §12·§13 - 현행 문서에 해당 섹션 없음)의 정리. 개정(§0)이 확정되는 착수 세션에서 수행.

## 9. 검증 계획

- **Phase 0 게이트(선행)**: 운영 레시피로 2좌석(win 워커 + mac 워커) 2라운드 토론 1회 도그푸딩. 판정 = 발언 품질(순차-인지가 실제로 반박을 만드나)과 마찰(지연·좌석 부재)이 Phase 1 투자를 정당화하는가. 하지 않으면 Phase 1 착수 금지.
- **Phase 1 E2E**: start_discussion(헤드리스 2 + 라이브 claude 1(live:true), 2라운드) → 인박스 라운드 RESULT 수신 → read_transcript(`debate:<id>`) 라운드 구조 확인 → 합의문 도착 → stop_discussion 중도 중단(진행 중 task의 늦은 완료가 canceled 가드에 막히는 것까지) → 브로커 재기동 후 고아 sweep 동작.
- **온보딩 절차 검증**: 토론 lane을 node.toml + 양 머신 재부팅 복구 스크립트에 반영하고 재부팅 1회 생존 확인.
- 회귀: lib 테스트 전량 + driver 단위 테스트(부분 라운드·타임아웃·재기동 sweep은 시계 주입으로).

## 10. 관련 백로그와의 관계

- v2-54 P2의 get_task `wait_secs` 롱폴이 먼저 생기면 Phase 0 레시피의 폴링이 단순해진다(의존은 아님).
- 이슈 #123(세션 동작 표시)의 in-turn 신호는 토론 라운드 진행 표시와 표면이 겹칠 수 있으나 독립 진행 가능.
- 이슈 #115 중 rmcp 2.2는 §7-7 순서 조율 대상.

## 부록 A. 비채택 대안

1. **chat REPL에 mesh 좌석(MeshSeatRunner + roster 확장).** 비채택 사유: ① Phase 1이 재사용하지 않는 폐기성 코드(driver는 Runner trait 자체를 안 씀) ② 발의 문제(TUI 따로 열기) 미해결 ③ 검증 목적은 Phase 0이 코드 0으로 대체 ④ 숨은 규모가 '소'가 아님(mcp_client가 worker 피처 게이트 뒤라 피처 결정 필요, get_task 사람용 텍스트 파싱은 v2-52 ④가 걷어낸 취약 패턴의 재생산, roster dedup 규칙 예외 처리 2곳). "로컬 chat에 원격 좌석"이 독립적으로 필요해지면 별도 발의.
2. **총괄 세션 오케스트레이션의 상시화(Phase 0을 최종 형태로).** 비채택 사유: 총괄 컨텍스트·토큰 소모, 라운드 조립 비결정성, 총괄 점유. 단 Phase 0은 가치 검증 게이트로는 최적이라 선행 단계로 채택.
3. **driver의 이벤트 버스 대기.** 비채택 사유: 인프로세스 폴링으로 충분하고, 버스를 쓰면 sweep 무이벤트·Lagged 대응 규약이 추가로 필요해진다(리스크의 자기 생산).
4. **대시보드 토론 시작 폼.** 관제탑 원칙(뷰+목표 제출)상 불가능하지는 않으나, 세션 표면(MCP)이 먼저다. 후속 재검토 가능.
5. **페르소나 1급 시스템.** 좌석 instruction 자유 텍스트로 충분(YAGNI). 필요가 실측되면 재발의.
