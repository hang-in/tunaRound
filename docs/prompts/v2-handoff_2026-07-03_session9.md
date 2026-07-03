# tunaRound 핸드오프 - 2026-07-03 세션9 (공개 마이그레이션 + 브로커 거버넌스)

> 콜드 스타트 가정. 이 문서 + 아래 "정본 포인터"로 이어갈 수 있게 씀. **다음 세션 목적 = 브로커 거버넌스 구현.** 이전 세션8은 [session8-refactor](v2-handoff_2026-07-03_session8-refactor.md).
>
> ⚠️ **레포가 이제 PUBLIC입니다.** LAN IP·토큰·사설 호스트를 문서/코드에 평문으로 쓰지 말 것(이번 세션에 히스토리까지 퍼지함). placeholder(`<브로커-LAN-IP>`·`<TOKEN>`·`[사설호스트]`)만 사용.

## ⓪ 가장 먼저

1. **레포 확인**: `git remote -v`가 `hang-in/tunaRound`(PUBLIC, -private 아님)인지. 이번 세션에 filter-repo로 시크릿 퍼지 후 새 public 레포로 이전함. 옛 것은 `hang-in/tunaRound-private`(PRIVATE 아카이브, 손대지 말 것).
2. `git pull --rebase origin main` (head = `e3fe132` 부근, 브로커 거버넌스 노트까지).
3. cargo는 **Bash 툴**로. 이 Windows 박스는 rustc가 **전체 병렬 재빌드 시 메모리 압박으로 크래시**하니, `cargo test`는 **`CARGO_INCREMENTAL=0 ... -j 4`** 로. 검증 = `cargo test --features "morphology mcp serve worker"` → **321 pass 기대**.

## ① 다음 세션 목적 = 브로커 거버넌스 구현

정본: **[docs/design/v2-broker-governance_2026-07-03.md](../design/v2-broker-governance_2026-07-03.md)**. §4 우선순위 순서대로:

1. **네이밍 컨벤션 문서화** + a2a-usage에 "to_agent는 워커만"(dispatcher id는 from 전용). 비용 0.
2. **Agent Card/poll에 능력 광고**(runner·write 여부·빌드 피처). doctor·dispatch가 참고.
3. **고착 노출**: `poll`/신규 `tasks` 출력에 오래된 `working`(updatedAt 낡음)을 "stuck?"으로 표시. `updatedAt` 이미 있어 거의 공짜.
4. **no-consumer TTL 알림**: submitted가 TTL 초과 시 `expired` + poll 표시.
5. **워커 격리**: write task는 node 실행 클론과 분리된 워크트리에서(self-disruption 방지).
6. (후속) claim TTL requeue, 하트비트, 능력 기반 자동 라우팅.

근거가 된 이번 세션 두 실패(그대로 스펙): (a) `win-opus`(dispatcher)로 보낸 task가 폴러 없어 영구 submitted, (b) 뱃지 task가 맥 워커의 **자기 node 클론을 `reset --hard`로 갈아엎어 워커 자살** → 영구 `working` 고착(self-disruption).

## ② 이번 세션(9)이 한 것

- **R1-R10 리팩토링 → PR #1 머지.** R7(retriever/reader Result 계약)을 맥 워커 A2A로 완료. **PR CI(3-OS 매트릭스) 도입**, CI가 R3 Unix 이식성 버그(`kill -9 -PID` no-op) 포착 → `libc::kill(-pid,SIGKILL)` 수정. 정본 [리팩토링 계획](../plans/v2-refactor-from-reviews_2026-07-03.md).
- **poll 감시자 → PR #2·#3.** 감독 레인을 유휴 0토큰으로 wake(`tunaround poll` + 하네스 Monitor). gemini 리뷰(seen 메모리 상한) 반영.
- **node 고도화 → PR #3·#4·#5.** `tunaround init`/`node`/`doctor` + `NodeConfig`. config 1개 + 데몬 하나 = 워커 노드. 리뷰 8건 반영(kind 오타 보안 거부·node 실패레인 가시화·doctor 피처/AddrInUse 정밀화). 정본 [node 온보딩](../design/v2-node-onboarding_2026-07-03.md).
- **공개 마이그레이션(공개 못했던 이슈 해소).** filter-repo로 히스토리 272커밋 전체에서 시크릿(사설 호스트·SSH 계정·LAN IP·세션 토큰) 플레이스홀더화 → **새 PUBLIC `hang-in/tunaRound`(히스토리 보존, 시크릿 0)**. 옛 것은 `-private`로 rename. 함정: GitHub PR-ref가 옛 커밋을 남겨서 같은 레포 force-push는 유출 → **새 레포로 push**가 정답(히스토리 보존).
- **크로스머신 도그푸딩.** 맥↔윈 양방향 node 위임 실증(양쪽 `tunaround node`). README 뱃지·명령 갱신.
- **usecase 문서** [agent-dev-team](../reference/agent-dev-team.md), **거버넌스 노트**(위 ①).

