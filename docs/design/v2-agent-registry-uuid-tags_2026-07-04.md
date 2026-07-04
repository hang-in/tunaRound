# tunaRound v2: 에이전트 등록·발견·능력 라우팅 (UUID + 태그) - 설계 노트

> 2026-07-04. 4인 티키타카 이후 동구님이 어드레싱 모델을 재설계했다. 지금의 수동 문자열 id(`mac-claude`)는 얇은 시작이었고, 규모·유연성엔 **UUID(고유) + 태그(선택)** 모델이 맞다. 다음 세션 구현 대상. 배경: [협력업체 오케스트레이션 비전](v2-partner-orchestration-vision_2026-07-04.md) · [브로커 거버넌스 §6](v2-broker-governance_2026-07-03.md) · 사용법 [a2a-usage](../reference/a2a-usage.md).

## 0. 문제 (지금 모델의 한계)

- **어드레싱 = 사람이 약속한 자유 문자열**(`to_agent="mac-claude"`). 검증·등록·자동할당 없음.
- 한계: (1) `mac-claude`는 머신+러너만 담고 목적/인스턴스를 못 담아 **거칠고 충돌**(맥에 튜나라운드 claude가 둘이면?), (2) 발송자·폴러가 문자열을 수동으로 맞춰야 함(오타 불일치 = no-consumer). 네이밍 규약은 레지스트리 없던 시절의 **반창고**였다.

## 1. 핵심 모델 (K8s의 UID + labels 패턴)

| 층 | 역할 | 예 |
| --- | --- | --- |
| **UUID** | 유일 식별·라우팅 키(충돌 0) | `a3f9c4...`(task id처럼 randomblob(16) hex) |
| **태그(KV)** | 설명·선택(발견) | `machine=mac, runner=claude, role=supervised, project=tunaround, mode=write` |
| **display name**(선택) | 로스터 가독용 | `mac-claude-tunaround` 또는 docker식 자동 라벨 |

- **UUID는 워커가 자가 발급**(뜰 때 생성, 등록 라운드트립 불필요). 브로커는 등록으로 **기록만**.
- **감독 세션(책임자)은 config에 uuid 저장 → 재기동 시 정체성 유지.** 헤드리스 워커는 매번 새로 떠도 무방(ephemeral OK).
- **태그는 KV 맵**(표준키 machine/runner/role/project/mode + 자유키). 선택 = 부분집합 매칭(label selector).

## 2. 라우팅 (두 경로)

- **정확(by UUID)**: `to_agent=<uuid>`. 지금 문자열 자리에 uuid가 들어갈 뿐.
- **능력(by tag selector)**: dispatcher가 `{runner:claude, project:tunaround}` 같은 셀렉터로 질의 → 브로커가 **매칭되는 online uuid로 해석·라우팅**. dispatcher는 uuid를 몰라도 됨(발견이 문자열 추측을 대체).

**다중 매칭 시(갈림길, 채택):** **(a) dispatcher가 로스터 보고 직접 하나 고름 = 기본**(HITL·투명, semi-a2a에 부합). **(b) 브로커 자동 배정(queue group/부하분산) = 옵션**(자동성↑, 필요 시).

## 3. 등록·발견·생존 (거버넌스 합류)

- **register**: 워커/세션이 뜰 때 `{uuid, tags, display_name?}`를 브로커에 신고.
- **heartbeat**: 주기 ping으로 online 유지(무응답 = offline, TTL). = 거버넌스 §3.4 생존감지의 lease/heartbeat 계열과 같은 뼈대.
- **discovery**: `list_agents`(online 목록 + 태그)를 노출. = A2A **Agent Card `skills`/`capabilities`** 자리(거버넌스 §2/§6). 이미 `buildFeatures`로 씨앗 있음.

## 4. 재프레이밍 (기존과의 관계)

- **네이밍 규약(`mac-claude`)은 라우팅 id가 아니라 그냥 태그**가 된다(machine=mac, runner=claude). 거버넌스 §3.1 네이밍은 "태그 관례"로 흡수.
- **no-consumer(오타 불일치)가 구조적으로 소멸**: 발송자가 문자열을 찍는 게 아니라 태그로 발견하므로 오타 대상이 없다. (거버넌스 #4 no-consumer 표시는 안전망으로 유지하되 발생 빈도↓.)
- **이미 있는 것**: task id의 randomblob hex(= uuid 발생기 재사용), Agent Card buildFeatures(태그 광고 씨앗), lease/fail(생존감지), fail_task(NACK).
- **net-new**: agents 로스터(등록/하트비트/조회) + 태그 셀렉터 라우팅 + display name.

## 5. 다음 세션 구현 스케치

1. **로스터 저장**: 인메모리 HashMap<uuid, {tags, display, last_heartbeat}>가 얇은 시작(브로커 재기동 시 워커 재등록으로 복원). 영속(agents 테이블)은 필요 시.
2. **엔드포인트/도구**: `register_agent(uuid, tags, display?)` · `heartbeat(uuid)` · `list_agents(selector?)`를 MCP 도구 + `/a2a` 메서드로. worker/poll/node가 뜰 때 자동 register + 주기 heartbeat.
3. **라우팅 확장**: `send_task`가 `to_agent=<uuid>` 또는 `to_selector={...}`를 받음. selector면 브로커가 매칭 online uuid로 **발송 시점에 해석**해 task.to_agent에 concrete uuid 저장(태스크는 항상 구체 uuid로 남음). 다중 매칭은 (a) dispatcher에게 후보 반환 후 선택 or (b) 옵션 자동배정.
4. **CLI/config**: `--agent`가 uuid를 받거나 미지정 시 자가 생성 + `--tags k=v,k=v`. node.toml 레인에 `tags`. 감독 세션은 uuid를 config에 persist.
5. **하위호환**: 전환기엔 `to_agent`가 레거시 자유 문자열도 그대로 라우팅(uuid 아니어도 exact-match). 태그 셀렉터는 신규 경로로 추가.

## 6. 비범위(지금)

- 분산 합의·gossip 로스터(멀티 브로커) = 개인 규모엔 과함.
- 능력 best-fit 스코어링(브로커가 "가장 적합" 자동 선택) = (b) 이후.
- 인증·권한(누가 어떤 태그로 등록 가능한가) = 신뢰 네트워크 전제(후속).

## 7. 한 줄 결론

**어드레싱을 "사람이 맞추는 문자열"에서 "UUID(라우팅) + 태그(발견)"로 옮긴다.** UUID가 충돌을 없애고, 태그가 불투명성을 없앤다(능력으로 발견). 네이밍 규약은 태그로, no-consumer는 발견으로 흡수된다. 이게 협력업체 비전(온디맨드 소환)의 어드레싱 토대다.
