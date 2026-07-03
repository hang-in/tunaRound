---
title: tunaRound 핸드오프 - 2026-07-02~03 맥 왕복 (rc.1 릴리스 + 티키타카 셋업)
type: prompt
status: active
priority: P0
updated_at: 2026-07-03
owner: shared
summary: 맥 왕복 세션. 맥 전체 검증(빌드·테스트·설치·도그푸딩) + v0.1.0-rc.1 프리릴리스 발행(CI 3버그 수리) + 크로스머신 티키타카 코어 셋업(종료됨). ⚠️ Cargo.toml=0.1.0-rc.1(최종 전 되돌림). A2A worker 왕복(Phase 1 Task 5) ✅완료(⑦). 다음=최종 v0.1.0 또는 real workload 왕복.
---

# 맥 왕복 핸드오프 (2026-07-02~03)

> 이전: [session5](v2-handoff_2026-07-02_session5.md)(윈도우) + 그 후반(c59be32). 이 세션 = 맥에서 릴리스 검증·발행. 콜드 스타트 가정, 이 문서 + `checklist.md` + `context-notes.md`(하단) + [dev-mac-windows](../reference/dev-mac-windows.md).

## ⚠️ 즉시 주의 (2건)

1. **Cargo.toml `version = "0.1.0-rc.1"`.** rc용으로 올렸음(cargo-dist 태그=버전 요건). **최종 v0.1.0 태그 전 `version = "0.1.0"`으로 되돌리고** `cargo build`로 lock 동기 후 `git tag v0.1.0`.
2. **맥 티키타카 코어는 종료됨**(serve 프로세스 killed). 재기동법은 ⑤.

## ① 이 세션이 한 것

- **맥 전체 검증(맥 aarch64, 크로스플랫폼 이슈 0):** `cargo build`(기본/풀피처) · `cargo test`(기본 195 / 풀피처 212) · clippy · `cargo install --features "semantic mcp serve"`(release → `~/.cargo/bin/tunaround`) · **E2E 도그푸딩**(chat로 "v0.1.0 릴리스 준비" 토론, 결과문서 산출) · 미설치 CLI graceful(패닉X). Kiwi 자산404→lindera 폴백 정상, semantic 터널없음→FTS 폴백.
- **크로스머신 A2A 스모크(맥→윈도우 코어):** 네트워크 401/200 ✅, claude가 원격 전사 ALBATROSS 인용 ✅(데이터평면 실증). codex leg 실패(pull 취약).
- **v0.1.0-rc.1 프리릴리스 발행:** 도그푸딩 판정대로 rc 먼저. rc.1이 **CI 전용 버그 3개**를 잡음(전부 로컬 미검출) → 수리 후 CI green + 프리릴리스 생성(4타깃+인스톨러+tunaround.rb). **자세한 릴리스 교훈은 [dev-mac-windows §6](../reference/dev-mac-windows.md)** 에 영속.
  1. cargo-dist 태그버전=Cargo.toml버전 요구 → `0.1.0-rc.1`.
  2. `[profile.dist]` 필수 → Cargo.toml에 추가(`inherits="release" lto="thin"`).
  3. aarch64 ring 크로스컴파일 실패(`/imsvc`) → **arm64-win/linux 제외, 4타깃**(mac arm64·x64, win x64, linux x64).
  - prerelease는 homebrew publish 자동 skip(tap 불요). `gh run watch --exit-status` exit code 신뢰불가 → 잡 결론 직접 확인.
- **실패 CI 런 3개 삭제**(성공 1개 유지). **티키타카 코어 셋업**(맥 serve, 아래 ⑤).
- 문서: `docs/reference/release-readiness-v0.1.0_2026-07-02.md`(도그푸딩+검증+체크리스트), dev-mac-windows §6 교훈, README/CLAUDE.md/CHANGELOG 정정.

## ② 현재 상태

- **HEAD = `ae2fc71`**(pull 완료). 윈도우가 이 세션 중 **A2A dispatcher/inbox Phase 1** 추가: `src/a2a_server.rs`, `src/store/a2a.rs`, MCP 툴 `send_task/get_task`(dispatcher) + `poll/claim/complete`(inbox). **자율 제어평면(AutoLoop) Phase 1 착수.**
- **프리릴리스 `v0.1.0-rc.1` live**: https://github.com/hang-in/tunaRound/releases/tag/v0.1.0-rc.1 (isPrerelease=true, 15 assets).
- 검증치(참고, rc 커밋 기준): 기본 ~195 / 풀피처 ~212 pass. 최신 커밋(A2A Phase1) 후 `cargo test`로 재확인 필요.

## ③ 다음 (택1, 사용자 지정)

