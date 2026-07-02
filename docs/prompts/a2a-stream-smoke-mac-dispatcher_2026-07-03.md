# A2A 크로스머신 스트리밍 스모크: 맥 = 원격 dispatcher (2026-07-03, 세션8)

> 맥 Claude 세션용. A2A 스트리밍 Phase 2(SSE)의 크로스머신 검증. **맥은 원격 dispatcher**로 Windows 코어에 `SendStreamingMessage`를 SSE로 LAN 너머로 열어, task 생명주기를 실시간 스트림으로 관찰한다. 새 경로 = **SSE-over-LAN 전달**. 설계 정본 docs/design/v2-a2a-streaming_2026-07-03.md.

## 전제
- Windows가 코어 호스팅: `serve 0.0.0.0:8770`, LAN IP **192.0.2.10**. 같은 LAN.
- bearer 토큰 = **동구님이 채팅으로 전달**(레포 미커밋). 아래 `<TOKEN>`에 대입.
- 맥은 **MCP 등록 불요** - raw curl로 SSE를 직접 연다(대화형 도구 승인 불필요, 세션 재시작 불필요).
- agent-card로 스트리밍 지원 사전 확인(선택): `curl -s -H "Authorization: Bearer <TOKEN>" http://192.0.2.10:8770/.well-known/agent-card.json` -> `"streaming":true` 기대.

## 절차

### 1) SSE 스트림 열기 (백그라운드, curl -N 필수, 파일로 캡처)
```bash
curl -N -s --max-time 120 \
  -H "Authorization: Bearer <TOKEN>" \
  -H "Content-Type: application/json" \
  -H "Accept: text/event-stream" \
  -X POST http://192.0.2.10:8770/a2a \
  -d '{"jsonrpc":"2.0","id":"mac-s1","method":"SendStreamingMessage","params":{"message":{"messageId":"macm1","role":"user","parts":[{"text":"크로스머신 스트리밍 스모크: 이 task는 win-claude가 처리"}]},"fromAgent":"mac-claude","toAgent":"win-claude"}}' \
  > /tmp/mac-sse.out 2>&1 &
```
- `-N`(no-buffer) 필수. `--max-time 120`은 사람 릴레이 지연 대비 넉넉히(타임아웃 나면 재실행).

### 2) 첫 프레임 확인 + 보고
- 2~3초 후 `cat /tmp/mac-sse.out`. 첫 `data:` 프레임 = `result.task`(state=submitted). = **SSE-over-LAN 개방 성공.**
- 이때 task는 win-claude 앞으로 생성됨. **윈도우가 poll_tasks로 알아서 찾으므로 task_id를 굳이 relay 안 해도 됨**(원하면 보고). "SSE 열렸고 submitted 프레임 받았다"만 동구님께 알리면 윈도우가 worker로 처리 시작.

### 3) 윈도우 worker 처리 후 실시간 프레임 관찰
- 윈도우(win-claude)가 그 task를 MCP claim -> complete 하면, `/tmp/mac-sse.out`에 다음이 **실시간 추가**된다:
  - `statusUpdate`(state=working, final:false)
  - `artifactUpdate`(결과 artifact, lastChunk:true)
  - `statusUpdate`(state=completed, **final:true**) -> 스트림 종료.
- 최종 `cat /tmp/mac-sse.out` 전체를 보고.

## 성공 기준
맥이 LAN 너머 SSE 하나로 **submitted -> working -> artifact -> completed(final)** 전체 시퀀스를 실시간 수신하고 final 후 스트림이 닫힌다. = 크로스머신 스트리밍 스모크 성공. (사람은 "SSE 열었다/받았다" 신호만 relay, 작업 데이터는 SSE가 나름.)

## 다음(성공 시)
이기종 파트너(Codex-on-Ollama를 worker로) = Phase 2 파트너 확장. Agent Card skills 광고 -> best-fit 선택. 별도 세션.
