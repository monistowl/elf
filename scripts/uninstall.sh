#!/usr/bin/env bash
set -euo pipefail
PREFIX="${PREFIX:-$HOME/.local}"
OPT="$PREFIX/opt/elf"
BIN="$PREFIX/bin"
for exe in elf elf-gui elf-run; do
  rm -f "$BIN/$exe"
done
rm -f "$OPT/current"
echo "Uninstalled ELF assets; remove $OPT if you want to clear cached versions."
