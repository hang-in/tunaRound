# 세션28 핸드오프 (2026-07-13~14): fable 5 코드베이스 리뷰 -> 15개 수정 PR 머지 + CI 하드닝 + 브랜치 보호 + relay 라이브 검증·배포

> 진입점. 이번 세션은 **전체 코드베이스 리뷰(fable 5) 후 확증된 결함을 sonnet 실무자로 부려서 전량 패치**한 큰 세션이다. 정본 spec은 [tunaRound-v1-design](../design/tunaRound-v1-design_2026-06-29.md). 협업/거버넌스 규약은 CLAUDE.md 상단.

## 한 줄 요약

fable 5 멀티에이전트 리뷰로 **103개 confirmed findings**(major 29·minor 74)를 뽑고, 그중 **약 85개를 15개 수정 PR**로 봉합해 main에 전부 머지했다. 매 PR = `sonnet 실무자 정밀 패치 -> Opus 중앙 검증(fmt·clippy all-targets 풀피처·test) -> CI 3-OS green -> CodeRabbit·gemini 봇 리뷰 전수 반영 -> 머지`. 추가로 CI 공급망 하드닝, main 브랜치 보호 룰셋, codex-relay 비동기 재설계(라이브 검증·win 배포)까지 완료. **main 클린, 열린 PR 0, 로컬 브랜치 main만. 미커밋 없음.** main=`4d08d68`.

## 완료한 것 (PR 번호·커밋 재확인 가능)

리뷰 findings 수정 PR 15개(전부 머지):

| PR | 주제 | 핵심 |
|---|---|---|
| #95 | fail-visible 계약 | poll_tasks 에러계약(R1)·get_task failed 사유 렌더·조용한 에러 은폐 |
| #96 | 보안/인증 | 무토큰 노출 경고(soft)·상수시간 토큰비교·http 러너 토큰 분리·비대화형 ★ 오염 방지 |
| #97 | 이식성 | claude 프롬프트 argv->stdin(npm .cmd+개행 spawn 실패)·Windows presence 게이트 node 래퍼·경로 이식성 |
| #98 | 워커 신뢰성 | http 30초 타임아웃->600·러너 실행 중 heartbeat(60초)·context-map 가드·run_on_task 트리kill·파싱 방어 |
| #99 | codex 안전 | relay #88 게이트·승인 threadId 필터·turn_id 매칭·relay 정책 노브 |
| #100 | 검색/RAG | 벡터 폴백 순서 보존·vector_search model_id/dim 필터·index_vectors 트랜잭션 분리·검색 락 해제·MockEmbedder |
| #101 | REPL/오케스트레이터 | --core seed 데이터손실 가드·checkout no-op 거부·append 소실 표면화·run_round 부분폐기·pull mode |
| #102 | 대시보드 SPA | goal errors 표시·relay no-op 사유·폴 레이스 abort·FilterDropdown a11y·seenState/pulseTimers 누수 |
| #103 | watch/mcp | SSE Lagged 신호·워터마크 파일명 충돌·미래 since·jsonrpc id 대조+plain JSON·네트워크 재시도·spawn_blocking |
| #104 | store 무결성 | COMMIT 실패 ROLLBACK·append_turn BEGIN IMMEDIATE·created_at content키·try_fail requeue 가드·migrate 미래버전 가드·무가드 API cfg(test)·sync_presence 소유격리·★ 즉시삭제->7일 GC |
| #105 | mesh 운영/presence | restart PID 즉시 append·재부팅 stale 감지·kiwi 멱등·tombstone/.rx GC·autoarm 죽은키·doctor 훅sync·codex_input_cache 디스크영속·normalize_iso 비UTC offset·roster 중복engine 경고 |
| #106 | 테스트 커버리지 | run_one_pass·dashboard goal/search 게이트·presence 합성·watch 재접속·health/human-ping·codex-relay 무테스트 봉합 + goal 핸들러 IPv4-mapped loopback 실버그(#29) 수정 |
| #107 | 정리 | 미사용 daleui 제거·v2-47 문서 semantic 전제 정정 |
| #108 | CI 공급망 하드닝 | permissions read·persist-credentials false·서드파티 액션 SHA 핀·dependabot(groups)·release-features/scripts/audit 잡 |
| #112 | **relay 비동기(라이브 검증)** | 주입 중 heartbeat/lease(select)·ws 도달성 게이트·#9 취소불가 명시 |

브랜치 보호 룰셋: **main-protection**(id `18893642`, active). PR 필수 + 6개 CI 체크 required(build·test·clippy 3-OS / dashboard / fmt / release feature combo) + non-fast-forward + **Admin bypass:always**. 실제 teeth 확인됨 - #112가 windows CI 끝날 때까지 mergeState=BLOCKED였고 다 green 뒤 정상 머지.

