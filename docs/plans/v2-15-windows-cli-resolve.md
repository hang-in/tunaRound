---
title: "tunaRound v2 Plan 15: 러너 Windows CLI 해석 (gotcha #4, codex.cmd spawn)"
type: plan
status: planned
priority: P0
updated_at: 2026-06-30
owner: shared
summary: 러너가 Command::new("codex")로 spawn하는데 Windows엔 codex.exe가 없고 npm shim codex.cmd만 있어 실패(claude는 claude.exe라 OK). exec.rs에 Windows 전용 bin 해석(PATH에서 .exe/.cmd/.bat 탐색 -> 풀경로 반환, Rust가 .cmd를 cmd.exe로 자동 래핑)을 추가. 비Windows·확장자/경로 있는 bin은 불변. 이게 라이브 토론(두 자리)·Plan 14 라이브 스모크의 전제.
---

# tunaRound v2 Plan 15: 러너 Windows CLI 해석 Implementation Plan

> **For agentic workers:** TDD. **cargo는 Bash 툴로.**
> 진단(2026-06-30): `claude`=claude.exe(spawn OK), `codex`=npm shim(codex/codex.cmd/codex.ps1, codex.exe 없음). Rust `Command::new`는 확장자 없으면 `.exe`만 덧붙여 찾고 `.cmd`는 이름이 `.cmd`로 끝날 때만 cmd.exe 래핑 -> `Command::new("codex")` spawn 실패. tunaFlow wrap_windows_script 패턴 답습.

**Goal:** 러너가 Windows에서 npm/스크립트형 CLI(`codex.cmd` 등)도 spawn하게 해 실 토론(두 자리)을 동작시킨다.

**Architecture:** `src/runner/exec.rs`의 `run_with_watchdog`가 `Command::new(&spec.bin)` 하기 전에 `resolve_bin`으로 bin을 해석. Windows에서 확장자/경로 없는 bin이면 PATH 디렉토리를 돌며 `bin.exe`/`bin.cmd`/`bin.bat`/`bin.com`(이 순서)을 찾아 **풀경로**를 반환(Rust가 `.cmd`/`.bat` 풀경로를 cmd.exe로 자동 래핑, rustc>=1.77.2). 못 찾으면 원본 반환(자연 실패). 비Windows거나 이미 확장자/경로가 있으면 그대로. 순수 탐색부는 PATH 디렉토리를 인자로 받아 테스트 가능하게 분리.

**Tech Stack:** Rust 2024. 신규 의존성 없음. 선행: 없음(독립 버그 수정).

> 규율 #5/#6, TDD, 위임 Sonnet + Opus 리뷰. **부수 영향 주의:** 기존 .cmd 픽스처 테스트(풀경로 .cmd bin)는 "확장자 있음->불변" 가지라 무영향이어야 함.

---

## 범위

- **포함:** `src/runner/exec.rs`에 `resolve_bin`(Windows PATH/PATHEXT 해석) + `run_with_watchdog`에서 호출 + 순수 탐색 함수 테스트.
- **비포함:** codex MCP 배선(Plan 14 후속) · 러너 인자 변경 · PATHEXT 환경변수 완전 준수(고정 확장자 목록으로 충분).
- **불변식:** 비Windows = 완전 불변. Windows에서도 `claude`(claude.exe 탐색됨)·확장자 있는 bin·경로 있는 bin은 동작 동일. 기존 테스트 전부 통과.

## 파일 구조

| 파일 | 책임 |
|---|---|
| `src/runner/exec.rs` | (수정) `resolve_bin` + `resolve_bin_in`(순수, 테스트용) + `run_with_watchdog`에서 spawn 전 해석. |

---

### Task 1: resolve_bin + 배선 + 테스트

**Files:** Modify `src/runner/exec.rs`

