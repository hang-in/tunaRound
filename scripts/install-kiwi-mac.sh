#!/usr/bin/env bash
# macOS에서 Kiwi v0.22.2 lib+model tgz를 kiwi-rs 캐시 downloads/에 선주입하는 스크립트.
#
# 왜 필요한가: kiwi-rs 0.1.4의 bootstrap 자산 URL 파서 버그(GitHub 에셋 객체의 중첩 uploader{}에서
# 잘림)로 mac 자동다운로드가 항상 실패한다. 또한 최신(v0.23.2) libkiwi는 kiwi-rs 0.1.4 바인딩과
# ABI 불일치로 토큰화 SIGSEGV가 난다. 그래서 (1) ABI가 맞는 v0.22.2를 (2) downloads/에 tgz로 선주입해
# kiwi-rs의 download_release_asset이 파서 호출 전에 조기반환하도록 우회한다. install-kiwi-windows.sh와
# 같은 pre-seed 패턴이며, 재빌드가 필요 없다(권장안 A).
#
# ★ 이 스크립트만으로는 부족하다: kiwi-rs 기본 버전이 최신(v0.23.x=크래시)이라 실행 시
#   KIWI_RS_VERSION=v0.22.2 를 반드시 env로 지정해야 v0.22.2 캐시를 쓴다(아래 안내 참조).

set -euo pipefail

KIWI_VERSION="v0.22.2"
KIWI_VER_BARE="${KIWI_VERSION#v}"   # 캐시 디렉터리는 v 없는 버전(0.22.2)을 쓴다.

if ! command -v gh &>/dev/null; then
  echo "[kiwi-install] 오류: gh CLI가 필요합니다(자산 다운로드). https://cli.github.com 설치 후 재실행하세요." >&2
  exit 1
fi

# kiwi-rs 캐시 루트: ~/Library/Caches/kiwi-rs/<version>/downloads/
# (kiwi-rs 0.1.4 resolve_cache_root 실소스 기준: macOS는 XDG_CACHE_HOME을 보지 않고 $HOME/Library/Caches
#  고정, 유일한 override는 KIWI_RS_CACHE_DIR - 스크립트도 정확히 그 규칙만 미러링한다. gemini 리뷰 반영.)
CACHE_ROOT="${KIWI_RS_CACHE_DIR:-$HOME/Library/Caches}/kiwi-rs/$KIWI_VER_BARE"
DOWNLOADS_DIR="$CACHE_ROOT/downloads"

# 아키텍처 분기(arm64 / x86_64).
ARCH="$(uname -m)"
case "$ARCH" in
  arm64|aarch64) LIB_ASSET="kiwi_mac_arm64_${KIWI_VERSION}.tgz" ;;
  x86_64)        LIB_ASSET="kiwi_mac_x86_64_${KIWI_VERSION}.tgz" ;;
  *) echo "[kiwi-install] 오류: 지원하지 않는 아키텍처: $ARCH" >&2; exit 1 ;;
esac
MODEL_ASSET="kiwi_model_${KIWI_VERSION}_base.tgz"

echo "[kiwi-install] arch=$ARCH  버전=$KIWI_VERSION"
echo "[kiwi-install] downloads/: $DOWNLOADS_DIR"

# ── 멱등 체크: 두 tgz가 이미 있으면 건너뛴다 ──────────────────────────────────
if [ -f "$DOWNLOADS_DIR/$LIB_ASSET" ] && [ -f "$DOWNLOADS_DIR/$MODEL_ASSET" ]; then
  echo "[kiwi-install] 이미 선주입돼 있습니다. 건너뜁니다."
  echo "[kiwi-install]   lib:   $DOWNLOADS_DIR/$LIB_ASSET"
  echo "[kiwi-install]   model: $DOWNLOADS_DIR/$MODEL_ASSET"
else
  mkdir -p "$DOWNLOADS_DIR"
  echo "[kiwi-install] bab2min/Kiwi $KIWI_VERSION 자산 다운로드 중..."
  gh release download "$KIWI_VERSION" \
    --repo bab2min/Kiwi \
    --pattern "$LIB_ASSET" \
    --pattern "$MODEL_ASSET" \
    --dir "$DOWNLOADS_DIR" \
    --clobber
  # 다운로드 검증(gh가 조용히 스킵했는지 방어).
  for f in "$LIB_ASSET" "$MODEL_ASSET"; do
    if [ ! -f "$DOWNLOADS_DIR/$f" ]; then
      echo "[kiwi-install] 오류: 자산을 받지 못했습니다: $f" >&2
      exit 1
    fi
  done
  echo "[kiwi-install] 선주입 완료."
fi

echo ""
echo "[kiwi-install] 다음 실행부터 아래 env 로 v0.22.2 캐시를 쓰게 하세요(기본값=최신=크래시):"
echo "    export KIWI_RS_VERSION=$KIWI_VERSION"
echo "[kiwi-install] 확인: KIWI_RS_VERSION=$KIWI_VERSION tunaround doctor  ->  'OK morphology: Kiwi 로드됨'"
echo "[kiwi-install] (kiwi-rs가 첫 init 때 downloads/의 tgz를 lib/·models/ 로 추출합니다.)"
