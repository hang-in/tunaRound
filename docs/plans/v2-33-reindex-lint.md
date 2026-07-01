# Plan v2-33: reindex/lint 명령 (로드맵 step 8)

> 모델 교체(step 2)·스키마 변경·토크나이저 변경 후 검색 인덱스 복구.

## 설계

messages가 SoR. messages_fts·message_vectors는 파생. reindex = 파생을 SoR에서 재생성.

- `SqliteStore::list_sessions() -> Vec<String>`(sessions.id 전부).
- `SqliteStore::index_stats() -> (sessions, messages, fts, vectors, validity)` 카운트(lint 리포트).
- **`--reindex` 서브 모드**(main, sqlite): --db 필수. 모든 세션 load_session → save_session(현재 fts 토크나이저로 FTS 재구성) → index_vectors(semantic이면 재임베딩; step 2 model_id 키로 모델 교체 시 갱신). 전후 stats 출력.

## 테스트

- list_sessions/index_stats 라운드트립.
- reindex: 토크나이저 바꿔 재색인하면 FTS가 새 토큰으로 검색됨(또는 최소한 세션 수·재색인 카운트).

## 범위

store/sqlite.rs(list_sessions/index_stats), main.rs(--reindex). 읽기 위주 유지보수, 기존 동작 불변.
