# Plan v2-27: Stage 3d post_turn(쓰기 권위) + get_roster

> (A) 코어-백엔드 Stage 3d. docs/design/v2-A2A-core-backend_2026-06-30.md, Plan 25 3d "원격 프론트/에이전트가 코어에 턴을 씀. 분산 쓰기 권위".
> 3a-3(front=core 단일 프로세스)까지 = 원격이 코어에서 **읽기**(read_transcript/search_context)는 됨. 3d = 원격이 코어에 **쓰기**(post_turn) + 로스터 조회(get_roster).

## 착수 전 발견(중요): persist가 전량 교체라 단순 append는 덮어쓰인다

- `MessageIndexer::persist`(REPL이 매 라운드 호출)는 `SqliteStore::save_session` = **`DELETE FROM messages WHERE session_id` 후 전체 트리 재삽입**(전량 교체 의미론).
- 따라서 원격이 post_turn으로 DB에 턴을 append해도, `--core`(REPL 존재) 모드에선 **다음 REPL persist가 그 append를 덮어쓴다**. REPL 인메모리 트리가 권위.
- 즉 post_turn은 "어느 토폴로지에서 누가 쓰기 권위를 갖는가"를 먼저 정해야 일관적이다. 이게 3d의 진짜 결정.

## get_roster (읽기, 저위험) - 합의 가능

- MCP 툴 `get_roster(session_id?)` → 토론 참가자 목록(engine, role) 반환. 원격 에이전트/프론트가 좌석 구성을 발견.
- 배선: 서버가 로스터 스냅샷(Vec<{engine, role}>)을 보유. `--core`는 participants에서, `--serve-mcp` 헤드리스는 `--roster` 파일에서(없으면 "로스터 미연결").
- 주의: `--core` 분기가 현재 participants 빌드 **이전**에 서버를 spawn → 로스터 전달하려면 (a) 서버 spawn을 participants 빌드 후로 이동, 또는 (b) `Arc<Mutex<Option<Roster>>>` 핸들을 나중에 채움. (a)가 단순.

## post_turn (쓰기) - 일관성 모델 결정 필요(아래 옵션)

세 토폴로지 옵션. 공통 부품: `TranscriptWriter` 트레잇 + `SqliteStore::append_turn(session_id, speaker, content)`(head 자식으로 새 id 삽입, head 갱신). 차이는 **REPL 권위와의 조정**.

- **옵션 A - 헤드리스 코어 한정**: `--serve-mcp` 헤드리스(REPL 없음)에서만 post_turn 허용 = 코어가 단일 writer, 클로버 없음. `--core`에선 post_turn 거부("REPL이 권위"). 작고 일관적. 단 헤드리스 코어엔 아직 턴을 **구동**할 프론트가 없어 소비자 미완(얇은 원격 프론트가 생겨야 의미).
- **옵션 B - front=core REPL 병합**: post_turn이 코어 트리에 append, REPL이 라운드마다 DB 재로드 후 인메모리 트리에 외부 턴 병합 → persist는 합집합. 방금 만든 3a-3 front=core에 **원격 에이전트가 라이브로 턴 주입** 가능(가장 유용). 비용: id 권위 일원화(append id 충돌 방지) + 병합 로직 = 중간 규모, 동시성 주의.
- **옵션 C - 얇은 원격 프론트까지**: post_turn/get_roster + 원격 1좌석 프론트 모드(`--connect <url>`: 자기 좌석만 구동, post_turn으로 코어에 씀). 소비자까지 완성 = 분산 토론 실증. 가장 크고 완결적(3e 인접).

## 결정: 옵션 B (front=core 병합) - 사용자 확정 2026-07-01

### 정합 메커니즘: DB를 id 권위로 일원화 (공유 Mutex 불필요)

- **`SqliteStore::append_turn(session_id, speaker, content, fts_tok) -> u64`**: 단일 트랜잭션 증분 INSERT. `new_id = max(msg_id)+1`, `parent_id = sessions.head_id`, message+FTS 삽입, `sessions.head_id = new_id` 갱신, COMMIT. 전량 교체 아님(클로버 없음). SQLite 쓰기 직렬화(WAL 단일 writer)라 동시 append 안전.
- **REPL core-sync 모드**(=`--core`처럼 코어 DB가 공유 권위일 때만): append_round가
  1. `sync_from_core()`: load_session(sid)로 DB 재로드 → DB에 있고 self.messages에 없는 id(외부 post_turn)를 인메모리 트리에 병합, head를 DB head로.
  2. 각 에이전트 발언을 `append_turn`으로 DB에 씀 → DB가 부여한 id를 self.messages에 반영(head 갱신).
  3. 전량 persist 생략(append_turn이 이미 씀).
  → REPL과 post_turn이 **같은 DB id 권위**를 공유하므로 id 충돌·클로버 구조적으로 없음. 별 연결이어도 SQLite 트랜잭션이 직렬화.
- **비-core-sync(기본 --db, 또는 --core 아님)**: 기존 full-replace persist 그대로 = **동작 불변**.
- 벡터 색인: append_turn은 FTS만(검색 필수). semantic 벡터는 후속(opt-in 피처, core-sync에선 지연).

### 슬라이스

1. `TranscriptWriter` 트레잇 + `SqliteStore::append_turn` + `SqliteTranscriptWriter`. 단위테스트(append→load_session에 head 자식으로 보임).
2. MCP `post_turn`(writer) + `get_roster`(로스터 스냅샷) 툴 + 서버 배선.
3. REPL core-sync(reader+writer 옵션) + append_round 병합/증분 경로 + 비활성 시 기존 동작. 단위테스트(외부 append가 다음 라운드 prior에 포함, REPL 턴이 안 덮음).
4. main `--core`: writer+로스터 스냅샷 서버 주입 + REPL core-sync 동일 DB 배선. 서버 spawn을 participants 빌드 후로 이동(로스터 전달).
5. 라이브 e2e: 원격(별 REPL/`--connect` 또는 직접 MCP)에서 post_turn → front=core REPL 다음 라운드가 그 턴을 prior로 인용.

옵션 A(헤드리스 한정)는 소비자 부재로 speculative, 옵션 C(얇은 프론트)는 한 슬라이스엔 큼 → B 채택.

## 테스트(공통)

- get_roster: 서버에 로스터 주입 후 MCP 호출이 좌석 목록 반환(단위). 미연결 시 안내.
- post_turn: append_turn 후 read_transcript가 새 턴 포함(단위). 옵션 B면 REPL 병합 단위테스트(외부 append → 다음 라운드 prior에 포함, persist가 안 덮음).
- 라이브: 옵션에 따라.

## 범위 밖

3c Tailscale(ops) · codex bearer-env · 3e 영속 에이전트.
