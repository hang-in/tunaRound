# Kiwi Windows 설치 가이드 (v0.22.2)

## 왜 v0.22.2인가

kiwi-rs 0.1.4(현재 사용 버전)는 Kiwi v0.22.2 ABI를 겨냥합니다.

- **Kiwi v0.23.2(latest)** 는 ABI 불일치로 `STATUS_ACCESS_VIOLATION` crash 발생. 사용 금지.
- **kiwi-rs 0.1.4 auto-download** 는 GitHub release API 파싱 로직 버그로 깨져 있음(upstream 이슈). 우리는 discovery 경로 우회로 해결합니다.
- **v0.22.2 수동 설치** 가 현재 유일한 안정 경로입니다. 검증 완료(Windows x64, 2026-06-30).

## Discovery 경로 (env 변수 불필요)

kiwi-rs는 `Kiwi::init()` 호출 시 다음 순서로 libkiwi를 탐색합니다.

1. `KIWI_LIBRARY_PATH` 환경 변수 (설정 시)
2. `KIWI_MODEL_PATH` 환경 변수 (설정 시)
3. `%LOCALAPPDATA%\kiwi\` 기본 경로

`%LOCALAPPDATA%\kiwi\lib\kiwi.dll` 과 `%LOCALAPPDATA%\kiwi\models\cong\base` 에 설치하면 환경 변수 없이도 자동으로 로드됩니다.

## 설치 방법

```bash
# Git Bash / MSYS2 에서 실행
bash scripts/install-kiwi-windows.sh
```

스크립트가 `gh` CLI로 v0.22.2 에셋을 받아 `%LOCALAPPDATA%\kiwi` 에 추출합니다. 이미 설치돼 있으면 건너뜁니다(멱등).

사전 요건: `gh` CLI 설치 + `gh auth login` 완료.

## 미설치 시 폴백

Kiwi `init()` 이 실패하면 `create_tokenizer("kiwi")`는 자동으로 lindera로 폴백합니다. 오류 메시지가 `stderr` 에 출력되고 토크나이저는 정상 동작합니다. 기존 테스트는 Kiwi 설치 여부와 무관하게 통과합니다.

## 검증 절차

```bash
# 기본 빌드(morphology 포함) - Kiwi 경로로 동작
cargo test

# 품질 측정(ignored 테스트, --db 필요)
cargo test --features "semantic morphology" --test search_quality -- --ignored --nocapture
```

`create_tokenizer_kiwi_returns_working_tokenizer` 테스트는 Kiwi 설치 시 Kiwi를 쓰고, 미설치 시 lindera로 폴백합니다. 둘 다 PASS여야 합니다.

## linux-aarch64 제외 이유

kiwi-rs가 linux/aarch64 바이너리를 제공하지 않아 컴파일 자체가 안 됩니다. 해당 플랫폼은 cfg로 Kiwi를 비활성화하고 lindera를 씁니다.

## 주의 사항

- kiwi-rs가 신버전으로 업데이트되면 ABI 핀을 재검토하세요.
- Kiwi 외래어 음절분할(cong/base 모델)은 FTS raw+prefix 색인으로 커버됩니다. 모델 타입 튜닝은 후속 계획.