## ③ 정본 포인터 (정확히)

- **거버넌스(다음 세션 주제)**: [v2-broker-governance_2026-07-03.md](../design/v2-broker-governance_2026-07-03.md)
- node 온보딩 설계: [v2-node-onboarding_2026-07-03.md](../design/v2-node-onboarding_2026-07-03.md)
- 에이전트 개발팀 usecase: [agent-dev-team.md](../reference/agent-dev-team.md)
- A2A 사용법(dispatcher/코어/워커): [a2a-usage.md](../reference/a2a-usage.md)
- 파트너 위임(원설계): [v2-a2a-partner-delegation_2026-07-02.md](../design/v2-a2a-partner-delegation_2026-07-02.md)
- 워커 데몬 설계: [v2-a2a-worker-daemon_2026-07-03.md](../design/v2-a2a-worker-daemon_2026-07-03.md)
- 리팩토링 계획(R1-R10): [v2-refactor-from-reviews_2026-07-03.md](../plans/v2-refactor-from-reviews_2026-07-03.md)
- 현행 spec: [tunaRound-v1-design_2026-06-29.md](../design/tunaRound-v1-design_2026-06-29.md) · 진행: [../plans/index.md](../plans/index.md)
- 이전 핸드오프: [session8-refactor](v2-handoff_2026-07-03_session8-refactor.md) · [session6](v2-handoff_2026-07-03_session6.md)

## ④ 인프라 상태

- **레포**: PUBLIC `hang-in/tunaRound`(로컬 origin=이것). 아카이브 `hang-in/tunaRound-private`.
- **CI**: 3-OS 매트릭스(.github/workflows/ci.yml), public이라 무료 무제한. docs-only는 `paths-ignore`로 스킵.
- **리뷰 봇**: `.coderabbit.yaml`·`.gemini/styleguide.md`는 레포에 있음. 새 public 레포에 봇 앱을 **재설치**해야 PR 리뷰 작동.
- **브로커/워커(이번 세션 도그푸딩용)**: Windows가 `tunaround node`로 브로커+win-worker 상주(`<브로커-LAN-IP>:8770`, 토큰 `<TOKEN>`=채팅 전달, db=LOCALAPPDATA). **다음 세션 시작 시 죽어 있을 수 있음**(재부팅 등) → 필요하면 재기동. 맥 워커는 뱃지 task self-disruption으로 클론이 꼬였을 수 있으니 **맥은 새 public 레포로 clean 재클론** 권장.
- **백업**: filter-repo 이전 상태 번들 = scratchpad `tunaRound-prepurge-backup.bundle`(세션 종료 시 사라짐 - 필요하면 영구 위치로 옮길 것). `pip install git-filter-repo` 설치됨.

## ⑤ 남은 항목(비거버넌스, 후순위)

- **dispatch CLI** `send`/`watch`/`tasks`(raw curl 대체) - node 고도화 후속 PR로 계획만.
- **Ollama Cloud 워커**: `runner=http`가 코어 토큰을 LLM 키로 재사용해 인증 클라우드(kimi 등) 불가 → 레인에 `http_api_key`(`@env:`) 분리 필드 추가 필요(소, 논의만 함). `engines` 피처 필요, doctor가 이미 피처 부재 잡음.
- 릴리스 `v0.1.0` 태그(public이라 installer/brew 익명 다운로드 작동 - 이전 404 해소). Cargo.toml `version="0.1.0-rc.1"` → 태그 전 `0.1.0`으로.

## ⑥ 규율(유지)

- 구현 위임=Sonnet 서브 or A2A 워커 + Opus 리뷰·독립검증. cargo=Bash(`-j 4` + `CARGO_INCREMENTAL=0`). 한국어 마침표·새파일 역할주석·em-dash 금지. **public 레포이므로 시크릿(LAN IP·토큰·사설호스트) 평문 금지.** 굵직한 결정 재론 금지.
