#!/bin/sh
# Regenerates the README screenshots in docs/screenshots/ from the built-in fake demo data and an invented
# Bluetooth capture (scripts/make-demo-capture.py). Runs the app inside a headless Wayland compositor (cage), so
# nothing shows on your screen and nothing real is captured; the data folder is a throwaway temp directory.
# Needs: cage, wlr-randr, ImageMagick (magick), python3. Build first: cargo build --release.
set -eu
cd "$(dirname "$0")/.."
out=docs/screenshots; mkdir -p "$out"
tmp=$(mktemp -d); trap 'rm -rf "$tmp"' EXIT
python3 -I scripts/make-demo-capture.py "$tmp/demo.snoop"
shot() { # name, env assignments...
  name=$1; shift
  WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 timeout 90 cage -- sh -c "wlr-randr --output HEADLESS-1 --custom-mode 1400x860 >/dev/null 2>&1; sleep 1; exec env XDG_DATA_HOME=$tmp AIRSCOPE_DEMO=1 AIRSCOPE_SHOT=$out/$name.ppm $* target/release/airscope" >/dev/null 2>&1
  magick "$out/$name.ppm" "$out/$name.png" && rm "$out/$name.ppm"
  echo "$out/$name.png"
}
shot radar
shot radar-info AIRSCOPE_SEL=AC:80:0A:3D:62:C0
shot spectrum AIRSCOPE_SCREEN=spectrum
shot devices AIRSCOPE_SCREEN=devices AIRSCOPE_EXPAND=1
shot diagnostics AIRSCOPE_SCREEN=diag AIRSCOPE_SNOOP="$tmp/demo.snoop"
shot diagnostics-device AIRSCOPE_SCREEN=diag AIRSCOPE_INFO=1 AIRSCOPE_SNOOP="$tmp/demo.snoop"
shot vendor-list AIRSCOPE_VENDOR=1
