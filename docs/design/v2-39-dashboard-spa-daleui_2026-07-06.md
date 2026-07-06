# 설계: 대시보드 SPA (Vite + React + DaleUI) (2026-07-06)

> 정본. v2-38 대시보드 MVP(인라인 HTML)의 **프론트를 DaleUI React SPA로 재플랫폼**한다. v2-38의 **백엔드(SSE·roster·goal API)는 그대로 재사용**하고 인라인 `DASHBOARD_HTML`만 SPA로 대체. 계획 = [v2-39](../plans/v2-39-dashboard-spa.md). 사용자 결정: 서빙=embed+feature-gate.

## 0. 배경·결정

- v2-38로 대시보드 3요소(roster / 라이브 task 피드 / goal 폼)를 인라인 HTML+JS로 구현·라이브 검증했다. 백엔드 API(`GET /dashboard/events` SSE, `GET /dashboard/roster` JSON, `POST /a2a` SendMessage)는 견고하다.
- 사용자 요구: 대시보드 UI를 **DaleUI**(github.com/DaleStudy/daleui) 도입해 미려하게. DaleUI = React 19 + Panda CSS 컴포넌트 라이브러리라 인라인 HTML로는 못 쓰고 **프론트 빌드 파이프라인**이 필요하다.
- **서빙 결정 = 바이너리 임베드 + feature-gate**(사용자 확정). 근거: cargo-dist 단일 바이너리 배포와 정합(추가 파일·경로 0, zero-friction). "터미널 순수파에 강요 안 함"은 서빙 방식이 아니라 **cargo `dashboard` feature**로 해결(기본 lean, release 프리빌트는 ON). "리치=optional"은 애초에 브라우저 URL이라 자동 성립(embed/dir 무관).

## 1. 아키텍처

### 1.1 구성
- 신규 `frontend/` = Vite + React 19 + TypeScript + `daleui@^1.1.1`. `npm run build` → `frontend/dist/`(index.html + assets/).
- Vite `base: "/dashboard/"` → asset이 `/dashboard/assets/*`로 나가고 index는 `/dashboard`에 매핑.
- 폰트: `pretendard`, `@fontsource-variable/jetbrains-mono`(DaleUI peerDep). `daleui/styles.css` import.

### 1.2 백엔드 재사용 (net-new 최소)
SPA가 소비하는 API는 **전부 이미 존재**:
- `GET /dashboard/events` (SSE, 전역 TaskEvent) - v2-38 T2.
- `GET /dashboard/roster` (JSON, online 감독) - v2-38 T2.
- `POST /a2a` (JSON-RPC SendMessage, bearer) - goal 제출. v2-38 T3가 쓰던 그 경로.
신규 백엔드 = **정적 에셋 서빙 라우트뿐**(embed).

### 1.3 서빙 (rust-embed + feature-gate)
- Cargo feature **`dashboard`**(신규). rust-embed 의존은 이 feature에서만.
- `#[cfg(feature="dashboard")]`: `#[derive(RustEmbed)] #[folder="frontend/dist"] struct DashAssets;` (debug=디스크 읽기, release=바이너리 내장).
- 라우트(무인증 outer, read-only):
  - `GET /dashboard` → `index.html`.
  - `GET /dashboard/assets/{*path}` → 임베드 에셋(정확 MIME).
  - (favicon 등 index가 참조하는 루트 상대자원은 base=/dashboard/라 assets 밑으로 떨어짐.)
- **라우트 우선순위**: 구체 API 라우트(`/dashboard/events`, `/dashboard/roster`)가 SPA 자원 라우트와 경로 미충돌(SPA는 `/dashboard`와 `/dashboard/assets/*`만). 충돌 없음.
- feature **OFF**: `/dashboard`는 "이 빌드는 dashboard 피처 없이 빌드됨(--features dashboard 또는 release 바이너리 사용)" 최소 안내 페이지. **API 라우트(events/roster)는 serve feature라 항상 존재**(SPA 유무 무관, 다른 클라이언트도 씀).
- 인라인 `DASHBOARD_HTML`(v2-38) = SPA가 대체하므로 제거. serve feature의 폴백 안내만 남김.

