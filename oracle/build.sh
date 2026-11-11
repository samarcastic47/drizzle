#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")"

echo "Building drizzle-oracle for wasm32-wasip2..."
CARGO="${CARGO:-cargo}"
if ! command -v "$CARGO" >/dev/null 2>&1; then
    CARGO="$HOME/.cargo/bin/cargo"
fi
"$CARGO" build --target wasm32-wasip2 --release
cp target/wasm32-wasip2/release/drizzle-oracle.wasm ./drizzle-oracle.wasm
ls -lh drizzle-oracle.wasm
