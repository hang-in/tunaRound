---
title: "tunaRound v2 Plan 08: 한국어 형태소 토크나이저 포팅 (Kiwi 메인 + lindera 폴백)"
type: plan
status: done
priority: P1
updated_at: 2026-06-30
owner: shared
summary: 한국어 FTS 형태소 문제 해결의 토대. secall 정본 tokenizer.rs(Tokenizer trait + KiwiTokenizer + LinderaKoTokenizer + create_tokenizer factory) 포팅. Kiwi 메인(품질), lindera 폴백(임베드·오프라인). POS keep-tags NNG/NNP/NNB/VV/VA/SL(외국어). morphology feature 게이트(기본 빌드 무영향). anyhow/tracing 없이 String 에러+eprintln 적응. 격리 모듈, 아직 미배선.
---

# tunaRound v2 Plan 08: 한국어 토크나이저 포팅 Implementation Plan

## 실행 결과 (2026-06-30, done — 단 Kiwi 런타임 주의)

브랜치 `feat/v2-ko-tokenizer`(74f8771 lindera, 1059be8 Kiwi). 기본 `cargo test` 66(불변, morphology off=격리 OK), `cargo test --features morphology` 72 pass + 4 ignored. build/clippy 둘 다 + `--features morphology` 경고 0. secall 정본 포팅 충실(String 에러/eprintln, anyhow/tracing 미도입, keep-tags NNG/NNP/NNB/VV/VA/SL, `DEFAULT_BACKEND="kiwi"`, kiwi->lindera 폴백).

- Task 1: lindera 경로 + Tokenizer trait + factory + tokenize_fallback (커밋 `74f8771`).
- Task 2: KiwiTokenizer(플랫폼 cfg, Mutex<KiwiWrapper>) + create_tokenizer kiwi 메인 (커밋 `1059be8`).
- **kiwi-rs 컴파일: 성공**(mac aarch64, v0.1.4). 과거 Mac 컴파일 이슈는 해소.

### ⚠️ 중요 발견: Kiwi 런타임 부트스트랩 실패 -> 현재 lindera로 동작

라이브 테스트(`kiwi_tokenizes_korean_live`)에서 **Kiwi가 런타임에 네이티브 라이브러리 로드 실패**:
```
kiwi-rs init failed: libkiwi.dylib 로드 실패 + auto-download 404
(release asset not found: kiwi_mac_arm64_v0.23.2.tgz)
```
즉 **kiwi-rs 0.1.4가 libkiwi v0.23.2를 받으려다 upstream 릴리스 에셋이 없어 실패** -> `create_tokenizer("kiwi")`가 **lindera로 폴백**(그래서 폴백 테스트는 통과). 사용자 선호(Kiwi 메인)는 코드상 준비됐으나 **현재 실효는 lindera**.
- **해결 후보(후속):** kiwi-rs 버전 핀(에셋 존재하는 버전) / libkiwi 수동 설치(~/Library/Caches/kiwi-rs/) / upstream 이슈 확인.
- **Windows(다음 세션):** Kiwi는 cfg로 제외됨 -> Windows에선 어차피 **lindera만**. 즉 Windows 작업에선 Kiwi 런타임 이슈 무관, lindera가 정상 경로.

---

> **For agentic workers:** REQUIRED SUB-SKILL: superpowers:subagent-driven-development + test-driven-development. Steps use checkbox (`- [ ]`).
> 출처: `~/privateProject/secall/crates/secall-core/src/search/tokenizer.rs` (정본). 결정: docs/design/v2-context-memory-direction_2026-06-30.md, 메모리 korean-search-port-secall. **Kiwi 메인, lindera 폴백.** 아키텍처 재론 금지.

**Goal:** "검색"이 "검색을"을 못 잡는 FTS 형태소 문제의 토대를 깐다. 형태소 분석기로 어간 토큰을 뽑는 `Tokenizer`를 포팅한다(아직 FTS 배선 없음, 토큰 생성까지). Kiwi 메인 + lindera 폴백.

