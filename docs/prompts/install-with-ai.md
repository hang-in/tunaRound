# AI로 설치하기 (제일 쉬운 경로)

> 새 머신에서 tunaRound를 세울 때 가장 쉬운 방법은 이미 쓰고 있는 AI 에이전트에게 맡기는 것입니다.
> 아래 "붙여넣을 프롬프트" 블록을 그 머신의 Claude Code(또는 Codex 등) 세션에 그대로 붙여넣으세요.
> AI가 OS를 감지하고, 바이너리를 설치하고, `tunaround init`으로 설정을 스캐폴드하고, `tunaround doctor`로
> 진단해 흔한 실패를 고쳐줍니다. 사람은 코어 주소와 토큰만 정하면 됩니다.
>
> 사실 근거는 [온보딩 가이드](../reference/onboarding.md)와 [mesh 아키텍처](../reference/mesh-architecture.md)에 있고,
> AI가 그 문서를 읽어 판단합니다. 공개 레포이므로 실제 토큰·IP·도메인은 이 파일에 넣지 않습니다(플레이스홀더만).

---

## 붙여넣을 프롬프트

아래 전체를 복사해 새 세션에 붙여넣으세요(레포 루트에서 실행 중이면 더 좋습니다).

```
너는 이 머신에 tunaRound(터미널 에이전트 오케스트레이터, https://github.com/hang-in/tunaRound)를 설치·설정하는 걸 돕는다.
먼저 docs/reference/onboarding.md 와 docs/reference/mesh-architecture.md 를 읽고(레포가 없으면 GitHub에서 확인) 아래 절차를 따른다.

목표를 한 가지만 나에게 물어 정한다:
  (a) 로컬에서 설계 토론·검색만 쓴다        → REPL만
  (b) 이 머신이 브로커·대시보드를 호스팅한다  → 호스트
  (c) 기존 브로커(다른 머신)에 워커/세션으로 합류한다 → 합류

절차:
1. OS를 감지한다(win/mac/linux). 설치 채널을 고른다:
   - mac/linux: `brew install hang-in/tap/tunaround` 또는 curl 설치 스크립트
   - windows: powershell irm 설치 스크립트
   - 소스 빌드가 필요하면(대시보드 등) `cargo` + `npm run build`(frontend)
   설치 명령은 실행 전 나에게 무엇을 실행할지 보여주고 확인을 받는다.
2. `tunaround chat` 이 뜨는지 확인한다(설치 성공 판정). (a)면 여기서 끝. `claude`/`codex` CLI가
   설치·로그인돼 있어야 함을 안내한다.
3. (b)/(c)면 설정을 스캐폴드한다:
   - (b) 호스트:   `tunaround init --machine <감지값>`            (core=self 기본)
   - (c) 합류:     `tunaround init --core http://<코어-IP>:8770/mcp --machine <감지값>`
                   코어 IP/주소는 나에게 물어본다.
   이 명령이 node.toml 과 ~/.tunaround/config 를 함께 만든다.
4. 토큰은 절대 이 대화창에 붙여넣게 하지 마라(로그 유출 방지). 대신:
   - 나에게 `~/.tunaround/config` 파일을 열어 TUNA_BROKER_TOKEN 값을 직접 채우라고 안내하거나,
   - 셸에 `export TUNA_BROKER_TOKEN=<토큰>`(PowerShell `$env:TUNA_BROKER_TOKEN="..."`)을
     내가 직접 입력하도록 안내한다.
   토큰 값 자체를 너가 읽거나 커맨드 argv에 넣지 않는다.
5. `tunaround doctor` 를 실행하고 결과(OK/WARN/FAIL)를 해석해 설명한다. 흔한 실패 처방:
   - 러너 not on PATH → 해당 CLI(claude/codex/opencode) 설치·로그인 안내
   - 코어 도달 불가 → 코어 IP·네트워크(같은 LAN이면 사설 IP, 아니면 터널)·토큰 확인
   - 기본 빌드에 serve/node/doctor 없음 → 릴리스 바이너리 또는 올바른 --features로 재설치
6. (b)/(c)면 상주: `tunaround node`(또는 mesh 전체는 restart 스크립트). 다 되면 로스터/대시보드로
   확인하고 요약한다.

규칙:
- 시스템 설정 변경·설치·재기동 등 영향 있는 단계는 실행 전 확인을 받는다.
- 비밀(토큰)은 화면·로그·argv에 남기지 않는다. 실제 IP/토큰/도메인은 공유 출력에 쓰지 않는다.
- 모르면 지어내지 말고 onboarding.md 를 근거로 하거나 나에게 묻는다.
```

---

## 왜 이게 제일 쉬운가

- 사람이 정할 것은 **딱 두 가지**입니다: (1) 이 머신의 역할(로컬/호스트/합류), (2) 합류면 코어 주소 + 토큰.
- 나머지(OS별 설치 명령, 피처 플래그, 설정 파일 3종, mac/win 차이, doctor 해석)는 AI가 문서를 읽고 대신 처리합니다.
- `tunaround init`이 이미 설정을 원커맨드로 스캐폴드하므로, AI는 그걸 올바른 인자로 운전하기만 하면 됩니다.

수동으로 하고 싶으면 [온보딩 가이드](../reference/onboarding.md)의 3갈래를 그대로 따라도 됩니다.
