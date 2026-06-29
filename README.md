# tunaRound

터미널에서 **Codex CLI와 Claude Code가 토론으로 의견을 교환하며 협업**하는 앱.

여러 AI 코딩 에이전트를 한 대화에 앉히고, 구조화된 라운드로 토론하게 한 뒤, 토론이 수렴하면 결론짓습니다. 멀티세션(동시 진행·관찰·재개)은 Redis로 코디네이션합니다. 최종 목표는 두 에이전트의 **협업 코딩**이고, 그 앞단에 "서로 의견을 교환하는 토론"을 둡니다.

## 위치

- **에이전트 구동 + 토론**은 [tunaFlow](https://github.com/hang-in/tunaFlow)(데스크톱 오케스트레이터)에서, **멀티세션·대화 흐름**은 [tunaSalon](https://github.com/hang-in/tunaSalon)에서 검증된 것을 터미널 네이티브로 결합합니다.
- 흐름 엔진은 "언제 토론이 수렴했나(이제 결론 낼 때인가)"를 판단하는 데 씁니다.

## 상태

**설계 단계.** 승인된 설계는 [docs/design/tunaRound-v1-design.md](docs/design/tunaRound-v1-design.md)에 있습니다. 구현은 다음 단계입니다.

## 스택 (예정)

Rust + tokio + ratatui + redis + rusqlite.
