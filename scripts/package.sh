#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:-$(git describe --tags --always)}"
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
ARCH=$(uname -m)
case "$ARCH" in
  x86_64|amd64) ARCH=x86_64 ;;
  aarch64|arm64) ARCH=aarch64 ;;
esac

TARGET_DIR="release/${VERSION}"
mkdir -p "$TARGET_DIR/bin"

cargo build --release --workspace

cp target/release/elf "$TARGET_DIR/bin/elf"
cp target/release/elf-gui "$TARGET_DIR/bin/elf-gui"
cp target/release/elf-run "$TARGET_DIR/bin/elf-run"

FILE="elf-${VERSION}-${ARCH}-${OS}.tar.xz"
DEST="release/$FILE"
rm -f "$DEST"
tar -cJf "$DEST" -C release "${VERSION}"
sha256sum "$DEST" > "release/${FILE}.sha256"

echo "Packaged $DEST"
