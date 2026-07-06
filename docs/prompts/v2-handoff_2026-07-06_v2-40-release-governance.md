# 핸드오프: v2-40 완성 + 0.3.0 릴리스 + 토큰 로테이션 + 거버넌스 (2026-07-06 세션15)

> WIN 핸드오프. **라이브 접속값(브로커/토큰/PID)은 gitignored `docs/reference/backend-private.md` 하단을 먼저 읽어라.** 토큰은 이제 **User env `TUNA_BROKER_TOKEN`(setx)**에만 있다(파일·argv 평문 없음). 레포 PUBLIC=평문 금지.

## 이 세션(15)에 한 것

1. **세션14 잔여 머지**: PR #11(IP redact)·#12(대시보드) → main.
2. **v2-40 유니버설 세션 버스 완성**(PR #13 머지):
   - **S1 자동무장 훅**(`.claude/hooks/tuna-autoarm.py`/`tuna-disarm.py` + `.claude/settings.json`): opt-in `TUNA_AUTOARM=1` → detached `tunaround poll`(register+heartbeat) → 세션이 로스터 등장. uuid=세션id + display_name.
   - **S2 발견 리포터**: 브로커 candidate 저장/조회(`report_candidates`/`list_candidates` + `GET /dashboard/candidates`, armed overlay) + `tunaround discover`(로컬 claude 세션 열거, machine 속성). **automation 노이즈 필터**: claude-mem observer(cwd=`~/.claude-mem/`) + secall 위키/저널(첫 user 메시지 `<!--` 마커) 제외.
   - **S3 후보 패널**: 대시보드 "발견된 세션" + "연결"(그 세션에 붙여넣을 arm 프롬프트 팝업).
   - **S4 codex 직접 제어**: `POST /dashboard/control`(loopback) → in-process `codex_inject::run` turn/start.
   - **대시보드/UX**: 라이브 피드를 **task별 카드**로(상태 갱신+클릭 이력 펼침). 명칭 **총괄/관리자/실무자**(구 총감독/감독/워커).
   - **보안(리뷰 반영)**: `/dashboard/goal`·`/dashboard/control` local CSRF(Sec-Fetch-Site) + control ws loopback(SSRF) + `serve`/`poll`/`discover` `--token` env 폴백(argv 노출 제거) + 훅 하드닝.
3. **토큰 유출 대응**: 평문 커밋된 옛 토큰 발견 → git 히스토리 filter-branch 퍼지 → **로테이션**(새 토큰=User env, 파일/argv 평문 0) → **양 머신 env 기반 재기동**(win·mac 워처 전부 `--token` 없음 확인). 옛 토큰 폐기.
4. **정체 규명(도그푸딩)**: `observer-sessions`=claude-mem observer(맥), `저널` 0-turn=secall 위키/저널 자동화(윈). → discover automation 필터로 반영.
5. **버전 관리 정상화**: 0.2.2 이후 15커밋을 버전 안 올려 쌓은 것(exit 2 근본원인) 해소 → **0.3.0 bump + CHANGELOG + v0.3.0 릴리스**(cargo-dist 6타깃 + brew). 양 머신 0.3.0 설치.
6. **거버넌스 규약**: CLAUDE.md "총괄/관리자/실무자 협업 위계" 추가 — 총괄=main 머지 독점+공유파일 소유 / 관리자=브랜치 PR / 실무자=1 task 1 worktree. A2A 브로커=조율 레이어. mac도 채택(자신=관리자 인지).

## 라이브 상태 (backend-private 참조)

- 4자 로스터 online: win-opus-boss(총괄/boss) + mac-claude-sup·mac-codex-sup·win-codex-sup(관리자/supervised). 전부 env 토큰(argv 노출 0).
- win detached: broker·win-codex-sup watcher·discover·win-opus-boss poll. 재부팅 시 소멸(backend-private 재기동 절차).
- 대시보드 http://127.0.0.1:8770/dashboard 라이브(카드 피드·후보 패널·연결 팝업·codex 제어 폼).

## 다음 후보 (급하지 않음)

- **role 태그 값까지 명칭 통일**(boss/supervised/worker → 총괄/관리자/실무자): 셀렉터·autoarm 기본값·워처 명령 영향, 별도 마이그레이션.
- **arm 팝업 정교화**: 크로스머신 core 주소(뷰어 origin≠대상 머신) + 대상 머신 바이너리 버전 전제(0.3.0). "연결"의 실제 arm 왕복 라이브 검증.
- **온보딩 스무더**: `doctor` 확장(토큰 env/바이너리 버전/브로커 도달 진단).
- codex app-server(win) 옛 토큰 env면 재기동 필요(win-codex-sup codex 처리 복구).

## 첫 행동

1. `docs/reference/backend-private.md` 하단 라이브값 확인. 죽었으면 그 블록 커맨드로 재기동(**전부 env 토큰, `--token` argv 금지**). 브로커 listen 확인 후 watcher(레이스 회피).
2. 규율: 구현 위임 ①tunaLlama ②A2A codex ③Sonnet(프론트/버전UI는 Opus/Sonnet), 아키텍트·리뷰=Opus. GitHub Flow + 3-OS CI + 봇 리뷰(**PR 던지면 CodeRabbit/Gemini 리뷰 오니 확인·반영**). 거버넌스 규약(총괄/관리자/실무자) 준수. 레포 PUBLIC=평문 토큰/LAN IP 금지.
