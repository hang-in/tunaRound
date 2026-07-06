# Plan v2-39: 대시보드 SPA (Vite + React + DaleUI)

> 설계 정본 = [v2-39-dashboard-spa-daleui](../design/v2-39-dashboard-spa-daleui_2026-07-06.md). v2-38 백엔드(SSE·roster·goal API) 재사용, 인라인 HTML을 DaleUI React SPA로 대체. 서빙=rust-embed + `dashboard` feature-gate. `feat/orchestrator-dashboard`에서 이어감 → 한 PR. 구현 위임 ①tunaLlama ②A2A codex ③Sonnet, Opus 리뷰·검증.

## Tasks

- [ ] S1: `frontend/` 스캐폴드 - Vite + React 19 + TS + `daleui@^1.1.1`(+ pretendard, jetbrains-mono). `base:/dashboard/`, `daleui/styles.css` import, dev proxy(/dashboard/events·/roster·/a2a → 127.0.0.1:8770). 최소 App 렌더 확인(`npm run build` 성공). DaleUI Provider/테마 셋업 확인.
- [ ] S2: 3요소 컴포넌트 구현(DaleUI) - roster 패널(Card+Tag, 5초 폴) / 라이브 피드(SSE EventSource, Card 목록, cap) / goal 폼(PasswordInput+TextInput+Select+Button → POST /a2a SendMessage, sel:/agent: 분기, 401/error/다중매칭 표시). 반응형.
- [ ] S3: 브로커 서빙 - Cargo `dashboard` feature + rust-embed(`frontend/dist`) + `GET /dashboard`(index)·`/dashboard/assets/{*path}`(에셋) 라우트(무인증 outer). feature OFF=안내 페이지. 인라인 DASHBOARD_HTML 제거(events/roster API는 serve feature로 유지). 라우트 우선순위·MIME 확인.
- [ ] S4: CI - 3-OS 워크플로에 node setup + frontend 빌드 단계(cargo 앞), `dashboard` feature 포함 조합 1+ 검증. dist gitignore.
- [ ] S5: 검증 - npm build→cargo build(dashboard) 임베드, 브라우저 /dashboard DaleUI 렌더, goal→task→SSE 피드 라이브, feature OFF 안내+API 동작. 3-OS CI green. PR.

## 위임 규약
- frontend(S1·S2) = tunaLlama(React/Vite/DaleUI). DaleUI 컴포넌트 API는 Opus가 .d.ts/스토리로 확인해 스펙에 명시. 서빙(S3)·CI(S4) = tunaLlama 또는 Sonnet, Opus 리뷰. Opus가 라이브 검증.
- feat/orchestrator-dashboard(v2-38 T1~T3 위)에서 이어감. dist 미커밋.

## 비범위 (후속)
- v2-38 T4(claude post_turn emit 피드 합류) 별 PR. codex 원시 델타 패널. 다크모드 토글. 토큰 sessionStorage.