- [ ] **Step 1: 실패 테스트 먼저** — 순수 탐색 함수로(PATH 의존 회피):
```rust
    #[cfg(windows)]
    #[test]
    fn resolve_finds_cmd_in_dir() {
        let dir = std::env::temp_dir().join("tuna_resolve_test");
        let _ = std::fs::create_dir_all(&dir);
        let cmd = dir.join("mytool.cmd");
        std::fs::write(&cmd, "@echo off\r\n").unwrap();
        // 확장자 없는 "mytool" -> dir의 mytool.cmd 풀경로.
        let got = resolve_bin_in("mytool", &[dir.clone()]);
        assert_eq!(got.as_deref(), Some(cmd.to_str().unwrap()));
        let _ = std::fs::remove_file(&cmd);
    }

    #[cfg(windows)]
    #[test]
    fn resolve_keeps_bin_with_extension() {
        // 이미 .cmd면 탐색 안 하고 None(=원본 유지 신호) 또는 그대로.
        assert!(resolve_bin_in("foo.cmd", &[]).is_none());
    }

    #[test]
    fn resolve_bin_noop_for_pathed() {
        // 경로/확장자 있으면 원본 그대로(크로스플랫폼 불변 가지).
        let p = "some/dir/tool.sh";
        assert_eq!(resolve_bin(p), p);
    }
```

- [ ] **Step 2: 구현**
```rust
/// Windows에서 확장자 없는 bin을 PATH 디렉토리들에서 .exe/.cmd/.bat/.com 순으로 찾아 풀경로 반환.
/// 이미 확장자/경로가 있거나 못 찾으면 None(원본 유지). 순수 함수(테스트용).
#[cfg(windows)]
fn resolve_bin_in(bin: &str, dirs: &[std::path::PathBuf]) -> Option<String> {
    use std::path::Path;
    if bin.contains('/') || bin.contains('\\') || Path::new(bin).extension().is_some() {
        return None;
    }
    const EXTS: [&str; 4] = ["exe", "cmd", "bat", "com"];
    for dir in dirs {
        for ext in EXTS {
            let cand = dir.join(format!("{bin}.{ext}"));
            if cand.is_file() {
                return cand.to_str().map(|s| s.to_string());
            }
        }
    }
    None
}

/// spawn용 bin 해석. Windows에서 PATH를 뒤져 풀경로화(.cmd 래핑 가능케). 그 외/실패 시 원본.
fn resolve_bin(bin: &str) -> String {
    #[cfg(windows)]
    {
        if let Ok(path) = std::env::var("PATH") {
            let dirs: Vec<std::path::PathBuf> = std::env::split_paths(&path).collect();
            if let Some(found) = resolve_bin_in(bin, &dirs) {
                return found;
            }
        }
        bin.to_string()
    }
    #[cfg(not(windows))]
    {
        bin.to_string()
    }
}
```
  - `run_with_watchdog`: `let mut cmd = Command::new(resolve_bin(&spec.bin));` (기존 `Command::new(&spec.bin)` 교체). 나머지 동일.

- [ ] **Step 3: 검증 + 커밋**
  - `cargo test`(기본) — 기존 전부 + 신규 PASS. **기존 .cmd/.sh 픽스처 러너 테스트 불변 확인**(확장자 있는 bin이라 resolve no-op). `cargo test --features "sqlite morphology semantic mcp"`도 불변. clippy 전 조합 0.
  - 커밋: `fix(runner): Windows CLI 해석(codex.cmd spawn) - gotcha #4`.

---

## Self-Review (작성자 체크)
- **진단 정확:** codex spawn 실패 = codex.exe 부재 + Rust .cmp 미탐색. PATH 해석으로 .cmd 풀경로화 -> Rust 자동 cmd.exe 래핑.
- **불변/격리:** 비Windows 완전 불변. 확장자/경로 있는 bin 불변(기존 .cmd 픽스처 테스트 무영향). claude는 claude.exe 탐색되어 동일 동작.
- **테스트성:** PATH 의존부와 순수 탐색부 분리(temp dir로 결정적 테스트).
- **답습:** tunaFlow wrap_windows_script 개념. 단 Rust 1.77.2+ .cmd 자동 래핑을 활용해 cmd.exe 명시 래핑 불필요(풀경로만 주면 됨).

## 위험 / 한계 (후속)
- **PATHEXT 미완전 준수:** 고정 확장자 목록(exe/cmd/bat/com). PATHEXT 커스텀은 드물어 보류.
- **codex 인증/설정:** spawn 돼도 codex 자체 auth/config 필요(사용자 환경). 이 수정은 spawn 가능까지.
- **라이브 검증:** 실제 codex 동작은 Plan 14 라이브 스모크에서(두 자리). 이 수정은 그 전제.
