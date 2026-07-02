# 크로스머신 A2A 스모크 핸드오프 (맥 .184 → 윈도우 코어 .179)

> **임시 문서**(스모크 검증용, 완료 후 삭제 예정). LAN 사설 IP + throwaway 토큰만 사용. 목적: tunaRound half-A2A가 실제 두 머신(같은 공유기 LAN)에서 되는지 1회 검증. 지금까지 검증은 loopback까지였음.

## 구도

- **윈도우(192.0.2.10)** = 코어 호스트. `serve 0.0.0.0:8770`, 토큰 `TUNA_SMOKE`, DB 시드 = session "default"에 발언 1턴(코드명 **ALBATROSS**, 이 코드명은 윈도우 코어 전사에만 존재).
- **맥(192.0.2.11)** = 이 문서 실행 주체. 코어에 붙어 그 전사를 읽어 ALBATROSS를 인용하면 크로스머신 A2A 읽기 실증.
- 윈도우 방화벽 인바운드 8770(Private) 허용 규칙 추가됨.

## 전제: 윈도우 코어가 떠 있어야 함

이 핸드오프 작성 시점엔 윈도우에서 코어가 background로 떠 있음. 만약 내려갔으면(맥 스텝에서 연결 안 되면) **윈도우에서** 재기동:
```bash
# 윈도우 레포에서
./target/debug/tunaround.exe serve 0.0.0.0:8770 --db target/e2e/shared.db --token TUNA_SMOKE
# (시드가 없으면: cargo run --features "morphology sqlite" --example seed_smoke -- target/e2e/shared.db 먼저.
#  단 seed_smoke.rs는 스모크용 임시 파일이라 이미 지웠을 수 있음. 그 경우 아무 db로 빈 코어를 띄우고
#  아래 2단계 대신 curl 도달/bearl(401/200)까지만 확인해도 네트워크 레그는 증명됨.)
```

## 맥에서 실행

### 1단계: 도달 + bearer (빌드 불필요, curl)
```bash
curl -s -o /dev/null -w "no-token: %{http_code}\n" --max-time 8 http://192.0.2.10:8770/mcp

curl -s -o /dev/null -w "with-token: %{http_code}\n" --max-time 8 \
  -H "Authorization: Bearer TUNA_SMOKE" \
  -H "Content-Type: application/json" \
  -H "Accept: application/json, text/event-stream" \
  -X POST http://192.0.2.10:8770/mcp \
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"mac-smoke","version":"0"}}}'
```
- 기대: `no-token: 401` + `with-token: 200` = 크로스머신 네트워크 + 코어 + bearer 통과.
- `000`/timeout/refused = 방화벽·네트워크 문제(윈도우 쪽 조치 필요).

### 2단계: 진짜 A2A 읽기 (맥 tunaround)
맥 레포에서(먼저 `git pull` + `cargo build`; Kiwi 자동다운로드 실패해도 lindera 폴백이라 무관):
```bash
cargo run -- join http://192.0.2.10:8770/mcp --token TUNA_SMOKE
```
REPL 프롬프트에:
```
전사를 read_transcript 도구로 확인하고, 이전 발언의 코드명을 정확히 인용해줘.
```
- 기대: 맥 아키텍트(claude)가 **ALBATROSS**를 인용 → **크로스머신 A2A 읽기 실증**. codex 좌석도 pull 가능(behavioral read-only).
- `/quit`로 종료.

## 결과 판정

- 1단계 401/200 + 2단계 ALBATROSS 인용 = **half-A2A 크로스머신 실증 완료.** checklist "분산 라이브 스모크" 체크, context-notes 기록.
- 실패 지점(연결/인증/인용) 기록해서 윈도우 세션에 공유.

## 정리(스모크 후)

- 윈도우: 코어 프로세스 종료(`taskkill //F //IM tunaround.exe`), `target/e2e/` 삭제.
- 이 문서 + `examples/seed_smoke.rs`(있으면) `git rm`로 제거(임시 검증물).
