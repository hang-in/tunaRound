# v2-53 대시보드 전면 재편 (관제탑 shell 리디자인)

> 확정 목업 HTML을 React로 이식하는 프레젠테이션 재편. 데이터층·기존 동작은 보존한다(재작성 아님).

## 목표

- 세로 스택(헤더 위에 섹션 나열) 레이아웃을 **shell 레이아웃**(sticky 헤더 + 사이드바 로스터 + main 피드 + fixed 푸터 헬스)으로 재편한다.
- 목업의 구조·클래스·색토큰·키프레임을 그대로 옮긴다(index.css 전면 재작성 + 컴포넌트 클래스 교체).
- 아이콘은 lucide-react로 통일한다(브랜드 아이콘 claude/codex/mac/win은 커스텀 유지).
- StatTiles를 서버소스화(health.task_counts)해 리로드 안정성을 확보한다.

## 핵심 원칙 (절대 보존)

- Feed SSE 누적·중복가드·트림. Roster buildRoster·★ autoBoss(activity.ts). Health 5초 폴. Search 디바운스.
  GoalForm loopback 제출 + remoteViewer 403 관전. 알림 옵트인 중복 발화 방지. 다크모드 이중지원 토큰.
- 데이터 흐름·API 계약은 건드리지 않는다. 프레젠테이션(클래스·배치·아이콘)만 재편한다.

## 백엔드 변경 (작음, src/mcp/server.rs + src/store/sqlite/tasks.rs)

- [x] `store::sqlite::tasks::count_by_state()` 추가: `SELECT state, COUNT(*) FROM tasks GROUP BY state` 순수 질의 + 단위테스트.
- [x] `dashboard_health_handler`의 `Health` 응답에 `version`(env! CARGO_PKG_VERSION) + `task_counts { working, completed, failed }` 추가.
      working = 진행 중(open) = submitted+working+input_required(목업이 진행중==열린 동일값). fail-visible 유지(질의 실패=500).
- [x] api.ts `BrokerHealth`에 version·task_counts 타입 반영.

## 프론트 컴포넌트 매핑

| 목업 영역 | 컴포넌트 | 변경 |
|---|---|---|
| `header.hdr` | Header.tsx | 재편: brand+ver / omnisearch(Search 흡수) / 목표버튼·테마토글·알림·연결닷·시계 |
| omnisearch dd | Header 내 Omnisearch | SearchPanel 로직 이관(디바운스 400ms, a2a/* 결과, 클릭=전문) |
| `aside.sidebar` | Roster.tsx | 사이드바로. 머신 그룹 + srow(★ boss·runner pill·상대시간) + 행 확장 유지. 하단 side-empty |
| `.tiles` | StatTiles.tsx | 온라인 관리자(roster) + 진행중/완료/실패(health.task_counts, 서버 집계 뱃지) |
| `.card.feed` | Feed.tsx | MAX 50→200·replay=200. 필터=검색+상태/머신/러너 드롭다운(체크박스). 카드 확장 유지 |
| `.card.timeline` | PresenceTimeline.tsx | 터미널 로그 라인 스타일(.log/.logline). 폴/데이터 유지 |
| `footer.dash` | Footer.tsx(신설) | 헬스(좌)/스캐너(우). 타이틀 제거. HealthPanel 데이터 이관 |
| `.modal` | GoalForm.tsx→모달 | 헤더 버튼이 오픈. 기존 폼 내용. 원격=관전 경고. Esc/스크림 닫기 |
| `.dot` heartbeat | index.css | breathe+beat 키프레임. 연결·스캐너 닷. prefers-reduced-motion 존중 |
| theme toggle | Header + useTheme | dataset.theme 토글 + localStorage. Sun/Moon |

- WorkerSection.tsx: 파일 유지, main 렌더에서 제거(사이드바에 안 넣음).
- SearchPanel.tsx: 로직을 Omnisearch로 흡수, 파일 제거.
- HealthPanel.tsx: 데이터 폴을 App으로 lift(Header/StatTiles/Footer가 health 공유), 파일 제거.

## 상태 리프팅 (App.tsx)

- health 5초 폴을 App으로 이동 → version(Header)·task_counts(StatTiles)·나머지(Footer)에 공유.
- modalOpen 상태 소유. Header 목표버튼·로스터 "이 세션에 목표"가 오픈.
- theme 초기화(마운트 시 localStorage/OS).

## 단계 체크리스트

1. [x] 브랜치 + plan doc + lucide-react 설치.
2. [x] 백엔드: count_by_state + 테스트 + health version/task_counts + api.ts 타입.
3. [x] index.css 전면 재작성(목업 토큰·클래스·키프레임 + 컴포넌트 보조 클래스).
4. [x] Header(brand·omnisearch·actions·theme·notify·clock).
5. [x] Roster 사이드바화. StatTiles 서버소스. Feed 드롭다운 필터·200. PresenceTimeline 로그라인. Footer 신설. GoalForm 모달화.
6. [x] App shell 배치 + health lift + modal + theme.
7. [x] 검증: npm build(lint 0) + cargo build/clippy/test. 라이트/다크·반응형·remote 경로 확인.

## 이탈 / 결정

- 온라인 관리자 타일은 roster 파생이라 "서버 집계" 뱃지를 붙이지 않는다(목업은 4개 모두 srv 표기했으나 task 의도=서버소스 3개만).
- 사이드바 머신 그룹의 infra 도트(presence·codex주입)는 제거: presence 도달성은 푸터 스캐너가 canonical. codex-inject 데이터는 GoalForm 대상필터에 계속 사용.
- Feed 필터는 단일선택 → 다중선택(Set) 체크박스로: 목업 체크박스 UI + 클라 필터 유지(서버 무변경).
