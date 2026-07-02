---
title: tunaRound 핸드오프 - 2026-07-02~03 맥 왕복 (rc.1 릴리스 + 티키타카 셋업)
type: prompt
status: active
priority: P0
updated_at: 2026-07-03
owner: shared
summary: 맥 왕복 세션. 맥 전체 검증(빌드·테스트·설치·도그푸딩) + v0.1.0-rc.1 프리릴리스 발행(CI 3버그 수리) + 크로스머신 티키타카 코어 셋업(종료됨). ⚠️ Cargo.toml=0.1.0-rc.1(최종 전 되돌림). 다음=윈도우 A2A worker 핸드오프(ae2fc71) 또는 최종 v0.1.0.
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

- **A) 윈도우 A2A worker 핸드오프**: `ae2fc71`("A2A 도그푸딩 맥 worker 핸드오프") = 맥이 worker로 dispatcher/inbox를 실사용해보는 도그푸딩 과제. 자율 A2A 제어평면 Phase 1 검증.
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
