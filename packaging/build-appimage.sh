#!/bin/sh
# Builds dist/Airscope-x86_64.AppImage.
# The compile runs in an Ubuntu 22.04 container so the binary only needs glibc 2.35 (runs on most distros
# from 2022 on). Needs podman. The GUI libraries (Wayland/X11/GL) are loaded from the host at runtime.
set -eu
cd "$(dirname "$0")/.."
TOOL_VER=1.9.0
TOOL_SHA256=46fdd785094c7f6e545b61afcfb0f3d98d8eab243f644b4b17698c01d06083d1
mkdir -p dist packaging/.cache

if [ "${NO_CONTAINER:-}" = 1 ]; then
  # CI: the runner is already Ubuntu 22.04
  CARGO_TARGET_DIR=target-appimage cargo build --release --locked
  strip target-appimage/release/airscope
else
  podman run --rm -v "$PWD":/src:Z -w /src ubuntu:22.04 sh -c '
    set -eu
    apt-get update -qq
    DEBIAN_FRONTEND=noninteractive apt-get install -y -qq curl ca-certificates build-essential pkg-config >/dev/null
    curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal >/dev/null
    . "$HOME/.cargo/env"
    CARGO_TARGET_DIR=/src/target-appimage cargo build --release --locked
    strip /src/target-appimage/release/airscope
  '
fi

TOOL=packaging/.cache/appimagetool-$TOOL_VER
if [ ! -x "$TOOL" ]; then
  curl -sSfL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/$TOOL_VER/appimagetool-x86_64.AppImage"
  echo "$TOOL_SHA256  $TOOL" | sha256sum -c - || { rm -f "$TOOL"; echo "appimagetool checksum mismatch" >&2; exit 1; }
  chmod +x "$TOOL"
fi

rm -rf dist/AppDir
mkdir -p dist/AppDir/usr/bin
cp target-appimage/release/airscope dist/AppDir/usr/bin/
cp packaging/airscope.desktop packaging/airscope.png packaging/airscope.svg dist/AppDir/
cp LICENSE dist/AppDir/ 2>/dev/null || true
printf '#!/bin/sh\nexec "$(dirname "$(readlink -f "$0")")/usr/bin/airscope" "$@"\n' > dist/AppDir/AppRun
chmod +x dist/AppDir/AppRun
ARCH=x86_64 "$TOOL" --appimage-extract-and-run dist/AppDir dist/Airscope-x86_64.AppImage
echo "Built dist/Airscope-x86_64.AppImage"