- ✅ **A) 윈도우 A2A worker 핸드오프 - 완료(2026-07-03, ⑦ 참조)**: `ae2fc71`("A2A 도그푸딩 맥 worker 핸드오프") = 맥이 worker로 dispatcher/inbox를 실사용해보는 도그푸딩 과제. 자율 A2A 제어평면 Phase 1 검증. win-claude(dispatcher)→mac-claude(worker) 왕복 1회 성사(Task 5).
- **B) 최종 v0.1.0 릴리스**: ⚠️①대로 Cargo.toml `0.1.0` 되돌림 → rc 아티팩트 맥/윈도우 설치검증 → `hang-in/homebrew-tap` 레포 + `HOMEBREW_TAP_TOKEN` 시크릿 → `git tag v0.1.0 && push` → 정식 Release + brew. (arm64-win/linux 크로스는 후속 zigbuild/xwin.)
- **C) 티키타카로 진행**: ⑤로 코어 재기동 + 양쪽 join.

## ④ 정직한 A2A 성숙도 (재확인)

데이터평면(공유 전사 pull/post) = 크로스머신 됨(claude). 제어평면(턴·종료) = 사람. codex 원격 pull 취약. 윈도우가 착수한 dispatcher/inbox가 자율 제어평면(AutoLoop)의 Phase 1. "semi-A2A"로 용어 정렬됨(CHANGELOG).

## ⑤ 티키타카 코어 재기동 (맥 호스팅)

```bash
# 맥(호스트): 192.0.2.11, 방화벽 disabled라 LAN 도달됨
tunaround serve 0.0.0.0:8770 --token [REDACTED_TOKEN] --db "$HOME/.tunaround/tikitaka.db" &
# 윈도우(접속):
tunaround join http://192.0.2.11:8770/mcp --token [REDACTED_TOKEN]
# 맥도 참가하려면(loopback):
tunaround join http://127.0.0.1:8770/mcp --token [REDACTED_TOKEN]
# 종료: pkill -f "serve 0.0.0.0:8770"
```
- 준비 폴링: `curl --retry 15 --retry-delay 1` (Kiwi init ~3초). no-token 401 / with-token 200 확인 후 사용.

## ⑥ 규율

