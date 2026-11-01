#!/bin/bash
# Cross-compile Picline for Windows from a Mac and package the two exes.
# Usage: scripts/build-win.sh [x64|arm64|both]
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

VERSION="${PICLINE_VERSION:-0.1.1}"
WHICH="${1:-both}"
DIST="dist"
mkdir -p "$DIST"

build() {
  local target="$1" suffix="$2"
  echo "==> $target"
  cargo build --release --target "$target"
  cp "target/$target/release/picline.exe" "$DIST/Picline-$VERSION-$suffix.exe"
  ls -l "$DIST/Picline-$VERSION-$suffix.exe"
}

case "$WHICH" in
  x64)   build x86_64-pc-windows-gnullvm win-x64 ;;
  arm64) build aarch64-pc-windows-gnullvm win-arm64 ;;
  both)
    build x86_64-pc-windows-gnullvm win-x64
    build aarch64-pc-windows-gnullvm win-arm64
    ;;
  *) echo "Usage: $0 [x64|arm64|both]" >&2; exit 2 ;;
esac

echo "Done. Artifacts in $DIST/"