**Architecture:** 신규 `src/search/` 모듈. secall `tokenizer.rs`를 거의 그대로 포팅하되 tunaRound에 맞게 적응: anyhow -> `String` 에러, tracing -> `eprintln!`(tunaSalon `tokenize_ko.rs` 방식). `Tokenizer` trait(`tokenize` + `tokenize_for_fts` 기본구현 = join) + `LinderaKoTokenizer`(embed-ko-dic) + `KiwiTokenizer`(kiwi-rs, 플랫폼 cfg) + `SimpleTokenizer` + `create_tokenizer(backend)`(kiwi 실패 시 lindera 폴백) + `tokenize_fallback`. POS keep-tags = NNG/NNP/NNB/VV/VA/SL. `morphology` feature 뒤로 게이트(기본 빌드/테스트 무영향).

**Tech Stack:** Rust 2024. 신규 의존성(optional, `morphology` feature): `kiwi-rs = "0.1"`, `lindera = { version = "2.3.4", features = ["embed-ko-dic"] }`. 선행: v2 Plan 01~07 done.

> 규율: #5 한국어 마침표, #6 새 파일 첫 줄 역할 주석, TDD. **리스크: kiwi-rs 컴파일**(과거 Mac 이슈). 그래서 lindera(안전)를 Task 1, Kiwi를 Task 2로 분리. Task 2에서 `cargo build --features morphology`로 kiwi-rs 컴파일을 먼저 검증, 실패 시 중단·보고(스래싱 금지).

---

## 범위

- **포함:** `morphology` feature + optional 의존(kiwi-rs/lindera) + `src/search/{mod.rs,tokenizer.rs}` + `src/lib.rs` 모듈 선언. Tokenizer trait + Lindera + Kiwi + Simple + factory + fallback + 테스트.
- **비포함(후속 plan):** FTS 배선(선-형태소화 저장), 벡터(원격 Ollama), 하이브리드, 검색 주입. 이 plan은 토크나이저 모듈만(어디서도 호출 안 함, 격리).

## 파일 구조

| 파일 | 책임 |
|---|---|
| `Cargo.toml` | (수정) optional kiwi-rs/lindera + `[features] morphology`. |
| `src/search/mod.rs` | (신규) `#[cfg(feature="morphology")] pub mod tokenizer;` + 재노출. |
| `src/search/tokenizer.rs` | (신규) secall 포팅: Tokenizer trait + Lindera + Kiwi + Simple + create_tokenizer + fallback + 테스트. |
| `src/lib.rs` | (수정) `pub mod search;`. |

> 선제 설계: 격리 모듈(기존 코드 미접촉). feature 게이트로 기본 빌드 무영향. Kiwi 플랫폼 cfg(Windows/linux-aarch64 제외)는 secall 그대로.

---

### Task 1: 의존성 + lindera 경로 + factory (안전 토대)

**Files:**
- Modify: `Cargo.toml`, `src/lib.rs`
- Create: `src/search/mod.rs`, `src/search/tokenizer.rs`

- [ ] **Step 1: Cargo 의존성 + feature**
```toml
[dependencies]
# ... 기존 ...
kiwi-rs = { version = "0.1", optional = true }
lindera = { version = "2.3.4", features = ["embed-ko-dic"], optional = true }

[features]
morphology = ["dep:kiwi-rs", "dep:lindera"]
```
  - `src/lib.rs`에 `pub mod search;` 추가.
  - `src/search/mod.rs`(신규): 첫 줄 `// 검색 레이어: 한국어 형태소 토크나이저(추후 FTS·벡터·하이브리드).` + `#[cfg(feature = "morphology")] pub mod tokenizer;`.

- [ ] **Step 2: `cargo build --features morphology` 로 lindera 컴파일 확인.** (embed-ko-dic은 사전 임베드라 다운로드 없음, 컴파일만.) 깨지면 멈추고 보고.

