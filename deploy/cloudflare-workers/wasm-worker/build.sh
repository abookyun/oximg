#!/usr/bin/env bash
# wrangler's [build] command: oximg as a wasip1 reactor, then wasm-opt.
set -euo pipefail
cd "$(dirname "$0")"
. ../env.sh
cargo build --release --target wasm32-wasip1
mkdir -p build
wasm-opt -O3 --strip-debug --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int \
  --enable-sign-ext --enable-mutable-globals \
  target/wasm32-wasip1/release/oximg_wasm.wasm -o build/oximg.wasm
ls -l build/oximg.wasm