`checklist.md`·`context-notes.md`(#7). cargo=Bash. 검증과 commit 분리. 한국어 마침표(#5)·새파일 첫줄 역할주석(#6)·em-dash 금지. 위임 Sonnet+Opus 리뷰. 굵직한 결정 재론 금지(claude-mem). 머신 이동 전 push/후 pull. 배포 전 도그푸딩.

## ⑦ 도그푸딩 결과: semi-a2a 크로스머신 왕복 (Phase 1 Task 5, 2026-07-03 완료)

**구도**: win-claude(dispatcher) → mac-claude(worker) 단방향 왕복. win이 `send_task`, mac이 `poll_tasks → claim_task → 수행 → complete_task`, win이 `get_task`로 state=completed + artifact 확인. task 내용 = "src/store/a2a.rs TaskState enum 배리언트 한 줄 요약"(경로 검증용 trivial). task_id=83f0e576.

**양방향 성사(2026-07-03)**: 정방향 win→mac(Task 5, `83f0e576`)에 이어 역방향 mac→win(`76ea0b9c`)도 claim→complete→get_task 검증 완료. 양쪽 leg 모두 성사(claude↔claude).

**성사된 것(짧게)**:
- 통신 계약이 실코드와 일치. submit→claim(working)→complete(completed)→verify 전 구간 무에러. TaskState 6-state가 `a2a.rs`의 enum·`as_str`와 일치.
- 크로스머신 연결 견고. 맥→윈도우 코어(:8770/mcp + Bearer) 도달·인증·폴링 정상. "claude leg"가 실제 왕복으로 재확인.

**마찰·교훈**:
1. **MCP 툴 세션시작 로드 → 워커 온보딩 2-세션 댄스(단, 회피 가능).** `claude mcp add` 등록 경로에 한정된 마찰이다("등록→종료→재시작→폴링"). win-claude가 **raw HTTP MCP**(initialize→session-id→notifications/initialized→tools/call)로 엔드포인트를 직접 두들겨 등록·재시작 없이 워커로 동작함을 실증 → 2-세션 마찰 회피됨. 대가: 대화형 도구 승인 UX가 없어 사람이 도구 승인을 못 봄(semi-a2a HITL 맥락엔 트레이드오프). 완화: 워커 진입 전 MCP 선등록 안내, join류 서브커맨드로 "등록+워커 진입" 일괄, 또는 raw HTTP 워커 레시피.
2. **"semi"의 실체 = 사람이 두 세션 사이 트리거·통보·판정 릴레이.** 통신은 진짜 A2A지만 자율 루프(AutoLoop, Stage 4) 부재 = 설계 의도대로.
3. **폴링 모델에 discovery 공백.** push/notify 채널 부재가 자율화의 실질 병목. 자율 시 30초 폴 루프의 빈 폴링 토큰 비용 트레이드오프.
4. **검증한 건 경로지 가치가 아님.** trivial task라 무거운 위임(구현·리뷰)의 결과 Artifact·타임아웃·실패 경로 미검증.
5. **codex leg 미검증(역방향은 검증됨).** 역방향(맥 dispatcher→윈도우 worker)은 `76ea0b9c`로 실증됨. codex 워커만 승인취약(#24135)으로 남아 app-server(3e) 후속.
6. **코어 리셋 복구 실증 + 조용한 task 소멸.** 윈도우가 코어를 리셋(DB 초기화)해도 같은 주소/토큰이면 클라이언트는 재등록·세션 재시작 없이 `send_task` 재디스패치로 즉시 복구됨(맥이 `907f5c82` 소멸 → `76ea0b9c` 재발송으로 실증). 단 옛 task_id는 "task 없음"으로 조용히 사라질 뿐 리셋 통지가 없어, dispatcher가 죽은 id를 계속 폴링할 위험 = #3(push/notify 부재)과 동근원.

**다음 권장(우선순위)**: (1) real workload 왕복 1회(작은 파일수정+diff 반환)로 "경로"에서 "가치"로. (2) codex leg(app-server 3e). (3) 온보딩 마찰 완화(raw HTTP 워커 레시피 문서화 or join 일괄). (4) AutoLoop(Stage 4)는 push/notify 전까지 계속 보류.

## ⑧ 크로스머신 SSE 스트리밍 스모크 (Phase 2, 2026-07-03 성공)

**구도**: 맥 = 원격 dispatcher. raw curl로 Windows 코어(`192.0.2.10:8770/a2a`)에 `SendStreamingMessage`(SSE, `-N`)를 LAN 너머로 열고 `/tmp/mac-sse.out` 캡처. task는 win-claude 앞으로 생성되고 윈도우가 MCP poll→claim→complete. 지시=`docs/prompts/a2a-stream-smoke-mac-dispatcher_2026-07-03.md`, 설계=`docs/design/v2-a2a-streaming_2026-07-03.md`.

**성공(4프레임 실시간 수신, task `53806631`)**: `task`(submitted) → (하트비트 `:` 코멘트로 ~110s 유휴 버팀) → `statusUpdate`(working, final:false) → `artifactUpdate`(lastChunk:true, 결과 텍스트) → `statusUpdate`(completed, **final:true**) → 스트림 정상 종료(curl exit 0, max-time 120 이내). 사전 확인: agent-card `streaming:true`·Bearer auth·LAN 도달.

**교훈**:
- **스트리밍이 ⑦#3(폴링 discovery 공백)의 실동작 답.** 하트비트로 긴 유휴를 버티며 상태변화를 push로 실시간 전달 → dispatcher가 빈 폴링 없이 수신. 자율화 병목이던 통지 채널을 SSE가 메움.
- **맥 워커/디스패처 모두 MCP 등록·세션 재시작 불요(raw curl).** ⑦#1의 raw HTTP 회피가 dispatcher 측에서도 성립.
- 사람 역할 = "열었다/받았다" 신호 릴레이뿐. 작업 데이터(생명주기 프레임)는 SSE가 자체 전달.

**다음(성공 시)**: 이기종 파트너(Codex-on-Ollama worker) = Phase 2 파트너 확장. Agent Card skills 광고 → best-fit 선택(별도 세션).

## ⑨ 워커 데몬 도그푸딩: R5 성공 + 발견 리팩토링 항목 (2026-07-03)

**성공(R5)**: mac-worker(`work --once --runner claude --write`)가 `refactor/reviews-2026-07-03`에서 R5(task `611c86de`)를 claim → claude 편집(`src/store/sqlite.rs` orphan 정리) → complete 전 구간 성공, **404 없이 완료**. 커밋 `d4b6815` 푸시. 검증: 컴파일 클린 + 새 테스트 `save_session_shrink_cleans_orphan…` ok. → 3자(Opus 통합 + Codex R6 + Mac R5) + 네트워크 레그 완성.

**finding 1 — complete_task 404는 간헐적.** R5에선 안 남(짧은 편집). 긴 러너 실행 시에만 MCP 세션 만료로 404(윈도우 R10). 즉 R10(complete 전 세션 재연결)은 유효하되 상시는 아님.

**finding 2 (신규) — 워커 데몬이 코어 다운에 무방비.** `work --interval 20` 연속 데몬 실행 중 코어(Windows serve)가 죽자: task `b565f68f`를 claim한 뒤 (1) complete가 transport 에러(`error sending request for url`)로 실패 (2) claim한 task가 **working 고아**로 남음(로컬 편집 결과물도 없음) (3) 이후 poll이 무한 에러 스팸. 진단: ping은 되는데 8770만 미서빙 = 머신 정상, 코어 프로세스만 다운. **→ 윈도우 R-리스트로 격상 요청. 제안 리팩토링**: 워커에 (a) 연결 실패 시 지수 backoff + 에러 로그 억제, (b) 코어 재연결 시 MCP 세션 재수립, (c) claim했으나 미완인 task 재개 또는 자동 release(working 고아 방지). R10(세션 재연결)의 상위 집합.

**현 상태(2026-07-03)**: 코어 다운으로 맥 데몬 중단 권고(Ctrl-C) → 코어 재기동 후 재실행. `b565f68f`는 재기동 후 리셋/재큐 필요(윈도우 조치).