- [ ] **Step 3: 실패 테스트 먼저 (`src/search/tokenizer.rs`의 `mod tests`)** — 파일 첫 줄 `// 한국어 형태소 토크나이저(secall 포팅): Tokenizer trait + Kiwi/lindera + factory.`
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lindera_splits_josa_and_keeps_stem() {
        let tok = LinderaKoTokenizer::new().expect("lindera ko-dic load");
        let tokens = tok.tokenize("아키텍처를 설계한다");
        assert!(!tokens.is_empty());
        let joined = tokens.join(" ");
        assert!(joined.contains("아키텍처") || joined.contains("설계"), "조사 분리 실패: {joined:?}");
    }

    #[test]
    fn lindera_keeps_foreign_sl_terms() {
        let tok = LinderaKoTokenizer::new().expect("load");
        let tokens = tok.tokenize("Rust workspace 검색");
        assert!(!tokens.join(" ").is_empty()); // 영어 용어가 SL로 살아남음
    }

    #[test]
    fn empty_and_special_no_panic() {
        let tok = LinderaKoTokenizer::new().expect("load");
        assert!(tok.tokenize("").is_empty());
        let _ = tok.tokenize("!@#$%^"); // 패닉만 없으면 됨
    }

    #[test]
    fn create_tokenizer_unknown_falls_back_to_lindera() {
        assert!(create_tokenizer("unknown").is_ok());
    }

    #[test]
    fn fallback_splits_and_filters() {
        let t = tokenize_fallback("hello,world ab");
        assert!(t.contains(&"hello".to_string()) && t.contains(&"world".to_string()) && t.contains(&"ab".to_string()));
    }
}
```

- [ ] **Step 4: 구현 (`src/search/tokenizer.rs`)** — secall `tokenizer.rs`를 읽어 포팅하되 적응:
  - `Tokenizer` trait: `fn tokenize(&self, text: &str) -> Vec<String>;` + 기본 `fn tokenize_for_fts(&self, text: &str) -> String { self.tokenize(text).join(" ") }`.
  - `LinderaKoTokenizer`: secall 그대로(embed-ko-dic, KoreanKeepTagsTokenFilter NNG/NNP/NNB/VV/VA/SL, surface 소문자, 1글자 제외). **`new() -> Result<Self, String>`**(anyhow -> String). 토큰 오류·빈 결과 시 `tokenize_fallback`.
  - `SimpleTokenizer`(fallback wrapper).
  - `tokenize_fallback`: 공백/ASCII구두점 분리, 소문자, 1글자 제외.
  - `create_tokenizer(backend: &str) -> Result<Box<dyn Tokenizer>, String>`: Task 1에선 **항상 lindera**(kiwi 분기는 Task 2). 시그니처/이름은 secall과 동일.
  - **anyhow/tracing 쓰지 마라**(tunaRound 의존성 아님). `String` 에러 + `eprintln!`.

- [ ] **Step 5: 검증 + 커밋**
  - `cargo test --features morphology` -> 신규 토크나이저 테스트 PASS. `cargo test`(기본) -> 기존 66 그대로(morphology off라 search::tokenizer 미컴파일). `cargo build`/`clippy --all-targets`(둘 다, 그리고 `--features morphology`로도) 경고 0.
  - `git add Cargo.toml Cargo.lock src/lib.rs src/search/ && git commit -m "feat(search): 한국어 토크나이저 포팅 - lindera 경로 + factory (morphology feature)"` (push 금지).

---

### Task 2: Kiwi 경로 + 메인 백엔드 (risk-gate)

**Files:**
- Modify: `src/search/tokenizer.rs`

- [ ] **Step 1: kiwi-rs 컴파일 먼저 검증** — `cargo build --features morphology`가 kiwi-rs를 컴파일하는지(아직 코드에서 안 쓰더라도, 의존성은 이미 추가됨). **kiwi-rs 컴파일이 실패하면 여기서 멈추고 보고**(과거 Mac 이슈 재발 가능성). 그 경우 Task 2 중단, lindera-only(Task 1)로 마감하고 사용자 결정 대기.

- [ ] **Step 2: KiwiTokenizer 포팅 (secall 그대로 + 적응)** — 플랫폼 cfg 유지:
```rust
#[cfg(all(not(target_os = "windows"), not(all(target_os = "linux", target_arch = "aarch64"))))]
mod kiwi_impl { /* secall KiwiWrapper(Send) + Mutex<KiwiWrapper> + KiwiTokenizer */ }
#[cfg(all(not(target_os = "windows"), not(all(target_os = "linux", target_arch = "aarch64"))))]
pub use kiwi_impl::KiwiTokenizer;
```
  - `KiwiTokenizer::new() -> Result<Self, String>`(kiwi_rs::Kiwi::init, anyhow -> String). tokenize: 같은 keep-tags(NNG/NNP/NNB/VV/VA/SL), 소문자, 1글자 제외, 오류·빈 결과 시 fallback. Mutex<KiwiWrapper>(Send-not-Sync) 패턴 secall 그대로.
  - **주의:** `kiwi_rs::Kiwi::init()`은 첫 실행 시 모델 ~50MB를 ~/.cache/kiwi에 다운로드(런타임, 컴파일 아님).

- [ ] **Step 3: factory에 kiwi 메인 + lindera 폴백**
```rust
pub fn create_tokenizer(backend: &str) -> Result<Box<dyn Tokenizer>, String> {
    match backend {
        #[cfg(all(not(target_os = "windows"), not(all(target_os = "linux", target_arch = "aarch64"))))]
        "kiwi" => match KiwiTokenizer::new() {
            Ok(t) => Ok(Box::new(t)),
            Err(e) => {
                eprintln!("[tunaRound] kiwi-rs 초기화 실패, lindera 폴백: {e}");
                Ok(Box::new(LinderaKoTokenizer::new()?))
            }
        },
        _ => Ok(Box::new(LinderaKoTokenizer::new()?)),
    }
}
```
  - (선택) 기본 백엔드 상수 `pub const DEFAULT_BACKEND: &str = "kiwi";`로 "Kiwi 메인" 의도를 코드에 명시.

- [ ] **Step 4: 테스트**
```rust
    #[test]
    fn create_tokenizer_kiwi_returns_working_tokenizer() {
        // kiwi 모델/네트워크 없으면 lindera로 폴백되어도 OK. 한국어 토큰이 나오면 통과.
        let tok = create_tokenizer("kiwi").expect("kiwi or lindera fallback");
        let tokens = tok.tokenize("아키텍처를 설계한다");
        assert!(!tokens.is_empty());
    }

    #[cfg(all(not(target_os = "windows"), not(all(target_os = "linux", target_arch = "aarch64"))))]
    #[test]
    #[ignore] // 수동: kiwi 모델 ~50MB 다운로드 필요
    fn kiwi_tokenizes_korean_live() {
        let tok = KiwiTokenizer::new().expect("kiwi init (model download)");
        assert!(!tok.tokenize("아키텍처를 설계한다").is_empty());
    }
