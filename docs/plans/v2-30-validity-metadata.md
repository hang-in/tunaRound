# Plan v2-30: 유효성 메타데이터 (로드맵 step 4)

> docs/design/v2-temporal-validity-direction_2026-07-01.md step 4. 시간성·유효성 흡수 시작.

## 설계 판단: 별도 테이블 (StoredMessage 불변)

`messages`/`StoredMessage`에 컬럼을 추가하면 모든 struct 리터럴이 깨지는 큰 blast radius + JSON/Redis 직렬화 하위호환 문제. 또 Memora 철학(원문 memory value와 abstraction/anchors 분리)에 어긋난다. → **별도 `message_validity` 테이블**로 원문은 순수하게 두고 유효성/요약/앵커를 레이어링한다.

## 스키마 (v3→v4)

```
CREATE TABLE message_validity (
    session_id           TEXT NOT NULL,
    msg_id               INTEGER NOT NULL,
    valid_state          TEXT NOT NULL DEFAULT 'active', -- active|superseded|rejected|stale|unknown
    superseded_by_msg_id INTEGER,
    abstraction          TEXT,   -- 결정 요약(Memora primary abstraction)
    anchors              TEXT,   -- 검색 단서(모듈·에러·쟁점; JSON array or newline)
    updated_at           TEXT NOT NULL DEFAULT (datetime('now')),
    PRIMARY KEY(session_id, msg_id)
);
```
새 TABLE이라 migrate에서 CREATE IF NOT EXISTS로 fresh·기존 DB 모두 처리(ALTER 불요).

## step 4 범위 = 데이터 레이어

- `store::Validity { valid_state, superseded_by: Option<u64>, abstraction: Option<String>, anchors: Option<String> }`.
- `SqliteStore::set_validity(sid, msg_id, valid_state, superseded_by)`: upsert. abstraction/anchors 보존(ON CONFLICT는 valid_state/superseded만 갱신).
- `SqliteStore::set_annotation(sid, msg_id, abstraction, anchors)`: upsert. valid_state 보존.
- `SqliteStore::get_validity(sid, msg_id) -> Option<Validity>`.
- 라운드트립 단위테스트.

## 후속 (step 5에서)

- 검색 랭킹이 message_validity를 LEFT JOIN해 non-active(superseded/rejected/stale) 디프리오리티.
- REPL 커맨드(`/supersede <id> [<by_id>]`, `/reject <id>`)로 사람이 유효성 지정 → set_validity 배선.
- abstraction/anchors 생성 파이프라인(에이전트 요약)은 더 뒤(컬럼만 준비).

## 범위

store/mod.rs(Validity), store/sqlite.rs(스키마+set/get). StoredMessage·직렬화 불변. 기존 동작 불변(테이블 미사용 시).
