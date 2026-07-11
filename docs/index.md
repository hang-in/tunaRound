# tunaRound 문서

이 문서는 tunaRound의 문서 입구입니다. 처음 사용하는 사람, 여러 머신을 연결하려는 사람, 코드를 수정하려는 사람이 서로 다른 문서를 바로 찾을 수 있도록 나눴습니다.

## 처음 사용하는 경우

| 알고 싶은 것 | 읽을 문서 |
| --- | --- |
| tunaRound가 무엇인지 | [README](../README.md) |
| 설치하고 처음 실행하는 방법 | [온보딩 가이드](reference/onboarding.md) |
| A2A 작업을 보내고 받는 방법 | [A2A 작업 위임 사용법](reference/a2a-usage.md) |
| 여러 머신과 에이전트가 연결되는 구조 | [mesh 아키텍처](reference/mesh-architecture.md) |
| macOS와 Windows를 함께 쓰는 방법 | [macOS와 Windows 구성](reference/dev-mac-windows.md) |

권장 순서는 다음과 같습니다.

1. `README.md`에서 프로젝트의 목적과 기본 명령을 확인합니다.
2. `onboarding.md`에서 자신에게 맞는 설치 경로를 고릅니다.
3. 여러 머신을 연결할 때만 `mesh-architecture.md`를 읽습니다.
4. 실제 작업 위임 명령이 필요할 때 `a2a-usage.md`를 참고합니다.

## 소스에서 실행하거나 기여하는 경우

| 문서 | 내용 |
| --- | --- |
| [소스 빌드와 개발 실행](development/source-run.md) | 피처 조합, 대시보드 빌드, 테스트와 디버깅 |
| [개발 규칙](reference/development-guidelines.md) | 변경 범위, 설계 경계, 테스트, 커밋 규칙 |
| [버전과 릴리스 정책](reference/versioning.md) | 버전 번호, CHANGELOG, 릴리스 절차 |
| [CHANGELOG](../CHANGELOG.md) | 사용자에게 영향을 주는 버전별 변경 내역 |
| [v1 설계 기록](design/tunaRound-v1-design_2026-06-29.md) | 프로젝트가 사람 주도 2-에이전트 토론 도구로 시작한 배경 |

현재 전체 구조는 단일 설계 문서 하나보다 README와 reference 문서에 나뉘어 설명되어 있습니다. v1 설계 기록은 현행 스펙이 아니라 프로젝트의 출발점을 설명하는 역사 문서입니다.

## 작업 문서

아래 문서는 일반 사용 설명서가 아니라 개발 작업을 기록하거나 에이전트에게 일을 넘기기 위한 자료입니다.

| 폴더 | 역할 |
| --- | --- |
| [`plans/`](plans/) | 진행 중인 구현 계획 |
| [`prompts/`](prompts/) | 재사용 프롬프트와 작업 인계문 |
| [`archive/`](archive/) | 완료되거나 보류된 계획과 구버전 문서 |
| [`design/`](design/) | 기능별 설계 문서와 역사적 설계 기록 |
| [`reference/`](reference/) | 사용자·운영자·개발자용 참고 문서 |

작업 상태만 확인하려면 [`plans/index.md`](plans/index.md)를 봅니다. 일반 사용자는 개별 plan이나 prompt를 읽을 필요가 없습니다.

## 문서 작성 규칙

새로운 plan과 prompt에는 다음 frontmatter를 사용합니다.

```yaml
---
title: ...
type: plan | prompt | reference | how-to | archive
status: draft | in_progress | partial | done | archived
priority: P0 | P1 | P2 | P3
updated_at: YYYY-MM-DD
owner: claude | human | shared
summary: 한두 줄 요약
---
```

`priority`는 plan에만 필수입니다. 사용자 문서는 frontmatter보다 제목, 대상 독자, 실행 순서가 먼저 드러나야 합니다.