```

- [ ] **Step 5: 검증 + 커밋**
  - `cargo build --features morphology` 경고 0(kiwi-rs 컴파일 성공). `cargo test --features morphology` PASS(ignore 1 스킵). `cargo test`(기본) 66 그대로. `cargo clippy --features morphology --all-targets` 클린.
  - `git add src/search/tokenizer.rs && git commit -m "feat(search): Kiwi 토크나이저 + create_tokenizer kiwi 메인/lindera 폴백"` (push 금지).

---

## Self-Review (작성자 체크)

- **결정 준수:** Kiwi 메인 + lindera 폴백(사용자 확정). secall 정본 포팅(재발명 안 함). POS keep-tags SL로 한/영/코드 혼용 대응.
- **placeholder:** 없음. 출처(secall tokenizer.rs) 명시.
- **격리/무영향:** `morphology` feature off가 기본 -> 기존 빌드/66테스트 불변. 신규 모듈, 기존 코드 미접촉.
- **리스크 관리:** lindera(안전·임베드) Task 1, Kiwi(컴파일 리스크) Task 2로 분리 + 컴파일 검증 게이트. 실패 시 lindera-only로 마감 가능.
- **의존성 적응:** anyhow/tracing 미도입(String 에러 + eprintln). kiwi-rs/lindera는 optional + feature.

## 위험 / 한계 (문서화된 후속)

- **kiwi-rs 컴파일:** 과거 Mac 이슈. Task 2 Step 1에서 검증, 실패 시 lindera-only로 폴백 마감 + 보고. 현 dev(mac aarch64)에선 secall이 동작하므로 가능성 높음.
- **Kiwi 모델 다운로드:** 첫 init ~50MB(~/.cache/kiwi). CI/오프라인에선 lindera 폴백. kiwi 라이브 테스트는 #[ignore].
- **크로스플랫폼 의존:** Windows/linux-aarch64에서 kiwi-rs 자체가 optional dep로 컴파일 안 될 수 있음(현재 mac만 타깃이라 보류). 필요 시 `[target.'cfg(...)'.dependencies]`로 분리.
- **미배선:** 토크나이저는 아직 FTS/검색에 안 쓰임. 다음 plan(SQLite FTS 선-형태소화 저장)에서 `tokenize_for_fts` 사용.