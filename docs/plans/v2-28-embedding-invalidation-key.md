# Plan v2-28: 임베딩 무효화 키에 model_id 포함 (시간성·유효성 로드맵 step 2)

> docs/design/v2-temporal-validity-direction_2026-07-01.md step 2. 기반 안정화(실버그 수정).

## 문제

`index_vectors`의 증분 가드가 `content_hash(&msg.content)`(내용만)로 skip 판단한다. 임베딩 **모델을 바꿔도**(bge-m3 → 다른 모델) content_hash가 같아 **stale 벡터를 skip** → 차원/공간이 섞인 벡터로 cosine. dim mismatch가 아니면 조용한 품질 저하. 무효화 키에 모델 정체성이 빠져 있다.

## 설계

- **Embedder 트레잇에 `fn model_id(&self) -> String` 추가**: 모델 정체성(provider:model). MockEmbedder=`mock-{dim}`, OllamaEmbedder=`ollama:{model}`.
- **message_vectors에 `model_id TEXT` 컬럼**: 스키마 v2→v3. CREATE_MESSAGE_VECTORS에 컬럼 추가(fresh DB) + migrate에서 기존 DB는 `ALTER TABLE ADD COLUMN`(column_exists 가드). 기존 행은 model_id NULL → 다음 색인 때 재임베딩 트리거(자동 복구).
- **skip 로직**: 기존 행의 (content_hash, model_id)를 읽어 **content_hash 동일 AND model_id == 현재 모델**일 때만 skip. 둘 중 하나라도 다르면 재임베딩+upsert. upsert 시 model_id 저장.
- dim은 model_id가 함의하므로 별도 체크 불요(모델 바뀌면 model_id로 이미 재색인).

## 테스트

- Embedder::model_id 단위(mock/ollama 표기).
- index_vectors: 같은 모델 재색인=skip(임베드 호출 0), 모델 바꾸면(model_id 다름) 재임베딩(벡터 갱신). 카운팅 임베더 또는 dim 다른 두 MockEmbedder.
- 마이그레이션: v2 스키마(model_id 없음) DB 열면 ALTER로 컬럼 추가, 기존 데이터 보존.

## 범위

store/embedding.rs(트레잇+2구현), store/sqlite.rs(스키마/migrate/index_vectors). 동작: 모델 동일 시 기존과 같음(behavior-preserving), 모델 변경 시에만 재색인.
