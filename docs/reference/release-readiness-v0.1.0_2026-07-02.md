---
title: "v0.1.0 릴리스 준비 (도그푸딩 + 맥 검증, 2026-07-02)"
type: reference
status: snapshot
canonical: false
updated_at: 2026-07-02
owner: shared
summary: tunaround 자체로 v0.1.0 릴리스 준비를 도그푸딩한 결과 + 맥에서의 빌드·테스트·설치·A2A 스모크 검증 현황. 판정=rc.1 먼저. 남은 체크리스트를 me-done / 동구님-action으로 분담.
---

# v0.1.0 릴리스 준비

> **도그푸딩:** tunaround `chat`(claude 아키텍트 + codex 리뷰어)로 "v0.1.0 릴리스 준비"를 토론시킨 결과(맥, 2026-07-02). 원 전사는 임시(`/tmp`); 아래는 판정·체크리스트·검증현황 정제본.

## 도그푸딩 판정 (양 에이전트 조건부 동의)

**지금 `v0.1.0` 직접 태그 금지 → `v0.1.0-rc.1`부터.** 6타깃 릴리스 CI가 한 번도 안 돌았고(특히 aarch64-linux 컨테이너 경로 + Apple 타깃은 로컬 `cargo build`로 안 걸러짐), 첫 cargo-dist 실행을 최종 태그로 시험하는 건 불필요한 위험. codex 정정: "rc면 homebrew 자동 스킵"은 단정 불가 — 실제 조건 `!announcement_is_prerelease || publish_prereleases`, `dist plan`으로 확인.

## 맥 검증 (이 세션, 2026-07-02)

| 항목 | 결과 |
|---|---|
| `cargo build`(기본/풀피처) | ✅ 맥(aarch64) 컴파일 OK |
| `cargo test` | ✅ 기본 195 / 풀피처 212 pass, clippy 클린 |
| `cargo install --path . --features "semantic mcp serve"` | ✅ `~/.cargo/bin/tunaround v0.1.0`(release) |
| E2E 도그푸딩(`chat`) | ✅ claude+codex 라운드·전사·저장·DB, 크래시 0 |
| graceful 저하 | ✅ Kiwi→lindera(자산404) · semantic→FTS(터널없음) · 미설치CLI→에러(패닉X, `[에러] Spawn...`) |
| `dist plan` | ✅ 6타깃 + 인스톨러(sh/ps1/homebrew) + 체크섬 매니페스트 유효 |
| 크로스머신 A2A 스모크(맥→윈도우 코어) | ⚠️ 부분: 네트워크 401/200 ✅, **claude가 원격 전사 ALBATROSS 인용 ✅**, codex read_transcript 취소(pull 취약) ✗ |

→ **README "macOS 실행은 확인 중"은 해소**(이번 검증으로 macOS aarch64 실증). Kiwi만 lindera 폴백.

## 릴리스 체크리스트 (분담)

**me-done (이 세션 처리):**
- [x] 맥 빌드·테스트·설치·도그푸딩 검증
- [x] 미설치 CLI graceful 에러 확인(패닉 없음)
- [x] `dist plan` 매니페스트 유효성
- [x] README macOS 상태 갱신 + Kiwi/AGPL 문구
- [x] CLAUDE.md `install-kiwi-*.sh` 복수형 정정(실제 windows 하나)
- [x] `CHANGELOG.md` 최소본 추가
- [x] 크로스머신 A2A 스모크(claude leg 실증)

**rc.1 CI (완료, 2026-07-02):**
- [x] `v0.1.0-rc.1` 태그 → 릴리스 CI **성공**. **프리릴리스 생성**(assets 15: 4타깃 tarball/zip+sha256, shell/powershell 인스톨러, tunaround.rb, source, manifest).
- rc.1이 잡은 CI 전용 버그 3개(전부 로컬 미검출, 순차 수정):
  1. **버전=태그 불일치**: cargo-dist는 git 태그 버전 = Cargo.toml 패키지 버전 요구 → Cargo.toml `0.1.0-rc.1`(프리릴리스 관례). 최종엔 `0.1.0`으로.
  2. **`[profile.dist]` 누락**: `dist init`이 넣어야 할 프로파일 없어 `cargo build --profile dist` 실패 → Cargo.toml에 `[profile.dist] inherits="release" lto="thin"` 추가.
  3. **aarch64 크로스컴파일 실패**(ring C의 `/imsvc`, arm64-win는 cargo-xwin ring 난제) → **arm64-windows·arm64-linux 제외, 4타깃**(mac arm64·x64, win x64, linux x64).
- **homebrew publish = prerelease라 skipped 확정**(codex 예측 맞음, tap 없이도 rc 안전).

**동구님-action (남음):**
- [ ] rc 아티팩트 맥/윈도우 각 1회 설치·실행(installer.sh/ps1 또는 tarball).
- [ ] `hang-in/homebrew-tap` 레포 생성 + `HOMEBREW_TAP_TOKEN` 시크릿(최종 brew용).
- [ ] **최종 v0.1.0**: Cargo.toml `version="0.1.0"`으로 되돌림 + `git tag v0.1.0 && push` → 정식 Release + homebrew 발행 + `brew install` 검증.
- [ ] (후속) arm64-windows/linux 크로스는 zigbuild/xwin ring 설정 조정 후 재추가.

## 알려진 제약 (릴리스 노트 반영)

- Kiwi 네이티브 자산 자동다운로드 실패 시 lindera 폴백(맥 aarch64 현재 이 상태). semantic은 Ollama 필요, 없으면 FTS. codex 원격 pull 불안정(claude는 안정).
- doctor(Stage 4) 부재는 수용 가능(폴백 graceful, 미설치 CLI actionable 에러 확인됨). 신규 유저 첫 실행 경험 개선용 최소 진단은 후속.
