#!/bin/sh
# Regenerates the README screenshots in docs/screenshots/ from the built-in fake demo data.
# Runs the app inside a headless Wayland compositor (cage), so nothing shows on your screen and nothing
# real is captured. Needs: cage, ImageMagick (magick). Build first: cargo build --release.
set -eu
cd "$(dirname "$0")/.."
out=docs/screenshots; mkdir -p "$out"
shot() { # name, env assignments...
  name=$1; shift
  WLR_BACKENDS=headless WLR_LIBINPUT_NO_DEVICES=1 timeout 90 cage -- env AIRSCOPE_DEMO=1 AIRSCOPE_SHOT="$out/$name.ppm" "$@" target/release/airscope >/dev/null 2>&1
  magick "$out/$name.ppm" "$out/$name.png" && rm "$out/$name.ppm"
  echo "$out/$name.png"
}
shot radar
shot radar-info AIRSCOPE_SEL=AC:80:0A:3D:62:C0
shot spectrum AIRSCOPE_SCREEN=spectrum
shot devices AIRSCOPE_SCREEN=devices
shot vendor-list AIRSCOPE_VENDOR=1