배포: **win mesh만** 새 릴리스 바이너리(`target/release/tunaround.exe`, semantic·engines·a2a-out 포함)로 `restart-win-mesh.ps1 -SourceBin` 배포 완료. mesh.pids=`39244(broker) 18520(scanner) 44688(codex-relay 새코드) 38324(watch-results)`. 라이브 확인: relay가 테스트 task 2건(2+2=4, 7×6=42)을 claim->주입->codex 답변->completed 정상 처리.

## 다음 세션 첫 행동 (우선순위)

1. **mac relay 재배포**(win만 배포됨, mac은 아직 옛 바이너리로 relay 실행 중). mac에서 `git pull` -> `cargo build --release --features "semantic morphology mcp serve worker engines a2a-out" --bin tunaround` -> `bash ~/.tunaround/restart-mac-mesh.sh -SourceBin <새 exe>`. 또는 win에서 mac-claude 세션(online)에 A2A task로 위임 가능.
2. **v0.5.0 릴리스 준비 때 B-2·B-3 묶어서**: (B-2) 대시보드 릴리스 포함 = `dist-workspace.toml [dist] features`에 dashboard 추가 + release.yml build-setup으로 setup-node/npm build 주입(frontend/dist가 gitignore라 릴리스 체크아웃에 없음) / (B-3) 라이선스 NOTICE = cargo-about로 THIRD-PARTY-NOTICES 생성 + dist include. **둘 다 실제 릴리스 run으로만 검증**되니 v0.5.0 준비 세션에서 함께. 릴리스 태그 push는 승인 예외(도그푸딩 후).
3. 여유 있으면 relay 완전 동시성(#8의 잔여 = 같은 머신 여러 codex 세션 병렬 배달, 현재는 세션당 순차)·watch-results at-least-once(#38 서버 Lagged 워터마크 갭)는 후속 후보.

## 미커밋/브랜치/백그라운드

- 미커밋 없음. 현재 브랜치 main. 원격/로컬 열린 브랜치 0(전부 머지·삭제).
- 백그라운드: 세션 시작 시 건 **A2A 수신 Monitor(poll --agent 84692f4c ...)** 가 이 세션에 상주 중이었다(새 세션은 SessionStart 훅이 자동 재무장). 각종 검증/CI 모니터는 전부 종료됨.

## 이번 세션 확정된 결정·교훈 (재론 금지)

- **브랜치 보호 = admin-bypass 룰셋**으로 확정. paths-ignore(macOS 비용) 유지 + 관리자 override 가능. 강한 강제(owner도 CI green 없이 못 머지)는 paths-ignore 제거 + bypass 제거 필요라 비용 트레이드오프로 보류(사용자 선택 남김). **주의: 이제 `gh pr merge`는 6개 required 체크가 다 green이어야 통과(BLOCKED->CLEAN). main 직접 push는 admin bypass로만.**
- **GitHub Actions 일시 장애**("Failed to resolve action download info. Service Unavailable")로 #102·#103·main CI가 스퓨리어스 실패. **로그로 인프라 사유 확인 -> `gh run rerun <id> --failed`로 재실행하면 green.** 코드 문제로 오판 금지.
- **중앙 검증에 `cargo fmt --all -- --check` 필수**(초기에 빠뜨려 CI fmt 게이트에서 여러 PR이 튕김). 이후 매 검증에 포함.
- **relay 라이브 검증 방법**: broker 무중단으로 relay만 교체(기존 relay PID kill -> 새 바이너리로 codex-relay 기동, env는 ~/.tunaround/config 소싱) -> /a2a SendMessage로 codex 세션에 테스트 task -> task get으로 completed 관찰. 검증 후 `restart-win-mesh.ps1 -SourceBin`으로 정식 배포. **restart-win-mesh는 wall-time이 2분+ 걸릴 수 있어 bash 타임아웃이 나도 데몬은 detached라 완료됨 - mesh.pids·포트로 결과 확인.**
- **codex app-server에 turn 취소(interrupt) API 없음**(codex_appserver.rs grep 0건). #9는 완전 취소 불가라 fail 사유에 "서버측 턴 계속 실행 가능" 명시로 대응(프로토콜 지원 전까지).
- **봇 오판 식별**: gemini의 `u64::is_multiple_of` HIGH·`ws_reachable` cfg HIGH는 오판(clippy 1.94가 is_multiple_of 권장, codex_relay 모듈 전체가 이미 worker-gated). DeepSource `/tmp` 리터럴 경고도 프리픽스 상수 오탐(자문성). 근거로 기각.
- 리뷰 정본: 스크래치패드에 `tunaround-review-2026-07-13.md`(103건 전문)로 사용자 전달됨. 남은 미패치는 대부분 B-2/B-3(릴리스) + RustSec `encoding`(lindera 업스트림 몫, 취약점 아직 없음, audit 잡이 RUSTSEC-2021-0153 무시로 감시).
