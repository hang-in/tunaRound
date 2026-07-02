# A2A 도그푸딩: 맥 = worker (2026-07-03, semi-a2a Phase 1 Task 5)

> 맥 Claude 세션용 핸드오프. semi-a2a 파트너 위임 Phase 1의 라이브 크로스머신 도그푸딩. 맥은 **worker**(윈도우 boss가 던진 task를 받아 수행·완료). 설계·레시피: docs/design/v2-a2a-partner-delegation_2026-07-02.md §12.

## 전제
- 윈도우가 **코어를 호스팅**: `serve 0.0.0.0:8770`, LAN IP **192.0.2.10**. 맥·윈도우 같은 LAN.
- 코어 URL(MCP): `http://192.0.2.10:8770/mcp`. bearer 토큰은 **동구님이 채팅으로 전달**(레포 미커밋).
- 윈도우가 코어를 켜둔 상태여야 함(툴이 안 붙으면 동구님/윈도우에 코어 기동 여부 확인).

## 셋업 (맥)
1. `git pull`. 최신 Phase 1 A2A 코드(커밋 b1ba880 이상 = `src/a2a_server.rs` + `src/mcp.rs`의 A2A 툴·`src/store/a2a.rs`) 포함 확인.
2. 빌드: `cargo build --features "semantic morphology mcp serve"`. (맥 aarch64에서 검증된 조합. kiwi 자산 없어도 lindera 폴백 정상.)
3. 코어를 이 레포용 MCP 서버로 등록(Claude Code):
   ```
   claude mcp add --transport http tuna-core http://192.0.2.10:8770/mcp --header "Authorization: Bearer <토큰>"
   ```
   -> **등록 후 이 레포에서 새 Claude Code 세션을 시작**해야 MCP 서버가 로드됨(세션 중 추가는 재시작 필요).
4. 새 worker 세션에서 툴 노출 확인: `poll_tasks`/`claim_task`/`complete_task`(+`send_task`/`get_task`)가 보여야 함. 안 보이면 등록/재시작/코어 도달(위 전제) 재확인.

## worker 실행 (agent id = `mac-claude`)
**수동 1회전(먼저 이걸로 손맛 확인 권장)**:
- `poll_tasks agent=mac-claude` -> 열린 task가 나오면 그 task_id로:
- `claim_task task_id=<id>`(착수 표시) -> 지시(msg 본문) 수행 -> `complete_task task_id=<id> result="<결과 요약>"`.

**연속 폴링(선택)**:
```
/loop 30s "poll_tasks agent=mac-claude. 열린 task가 있으면 각각 claim_task로 착수하고 지시대로 수행한 뒤 complete_task에 결과 요약을 넣어라. 없으면 다음 폴링까지 대기."
```

## 보고
- **complete_task에 결과를 넣으면 끝.** 윈도우 boss가 `get_task`로 그 결과(artifact)를 확인함. 별도 채팅 보고 불필요(task가 채널).
- 문제(툴 미노출·코어 미도달·에러)면 그 내용을 동구님에게. 코어 재시작/토큰/방화벽은 윈도우 쪽 담당.

## 참고
- 대화형이라 사람(맥)이 MCP 도구 호출을 승인 -> codex #24135 무관.
- 이건 semi-a2a(HITL) 도그푸딩. 첫 task는 윈도우가 가벼운 것(예: "이 파일 요약" 또는 "특정 함수 리뷰")을 던질 예정.
- 성공 기준: 윈도우 send_task -> 맥 poll/claim/수행/complete -> 윈도우 get_task로 결과 확인, 이 왕복 1회.
