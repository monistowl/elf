#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

VERSION="${1:-latest}"
PREFIX="${PREFIX:-$HOME/.local}"
OPT="$PREFIX/opt/elf"
BIN="$PREFIX/bin"
mkdir -p "$OPT" "$BIN"

OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
esac

BASE_URL="${BASE_URL:-https://example.com/elf/releases}"
if [ "$VERSION" = "latest" ]; then
  VERSION=$(curl -fsSL "$BASE_URL/LATEST.txt")
fi
FILE="elf-${VERSION}-${ARCH}-${OS}.tar.xz"
TMP=$(mktemp --suffix="-$FILE")

LOCAL_RELEASE_DIR="${LOCAL_RELEASE_DIR:-$REPO_ROOT/release}"
LOCAL_TARBALL="$LOCAL_RELEASE_DIR/$FILE"
LOCAL_SHA="$LOCAL_RELEASE_DIR/${FILE}.sha256"

EXPECTED=""
if [ -f "$LOCAL_TARBALL" ]; then
  echo "Using local release artifact $LOCAL_TARBALL"
  cp "$LOCAL_TARBALL" "$TMP"
  if [ -f "$LOCAL_SHA" ]; then
    EXPECTED=$(awk '{print $1}' "$LOCAL_SHA")
  else
    echo "Warning: local SHA256 file not found at $LOCAL_SHA; skipping hash validation"
  fi
else
  URL="$BASE_URL/$VERSION/$FILE"
  SHASUM_URL="$URL.sha256"
  curl -fsSL "$URL" -o "$TMP"
  SHASUM_TMP=$(mktemp)
  if curl -fsSL "$SHASUM_URL" >"$SHASUM_TMP"; then
    EXPECTED=$(awk '{print $1}' "$SHASUM_TMP")
  fi
  rm -f "$SHASUM_TMP"
fi

if [ -n "$EXPECTED" ]; then
  if command -v sha256sum >/dev/null; then
    COMPUTED=$(sha256sum "$TMP" | awk '{print $1}')
  elif command -v shasum >/dev/null; then
    COMPUTED=$(shasum -a 256 "$TMP" | awk '{print $1}')
  else
    echo "Warning: checksum verifier not found; skipping hash validation"
    COMPUTED="$EXPECTED"
  fi
  if [ "$COMPUTED" != "$EXPECTED" ]; then
    rm -f "$TMP"
    echo "Checksum mismatch for $FILE"
    exit 1
  fi
fi
DEST="$OPT/$VERSION"
rm -rf "$DEST" && mkdir -p "$DEST"
tar -xJf "$TMP" -C "$DEST"
rm -f "$TMP"
ln -sf "$DEST" "$OPT/current"
for exe in elf elf-gui elf-run; do
  ln -sf "$OPT/current/bin/$exe" "$BIN/$exe"
done

cat <<INFO
Installed ELF $VERSION to $DEST
Symlinks placed in $BIN (ensure it is on your PATH).
INFO
