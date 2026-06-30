# Plan v2-21: 검색 품질 측정 + FTS 리콜 개선 (Stage 0)

> (A) 코어-백엔드 설계 Stage 0의 첫 슬라이스. docs/design/v2-A2A-core-backend_2026-06-30.md.
> 측정-우선 규율(#10). 백엔드(Ollama 터널) 불필요한 **FTS(어휘) 결정적 측정·개선**에 집중. 벡터/하이브리드는 터널 의존이라 분리(후속).

## 목표

현 검색 게이지(`tests/search_quality.rs`)는 합성 6발언 + 사람 눈 판정이라 (1) 현실성 부족 (2) 객관 수치 부재. 이를 **현실 코퍼스 + 자동 메트릭(recall@k / MRR)**으로 바꿔, FTS 리콜의 baseline을 수치화하고 약점을 개선한다.

## 확증된 1순위 약점 가설 (코드 확인)

`tokenizer.rs:33-39 fts_query`가 질의 토큰을 공백조인 → `sqlite.rs:283 MATCH ?1`이 **암묵적 AND**. 다중어 질의는 전 토큰 동시 출현을 강제해 리콜이 깎인다(긴 질의일수록 0건 위험). bm25가 이미 랭킹하므로 표준 해법 = **OR + bm25 랭킹**(관련 토큰 많은 문서가 상위). 단 측정으로 확증 후 적용.

## Task 1: 현실 코퍼스 + 자동 메트릭 하네스 (Sonnet 위임)

신규 `tests/search_recall.rs`. **`morphology` 피처만**으로 동작(`semantic` 불요 = 터널 없이 결정적). 기존 `search_quality.rs`(semantic 게이트, 눈 판정)는 보존.

### 코퍼스 (이 스펙 그대로 사용, 임의 변경 금지)

실제 tunaRound식 한국어 설계 토론 전사. 굴절·외래어·동의어를 의도적으로 흩어 리콜을 시험한다. `StoredMessage{ id, parent_id, speaker, content }`, parent_id = id-1(루트 1=None), head = 마지막 id.

```
1  claude  인증 방식을 정하자. 세션 쿠키 대신 토큰 기반으로 가는 게 확장에 유리하다.
2  codex   동의한다. 액세스 토큰은 짧게, 리프레시 토큰으로 갱신하는 구조가 안전하다.
3  claude  토큰을 어디 보관하지? 로컬스토리지는 XSS에 취약하니 httpOnly 쿠키를 쓰자.
4  codex   로그인 흐름은 OAuth 위임으로 가면 비밀번호를 직접 안 다뤄도 된다.
5  claude  검색은 형태소 분석으로 색인하고 FTS5 전문검색으로 질의하자.
6  codex   외래어가 문제다. 임베딩 같은 단어가 형태소 분석에서 음절로 쪼개진다.
7  claude  그러면 raw 토큰을 같이 색인해서 외래어를 원형으로 살리자.
8  codex   의미 검색이 필요하면 벡터 임베딩을 붙여 하이브리드로 융합하면 된다.
9  claude  맥락이 길어지면 매 턴 전체를 다시 넣는 건 토큰 낭비다.
10 codex   최근 몇 턴만 유지하고 나머지는 검색으로 필요할 때 끌어오자.
11 claude  요약을 만들어 라운드 사이에 이월하면 재전송을 더 줄일 수 있다.
12 codex   여러 프로세스가 한 토론을 공유하려면 Redis로 상태를 미러링하자.
13 claude  스냅샷과 이벤트 스트림을 함께 두면 끊겨도 재개가 된다.
14 codex   참가자가 늘면 좌석마다 역할을 주입해서 발언을 시켜야 한다.
15 claude  로컬 LLM도 좌석으로 받자. ollama나 lmstudio를 HTTP로 부르면 된다.
16 codex   코드 작성은 한 명에게만 쓰기 권한을 주고 나머지는 읽기만 하자.
17 claude  결론이 나면 문서로 자동 기록해서 구현 단계로 넘기자.
18 codex   테스트와 빌드 검증은 커밋과 분리해서 단계로 두는 게 안전하다.
19 claude  배포는 점진적으로. 한 번에 다 바꾸면 되돌리기 어렵다.
20 codex   관측을 위해 다른 프로세스가 토론을 구경만 하는 모드도 필요하다.
```

### 질의 + gold (관련 msg_id 집합)

종류별로 정확/굴절/외래어/동의어/다중어를 섞는다. gold = 사람 판정 관련 발언.

```
Q1  "토큰"                      gold {1,2,3}          정확
Q2  "인증을"                    gold {1}              굴절(조사)
Q3  "임베딩"                    gold {6,8}            외래어 정확
Q4  "외래어 색인"               gold {6,7}            다중어(AND면 깨질 후보)
Q5  "로그인 방식"               gold {1,4}            동의어(인증/OAuth)
Q6  "재주입을 줄이는 방법"      gold {9,10,11}        다중어+굴절
Q7  "프로세스 공유 재개"        gold {12,13}          다중어(AND면 0건 후보)
Q8  "로컬 LLM 좌석"             gold {15}             외래어+다중어
Q9  "쓰기 권한"                 gold {16}             정확
Q10 "결론 문서 기록"            gold {17}             다중어
```

### 메트릭

- `recall@k = |retrieved_topk ∩ gold| / |gold|`, k=3·5.
- `MRR = 1/rank(첫 gold)`, 없으면 0.
- 질의 전체 평균(mean recall@3, recall@5, MRR) 출력.
- **FTS 단독(결정적)**: `store.search(tok.fts_query(q), k)`로 측정. `--nocapture`로 질의별 표 + 집계 출력.
- 회귀 가드 테스트 1개: mean recall@5 ≥ baseline(측정 후 수치 확정, 처음엔 출력만 하고 assert는 Task 2에서 박는다).

위임 범위 = 코퍼스 함수 + 질의/gold 테이블 + 메트릭 계산 + 출력. **로직만, 코퍼스/질의/gold 텍스트는 위 스펙 그대로.**

## Task 2: baseline 측정 + 약점 식별 (Opus) — done 2026-06-30

baseline(lindera 경로, 결정적): **mean recall@3=0.483 / recall@5=0.550 / MRR=0.600** (10질의 중 4개 0건).

**AND가 단일 지배 원인 - 확증:**
- Q7 "프로세스 공유 재개" 0건(3토큰 동시출현 발언 없음). Q4 "외래어 색인" id6 탈락(색인 없음).
- Q2 "인증을" 0건 = `인증*`(형태소) AND `인증을*`(raw)의 **자기충돌**: raw 굴절형이 색인 어디에도 안 맞아 전체 0건. → OR이면 `인증*`이 id1 매치로 구제.
- Q6 "재주입을 줄이는 방법" 0건(다중어 AND + 복합어 미스).
- **결론: OR+bm25로 Q2/Q4/Q6/Q7 대부분 구제 가능.** bm25가 다토큰 매치를 상위로 → 정밀도 보존.

**회귀 가드:** baseline R@5=0.55를 floor로(Task 3 전). Task 3 개선 후 새 수치로 상향.
**주의:** 하네스가 production 메인 kiwi 아닌 lindera로 측정(결정성). AND 문제는 토크나이저 무관이라 결론 불변. kiwi 스팟체크 후속.

## Task 3: FTS 리콜 개선 (Sonnet, Opus 리뷰) — done 2026-06-30 (미커밋)

`fts_query` 마지막 join 공백(AND) → " OR ". 단일 토큰은 불변. 변경 1곳, grep상 fts_query 출력 포맷을 직접 비교하는 코드 없음(호출부 모두 MATCH 파라미터).

**결과:** mean recall@3 0.483→**0.833**, recall@5 0.550→**0.900**, MRR 0.600→**0.900**. Q2/Q4/Q7 구제. 회귀 가드 `mean_r5 >= 0.89`. 검증: 기본/morphology 테스트 통과, clippy 클린.

**리뷰 지적(Opus):**
- **정밀도 미측정 = OR의 유일 리스크.** 하네스는 recall-only. OR이 top-k에 비-gold 혼입(Q4 id5, Q7 id20, Q8 id14/3). gold는 1순위(MRR 0.9)라 noise는 하위. RAG K=5 주입엔 수용 가능하나 recall만 보면 과대평가. → **후속: precision@k 추가해 정직한 게이지화.**
- **천장 정정:** Q5는 어휘 직접공유라 OR로 풀림(동의어 아님). **진짜 천장 = Q6**("재주입" 질의 vs "재전송" 코퍼스 = 순수 어휘 공백) = 벡터/동의어 YAGNI 해소 근거점.
- OR은 production 기본 동작 변경(RAG/`/search`/MCP). 북극성이 리콜이고 bm25가 gold를 1순위 유지하므로 기본값으로 타당.

## 후속 (이 슬라이스에서 식별)
- precision@k를 search_recall 하네스에 추가(OR noise 정량화). 그 뒤 retriever K 재검토.
- Q6류 어휘 공백 → 벡터/하이브리드 측정(터널 의존, 별 슬라이스). YAGNI 해소 근거 확보됨.
- kiwi 경로 스팟체크(production 메인).

## Task 3: FTS 리콜 개선 (Sonnet 위임, Opus 리뷰)

측정이 가리킨 최우선 약점 적용. 1순위 후보 = `fts_query` AND→OR(bm25 랭킹 유지). 변경은 **opt-in/behavior-preserving 우선** 고려(기존 단일어 테스트 불변 확인). 재측정으로 recall 델타 입증, 회귀 가드 갱신.

## 비포함(후속)

- 벡터/하이브리드 품질(터널 의존) = 별 슬라이스.
- 요약 carry-forward(Stage 0 항목2) = 별 plan.
- 쿼리 확장/동의어 사전 = 측정이 어휘만으론 부족함을 입증하면.