### 1.4 인증 경계 (불변)
- 대시보드 read(SPA 로드·events·roster) = 무인증 outer(로컬 read-only). goal 제출(write) = `/a2a` bearer 게이트. SPA는 토큰 입력 필드(선택: sessionStorage 보관)로 받아 `Authorization: Bearer`로만 전송(미영속 서버측). v2-38 T3와 동일 정책.

## 2. 화면 (DaleUI 컴포넌트 매핑)

3요소를 DaleUI로. 사용 컴포넌트(v1.1.1 export): Box/Flex/Grid/HStack/VStack/Card, Heading/Text, TextInput/PasswordInput/Select/Button/Label, Tag, Icon(lucide).

- **헤더**: Heading "총감독 대시보드" + 연결/토큰 상태 Tag.
- **roster 패널**: 감독별 Card. uuid(+display_name), 태그를 Tag 나열, online/stale를 색 Tag(heartbeat 기준). `/dashboard/roster` 5초 폴(또는 events 겸용).
- **라이브 task 피드**: 세로 목록(VStack). 각 이벤트 = Card/행: `[event]` Tag(status/completed 색) + id8 + from→to + state + artifact 첫줄. `/dashboard/events` SSE(EventSource) 구독, 상단 prepend, 상위 N cap.
- **goal 제출 폼**: Card 안 PasswordInput(토큰) + TextInput(목표) + Select(대상: roster로 채운 감독 + "모든 감독 role=supervised" 옵션) + Button. 제출=`POST /a2a` SendMessage. 결과(성공 task_id/에러/다중매칭 후보) Text로 표시.

반응형(Grid/Flex), 라이트/다크는 DaleUI 토큰 따름.

## 3. 개발·빌드·CI

- **dev 루프**: `npm run dev`(Vite HMR, localhost:5173). API는 Vite proxy로 브로커(127.0.0.1:8770)에 프록시(`/dashboard/events`,`/dashboard/roster`,`/a2a`). cargo 재빌드 불요.
- **프로덕션**: `npm ci && npm run build`(frontend) → `cargo build --features "... dashboard"`(dist 임베드). rust-embed release=내장.
- **CI(3-OS)**: 기존 워크플로에 node setup + `frontend` 빌드 단계를 cargo 앞에 추가. `dashboard` feature 포함 조합 1개 이상 빌드·검증. (frontend 산출은 OS 독립이나 임베드는 per-OS cargo 빌드라 각 잡이 frontend 빌드.)
- **dist는 gitignore**(빌드 산출물 미커밋). CI/release가 매번 빌드. Rust-only 빌드(`dashboard` OFF)는 dist·node 불요.

## 4. 범위 / 비범위

- **범위(이 설계)**: frontend 스캐폴드 + 3요소 DaleUI 구현 + rust-embed 서빙 + feature + CI node 단계 + 인라인 HTML 제거.
- **비범위(후속)**: v2-38 T4(claude 감독 post_turn emit 피드 합류) - 별 PR. codex 원시 델타 패널. 다크모드 토글 UI. 대시보드 라우팅(멀티페이지). 토큰 sessionStorage 편의(선택 적용).

## 5. 열린 질문

- CI에서 `dashboard` feature를 어느 잡에 넣나(전 3-OS vs 1 OS만 full). 최소 = release 빌드 경로 + 1 CI 잡에서 frontend+dashboard 빌드 검증.
- rust-embed debug 디스크읽기 시 dist 부재면? → 핸들러가 index 없으면 "빌드 필요" 안내(패닉 금지).
- React 19 + DaleUI Provider 필요 여부(styles.css import만으로 되는지, 테마 Provider 필요한지) = 스캐폴드 시 확인.

## 6. 검증

1. `npm run build` 성공 → `cargo build --features "... dashboard"` 임베드 성공.
2. 브로커 기동 후 브라우저 `/dashboard` = DaleUI SPA 렌더(roster/피드/폼).
3. 라이브: goal 제출→task 생성→SSE 피드 실시간 반영, roster online 표시.
4. feature OFF 빌드 = 안내 페이지, events/roster API는 동작.
5. 3-OS CI green(node 빌드 포함).
