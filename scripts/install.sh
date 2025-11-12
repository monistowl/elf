#!/usr/bin/env bash
set -euo pipefail

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
URL="$BASE_URL/$VERSION/$FILE"
SHASUM_URL="$URL.sha256"

TMP=$(mktemp --suffix="-$FILE")
curl -fsSL "$URL" -o "$TMP"
if curl -fsSL "$SHASUM_URL" >/tmp/elf-sha256.txt; then
  (cd "$OPT" && sha256sum -c /tmp/elf-sha256.txt) || (rm -f "$TMP" && exit 1)
fi
DEST="$OPT/$VERSION"
rm -rf "$DEST" && mkdir -p "$DEST"
tar -xJf "$TMP" -C "$DEST"
ln -sf "$DEST" "$OPT/current"
for exe in elf elf-gui elf-run; do
  ln -sf "$OPT/current/bin/$exe" "$BIN/$exe"
done

cat <<INFO
Installed ELF $VERSION to $DEST
Symlinks placed in $BIN (ensure it is on your PATH).
INFO
