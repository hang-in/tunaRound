# tunaRound v2: 브로커 거버넌스 - 에이전트 네이밍 + 미수신/미처리 task 처리 (설계 노트)

> 2026-07-03 세션9. 크로스머신 도그푸딩 중 두 혼란이 드러났다. (1) `win-opus`(dispatcher)로 task를 보냈더니 아무도 폴링 안 해 영영 submitted. (2) read-only 워커에 write task가 가면 실패. 이를 계기로 브로커의 어드레싱·미배달 규칙을 정리한다. 정본 배경: [파트너 위임](v2-a2a-partner-delegation_2026-07-02.md) · 사용법 [a2a-usage](../reference/a2a-usage.md).

## 0. 프레이밍

**우리 브로커 = agent-id 라우팅 task queue.** 그래서 메시지 큐/액터 시스템의 성숙한 패턴이 그대로 적용된다. `agent id`는 NATS의 subject + queue group에 해당하고(같은 id를 폴링하는 워커들이 한 그룹), 던지기는 producer, 워커는 consumer다. 새 메커니즘을 발명하기보다 이 렌즈로 기존 패턴을 빌려온다.

## 1. 문제 두 축

### 축 A: 네이밍/어드레싱
- 증상: `win-opus`는 dispatcher(던지는 쪽)라 poll/claim을 안 하는데, `to_agent=win-opus`로 보내면 소비자가 없어 영원히 대기.
- 근인: agent id가 "역할(워커냐 dispatcher냐)"을 드러내지 않고, 브로커가 "폴러 없는 id로의 발송"을 막지도 알리지도 않음.

### 축 B: 미수신/미처리
- **no-consumer**: 폴링하는 워커가 없는 id로 간 task = 조용히 영구 submitted(dead letter인데 알림이 없음).
- **능력 불일치**: read-only 워커에 write task, 또는 engines 없이 빌드된 워커에 `runner=http` task = claim 후 실패(또는 애매하게 처리).
- **claim 후 워커 사망**: 긴 실행 중 워커/세션이 죽으면 working에 고착(R10 계열).

## 2. 레퍼런스 (프리아트)

| 문제 | 성숙한 패턴 | 출처 |
| --- | --- | --- |
| 어드레싱/그룹 | subject + **queue group**, routing key | NATS, Celery |
| 액터 주소/감독 | address, supervision, dead letters | Akka, Erlang/OTP |
| no-consumer / 미배달 | **Dead Letter Queue(DLQ)** | SQS, RabbitMQ, Kafka |
| 미완료 재배달 | **visibility timeout / TTL**, requeue | SQS |
| 워커 거부 | **ack / nack** | RabbitMQ |
| 능력 기반 라우팅 | capability advertisement | **A2A Agent Card `skills`**(우리가 이미 보유) |

핵심: 축 B의 표준 자리를 **A2A Agent Card**가 이미 마련해뒀다(능력 광고). 축 A는 큐/서브젝트 네이밍 관례를 따르면 된다.

## 3. 규칙 (채택)

### 3.1 네이밍/어드레싱
- **`to_agent`는 폴링하는 워커 id만.** dispatcher id는 `from_agent` 전용이며 절대 `to_agent`가 되지 않는다.
- **네이밍 = `{머신}-{역할|러너}`**: 워커는 `mac-worker`·`win-worker`(자동, claude 기본)·`mac-codex`·`mac-llm` 등 이름만 봐도 워커임과 러너가 드러나게. dispatcher는 `{머신}-dispatch`나 사람 이름(`win-opus`) - 이건 던지기 전용.
- **레인 종류 접미어 관례**: 자동(헤드리스)=`-worker`류, 감독(대화형 세션이 poll)=`-claude`/`-codex`류. 2레인 문서와 일치([agent-dev-team](../reference/agent-dev-team.md)).

### 3.2 능력 광고 (Agent Card)
- 각 워커/노드가 Agent Card(또는 poll 응답)에 **runner 종류 + write 가능 여부 + 빌드 피처**를 노출한다. dispatcher·doctor가 이를 참고해 라우팅/진단한다.
- A2A 스펙의 `skills`/`capabilities` 필드가 표준 자리. 최소판: `{agent, runner, mode(read-only|write)}`.

### 3.3 미배달/미처리 처리
- **no-consumer TTL**: 브로커가 오래(예: N분) claim 안 된 submitted를 `expired`로 전이하고, poll/카드에 "이 agent 폴러 없음"을 표시한다. 조용한 영구 대기를 없앤다.
- **워커 NACK**: 못 하는 task는 `fail_task`에 명확한 사유("read-only 레인인데 write 필요", "engines 피처 없음")를 남긴다(이미 fail_task 전이 보유 - 사유 표준화만).
- **claim 후 사망 requeue**: working이 타임아웃을 넘기면 재큐(submitted 복귀) 또는 dead-letter. visibility timeout 패턴. 상세는 §3.4.

### 3.4 워커 생존 감지 + 격리 (claim 후 사망)

증상: 워커가 claim(→working)한 뒤 죽으면(프로세스 사망·세션 만료·self-disruption) task가 영구 `working` 고착, 아무 신호 없음. 2026-07-03 뱃지 task가 이 케이스(`updatedAt`이 claim 시각에 얼어붙음).

