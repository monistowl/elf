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
FILE_NAME() {
  printf "elf-%s-%s-%s.tar.xz" "$1" "$ARCH" "$OS"
}
LOCAL_RELEASE_DIR="${LOCAL_RELEASE_DIR:-$REPO_ROOT/release}"
LOCAL_BUILD_DIR="${LOCAL_BUILD_DIR:-$REPO_ROOT/target/release}"
LOCAL_BUILD_VERSION="${LOCAL_BUILD_VERSION:-dev-local}"

REQUESTED_VERSION="${1:-latest}"
INSTALL_VERSION="$REQUESTED_VERSION"
USE_LOCAL_BUILD=0
USE_TARBALL=0
ARTIFACT=""
TMP=""
EXPECTED=""

FILE=""

if [ "$REQUESTED_VERSION" != "latest" ]; then
  FILE=$(FILE_NAME "$REQUESTED_VERSION")
  LOCAL_TARBALL="$LOCAL_RELEASE_DIR/$FILE"
  LOCAL_SHA="$LOCAL_RELEASE_DIR/${FILE}.sha256"
  if [ -f "$LOCAL_TARBALL" ]; then
    echo "Using local release artifact $LOCAL_TARBALL"
    USE_TARBALL=1
    ARTIFACT="$LOCAL_TARBALL"
    if [ -f "$LOCAL_SHA" ]; then
      EXPECTED=$(awk '{print $1}' "$LOCAL_SHA")
    else
      echo "Warning: local SHA256 file not found at $LOCAL_SHA; skipping hash validation"
    fi
  fi
fi

if [ "$USE_TARBALL" -eq 0 ] && [ -x "$LOCAL_BUILD_DIR/elf" ] && [ -x "$LOCAL_BUILD_DIR/elf-gui" ]; then
  echo "Installing from local build artifacts in $LOCAL_BUILD_DIR"
  USE_LOCAL_BUILD=1
  if [ "$REQUESTED_VERSION" = "latest" ]; then
    INSTALL_VERSION="$LOCAL_BUILD_VERSION"
  fi
fi

if [ "$USE_TARBALL" -eq 0 ] && [ "$USE_LOCAL_BUILD" -eq 0 ]; then
  if [ "$REQUESTED_VERSION" = "latest" ]; then
    INSTALL_VERSION=$(curl -fsSL "$BASE_URL/LATEST.txt")
  fi
  FILE=$(FILE_NAME "$INSTALL_VERSION")
  LOCAL_TARBALL="$LOCAL_RELEASE_DIR/$FILE"
  LOCAL_SHA="$LOCAL_RELEASE_DIR/${FILE}.sha256"
  if [ -f "$LOCAL_TARBALL" ]; then
    echo "Using local release artifact $LOCAL_TARBALL"
    USE_TARBALL=1
    ARTIFACT="$LOCAL_TARBALL"
    if [ -f "$LOCAL_SHA" ]; then
      EXPECTED=$(awk '{print $1}' "$LOCAL_SHA")
    else
      echo "Warning: local SHA256 file not found at $LOCAL_SHA; skipping hash validation"
    fi
  else
    TMP=$(mktemp --suffix="-$FILE")
    URL="$BASE_URL/$INSTALL_VERSION/$FILE"
    SHASUM_URL="$URL.sha256"
    curl -fsSL "$URL" -o "$TMP"
    SHASUM_TMP=$(mktemp)
    if curl -fsSL "$SHASUM_URL" >"$SHASUM_TMP"; then
      EXPECTED=$(awk '{print $1}' "$SHASUM_TMP")
    fi
    rm -f "$SHASUM_TMP"
    ARTIFACT="$TMP"
    USE_TARBALL=1
  fi
fi

if [ "$USE_LOCAL_BUILD" -eq 0 ] && [ -n "$EXPECTED" ]; then
  if command -v sha256sum >/dev/null; then
    COMPUTED=$(sha256sum "$ARTIFACT" | awk '{print $1}')
  elif command -v shasum >/dev/null; then
    COMPUTED=$(shasum -a 256 "$ARTIFACT" | awk '{print $1}')
  else
    echo "Warning: checksum verifier not found; skipping hash validation"
    COMPUTED="$EXPECTED"
  fi
  if [ "$COMPUTED" != "$EXPECTED" ]; then
    [ -f "$ARTIFACT" ] && rm -f "$ARTIFACT"
    echo "Checksum mismatch for $FILE"
    exit 1
  fi
fi

DEST="$OPT/$INSTALL_VERSION"
rm -rf "$DEST" && mkdir -p "$DEST"
if [ "$USE_LOCAL_BUILD" -eq 1 ]; then
  mkdir -p "$DEST/bin"
  cp "$LOCAL_BUILD_DIR/elf" "$DEST/bin/elf"
  cp "$LOCAL_BUILD_DIR/elf-gui" "$DEST/bin/elf-gui"
  if [ -x "$LOCAL_BUILD_DIR/elf-run" ]; then
    cp "$LOCAL_BUILD_DIR/elf-run" "$DEST/bin/elf-run"
  else
    cat <<'WRAPPER' >"$DEST/bin/elf-run"
#!/usr/bin/env sh
HERE="$(dirname "$0")"
exec "$HERE/elf" run-simulate "$@"
WRAPPER
    chmod +x "$DEST/bin/elf-run"
  fi
else
  tar -xJf "$ARTIFACT" -C "$DEST"
  if [ -n "$TMP" ] && [ "$ARTIFACT" = "$TMP" ]; then
    rm -f "$TMP"
  fi
fi
ln -sf "$DEST" "$OPT/current"
for exe in elf elf-gui elf-run; do
  ln -sf "$OPT/current/bin/$exe" "$BIN/$exe"
done

cat <<INFO
Installed ELF $INSTALL_VERSION to $DEST
Symlinks placed in $BIN (ensure it is on your PATH).
INFO
