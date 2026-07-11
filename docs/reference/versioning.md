# 버전과 릴리스 정책

이 문서는 tunaRound의 버전 번호를 정하고 릴리스를 만드는 기준을 설명합니다.

버전의 단일 출처는 `Cargo.toml`이며, 사용자에게 보이는 변경 내역은 `CHANGELOG.md`에 기록합니다. 태그가 push되면 cargo-dist 기반 GitHub Actions가 설치 파일과 릴리스 아티팩트를 만듭니다.

## 1. 버전 번호

버전은 Semantic Versioning 형식인 `MAJOR.MINOR.PATCH`를 사용합니다.

현재는 1.0 이전이므로 다음 기준을 적용합니다.

| 변경 종류 | 버전 증가 | 예시 |
| --- | --- | --- |
| 기능 추가 | MINOR | `0.4.0` → `0.5.0` |
| 호환되지 않는 변경 | MINOR | `0.4.0` → `0.5.0` |
| 하위 호환 버그 수정 | PATCH | `0.4.0` → `0.4.1` |
| 문서와 내부 정리만 | 보통 PATCH | `0.4.0` → `0.4.1` |

1.0 이후에는 표준 SemVer를 따릅니다.

- MAJOR는 호환되지 않는 변경입니다.
- MINOR는 하위 호환 기능 추가입니다.
- PATCH는 하위 호환 수정입니다.

## 2. 버전의 단일 출처

현재 버전은 `Cargo.toml`의 `package.version`만 수정합니다.

코드에서는 다음 값을 사용합니다.

```rust
env!("CARGO_PKG_VERSION")
```

README, Agent Card 구현, 별도 상수에 버전 번호를 중복해서 하드코딩하지 않습니다.

현재 버전은 다음 명령으로도 확인할 수 있습니다.

```bash
cargo pkgid
```

## 3. CHANGELOG 작성

`CHANGELOG.md`는 Keep a Changelog 형식을 따릅니다.

사용자에게 보이는 변경이 있는 PR은 `## [Unreleased]` 아래에 항목을 추가합니다.

| 분류 | 내용 |
| --- | --- |
| Added | 새 기능과 새 명령 |
| Changed | 기존 동작이나 기본값 변경 |
| Fixed | 버그 수정 |
| Removed | 제거된 기능 |
| Security | 보안 관련 수정 |

내부 리팩터링, 테스트 정리처럼 사용자 동작에 영향이 없는 변경은 생략할 수 있습니다.

좋은 항목은 구현 방식보다 사용자가 무엇을 얻게 되는지 먼저 설명합니다.

```markdown
### Added

- 워커를 태그로 검색해 작업 대상을 고를 수 있습니다.
```

다음과 같이 내부 계획 번호와 구현 세부만 적는 방식은 피합니다.

```markdown
- v2-44 report_presence 및 registry sync 구현.
```

내부 계획 번호가 추적에 필요하면 사용자 설명 뒤에 괄호로 덧붙입니다.

## 4. 릴리스 전 확인

릴리스하기 전에 다음 조건을 확인합니다.

- `main`이 원격과 동기화되어 있음
- `CHANGELOG.md`의 `Unreleased`가 정리되어 있음
- 버전 증가 종류가 변경 내용과 맞음
- 필수 테스트와 Clippy가 통과함
- GitHub Actions가 현재 기본 브랜치에서 정상임

권장 검증입니다.

```bash
git checkout main
git pull --rebase origin main
cargo fmt -- --check
cargo test
cargo clippy --all-targets --all-features -- -D warnings
dist plan
```

## 5. cargo-release로 릴리스하기

처음 한 번 설치합니다.

```bash
cargo install cargo-release
```

먼저 dry run으로 계획을 확인합니다.

```bash
cargo release patch
# 또는
cargo release minor
```

문제가 없으면 실제 릴리스를 실행합니다.

```bash
cargo release patch --execute
# 또는
cargo release minor --execute
```

설정은 `release.toml`에 있습니다.

실행하면 다음 과정이 이어집니다.

1. `Cargo.toml` 버전 증가
2. `CHANGELOG.md`의 `Unreleased`를 새 버전과 날짜로 확정
3. 새 빈 `Unreleased` 섹션 생성
4. `chore(release): v<버전>` 커밋 생성
5. `v<버전>` 태그 생성
6. 커밋과 태그 push
7. 태그를 받은 cargo-dist 워크플로가 릴리스 아티팩트 생성

자동화가 파일을 어떻게 바꾸는지 dry run에서 반드시 확인합니다.

## 6. 태그 규칙

정식 릴리스 태그는 다음 형식입니다.

```text
vMAJOR.MINOR.PATCH
```

예시입니다.

```text
v0.5.0
v0.5.1
```

프리릴리스는 다음처럼 표기합니다.

```text
v0.5.0-rc.1
```

태그는 가능한 한 `cargo release`가 만들게 합니다. 수동 태그는 `Cargo.toml`, CHANGELOG, 태그가 서로 어긋날 수 있어 피합니다.

## 7. 프리릴리스

릴리스 파이프라인이나 설치 파일을 먼저 확인해야 할 때 `rc` 태그를 사용할 수 있습니다.

프리릴리스에서는 `Cargo.toml` 버전과 Git 태그가 정확히 같아야 합니다.

```toml
version = "0.5.0-rc.1"
```

```text
v0.5.0-rc.1
```

검증이 끝난 뒤 정식 버전으로 되돌리고 정식 태그를 만듭니다.

프리릴리스의 Homebrew 발행 여부와 타깃 목록은 현재 `.github/workflows/release.yml`과 cargo-dist 설정을 기준으로 확인합니다. 과거 릴리스의 타깃 수를 문서에 고정하지 않습니다.

## 8. 릴리스 실패 시 확인 순서

1. Git 태그와 `Cargo.toml` 버전이 같은지 확인합니다.
2. `Cargo.toml`에 cargo-dist가 사용하는 프로파일이 있는지 확인합니다.
3. GitHub Actions의 개별 job 결과를 확인합니다.
4. 특정 플랫폼만 실패하면 해당 타깃의 네이티브 의존성과 크로스 컴파일 로그를 확인합니다.
5. 실패한 릴리스 위에 새 태그를 덧붙이기 전에 원인을 수정하고 버전 정책에 맞게 다시 진행합니다.

## 9. 다음 버전 결정

다음 릴리스의 내용은 `CHANGELOG.md`의 `Unreleased`에서 확인합니다.

- 새 기능이나 호환되지 않는 변경이 있으면 `minor`
- 수정과 문서 변경만 있으면 `patch`
- 1.0 이후 호환되지 않는 변경이면 `major`

현재 버전 번호나 다음 버전 후보를 이 문서에 직접 적지 않습니다. 해당 정보는 `Cargo.toml`과 `CHANGELOG.md`가 맡습니다.