**생존 감지 - 층위별(싼 것부터):**
1. **고착 노출 (가장 쌈, 우선)**: 브로커가 이미 `updatedAt`을 보유하므로, `poll`/`tasks` 출력에 `working`인데 updatedAt이 오래된 task를 "stuck?"으로 표시만 한다. 사람/dispatcher가 취소·재던짐 결정. 워커 변경 0, 자동 개입 0 = semi-a2a에 부합.
2. **claim TTL requeue (브로커만)**: claim 시 `claimed_at` 기록 → 넉넉한 TTL(예 20~30분) 초과 `working`을 자동 `submitted` 복귀 또는 `expired`. 재큐-루프 방지 위해 max-retry 필요.
3. **하트비트/lease 갱신 (워커+브로커)**: 워커가 실행 중 주기 ping → 무응답 시 사망 판정·재큐. 정밀하나 워커 배선 필요. **YAGNI**(작업이 늘 길고 사망이 잦을 때만).

레퍼런스: SQS visibility timeout(2), Temporal/Cadence activity heartbeat(3, 긴 작업 표준), K8s node lease. 2·3은 같은 "lease" 가족(정적 TTL vs 갱신형).

**워커 격리 (self-disruption 방지):** 위 생존 감지는 "죽으면 안 썩게"의 일반 안전망일 뿐, self-disruption은 못 고친다. 워커의 project-path가 **node 자신이 도는 살아있는 클론**이면, repo-switch·`reset --hard`·`git clean` 같은 write task가 발밑을 갈아엎어 node·워커를 자살시킨다(2026-07-03 뱃지 task 실증). 재큐하면 **재자살 무한루프**가 된다. → fix: 워커는 **node가 도는 클론과 분리된 별도 클론/워크트리**에서 write(임시 워크트리 `isolation:worktree` 답습). 관계: **생존 감지 = 죽으면 안 썩게(일반), 워커 격리 = 애초에 자살 안 하게(이 케이스)** - 상보적.

## 4. tunaround 최소 적용 (우리가 이미 반쯤 보유)

이미 가진 것: **8-state**(submitted/working/failed/canceled = dead-letter 재료), **Agent Card**(능력 광고 자리), **fail_task 전이**(NACK 재료), **doctor**(피처/능력 진단). 얹을 것만.

**우선순위(YAGNI, 개인 2~3머신엔 풀 DLQ 과함):**
1. **네이밍 컨벤션 문서화**(이 문서 §3.1) + a2a-usage에 "to_agent는 워커만" 한 줄. (비용 0)
2. **Agent Card/poll에 능력 광고**(runner·write·피처). doctor·dispatch가 참고. (소)
3. **고착 노출**: `poll`/`tasks`에 오래된 `working`(updatedAt 낡음)을 "stuck?"으로 표시(§3.4-1). `updatedAt` 이미 있어 거의 공짜. (소)
4. **no-consumer TTL 알림**: submitted가 TTL 초과 시 expired + poll 표시. (소-중)
5. **워커 격리**: write task는 node 실행 클론과 분리된 워크트리에서(§3.4). self-disruption 방지. (소-중)
6. (후속) claim TTL requeue, 하트비트, 능력 기반 자동 라우팅.

## 5. 비범위

- 풀 DLQ 인프라·재시도 정책 엔진·우선순위 큐 = 개인 규모엔 과함. 필요 신호 시.
- 능력 기반 "best-fit 자동 선택"(dispatcher가 카드 보고 최적 워커 자동 고름) = Phase 2 이후.

## 5.5 구현 현황 (세션10, 2026-07-04)

§4 우선순위 1~5를 모두 구현했다(사용법 [a2a-usage §8](../reference/a2a-usage.md)).

- **#1 네이밍 컨벤션**: a2a-usage.md에 "to_agent는 폴링하는 워커 id만" + `{머신}-{역할|러너}` 관례 문서화.
- **#2 능력 광고**: Agent Card에 `buildFeatures`(compile-time cfg!로 코어의 컴파일된 피처). 워커별 runner/write 광고는 워커 레지스트리 필요 → §6 후속. poll엔 미추가(poll=task 목록이지 capability 아님).
- **#3 고착 노출**: `poll_tasks`/`get_task`/`tasks`에 `⚠stuck?(N분)` 표시(working·updatedAt 낡음).
- **#4 no-consumer**: `⚠no-consumer?(N분)` 표시(submitted·createdAt TTL 초과) + 신규 `tasks` MCP 도구(브로커 전역 조망, 폴러 없는 task까지 보임). **편차**: §3.3/§4-4는 "expired로 전이"를 적었으나, **A2A 스펙에 expired state가 없어**(세션8 interop 정직화 유지) 비표준 enum을 추가하는 대신 **표시 신호**로 구현했다. dispatcher는 침묵 대신 신호를 받고, 자동 전이(requeue)는 §6로 미룬다.
- **#5 워커 격리**: **편차**: 자동 워크트리 프로비저닝(§3.4) 대신 **가드레일**로 구현. write 워커의 작업 디렉터리가 node 실행 클론과 겹치면(canonical 경로 조상/자손/동일) 진입 시 거부한다(`write_lane_disrupts_node`). 실제 사고(2026-07-03 뱃지 task)를 막는 안전망이며, 자동 워크트리 생성은 Windows git 리스크·검증 필요로 후속.

검증: `cargo test --features "morphology mcp serve worker"` 344 pass, clippy clean. 커밋 4개(#3·#4 / #2 / #5 / 문서).

## 6. 한 줄 결론

브로커를 "agent-id 라우팅 task queue"로 명시적으로 인정하고, **네이밍 관례(워커만 to_agent) + Agent Card 능력 광고 + no-consumer TTL** 세 가지만 얹으면 오늘의 두 혼란이 구조적으로 사라진다. 나머지는 SQS/NATS/Celery/A2A의 기존 패턴을 필요할 때 빌려온다.
