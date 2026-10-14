#!/usr/bin/env bash
# Build the browser client: Rust -> wasm32 -> wasm-bindgen glue in web/pkg.
#
#   scripts/build-client.sh          # release build
#   scripts/build-client.sh --dev    # faster, unoptimised build
set -euo pipefail
cd "$(dirname "$0")/.."

PROFILE=release
CARGO_FLAGS=(--release)
if [[ "${1:-}" == "--dev" ]]; then
  PROFILE=debug
  CARGO_FLAGS=()
fi

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "Installing the wasm32-unknown-unknown target..."
  rustup target add wasm32-unknown-unknown
fi

# wasm-bindgen-cli must match the wasm-bindgen crate version exactly.
WB_VERSION=$(grep -m1 -A1 'name = "wasm-bindgen"' Cargo.lock | grep version | cut -d'"' -f2)
if ! command -v wasm-bindgen >/dev/null || [[ "$(wasm-bindgen --version | cut -d' ' -f2)" != "$WB_VERSION" ]]; then
  echo "Installing wasm-bindgen-cli $WB_VERSION..."
  cargo install wasm-bindgen-cli --version "$WB_VERSION" --locked
fi

cargo build -p moba-client --target wasm32-unknown-unknown "${CARGO_FLAGS[@]}"
wasm-bindgen --target web --no-typescript --out-dir web/pkg \
  "target/wasm32-unknown-unknown/$PROFILE/moba_client.wasm"

if command -v wasm-opt >/dev/null && [[ "$PROFILE" == release ]]; then
  wasm-opt -O3 web/pkg/moba_client_bg.wasm -o web/pkg/moba_client_bg.wasm
fi

echo "Client built into web/pkg ($(du -h web/pkg/moba_client_bg.wasm | cut -f1))."
echo "Run the server:  cargo run --release -p moba-server   then open http://localhost:8080"
