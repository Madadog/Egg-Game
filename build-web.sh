#!/usr/bin/env bash
# Build the Bevy frontend for the browser (wasm32-unknown-unknown).
#
# Output lands in web/ alongside the tracked web/index.html:
#   web/egg_game_bevy.js       — wasm-bindgen JS glue (ES module)
#   web/egg_game_bevy_bg.wasm  — the wasm binary
#   web/assets/                — a copy of assets/, fetched at runtime
#
# Requires: the wasm32 target (`rustup target add wasm32-unknown-unknown`) and a
# `wasm-bindgen` CLI matching the wasm-bindgen crate version (currently 0.2.122).
# `wasm-opt` is used to shrink the binary if present.
#
# Usage:
#   ./build-web.sh            # release build (default)
#   ./build-web.sh dev        # faster, larger debug build
# Then serve and open:
#   (cd web && python3 -m http.server 8080)   # http://localhost:8080
set -euo pipefail
cd "$(dirname "$0")"

PROFILE="${1:-release}"
OUT=web
CRATE=egg_game_bevy
TARGET=wasm32-unknown-unknown

# cargo's release profile lives in target/<triple>/release, dev in .../debug.
case "$PROFILE" in
  release) BUILD_DIR=release ;;
  dev | debug) BUILD_DIR=debug; PROFILE=dev ;;
  *) BUILD_DIR="$PROFILE" ;;
esac

echo "==> cargo build (--profile $PROFILE, target $TARGET)"
cargo build --profile "$PROFILE" --target "$TARGET"

WASM="target/$TARGET/$BUILD_DIR/$CRATE.wasm"

echo "==> wasm-bindgen"
wasm-bindgen --no-typescript --target web --out-dir "$OUT" "$WASM"

if command -v wasm-opt >/dev/null 2>&1; then
  echo "==> wasm-opt -Oz"
  wasm-opt -Oz -o "$OUT/${CRATE}_bg.wasm" "$OUT/${CRATE}_bg.wasm"
else
  echo "==> wasm-opt not found, skipping size optimization"
fi

echo "==> copy assets"
rm -rf "$OUT/assets"
cp -r assets "$OUT/assets"

echo "==> done. Serve with: (cd $OUT && python3 -m http.server 8080)"
