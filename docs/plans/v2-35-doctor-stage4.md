# Plan v2-35: doctor Stage 4 갭 채우기 (Kiwi/형태소 + Ollama 도달)

> 세션12(2026-07-04). 배포·온보딩 [설계 §C](../design/v2-deploy-onboarding_2026-07-02.md)의 doctor 프리플라이트 잔여 항목. 기존 `run_doctor`(세션9, node.toml 기반)에 **additive**로 두 갭만 채운다.

## 정찰 결론 (이미 있는 것 vs 갭)

기존 `run_doctor`(src/main.rs:452, serve+worker 게이트, node.toml 필수)가 **이미 커버**:
- config 로드/파싱 · token · core=self 포트 바인드/브로커 응답/db 상위디렉터리 · core=remote agent-card 도달+bearer · 레인별 러너 PATH(claude/codex/opencode) · http/a2a 필수설정+피처 게이트 · project 디렉터리.

설계 §C 대비 **남은 갭 2개**:
1. **Ollama/http 도달**: 현재 http 레인은 `http_base_url` 존재만 확인, 실제 ping 안 함.
2. **Kiwi/형태소 백엔드 상태**: 전혀 미확인. "Kiwi 자동다운로드 성공"(§C) 검증 없음.

## 설계 결정

- **기존 `run_doctor` 확장(신규 서브커맨드 아님).** node.toml 기반 유지. config-less 일반 프리플라이트 모드는 후속(YAGNI).
- **claude/codex 인증 심층 검증(로그인 여부)은 비범위.** CLI 실행이 필요해 취약(행·latency). PATH 확인 유지. 후속.
- Kiwi probe는 반환값으로 kiwi/lindera 구분이 안 되므로(폴백을 삼킴) `Tokenizer` trait에 `backend_name()` 추가가 선행.

## 태스크

### T1: Tokenizer::backend_name() + doctor Kiwi/형태소 probe
- `src/search/tokenizer.rs`: `Tokenizer` trait에 `fn backend_name(&self) -> &'static str` 추가. `KiwiTokenizer`="kiwi", `LinderaKoTokenizer`="lindera". (tokenize_fallback 전용 더미 impl 있으면 "fallback".)
- `src/main.rs run_doctor`: 형태소 백엔드 probe를 **레인 루프와 무관하게 1회**(환경 레벨) 출력.
  - `#[cfg(feature = "morphology")]`: `create_tokenizer("kiwi")` 호출 → `.backend_name()`이 "kiwi"면 `OK morphology: Kiwi 로드됨(자동다운로드/캐시 성공)`, "lindera"면 `WARN morphology: Kiwi 폴백=lindera(형태소 품질 저하, install-kiwi 스크립트 참고)`. (FAIL 아님 - lindera 폴백은 동작하므로.)
  - `#[cfg(not(feature = "morphology"))]`: `WARN morphology: 미빌드(FTS는 fallback 토크나이저 사용)`.
- 위치: config 로드 성공 직후(레인 루프 전), fails에 영향 없음(WARN만).

### T2: doctor http 레인 Ollama 도달 ping
- `run_doctor`의 기존 `"http"` 레인 분기(`#[cfg(feature="engines")]`, http_base_url Some 경로)에서, base_url이 있으면 **짧은 타임아웃(3s) reqwest::blocking GET**으로 도달 확인.
  - 도달(HTTP 응답 아무거나) → `OK lane .. runner=http: base_url {u} 도달`.
  - 도달 실패(연결 거부/타임아웃) → `WARN lane .. runner=http: base_url {u} 도달 불가(LLM 미기동?)`. (FAIL 아님 - 콜드 스타트/나중 기동 가능. 설정 자체는 유효.)
- reqwest::blocking은 이미 run_doctor에서 사용 중(core 도달 체크). 신규 의존 0.
- ping 대상 = base_url 그대로 GET(OpenAI-compat 서버는 보통 200/404 응답=도달). `/v1/models` 등 경로 가정 안 함(서버별 상이).

## 검증
- `cargo build`/`test`/clippy 기본 + `--features "morphology mcp serve worker engines"`(doctor+kiwi+ollama 경로 포함). 베이스라인 414.
- `backend_name` 단위테스트(lindera="lindera"). Kiwi는 런타임 자산 의존이라 단위테스트 없이 doctor 라이브 스모크로.
- 라이브 스모크: 임의 node.toml로 `tunaround doctor` 실행 → 형태소 줄 + (http 레인 있으면) Ollama 도달 줄 확인.

## 비범위
- config-less 일반 프리플라이트 모드 · claude/codex 인증 심층(로그인) 검증 · Kiwi 자동 설치 트리거(진단만).
