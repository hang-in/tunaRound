# 버전 관리 정책 (SemVer + cargo-release)

> 공개 레포이므로 버전을 [Semantic Versioning](https://semver.org/lang/ko/)으로 관리하고, 매 릴리스를 `cargo-release`로 고정한다. 릴리스 아티팩트는 태그 push 시 cargo-dist(`.github/workflows/release.yml`)가 빌드한다.

## 1. 버전 규칙 (SemVer, pre-1.0)

버전은 `MAJOR.MINOR.PATCH`. 1.0 이전(`0.x`)에는 아래 pre-1.0 관례를 따른다.

- **0.x 동안 MINOR**(`0.MINOR.0`): 기능 추가 **또는 파괴적 변경**. 0.x는 공개 API 안정성을 보장하지 않으므로 파괴적 변경도 MINOR로 낸다.
- **0.x 동안 PATCH**(`0.x.PATCH`): 하위호환 버그픽스·문서·내부 정리.
- 1.0 이후에는 표준 SemVer(MAJOR=파괴적, MINOR=기능, PATCH=픽스).

버전의 **단일 출처는 `Cargo.toml`의 `version`** 하나다. 코드는 `env!("CARGO_PKG_VERSION")`로 읽는다(예: A2A Agent Card의 `version`). 다른 곳에 버전을 하드코딩하지 않는다.

## 2. CHANGELOG 규율

형식은 [Keep a Changelog](https://keepachangelog.com/ko/1.1.0/).

- **사용자 가시 변경이 있는 모든 PR은 `CHANGELOG.md`의 `## [Unreleased]`에 항목을 추가한다**(추가/변경/고침/제거 소분류).
- 내부 리팩토링·테스트 전용 등 사용자 무관 변경은 생략 가능.
- 릴리스 시 `[Unreleased]`가 그대로 새 버전 섹션이 된다(아래 자동화가 처리).

## 3. 릴리스 흐름 (cargo-release)

설치(1회): `cargo install cargo-release`.

릴리스(예: 기능 묶음 -> MINOR):

```bash
git checkout main && git pull --rebase origin main
cargo release minor           # 미리보기(dry-run이 기본). 계획을 확인한다.
cargo release minor --execute # 실제 실행
```

`cargo release <patch|minor|major> --execute`가 자동으로:

1. `Cargo.toml` 버전을 범프한다.
2. `CHANGELOG.md`의 `## [Unreleased]`를 `## [<버전>] - <날짜>`로 굳히고 새 빈 `[Unreleased]`를 심는다(`release.toml`의 `pre-release-replacements`).
3. `chore(release): v<버전>` 커밋을 만든다.
4. `v<버전>` 태그를 단다.
5. 커밋·태그를 push한다.
6. 태그 push가 `release.yml`(cargo-dist)을 발화 -> 6타깃 인스톨러/아티팩트 빌드.

설정은 `release.toml`에 있다(게시 안 함=앱이라 crates.io 비대상, main 브랜치에서만 릴리스).

## 4. 태그 규약

- 형식: `vMAJOR.MINOR.PATCH`(예: `v0.2.0`). 프리릴리스는 `v0.2.0-rc.1`.
- `release.yml` 트리거 패턴(`**[0-9]+.[0-9]+.[0-9]+*`)과 정합한다.
- 태그는 `cargo release`가 만든다(수동 `git tag` 지양 - Cargo.toml/CHANGELOG와 어긋남 방지).

## 5. 현재 상태

- 마지막 안정 baseline: **0.1.0**(`Cargo.toml`). rc 기간(`v0.1.0-rc.1`) 종료.
- 다음 릴리스: **0.2.0**(0.1.0 이후 A2A 위임·스트리밍·워커·outbound·거버넌스 = `[Unreleased]`). 관련 PR 머지 후 `cargo release minor --execute`.
