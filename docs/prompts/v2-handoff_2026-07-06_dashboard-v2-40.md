# 핸드오프: 총감독 대시보드 완성(목업 React) + v2-40 세션버스 착수 (2026-07-06 세션14)

> WIN 핸드오프. **라이브 접속값(브로커/토큰/PID/포트)은 gitignored `docs/reference/backend-private.md` 하단 "세션14 최종 라이브 상태"를 먼저 읽어라.** 레포 PUBLIC=평문 금지. **이 세션 작업은 브랜치 `feat/orchestrator-dashboard`(PR #12)에 있다.** 다음 = 1) PR #12 머지 → 2) v2-40 S1(자동무장 훅).

## 이 세션(14)에 한 것 (요약)

1. **roster 복구**: win watcher를 `--tags` 붙여 재기동(heartbeat 상시 online) + 맥이 A2A로 자율 재기동 → 3자 감독(mac-claude-sup·mac-codex-sup·win-codex-sup) online.
2. **대시보드 v2-38/39 완성**: T1(라우트)·T2(전역 SSE 피드 + roster JSON)·T3(goal 폼) → **DaleUI SPA** → **Claude Design 목업을 plain React로 재이식**(DaleUI 제거, 번들 258→205KB). rust-embed로 바이너리 임베드(`dashboard` cargo feature). 커밋 350ed05·4d8b231·bec79fe.
3. **goal 백엔드**: `POST /dashboard/goal` = **loopback만(무토큰) / 원격 403(read-only 관전)**. `{text,targets}`→대상마다 task 생성(SSE 자동). ConnectInfo peer.is_loopback. **결정: 로컬=풀컨트롤, 원격=관전.**
4. **디자인 반영**(사용자 피드백): 로스터를 피드와 동일 패널+행 구조로 통일 / shields 뱃지 **값별 색**(mac≠win, claude≠codex) / mac·win 아이콘 박스 제거 / **가짜 총감독 카드 제거 → 대등 카드 + ★토글로 "현재 총감독" 지정**(localStorage, 앉는 머신 따라).
5. **README 최신화** + **v2-40 유니버설 세션 버스 설계**(docs/design/v2-40) + **Planka 보드**(사용자 tunaRound 프로젝트, 백로그 17카드, https://plan.d9ng.co.kr/boards/1813013259255547454).

## 다음: 1) PR #12 머지 → 2) v2-40 S1

### 1) PR #12(대시보드) 머지 (먼저)
- **PR #12** = feat/orchestrator-dashboard → main. v2-38 T1-T3 + v2-39 목업 React SPA + goal loopback + 디자인. 3-OS CI + **dashboard 잡**(node frontend 빌드) + CodeRabbit. **CI green + 브라우저 시각 확인** 후 머지.
- **PR #11**(IP redact→main)도 열려 있음. 권고: #11 먼저 머지 후 #12(둘 다 같은 핸드오프 파일 redact라 무충돌). 또는 #12만(redact 포함).
- 머지 후 main = 대시보드 정식 반영. 대시보드 개발 루프: `frontend`에서 `npm run dev`(HMR) 또는 `npm run build` 후 브로커가 디스크서 읽음(재기동 불요, rust-embed debug).

### 2) v2-40 유니버설 세션 버스 S1 (그다음)
- 정본 [docs/design/v2-40-universal-session-bus_2026-07-06.md](../design/v2-40-universal-session-bus_2026-07-06.md). 목표=임의 세션(예 tunaRound→secall)의 A2A 주소화·발견·제어.
- **S1 = Claude Code SessionStart 훅으로 세션 자동무장**(opt-in `TUNA_AUTOARM=1`): register_agent + Monitor-watched poll 워처 + 정리(SessionEnd). 한 머신 실증 → LAN 복제. 그러면 총감독(현재 로스터 밖)도 등록돼 대시보드에 뜬다.
- 발견≠제어 유의(claude=워처 opt-in / codex=app-server ws). 단계 S2 발견 리포터 → S3 대시보드 후보패널 → S4 codex 직접제어 → S5 검증. 안전 스코핑(바쁜 세션 비파괴, project 격리).

## 첫 행동

1. `docs/reference/backend-private.md` "세션14 최종 라이브 상태" 읽어 브로커(35652)/watcher(46744)/app-server(34176) 확인. 죽었으면 그 블록 커맨드로 재기동(브로커 listen 확인 후 watcher, 레이스 회피).
2. **`git checkout feat/orchestrator-dashboard`**(이 세션 작업 브랜치). PR #12 CI/CodeRabbit 확인 → 브라우저 http://127.0.0.1:8770/dashboard 최종 확인 → 머지. 그다음 v2-40 S1.
3. 규율: 구현 위임 ①tunaLlama ②A2A codex ③Sonnet, **단 프론트/버전 UI 라이브러리 조립은 Opus/Sonnet**(tunaLlama 드리프트, 메모리 참조). Opus 리뷰. GitHub Flow + 3-OS CI + 봇 리뷰. 레포 PUBLIC=평문 토큰/LAN IP 금지. 굵직한 결정 재론 금지.

## 진행 중 브랜치/PR
- `feat/orchestrator-dashboard` `bec79fe` = 대시보드 전체(v2-38 T1-T3 + v2-39 목업 React + goal loopback + 디자인). **PR #12** 열림.
- `fix/redact-lan-ip` = **PR #11**(IP redact→main).
- main = `fbd99db`(세션13). PR #11·#12 머지 대기.
