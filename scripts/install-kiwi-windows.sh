#!/usr/bin/env bash
# Windows에서 Kiwi v0.22.2 libkiwi + base 모델을 %LOCALAPPDATA%\kiwi에 설치하는 스크립트.

set -euo pipefail

if ! command -v gh &>/dev/null; then
  echo "[kiwi-install] 오류: gh CLI가 필요합니다(자산 다운로드). https://cli.github.com 설치 후 재실행하세요." >&2
  exit 1
fi

KIWI_VERSION="v0.22.2"
# Windows LOCALAPPDATA를 bash 경로로 변환.
# Git Bash/MSYS2: $LOCALAPPDATA는 환경에 있음. 없으면 기본 경로 추정.
if [ -z "${LOCALAPPDATA:-}" ]; then
  LOCALAPPDATA="$USERPROFILE/AppData/Local"
fi
# MSYS/Cygwin 경로 변환 (ex: C:\Users\... -> /c/Users/...).
if command -v cygpath &>/dev/null; then
  KIWI_DIR="$(cygpath -u "$LOCALAPPDATA")/kiwi"
else
  # Git Bash: 슬래시 변환 없이 그대로 쓰면 mkdir 실패할 수 있으므로 변환.
  KIWI_DIR="${LOCALAPPDATA//\\//}/kiwi"
fi

LIB_DIR="$KIWI_DIR/lib"
MODEL_DIR="$KIWI_DIR/models/cong/base"

echo "[kiwi-install] 대상 경로: $KIWI_DIR"

# ── 멱등 체크 ──────────────────────────────────────────────────────────────────
# MODEL_DIR은 아래에서 tgz 추출 전에 먼저 mkdir로 생기므로, 디렉터리 존재만 보면 dll은 성공했으나
# 모델 추출이 실패한 반쪽 설치(dll+빈 MODEL_DIR)를 "이미 설치됨"으로 오판해 건너뛴다. 실제로 모델
# 파일이 들어있는지(비어있지 않은지)까지 확인한다.
if [ -f "$LIB_DIR/kiwi.dll" ] && [ -d "$MODEL_DIR" ] && [ -n "$(ls -A "$MODEL_DIR" 2>/dev/null)" ]; then
  echo "[kiwi-install] 이미 설치돼 있습니다. 건너뜁니다."
  echo "[kiwi-install] dll: $LIB_DIR/kiwi.dll"
  echo "[kiwi-install] 모델: $MODEL_DIR"
  exit 0
fi

mkdir -p "$LIB_DIR" "$MODEL_DIR"

TMPDIR_WORK="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_WORK"' EXIT

echo "[kiwi-install] $KIWI_VERSION 다운로드 중..."
gh release download "$KIWI_VERSION" \
  --repo bab2min/Kiwi \
  --pattern "kiwi_win_x64_${KIWI_VERSION}.zip" \
  --pattern "kiwi_model_${KIWI_VERSION}_base.tgz" \
  --dir "$TMPDIR_WORK" \
  --clobber

# ── dll 추출 ──────────────────────────────────────────────────────────────────
ZIP_FILE="$TMPDIR_WORK/kiwi_win_x64_${KIWI_VERSION}.zip"
if [ ! -f "$ZIP_FILE" ]; then
  echo "[kiwi-install] 오류: zip 파일을 찾을 수 없습니다: $ZIP_FILE" >&2
  exit 1
fi
echo "[kiwi-install] kiwi.dll 추출 중..."
# zip 내부 배치는 lib/kiwi.dll(v0.22.2 실측). -j로 경로를 벗겨 $LIB_DIR/kiwi.dll로 놓는다.
# unzip이 없는 Git Bash 배포판이 있어 PowerShell Expand-Archive 폴백을 둔다(gemini 리뷰).
if command -v unzip &>/dev/null; then
  unzip -j -o "$ZIP_FILE" "lib/kiwi.dll" -d "$LIB_DIR"
else
  EXTRACT_DIR="$TMPDIR_WORK/zip-extract"
  powershell.exe -NoProfile -Command "Expand-Archive -LiteralPath '$(cygpath -w "$ZIP_FILE" 2>/dev/null || echo "$ZIP_FILE")' -DestinationPath '$(cygpath -w "$EXTRACT_DIR" 2>/dev/null || echo "$EXTRACT_DIR")' -Force"
  cp "$EXTRACT_DIR/lib/kiwi.dll" "$LIB_DIR/kiwi.dll"
fi

# ── 모델 추출 ─────────────────────────────────────────────────────────────────
TGZ_FILE="$TMPDIR_WORK/kiwi_model_${KIWI_VERSION}_base.tgz"
if [ ! -f "$TGZ_FILE" ]; then
  echo "[kiwi-install] 오류: 모델 파일을 찾을 수 없습니다: $TGZ_FILE" >&2
  exit 1
fi
echo "[kiwi-install] base 모델 추출 중..."
# tgz 내부 배치는 models/cong/base/<파일들>(3계층, v0.22.2 실측) - 3계층을 벗겨 $MODEL_DIR 바로 아래에 놓는다.
tar -xzf "$TGZ_FILE" -C "$MODEL_DIR" --strip-components=3

echo "[kiwi-install] 완료."
echo "  dll: $LIB_DIR/kiwi.dll"
echo "  모델: $MODEL_DIR"
echo ""
echo "이제 'cargo test --features morphology'를 실행하면 Kiwi로 동작합니다."
